//! Smoke checks for the source-built package, compiled with its build toolchain.
#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};

fn check(binary: &Path, args: &[&str], input: &[u8], expected: &[u8]) -> io::Result<()> {
    let mut child = Command::new(binary)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let delivered = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output()?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "{} failed with {}",
            args[0], output.status
        )));
    }
    delivered?;
    if output.stdout != expected {
        return Err(io::Error::other(format!(
            "unexpected package output for {}",
            args[0]
        )));
    }
    Ok(())
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 1 {
        return Err(io::Error::other(
            "usage: fastmash-test-source-package BIN_DIR",
        ));
    }
    let bin = Path::new(&args[0]);
    let binary = bin.join("fastmash");
    let status = Command::new(&binary)
        .arg("--version")
        .env("LC_ALL", "C")
        .status()?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "version command failed with {status}"
        )));
    }
    let metadata = fs::metadata(&binary)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(io::Error::other("fastmash is not installed and executable"));
    }
    check(&binary, &["sum", "1", "mean", "1"], b"1\n2\n3\n", b"6\t2\n")?;
    check(
        &binary,
        &["-s", "-g", "1", "sum", "2"],
        b"b\t2\na\t1\na\t3\n",
        b"a\t4\nb\t2\n",
    )?;
    check(
        &binary,
        &["geomean", "1"],
        b"1\n2\n3\n",
        b"1.8171205928321\n",
    )
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("source package checks: {error}");
            ExitCode::FAILURE
        }
    }
}
