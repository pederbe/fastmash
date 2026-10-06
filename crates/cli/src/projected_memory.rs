//! How much memory the in-process sort's chunk takes before it spills.
//!
//! `FASTMASH_SORT_MEMORY_BYTES` fixes it. Otherwise a sort starts at `START`,
//! and each time its chunk fills it may take more (`grow`): for input from a
//! regular file, enough to hold the rest of the input (an estimate from the
//! bytes read and the chunk's memory so far, with a tenth to spare) and at
//! most the buffer GNU `sort` would fill for that file; for piped input,
//! twice its memory. Either only when the memory it would take stays within
//! every limit read at that moment:
//!
//! - at each cgroup level with a limit, a quarter of what is left under it,
//!   and a growth of at most this job's share of it (per usable CPU) after a
//!   reserve, so that as many jobs as CPUs, started together or one after
//!   another, stay within the limit;
//! - on the host, a quarter of the available memory, and a growth of at
//!   most the share (per running fastmash) of what is available above an
//!   eighth of the total, with less than half the swap in use;
//! - half of what is left of the address-space and data rlimits.
//!
//! A fact that cannot be read, or that contradicts another, keeps `START`.
//! Once the chunk has spilled the sort keeps its memory.
//! On macOS the Linux memory sources are absent, so the chunk spills at the
//! initial target rather than growing. This is a chunk target, not an input cap.
use super::{Failure, unsupported};
use std::{
    fs,
    io::Seek,
    os::fd::AsFd,
    path::{Component, Path, PathBuf},
};

/// The memory a sort starts with.
pub(super) const START: usize = 64 << 20;

/// A sort's memory target: its bytes, and whether its chunk may grow
/// (`grow`) when it fills.
#[derive(Clone, Copy, Debug)]
pub(super) struct Target {
    pub(super) bytes: usize,
    pub(super) grows: bool,
}

/// The memory target of a sort: `FASTMASH_SORT_MEMORY_BYTES` (`setting`), fixed, or
/// `START`, grown as the chunk fills.
pub(super) fn target(setting: Option<&std::ffi::OsStr>) -> Result<Target, Failure> {
    match setting {
        None => Ok(Target {
            bytes: START,
            grows: true,
        }),
        Some(value) => value
            .to_str()
            .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .and_then(|s| s.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .map(|bytes| Target {
                bytes,
                grows: false,
            })
            .ok_or_else(|| unsupported("FASTMASH_SORT_MEMORY_BYTES must be a positive byte count")),
    }
}

/// The memory a full chunk of `memory` bytes, holding the sort's first
/// `records` records, may take next: from facts read now (`next`), or `None`
/// to spill. `FASTMASH_SORT_TRACE` reports each decision on standard error.
pub(super) fn grow(memory: usize, records: u64) -> Option<usize> {
    let facts = Facts {
        input: regular_input().map(|(size, consumed)| Input {
            size,
            consumed,
            records,
            threads: fastmash_sort_process::sort_threads(),
        }),
        cpus: std::thread::available_parallelism().map_or(1, |n| n.get() as u64),
        sorts: running_sorts(Path::new("/")),
        ..host(Path::new("/"))
    };
    let next = next(&facts, memory as u64);
    if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
        let mib = |bytes: u64| bytes >> 20;
        let message = match next {
            Ok(bytes) => format!(
                "sort memory: chunk of {} MiB full after {records} records: grows to {} MiB\n",
                mib(memory as u64),
                mib(bytes)
            ),
            Err(reason) => format!(
                "sort memory: chunk of {} MiB full after {records} records: stays ({reason})\n",
                mib(memory as u64)
            ),
        };
        let _ = std::io::Write::write_all(&mut std::io::stderr(), message.as_bytes());
    }
    next.ok().and_then(|bytes| usize::try_from(bytes).ok())
}

/// A memory limit, and the memory used against it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Headroom {
    pub(super) limit: u64,
    pub(super) used: u64,
}

/// The host's memory (`/proc/meminfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Host {
    pub(super) total: u64,
    pub(super) available: u64,
    pub(super) swap_total: u64,
    pub(super) swap_free: u64,
}

