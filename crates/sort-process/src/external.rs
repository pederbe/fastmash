//! Linux-only external sort transport and process lifetime.
use super::{Kind, Message, PROTOCOL, StartError, linux, sort_threads, supervisor, temporary_dir};
use std::{
    io::{self, BufReader},
    os::fd::{AsFd, AsRawFd, OwnedFd},
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
};
/// `temporary_dir()` as the absolute path the supervisor needs: a relative
/// `TMPDIR` is joined to the working directory, and `.`, repeated and
/// trailing separators are removed (`..` is kept; nothing is resolved).
fn absolute_temporary_dir() -> Result<PathBuf, StartError> {
    let dir = temporary_dir().map_err(|error| StartError::Temporary {
        base: std::env::temp_dir(),
        error,
    })?;
    let dir = if dir.is_relative() {
        std::env::current_dir()?.join(dir)
    } else {
        dir
    };
    Ok(dir.components().collect())
}

/// The system `sort` that the external route runs.
pub(crate) const SYSTEM_SORT: &str = "/usr/bin/sort";

/// `fastmash-sort-supervisor` beside the running executable.
fn supervisor_path() -> io::Result<PathBuf> {
    Ok(std::env::current_exe()?.with_file_name("fastmash-sort-supervisor"))
}

/// Whether the external route can work for a job with this field separator
/// (`None`: whitespace): every supported `sort` accepts the separator,
/// `admit` succeeds, the supervisor is an executable file beside the running
/// executable, `SYSTEM_SORT` is runnable, and the temporary directory
/// `Session::start` would use is a writable, searchable directory. Beyond the
/// separator, a few system calls.
pub fn available(separator: Option<u8>) -> bool {
    passable_separator(separator)
        && admit().is_ok()
        && supervisor_path().is_ok_and(|path| executable_file(&path))
        && runnable_sort(Path::new(SYSTEM_SORT))
        && absolute_temporary_dir()
            .is_ok_and(|dir| dir.is_dir() && linux::access(&dir, linux::W_OK | linux::X_OK))
}

/// Whether every supported `sort` accepts this separator: uutils `sort`
/// rejects one that is not valid UTF-8, so no byte from 0x80 is passed.
fn passable_separator(separator: Option<u8>) -> bool {
    separator.is_none_or(|byte| byte < 0x80)
}

/// A regular file this process may execute, following symbolic links.
fn executable_file(path: &Path) -> bool {
    path.is_file() && linux::access(path, linux::X_OK)
}

/// Whether `path` is a `sort` the external route can run: an executable file
/// whose real name, after symbolic links, is not `busybox` or `toybox`, which
/// reject GNU options the route passes, such as `--parallel`.
fn runnable_sort(path: &Path) -> bool {
    executable_file(path)
        && std::fs::canonicalize(path).is_ok_and(|real| {
            !matches!(
                real.file_name().and_then(|name| name.to_str()),
                Some("busybox" | "toybox")
            )
        })
}

pub struct Session {
    child: Option<Child>,
    control: Option<OwnedFd>,
    reader: Option<BufReader<ChildStdout>>,
}

/// Whether this process can supervise the system `sort` safely: `HUP`, `INT`,
/// `TERM` and `PIPE` unblocked, `HUP`, `INT` and `TERM` at their default
/// dispositions, waitable children, pidfds, and the `close_range` call the
/// supervisor makes (Linux 5.11).
pub fn admit() -> io::Result<()> {
    let mask = linux::mask(None)?;
    if mask & (linux::CANCEL | (1 << 12)) != 0 {
        return Err(io::Error::other(
            "blocked cancellation signals are unsupported for sorting",
        ));
    }
    linux::waitable_children()?;
    linux::cancellation_dispositions()?;
    drop(linux::pidfd(std::process::id())?);
    linux::close_range_cloexec()?;
    Ok(())
}

