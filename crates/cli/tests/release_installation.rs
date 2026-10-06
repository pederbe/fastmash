//! The Linux release check executes an installed, checksummed pair.
#![cfg(target_os = "linux")]

use std::{fs, path::Path, process::Command};

#[path = "support/temp_dir.rs"]
mod test_dir;

fn installed_pair(directory: &Path) {
    fs::copy(env!("CARGO_BIN_EXE_fastmash"), directory.join("fastmash")).unwrap();
    fs::copy(
        env!("CARGO_BIN_EXE_fastmash-sort-supervisor"),
        directory.join("fastmash-sort-supervisor"),
    )
    .unwrap();
    record_checksums(directory);
}

fn record_checksums(directory: &Path) {
    let output = Command::new("sha256sum")
        .args(["fastmash", "fastmash-sort-supervisor"])
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    fs::write(directory.join("qualified.sha256"), output.stdout).unwrap();
}

fn check(directory: &Path, version: &str) -> std::process::Output {
    Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../release/check-installed.sh"
        ))
        .arg(version)
        .arg(directory)
        .arg(directory.join("qualified.sha256"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .current_dir(directory)
        .output()
        .unwrap()
}

#[test]
fn release_check_accepts_the_installed_qualified_pair() {
    let directory = test_dir::TempDir::new("release-installed");
    installed_pair(&directory.0);
    let output = check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(output.status.success(), "{output:?}");
}

#[test]
fn release_check_rejects_a_version_from_another_release() {
    let directory = test_dir::TempDir::new("release-version");
    installed_pair(&directory.0);
    let output = check(&directory.0, "0.0.0-wrong-release");
    assert!(!output.status.success(), "{output:?}");
}

#[test]
fn release_check_rejects_changed_installed_bytes() {
    use std::io::Write;
    let directory = test_dir::TempDir::new("release-checksum");
    installed_pair(&directory.0);
    fs::OpenOptions::new()
        .append(true)
        .open(directory.0.join("fastmash"))
        .unwrap()
        .write_all(b"changed after qualification")
        .unwrap();
    let output = check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("fastmash: FAILED"));
}

#[test]
fn release_check_rejects_a_missing_or_incompatible_supervisor() {
    let directory = test_dir::TempDir::new("release-supervisor");
    installed_pair(&directory.0);
    let supervisor = directory.0.join("fastmash-sort-supervisor");
    fs::remove_file(&supervisor).unwrap();
    let missing = check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!missing.status.success(), "{missing:?}");
    assert!(String::from_utf8_lossy(&missing.stderr).contains("not installed and executable"));

    // Its checksum is admitted here to isolate the supervisor execution check.
    fs::copy("/usr/bin/false", &supervisor).unwrap();
    record_checksums(&directory.0);
    let incompatible = check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!incompatible.status.success(), "{incompatible:?}");
}
