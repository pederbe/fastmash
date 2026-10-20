#![forbid(unsafe_code)]
//! Real native standard-stream faults with owned spawn file actions.
// Reuse the enclosing integration target's established fixture helpers.
use crate::{executable, temp_dir};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::close;
use std::{
    error::Error,
    ffi::{CString, OsStr},
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::ffi::OsStrExt,
        unix::fs::OpenOptionsExt,
    },
    path::Path,
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const CHILD_LIMIT: Duration = Duration::from_secs(5);
const CLEANUP_RESERVE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub enum Streams {
    Ordinary,
    Closed(u8),
    WrongAccess(u8),
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct Observed {
    pub parent_binding_fds: [i32; 3],
    pub parent_binding_flags: [i32; 3],
    pub parent_close_results: [nix::Result<()>; 3],
    pub status: WaitStatus,
    pub timed_out: bool,
    pub cancellation: Option<nix::Result<()>>,
    pub stdout_length: u64,
    pub stderr_length: u64,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[allow(dead_code)]
pub fn invoke(
    arguments: &[&str],
    input: &[u8],
    streams: Streams,
    directory: &Path,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Observed> {
    fs::write(directory.join("stdin"), input)?;
    for name in ["stdout", "stderr"] {
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(directory.join(name))?,
        );
    }
    let wrong = match streams {
        Streams::WrongAccess(mask) => mask,
        _ => 0,
    };
    let files = [
        if wrong & 1 != 0 {
            File::options().write(true).open("/dev/null")?
        } else {
            File::open(directory.join("stdin"))?
        },
        if wrong & 2 != 0 {
            File::open("/dev/null")?
        } else {
            File::options().write(true).open(directory.join("stdout"))?
        },
        if wrong & 4 != 0 {
            File::open("/dev/null")?
        } else {
            File::options().write(true).open(directory.join("stderr"))?
        },
    ];
    let parent_binding_flags = [
        fcntl(&files[0], FcntlArg::F_GETFL)?,
        fcntl(&files[1], FcntlArg::F_GETFL)?,
        fcntl(&files[2], FcntlArg::F_GETFL)?,
    ];
    let access = parent_binding_flags.map(|flags| flags & OFlag::O_ACCMODE.bits());
    let expected_access = [
        if wrong & 1 != 0 {
            OFlag::O_WRONLY
        } else {
            OFlag::O_RDONLY
        },
        if wrong & 2 != 0 {
            OFlag::O_RDONLY
        } else {
            OFlag::O_WRONLY
        },
        if wrong & 4 != 0 {
            OFlag::O_RDONLY
        } else {
            OFlag::O_WRONLY
        },
    ]
    .map(|flags| flags.bits());
    if access != expected_access {
        return Err(format!("incorrect parent descriptor access: {parent_binding_flags:?}").into());
    }
    invoke_bound(
        BoundCommand {
            binary: &executable::fastmash(),
            arguments,
            files: files.map(OwnedFd::from),
            closed: match streams {
                Streams::Closed(mask) => mask,
                _ => 0,
            },
            environment: &[("LC_ALL", "C"), ("PATH", "/usr/bin:/bin")],
        },
        directory,
        stdout_limit,
        stderr_limit,
    )
}

/// Caller-owned real stream bindings. Capture files must exist even when a
/// selected stream goes elsewhere, so its unselected output remains observable.
pub struct BoundCommand<'a> {
    pub binary: &'a OsStr,
    pub arguments: &'a [&'a str],
    pub files: [OwnedFd; 3],
    pub closed: u8,
    pub environment: &'a [(&'a str, &'a str)],
}

