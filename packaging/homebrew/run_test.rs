//! Installed source-package checks, built once with the formula's Rust toolchain.
#![forbid(unsafe_code)]

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, ExitCode, Output, Stdio};

fn command(binary: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(binary);
    command.args(args).env("LC_ALL", "C");
    command
}

fn pipe(binary: &Path, args: &[&str], input: &[u8]) -> io::Result<Output> {
    let mut child = command(binary, args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let delivered = child.stdin.take().unwrap().write_all(input);
    let output = child.wait_with_output()?;
    delivered?;
    Ok(output)
}

fn expect(output: &Output, bytes: &[u8], label: &str) -> io::Result<()> {
    if !output.status.success() || output.stdout != bytes {
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
    let [bin, docs, version] = args.as_slice() else {
        return Err(io::Error::other(
            "usage: fastmash-test-homebrew BIN_DIR DOC_DIR VERSION",
        ));
    };
    let version = version
        .to_str()
        .ok_or_else(|| io::Error::other("version must be UTF-8"))?;
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
        let metadata = fs::metadata(Path::new(docs).join(name))?;
        if !metadata.is_file() || metadata.len() == 0 {
            return Err(io::Error::other(format!(
                "missing or empty document: {name}"
            )));
        }
    }
    let binary = Path::new(bin).join("fastmash");
    expect(
        &command(&binary, &["--version"]).output()?,
        format!("fastmash {version}\n").as_bytes(),
        "version",
    )?;
    expect(
        &pipe(&binary, &["sum", "1", "mean", "1"], b"1\n2\n3\n")?,
        b"6\t2\n",
        "arithmetic",
    )?;
    expect(
        &pipe(&binary, &["geomean", "1"], b"1\n2\n3\n")?,
        b"1.8171205928321\n",
        "geometric mean",
    )?;

    // Keep file-backed stdin, since the original formula tested this input mode.
    fs::write("input", b"b\t2\t3\na\t1\t2\na\t3\t6\n")?;
    let output = command(&binary, &["-sg1", "pcov", "2:3"])
        .env("FASTMASH_GROUPING", "sort")
        .env("FASTMASH_SORT_TRACE", "1")
        .stdin(Stdio::from(File::open("input")?))
        .output()?;
    fs::write("trace", &output.stderr)?;
    expect(&output, b"a\t2\nb\t0\n", "paired operation")?;
    let supervisor = Path::new(bin).join("fastmash-sort-supervisor");
    match fs::symlink_metadata(supervisor) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
        Ok(_) => return Err(io::Error::other("unexpected Sort supervisor")),
    }
    if !matches!(
        output.stderr.as_slice(),
        b"sort route: native sort of the selected fields\n"
            | b"sort route: native sort of whole records\n"
    ) {
        return Err(io::Error::other("paired operation did not use native sort"));
    }
    {
        fs::write("rmdup-input", b"b\t2\na\t1\nb\t3\n")?;
        let rmdup = command(&binary, &["-s", "rmdup", "1"])
            .env("FASTMASH_SORT_TRACE", "1")
            .stdin(Stdio::from(File::open("rmdup-input")?))
            .output()?;
        expect(&rmdup, b"a\t1\nb\t2\n", "sorted rmdup")?;
        if rmdup.stderr != b"sort route: native sort of whole records\n" {
            return Err(io::Error::other(format!(
                "sorted rmdup did not use native sorting: {:?}",
                String::from_utf8_lossy(&rmdup.stderr)
            )));
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Homebrew package checks: {error}");
            ExitCode::FAILURE
        }
    }
}
