#![forbid(unsafe_code)]
//! Child-only alarm setup through the prepared safe launcher.
use std::{ffi::OsStr, path::PathBuf, process::Command};

pub fn bounded(binary: impl AsRef<OsStr>, seconds: u32) -> Command {
    assert!((1..=60).contains(&seconds));
    let launcher = std::env::var_os("FASTMASH_TEST_PROCESS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("fastmash-test-process")
        });
    assert!(
        launcher.is_file(),
        "prepare fastmash-test-process before native command tests"
    );
    let mut command = Command::new(launcher);
    command.arg(seconds.to_string()).arg(binary);
    command
}
