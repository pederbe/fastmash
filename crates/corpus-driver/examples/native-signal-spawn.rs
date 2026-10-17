#![forbid(unsafe_code)]

//! Exercise selected native signal contracts through safe spawn attributes.

use fastmash_corpus_driver::fixture::{digest, encode};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, fcntl};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, PosixSpawnFlags, posix_spawn};
use nix::sys::signal::{SigSet, Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{close, pipe};
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
const RUN_LIMIT: Duration = Duration::from_secs(45);
const CLEANUP_RESERVE: Duration = Duration::from_secs(1);

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    blocked: Option<Signal>,
    readerless: bool,
}

const CASES: [Case; 6] = [
    Case {
        name: "ordinary",
        blocked: None,
        readerless: false,
    },
    Case {
        name: "blocked-sigpipe",
        blocked: Some(Signal::SIGPIPE),
        readerless: false,
    },
    Case {
        name: "blocked-sigint",
        blocked: Some(Signal::SIGINT),
        readerless: false,
    },
    Case {
        name: "blocked-sigterm",
        blocked: Some(Signal::SIGTERM),
        readerless: false,
    },
    Case {
        name: "blocked-sighup",
        blocked: Some(Signal::SIGHUP),
        readerless: false,
    },
    Case {
        name: "readerless-stdout",
        blocked: None,
        readerless: true,
    },
];

struct Expected {
    exit_code: Option<i32>,
    signal: Option<i32>,
    stdout: Option<&'static [u8]>,
    stderr: &'static [u8],
}

impl Case {
    fn expected(self) -> Expected {
        // Independent of candidate output, from the native_streams.rs tests.
        if self.readerless {
            Expected {
                exit_code: None,
                signal: Some(Signal::SIGPIPE as i32),
                stdout: None,
                stderr: b"",
            }
        } else if self.blocked == Some(Signal::SIGPIPE) {
            Expected {
                exit_code: Some(77),
                signal: None,
                stdout: Some(b""),
                stderr: b"fastmash: blocked SIGPIPE is unsupported\n",
            }
        } else {
            Expected {
                exit_code: Some(0),
                signal: None,
                stdout: Some(b"2\n"),
                stderr: b"",
            }
        }
    }

    fn declaration(self) -> serde_json::Value {
        let expected = self.expected();
        json!({
            "name": self.name,
            "blocked_signal": self.blocked.map(|signal| format!("{signal:?}")),
            "blocked_signal_number": self.blocked.map(|signal| signal as i32),
            "readerless_stdout": self.readerless,
            "initial_sigpipe_default": self.readerless,
            "expected_exit_code": expected.exit_code,
            "expected_signal": expected.signal,
            "expected_stdout_hex": expected.stdout.map(encode),
            "expected_stderr_hex": encode(expected.stderr),
        })
    }
}

#[derive(Default, Serialize)]
struct Observed {
    setup_completed: bool,
    parent_binding_fds: Option<[i32; 3]>,
    parent_binding_flags: Option<[i32; 3]>,
    pipe_reader_fd: Option<i32>,
    pipe_reader_closed: bool,
    pipe_reader_close_error: Option<String>,
    attribute_flags: Option<i32>,
    attribute_mask: Vec<i32>,
    attribute_default_signals: Option<Vec<i32>>,
    pid: Option<i32>,
    wait_status: Option<String>,
    exit_code: Option<i32>,
    terminating_signal: Option<i32>,
    timed_out: bool,
    reaped: bool,
    cancellation: Vec<String>,
    parent_close_results: Vec<Option<String>>,
}

