#![forbid(unsafe_code)]
//! Execute one Command while owning its selected full-output destination.
#[cfg(target_os = "linux")]
use std::fs::File;
use std::{
    ffi::OsStr,
    path::Path,
    process::{Command, Output, Stdio},
};

/// `root` must be a fresh sibling of the caller's private fixture, outside its
/// ordinary cleanup guard. A failed native detach retains the image and proof.
pub fn output(command: Command, binary: &OsStr, full: bool, root: &Path) -> Output {
    output_with_version(command, binary, full, root, b"fastmash ")
}

/// Preserve calibration against a named reference's version action too.
pub fn output_with_version(
    command: Command,
    binary: &OsStr,
    full: bool,
    root: &Path,
    version_prefix: &[u8],
) -> Output {
    #[cfg(target_os = "linux")]
    let _ = (binary, root, version_prefix);
    #[cfg(target_os = "macos")]
    let mut full_output = full.then(|| {
        fastmash_corpus_driver::full_output::FullOutput::with_version_prefix(
            Path::new(binary),
            root,
            version_prefix,
        )
        .unwrap()
    });
    // Declare this local after the owner so its configured file handles close
    // first even when spawning or collecting the child's output fails.
    let mut command = command;
    #[cfg(target_os = "linux")]
    let destination: Stdio = if full {
        File::options()
            .write(true)
            .open("/dev/full")
            .unwrap()
            .into()
    } else {
        Stdio::piped()
    };
    #[cfg(target_os = "macos")]
    let destination: Stdio = full_output
        .as_mut()
        .map_or_else(Stdio::piped, |full| full.destination().unwrap().into());
    let out = command.stdout(destination).output().unwrap();
    // output() reaps the child but Command retains its parent-side file copies.
    drop(command);
    #[cfg(target_os = "macos")]
    if let Some(mut full) = full_output {
        full.verify().unwrap();
        full.finish().unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
    out
}
