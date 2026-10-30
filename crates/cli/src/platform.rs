//! Native process primitives used by the standard streams and signal policy.
use std::io;

pub(super) use super::standard_stream_io::{read, seek, write};

/// Closes an exclusively owned raw descriptor, without retrying an uncertain
/// close. Callers must finish every use of the descriptor first.
#[forbid(unsafe_code)]
pub(super) fn close(fd: usize) -> io::Result<()> {
    nix::unistd::close(fd as std::os::fd::RawFd).map_err(io::Error::from)
}

#[forbid(unsafe_code)]
pub(super) fn sigpipe_mask() -> io::Result<u64> {
    use nix::sys::signal::{SigSet, SigmaskHow, Signal, sigprocmask};
    // The pinned pthread query wrapper misses positive error returns. Use the
    // native -1/errno interface and initialized storage instead.
    let mut mask = SigSet::empty();
    sigprocmask(SigmaskHow::SIG_SETMASK, None, Some(&mut mask)).map_err(io::Error::from)?;
    Ok(u64::from(mask.contains(Signal::SIGPIPE)) << 12)
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
    fn sigpipe_observation_preserves_the_calling_threads_mask() {
        use nix::sys::signal::{SigSet, SigmaskHow, Signal, sigprocmask};
        let current = || {
            let mut mask = SigSet::empty();
            sigprocmask(SigmaskHow::SIG_SETMASK, None, Some(&mut mask)).map(|()| mask)
        };
        let original = current().unwrap();
        let mut blocked = original;
        blocked.add(Signal::SIGPIPE);
        blocked.add(Signal::SIGTERM);
        sigprocmask(SigmaskHow::SIG_SETMASK, Some(&blocked), None).unwrap();
        let observed = sigpipe_mask();
        let after = current();
        sigprocmask(SigmaskHow::SIG_SETMASK, Some(&original), None).unwrap();
        assert_ne!(observed.unwrap() & (1 << 12), 0);
        assert_eq!(after.unwrap(), blocked);
    }

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
