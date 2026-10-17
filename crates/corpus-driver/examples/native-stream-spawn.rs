#![forbid(unsafe_code)]

//! Compare safe spawn file actions with the established standard-stream matrix.

use fastmash_corpus_driver::fixture::{digest, encode};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, fcntl};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, posix_spawn};
use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, close};
use serde::Serialize;
use serde_json::json;
use std::error::Error;
use std::ffi::CString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const CHILD_LIMIT: Duration = Duration::from_secs(5);
const RUN_LIMIT: Duration = Duration::from_secs(90);
const CLEANUP_RESERVE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Serialize)]
struct Case {
    kind: &'static str,
    mask: u8,
}

impl Case {
    fn expected(self) -> (i32, &'static [u8], &'static [u8]) {
        // Frozen from native_streams.rs, independently of observed bytes.
        let status = i32::from(self.mask & 3 != 0);
        let stdout = if self.mask & 3 == 0 { &b"2\n"[..] } else { b"" };
        let stderr = if self.mask == 0 || self.mask & 4 != 0 {
            &b""[..]
        } else if self.mask & 1 != 0 {
            &b"fastmash: read error: Bad file descriptor\n"[..]
        } else {
            &b"fastmash: write error: Bad file descriptor\n"[..]
        };
        (status, stdout, stderr)
    }
}

#[derive(Default, Serialize)]
struct Observed {
    setup_completed: bool,
    parent_binding_fds: Option<[i32; 3]>,
    parent_binding_flags: Option<[i32; 3]>,
    pid: Option<i32>,
    wait_status: Option<String>,
    exit_code: Option<i32>,
    timed_out: bool,
    reaped: bool,
    cancellation: Vec<String>,
    parent_close_results: Vec<Option<String>>,
}

fn main() {
    let status = match run() {
        Ok(passed) => i32::from(!passed),
        Err(error) => {
            eprintln!("native-stream-spawn: {error}");
            2
        }
    };
    std::process::exit(status);
}