impl Session {
    /// Start the companion and GNU `sort` on standard input, sorting by
    /// `keys` (1-based fields) stably, as `sort -s -k K,K ...` would. Its
    /// temporary files go in a private directory under `temporary_dir()`.
    pub fn start(
        keys: &[u64],
        separator: Option<u8>,
        zero_terminated: bool,
        ignore_case: bool,
    ) -> Result<Self, StartError> {
        admit()?;
        let base = absolute_temporary_dir()?;
        if keys.is_empty() || separator == Some(0) {
            return Err(io::Error::other("invalid sort configuration").into());
        }
        let (control, child_control) = linux::socket_pair()?;
        let parent = linux::pidfd(std::process::id())?;
        if [
            control.as_raw_fd(),
            child_control.as_raw_fd(),
            parent.as_raw_fd(),
        ]
        .iter()
        .any(|fd| *fd <= 2)
        {
            return Err(io::Error::other("sorting requires open standard streams").into());
        }
        let mut command = Command::new(supervisor_path()?);
        command
            .arg(format!("--supervise-{PROTOCOL}"))
            .arg(child_control.as_raw_fd().to_string())
            .arg(parent.as_raw_fd().to_string())
            .arg(&base);
        let old = linux::mask(None)?;
        supervisor::Config {
            mask: old,
            separator,
            keys: keys.to_vec(),
            zero_terminated,
            ignore_case,
            threads: sort_threads(),
        }
        .args(&mut command);
        command
            .env_clear()
            .env("LC_ALL", "C")
            .env("LANG", "C")
            .env("LANGUAGE", "C")
            .stdin(Stdio::inherit())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        linux::cloexec(child_control.as_fd(), false)?;
        linux::cloexec(parent.as_fd(), false)?;
        linux::mask(Some(old | linux::CANCEL))?;
        let spawned = command.spawn();
        // Restoration failure is terminal; continuing could invalidate arithmetic.
        linux::restore_mask(old);
        drop(child_control);
        drop(parent);
        let mut child = spawned.map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                StartError::MissingCompanion(error)
            } else {
                StartError::Other(error)
            }
        })?;
        let reader = child.stdout.take().map(BufReader::new);
        let mut session = Self {
            child: Some(child),
            control: Some(control),
            reader,
        };
        let reply = match session.receive() {
            Ok(reply) => reply,
            // A companion that exits without replying may have failed for any
            // reason; one from another release says so with `Kind::Mismatch`.
            Err(error) => {
                session.reader.take();
                session.control.take();
                let _ = session.reap();
                return Err(StartError::Other(error));
            }
        };
        match reply.kind {
            Kind::Mismatch => {
                session.reader.take();
                session.control.take();
                let _ = session.reap();
                Err(StartError::Mismatch)
            }
            Kind::Ready if reply.status == 0 && reply.cleanup == 0 => Ok(session),
            Kind::Failed | Kind::TemporaryFailed => {
                session.reader.take();
                session.control.take();
                session.reap()?;
                if reply.kind == Kind::TemporaryFailed {
                    return Err(StartError::Temporary {
                        base,
                        error: io::Error::from_raw_os_error(reply.status),
                    });
                }
                Err(io::Error::other("sort supervisor setup failed").into())
            }
            _ => Err(io::Error::other("unexpected sort startup message").into()),
        }
    }
    pub fn reader(&mut self) -> &mut BufReader<ChildStdout> {
        self.reader.as_mut().expect("live sort reader")
    }
    fn receive(&mut self) -> io::Result<Message> {
        let fd = self
            .control
            .as_ref()
            .ok_or_else(|| io::Error::other("closed sort session"))?
            .as_fd();
        loop {
            if let Some(message) = Message::receive(fd)? {
                return Ok(message);
            }
            if self.child.as_mut().unwrap().try_wait()?.is_some() {
                return Message::receive(fd)?
                    .ok_or_else(|| io::Error::other("sort supervisor exited without status"));
            }
            linux::poll(&mut [linux::Poll::new(fd, 1)], 50)?;
        }
    }
    fn request(&mut self, kind: Kind) -> io::Result<()> {
        let fd = self.control.as_ref().unwrap().as_fd();
        loop {
            match Message::new(kind).send(fd) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e),
            }
            if self.child.as_mut().unwrap().try_wait()?.is_some() {
                return Err(io::Error::other("sort supervisor exited"));
            }
            linux::poll(&mut [linux::Poll::new(fd, 4)], 50)?;
        }
    }
    fn reap(&mut self) -> io::Result<()> {
        if let Some(mut child) = self.child.take() {
            let status = child.wait()?;
            if !status.success() {
                return Err(io::Error::other("sort supervisor failed"));
            }
        }
        Ok(())
    }
    pub fn finish(&mut self, abort: bool) -> io::Result<Message> {
        self.reader.take();
        let result = self
            .request(if abort { Kind::Abort } else { Kind::Finish })
            .and_then(|_| self.receive());
        // EOF also asks the supervisor to abort if the protocol failed.
        self.control.take();
        let reaped = self.reap();
        let message = result?;
        reaped?;
        if message.kind != Kind::Done {
            return Err(io::Error::other("invalid sort completion"));
        }
        Ok(message)
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.reader.take();
            // Closing the private peer triggers cleanup even after a failed send.
            self.control.take();
            let _ = self.reap();
        }
    }
}

