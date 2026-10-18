//! Invalid usage must fail before any fixture or native child is started.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::process::Command;

#[test]
fn invalid_arguments_do_not_start_native_work_or_create_evidence() {
    let evidence = std::env::temp_dir().join(format!(
        "fastmash-publication-invalid-{}",
        std::process::id()
    ));
    assert!(!evidence.exists());
    for arguments in [vec![], vec!["old", "new"], vec!["a", "b", "c", "d"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-package-publication"))
            .args(arguments)
            .env("PUBLICATION_TEST_EVIDENCE", &evidence)
            .env("CONTAINER_ENGINE", "/nonexistent-publication-engine")
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert!(
            output.stderr.starts_with(b"publication checks: usage:"),
            "{output:?}"
        );
        assert!(fs::symlink_metadata(&evidence).is_err());
    }
}