/// A regular-file input, as GNU `sort` sizes its buffer for it.
#[derive(Clone, Copy, Debug)]
pub(super) struct Input {
    /// The file's size (`st_size`).
    pub(super) size: u64,
    /// The bytes read from the file so far, and the records they held.
    pub(super) consumed: u64,
    pub(super) records: u64,
    /// The threads GNU `sort` would use (`sort_threads`).
    pub(super) threads: u32,
}

/// What a chunk's growth depends on. `None` is a fact that could not be
/// read, which keeps the chunk as it is.
#[derive(Clone, Debug, Default)]
pub(super) struct Facts {
    /// A regular-file input; `None` for piped and other input.
    pub(super) input: Option<Input>,
    /// The limit and use of each cgroup level with a memory limit.
    pub(super) cgroups: Option<Vec<Headroom>>,
    pub(super) host: Option<Host>,
    /// `RLIMIT_AS` against `VmSize`, and `RLIMIT_DATA` against `VmData`,
    /// when finite.
    pub(super) address_space: Option<Headroom>,
    pub(super) data: Option<Headroom>,
    /// Whether the rlimits and their use could be read.
    pub(super) limits_read: bool,
    /// The CPUs this process may use, and the fastmash processes running
    /// (this one among them).
    pub(super) cpus: u64,
    pub(super) sorts: u64,
}

/// GNU `sort`'s bytes per line besides the line's text on `threads`
/// threads: a 32-byte line structure and half another on one thread, and one
/// for each merge level, 1 + ceil(log2 threads), on more (coreutils sort.c,
/// `sort`).
fn bytes_per_line(threads: u32) -> u128 {
    if threads <= 1 {
        48
    } else {
        32 * (1 + u128::from(threads.next_power_of_two().trailing_zeros()))
    }
}

/// The buffer GNU `sort` fills for `input`: the file, and `bytes_per_line`
/// for each of its lines, estimated from the mean size of the records read
/// so far.
fn gnu_buffer(input: Input) -> Option<u128> {
    if input.consumed == 0 || input.records == 0 {
        return None;
    }
    let lines = u128::from(input.size) * u128::from(input.records) / u128::from(input.consumed);
    Some(u128::from(input.size) + lines * bytes_per_line(input.threads))
}

/// The memory a full chunk of `memory` bytes takes next under `facts`, or
/// why it stays: see the module's rule.
pub(super) fn next(facts: &Facts, memory: u64) -> Result<u64, &'static str> {
    let host = facts.host.ok_or("host memory unreadable")?;
    let cgroups = facts.cgroups.as_ref().ok_or("cgroup memory unreadable")?;
    if !facts.limits_read {
        return Err("resource limits unreadable");
    }
    let memory = u128::from(memory);
    let target = match facts.input {
        // The rest of the file, at the chunk's memory per byte so far.
        Some(input) if input.consumed > 0 && input.size > input.consumed => {
            let whole = memory * u128::from(input.size) / u128::from(input.consumed);
            let target = whole + whole / 10;
            if gnu_buffer(input).is_none_or(|buffer| target > buffer) {
                return Err("beyond GNU sort's buffer for the file");
            }
            target
        }
        _ => memory * 2,
    };
    if target <= memory {
        return Err("no growth");
    }
    let growth = target - memory;
    for limit in [facts.address_space, facts.data].into_iter().flatten() {
        if target > u128::from(limit.limit.saturating_sub(limit.used)) / 2 {
            return Err("resource limit");
        }
    }
    let cpus = u128::from(facts.cpus.max(1));
    for level in cgroups {
        let left = u128::from(level.limit.saturating_sub(level.used));
        let reserve = u128::from((level.limit / 8).max(128 << 20));
        if growth > left.saturating_sub(reserve) / cpus || target > left / 4 {
            return Err("cgroup memory limit");
        }
    }
    let spare = u128::from(host.available.saturating_sub(host.total / 8));
    if growth > spare / u128::from(facts.sorts.max(1)) || target > u128::from(host.available) / 4 {
        return Err("available memory");
    }
    if host.swap_total > 0 && host.swap_free < host.swap_total / 2 {
        return Err("swap in use");
    }
    u64::try_from(target).map_err(|_| "no growth")
}

/// The size of standard input when it is a regular file of positive size,
/// and its offset: the bytes read from it so far.
fn regular_input() -> Option<(u64, u64)> {
    let mut file = fs::File::from(std::io::stdin().as_fd().try_clone_to_owned().ok()?);
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() == 0 {
        return None;
    }
    Some((metadata.len(), file.stream_position().ok()?))
}

