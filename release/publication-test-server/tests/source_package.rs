#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-source-package-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_fastmash-source-package-fixture"),
            path.join("fastmash"),
        )
        .unwrap();
        Self(path)
    }

    fn run(&self, mode: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-test-source-package"))
            .arg(&self.0)
            .env("SOURCE_PACKAGE_FIXTURE", mode)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn successful_source_package_checks_keep_version_output() {
    let fixture = Fixture::new();
    let output = fixture.run("");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fastmash fixture\n");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn wrong_bytes_and_failed_commands_with_matching_bytes_are_rejected() {
    let fixture = Fixture::new();
    for mode in ["wrong-output", "failed-command"] {
        let output = fixture.run(mode);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(output.stdout, b"fastmash fixture\n");
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn missing_and_nonexecutable_commands_are_rejected() {
    for missing in [true, false] {
        let fixture = Fixture::new();
        let supervisor = fixture.0.join("fastmash");
        if missing {
            fs::remove_file(supervisor).unwrap();
        } else {
            fs::set_permissions(supervisor, fs::Permissions::from_mode(0o644)).unwrap();
        }
        assert_eq!(fixture.run("").status.code(), Some(1));
    }
}

#[test]
fn failed_version_and_missing_program_are_rejected() {
    let fixture = Fixture::new();
    assert_eq!(fixture.run("failed-version").status.code(), Some(1));
    fs::remove_file(fixture.0.join("fastmash")).unwrap();
    assert_eq!(fixture.run("").status.code(), Some(1));
}

#[test]
fn an_explicit_binary_directory_is_required() {
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-source-package"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("BIN_DIR"));
}
