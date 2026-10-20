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

/// Preserve ignored SIGXFSZ across the fresh launcher and target execs.
/// Set the selected file-size limit after the caller's env_clear().
#[allow(dead_code)] // Other suites use only the ordinary alarm launcher.
pub fn ignoring_file_size_signal(binary: impl AsRef<OsStr>, seconds: u32) -> Command {
    let launcher = bounded(binary, seconds);
    let mut command = Command::new("/bin/sh");
    command
        .args(["-c", r#"trap '' XFSZ && exec "$@""#, "fastmash-file-size"])
        .arg(launcher.get_program())
        .args(launcher.get_args());
    command
}
