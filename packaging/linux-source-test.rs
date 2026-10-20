//! Installed checks shared by the retained Fedora and Nix source recipes.
#![forbid(unsafe_code)]

use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, ExitCode, Output, Stdio};

fn pipe(mut command: Command, input: &[u8]) -> io::Result<Output> {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let delivered = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output()?;
    delivered?;
    Ok(output)
}

fn command(binary: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command.args(args).env("LC_ALL", "C");
    command
}

fn expect(output: &Output, expected: &[u8], label: &str) -> io::Result<()> {
    if !output.status.success() || output.stdout != expected {
        return Err(io::Error::other(format!(
            "{label}: status {}, stdout {:?}, stderr {:?}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

fn run() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let (fedora, bin, version, docs) = match args.as_slice() {
        [mode, bin, version] if mode == "fedora" => (true, bin, version, None),
        [mode, bin, version, docs] if mode == "nix" => (false, bin, version, Some(docs)),
        _ => {
            return Err(io::Error::other(
                "usage: fastmash-test-linux-source fedora BIN_DIR VERSION | nix BIN_DIR VERSION DOC_DIR",
            ));
        }
    };
    let bin = Path::new(bin);
    for name in ["fastmash", "fastmash-sort-supervisor"] {
        let metadata = fs::metadata(bin.join(name))?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
            return Err(io::Error::other(format!(
                "missing or nonexecutable program: {name}"
            )));
        }
    }
    if let Some(docs) = docs {
        for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
            let metadata = fs::metadata(Path::new(docs).join(name))?;
            if !metadata.is_file() || metadata.len() == 0 {
                return Err(io::Error::other(format!(
                    "missing or empty document: {name}"
                )));
            }
        }
    }
    let version = version
        .to_str()
        .ok_or_else(|| io::Error::other("version must be UTF-8"))?;
    let binary = bin.join("fastmash");
    expect(
        &command(&binary, &["--version"]).output()?,
        format!("fastmash {version}\n").as_bytes(),
        "version",
    )?;
    expect(
        &pipe(command(&binary, &["sum", "1", "mean", "1"]), b"1\n2\n3\n")?,
        b"6\t2\n",
        "arithmetic",
    )?;
    expect(
        &pipe(command(&binary, &["geomean", "1"]), b"1\n2\n3\n")?,
        b"1.8171205928321\n",
        "geometric mean",
    )?;
    let mut route = Command::new(&binary);
    route
        .args(["-sg1", "pcov", "2:3"])
        .env_clear()
        .env("LC_ALL", "C")
        .env("FASTMASH_GROUPING", "sort")
        .env("FASTMASH_SORT_TRACE", "1");
    if fedora {
        route.env("PATH", "/usr/bin:/bin");
    }
    let output = pipe(route, b"b\t2\t3\na\t1\t2\na\t3\t6\n")?;
    expect(&output, b"a\t2\nb\t0\n", "paired operation")?;
    if output.stderr != b"sort route: system sort\n" {
        return Err(io::Error::other(format!(
            "paired operation did not use the Sort supervisor: {:?}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
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
