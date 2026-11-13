#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "support/copy_executable.rs"]
mod copy_executable;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-linux-source-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        copy_executable::copy_executable(
            env!("CARGO_BIN_EXE_fastmash-linux-source-fixture"),
            path.join("bin/fastmash"),
        );
        Self(path)
    }

    fn run(&self, mode: &str, version: &str) -> Output {
        fs::write(self.0.join("bin/mode"), mode).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash-test-linux-source"));
        command
            .arg("fedora")
            .arg(self.0.join("bin"))
            .arg(version)
            .env("PATH", "/unexpected")
            .env("SOURCE_PACKAGE_SHOULD_BE_CLEARED", "yes")
            .current_dir(&self.0)
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
fn fedora_keeps_pipe_input_and_its_exact_route_environment() {
    let fixture = Fixture::new();
    let output = fixture.run("", "fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[test]
fn failed_commands_wrong_bytes_versions_and_routes_are_rejected() {
    let fixture = Fixture::new();
    for mode in [
        "failed-version",
        "failed-command",
        "failed-route",
        "wrong-output",
        "wrong-geomean",
        "wrong-paired",
        "wrong-route",
    ] {
        let output = fixture.run(mode, "fixture");
        assert_eq!(output.status.code(), Some(1), "{mode}: {output:?}");
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(fixture.run("", "another-version").status.code(), Some(1));
}

#[test]
fn missing_and_nonexecutable_programs_are_rejected() {
    let fixture = Fixture::new();
    let path = fixture.0.join("bin/fastmash");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    let output = fixture.run("", "fixture");
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("not an executable file"));
    assert!(text.contains(&path.display().to_string()));

    fs::remove_file(&path).unwrap();
    let output = fixture.run("", "fixture");
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("cannot inspect installed executable"));
    assert!(text.contains(&path.display().to_string()));
}

#[test]
fn invalid_invocations_fail() {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["fedora", "bin"],
        vec!["nix", "bin", "version", "docs"],
        vec!["fedora", "bin", "version", "docs"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-linux-source"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    }
}
