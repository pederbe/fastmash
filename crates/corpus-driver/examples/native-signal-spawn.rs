#![forbid(unsafe_code)]

//! Exercise selected native signal contracts through safe spawn attributes.

use fastmash_corpus_driver::fixture::{digest, encode};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, fcntl};
use nix::spawn::{PosixSpawnAttr, PosixSpawnFileActions, PosixSpawnFlags, posix_spawn};
use nix::sys::signal::{SigSet, Signal, kill, raise};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{close, execve, pipe};
use serde::Serialize;
use serde_json::json;
use std::error::Error;
use std::ffi::{CString, OsStr};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const CHILD_LIMIT: Duration = Duration::from_secs(5);
const RUN_LIMIT: Duration = Duration::from_secs(60);
const CLEANUP_RESERVE: Duration = Duration::from_secs(1);
const HELPER_MODE: &str = "__inherited_ignored_sigpipe";
const HELPER_FAILURE: i32 = 125;
const EXEC_FAILURE: i32 = 126;

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum Launch {
    Candidate,
    InheritedIgnore,
    InvalidExecControl,
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    blocked: Option<Signal>,
    readerless: bool,
    launch: Launch,
}

const CASES: [Case; 9] = [
    Case {
        name: "ordinary",
        blocked: None,
        readerless: false,
        launch: Launch::Candidate,
    },
    Case {
        name: "blocked-sigpipe",
        blocked: Some(Signal::SIGPIPE),
        readerless: false,
        launch: Launch::Candidate,
    },
    Case {
        name: "blocked-sigint",
        blocked: Some(Signal::SIGINT),
        readerless: false,
        launch: Launch::Candidate,
    },
    Case {
        name: "blocked-sigterm",
        blocked: Some(Signal::SIGTERM),
        readerless: false,
        launch: Launch::Candidate,
    },
    Case {
        name: "blocked-sighup",
        blocked: Some(Signal::SIGHUP),
        readerless: false,
        launch: Launch::Candidate,
    },
    Case {
        name: "readerless-stdout",
        blocked: None,
        readerless: true,
        launch: Launch::Candidate,
    },
    Case {
        name: "inherited-ignore-ordinary",
        blocked: None,
        readerless: false,
        launch: Launch::InheritedIgnore,
    },
    Case {
        name: "inherited-ignore-readerless",
        blocked: None,
        readerless: true,
        launch: Launch::InheritedIgnore,
    },
    Case {
        name: "inherited-ignore-invalid-exec",
        blocked: None,
        readerless: false,
        launch: Launch::InvalidExecControl,
    },
];

struct Expected {
    exit_code: Option<i32>,
    signal: Option<i32>,
    stdout: Option<&'static [u8]>,
    stderr: &'static [u8],
}

