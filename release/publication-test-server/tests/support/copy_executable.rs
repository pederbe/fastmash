//! Copy executable fixtures without opening writable handles in the test harness.
#![forbid(unsafe_code)]

use std::{path::Path, process::Command};

pub fn copy_executable(source: impl AsRef<Path>, destination: impl AsRef<Path>) {
    // This single-threaded child exits before the fixture is launched. Other
    // test threads cannot inherit its writable destination descriptor.
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-source-package-fixture"))
        .arg("--copy-executable")
        .arg(source.as_ref())
        .arg(destination.as_ref())
        .env_clear()
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "copy {:?} to {:?}: {output:?}",
        source.as_ref(),
        destination.as_ref()
    );
}