/// The host's memory facts and this process's limits, read under `root`
/// (`/` outside tests).
fn host(root: &Path) -> Facts {
    let meminfo = read(&root.join("proc/meminfo"));
    let field = |name| meminfo.as_deref().and_then(|text| kib(text, name));
    let host = match (field("MemTotal"), field("MemAvailable")) {
        (Some(total), Some(available)) => Some(Host {
            total,
            available,
            swap_total: field("SwapTotal").unwrap_or(0),
            swap_free: field("SwapFree").unwrap_or(0),
        }),
        _ => None,
    };
    let status = read(&root.join("proc/self/status"));
    let used = |name| status.as_deref().and_then(|text| kib(text, name));
    let limits = read(&root.join("proc/self/limits"));
    let limit = |name| limits.as_deref().map(|text| soft_limit(text, name));
    let (address_space, data) = (limit("Max address space"), limit("Max data size"));
    let headroom = |limit: Option<Option<u64>>, name| match limit {
        Some(Some(limit)) => used(name).map(|used| Some(Headroom { limit, used })),
        Some(None) => Some(None),
        None => None,
    };
    let (address_space, data) = (headroom(address_space, "VmSize"), headroom(data, "VmData"));
    Facts {
        cgroups: cgroups(root),
        host,
        limits_read: address_space.is_some() && data.is_some(),
        address_space: address_space.flatten(),
        data: data.flatten(),
        ..Facts::default()
    }
}

/// A file's text, with any bytes that are not UTF-8 replaced.
fn read(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// The bytes of a `name:  N kB` line of `/proc/self/status` or
/// `/proc/meminfo`.
fn kib(text: &str, name: &str) -> Option<u64> {
    let value = text
        .lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))?;
    let mut words = value.split_whitespace();
    let number = words.next()?.parse::<u64>().ok()?;
    match words.next() {
        Some("kB") => number.checked_mul(1024),
        _ => None,
    }
}

/// The soft limit of the `name` line of `/proc/self/limits`: `None` when
/// unlimited (or missing, as unlimited).
fn soft_limit(text: &str, name: &str) -> Option<u64> {
    let rest = text.lines().find_map(|line| line.strip_prefix(name))?;
    rest.split_whitespace().next()?.parse().ok()
}

/// A number file of a cgroup: `None` when it is missing or is `max`.
fn number(path: &Path) -> Option<u64> {
    read(path)?.trim().parse().ok()
}

/// The `name` counter of a cgroup's `memory.stat`.
fn stat(text: &str, name: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))?
        .trim()
        .parse()
        .ok()
}

