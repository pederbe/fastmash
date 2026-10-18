#![forbid(unsafe_code)]
#![cfg(unix)]

// Exercise the private capture owner with Cargo's standalone Rust fixture,
// without widening the locale tooling's public interface for development tests.
#[path = "../src/capture.rs"]
mod capture;

use capture::{Budget, capture, capture_pipe};
use fastmash_locale_tools::Result;
use fastmash_locale_tools::collation_plan::punctuation_words;
use serde_json::Value;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "locale-capture-{label}-{}-{stamp}",
        std::process::id()
    ));
    DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}

fn fixture(mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_locale-capture-fixture"));
    command
        .arg(mode)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C");
    command
}

fn read_json(directory: &Path, name: &str) -> Value {
    serde_json::from_slice(&fs::read(directory.join(name)).unwrap()).unwrap()
}

fn assert_clean_wait(waited: &Value) {
    assert_eq!(waited["reaped"], true);
    assert!(waited["raw_status"].is_number());
    assert!(waited["kill_error"].is_null());
    assert!(waited["wait_error"].is_null());
    assert!(waited["input_error"].is_null());
}

fn assert_group_signal(waited: &Value) {
    assert!(matches!(
        waited["group_signal"].as_str(),
        Some("sent" | "already_absent")
    ));
}