pub fn invoke_bound(
    command: BoundCommand<'_>,
    directory: &Path,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Observed> {
    let BoundCommand {
        binary,
        arguments,
        files,
        closed,
        environment,
    } = command;
    let executable = fs::canonicalize(binary)?;
    let executable = CString::new(executable.as_os_str().as_bytes())?;
    let parent_binding_fds = files.each_ref().map(|file| file.as_raw_fd());
    let parent_binding_flags = [
        fcntl(&files[0], FcntlArg::F_GETFL)?,
        fcntl(&files[1], FcntlArg::F_GETFL)?,
        fcntl(&files[2], FcntlArg::F_GETFL)?,
    ];
    let mut actions = PosixSpawnFileActions::init()?;
    for (target, source) in [0, 1, 2].into_iter().zip(parent_binding_fds) {
        if source < 3 {
            return Err("parent binding unexpectedly occupies a standard descriptor".into());
        }
        actions.add_dup2(source, target)?;
    }
    for target in 0..3 {
        if closed & (1 << target) != 0 {
            actions.add_close(target)?;
        }
    }
    let arguments = std::iter::once("fastmash")
        .chain(arguments.iter().copied())
        .map(CString::new)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let attributes = PosixSpawnAttr::init()?;
    let environment = environment
        .iter()
        .map(|(key, value)| CString::new(format!("{key}={value}")))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let deadline = Instant::now() + CHILD_LIMIT;
    let cancel_at = deadline - CLEANUP_RESERVE;
    let spawned = posix_spawn(
        executable.as_c_str(),
        &actions,
        &attributes,
        &arguments,
        &environment,
    );
    drop(actions);
    drop(attributes);
    // Keep every source descriptor owned through spawn, then consume it once.
    // A close error is recorded without retrying an uncertain descriptor number.
    let parent_close_results = files.map(close);
    let pid = spawned?;
    let mut timed_out = false;
    let mut cancellation = None;
    let status = loop {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status @ (WaitStatus::Exited(..) | WaitStatus::Signaled(..))) => break status,
            Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => {}
            result => {
                // An ownership error does not authorize signalling the old PID.
                return Err(
                    format!("unexpected wait result: {result:?}; cleanup unconfirmed").into(),
                );
            }
        }
        if cancellation.is_none() && Instant::now() >= cancel_at {
            timed_out = true;
            cancellation = Some(kill(pid, Signal::SIGKILL));
        }
        if Instant::now() >= deadline {
            return Err(
                format!("child was not reaped within five seconds: {cancellation:?}").into(),
            );
        }
        thread::sleep(Duration::from_millis(2));
    };
    timed_out |= Instant::now() >= deadline;
    Ok(Observed {
        parent_binding_fds,
        parent_binding_flags,
        parent_close_results,
        status,
        timed_out,
        cancellation,
        stdout_length: fs::metadata(directory.join("stdout"))?.len(),
        stderr_length: fs::metadata(directory.join("stderr"))?.len(),
        stdout: read_prefix(&directory.join("stdout"), stdout_limit)?,
        stderr: read_prefix(&directory.join("stderr"), stderr_limit)?,
    })
}

fn read_prefix(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

impl Observed {
    pub fn into_output(self) -> std::process::Output {
        use std::os::unix::process::ExitStatusExt;
        assert!(!self.timed_out, "{self:?}");
        assert!(self.cancellation.is_none(), "{self:?}");
        assert!(
            self.parent_close_results
                .iter()
                .all(|result| result.is_ok()),
            "{self:?}"
        );
        assert_eq!(self.stdout_length, self.stdout.len() as u64, "{self:?}");
        assert_eq!(self.stderr_length, self.stderr.len() as u64, "{self:?}");
        let raw = match self.status {
            WaitStatus::Exited(_, code) => code << 8,
            WaitStatus::Signaled(_, signal, core) => signal as i32 | if core { 128 } else { 0 },
            status => panic!("unexpected terminal child status: {status:?}"),
        };
        std::process::Output {
            status: std::process::ExitStatus::from_raw(raw),
            stdout: self.stdout,
            stderr: self.stderr,
        }
    }
}

/// Run a command with genuinely closed inherited stdin, before Rust startup.
/// Keep observations bounded and reject incomplete captures or cleanup.
#[allow(dead_code)]
pub fn closed_stdin(arguments: &[&str]) -> std::process::Output {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let directory = temp_dir::TempDir::new(&format!("closed-stdin-{id}"));
    let path = directory.0.clone();
    let observed = invoke(arguments, b"", Streams::Closed(1), &path, 1 << 20, 1 << 20);
    drop(directory);
    assert!(!path.try_exists().unwrap(), "temporary cleanup: {path:?}");
    observed
        .unwrap_or_else(|error| panic!("closed stdin: {error}"))
        .into_output()
}