fn main() {
    let status = match run() {
        Ok(passed) => i32::from(!passed),
        Err(error) => {
            eprintln!("native-signal-spawn: {error}");
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
    write_json(
        &directory.join("plan.json"),
        &json!({
            "executable": executable.to_string_lossy(),
            "executable_path_hex": encode(executable.as_os_str().as_bytes()),
            "executable_sha256": executable_hash,
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "argv": ["fastmash", "count", "1"],
            "environment": {"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
            "input_hex": encode(b"a\nb\n"),
            "child_limit_seconds": 5, "cleanup_reserve_seconds": 1,
            "aggregate_limit_seconds": 45,
            "cases": CASES.map(Case::declaration),
        }),
    )?;

    let mut attempted = 0;
    let mut passed = 0;
    let mut stop_reason = None;
    for (index, case) in CASES.into_iter().enumerate() {
        if started.elapsed() >= RUN_LIMIT.saturating_sub(CHILD_LIMIT) {
            stop_reason = Some("insufficient aggregate time for another bounded child");
            break;
        }
        let row = directory.join(format!("{index:02}-{}", case.name));
        DirBuilder::new().mode(0o700).create(&row)?;
        let expected = case.expected();
        fs::write(row.join("stdin"), b"a\nb\n")?;
        fs::write(row.join("expected.stderr"), expected.stderr)?;
        if let Some(stdout) = expected.stdout {
            fs::write(row.join("expected.stdout"), stdout)?;
            create_capture(&row.join("stdout"))?;
        }
        create_capture(&row.join("stderr"))?;
        write_json(&row.join("declared.json"), &case.declaration())?;
        let mut observed = Observed::default();
        let attempt = invoke(case, &native_path, &row, started + RUN_LIMIT, &mut observed);
        attempted += 1;
        let stdout = if let Some(expected_out) = expected.stdout {
            Some(read_prefix(&row.join("stdout"), expected_out.len() + 1)?)
        } else {
            None
        };
        let stdout_length = if expected.stdout.is_some() {
            Some(fs::metadata(row.join("stdout"))?.len())
        } else {
            None
        };
        let stderr = read_prefix(&row.join("stderr"), expected.stderr.len() + 1)?;
        let stderr_length = fs::metadata(row.join("stderr"))?.len();
        let cleanup_confirmed = observed.reaped
            && observed.parent_close_results.len() == 3
            && observed.parent_close_results.iter().all(Option::is_none)
            && (!case.readerless || observed.pipe_reader_closed);
        let stdout_matches = match expected.stdout {
            Some(bytes) => {
                stdout.as_deref() == Some(bytes) && stdout_length == Some(bytes.len() as u64)
            }
            None => true, // No stdout receiver exists in the readerless case.
        };
        let matched = attempt.is_ok()
            && cleanup_confirmed
            && !observed.timed_out
            && observed.cancellation.is_empty()
            && observed.exit_code == expected.exit_code
            && observed.terminating_signal == expected.signal
            && stdout_matches
            && stderr_length == expected.stderr.len() as u64
            && stderr == expected.stderr;
        write_json(
            &row.join("observed.json"),
            &json!({
                "observed": observed,
                "setup_or_wait_error": attempt.as_ref().err().map(ToString::to_string),
                "parent_binding_scope_released": true,
                "cleanup_confirmed": cleanup_confirmed,
                "stdout_capture_available": !case.readerless,
                "stdout_length": stdout_length, "stderr_length": stderr_length,
                "stdout_prefix_hex": stdout.as_deref().map(encode),
                "stderr_prefix_hex": encode(&stderr), "matched": matched,
            }),
        )?;
        if matched {
            passed += 1;
        }
        if attempt.is_err() || !cleanup_confirmed || observed.timed_out {
            stop_reason = Some("setup, wait, deadline or cleanup failed");
            break;
        }
    }
    let aggregate_deadline_exceeded = started.elapsed() >= RUN_LIMIT;
    let success = attempted == CASES.len() && passed == CASES.len() && !aggregate_deadline_exceeded;
    write_json(
        &directory.join("summary.json"),
        &json!({
            "declared": CASES.len(), "attempted": attempted, "passed": passed,
            "unattempted": CASES[attempted..].iter().map(|case| case.name).collect::<Vec<_>>(),
            "stop_reason": stop_reason, "aggregate_deadline_exceeded": aggregate_deadline_exceeded,
            "passed_all": success, "elapsed_seconds": started.elapsed().as_secs_f64(),
        }),
    )?;
    println!(
        "native-signal-spawn: {passed}/{attempted} passed, {} declared",
        CASES.len()
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
    let input = File::open(row.join("stdin"))?;
    let stdout = if case.readerless {
        let (reader, writer) = pipe()?;
        observed.pipe_reader_fd = Some(reader.as_raw_fd());
        let closed = close(reader);
        observed.pipe_reader_closed = closed.is_ok();
        observed.pipe_reader_close_error = closed.as_ref().err().map(ToString::to_string);
        closed?;
        File::from(writer)
    } else {
        File::options().write(true).open(row.join("stdout"))?
    };
    let files = [
        input,
        stdout,
        File::options().write(true).open(row.join("stderr"))?,
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
    // Plain pipe() has no portable close-on-exec flag. After duplication,
    // close all child source aliases, including the pipe writer's original fd.
    for file in &files {
        actions.add_close(file.as_raw_fd())?;
    }
    let mut mask = SigSet::empty();
    if let Some(signal) = case.blocked {
        mask.add(signal);
    }
    let mut attributes = PosixSpawnAttr::init()?;
    attributes.set_sigmask(&mask)?;
    let mut flags = PosixSpawnFlags::POSIX_SPAWN_SETSIGMASK;
    if case.readerless {
        let mut defaults = SigSet::empty();
        defaults.add(Signal::SIGPIPE);
        attributes.set_sigdefault(&defaults)?;
        flags |= PosixSpawnFlags::POSIX_SPAWN_SETSIGDEF;
        observed.attribute_default_signals = Some(
            attributes
                .sigdefault()?
                .iter()
                .map(|signal| signal as i32)
                .collect(),
        );
    }
    attributes.set_flags(flags)?;
    observed.attribute_flags = Some(attributes.flags()?.bits());
    observed.attribute_mask = attributes
        .sigmask()?
        .iter()
        .map(|signal| signal as i32)
        .collect();
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
    // Retain each owner, including the pipe writer, until spawn has returned.
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
            Ok(status @ WaitStatus::Signaled(_, signal, _)) => {
                observed.wait_status = Some(format!("{status:?}"));
                observed.terminating_signal = Some(signal as i32);
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
                ).into());
            }
        }
        // Terminal waits and ownership errors never reach further PID operations.
        if !cancelled && Instant::now() >= cancel_at {
            observed.timed_out = true;
            observed
                .cancellation
                .push(match kill(pid, Signal::SIGKILL) {
                    Ok(()) => "SIGKILL child: sent".into(),
                    Err(error) => format!("SIGKILL child: {error}"),
                });
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

fn create_capture(path: &Path) -> io::Result<()> {
    drop(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)?,
    );
    Ok(())
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
