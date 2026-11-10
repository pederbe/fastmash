//! Native process primitives used by the standard streams and signal policy.
#![forbid(unsafe_code)]
use std::io;

pub(super) use super::standard_stream_io::{read, seek, write};

/// Closes an exclusively owned raw descriptor, without retrying an uncertain
/// close. Callers must finish every use of the descriptor first.
pub(super) fn close(fd: usize) -> io::Result<()> {
    nix::unistd::close(fd as std::os::fd::RawFd).map_err(io::Error::from)
}

pub(super) fn sigpipe_mask() -> io::Result<u64> {
    use nix::sys::signal::{SigSet, SigmaskHow, Signal, sigprocmask};
    // The pinned pthread query wrapper misses positive error returns. Use the
    // native -1/errno interface and initialized storage instead.
    let mut mask = SigSet::empty();
    sigprocmask(SigmaskHow::SIG_SETMASK, None, Some(&mut mask)).map_err(io::Error::from)?;
    Ok(u64::from(mask.contains(Signal::SIGPIPE)) << 12)
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
}
