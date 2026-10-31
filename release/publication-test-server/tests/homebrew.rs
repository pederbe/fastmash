#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-homebrew-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        fs::create_dir(path.join("docs")).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_fastmash-source-package-fixture"),
            path.join("bin/fastmash"),
        )
        .unwrap();
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
            fs::write(path.join("docs").join(name), b"nonempty\n").unwrap();
        }
        Self(path)
    }

    fn run(&self, mode: &str, version: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-test-homebrew"))
            .arg(self.0.join("bin"))
            .arg(self.0.join("docs"))
            .arg(version)
            .env("SOURCE_PACKAGE_FIXTURE", mode)
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
fn valid_installed_package_uses_file_input_and_the_platform_route() {
    let fixture = Fixture::new();
    let output = fixture.run("", "fixture");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    assert_eq!(
        fs::read(fixture.0.join("input")).unwrap(),
        b"b\t2\t3\na\t1\t2\na\t3\t6\n"
    );
    assert!(
        fs::read(fixture.0.join("trace"))
            .unwrap()
            .starts_with(b"sort route:")
    );
}

#[test]
fn wrong_outputs_failed_commands_and_wrong_routes_are_rejected() {
    let fixture = Fixture::new();
    for mode in [
        "wrong-output",
        "wrong-geomean",
        "wrong-paired-output",
        "wrong-route",
        "failed-command",
        "failed-version",
        "failed-route",
    ] {
        let output = fixture.run(mode, "fixture");
        assert_eq!(output.status.code(), Some(1), "{mode}: {output:?}");
        assert!(!output.stderr.is_empty());
    }
    assert_eq!(fixture.run("", "another-version").status.code(), Some(1));
}

#[test]
fn missing_or_empty_documents_are_rejected() {
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
        let fixture = Fixture::new();
        let path = fixture.0.join("docs").join(name);
        fs::write(&path, []).unwrap();
        assert_eq!(fixture.run("", "fixture").status.code(), Some(1));
        fs::remove_file(path).unwrap();
        assert_eq!(fixture.run("", "fixture").status.code(), Some(1));
    }
}

#[test]
fn wrong_supervisor_presence_is_rejected() {
    let fixture = Fixture::new();
    let supervisor = fixture.0.join("bin/fastmash-sort-supervisor");
    fs::write(supervisor, b"unexpected").unwrap();
    assert_eq!(fixture.run("", "fixture").status.code(), Some(1));
}

#[test]
fn missing_program_and_arguments_are_rejected() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.0.join("bin/fastmash")).unwrap();
    assert_eq!(fixture.run("", "fixture").status.code(), Some(1));
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-homebrew"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
}