#[test]
fn frozen_numeric_input_keeps_its_limit_and_receipt_shape() {
    let root = root("frozen");
    let accepted = root.join("accepted");
    fs::create_dir(&accepted).unwrap();
    let bytes = vec![b'x'; 223];
    let captured = capture(
        &mut fixture("echo"),
        Some(&bytes),
        &accepted,
        Budget::comparison(Duration::from_secs(2)),
    )
    .unwrap();
    assert!(captured.status.success());
    assert_eq!(captured.stdout, bytes);
    assert_eq!(captured.stderr, b"pipe-eof\n");
    let delivery = read_json(&accepted, "stdin-delivery.json");
    assert_eq!(delivery.as_object().unwrap().len(), 4);
    assert_eq!(delivery["delivered"], true);
    assert_eq!(delivery["writer_closed"], true);
    assert_eq!(delivery["broken_pipe"], false);
    assert!(delivery["error"].is_null());
    let waited = read_json(&accepted, "wait.json");
    assert_clean_wait(&waited);
    assert_eq!(waited.as_object().unwrap().len(), 12);
    assert_eq!(waited["execution_milliseconds"], 2_000);
    assert_eq!(waited["cleanup_milliseconds"], 1_000);
    assert_eq!(waited["timeout_starts_after_spawn"], true);

    let refused = root.join("refused");
    fs::create_dir(&refused).unwrap();
    let error = capture(
        &mut fixture("echo"),
        Some(&[b'x'; 224]),
        &refused,
        Budget::comparison(Duration::from_secs(2)),
    )
    .err()
    .unwrap();
    assert_eq!(
        error.to_string(),
        "comparison input exceeds the frozen 223-byte workload"
    );
    assert_eq!(fs::read_dir(&refused).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn query_without_input_keeps_null_stdin_and_the_total_budget() {
    let root = root("query");
    let captured = capture(
        &mut fixture("null"),
        None,
        &root,
        Budget::query(Duration::from_secs(2)),
    )
    .unwrap();
    assert!(captured.status.success());
    assert!(captured.stdout.is_empty());
    assert_eq!(captured.stderr, b"pipe-eof\n");
    assert!(!root.join("stdin").exists());
    assert!(!root.join("stdin-delivery.json").exists());
    let waited = read_json(&root, "wait.json");
    assert_clean_wait(&waited);
    assert_eq!(waited["execution_milliseconds"], 1_500);
    assert_eq!(waited["cleanup_milliseconds"], 500);
    assert_eq!(waited["timeout_starts_after_spawn"], false);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn short_comparison_budget_delivers_input_and_reaps_a_nonreading_child() {
    let root = root("small-no-read");
    let input = b"0\t0\n1\t1\n";
    let started = Instant::now();
    let result = capture(
        &mut fixture("nonreading"),
        Some(input),
        &root,
        Budget::comparison(Duration::from_millis(250)),
    );
    assert!(result.is_err());
    assert!(started.elapsed() >= Duration::from_millis(250));
    let waited = read_json(&root, "wait.json");
    assert_clean_wait(&waited);
    assert_eq!(waited["timed_out"], true);
    assert_eq!(waited["signal"], 9);
    assert!(waited["exit_code"].is_null());
    assert_eq!(waited["execution_milliseconds"], 250);
    assert_eq!(waited["cleanup_milliseconds"], 1_000);
    assert_eq!(waited["timeout_starts_after_spawn"], true);
    let delivery = read_json(&root, "stdin-delivery.json");
    assert_eq!(delivery["delivered"], true);
    assert_eq!(delivery["writer_closed"], true);
    assert_eq!(delivery["broken_pipe"], false);
    assert!(delivery["error"].is_null());
    assert_eq!(fs::read(root.join("stdin")).unwrap(), input);
    assert!(root.join("stdout").is_file());
    assert!(root.join("stderr").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn punctuation_workload_reaches_real_pipe_and_eof_byte_for_byte() {
    let root = root("punctuation");
    let words = punctuation_words();
    assert_eq!(words.len(), 1_983);
    let input = format!("{}\n", words.join("\n")).into_bytes();
    assert_eq!(input.len(), 7_786);
    let captured = capture_pipe(
        &mut fixture("echo"),
        Some(&input),
        &root,
        Budget::comparison(Duration::from_secs(2)),
    )
    .unwrap();
    assert!(captured.status.success());
    assert_eq!(captured.stdout, input);
    assert_eq!(captured.stderr, b"pipe-eof\n");
    assert_eq!(fs::read(root.join("stdin")).unwrap(), input);
    let delivery = read_json(&root, "stdin-delivery.json");
    assert_eq!(delivery["delivered"], true);
    assert_eq!(delivery["bytes_written"], 7_786);
    assert_eq!(delivery["writer_closed"], true);
    assert_eq!(delivery["broken_pipe"], false);
    assert!(delivery["error"].is_null());
    let waited = read_json(&root, "wait.json");
    assert_clean_wait(&waited);
    assert_eq!(waited["timed_out"], false);
    assert_group_signal(&waited);
    assert_eq!(waited["process_group"], waited["pid"]);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn backpressure_and_partial_writes_preserve_all_input() {
    let root = root("backpressure");
    let input: Vec<u8> = (0..2 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect();
    let captured = capture_pipe(
        &mut fixture("slow-echo"),
        Some(&input),
        &root,
        Budget::comparison(Duration::from_secs(5)),
    )
    .unwrap();
    assert!(captured.status.success());
    assert_eq!(captured.stdout, input);
    assert_eq!(captured.stderr, b"pipe-eof\n");
    let delivery = read_json(&root, "stdin-delivery.json");
    assert_eq!(delivery["would_block"], true);
    assert_eq!(delivery["bytes_written"], input.len());
    assert_eq!(delivery["delivered"], true);
    assert_eq!(delivery["writer_closed"], true);
    assert!(delivery["error"].is_null());
    assert_clean_wait(&read_json(&root, "wait.json"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn early_close_preserves_delivery_failure_and_native_exit_or_signal() {
    for (mode, code, signal) in [("early-close", Some(17), None), ("signal", None, Some(15))] {
        let root = root(mode);
        let input = vec![b'x'; 2 * 1024 * 1024];
        let captured = capture_pipe(
            &mut fixture(mode),
            Some(&input),
            &root,
            Budget::comparison(Duration::from_secs(2)),
        )
        .unwrap();
        assert_eq!(captured.status.code(), code);
        assert_eq!(captured.status.signal(), signal);
        assert_eq!(captured.stdout, b"\xffearly\n");
        assert_eq!(captured.stderr, b"\x80closed\n");
        let delivery = read_json(&root, "stdin-delivery.json");
        assert_eq!(delivery["delivered"], false);
        assert_eq!(delivery["broken_pipe"], true);
        assert_eq!(delivery["writer_closed"], true);
        assert!(delivery["bytes_written"].as_u64().unwrap() < input.len() as u64);
        assert!(delivery["error"].is_null());
        let waited = read_json(&root, "wait.json");
        assert_clean_wait(&waited);
        assert_eq!(waited["timed_out"], false);
        assert_group_signal(&waited);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn nonreading_child_cannot_block_the_parent_past_its_budget() {
    let root = root("large-no-read");
    let input = vec![b'x'; 2 * 1024 * 1024];
    let started = Instant::now();
    let result = capture_pipe(
        &mut fixture("nonreading"),
        Some(&input),
        &root,
        Budget::comparison(Duration::from_millis(250)),
    );
    assert!(result.is_err());
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert!(started.elapsed() < Duration::from_secs(3));
    let delivery = read_json(&root, "stdin-delivery.json");
    assert_eq!(delivery["delivered"], false);
    assert_eq!(delivery["writer_closed"], true);
    assert_eq!(delivery["would_block"], true);
    assert_eq!(delivery["broken_pipe"], false);
    assert!(delivery["bytes_written"].as_u64().unwrap() < input.len() as u64);
    assert!(delivery["error"].is_null());
    let waited = read_json(&root, "wait.json");
    assert_clean_wait(&waited);
    assert_eq!(waited["timed_out"], true);
    assert_eq!(waited["signal"], 9);
    assert_group_signal(&waited);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn descendants_holding_input_are_stopped_even_after_the_leader_exits() {
    for mode in ["descendant", "orphan"] {
        let root = root(mode);
        let pidfile = root.join("descendant.pid");
        let input = vec![b'x'; 2 * 1024 * 1024];
        let started = Instant::now();
        let result = capture_pipe(
            fixture(mode).arg(&pidfile),
            Some(&input),
            &root,
            Budget::comparison(Duration::from_millis(250)),
        );
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        let waited = read_json(&root, "wait.json");
        assert_clean_wait(&waited);
        assert_eq!(waited["timed_out"], true);
        assert_eq!(waited["group_signal"], "sent");
        if mode == "orphan" {
            assert_eq!(waited["exit_code"], 0);
            assert!(waited["signal"].is_null());
        } else {
            assert_eq!(waited["signal"], 9);
        }
        let delivery = read_json(&root, "stdin-delivery.json");
        assert_eq!(delivery["delivered"], false);
        assert_eq!(delivery["writer_closed"], true);
        assert!(delivery["error"].is_null());
        assert_stopped(&pidfile);
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn successful_delivery_and_leader_exit_still_stop_a_descendant() {
    let root = root("delivered-orphan");
    let pidfile = root.join("descendant.pid");
    // This is larger than the frozen numeric input, but fits a fresh POSIX pipe.
    let input = vec![b'x'; 300];
    let captured = capture_pipe(
        fixture("orphan").arg(&pidfile),
        Some(&input),
        &root,
        Budget::comparison(Duration::from_secs(2)),
    )
    .unwrap();
    assert!(captured.status.success());
    let waited = read_json(&root, "wait.json");
    assert_clean_wait(&waited);
    assert_eq!(waited["timed_out"], false);
    assert_eq!(waited["exit_code"], 0);
    assert_eq!(waited["group_signal"], "sent");
    assert_eq!(read_json(&root, "stdin-delivery.json")["delivered"], true);
    assert_stopped(&pidfile);
    fs::remove_dir_all(root).unwrap();
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
        // A killed orphan may await its system reaper. It cannot retain input.
        #[cfg(target_os = "linux")]
        if fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|status| {
            status
                .rsplit_once(") ")
                .is_some_and(|(_, rest)| rest.starts_with('Z'))
        }) {
            return;
        }
        assert!(Instant::now() < deadline, "descendant {pid} remained live");
        std::thread::sleep(Duration::from_millis(5));
    }
}