impl Case {
    fn input(self) -> &'static [u8] {
        if self.launch == Launch::Candidate {
            b"a\nb\n"
        } else {
            b"a\n"
        }
    }

    fn expected(self) -> Expected {
        // Independent of candidate output, from the native_streams.rs tests.
        if self.launch == Launch::InvalidExecControl {
            Expected {
                exit_code: Some(EXEC_FAILURE),
                signal: None,
                stdout: Some(b""),
                stderr: b"",
            }
        } else if self.readerless {
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
                stdout: Some(if self.launch == Launch::Candidate {
                    b"2\n"
                } else {
                    b"1\n"
                }),
                stderr: b"",
            }
        }
    }

    fn declaration(self) -> serde_json::Value {
        let expected = self.expected();
        json!({
            "name": self.name,
            "launch": self.launch,
            "candidate_acceptance_case": self.launch != Launch::InvalidExecControl,
            "input_hex": encode(self.input()),
            "payload_argv": ["fastmash", "count", "1"],
            "blocked_signal": self.blocked.map(|signal| format!("{signal:?}")),
            "blocked_signal_number": self.blocked.map(|signal| signal as i32),
            "readerless_stdout": self.readerless,
            "initial_sigpipe_default": self.readerless && self.launch == Launch::Candidate,
            "helper_exec_mask": if self.launch == Launch::Candidate { None } else { Some("SIGTERM only") },
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
    waited_pid: Option<i32>,
    wait_status: Option<String>,
    exit_code: Option<i32>,
    terminating_signal: Option<i32>,
    timed_out: bool,
    reaped: bool,
    cancellation: Vec<String>,
    parent_close_results: Vec<Option<String>>,
}

fn main() {
    if std::env::args_os().nth(1).as_deref() == Some(OsStr::new(HELPER_MODE)) {
        std::process::exit(helper_main());
    }
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
    let helper = fs::canonicalize(std::env::current_exe()?)?;
    let helper_hash = digest(&fs::read(&helper)?);
    let helper_path = CString::new(helper.as_os_str().as_bytes())?;
    DirBuilder::new().mode(0o700).create(&directory)?;
    write_json(
        &directory.join("plan.json"),
        &json!({
            "executable": executable.to_string_lossy(),
            "executable_path_hex": encode(executable.as_os_str().as_bytes()),
            "executable_sha256": executable_hash,
            "helper_executable": helper.to_string_lossy(),
            "helper_path_hex": encode(helper.as_os_str().as_bytes()),
            "helper_sha256": helper_hash,
            "helper_mode": HELPER_MODE,
            "helper_runtime_prerequisite": "ordinary Rust main, reviewed default SIGPIPE ignore, no caught handler or alternate broken-pipe compiler option",
            "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
            "argv": ["fastmash", "count", "1"],
            "environment": {"LC_ALL": "C", "PATH": "/usr/bin:/bin"},
            "direct_input_hex": encode(b"a\nb\n"),
            "helper_input_hex": encode(b"a\n"),
            "child_limit_seconds": 5, "cleanup_reserve_seconds": 1,
            "aggregate_limit_seconds": 60,
            "cases": CASES.map(Case::declaration),
        }),
    )?;

    let mut attempted = 0;
    let mut passed = 0;
    let mut candidate_passed = 0;
    let mut exec_control_passed = 0;
    let mut stop_reason = None;
    for (index, case) in CASES.into_iter().enumerate() {
        if started.elapsed() >= RUN_LIMIT.saturating_sub(CHILD_LIMIT) {
            stop_reason = Some("insufficient aggregate time for another bounded child");
            break;
        }
        let row = directory.join(format!("{index:02}-{}", case.name));
        DirBuilder::new().mode(0o700).create(&row)?;
        let row = fs::canonicalize(row)?;
        let target = if case.launch == Launch::InvalidExecControl {
            let missing = row.join("nonexistent-exec-target");
            if missing.try_exists()? {
                return Err("invalid-exec target unexpectedly exists".into());
            }
            CString::new(missing.as_os_str().as_bytes())?
        } else {
            native_path.clone()
        };
        let expected = case.expected();
        fs::write(row.join("stdin"), case.input())?;
        fs::write(row.join("expected.stderr"), expected.stderr)?;
        if let Some(stdout) = expected.stdout {
            fs::write(row.join("expected.stdout"), stdout)?;
            create_capture(&row.join("stdout"))?;
        }
        create_capture(&row.join("stderr"))?;
        write_json(
            &row.join("declared.json"),
            &json!({
                "case": case.declaration(), "target_path_hex": encode(target.as_bytes()),
                "helper_path_hex": if case.launch == Launch::Candidate { None } else { Some(encode(helper_path.as_bytes())) },
                "helper_evidence_directory_hex": if case.launch == Launch::Candidate { None } else { Some(encode(row.as_os_str().as_bytes())) },
            }),
        )?;
        let mut observed = Observed::default();
        let attempt = invoke(
            case,
            &target,
            &helper_path,
            &row,
            started + RUN_LIMIT,
            &mut observed,
        );
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
        let (helper_evidence, helper_verified) = if case.launch == Launch::Candidate {
            (serde_json::Value::Null, true)
        } else {
            match verify_helper(case, &row, &target, &observed) {
                Ok(result) => result,
                Err(error) => (json!({"verification_error": error.to_string()}), false),
            }
        };
        let matched = attempt.is_ok()
            && helper_verified
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
                "helper_evidence": helper_evidence,
                "candidate_accepted": matched && case.launch != Launch::InvalidExecControl,
                "expected_exec_failure_control": case.launch == Launch::InvalidExecControl,
            }),
        )?;
        if matched {
            passed += 1;
            if case.launch == Launch::InvalidExecControl {
                exec_control_passed += 1;
            } else {
                candidate_passed += 1;
            }
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
            "candidate_declared": 8, "candidate_passed": candidate_passed,
            "exec_failure_control_declared": 1, "exec_failure_control_passed": exec_control_passed,
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
    helper: &CString,
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
    if case.readerless && case.launch == Launch::Candidate {
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
    let evidence_path = CString::new(row.as_os_str().as_bytes())?;
    let helper_mode = CString::new(HELPER_MODE)?;
    let arguments = if case.launch == Launch::Candidate {
        vec![c"fastmash", c"count", c"1"]
    } else {
        vec![
            c"native-signal-spawn",
            helper_mode.as_c_str(),
            executable.as_c_str(),
            evidence_path.as_c_str(),
        ]
    };
    let spawned = posix_spawn(
        if case.launch == Launch::Candidate {
            executable.as_c_str()
        } else {
            helper.as_c_str()
        },
        &actions,
        &attributes,
        &arguments,
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
            Ok(status @ WaitStatus::Exited(waited, code)) => {
                observed.wait_status = Some(format!("{status:?}"));
                observed.waited_pid = Some(waited.as_raw());
                observed.exit_code = Some(code);
                observed.reaped = true;
                break;
            }
            Ok(status @ WaitStatus::Signaled(waited, signal, _)) => {
                observed.wait_status = Some(format!("{status:?}"));
                observed.waited_pid = Some(waited.as_raw());
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

fn helper_main() -> i32 {
    let mut args = std::env::args_os().skip(2);
    let Some(target) = args.next() else {
        return HELPER_FAILURE;
    };
    let Some(directory) = args.next() else {
        return HELPER_FAILURE;
    };
    if args.next().is_some() {
        return HELPER_FAILURE;
    }
    let target = PathBuf::from(target);
    let directory = PathBuf::from(directory);
    match helper_exec(&target, &directory) {
        Ok(status) => status,
        Err(error) => {
            let _ = write_private_json(
                &directory.join("helper-error.json"),
                &json!({
                    "pid": std::process::id(), "error": error.to_string(),
                }),
            );
            HELPER_FAILURE
        }
    }
}

fn helper_exec(target: &Path, directory: &Path) -> Result<i32> {
    // This fresh ordinary Rust main has no caught SIGPIPE handler. Its pinned
    // default runtime supplies ignore. Survival alone is not the calibration.
    let target = CString::new(target.as_os_str().as_bytes())?;
    let pid = std::process::id();
    let empty = SigSet::empty();
    empty.thread_set_mask()?;
    let before_raise = mask_readback(&empty, 0)?;
    raise(Signal::SIGPIPE)?;
    write_private_json(
        &directory.join("calibration.json"),
        &json!({
            "pid": pid, "target_path_hex": encode(target.as_bytes()),
            "mask_replacement_succeeded": true, "mask_readback": before_raise,
            "raise_sigpipe_returned": true,
        }),
    )?;

    let mut term = SigSet::empty();
    term.add(Signal::SIGTERM);
    term.thread_set_mask()?;
    let before_exec = mask_readback(&term, 1u64 << (Signal::SIGTERM as u32 - 1))?;
    write_private_json(
        &directory.join("exec-ready.json"),
        &json!({
            "pid": pid, "target_path_hex": encode(target.as_bytes()),
            "mask_replacement_succeeded": true, "mask_readback": before_exec,
        }),
    )?;
    // A successful direct exec cannot return and preserves this process ID.
    // The error arm neither raises a signal nor fabricates a candidate result.
    let error = match execve(
        target.as_c_str(),
        &[c"fastmash", c"count", c"1"],
        &[c"LC_ALL=C", c"PATH=/usr/bin:/bin"],
    ) {
        Ok(never) => match never {},
        Err(error) => error,
    };
    write_private_json(
        &directory.join("exec-error.json"),
        &json!({
            "pid": pid, "target_path_hex": encode(target.as_bytes()),
            "stage": "execve", "errno": error as i32, "error": error.to_string(),
        }),
    )?;
    Ok(EXEC_FAILURE)
}

fn mask_readback(expected: &SigSet, expected_bits: u64) -> Result<serde_json::Value> {
    let actual = SigSet::thread_get_mask()?;
    if actual != *expected || actual.contains(Signal::SIGPIPE) {
        return Err(
            "helper signal mask did not match the requested unblocked-SIGPIPE state".into(),
        );
    }
    // Nix equality omits Linux real-time signals. Reuse the corpus driver's
    // native thread-status witness rather than treating that equality as exact.
    #[cfg(target_os = "linux")]
    let linux_sigblk = {
        let status = fs::read_to_string("/proc/thread-self/status")?;
        let value = status
            .lines()
            .find_map(|line| line.strip_prefix("SigBlk:"))
            .ok_or("kernel thread status has no SigBlk signal mask")?
            .trim();
        if parse_sigblk(value)? != expected_bits {
            return Err("helper Linux signal mask differs from the exact requested mask".into());
        }
        Some(value.to_owned())
    };
    #[cfg(target_os = "macos")]
    let linux_sigblk: Option<String> = {
        let _ = expected_bits;
        None
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err("helper signal calibration is unsupported on this platform".into());
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Ok(json!({
        "nix_signals": actual.iter().map(|signal| signal as i32).collect::<Vec<_>>(),
        "linux_sigblk": linux_sigblk,
    }))
}

#[cfg(target_os = "linux")]
fn parse_sigblk(value: &str) -> Result<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("kernel thread status has an invalid SigBlk signal mask".into());
    }
    Ok(u64::from_str_radix(value, 16)?)
}

fn verify_helper(
    case: Case,
    directory: &Path,
    target: &CString,
    observed: &Observed,
) -> Result<(serde_json::Value, bool)> {
    let calibration = read_optional_json(&directory.join("calibration.json"))?;
    let ready = read_optional_json(&directory.join("exec-ready.json"))?;
    let exec_error = read_optional_json(&directory.join("exec-error.json"))?;
    let helper_error = read_optional_json(&directory.join("helper-error.json"))?;
    let target_hex = encode(target.as_bytes());
    let identity_matches = |record: &serde_json::Value| {
        observed.pid.is_some()
            && observed.pid == observed.waited_pid
            && record["pid"].as_u64() == observed.pid.map(|pid| pid as u64)
            && record["target_path_hex"].as_str() == Some(target_hex.as_str())
    };
    let calibrated = calibration.as_ref().is_some_and(|record| {
        identity_matches(record)
            && record["mask_replacement_succeeded"] == true
            && record["raise_sigpipe_returned"] == true
            && mask_record_matches(&record["mask_readback"], false)
    });
    let exec_ready = ready.as_ref().is_some_and(|record| {
        identity_matches(record)
            && record["mask_replacement_succeeded"] == true
            && mask_record_matches(&record["mask_readback"], true)
    });
    let exec_outcome = if case.launch == Launch::InvalidExecControl {
        exec_error.as_ref().is_some_and(|record| {
            identity_matches(record)
                && record["stage"] == "execve"
                && record["errno"].as_i64() == Some(Errno::ENOENT as i64)
        })
    } else {
        exec_error.is_none()
    };
    let verified = calibrated && exec_ready && exec_outcome && helper_error.is_none();
    Ok((
        json!({
            "calibration": calibration, "exec_ready": ready, "exec_error": exec_error,
            "helper_error": helper_error, "verified": verified,
        }),
        verified,
    ))
}

fn mask_record_matches(record: &serde_json::Value, term: bool) -> bool {
    let expected = if term {
        vec![Signal::SIGTERM as i32]
    } else {
        vec![]
    };
    if record["nix_signals"] != json!(expected) {
        return false;
    }
    #[cfg(target_os = "linux")]
    {
        record["linux_sigblk"]
            .as_str()
            .and_then(|value| parse_sigblk(value).ok())
            == Some(if term {
                1u64 << (Signal::SIGTERM as u32 - 1)
            } else {
                0
            })
    }
    #[cfg(target_os = "macos")]
    {
        record["linux_sigblk"].is_null()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    false
}

fn read_optional_json(path: &Path) -> Result<Option<serde_json::Value>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write_private_json(path: &Path, value: &serde_json::Value) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&serde_json::to_vec_pretty(value)?)?;
    file.flush()?;
    close(file)?;
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
