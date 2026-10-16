//! Static profiling through complete Commands, with exact stream expectations.
#![forbid(unsafe_code)]
#![cfg(feature = "hotpath")]

use std::{fs, process::Command};

#[path = "support/executable.rs"]
mod executable;
#[path = "support/temp_dir.rs"]
mod temp_dir;

#[test]
fn static_reports_preserve_success_error_and_refusal_streams() {
    let directory = temp_dir::TempDir::new("profiling-command");
    let input = directory.0.join("input");
    fs::write(&input, b"1\n2\n3\n").unwrap();
    for (index, args, status, stdout, stderr) in [
        (0, vec!["sum", "1"], 0, b"6\n".as_slice(), b"".as_slice()),
        (
            1,
            vec!["sum", "0"],
            1,
            b"",
            b"fastmash: invalid field '0' for operation 'sum'\n",
        ),
        (
            2,
            vec!["-s", "-g1", "sum", "1"],
            77,
            b"",
            b"fastmash: FASTMASH_GROUPING must be sort or hash\n",
        ),
    ] {
        let report = directory.0.join(format!("report-{index}.json"));
        let mut command = Command::new(executable::fastmash());
        let output = command
            .env_clear()
            .env("LC_ALL", "C")
            .env(
                "FASTMASH_GROUPING",
                if status == 77 { "invalid" } else { "sort" },
            )
            .env("HOTPATH_OUTPUT_PATH", &report)
            .env("HOTPATH_OUTPUT_FORMAT", "json")
            .env("HOTPATH_METRICS_SERVER_OFF", "1")
            .args(args)
            .stdin(fs::File::open(&input).unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(status), "{output:?}");
        assert_eq!(output.stdout, stdout, "{output:?}");
        assert_eq!(output.stderr, stderr, "{output:?}");
        let report = fs::read_to_string(report).unwrap();
        assert!(report.starts_with('{'), "{report}");
        assert!(report.contains("fastmash::Command"), "{report}");
    }
}

#[test]
fn static_capture_requires_an_isolated_new_report_and_no_listener() {
    let directory = temp_dir::TempDir::new("profiling-configuration");
    let existing = directory.0.join("existing");
    fs::write(&existing, b"preserve existing data").unwrap();
    for (path, metrics_off, expected) in [
        (
            directory.0.join("unused"),
            "0",
            b"fastmash: static profiling requires HOTPATH_METRICS_SERVER_OFF=1\n".as_slice(),
        ),
        (
            existing.clone(),
            "1",
            b"fastmash: profiling requires a new writable HOTPATH_OUTPUT_PATH file\n",
        ),
        (
            "/dev/stdout".into(),
            "1",
            b"fastmash: profiling requires a new writable HOTPATH_OUTPUT_PATH file\n",
        ),
    ] {
        let output = Command::new(executable::fastmash())
            .env_clear()
            .env("LC_ALL", "C")
            .env("HOTPATH_OUTPUT_PATH", path)
            .env("HOTPATH_METRICS_SERVER_OFF", metrics_off)
            .args(["sum", "1"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(77), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert_eq!(output.stderr, expected, "{output:?}");
    }
    assert_eq!(fs::read(existing).unwrap(), b"preserve existing data");
    assert!(!directory.0.join("unused").exists());
}
