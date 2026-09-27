//! Transport for the stable external-sort companion, with no numeric code.
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]
#[cfg(not(all(
    target_os = "linux",
    target_arch = "x86_64",
    target_pointer_width = "64"
)))]
compile_error!("fastmash-sort-process supports only Linux x86-64");
/// Process primitives shared with the companion; not a stable API.
#[doc(hidden)]
pub mod linux;
/// The companion executable's roles; not a stable API.
#[doc(hidden)]
pub mod supervisor;
use std::{
    fmt,
    fs::{File, OpenOptions},
    io,
    io::BufReader,
    os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdout, Command, Stdio},
};

/// The directory for temporary sort data, shared by the native spill and the
/// external route: `TMPDIR` as given, or `/tmp` when it is unset. An empty
/// `TMPDIR` names no directory: the error opening it would give.
pub fn temporary_dir() -> io::Result<PathBuf> {
    let dir = std::env::temp_dir();
    if dir.as_os_str().is_empty() {
        return Err(io::Error::from_raw_os_error(2));
    }
    Ok(dir)
}

/// A new read-write file in `temporary_dir()` that no name refers to, so it is
/// removed when closed: `O_TMPFILE` where the filesystem supports it, otherwise
/// (overlayfs before Linux 6.10, as in containers, NFS, 9p) a private file
/// with a random name, unlinked as soon as it is open.
pub fn anonymous_file() -> io::Result<File> {
    const O_TMPFILE: i32 = 0o20_200_000;
    const EISDIR: i32 = 21;
    const EOPNOTSUPP: i32 = 95;
    let dir = temporary_dir()?;
    match OpenOptions::new()
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(O_TMPFILE)
        .open(&dir)
    {
        // EISDIR: a kernel without O_TMPFILE saw only its O_DIRECTORY part.
        Err(e) if matches!(e.raw_os_error(), Some(EOPNOTSUPP | EISDIR)) => named_file(&dir),
        result => result,
    }
}
fn named_file(dir: &Path) -> io::Result<File> {
    for _ in 0..8 {
        let mut random = [0u8; 16];
        linux::random(&mut random)?;
        let name = random
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let path = dir.join(format!("fastmash-spill-{name}"));
        // create_new (O_EXCL) never follows or reuses an existing entry.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
        {
            Ok(file) => {
                std::fs::remove_file(&path)?;
                return Ok(file);
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    // Eight random names all taken.
    Err(io::Error::from_raw_os_error(17))
}

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

/// Threads for the external sort, as GNU sort chooses its default: the
/// processors available to this process (affinity and CPU quota), replaced by
/// a positive `OMP_NUM_THREADS` (its first value) and capped by
/// `OMP_THREAD_LIMIT`, then at most 8.
fn sort_threads() -> u32 {
    let available = std::thread::available_parallelism().map_or(1, |n| n.get());
    threads_from(
        std::env::var("OMP_NUM_THREADS").ok().as_deref(),
        std::env::var("OMP_THREAD_LIMIT").ok().as_deref(),
        u32::try_from(available).unwrap_or(u32::MAX),
    )
}
fn threads_from(num: Option<&str>, limit: Option<&str>, available: u32) -> u32 {
    let positive = |value: Option<&str>| {
        value
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse::<u32>().ok())
            .filter(|n| *n > 0)
    };
    let mut threads = positive(num).unwrap_or(available);
    if let Some(limit) = positive(limit) {
        threads = threads.min(limit);
    }
    threads.clamp(1, 8)
}

/// Why the external sort could not start.
#[derive(Debug)]
pub enum StartError {
    /// `fastmash-sort-supervisor` is not beside the running executable.
    MissingCompanion(io::Error),
    /// The private directory under `base` could not be created.
    Temporary { base: PathBuf, error: io::Error },
    /// Anything else: runtime requirements, protocol or process failures.
    Other(io::Error),
}
impl From<io::Error> for StartError {
    fn from(error: io::Error) -> Self {
        Self::Other(error)
    }
}
impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingCompanion(error) | Self::Other(error) => error.fmt(f),
            Self::Temporary { base, error } => {
                write!(
                    f,
                    "cannot create a directory in '{}': {error}",
                    base.display()
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ready = 1,
    Finish = 2,
    Abort = 3,
    Done = 4,
    Failed = 5,
    /// Setup failed creating the temporary directory; status is its errno.
    TemporaryFailed = 6,
}
#[derive(Clone, Copy, Debug)]
pub struct Message {
    pub kind: Kind,
    pub status: i32,
    pub cleanup: i32,
}
impl Message {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            status: 0,
            cleanup: 0,
        }
    }
    pub fn send(self, fd: BorrowedFd<'_>) -> io::Result<()> {
        let mut data = [0u8; 16];
        data[..4].copy_from_slice(b"FMS1");
        data[4..8].copy_from_slice(&(self.kind as u32).to_le_bytes());
        data[8..12].copy_from_slice(&self.status.to_le_bytes());
        data[12..].copy_from_slice(&self.cleanup.to_le_bytes());
        linux::send(fd, &data)
    }
    pub fn receive(fd: BorrowedFd<'_>) -> io::Result<Option<Self>> {
        // One extra byte makes every oversized/truncated seqpacket invalid.
        let mut data = [0u8; 17];
        let n = match linux::receive(fd, &mut data) {
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(None),
            result => result?,
        };
        if n != 16 || &data[..4] != b"FMS1" {
            return Err(io::Error::other("invalid or closed sort control channel"));
        }
        let kind = match u32::from_le_bytes(data[4..8].try_into().unwrap()) {
            1 => Kind::Ready,
            2 => Kind::Finish,
            3 => Kind::Abort,
            4 => Kind::Done,
            5 => Kind::Failed,
            6 => Kind::TemporaryFailed,
            _ => return Err(io::Error::other("unknown sort control message")),
        };
        Ok(Some(Self {
            kind,
            status: i32::from_le_bytes(data[8..12].try_into().unwrap()),
            cleanup: i32::from_le_bytes(data[12..16].try_into().unwrap()),
        }))
    }
}

pub struct Session {
    child: Option<Child>,
    control: Option<OwnedFd>,
    reader: Option<BufReader<ChildStdout>>,
}

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
        let companion = std::env::current_exe()?.with_file_name("fastmash-sort-supervisor");
        let mut command = Command::new(companion);
        command
            .arg("--supervise")
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
        let reply = session.receive()?;
        match reply.kind {
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
    fn named_fallback_file_is_private_usable_and_already_unlinked() {
        use std::io::{Read, Seek, Write};
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("fastmash-named-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut file = named_file(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        file.write_all(b"run").unwrap();
        file.rewind().unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "run");
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn sort_threads_follow_gnu_sort_defaults() {
        assert_eq!(threads_from(None, None, 16), 8);
        assert_eq!(threads_from(None, None, 4), 4);
        assert_eq!(threads_from(None, None, 0), 1);
        assert_eq!(threads_from(Some("2"), None, 16), 2);
        assert_eq!(threads_from(Some("3,2"), None, 16), 3);
        assert_eq!(threads_from(Some(" 6"), None, 2), 6);
        assert_eq!(threads_from(Some("20"), None, 2), 8);
        assert_eq!(threads_from(Some("0"), None, 4), 4);
        assert_eq!(threads_from(Some("x"), None, 4), 4);
        assert_eq!(threads_from(None, Some("3"), 16), 3);
        assert_eq!(threads_from(Some("6"), Some("2"), 16), 2);
        assert_eq!(threads_from(None, Some("0"), 16), 8);
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
            &record(7)[..],
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