/// The memory limits of the process's cgroups under `root`, each with the
/// memory its cgroup uses that reclaim cannot free: in cgroup v2, each
/// level's lower of `memory.max` and `memory.high`, against `memory.current`
/// less clean page cache, plus swap; in cgroup v1, `hierarchical_memory_limit`
/// against `memory.usage_in_bytes` likewise. `None` when the membership, a
/// limited level's use, or the process's own place in the hierarchy cannot
/// be confirmed.
fn cgroups(root: &Path) -> Option<Vec<Headroom>> {
    let membership = read(&root.join("proc/self/cgroup"))?;
    let mut levels = Vec::new();
    for line in membership.lines() {
        // hierarchy:controllers:path
        let mut parts = line.splitn(3, ':');
        let (Some(_), Some(controllers), Some(path)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        if controllers.is_empty() {
            unified(&root.join("sys/fs/cgroup"), path, &mut levels)?;
        } else if controllers.split(',').any(|name| name == "memory") {
            legacy(root, path, &mut levels)?;
        }
    }
    Some(levels)
}

/// Cgroup v2 levels with a limit, from the hierarchy's root `mount` down to
/// the process's cgroup `path`, which must list this process.
fn unified(mount: &Path, path: &str, levels: &mut Vec<Headroom>) -> Option<()> {
    let mut dir = mount.to_path_buf();
    let mut dirs = vec![dir.clone()];
    for component in Path::new(path).components() {
        match component {
            Component::Normal(name) => {
                dir.push(name);
                dirs.push(dir.clone());
            }
            Component::RootDir | Component::CurDir => {}
            // A cgroup outside this namespace.
            _ => return None,
        }
    }
    // A cgroup namespace over the host's mount names the host's root: this
    // process is not listed there.
    let own = std::process::id().to_string();
    read(&dir.join("cgroup.procs"))?
        .lines()
        .any(|pid| pid.trim() == own)
        .then_some(())?;
    for dir in dirs {
        let limit = ["memory.max", "memory.high"]
            .iter()
            .filter_map(|file| number(&dir.join(file)))
            .min();
        if let Some(limit) = limit {
            let current = number(&dir.join("memory.current"))?;
            let stats = read(&dir.join("memory.stat"))?;
            let counter = |name| stat(&stats, name).unwrap_or(0);
            let used = current
                .saturating_sub(counter("active_file"))
                .saturating_sub(counter("inactive_file"))
                .saturating_add(counter("file_dirty"))
                .saturating_add(counter("file_writeback"))
                .saturating_add(number(&dir.join("memory.swap.current")).unwrap_or(0));
            levels.push(Headroom { limit, used });
        }
    }
    Some(())
}

/// The cgroup v1 memory controller's limit for the process's cgroup `path`,
/// through the controller's mount in `/proc/self/mountinfo`. Limits from
/// 2^62 mean none.
fn legacy(root: &Path, path: &str, levels: &mut Vec<Headroom>) -> Option<()> {
    let mounts = read(&root.join("proc/self/mountinfo"))?;
    for line in mounts.lines() {
        // id parent device root mount-point options [tags] - type source options
        let Some((mount, filesystem)) = line.split_once(" - ") else {
            continue;
        };
        let mut filesystem = filesystem.split(' ');
        let memory = filesystem.next() == Some("cgroup")
            && filesystem
                .nth(1)
                .is_some_and(|options| options.split(',').any(|option| option == "memory"));
        let mut fields = mount.split(' ').skip(3);
        let (true, Some(mount_root), Some(point)) = (memory, fields.next(), fields.next()) else {
            continue;
        };
        let (mount_root, point) = (unescape(mount_root), unescape(point));
        let below = Path::new(path).strip_prefix(&mount_root).ok()?;
        let dir = root
            .join(point.strip_prefix("/").unwrap_or(&point))
            .join(below);
        let stats = read(&dir.join("memory.stat"))?;
        let limit = stat(&stats, "hierarchical_memory_limit").filter(|&limit| limit < 1 << 62);
        if let Some(limit) = limit {
            let counter = |name| stat(&stats, name).unwrap_or(0);
            let used = number(&dir.join("memory.usage_in_bytes"))?
                .saturating_sub(counter("total_active_file"))
                .saturating_sub(counter("total_inactive_file"))
                .saturating_add(counter("total_dirty"))
                .saturating_add(counter("total_writeback"));
            levels.push(Headroom { limit, used });
        }
        return Some(());
    }
    None
}

/// A mount table field with its octal escapes (`\040` for a space) decoded.
fn unescape(field: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    let bytes = field.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let code = bytes.get(at + 1..at + 4).and_then(|digits| {
            let digits = std::str::from_utf8(digits).ok()?;
            u8::from_str_radix(digits, 8).ok()
        });
        match code {
            Some(byte) if bytes[at] == b'\\' => {
                out.push(byte);
                at += 4;
            }
            _ => {
                out.push(bytes[at]);
                at += 1;
            }
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(out))
}

/// The fastmash processes running under `root`'s `/proc`, this one among
/// them: at least one.
fn running_sorts(root: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(root.join("proc")) else {
        return 1;
    };
    let count = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.bytes().all(|b| b.is_ascii_digit()))
        })
        .filter(|entry| fs::read(entry.path().join("comm")).is_ok_and(|comm| comm == b"fastmash\n"))
        .count();
    (count as u64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TempDir as Root;

    const MIB: u64 = 1 << 20;
    const GIB: u64 = 1 << 30;

    /// The 6M-row input of the memory campaigns: 94,666,612 bytes, of which
    /// the first 15,777,768 held a million records.
    fn six_million(threads: u32) -> Input {
        Input {
            size: 94_666_612,
            consumed: 15_777_768,
            records: 1_000_000,
            threads,
        }
    }

    /// A 16 GiB host with 10 GiB available and no swap, no limits, 8 CPUs.
    fn roomy() -> Facts {
        Facts {
            input: None,
            cgroups: Some(Vec::new()),
            host: Some(Host {
                total: 16 * GIB,
                available: 10 * GIB,
                swap_total: 0,
                swap_free: 0,
            }),
            address_space: None,
            data: None,
            limits_read: true,
            cpus: 8,
            sorts: 1,
        }
    }

    #[test]
    fn gnu_buffer_follows_sort_bytes_per_line() {
        assert_eq!(
            [1, 2, 3, 4, 5, 8, 9].map(bytes_per_line),
            [48, 64, 96, 96, 128, 128, 160]
        );
        // 6M rows: GNU coreutils sort was measured at 825 MiB of anonymous
        // memory on eight threads and 386 MiB on one.
        assert_eq!(gnu_buffer(six_million(8)), Some(862_666_612));
        assert_eq!(gnu_buffer(six_million(1)), Some(382_666_612));
    }

    #[test]
    fn a_file_grows_to_hold_the_rest_within_every_limit() {
        let memory = 60 * MIB;
        // 60 MiB for the first sixth: the whole file, with a tenth to spare.
        let whole = u64::try_from(u128::from(memory) * 94_666_612 / 15_777_768).unwrap();
        let fits = whole + whole / 10;
        let file = Facts {
            input: Some(six_million(8)),
            ..roomy()
        };
        assert_eq!(next(&file, memory), Ok(fits));
        let cases: [(&str, Facts, Result<u64, &str>); 12] = [
            (
                "GNU sort holds less on one thread (383 MB)",
                Facts {
                    input: Some(six_million(1)),
                    ..roomy()
                },
                Err("beyond GNU sort's buffer for the file"),
            ),
            (
                "fewer lines give GNU sort less to hold",
                Facts {
                    input: Some(Input {
                        records: 100_000,
                        ..six_million(1)
                    }),
                    ..roomy()
                },
                Err("beyond GNU sort's buffer for the file"),
            ),
            (
                "a quarter of the cgroup's headroom",
                Facts {
                    cgroups: Some(vec![Headroom {
                        limit: 2 * GIB,
                        used: 600 * MIB,
                    }]),
                    ..file.clone()
                },
                Err("cgroup memory limit"),
            ),
            (
                "a cgroup with room for a CPU's share",
                Facts {
                    cgroups: Some(vec![Headroom {
                        limit: 64 * GIB,
                        used: GIB,
                    }]),
                    ..file.clone()
                },
                Ok(fits),
            ),
            (
                "a share per CPU of a cgroup's headroom",
                Facts {
                    cgroups: Some(vec![Headroom {
                        limit: 3 * GIB,
                        used: 100 * MIB,
                    }]),
                    ..file.clone()
                },
                Err("cgroup memory limit"),
            ),
            (
                "one CPU takes the whole share",
                Facts {
                    cgroups: Some(vec![Headroom {
                        limit: 3 * GIB,
                        used: 100 * MIB,
                    }]),
                    cpus: 1,
                    ..file.clone()
                },
                Ok(fits),
            ),
            (
                "a quarter of available memory",
                Facts {
                    host: Some(Host {
                        available: GIB,
                        ..roomy().host.unwrap()
                    }),
                    ..file.clone()
                },
                Err("available memory"),
            ),
            (
                "a share per running fastmash",
                Facts {
                    sorts: 40,
                    ..file.clone()
                },
                Err("available memory"),
            ),
            (
                "half the swap in use",
                Facts {
                    host: Some(Host {
                        swap_total: 8 * GIB,
                        swap_free: 3 * GIB,
                        ..roomy().host.unwrap()
                    }),
                    ..file.clone()
                },
                Err("swap in use"),
            ),
            (
                "half an address-space limit's headroom",
                Facts {
                    address_space: Some(Headroom {
                        limit: 900 * MIB,
                        used: 200 * MIB,
                    }),
                    ..file.clone()
                },
                Err("resource limit"),
            ),
            (
                "unreadable cgroups",
                Facts {
                    cgroups: None,
                    ..file.clone()
                },
                Err("cgroup memory unreadable"),
            ),
            (
                "unreadable limits",
                Facts {
                    limits_read: false,
                    ..file.clone()
                },
                Err("resource limits unreadable"),
            ),
        ];
        for (name, facts, expected) in cases {
            assert_eq!(next(&facts, memory), expected, "{name}");
        }
        // Piped input doubles, within the same limits.
        assert_eq!(next(&roomy(), memory), Ok(2 * memory));
        let small = Facts {
            host: Some(Host {
                available: 400 * MIB,
                ..roomy().host.unwrap()
            }),
            ..roomy()
        };
        assert_eq!(next(&small, memory), Err("available memory"));
        assert_eq!(
            next(
                &Facts {
                    host: None,
                    ..roomy()
                },
                memory
            ),
            Err("host memory unreadable")
        );
    }

    impl Root {
        fn file(&self, path: &str, contents: &str) -> &Self {
            let path = self.0.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
            self
        }
    }

    #[test]
    fn a_rejected_root_cannot_redirect_fixture_writes() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let parent = Root::new("memory-symlink-parent");
        let sentinel = parent.0.join("sentinel");
        fs::write(&sentinel, b"untouched").unwrap();
        let occupied = parent.0.join("old-root");
        let nested = occupied.join("proc/self");
        fs::create_dir_all(&nested).unwrap();
        symlink(&sentinel, nested.join("cgroup")).unwrap();
        // Without write permission here, a failed removal leaves the symlink.
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o500)).unwrap();
        let result = Root::create(occupied);
        // Restore access for the test-owned parent's cleanup, even on failure.
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
        match result {
            Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists),
            Ok(root) => {
                root.file("proc/self/cgroup", "overwritten");
                panic!("a preexisting root was accepted");
            }
        }
        assert_eq!(fs::read_link(nested.join("cgroup")).unwrap(), sentinel);
        assert_eq!(fs::read(sentinel).unwrap(), b"untouched");
    }

    #[test]
    fn cgroup_v2_limits_count_what_reclaim_cannot_free() {
        let own = format!("{}\n", std::process::id());
        let root = Root::new("v2");
        root.file("proc/self/cgroup", "0::/user.slice/job.scope\n")
            // The root cgroup has no memory.max.
            .file("sys/fs/cgroup/memory.stat", "anon 0\n")
            .file("sys/fs/cgroup/user.slice/memory.max", "4294967296\n")
            .file("sys/fs/cgroup/user.slice/memory.high", "max\n")
            .file("sys/fs/cgroup/user.slice/memory.current", "1073741824\n")
            .file(
                "sys/fs/cgroup/user.slice/memory.stat",
                "anon 100\nactive_file 268435456\ninactive_file 268435456\nfile_dirty 1048576\nfile_writeback 0\n",
            )
            .file("sys/fs/cgroup/user.slice/memory.swap.current", "1048576\n")
            .file("sys/fs/cgroup/user.slice/job.scope/memory.max", "max\n")
            .file(
                "sys/fs/cgroup/user.slice/job.scope/memory.high",
                "2147483648\n",
            )
            .file(
                "sys/fs/cgroup/user.slice/job.scope/memory.current",
                "536870912\n",
            )
            .file("sys/fs/cgroup/user.slice/job.scope/memory.stat", "anon 1\n")
            .file("sys/fs/cgroup/user.slice/job.scope/cgroup.procs", &own);
        assert_eq!(
            cgroups(&root.0),
            Some(vec![
                Headroom {
                    limit: 4 * GIB,
                    used: GIB / 2 + 2 * MIB,
                },
                Headroom {
                    limit: 2 * GIB,
                    used: GIB / 2,
                },
            ])
        );
        // A limited level whose use cannot be read confirms nothing.
        fs::remove_file(
            root.0
                .join("sys/fs/cgroup/user.slice/job.scope/memory.current"),
        )
        .unwrap();
        assert_eq!(cgroups(&root.0), None);
        // No limit anywhere: "max", or missing files.
        for file in ["user.slice/memory.max", "user.slice/job.scope/memory.high"] {
            root.file(&format!("sys/fs/cgroup/{file}"), "max\n");
        }
        assert_eq!(cgroups(&root.0), Some(Vec::new()));
        // A cgroup namespace over the host's mount names the host's root, which
        // does not list this process.
        root.file("proc/self/cgroup", "0::/\n")
            .file("sys/fs/cgroup/cgroup.procs", "1\n");
        assert_eq!(cgroups(&root.0), None);
        // A container's namespace root carries its limit and lists it.
        let container = Root::new("v2-container");
        container
            .file("proc/self/cgroup", "0::/\n")
            .file("sys/fs/cgroup/memory.max", "536870912\n")
            .file("sys/fs/cgroup/memory.current", "0\n")
            .file("sys/fs/cgroup/memory.stat", "")
            .file("sys/fs/cgroup/cgroup.procs", &format!("1\n{own}"));
        assert_eq!(
            cgroups(&container.0),
            Some(vec![Headroom {
                limit: GIB / 2,
                used: 0
            }])
        );
        // Without a membership, nothing is known.
        assert_eq!(cgroups(&Root::new("none").0), None);
    }

    #[test]
    fn cgroup_v1_limits_come_from_the_memory_mount() {
        let root = Root::new("v1");
        root.file(
            "proc/self/cgroup",
            "12:cpu,cpuacct:/job\n4:memory:/batch/job\n1:name=systemd:/job\n",
        )
        .file(
            "proc/self/mountinfo",
            "25 30 0:22 / /sys/fs/cgroup rw - tmpfs tmpfs rw\n\
             33 25 0:28 / /sys/fs/cgroup/cpu,cpuacct rw shared:14 - cgroup cgroup rw,cpu,cpuacct\n\
             35 25 0:30 /batch /sys/fs/cgroup/mem\\040ory rw shared:16 - cgroup cgroup rw,memory\n",
        )
        .file(
            "sys/fs/cgroup/mem ory/job/memory.stat",
            "cache 0\nhierarchical_memory_limit 3221225472\ntotal_inactive_file 1048576\n",
        )
        .file(
            "sys/fs/cgroup/mem ory/job/memory.usage_in_bytes",
            "1073741824\n",
        );
        assert_eq!(
            cgroups(&root.0),
            Some(vec![Headroom {
                limit: 3 * GIB,
                used: GIB - MIB,
            }])
        );
        // The unlimited value means no limit.
        root.file(
            "sys/fs/cgroup/mem ory/job/memory.stat",
            "hierarchical_memory_limit 9223372036854771712\n",
        );
        assert_eq!(cgroups(&root.0), Some(Vec::new()));
        // Without a mount table, nothing is known.
        fs::remove_file(root.0.join("proc/self/mountinfo")).unwrap();
        assert_eq!(cgroups(&root.0), None);
    }

    #[test]
    fn host_memory_and_limits_come_from_proc() {
        let root = Root::new("proc");
        root.file(
            "proc/meminfo",
            "MemTotal:       16384000 kB\nMemFree:  1 kB\nMemAvailable:    8192000 kB\nSwapTotal:       4096000 kB\nSwapFree:        4000000 kB\n",
        )
        .file(
            "proc/self/status",
            "Name:\tfastmash\nVmSize:\t  204800 kB\nVmData:\t  102400 kB\n",
        )
        .file(
            "proc/self/limits",
            "Limit                     Soft Limit           Hard Limit           Units     \n\
             Max data size             unlimited            unlimited            bytes     \n\
             Max address space         1073741824           unlimited            bytes     \n",
        )
        .file("proc/self/cgroup", "");
        let facts = host(&root.0);
        assert_eq!(
            facts.host,
            Some(Host {
                total: 16_384_000 * 1024,
                available: 8_192_000 * 1024,
                swap_total: 4_096_000 * 1024,
                swap_free: 4_000_000 * 1024,
            })
        );
        assert!(facts.limits_read);
        assert_eq!(
            facts.address_space,
            Some(Headroom {
                limit: GIB,
                used: 200 * MIB
            })
        );
        assert_eq!(facts.data, None);
        assert_eq!(facts.cgroups, Some(Vec::new()));
        // Without /proc/self/limits, the limits are unknown.
        fs::remove_file(root.0.join("proc/self/limits")).unwrap();
        assert!(!host(&root.0).limits_read);
        assert_eq!(host(&Root::new("empty").0).host, None);
    }

    #[test]
    fn running_sorts_count_fastmash_processes() {
        let root = Root::new("pids");
        root.file("proc/1/comm", "systemd\n")
            .file("proc/20/comm", "fastmash\n")
            .file("proc/21/comm", "fastmash\n")
            .file("proc/self/comm", "fastmash\n");
        assert_eq!(running_sorts(&root.0), 2);
        assert_eq!(running_sorts(&Root::new("no-proc").0), 1);
    }
}
