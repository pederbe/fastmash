//! Preserve invalid inherited streams without disabling Rust's descriptor safety.
#[cfg(target_os = "linux")]
use super::linux;
use super::platform;
use std::{
    fs,
    io::{self, Read, Seek, Write},
    os::{fd::AsFd, unix::fs::MetadataExt},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

static CLOSED: AtomicU8 = AtomicU8::new(0);
static STDERR_FAILED: AtomicBool = AtomicBool::new(false);

// ELF executable preinitializers run before Rust's standard-fd sanitization.
// Only raw syscalls and an atomic store are safe here; no runtime is initialized.
#[used]
#[cfg(target_os = "linux")]
#[unsafe(link_section = ".preinit_array")]
static CAPTURE: unsafe extern "C" fn() = capture;

#[cfg(target_os = "linux")]
unsafe extern "C" fn capture() {
    let mut closed = 0;
    for fd in 0..3 {
        // F_GETFD identifies closed slots without reading or writing the stream.
        // SAFETY: fcntl F_GETFD takes no pointers.
        if unsafe { linux::syscall(72, fd, 1, 0, 0) } == -9 {
            closed |= 1 << fd;
            // Occupy the slot so later opens cannot alias a standard stream.
            // Wrong access retains EBADF for children as well as this process.
            // SAFETY: the path is a static NUL-terminated string; the slot is free.
            let opened = unsafe {
                linux::syscall(
                    257,
                    (-100isize) as usize,
                    c"/dev/null".as_ptr() as usize,
                    usize::from(fd == 0),
                    0,
                )
            };
            if opened != fd as isize {
                // No allocation or diagnostic is safe during failed initialization.
                // SAFETY: exit_group takes no pointers and ends the process.
                unsafe { linux::syscall(231, 1, 0, 0, 0) };
            }
        }
    }
    CLOSED.store(closed, Ordering::Relaxed);
}

// dyld runs Mach-O constructors before Rust's runtime initializes and repairs
// closed standard descriptors. libc is available here; allocation and Rust's
// runtime are not. Native process tests verify this ordering.
#[cfg(target_os = "macos")]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func,mod_init_funcs")]
static CAPTURE: unsafe extern "C" fn() = capture;

#[cfg(target_os = "macos")]
unsafe extern "C" fn capture() {
    let mut closed = 0;
    for fd in 0..3 {
        // SAFETY: fcntl F_GETFD takes no pointers and inspects an inherited fd.
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1 {
            // SAFETY: __error returns this thread's live native errno slot.
            if unsafe { *libc::__error() } != libc::EBADF {
                // SAFETY: _exit ends the process without runtime teardown.
                unsafe { libc::_exit(1) };
            }
            closed |= 1 << fd;
            // Hold each closed slot with the wrong access mode, preserving an
            // EBADF result and preventing later opens from aliasing a stream.
            let access = if fd == 0 {
                libc::O_WRONLY
            } else {
                libc::O_RDONLY
            };
            // SAFETY: open reads a constant NUL-terminated path, no mode is
            // needed without O_CREAT, and the expected descriptor is free.
            let opened = unsafe { libc::open(c"/dev/null".as_ptr(), access) };
            if opened != fd {
                // SAFETY: no runtime or diagnostic is safe after failed capture.
                unsafe { libc::_exit(1) };
            }
        }
    }
    CLOSED.store(closed, Ordering::Relaxed);
}

pub(super) fn originally_closed(fd: usize) -> bool {
    CLOSED.load(Ordering::Relaxed) & (1 << fd) != 0
}

/// Child runtimes may turn invalid input into EOF. Check access before delegation.
pub(super) fn check_input_access() -> io::Result<()> {
    let flags = platform::flags(0)?;
    if flags & libc::O_ACCMODE as usize == libc::O_WRONLY as usize {
        Err(io::Error::from_raw_os_error(9))
    } else {
        Ok(())
    }
}

pub(super) fn finish_stderr() -> bool {
    let failed = STDERR_FAILED.load(Ordering::Relaxed);
    // Standard error has no Rust owner and this is its last use.
    let closed = platform::close(2);
    !failed
        && (closed.is_ok() || closed.is_err_and(|error| error.raw_os_error() == Some(libc::EBADF)))
}

pub(super) struct Stdin;
impl Read for Stdin {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            let result = platform::read(0, bytes);
            if !result
                .as_ref()
                .is_err_and(|error| error.raw_os_error() == Some(libc::EINTR))
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
            .is_err_and(|error| error.raw_os_error() == Some(libc::EINTR))
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
