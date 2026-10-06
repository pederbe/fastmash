//! Native process primitives used by the standard streams and signal policy.
use std::io;

#[cfg(target_os = "linux")]
fn checked(value: isize) -> io::Result<usize> {
    if value < 0 {
        Err(io::Error::from_raw_os_error(-value as i32))
    } else {
        Ok(value as usize)
    }
}

#[cfg(target_os = "macos")]
fn checked(value: isize) -> io::Result<usize> {
    if value < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(value as usize)
    }
}

pub(super) fn flags(fd: usize) -> io::Result<usize> {
    #[cfg(target_os = "linux")]
    // SAFETY: fcntl F_GETFL takes no pointers.
    let result = unsafe { super::linux::syscall(72, fd, 3, 0, 0) };
    #[cfg(target_os = "macos")]
    // SAFETY: fcntl F_GETFL takes no pointers.
    let result = unsafe { libc::fcntl(fd as libc::c_int, libc::F_GETFL) } as isize;
    checked(result)
}

/// Closes an exclusively owned raw descriptor, without retrying an uncertain
/// close. Callers must finish every use of the descriptor first.
pub(super) fn close(fd: usize) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    // SAFETY: close takes no pointers; callers surrender their descriptor.
    let result = unsafe { super::linux::syscall(3, fd, 0, 0, 0) };
    #[cfg(target_os = "macos")]
    // SAFETY: close takes no pointers; callers surrender their descriptor.
    let result = unsafe { libc::close(fd as libc::c_int) } as isize;
    checked(result).map(|_| ())
}

pub(super) fn read(fd: usize, bytes: &mut [u8]) -> io::Result<usize> {
    #[cfg(target_os = "linux")]
    // SAFETY: read writes at most the live slice's length into it.
    let result =
        unsafe { super::linux::syscall(0, fd, bytes.as_mut_ptr() as usize, bytes.len(), 0) };
    #[cfg(target_os = "macos")]
    // SAFETY: read writes at most the live slice's length into it.
    let result = unsafe { libc::read(fd as libc::c_int, bytes.as_mut_ptr().cast(), bytes.len()) };
    checked(result)
}

pub(super) fn write(fd: usize, bytes: &[u8]) -> io::Result<usize> {
    #[cfg(target_os = "linux")]
    // SAFETY: write reads at most the live slice's length from it.
    let result = unsafe { super::linux::syscall(1, fd, bytes.as_ptr() as usize, bytes.len(), 0) };
    #[cfg(target_os = "macos")]
    // SAFETY: write reads at most the live slice's length from it.
    let result = unsafe { libc::write(fd as libc::c_int, bytes.as_ptr().cast(), bytes.len()) };
    checked(result)
}

pub(super) fn seek(fd: usize, position: io::SeekFrom) -> io::Result<u64> {
    let (offset, whence) = match position {
        io::SeekFrom::Start(offset) => (offset as i64, libc::SEEK_SET),
        io::SeekFrom::Current(offset) => (offset, libc::SEEK_CUR),
        io::SeekFrom::End(offset) => (offset, libc::SEEK_END),
    };
    #[cfg(target_os = "linux")]
    // SAFETY: lseek takes no pointers.
    let result = unsafe { super::linux::syscall(8, fd, offset as usize, whence as usize, 0) };
    #[cfg(target_os = "macos")]
    // SAFETY: lseek takes no pointers.
    let result = unsafe { libc::lseek(fd as libc::c_int, offset, whence) } as isize;
    checked(result).map(|at| at as u64)
}

pub(super) fn sigpipe_mask() -> io::Result<u64> {
    #[cfg(target_os = "linux")]
    {
        fastmash_sort_process::linux::mask(None)
    }
    #[cfg(target_os = "macos")]
    {
        let mut mask = 0;
        // SAFETY: the null set leaves the mask unchanged and the live output
        // pointer receives one native sigset_t.
        if unsafe { libc::sigprocmask(libc::SIG_SETMASK, std::ptr::null(), &mut mask) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: mask was initialized and SIGPIPE is a valid native signal.
        match unsafe { libc::sigismember(&mask, libc::SIGPIPE) } {
            0 => Ok(0),
            1 => Ok(1 << 12),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

pub(super) fn default_sigpipe() -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        fastmash_sort_process::linux::default_sigpipe()
    }
    #[cfg(target_os = "macos")]
    {
        // SAFETY: SIGPIPE is valid and SIG_DFL is a native disposition.
        if unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) } == libc::SIG_ERR {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(target_os = "macos")]
#[cfg_attr(test, allow(dead_code))]
pub(super) fn admit() -> Result<(), super::Failure> {
    let mut version = [0u8; 64];
    let mut length = version.len();
    // SAFETY: sysctlbyname receives a constant NUL-terminated key and a live
    // buffer sized by length; no new value is supplied.
    let result = unsafe {
        libc::sysctlbyname(
            c"kern.osproductversion".as_ptr(),
            version.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 || length > version.len() {
        return Err(super::unsupported("unable to inspect macOS version"));
    }
    if !supported_version(&version[..length]) {
        return Err(super::unsupported(
            "Apple Silicon requires macOS 15 or later",
        ));
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn supported_version(bytes: &[u8]) -> bool {
    let major = bytes
        .split(|&byte| byte == b'.' || byte == 0)
        .next()
        .unwrap_or_default();
    !major.is_empty()
        && major.iter().all(u8::is_ascii_digit)
        && major
            .iter()
            .try_fold(0u32, |value, &byte| {
                value.checked_mul(10)?.checked_add(u32::from(byte - b'0'))
            })
            .is_some_and(|major| major >= 15)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_minimum_is_fifteen_and_unknown_versions_are_refused() {
        for version in [b"15.0\0".as_slice(), b"26.1\0", b"15\0"] {
            assert!(supported_version(version));
        }
        for version in [
            b"14.9\0".as_slice(),
            b"",
            b"unknown",
            b"15x.0",
            b"9999999999999",
        ] {
            assert!(!supported_version(version));
        }
    }
}
