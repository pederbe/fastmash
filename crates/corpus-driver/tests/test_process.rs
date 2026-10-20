#![forbid(unsafe_code)]
use std::ffi::OsString;
use std::os::unix::{ffi::OsStringExt, process::ExitStatusExt};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        if self.0.try_wait().unwrap().is_none() {
            self.0.kill().unwrap();
            self.0.wait().unwrap();
        }
    }
}

fn command(seconds: u32, mode: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash-test-process"));
    command
        .arg(seconds.to_string())
        .arg(env!("CARGO_BIN_EXE_fastmash-corpus-test-command"))
        .arg(mode)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn exec_preserves_pid_byte_arguments_environment_and_selected_mask() {
    for ignored in [false, true] {
        let mut command = command(5, "process-facts");
        command
            .arg(OsString::from_vec(vec![b'x', 0xff]))
            .env("SELECTED_ENVIRONMENT", OsString::from_vec(vec![0xfe]))
            .env("FASTMASH_TEST_PROCESS_BLOCKED", "15");
        if ignored {
            command.env("FASTMASH_TEST_PROCESS_IGNORE_PIPE", "1");
        }
        let child = command.spawn().unwrap();
        let pid = child.id();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let facts: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(facts["pid"], pid);
        assert_eq!(facts["argv0"], serde_json::json!(b"fastmash"));
        assert_eq!(facts["arguments"], serde_json::json!([[b'x', 0xff]]));
        assert_eq!(facts["selected_environment"], serde_json::json!([0xfe]));
        assert_eq!(facts["sigterm_blocked"], true);
        assert_eq!(facts["private_environment_present"], false);
    }
}

#[test]
fn alarm_survives_exec_and_ends_the_actual_stalled_candidate() {
    let mut child = Running(command(1, "sleep").spawn().unwrap());
    let started = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert_eq!(status.signal(), Some(nix::libc::SIGALRM));
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(4));
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn target_exit_status_is_preserved() {
    let output = command(5, "count").output().unwrap();
    assert_eq!(output.status.code(), Some(17));
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
}

#[test]
fn sixty_second_bound_survives_exec_and_invalid_bounds_are_rejected() {
    let output = command(60, "alarm-remaining").output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let remaining: u32 = std::str::from_utf8(&output.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!((31..=60).contains(&remaining), "{remaining}");
    for seconds in [0, 61] {
        let output = command(seconds, "alarm-remaining").output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("between 1 and 60 seconds"));
    }
}

#[test]
fn resource_limits_apply_to_the_target_without_private_environment() {
    let limits = [
        ("FASTMASH_TEST_PROCESS_FILE_SIZE", "file_size", 8192u64),
        #[cfg(target_os = "linux")]
        (
            "FASTMASH_TEST_PROCESS_ADDRESS_SPACE",
            "address_space",
            512 << 20,
        ),
    ];
    for (variable, field, limit) in limits {
        let output = command(5, "resource-facts")
            .env(variable, limit.to_string())
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        let facts: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(facts[field], serde_json::json!([limit, limit]));
        assert_eq!(facts["private_environment_present"], false);
        let output = command(5, "resource-facts")
            .env(variable, "invalid")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
    }
}
