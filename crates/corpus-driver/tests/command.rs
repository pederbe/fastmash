#![forbid(unsafe_code)]

use fastmash_corpus_driver::fixture::{digest, encode};
use flate2::Compression;
use flate2::write::GzEncoder;
use serde_json::{Value, json};
use std::fs::{self, DirBuilder};
use std::io::Write;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

struct TestRun {
    root: PathBuf,
    binary: PathBuf,
    fixture: PathBuf,
}

impl TestRun {
    fn new(cases: &[Value]) -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fastmash-corpus-selftest-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let binary = root.join("command");
        fs::copy(env!("CARGO_BIN_EXE_fastmash-corpus-test-command"), &binary).unwrap();
        // Admission requires the sibling's identity on Linux. The ordinary
        // fixture command does not execute it, so no product binary is needed.
        fs::copy(&binary, root.join("fastmash-sort-supervisor")).unwrap();
        let fixture = root.join("cases.jsonl.gz");
        let result = Self {
            root,
            binary,
            fixture,
        };
        result.write_cases(cases);
        result
    }

    fn write_cases(&self, cases: &[Value]) {
        let mut content = Vec::new();
        for case in cases {
            serde_json::to_writer(&mut content, case).unwrap();
            content.push(b'\n');
        }
        let mut compressed = GzEncoder::new(Vec::new(), Compression::default());
        compressed.write_all(&content).unwrap();
        let compressed = compressed.finish().unwrap();
        fs::write(&self.fixture, &compressed).unwrap();
        fs::write(self.root.join("cases-provenance.json"), serde_json::to_vec(&json!({
            "cases":cases.len(), "fixture_sha256":digest(&compressed), "content_sha256":digest(&content),
        })).unwrap()).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-corpus"))
            .arg("--binary")
            .arg(&self.binary)
            .arg("--fixture")
            .arg(&self.fixture)
            .args(args)
            .output()
            .unwrap()
    }

    fn evidence(&self) -> PathBuf {
        self.root.join("evidence")
    }
}

impl Drop for TestRun {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!(
                "failed corpus selftest captures retained at {}",
                self.root.display()
            );
            return;
        }
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn case(id: &str, mode: &str, input: &[u8], stdout: &[u8], stderr: &[u8], status: i32) -> Value {
    json!({"id":id, "io":"normal", "locale":"C", "args":[mode],
        "input_hex":encode(input), "expected":{"stdout_hex":encode(stdout),
        "stderr_hex":encode(stderr), "returncode":status}})
}

fn json_file(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn real_incorrect_stdout_stderr_and_status_fail_and_keep_going_retains_each() {
    let run = TestRun::new(&[
        case("wrong-output", "wrong-output", b"", b"", b"", 0),
        case("wrong-error", "wrong-error", b"", b"", b"", 0),
        case("wrong-status", "wrong-status", b"", b"", b"", 0),
        case(
            "last-passes",
            "echo",
            b"exact\0\xff\n",
            b"exact\0\xff\n",
            b"",
            0,
        ),
    ]);
    let first = run.run(&[]);
    assert_eq!(first.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&first.stdout).contains("stopped after 1"));
    let evidence = run.evidence();
    let all = run.run(&["--keep-going", "--evidence-dir", evidence.to_str().unwrap()]);
    assert_eq!(all.status.code(), Some(1));
    let result = json_file(&evidence.join("result.json"));
    assert_eq!(result["attempted"], 4);
    assert_eq!(result["passed"], 1);
    assert_eq!(
        result["failed"],
        json!(["wrong-output", "wrong-error", "wrong-status"])
    );
    let observations = fs::read_to_string(evidence.join("observations.jsonl")).unwrap();
    assert_eq!(observations.lines().count(), 3);
    assert!(observations.contains("696e636f72726563740a"));
    assert!(observations.contains("\"returncode\":17"));
}

