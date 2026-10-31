//! Private native Spill storage, thread policy and SIGPIPE support.
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]
#[cfg(target_os = "linux")]
#[doc(hidden)]
pub mod linux;
use std::{
    fs::{File, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
/// The directory for temporary native sort data: `TMPDIR` as given, or `/tmp` when it is unset. An empty
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
    getrandom::fill(bytes).map_err(io::Error::from)
}

/// Threads for native sorting, as GNU sort chooses its default: the
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
