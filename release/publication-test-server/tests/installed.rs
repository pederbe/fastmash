//! The private command interfaces reject invalid input before any native client.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::process::Command;

#[test]
fn checker_help_runs_without_inputs_native_tools_or_evidence_directories() {
    let evidence =
        std::env::temp_dir().join(format!("fastmash-checker-help-{}", std::process::id()));
    assert!(!evidence.exists());
    for binary in [
        env!("CARGO_BIN_EXE_fastmash-check-installed"),
        env!("CARGO_BIN_EXE_fastmash-check-archive"),
        env!("CARGO_BIN_EXE_fastmash-check-package-bytes"),
        env!("CARGO_BIN_EXE_fastmash-test-package-installation"),
        env!("CARGO_BIN_EXE_fastmash-test-package-publication"),
        env!("CARGO_BIN_EXE_fastmash-test-package-fixture-repair"),
        env!("CARGO_BIN_EXE_fastmash-test-linux-source"),
        env!("CARGO_BIN_EXE_fastmash-test-source-package"),
    ] {
        for option in ["--help", "-h"] {
            let output = Command::new(binary)
                .arg(option)
                .env("PATH", "")
                .env("CONTAINER_ENGINE", "/missing-engine-must-not-run")
                .env("INSTALLATION_TEST_EVIDENCE", &evidence)
                .env("PUBLICATION_TEST_EVIDENCE", &evidence)
                .output()
                .unwrap();
            assert!(output.status.success(), "{binary}: {output:?}");
            assert!(output.stderr.is_empty(), "{binary}: {output:?}");
            assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: fastmash-"));
            assert!(!evidence.exists());
        }
    }
}

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
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(text.contains("No such file"), "{output:?}");
    assert!(
        text.contains("cannot open package input directory /path-that-does-not-exist/fastmash")
    );
}
