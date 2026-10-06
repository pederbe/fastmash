//! Private spill storage and transport for the Linux external-sort companion.
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]
#[cfg(not(any(
    all(
        target_os = "linux",
        target_arch = "x86_64",
        target_pointer_width = "64"
    ),
    all(
        target_os = "macos",
        target_arch = "aarch64",
        target_pointer_width = "64"
    )
)))]
compile_error!("fastmash-sort-process supports Linux x86-64 and Apple Silicon macOS");
#[cfg(target_os = "linux")]
mod external;
/// Linux process primitives shared with the companion; not a stable API.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub mod linux;
/// The Linux companion executable's roles; not a stable API.
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub mod supervisor;
#[cfg(target_os = "macos")]
mod unavailable;
#[cfg(target_os = "linux")]
pub(crate) use external::SYSTEM_SORT;
#[cfg(target_os = "linux")]
pub use external::{Session, admit, available};
#[cfg(target_os = "linux")]
use std::os::fd::BorrowedFd;
use std::{
    fmt,
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
#[cfg(target_os = "macos")]
pub use unavailable::{Session, admit, available};

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
#[cfg(target_os = "linux")]
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
        random_bytes(&mut random)?;
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

/// Native macOS spill storage uses an exclusive private file, unlinked before
/// any caller receives it. The descriptor remains usable until closed.
#[cfg(target_os = "macos")]
pub fn anonymous_file() -> io::Result<File> {
    named_file(&temporary_dir()?)
}
#[cfg(target_os = "linux")]
fn random_bytes(bytes: &mut [u8]) -> io::Result<()> {
    linux::random(bytes)
}
#[cfg(target_os = "macos")]
fn random_bytes(bytes: &mut [u8]) -> io::Result<()> {
    for chunk in bytes.chunks_mut(256) {
        // SAFETY: getentropy writes exactly the chunk's live bytes on success.
        // Each request is within its 256-byte limit.
        if unsafe { libc::getentropy(chunk.as_mut_ptr().cast(), chunk.len()) } != 0 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Threads for the external sort, as GNU sort chooses its default: the
/// processors available to this process (affinity and CPU quota), replaced by
/// a positive `OMP_NUM_THREADS` (its first value) and capped by
/// `OMP_THREAD_LIMIT`, then at most 8.
pub fn sort_threads() -> u32 {
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

/// The version of the command line and control messages between `fastmash`
/// and `fastmash-sort-supervisor`, sent as the role `--supervise-PROTOCOL`.
/// Every version keeps two things, so either side can name a mismatch: the
/// control descriptor is the argument after the role, and a companion that
/// does not speak the requested version replies `Kind::Mismatch` there.
pub const PROTOCOL: u32 = 1;

/// Why the external sort could not start.
#[derive(Debug)]
pub enum StartError {
    /// `fastmash-sort-supervisor` is not beside the running executable.
    MissingCompanion(io::Error),
    /// The private directory under `base` could not be created.
    Temporary { base: PathBuf, error: io::Error },
    /// The companion is from another release: it replied `Kind::Mismatch`.
    Mismatch,
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
            Self::Mismatch => f.write_str("fastmash-sort-supervisor is from another release"),
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
    /// The companion does not speak the requested protocol version.
    Mismatch = 7,
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
    #[cfg(target_os = "linux")]
    pub fn send(self, fd: BorrowedFd<'_>) -> io::Result<()> {
        let mut data = [0u8; 16];
        data[..4].copy_from_slice(b"FMS1");
        data[4..8].copy_from_slice(&(self.kind as u32).to_le_bytes());
        data[8..12].copy_from_slice(&self.status.to_le_bytes());
        data[12..].copy_from_slice(&self.cleanup.to_le_bytes());
        linux::send(fd, &data)
    }
    #[cfg(target_os = "linux")]
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
            7 => Kind::Mismatch,
            _ => return Err(io::Error::other("unknown sort control message")),
        };
        Ok(Some(Self {
            kind,
            status: i32::from_le_bytes(data[8..12].try_into().unwrap()),
            cleanup: i32::from_le_bytes(data[12..16].try_into().unwrap()),
        }))
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;
    #[test]
    fn named_fallback_file_is_private_usable_and_already_unlinked() {
        use std::io::{Read, Seek, Write};
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("fastmash-named-test-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut file = named_file(&dir).unwrap();
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(file.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(file.metadata().unwrap().nlink(), 0);
        file.write_all(b"run").unwrap();
        file.rewind().unwrap();
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "run");
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn named_storage_failures_preserve_unrelated_entries() {
        let dir = std::env::temp_dir().join(format!(
            "fastmash-named-failure-test-{}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).unwrap();
        let missing = dir.join("missing");
        assert_eq!(
            named_file(&missing).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!missing.exists());
        let ordinary = dir.join("ordinary");
        std::fs::write(&ordinary, b"sentinel").unwrap();
        assert_eq!(
            named_file(&ordinary).unwrap_err().kind(),
            io::ErrorKind::NotADirectory
        );
        assert_eq!(std::fs::read(&ordinary).unwrap(), b"sentinel");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_file(ordinary).unwrap();
        std::fs::remove_dir(dir).unwrap();
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
}
