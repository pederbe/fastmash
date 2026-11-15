//! How much memory the in-process sort's chunk takes before it spills.
//!
//! `FASTMASH_SORT_MEMORY_BYTES` fixes it. Otherwise a sort's chunk takes
//! `FIRST`, and each time it fills it may take more (`grow`): for input from a
//! regular file, enough to hold the rest of the input (an estimate from the
//! bytes read and the chunk's memory so far, with a tenth to spare) and at
//! most the buffer GNU `sort` would fill for that file; for piped input,
//! twice its memory, or a 50% chunk increase when doubling does not fit.
//! Either only when the memory it would take stays within
//! every limit read at that moment:
//!
//! - at each cgroup level with a limit, half of what is left under it, and a
//!   growth of at most this job's share of it (per running fastmash) after a
//!   reserve, so that sorts running together share the limit and later ones
//!   still find room;
//! - on the host, a quarter of the available memory, and a growth of at
//!   most the share (per running fastmash) of what is available above an
//!   eighth of the total, with less than half the swap in use;
//! - half of what is left of the address-space and data rlimits.
//!
//! A sort below `START`, counting its run buffers and its producer's share,
//! asks for at least `START`. If that growth is refused, it still takes
//! `START` unless the cgroup rule refuses that too, so a sort started beside
//! others whose chunks have grown spills rather than overrunning their limit.
//! A fact that cannot be read, or that contradicts another, keeps `START`.
//! Once the chunk has spilled the sort keeps its memory. On macOS the Linux
//! memory sources are absent, so the chunk spills at `START` rather than
//! growing. This is a chunk target, not an input cap.
use super::{Failure, unsupported};
#[cfg(unix)]
#[path = "projected_memory_unix.rs"]
mod native;
#[cfg(all(test, target_os = "linux"))]
pub(super) use native::CGROUP_LIMITED;
#[cfg(target_os = "linux")]
pub(super) use native::cgroup_limited;
#[cfg(unix)]
pub(super) use native::grow;

/// The memory a sort is sized for, which it takes once a cgroup memory limit
/// leaves room for it.
pub(super) const START: usize = 64 << 20;

/// The memory a sort takes before its first decision (`grow`).
pub(super) const FIRST: usize = 16 << 20;

/// A sort's memory target: the bytes its buffers are sized for, the bytes its
/// chunk takes before it first fills, and whether it may grow (`grow`) then.
#[derive(Clone, Copy, Debug)]
pub(super) struct Target {
    pub(super) bytes: usize,
    pub(super) first: usize,
    pub(super) grows: bool,
}

/// The memory target of a sort: `FASTMASH_SORT_MEMORY_BYTES` (`setting`), fixed, or
/// `START`, taken from `FIRST` and grown as the chunk fills.
pub(super) fn target(setting: Option<&std::ffi::OsStr>) -> Result<Target, Failure> {
    match setting {
        None => Ok(Target {
            bytes: START,
            first: if cfg!(windows) { START } else { FIRST },
            grows: cfg!(unix),
        }),
        Some(value) => value
            .to_str()
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .map(|bytes| Target {
                bytes,
                first: bytes,
                grows: false,
            })
            .ok_or_else(|| unsupported("FASTMASH_SORT_MEMORY_BYTES must be a positive byte count")),
    }
}

/// A sort's growth (`grow`): given its memory, the part of it set aside
/// from the chunk, and its records so far, the memory it may take next.
pub(super) type Growth = fn(usize, usize, u64) -> Option<usize>;

/// Windows uses the fixed chunk target and ordinary spill I/O initially.
#[cfg(windows)]
pub(super) fn grow(_: usize, _: usize, _: u64) -> Option<usize> {
    None
}
