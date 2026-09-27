//! Preserve invalid inherited streams without disabling Rust's descriptor safety.
use super::linux;
use std::{
    io::{self, Read, Write},
    sync::atomic::{AtomicBool, AtomicU8, Ordering},
};

static CLOSED: AtomicU8 = AtomicU8::new(0);
static STDERR_FAILED: AtomicBool = AtomicBool::new(false);

// ELF executable preinitializers run before Rust's standard-fd sanitization.
// Only raw syscalls and an atomic store are safe here; no runtime is initialized.
#[used]
#[unsafe(link_section = ".preinit_array")]
static CAPTURE: unsafe extern "C" fn() = capture;

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

pub(super) fn originally_closed(fd: usize) -> bool {
    CLOSED.load(Ordering::Relaxed) & (1 << fd) != 0
}

/// Child runtimes may turn invalid input into EOF. Check access before delegation.
pub(super) fn check_input_access() -> io::Result<()> {
    // SAFETY: fcntl F_GETFL takes no pointers.
    let flags = result(unsafe { linux::syscall(72, 0, 3, 0, 0) })?;
    if flags & 3 == 1 {
        Err(io::Error::from_raw_os_error(9))
    } else {
        Ok(())
    }
}

pub(super) fn finish_stderr() -> bool {
    let failed = STDERR_FAILED.load(Ordering::Relaxed);
    // SAFETY: close takes no pointers; standard error has no Rust owner and
    // this is its last use.
    let closed = unsafe { linux::syscall(3, 2, 0, 0, 0) };
    !failed && (closed >= 0 || closed == -9)
}

fn result(value: isize) -> io::Result<usize> {
    if value < 0 {
        Err(io::Error::from_raw_os_error(-value as i32))
    } else {
        Ok(value as usize)
    }
}

pub(super) struct Stdin;
impl Read for Stdin {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        loop {
            // SAFETY: read writes at most `bytes.len()` bytes into `bytes`.
            let value =
                unsafe { linux::syscall(0, 0, bytes.as_mut_ptr() as usize, bytes.len(), 0) };
            if value != -4 {
                return result(value);
            }
        }
    }
}

pub(super) fn write(fd: usize, bytes: &[u8]) -> io::Result<usize> {
    loop {
        // SAFETY: write reads at most `bytes.len()` bytes from `bytes`.
        let value = unsafe { linux::syscall(1, fd, bytes.as_ptr() as usize, bytes.len(), 0) };
        if value != -4 {
            return result(value);
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
