//! The private command interfaces reject invalid input before any native client.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::process::Command;

#[test]
fn installed_checker_requires_its_three_public_arguments() {
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-check-installed"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("VERSION BIN_DIR QUALIFIED_CHECKSUMS")
    );
}

#[test]
fn package_checker_rejects_unknown_modes_before_engine_execution() {
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-package-installation"))
        .args(["unknown", "/", "0.2.0", "/missing"])
        .env("CONTAINER_ENGINE", "/missing-engine-must-not-run")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("usage:"), "{output:?}");
    assert!(!diagnostic.contains("No such file"), "{output:?}");
}

#[test]
fn package_checker_rejects_missing_inputs_before_engine_execution() {
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-package-installation"))
        .args([
            "repository",
            "/path-that-does-not-exist/fastmash",
            "0.2.0",
            "/missing",
        ])
        .env("CONTAINER_ENGINE", "/missing-engine-must-not-run")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("No such file"),
        "{output:?}"
    );
}