#[cfg(test)]
mod framing_tests {
    use super::*;

    fn record(kind: u32) -> [u8; 16] {
        let mut data = [0u8; 16];
        data[..4].copy_from_slice(b"FMS1");
        data[4..8].copy_from_slice(&kind.to_le_bytes());
        data
    }

    #[test]
    fn messages_round_trip_and_an_empty_channel_is_not_an_error() {
        let (a, b) = linux::socket_pair().unwrap();
        assert!(Message::receive(b.as_fd()).unwrap().is_none());
        Message {
            kind: Kind::Done,
            status: -3,
            cleanup: 7,
        }
        .send(a.as_fd())
        .unwrap();
        let got = Message::receive(b.as_fd()).unwrap().unwrap();
        assert_eq!((got.kind, got.status, got.cleanup), (Kind::Done, -3, 7));
    }

    #[test]
    fn malformed_or_closed_channels_are_errors() {
        let valid = record(1);
        let mut long = [0u8; 17];
        long[..16].copy_from_slice(&valid);
        let mut magic = valid;
        magic[0] = b'X';
        for bad in [
            &valid[..15],
            &long[..],
            &magic[..],
            &record(0)[..],
            &record(8)[..],
        ] {
            let (a, b) = linux::socket_pair().unwrap();
            linux::send(a.as_fd(), bad).unwrap();
            assert!(Message::receive(b.as_fd()).is_err(), "{bad:?}");
        }
        let (a, b) = linux::socket_pair().unwrap();
        drop(a);
        assert!(Message::receive(b.as_fd()).is_err());
    }
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn only_an_executable_sort_other_than_busybox_or_toybox_is_runnable() {
        let dir = std::env::temp_dir().join(format!("fastmash-runnable-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir(&dir).unwrap();
        let file = |name: &str, mode: u32| {
            let path = dir.join(name);
            std::fs::write(&path, b"#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            path
        };
        let sort = file("sort", 0o755);
        assert!(runnable_sort(&sort));
        symlink(&sort, dir.join("linked")).unwrap();
        assert!(runnable_sort(&dir.join("linked")));
        assert!(!runnable_sort(&dir.join("missing")));
        symlink(dir.join("missing"), dir.join("dangling")).unwrap();
        assert!(!runnable_sort(&dir.join("dangling")));
        assert!(!runnable_sort(&file("plain", 0o644)));
        assert!(!runnable_sort(&dir));
        // Executable, but reached as BusyBox or Toybox.
        for multicall in ["busybox", "toybox"] {
            let target = file(multicall, 0o755);
            assert!(!runnable_sort(&target), "{multicall}");
            let link = dir.join(format!("{multicall}-sort"));
            symlink(&target, &link).unwrap();
            assert!(!runnable_sort(&link), "{multicall}");
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_close_range_probe_marks_no_descriptor() {
        let (inherited, _peer) = linux::socket_pair().unwrap();
        linux::cloexec(inherited.as_fd(), false).unwrap();
        linux::close_range_cloexec().unwrap();
        // SAFETY: F_GETFD takes no pointers and reads a descriptor this test owns.
        let flags = unsafe { linux::syscall(72, [inherited.as_raw_fd() as usize, 1, 0, 0, 0, 0]) };
        assert_eq!(flags, 0);
    }

    #[test]
    fn only_ascii_separators_are_passed_to_the_system_sort() {
        assert!(passable_separator(None));
        for byte in [1, b'\t', b',', 0x7f] {
            assert!(passable_separator(Some(byte)), "{byte:#x}");
        }
        for byte in [0x80, 0xa7, 0xff] {
            assert!(!passable_separator(Some(byte)), "{byte:#x}");
        }
    }
}
