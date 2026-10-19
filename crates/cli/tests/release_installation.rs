//! The Linux release check executes an installed, checksummed pair.
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::{fs, path::Path, process::Command};

#[path = "support/temp_dir.rs"]
mod test_dir;

fn prepared_tool(name: &str, variable: &str) -> std::path::PathBuf {
    let path = std::env::var_os(variable)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/debug")
                .join(name)
        });
    assert!(
        path.is_file(),
        "prepare release tools before workspace tests: cargo build --locked --manifest-path release/publication-test-server/Cargo.toml --target-dir target --bins"
    );
    path.canonicalize().unwrap()
}

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
        .env(
            "FASTMASH_INSTALLED_CHECKER",
            prepared_tool("fastmash-check-installed", "FASTMASH_INSTALLED_CHECKER"),
        )
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
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unexpected installed output: --version"),
        "{output:?}"
    );
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

    fs::copy(env!("CARGO_BIN_EXE_fastmash-sort-supervisor"), &supervisor).unwrap();
    fs::set_permissions(&supervisor, fs::Permissions::from_mode(0o644)).unwrap();
    record_checksums(&directory.0);
    let nonexecutable = check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!nonexecutable.status.success(), "{nonexecutable:?}");
    assert!(
        String::from_utf8_lossy(&nonexecutable.stderr).contains("not installed and executable")
    );

    // Its checksum is admitted here to isolate the supervisor execution check.
    fs::copy("/usr/bin/false", &supervisor).unwrap();
    record_checksums(&directory.0);
    let incompatible = check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!incompatible.status.success(), "{incompatible:?}");
    assert!(
        String::from_utf8_lossy(&incompatible.stderr).contains("sort supervisor"),
        "{incompatible:?}"
    );
}

// Exercise the native archive's one-executable layout on Linux. Actual Mach-O
// execution is verified separately by the native installation workflow.
fn native_layout_check(directory: &Path, version: &str) -> std::process::Output {
    let tools = directory.join("tools");
    if !tools.exists() {
        fs::create_dir(&tools).unwrap();
        let fixture = prepared_tool(
            "fastmash-installed-platform-fixture",
            "FASTMASH_INSTALLED_PLATFORM_FIXTURE",
        );
        fs::copy(&fixture, tools.join("uname")).unwrap();
        fs::copy(&fixture, tools.join("shasum")).unwrap();
    }
    let mut path = tools.as_os_str().to_owned();
    path.push(":/usr/bin:/bin");
    Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../release/check-installed.sh"
        ))
        .arg(version)
        .arg(directory)
        .arg(directory.join("qualified.sha256"))
        .env_clear()
        .env("PATH", path)
        .env(
            "FASTMASH_INSTALLED_CHECKER",
            prepared_tool("fastmash-check-installed", "FASTMASH_INSTALLED_CHECKER"),
        )
        .current_dir(directory)
        .output()
        .unwrap()
}

fn native_layout(directory: &Path) {
    fs::copy(env!("CARGO_BIN_EXE_fastmash"), directory.join("fastmash")).unwrap();
    let output = Command::new("sha256sum")
        .arg("fastmash")
        .current_dir(directory)
        .output()
        .unwrap();
    assert!(output.status.success());
    fs::write(directory.join("qualified.sha256"), output.stdout).unwrap();
}

#[test]
fn release_check_accepts_the_native_one_executable_layout() {
    let directory = test_dir::TempDir::new("native-release-layout");
    native_layout(&directory.0);
    let output = native_layout_check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(output.status.success(), "{output:?}");

    fs::copy(
        env!("CARGO_BIN_EXE_fastmash-sort-supervisor"),
        directory.0.join("fastmash-sort-supervisor"),
    )
    .unwrap();
    let extra = native_layout_check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!extra.status.success(), "{extra:?}");
    assert!(String::from_utf8_lossy(&extra.stderr).contains("must not contain a supervisor"));
}

#[test]
fn native_release_check_rejects_wrong_version_or_changed_bytes() {
    use std::io::Write;
    let directory = test_dir::TempDir::new("native-release-identity");
    native_layout(&directory.0);
    let wrong = native_layout_check(&directory.0, "0.0.0-wrong-release");
    assert!(!wrong.status.success(), "{wrong:?}");
    fs::OpenOptions::new()
        .append(true)
        .open(directory.0.join("fastmash"))
        .unwrap()
        .write_all(b"changed after qualification")
        .unwrap();
    let changed = native_layout_check(&directory.0, env!("CARGO_PKG_VERSION"));
    assert!(!changed.status.success(), "{changed:?}");
    assert!(String::from_utf8_lossy(&changed.stdout).contains("fastmash: FAILED"));
}
