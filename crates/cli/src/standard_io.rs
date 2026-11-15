//! Preserve invalid inherited streams without disabling Rust's descriptor safety.
#![forbid(unsafe_code)]
use super::platform;
#[cfg(unix)]
use std::{
    fs,
    os::{fd::AsFd, unix::fs::MetadataExt},
};
use std::{
    io::{self, Read, Seek, Write},
    sync::atomic::{AtomicBool, Ordering},
};

static STDERR_FAILED: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
pub(super) use super::startup::originally_closed;
#[cfg(windows)]
pub(super) use super::windows_stdio::{originally_closed, regular_input_length};

pub(super) fn finish_stderr() -> bool {
    let failed = STDERR_FAILED.load(Ordering::Relaxed);
    // Standard error has no Rust owner and this is its last use.
    let closed = platform::close(2);
    !failed
        && (closed.is_ok()
            || closed.is_err_and(|error| super::io_errors::bad_descriptor(error.raw_os_error())))
}

pub(super) struct Stdin;
impl Read for Stdin {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            let result = platform::read(0, bytes);
            if !result
                .as_ref()
                .is_err_and(|error| error.kind() == io::ErrorKind::Interrupted)
            {
                return result;
            }
        }
    }
}

impl Seek for Stdin {
    fn seek(&mut self, position: io::SeekFrom) -> io::Result<u64> {
        platform::seek(0, position)
    }
}

/// The length of standard input when it is a regular file with content on
/// storage, which can be read again from an earlier offset. Kernel pseudo-
/// files regenerate their content on each read, and are left out: those
/// under /proc report no length, those under /sys occupy no blocks.
#[cfg(unix)]
pub(super) fn regular_input_length() -> Option<u64> {
    let file = fs::File::from(io::stdin().as_fd().try_clone_to_owned().ok()?);
    let metadata = file.metadata().ok()?;
    (metadata.is_file() && metadata.len() != 0 && metadata.blocks() != 0).then_some(metadata.len())
}

pub(super) fn write(fd: usize, bytes: &[u8]) -> io::Result<usize> {
    loop {
        let result = platform::write(fd, bytes);
        if !result
            .as_ref()
            .is_err_and(|error| error.kind() == io::ErrorKind::Interrupted)
        {
            return result;
        }
    }
}

pub(super) struct Stderr;
impl Write for Stderr {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = write(2, bytes);
        if result.is_err() {
            STDERR_FAILED.store(true, Ordering::Relaxed);
        }
        result
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
