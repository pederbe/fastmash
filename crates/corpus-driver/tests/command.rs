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
fn transport_dispositions_account_for_held_normal_cases_in_both_modes() {
    let mut held = case("held", "sleep", b"", b"", b"", 0);
    held["hold_stdin"] = json!(true);
    let mut observed = case("observe", "sleep", b"", b"", b"", 0);
    observed["observe_held"] = json!({"bytes":0});
    let mut fault = case("fault", "sleep", b"", b"", b"", 0);
    fault["io"] = json!("full");
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
