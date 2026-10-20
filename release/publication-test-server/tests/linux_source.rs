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
            "fastmash-linux-source-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::create_dir(path.join("bin")).unwrap();
        fs::create_dir(path.join("docs")).unwrap();
        for name in ["fastmash", "fastmash-sort-supervisor"] {
            fs::copy(
                env!("CARGO_BIN_EXE_fastmash-linux-source-fixture"),
                path.join("bin").join(name),
            )
            .unwrap();
        }
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
            fs::write(path.join("docs").join(name), b"nonempty\n").unwrap();
        }
        Self(path)
    }

    fn run(&self, recipe: &str, mode: &str, version: &str) -> Output {
        fs::write(self.0.join("bin/mode"), format!("{recipe}:{mode}")).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash-test-linux-source"));
        command.arg(recipe).arg(self.0.join("bin")).arg(version);
        if recipe == "nix" {
            command.arg(self.0.join("docs"));
        }
        command
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
fn both_recipes_keep_pipe_input_and_their_exact_route_environment() {
    let fixture = Fixture::new();
    for recipe in ["fedora", "nix"] {
        let output = fixture.run(recipe, "", "fixture");
        assert!(output.status.success(), "{recipe}: {output:?}");
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
}

#[test]
fn failed_commands_wrong_bytes_versions_and_routes_are_rejected() {
    let fixture = Fixture::new();
    for recipe in ["fedora", "nix"] {
        for mode in [
            "failed-version",
            "failed-command",
            "failed-route",
            "wrong-output",
            "wrong-geomean",
            "wrong-paired",
            "wrong-route",
        ] {
            let output = fixture.run(recipe, mode, "fixture");
            assert_eq!(output.status.code(), Some(1), "{recipe}/{mode}: {output:?}");
            assert!(!output.stderr.is_empty());
        }
        assert_eq!(
            fixture.run(recipe, "", "another-version").status.code(),
            Some(1)
        );
    }
}

#[test]
fn nix_requires_each_nonempty_document_while_fedora_keeps_its_existing_scope() {
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
        let fixture = Fixture::new();
        let path = fixture.0.join("docs").join(name);
        fs::write(&path, []).unwrap();
        assert_eq!(fixture.run("nix", "", "fixture").status.code(), Some(1));
        fs::remove_file(path).unwrap();
        assert_eq!(fixture.run("nix", "", "fixture").status.code(), Some(1));
        assert!(fixture.run("fedora", "", "fixture").status.success());
    }
}

#[test]
fn missing_and_nonexecutable_programs_are_rejected() {
    for name in ["fastmash", "fastmash-sort-supervisor"] {
        let fixture = Fixture::new();
        let path = fixture.0.join("bin").join(name);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        for recipe in ["fedora", "nix"] {
            assert_eq!(fixture.run(recipe, "", "fixture").status.code(), Some(1));
        }
        fs::remove_file(path).unwrap();
        for recipe in ["fedora", "nix"] {
            assert_eq!(fixture.run(recipe, "", "fixture").status.code(), Some(1));
        }
    }
}

#[test]
fn invalid_invocations_fail() {
    for args in [
        vec![],
        vec!["unknown"],
        vec!["fedora", "bin"],
        vec!["nix", "bin", "version"],
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
