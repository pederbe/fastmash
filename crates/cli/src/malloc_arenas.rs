//! glibc's malloc arenas under an address-space limit.
//!
//! glibc gives each thread that allocates an arena of its own, and each
//! arena reserves 64 MiB of address space, more as it grows. A limit on
//! address space (`ulimit -v`, as HPC schedulers set it) counts those
//! reservations although little of them is used, so a sort that projects its
//! records on several threads could exhaust it: it refused, or aborted when
//! the C library could not start a thread. Under such a limit, the process
//! keeps to one arena for each 512 MiB of it, from one to eight; threads that
//! share an arena wait for each other's allocations.
use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

/// The address space that a limit gives each malloc arena.
const PER_ARENA: u64 = 512 << 20;
/// The most malloc arenas under a limit: one for each thread of a sort.
const MOST: u64 = 8;

/// Caps glibc's malloc arenas when the address space is limited
/// (`RLIMIT_AS`), unless the environment sets their number. It must run
/// before any thread starts: glibc fixes its arena limit when a thread first
/// needs an arena of its own.
#[cfg_attr(test, allow(dead_code))]
pub(super) fn cap() {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes one `rlimit` through a pointer valid for it.
    if unsafe { libc::getrlimit(libc::RLIMIT_AS, &mut limit) } != 0 {
        return;
    }
    let Some(arenas) = arenas(limit.rlim_cur) else {
        return;
    };
    let tunables = std::env::var_os("GLIBC_TUNABLES");
    let alias = std::env::var_os("MALLOC_ARENA_MAX");
    if configured(tunables.as_deref(), alias.as_deref()) {
        return;
    }
    // SAFETY: the process is still single-threaded (glibc documents mallopt
    // as unsafe beside other threads), and mallopt takes plain integers.
    unsafe { libc::mallopt(libc::M_ARENA_MAX, arenas) };
}

/// Returns the free memory at the top of glibc's heap to the system, as work
/// that freed much of what it allocated ends and other work begins.
pub(super) fn trim() {
    // SAFETY: malloc_trim takes a plain integer and is thread-safe.
    unsafe { libc::malloc_trim(0) };
}

/// The malloc arenas under an address-space limit of `limit` bytes: one for
/// each 512 MiB, from one to eight; none without a limit.
fn arenas(limit: u64) -> Option<libc::c_int> {
    (limit != libc::RLIM_INFINITY).then(|| (limit / PER_ARENA).clamp(1, MOST) as libc::c_int)
}

/// Whether the environment sets glibc's number of arenas to a positive
/// number, which glibc takes: the tunable `glibc.malloc.arena_max` in
/// `GLIBC_TUNABLES` (`name=value` items separated by colons), or its alias
/// `MALLOC_ARENA_MAX`. glibc ignores other values, such as 0.
fn configured(tunables: Option<&OsStr>, alias: Option<&OsStr>) -> bool {
    let positive = |value: &[u8]| {
        !value.is_empty()
            && value.iter().all(u8::is_ascii_digit)
            && value.iter().any(|&digit| digit != b'0')
    };
    alias.is_some_and(|value| positive(value.as_bytes()))
        || tunables.is_some_and(|items| {
            items.as_bytes().split(|&byte| byte == b':').any(|item| {
                item.strip_prefix(b"glibc.malloc.arena_max=")
                    .is_some_and(positive)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_limit_gives_one_arena_for_each_512_mib_from_one_to_eight() {
        assert_eq!(arenas(libc::RLIM_INFINITY), None);
        for (limit, expected) in [
            (0, 1),
            (1, 1),
            (200_000 << 10, 1),
            (750_000 << 10, 1),
            ((1 << 30) - 1, 1),
            (1 << 30, 2),
            ((3 << 30) + (1 << 29), 7),
            (4 << 30, 8),
            (1 << 40, 8),
            (libc::RLIM_INFINITY - 1, 8),
        ] {
            assert_eq!(arenas(limit), Some(expected), "{limit}");
        }
    }

    #[test]
    fn an_arena_count_that_the_environment_sets_is_kept() {
        assert!(!configured(None, None));
        for (alias, set) in [
            ("2", true),
            ("16", true),
            ("", false),
            ("0", false),
            ("x", false),
        ] {
            assert_eq!(configured(None, Some(OsStr::new(alias))), set, "{alias}");
        }
        for (tunables, set) in [
            ("glibc.malloc.arena_max=2", true),
            ("glibc.malloc.tcache_count=0:glibc.malloc.arena_max=1", true),
            ("glibc.malloc.arena_max=4:glibc.malloc.tcache_count=0", true),
            ("", false),
            ("glibc.malloc.arena_test=2", false),
            ("glibc.malloc.arena_max", false),
            ("glibc.malloc.arena_max=", false),
            ("glibc.malloc.arena_max=0", false),
            ("glibc.malloc.arena_max=two", false),
            ("xglibc.malloc.arena_max=2", false),
        ] {
            assert_eq!(
                configured(Some(OsStr::new(tunables)), None),
                set,
                "{tunables}"
            );
        }
    }
}