#[test]
fn raw_arguments_environment_argv0_cwd_and_input_kind_reach_the_command() {
    for mode in ["pipe", "file"] {
        let mut facts = case("facts", "facts", b"", b"", b"", 0);
        facts.as_object_mut().unwrap().remove("args");
        facts["args_hex"] = json!([
            encode(b"facts"),
            encode(mode.as_bytes()),
            encode(b"raw\xff")
        ]);
        facts["posixly_correct"] = json!("");
        let run = TestRun::new(&[facts]);
        let args = if mode == "file" {
            vec!["--stdin-file"]
        } else {
            Vec::new()
        };
        let result = run.run(&args);
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

#[test]
fn early_input_closure_and_both_output_overflows_stay_bounded() {
    let input = vec![b'x'; 2 * 1024 * 1024];
    let run = TestRun::new(&[
        case("early", "early-close", &input, b"early\n", b"", 0),
        case("overflow", "overflow", b"", b"", b"", 0),
        case("overflow-error", "overflow-error", b"", b"", b"", 0),
        case("last", "echo", b"after\n", b"after\n", b"", 0),
    ]);
    let result = run.run(&["--keep-going"]);
    assert_eq!(result.status.code(), Some(1));
    let report = String::from_utf8_lossy(&result.stdout);
    assert!(report.contains("FAIL overflow:"));
    assert!(report.contains("FAIL overflow-error:"));
    assert!(report.contains("2 of 4 cases passed"));
    assert!(!report.contains("FAIL early:"));
}

#[test]
fn timeout_kills_the_whole_group_and_continue_after_failure_still_runs() {
    let run = TestRun::new(&[]);
    let pidfile = run.root.join("descendant.pid");
    let mut stalled = case("timeout", "descendant", b"", b"", b"", 0);
    stalled["args"] = json!(["descendant", pidfile]);
    stalled["hold_stdin"] = json!(true);
    run.write_cases(&[
        stalled,
        case("after", "echo", b"after\n", b"after\n", b"", 0),
    ]);
    let evidence = run.evidence();
    let started = Instant::now();
    let output = run.run(&["--keep-going", "--evidence-dir", evidence.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(started.elapsed() < Duration::from_secs(12));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 of 2 cases passed"));
    let observed: Value = serde_json::from_str(
        fs::read_to_string(evidence.join("observations.jsonl"))
            .unwrap()
            .lines()
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(observed["observed"]["timed_out"], true);
    assert_eq!(observed["observed"]["cleanup_confirmed"], true);
    assert_eq!(observed["observed"]["stdin_held_until_exit"], false);
    assert_stopped(&pidfile);
}

#[test]
fn a_successful_leader_cannot_leave_a_running_descendant() {
    let run = TestRun::new(&[]);
    let pidfile = run.root.join("orphan.pid");
    let mut orphan = case("orphan", "orphan", b"", b"", b"", 0);
    orphan["args"] = json!(["orphan", pidfile]);
    run.write_cases(&[orphan]);
    assert!(run.run(&[]).status.success());
    assert_stopped(&pidfile);
}

#[test]
fn an_exited_leader_cannot_leave_an_inherited_pipe_writer() {
    let run = TestRun::new(&[]);
    let pidfile = run.root.join("inherited-writer.pid");
    let mut orphan = case("orphan-writer", "orphan-output", b"input\n", b"", b"", 0);
    orphan["args"] = json!(["orphan-output", pidfile]);
    orphan["io"] = json!("pipe");
    orphan["observe_held"] = json!(true);
    orphan["expected"]["held"] = json!({"bytes":0,"alive":true,"stdout_hex":""});
    run.write_cases(&[orphan]);
    let evidence = run.evidence();
    let started = Instant::now();
    let output = run.run(&["--evidence-dir", evidence.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let result = json_file(&evidence.join("result.json"));
    assert_eq!(result["attempted"], 1);
    assert_eq!(result["passed"], 0);
    assert!(started.elapsed() < Duration::from_secs(5));
    let records = fs::read_to_string(evidence.join("observations.jsonl")).unwrap();
    let record: Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
    assert_eq!(record["observed"]["cleanup_confirmed"], true);
    assert_eq!(record["observed"]["timed_out"], false);
    assert_eq!(record["observed"]["overflow"], false);
    assert_eq!(record["observed"]["held"], Value::Null);
    assert_stopped(&pidfile);
}

fn assert_stopped(pidfile: &Path) {
    use nix::errno::Errno;
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    let pid = fs::read_to_string(pidfile).unwrap().parse::<i32>().unwrap();
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH) {
            return;
        }
        // A killed orphan can await its system reaper briefly on Linux. It has
        // no descriptors and cannot execute or keep the harness's pipe alive.
        #[cfg(target_os = "linux")]
        if fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|s| {
            s.rsplit_once(") ")
                .is_some_and(|(_, rest)| rest.starts_with('Z'))
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "descendant {pid} remained live");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn buffered_input_selects_eio_after_input_drain_without_output_acknowledgement() {
    let run = TestRun::new(&[]);
    let input = run.root.join("ordinary-input");
    fs::write(&input, b"input\n").unwrap();
    let ordinary = Command::new(&run.binary)
        .arg("buffered-input")
        .stdin(fs::File::open(&input).unwrap())
        .output()
        .unwrap();
    assert!(ordinary.status.success());
    assert_eq!(ordinary.stdout, b"after\n");
    assert!(ordinary.stderr.is_empty());

    let root = run.root.join("buffered");
    let output = fastmash_corpus_driver::late_input::buffered(
        &run.binary,
        &["buffered-input".into()],
        b"input\n",
        "C",
        Path::new(env!("CARGO_BIN_EXE_fastmash-corpus")),
        &root,
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(output.stdout, b"after\n");
    assert_eq!(output.stderr, b"actual late EIO confirmed\n");
    assert!(!root.join("acknowledged.json").exists());
    let queued = json_file(&root.join("input-queued.json"));
    let drained = json_file(&root.join("input-drained.json"));
    let stopped = json_file(&root.join("stopped.json"));
    let completed = json_file(&root.join("completed.json"));
    assert_eq!(queued["before_exec_release"], true);
    assert_eq!(queued["kernel_unread"], 6);
    assert_eq!(queued["exact_input_length"], 6);
    assert_eq!(queued["candidate_pid"], drained["candidate_pid"]);
    assert_eq!(drained["candidate_live"], true);
    assert_eq!(drained["exact_input_delivered"], true);
    assert_eq!(drained["kernel_unread"], 0);
    assert_eq!(drained["candidate_pid"], stopped["pid"]);
    assert_eq!(stopped["pid"], completed["pid"]);
    assert_eq!(stopped["kernel_unread"], 0);
    assert_eq!(stopped["acknowledgement_before_stop"], false);
    assert_eq!(completed["reaped"], true);
    assert_eq!(completed["terminal_reclaimed"], true);
    println!(
        "buffered input queued: {queued}; drain: {drained}; stop: {stopped}; completion: {completed}"
    );
}

#[test]
fn buffered_input_preserves_repeated_parse_errors_during_stop_selection() {
    let run = TestRun::new(&[]);
    for attempt in 0..1000 {
        let root = run.root.join(format!("parse-{attempt}"));
        let output = fastmash_corpus_driver::late_input::buffered(
            &run.binary,
            &["buffered-parse-error".into()],
            b"input\n",
            "C",
            Path::new(env!("CARGO_BIN_EXE_fastmash-corpus")),
            &root,
        )
        .unwrap_or_else(|error| panic!("parse attempt {attempt}: {error}"));
        assert_eq!(output.status.code(), Some(17));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"parse error before another read\n");
        assert_eq!(json_file(&root.join("completed.json"))["reaped"], true);
        let diagnostic = fs::read_to_string(root.join("coordinator.stderr")).unwrap();
        if diagnostic.contains("[DEBUG-buffered-stop]") {
            println!("parse attempt {attempt}: {diagnostic}");
        }
    }
}

#[test]
fn buffered_input_returns_early_errors_and_reaps_stalled_descendants() {
    let run = TestRun::new(&[]);
    let launcher = Path::new(env!("CARGO_BIN_EXE_fastmash-corpus"));
    let early = run.root.join("early");
    let output = fastmash_corpus_driver::late_input::buffered(
        &run.binary,
        &["wrong-status".into()],
        b"input\n",
        "C",
        launcher,
        &early,
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert!(!early.join("input-drained.json").exists());
    assert!(!early.join("stopped.json").exists());
    let completion = json_file(&early.join("completed.json"));
    assert_eq!(completion["reaped"], true);
    assert_eq!(completion["terminal_reclaimed"], false);
    println!("buffered early completion: {completion}");
    let parse = run.root.join("parse-error");
    let output = fastmash_corpus_driver::late_input::buffered(
        &run.binary,
        &["buffered-parse-error".into()],
        b"input\n",
        "C",
        launcher,
        &parse,
    )
    .unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert!(output.stdout.is_empty());
    assert_eq!(output.stderr, b"parse error before another read\n");
    let completion = json_file(&parse.join("completed.json"));
    assert_eq!(completion["reaped"], true);
    // The command may reject its first record before or after the drain
    // observation. Preserve its real error without requiring it to read again.
    assert_eq!(
        completion["terminal_reclaimed"],
        parse.join("stopped.json").exists()
    );
    let pidfile = run.root.join("buffered-descendant.pid");
    let stalled = run.root.join("stalled");
    let started = Instant::now();
    assert!(
        fastmash_corpus_driver::late_input::buffered(
            &run.binary,
            &[
                "buffered-input-stall".into(),
                pidfile.clone().into_os_string()
            ],
            b"input\n",
            "C",
            launcher,
            &stalled,
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(12));
    assert_stopped(&pidfile);
    let pid = json_file(&stalled.join("started.json"))["candidate_pid"]
        .as_u64()
        .unwrap();
    let cleanup = fs::read_to_string(stalled.join("candidate-cleanup.txt")).unwrap();
    assert!(cleanup.contains("reaped=true") && cleanup.contains(&format!("pid={pid} ")));
    assert!(matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    ));
}

#[test]
fn late_input_requires_real_acknowledgement_eio_and_owned_cleanup() {
    let mut fault = case(
        "late-input",
        "late-input",
        b"input\n",
        b"before\nafter\n",
        b"actual late EIO confirmed\n",
        1,
    );
    fault["io"] = json!("read-error");
    fault["ack_hex"] = json!(encode(b"before\n"));
    let run = TestRun::new(&[fault.clone()]);
    let evidence = run.evidence();
    let output = run.run(&["--evidence-dir", evidence.to_str().unwrap()]);
    assert!(output.status.success(), "{output:?}");
    let records = fs::read_to_string(evidence.join("observations.jsonl")).unwrap();
    let record: Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
    assert_eq!(record["observed"]["acknowledged"], encode(b"before\n"));
    assert_eq!(
        record["observed"]["input_error_mechanism"],
        "controlling-terminal-stop-resume-EIO"
    );
    assert_eq!(record["observed"]["cleanup_confirmed"], true);
    let root = evidence.join("late-input").join(digest(b"late-input"));
    assert_eq!(
        json_file(&root.join("acknowledged.json"))["candidate_live"],
        true
    );
    assert_eq!(
        json_file(&root.join("started.json"))["destination"]["isatty"],
        true
    );
    assert_eq!(
        json_file(&root.join("exec-mask.json"))["sighup_blocked"],
        false
    );
    assert_eq!(
        json_file(&root.join("started.json"))["coordinator_sighup_blocked"],
        true
    );
    assert_eq!(json_file(&root.join("stopped.json"))["signal"], "SIGSTOP");
    assert_eq!(
        json_file(&root.join("ordinary-control.json"))["matched"],
        true
    );
    assert!(run.run(&["--stdin-file"]).status.success());
    let mut wrong_ack = fault.clone();
    wrong_ack["ack_hex"] = json!(encode(b"unseen\n"));
    let mismatch = TestRun::new(&[wrong_ack]);
    let evidence = mismatch.evidence();
    assert_eq!(
        mismatch
            .run(&["--evidence-dir", evidence.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    let root = evidence.join("late-input").join(digest(b"late-input"));
    assert_eq!(
        json_file(&root.join("ordinary-control.json"))["matched"],
        true
    );
    assert!(!root.join("stopped.json").exists());
    let orphan = TestRun::new(&[]);
    let pidfile = orphan.root.join("early-writer.pid");
    let mut early = case("late-orphan", "orphan-output", b"input\n", b"", b"", 1);
    early["args"] = json!(["orphan-output", pidfile]);
    early["io"] = json!("read-error");
    early["ack_hex"] = json!(encode(b"unseen\n"));
    orphan.write_cases(&[early]);
    assert_eq!(orphan.run(&[]).status.code(), Some(2));
    assert_stopped(&pidfile);
    // Matching final bytes cannot replace a live acknowledgement.
    fault["args"] = json!(["early-close"]);
    fault["expected"]["stdout_hex"] = json!(encode(b"early\n"));
    fault["ack_hex"] = json!(encode(b"early\n"));
    let invalid = TestRun::new(&[fault]);
    let evidence = invalid.evidence();
    assert_eq!(
        invalid
            .run(&["--evidence-dir", evidence.to_str().unwrap()])
            .status
            .code(),
        Some(2)
    );
    let root = evidence.join("late-input").join(digest(b"late-input"));
    assert_eq!(
        json_file(&root.join("ordinary-control.json"))["matched"],
        true
    );
    assert!(!root.join("stopped.json").exists());
}

#[test]
fn late_input_timeout_reaps_owned_processes_and_stops_inherited_writer() {
    let run = TestRun::new(&[]);
    let pidfile = run.root.join("late-descendant.pid");
    let mut fault = case(
        "late-stall",
        "late-input-stall",
        b"input\n",
        b"before\nafter\n",
        b"actual late EIO confirmed\n",
        1,
    );
    fault["args"] = json!(["late-input-stall", pidfile]);
    fault["io"] = json!("read-error");
    fault["ack_hex"] = json!(encode(b"before\n"));
    run.write_cases(&[fault]);
    let evidence = run.evidence();
    let started = Instant::now();
    let output = run.run(&["--evidence-dir", evidence.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(started.elapsed() < Duration::from_secs(12));
    assert_stopped(&pidfile);
    let root = evidence.join("late-input").join(digest(b"late-stall"));
    let pid = json_file(&root.join("started.json"))["candidate_pid"]
        .as_u64()
        .unwrap();
    let cleanup = fs::read_to_string(root.join("candidate-cleanup.txt")).unwrap();
    assert!(cleanup.contains("reaped=true") && cleanup.contains(&format!("pid={pid} ")));
    assert!(matches!(
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None),
        Err(nix::errno::Errno::ESRCH)
    ));
}

#[test]
fn late_input_parent_cancellation_stops_the_coordinator_and_owned_group() {
    let run = TestRun::new(&[]);
    let pidfile = run.root.join("cancel-descendant.pid");
    let mut fault = case(
        "late-cancel",
        "late-input-stall",
        b"input\n",
        b"before\nafter\n",
        b"actual late EIO confirmed\n",
        1,
    );
    fault["args"] = json!(["late-input-stall", pidfile]);
    fault["io"] = json!("read-error");
    fault["ack_hex"] = json!(encode(b"before\n"));
    run.write_cases(&[fault]);
    let evidence = run.evidence();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastmash-corpus"))
        .arg("--binary")
        .arg(&run.binary)
        .arg("--fixture")
        .arg(&run.fixture)
        .arg("--evidence-dir")
        .arg(&evidence)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(4);
    while !pidfile.with_extension("ready").exists() {
        assert!(
            child.try_wait().unwrap().is_none(),
            "driver exited before cancellation control"
        );
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("candidate did not reach cancellation control");
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let root = evidence.join("late-input").join(digest(b"late-cancel"));
    let started = json_file(&root.join("started.json"));
    let candidate = root.join("candidate.pid");
    let coordinator = root.join("coordinator.pid");
    fs::write(
        &candidate,
        started["candidate_pid"].as_u64().unwrap().to_string(),
    )
    .unwrap();
    fs::write(
        &coordinator,
        started["coordinator_pid"].as_u64().unwrap().to_string(),
    )
    .unwrap();
    assert_stopped(&candidate);
    assert_stopped(&pidfile);
    assert_stopped(&coordinator);
    assert!(
        fs::read_to_string(root.join("candidate-cleanup.txt"))
            .unwrap()
            .contains("reaped=true")
    );
}

#[test]
fn held_output_observes_live_processes_actual_destinations_and_exact_prefixes() {
    for transport in ["normal", "pipe", "pty"] {
        let mut observed = case(
            "held",
            "held-output",
            b"input\n",
            b"before\nafter\n",
            b"",
            0,
        );
        observed["io"] = json!(transport);
        observed["observe_held"] = json!(true);
        observed["expected"]["held"] = json!({"bytes":7,"alive":true});
        if transport != "normal" {
            observed["expected"]["held"]["stdout_hex"] = json!(encode(b"before\n"));
        }
        let run = TestRun::new(&[observed.clone()]);
        let evidence = run.evidence();
        let output = run.run(&["--evidence-dir", evidence.to_str().unwrap()]);
        assert!(output.status.success(), "{transport}: {output:?}");
        let records = fs::read_to_string(evidence.join("observations.jsonl")).unwrap();
        let record: Value = serde_json::from_str(records.lines().next().unwrap()).unwrap();
        assert_eq!(record["observed"]["held"], observed["expected"]["held"]);
        assert_eq!(
            record["observed"]["destination"]["isatty"],
            transport == "pty"
        );
        assert!(
            record["observed"]["destination"]["block_size"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(record["observed"]["cleanup_confirmed"], true);
        let file = run.run(&["--stdin-file"]);
        assert!(file.status.success());
        assert!(String::from_utf8_lossy(&file.stdout).contains("0 of 0 cases passed"));
    }
    // Completed bytes alone cannot satisfy a pre-EOF observation.
    let mut buffered = case(
        "buffered",
        "held-buffered",
        b"input\n",
        b"before\nafter\n",
        b"",
        0,
    );
    buffered["io"] = json!("pipe");
    buffered["observe_held"] = json!(true);
    buffered["expected"]["held"] = json!({"bytes":7,"alive":true,"stdout_hex":encode(b"before\n")});
    let run = TestRun::new(&[buffered]);
    let output = run.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("held:"));
    // A leader that exits early must fail the live-process observation and
    // cannot leave a descendant holding the streaming destination open.
    let run = TestRun::new(&[]);
    let pidfile = run.root.join("writer-descendant.pid");
    let mut early = case("early", "orphan-output", b"input\n", b"", b"", 0);
    early["args"] = json!(["orphan-output", pidfile]);
    early["io"] = json!("pipe");
    early["observe_held"] = json!(true);
    early["expected"]["held"] = json!({"bytes":0,"alive":true,"stdout_hex":""});
    run.write_cases(&[early]);
    let output = run.run(&[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("held:"));
    assert_stopped(&pidfile);
}

#[test]
fn early_commands_finish_before_stdin_eof_and_file_mode_defers_them() {
    let mut held = case("held", "held-input", b"", b"stdin remains open\n", b"", 0);
    held["hold_stdin"] = json!(true);
    let ordinary = case("ordinary", "echo", b"after\n", b"after\n", b"", 0);
    for file in [false, true] {
        let run = TestRun::new(&[held.clone(), ordinary.clone()]);
        let evidence = run.evidence();
        let mut args = vec!["--evidence-dir", evidence.to_str().unwrap()];
        if file {
            args.push("--stdin-file");
        }
        let output = run.run(&args);
        assert!(output.status.success(), "{output:?}");
        let result = json_file(&evidence.join("result.json"));
        assert_eq!(result["passed"], if file { 1 } else { 2 });
        assert_eq!(result["deferred_cases"], if file { 1 } else { 0 });
        if !file {
            let outcomes = fs::read_to_string(evidence.join("observations.jsonl")).unwrap();
            let observed: Value = serde_json::from_str(outcomes.lines().next().unwrap()).unwrap();
            assert_eq!(observed["observed"]["stdin_held_until_exit"], true);
        }
    }
    // The same executable must detect the wrong lifecycle when EOF is sent.
    held["hold_stdin"] = json!(false);
    let run = TestRun::new(&[held]);
    assert_eq!(run.run(&[]).status.code(), Some(1));
}

#[test]
fn directory_and_closed_pipe_faults_are_real_and_keep_other_io_intact() {
    let mut directory = case(
        "directory",
        "directory-input",
        b"unused input",
        b"directory read confirmed\n",
        b"",
        0,
    );
    directory["io"] = json!("directory");
    let mut pipe = case(
        "closed-pipe",
        "broken-output",
        b"",
        b"",
        b"broken output confirmed\n",
        0,
    );
    pipe["io"] = json!("closed-pipe");
    let ordinary = case("ordinary", "echo", b"ordinary\n", b"ordinary\n", b"", 0);
    for file in [false, true] {
        let run = TestRun::new(&[directory.clone(), pipe.clone(), ordinary.clone()]);
        let evidence = run.evidence();
        let mut args = vec!["--evidence-dir", evidence.to_str().unwrap()];
        if file {
            args.push("--stdin-file");
        }
        let output = run.run(&args);
        assert!(output.status.success(), "{output:?}");
        let result = json_file(&evidence.join("result.json"));
        assert_eq!(result["attempted"], if file { 1 } else { 3 });
        assert_eq!(result["passed"], result["attempted"]);
        assert_eq!(result["deferred_cases"], if file { 2 } else { 0 });
        let dispositions = fs::read_to_string(evidence.join("dispositions.jsonl")).unwrap();
        let dispositions: Vec<Value> = dispositions
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(dispositions.len(), 3);
        if file {
            assert_eq!(dispositions[0]["disposition"], "deferred-transport");
            assert_eq!(
                dispositions[1]["reason"],
                fastmash_corpus_driver::fixture::FILE_TRANSPORT_REASON
            );
        } else {
            assert_eq!(dispositions[0]["disposition"], "directory-input");
            assert_eq!(dispositions[1]["disposition"], "closed-output-pipe");
        }
    }
    // The same probe executables reject ordinary descriptors, so a missing
    // transport cannot produce a false passing observation.
    directory["io"] = json!("normal");
    pipe["io"] = json!("normal");
    let run = TestRun::new(&[directory, pipe]);
    let output = run.run(&["--keep-going"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stdout).contains("0 of 2 cases passed"));
}

#[test]
fn full_output_refuses_a_candidate_that_fails_ordinary_calibration() {
    let mut full = case("full", "echo", b"", b"", b"", 0);
    full["io"] = json!("full");
    let run = TestRun::new(&[full]);
    let evidence = run.evidence();
    let output = run.run(&["--evidence-dir", evidence.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("ordinary candidate control differs"));
    assert!(!evidence.join("result.json").exists());
    assert!(
        evidence
            .join("full-output/observations/ordinary-count/observed.json")
            .is_file()
    );
}

#[test]
fn unreviewed_held_combinations_remain_deferred_in_both_modes() {
    let mut held = case("held", "sleep", b"unexpected input", b"", b"", 0);
    held["hold_stdin"] = json!(true);
    let mut observed = case("observe", "sleep", b"", b"", b"", 0);
    observed["observe_held"] = json!({"bytes":0});
    let mut fault = case("fault", "sleep", b"", b"", b"", 0);
    fault["io"] = json!("read-error");
    for file in [false, true] {
        let run = TestRun::new(&[
            held.clone(),
            observed.clone(),
            fault.clone(),
            case("ordinary", "echo", b"", b"", b"", 0),
        ]);
        let evidence = run.evidence();
        let mut args = vec!["--evidence-dir", evidence.to_str().unwrap()];
        if file {
            args.push("--stdin-file");
        }
        assert!(run.run(&args).status.success());
        let identity = json_file(&evidence.join("identity.json"));
        assert_eq!(identity["selected_cases"], 1);
        assert_eq!(identity["matched_cases"], 4);
        assert_eq!(
            identity["transport_dispositions"].as_array().unwrap().len(),
            3
        );
        assert_eq!(
            fs::read_to_string(evidence.join("dispositions.jsonl"))
                .unwrap()
                .lines()
                .count(),
            4
        );
    }
}

#[test]
fn only_named_diagnostics_and_system_sort_prefixes_are_adapted() {
    let mut native = case(
        "sorting/header-empty",
        "header-error",
        b"",
        b"",
        b"frozen Linux diagnostic\n",
        1,
    );
    let run = TestRun::new(&[
        native.clone(),
        case("sort-prefix", "sort-error", b"", b"", b"sort: sample\n", 0),
    ]);
    assert!(run.run(&[]).status.success());
    native["id"] = json!("unknown-header-error");
    run.write_cases(&[native]);
    assert_eq!(run.run(&[]).status.code(), Some(1));
}

#[test]
fn explicit_version_only_changes_successful_version_actions() {
    let run = TestRun::new(&[
        case("version", "version", b"", b"fastmash frozen\n", b"", 0),
        case("version-is-required-value", "echo", b"", b"", b"", 0),
    ]);
    assert!(run.run(&["--expected-version", "0.2.0"]).status.success());
    assert_eq!(run.run(&[]).status.code(), Some(1));
    assert_eq!(
        run.run(&["--expected-version", "incorrect"]).status.code(),
        Some(1)
    );
}

#[test]
fn a_blocked_inherited_signal_mask_is_rejected_before_command_launch() {
    use nix::sys::signal::{SigSet, Signal};
    let run = TestRun::new(&[case("ordinary", "echo", b"", b"", b"", 0)]);
    let old = SigSet::thread_get_mask().unwrap();
    let mut blocked = old;
    blocked.add(Signal::SIGUSR1);
    blocked.thread_set_mask().unwrap();
    let output = run.run(&[]);
    old.thread_set_mask().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unblocked signal mask"));
}

#[cfg(target_os = "linux")]
#[test]
fn a_real_time_only_inherited_mask_is_rejected_before_command_launch() {
    use nix::sys::signal::{SigSet, Signal};
    let run = TestRun::new(&[case("ordinary", "echo", b"", b"", b"", 0)]);
    let old = SigSet::thread_get_mask().unwrap();
    // sigfillset includes real-time signals. Removing every Nix enum signal
    // leaves precisely the Linux mask class that its iterator cannot observe.
    let mut realtime = SigSet::all();
    for signal in Signal::iterator() {
        realtime.remove(signal);
    }
    realtime.thread_set_mask().unwrap();
    let output = run.run(&[]);
    old.thread_set_mask().unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unblocked signal mask"));
}

#[test]
fn changing_fixture_identity_during_execution_invalidates_the_run() {
    let run = TestRun::new(&[]);
    let mut mutate = case("mutation", "mutate-file", b"", b"", b"", 0);
    mutate["args"] = json!(["mutate-file", run.fixture]);
    run.write_cases(&[mutate]);
    let output = run.run(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("identity changed"));
}

#[test]
fn fixture_identity_inventory_and_evidence_reuse_fail_closed() {
    let run = TestRun::new(&[case("okay", "echo", b"", b"", b"", 0)]);
    let evidence = run.evidence();
    fs::create_dir(&evidence).unwrap();
    fs::write(evidence.join("existing"), b"retained").unwrap();
    let result = run.run(&["--evidence-dir", evidence.to_str().unwrap()]);
    assert_eq!(result.status.code(), Some(2));
    assert_eq!(fs::read(evidence.join("existing")).unwrap(), b"retained");
    let provenance = run.root.join("cases-provenance.json");
    let original = fs::read(&provenance).unwrap();
    for key in ["fixture_sha256", "content_sha256"] {
        let mut changed: Value = serde_json::from_slice(&original).unwrap();
        changed[key] = json!("0".repeat(64));
        fs::write(&provenance, serde_json::to_vec(&changed).unwrap()).unwrap();
        let output = run.run(&[]);
        assert_eq!(output.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&output.stderr).contains("differs from its provenance"));
    }
    fs::write(&provenance, original).unwrap();
    fs::write(&run.fixture, b"corrupt").unwrap();
    assert_eq!(run.run(&[]).status.code(), Some(2));
    run.write_cases(&[
        case("duplicate", "echo", b"", b"", b"", 0),
        case("duplicate", "echo", b"", b"", b"", 0),
    ]);
    assert_eq!(run.run(&[]).status.code(), Some(2));
    #[cfg(target_os = "linux")]
    {
        fs::remove_file(run.root.join("fastmash-sort-supervisor")).unwrap();
        assert_eq!(run.run(&[]).status.code(), Some(2));
    }
}
