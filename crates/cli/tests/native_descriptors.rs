#![forbid(unsafe_code)]

//! Executed-command checks for closed and incorrectly opened standard streams.
#[path = "support/executable.rs"]
mod executable;
#[path = "support/temp_dir.rs"]
mod temp_dir;

use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::close;
use std::{
    error::Error,
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{fd::AsRawFd, unix::ffi::OsStrExt, unix::fs::OpenOptionsExt},
    path::Path,
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const CHILD_LIMIT: Duration = Duration::from_secs(5);
const CLEANUP_RESERVE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug)]
enum Streams {
    Ordinary,
    Closed(u8),
    WrongAccess(u8),
}

#[derive(Debug)]
struct Observed {
    parent_binding_fds: [i32; 3],
    parent_binding_flags: [i32; 3],
    parent_close_results: [nix::Result<()>; 3],
    status: WaitStatus,
    timed_out: bool,
    cancellation: Option<nix::Result<()>>,
    stdout_length: u64,
    stderr_length: u64,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[test]
fn ordinary_standard_streams_count_records() {
    check(Streams::Ordinary, 0, b"2\n", b"");
}

#[test]
fn closed_standard_stream_combinations_keep_their_original_failure() {
    for closed in 1u8..8 {
        let status = i32::from(closed & 3 != 0);
        let stdout: &[u8] = if closed & 3 == 0 { b"2\n" } else { b"" };
        let stderr: &[u8] = if closed & 4 != 0 {
            b""
        } else if closed & 1 != 0 {
            b"fastmash: read error: Bad file descriptor\n"
        } else {
            b"fastmash: write error: Bad file descriptor\n"
        };
        check(Streams::Closed(closed), status, stdout, stderr);
    }
}

#[test]
fn wrong_access_stream_combinations_keep_their_original_failure() {
    for wrong in 1u8..8 {
        let status = i32::from(wrong & 3 != 0);
        let stdout: &[u8] = if wrong & 3 == 0 { b"2\n" } else { b"" };
        let stderr: &[u8] = if wrong & 4 != 0 {
            b""
        } else if wrong & 1 != 0 {
            b"fastmash: read error: Bad file descriptor\n"
        } else {
            b"fastmash: write error: Bad file descriptor\n"
        };
        check(Streams::WrongAccess(wrong), status, stdout, stderr);
    }
}

fn check(streams: Streams, status: i32, stdout: &[u8], stderr: &[u8]) {
    let directory = temp_dir::TempDir::new(&format!("native-descriptors-{streams:?}"));
    let path = directory.0.clone();
    let observed = invoke(streams, &path, stdout.len() + 1, stderr.len() + 1);
    drop(directory);
    assert!(
        !path.try_exists().unwrap(),
        "{streams:?}: temporary cleanup"
    );
    let observed = observed.unwrap_or_else(|error| panic!("{streams:?}: {error}"));
    eprintln!(
        "{streams:?}: access flags={:?}, reaped {observed:?}",
        observed.parent_binding_flags
    );
    assert!(
        observed.parent_binding_fds.iter().all(|fd| *fd >= 3),
        "{streams:?}: {observed:?}"
    );
    assert!(
        observed
            .parent_close_results
            .iter()
            .all(|result| result.is_ok()),
        "{streams:?}: {observed:?}"
    );
    assert!(!observed.timed_out, "{streams:?}: {observed:?}");
    assert!(observed.cancellation.is_none(), "{streams:?}: {observed:?}");
    assert!(
        matches!(observed.status, WaitStatus::Exited(_, code) if code == status),
        "{streams:?}: {observed:?}"
    );
    assert_eq!(observed.stdout_length, stdout.len() as u64, "{streams:?}");
    assert_eq!(observed.stderr_length, stderr.len() as u64, "{streams:?}");
    assert_eq!(observed.stdout, stdout, "{streams:?}");
    assert_eq!(observed.stderr, stderr, "{streams:?}");
}

fn invoke(
    streams: Streams,
    directory: &Path,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Observed> {
    let executable = fs::canonicalize(executable::fastmash())?;
    let executable = CString::new(executable.as_os_str().as_bytes())?;
    fs::write(directory.join("stdin"), b"a\nb\n")?;
    for name in ["stdout", "stderr"] {
        drop(
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(directory.join(name))?,
        );
    }
    let wrong = match streams {
        Streams::WrongAccess(mask) => mask,
        _ => 0,
    };
    let files = [
        if wrong & 1 != 0 {
            File::options().write(true).open("/dev/null")?
        } else {
            File::open(directory.join("stdin"))?
        },
        if wrong & 2 != 0 {
            File::open("/dev/null")?
        } else {
            File::options().write(true).open(directory.join("stdout"))?
        },
        if wrong & 4 != 0 {
            File::open("/dev/null")?
        } else {
            File::options().write(true).open(directory.join("stderr"))?
        },
    ];
    let parent_binding_fds = files.each_ref().map(|file| file.as_raw_fd());
    let parent_binding_flags = [
        fcntl(&files[0], FcntlArg::F_GETFL)?,
        fcntl(&files[1], FcntlArg::F_GETFL)?,
        fcntl(&files[2], FcntlArg::F_GETFL)?,
    ];
    let access = parent_binding_flags.map(|flags| flags & OFlag::O_ACCMODE.bits());
    let expected_access = [
        if wrong & 1 != 0 {
            OFlag::O_WRONLY
        } else {
            OFlag::O_RDONLY
        },
        if wrong & 2 != 0 {
            OFlag::O_RDONLY
        } else {
            OFlag::O_WRONLY
        },
        if wrong & 4 != 0 {
            OFlag::O_RDONLY
        } else {
            OFlag::O_WRONLY
        },
    ]
    .map(|flags| flags.bits());
    if access != expected_access {
        return Err(format!("incorrect parent descriptor access: {parent_binding_flags:?}").into());
    }
    let mut actions = PosixSpawnFileActions::init()?;
    for (target, source) in [0, 1, 2].into_iter().zip(parent_binding_fds) {
        if source < 3 {
            return Err("parent binding unexpectedly occupies a standard descriptor".into());
        }
        actions.add_dup2(source, target)?;
    }
    if let Streams::Closed(mask) = streams {
        for target in 0..3 {
            if mask & (1 << target) != 0 {
                actions.add_close(target)?;
            }
        }
    }
    let attributes = PosixSpawnAttr::init()?;
    let deadline = Instant::now() + CHILD_LIMIT;
    let cancel_at = deadline - CLEANUP_RESERVE;
    let spawned = posix_spawn(
        executable.as_c_str(),
        &actions,
        &attributes,
        &[c"fastmash", c"count", c"1"],
        &[c"LC_ALL=C", c"PATH=/usr/bin:/bin"],
    );
    drop(actions);
    drop(attributes);
    // Keep every source descriptor owned through spawn, then consume it once.
    // A close error is recorded without retrying an uncertain descriptor number.
    let parent_close_results = files.map(close);
    let pid = spawned?;
    let mut timed_out = false;
    let mut cancellation = None;
    let status = loop {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status @ (WaitStatus::Exited(..) | WaitStatus::Signaled(..))) => break status,
            Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => {}
            result => {
                // An ownership error does not authorize signalling the old PID.
                return Err(
                    format!("unexpected wait result: {result:?}; cleanup unconfirmed").into(),
                );
            }
        }
        if cancellation.is_none() && Instant::now() >= cancel_at {
            timed_out = true;
            cancellation = Some(kill(pid, Signal::SIGKILL));
        }
        if Instant::now() >= deadline {
            return Err(
                format!("child was not reaped within five seconds: {cancellation:?}").into(),
            );
        }
        thread::sleep(Duration::from_millis(2));
    };
    timed_out |= Instant::now() >= deadline;
    Ok(Observed {
        parent_binding_fds,
        parent_binding_flags,
        parent_close_results,
        status,
        timed_out,
        cancellation,
        stdout_length: fs::metadata(directory.join("stdout"))?.len(),
        stderr_length: fs::metadata(directory.join("stderr"))?.len(),
        stdout: read_prefix(&directory.join("stdout"), stdout_limit)?,
        stderr: read_prefix(&directory.join("stderr"), stderr_limit)?,
    })
}

fn read_prefix(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}