fn run() -> Result<bool> {
    let started = Instant::now();
    let mut args = std::env::args_os().skip(1);
    let executable = args
        .next()
        .ok_or("expected EXECUTABLE FRESH_EVIDENCE_DIR")?;
    let directory = PathBuf::from(args.next().ok_or("expected a fresh evidence directory")?);
    if args.next().is_some() {
        return Err("expected exactly EXECUTABLE FRESH_EVIDENCE_DIR".into());
    }
    let executable = fs::canonicalize(executable)?;
    let executable_hash = digest(&fs::read(&executable)?);
    let native_path = CString::new(executable.as_os_str().as_bytes())?;
    DirBuilder::new().mode(0o700).create(&directory)?;

    let mut cases = vec![Case {
        kind: "ordinary",
        mask: 0,
    }];
    for kind in ["closed", "wrong-access"] {
        cases.extend((1..8).map(|mask| Case { kind, mask }));
    }
    write_json(
        &directory.join("plan.json"),
        &json!({
            "executable": executable.to_string_lossy(),
            "executable_path_hex": encode(executable.as_os_str().as_bytes()),
            "executable_sha256": executable_hash,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "argv": ["fastmash", "count", "1"],
            "environment": {"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
            "input_hex": encode(b"a\nb\n"),
            "child_limit_seconds": 5,
            "cleanup_reserve_seconds": 1,
            "aggregate_limit_seconds": 90,
            "cases": cases,
        }),
    )?;

    let mut attempted = 0;
    let mut passed = 0;
    let mut stop_reason = None;
    for (index, case) in cases.iter().copied().enumerate() {
        if started.elapsed() >= RUN_LIMIT.saturating_sub(CHILD_LIMIT) {
            stop_reason = Some("insufficient aggregate time for another bounded child");
            break;
        }
        let row = directory.join(format!("{index:02}-{}-{}", case.kind, case.mask));
        DirBuilder::new().mode(0o700).create(&row)?;
        let (expected_status, expected_out, expected_err) = case.expected();
        fs::write(row.join("stdin"), b"a\nb\n")?;
        fs::write(row.join("expected.stdout"), expected_out)?;
        fs::write(row.join("expected.stderr"), expected_err)?;
        write_json(
            &row.join("declared.json"),
            &json!({
                "case": case,
                "expected_exit_code": expected_status,
                "closed_mask": if case.kind == "closed" { case.mask } else { 0 },
                "wrong_access_mask": if case.kind == "wrong-access" { case.mask } else { 0 },
                "bindings": ["stdin file or write-only /dev/null",
                    "stdout capture or read-only /dev/null", "stderr capture or read-only /dev/null"],
            }),
        )?;
        // Captures stay on disk even when the selected stream is closed or
        // installed on /dev/null and therefore cannot write into its capture.
        for name in ["stdout", "stderr"] {
            drop(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .open(row.join(name))?,
            );
        }
        let mut observed = Observed::default();
        let attempt = invoke(case, &native_path, &row, started + RUN_LIMIT, &mut observed);
        attempted += 1;
        let stdout = read_prefix(&row.join("stdout"), expected_out.len() + 1)?;
        let stderr = read_prefix(&row.join("stderr"), expected_err.len() + 1)?;
        let stdout_length = fs::metadata(row.join("stdout"))?.len();
        let stderr_length = fs::metadata(row.join("stderr"))?.len();
        let cleanup_confirmed = observed.reaped
            && observed.parent_close_results.len() == 3
            && observed.parent_close_results.iter().all(Option::is_none);
        let matched = attempt.is_ok()
            && cleanup_confirmed
            && !observed.timed_out
            && observed.cancellation.is_empty()
            && observed.exit_code == Some(expected_status)
            && stdout_length == expected_out.len() as u64
            && stderr_length == expected_err.len() as u64
            && stdout == expected_out
            && stderr == expected_err;
        write_json(
            &row.join("observed.json"),
            &json!({
                "observed": observed,
                "setup_or_wait_error": attempt.as_ref().err().map(ToString::to_string),
                "parent_binding_scope_released": true,
                "cleanup_confirmed": cleanup_confirmed,
                "stdout_length": stdout_length,
                "stderr_length": stderr_length,
                "stdout_prefix_hex": encode(&stdout),
                "stderr_prefix_hex": encode(&stderr),
                "matched": matched,
            }),
        )?;
        if matched {
            passed += 1;
        }
        // Byte/status mismatches retain the remaining matrix. Uncertain
        // process cleanup or setup failure stops further launches.
        if attempt.is_err() || !cleanup_confirmed || observed.timed_out {
            stop_reason = Some("setup, wait, deadline or cleanup failed");
            break;
        }
    }
    let aggregate_deadline_exceeded = started.elapsed() >= RUN_LIMIT;
    let success = attempted == cases.len() && passed == cases.len() && !aggregate_deadline_exceeded;
    write_json(
        &directory.join("summary.json"),
        &json!({
            "declared": cases.len(), "attempted": attempted, "passed": passed,
            "unattempted": &cases[attempted..], "stop_reason": stop_reason,
            "aggregate_deadline_exceeded": aggregate_deadline_exceeded,
            "passed_all": success, "elapsed_seconds": started.elapsed().as_secs_f64(),
        }),
    )?;
    println!(
        "native-stream-spawn: {passed}/{attempted} passed, {} declared",
        cases.len()
    );
    Ok(success)
}

fn invoke(
    case: Case,
    executable: &CString,
    row: &Path,
    aggregate_deadline: Instant,
    observed: &mut Observed,
) -> Result<()> {
    let wrong = if case.kind == "wrong-access" {
        case.mask
    } else {
        0
    };
    let files = [
        if wrong & 1 != 0 {
            File::options().write(true).open("/dev/null")?
        } else {
            File::open(row.join("stdin"))?
        },
        if wrong & 2 != 0 {
            File::open("/dev/null")?
        } else {
            File::options().write(true).open(row.join("stdout"))?
        },
        if wrong & 4 != 0 {
            File::open("/dev/null")?
        } else {
            File::options().write(true).open(row.join("stderr"))?
        },
    ];
    observed.parent_binding_fds = Some(files.each_ref().map(|file| file.as_raw_fd()));
    observed.parent_binding_flags = Some([
        fcntl(&files[0], FcntlArg::F_GETFL)?,
        fcntl(&files[1], FcntlArg::F_GETFL)?,
        fcntl(&files[2], FcntlArg::F_GETFL)?,
    ]);
    let mut actions = PosixSpawnFileActions::init()?;
    for (target, file) in [0, 1, 2].into_iter().zip(&files) {
        if file.as_raw_fd() < 3 {
            return Err("parent binding unexpectedly occupies a standard descriptor".into());
        }
        actions.add_dup2(file.as_raw_fd(), target)?;
    }
    if case.kind == "closed" {
        for target in 0..3 {
            if case.mask & (1 << target) != 0 {
                actions.add_close(target)?;
            }
        }
    }
    let attributes = PosixSpawnAttr::init()?;
    observed.setup_completed = true;
    if Instant::now() + CHILD_LIMIT > aggregate_deadline {
        return Err("insufficient aggregate time after descriptor setup".into());
    }
    let child_deadline = (Instant::now() + CHILD_LIMIT).min(aggregate_deadline);
    let cancel_at = child_deadline - CLEANUP_RESERVE;
    let spawned = posix_spawn(
        executable.as_c_str(),
        &actions,
        &attributes,
        &[c"fastmash", c"count", c"1"],
        &[c"LC_ALL=C", c"PATH=/usr/bin:/bin"],
    );
    drop(actions);
    drop(attributes);
    // Every raw source number above remains owned until spawn has returned.
    // Consume those handles once, without retrying an uncertain close.
    for file in files {
        observed
            .parent_close_results
            .push(close(file).err().map(|error| error.to_string()));
    }
    let pid = spawned?;
    observed.pid = Some(pid.as_raw());
    let mut cancelled = false;
    loop {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status @ WaitStatus::Exited(_, code)) => {
                observed.wait_status = Some(format!("{status:?}"));
                observed.exit_code = Some(code);
                observed.reaped = true;
                break;
            }
            Ok(status @ WaitStatus::Signaled(..)) => {
                observed.wait_status = Some(format!("{status:?}"));
                observed.reaped = true;
                break;
            }
            Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => {}
            Err(Errno::ECHILD) => {
                return Err(
                    "waitpid returned ECHILD; child ownership and cleanup are unconfirmed".into(),
                );
            }
            result => {
                return Err(format!(
                    "unexpected wait result: {result:?}; child ownership and cleanup are unconfirmed"
                )
                .into());
            }
        }
        // Only nonterminal waits reach cancellation. Terminal results and
        // ownership errors never signal or probe the old numeric identity.
        if !cancelled && Instant::now() >= cancel_at {
            observed.timed_out = true;
            cancel(pid, observed);
            cancelled = true;
        }
        if Instant::now() >= child_deadline {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    if !observed.reaped {
        return Err("child was not reaped within its five-second bound".into());
    }
    observed.timed_out |= Instant::now() >= child_deadline;
    Ok(())
}

fn cancel(pid: Pid, observed: &mut Observed) {
    observed
        .cancellation
        .push(match kill(pid, Signal::SIGKILL) {
            Ok(()) => "SIGKILL child: sent".into(),
            Err(error) => format!("SIGKILL child: {error}"),
        });
}

fn read_prefix(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}
