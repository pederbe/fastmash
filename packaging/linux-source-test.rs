//! Installed checks for the retained Fedora source recipe.
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
    if args.len() == 1 && matches!(args[0].to_str(), Some("--help" | "-h")) {
        println!(
            "Check the Fastmash executable installed by a Fedora source recipe.\n\n\
Usage: fastmash-test-linux-source fedora BIN_DIR VERSION\n\
\n\
BIN_DIR contains the installed executable. VERSION is its expected version number.\n\
Successful checks are silent.\n\n\
Example, replacing VERSION and installed directories with the recipe's outputs:\n\
  fastmash-test-linux-source fedora /path/to/bin VERSION"
        );
        return Ok(());
    }
    let (bin, version) = match args.as_slice() {
        [mode, bin, version] if mode == "fedora" => (bin, version),
        _ => {
            return Err(io::Error::other(
                "usage: fastmash-test-linux-source fedora BIN_DIR VERSION",
            ));
        }
    };
    let bin = Path::new(bin);
    let binary = bin.join("fastmash");
    let metadata = fs::metadata(&binary).map_err(|error| {
        io::Error::other(format!(
            "cannot inspect installed executable {}: {error}",
            binary.display()
        ))
    })?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(io::Error::other(format!(
            "installed program is not an executable file: {}",
            binary.display()
        )));
    }
    let version = version
        .to_str()
        .ok_or_else(|| io::Error::other("version must be UTF-8"))?;
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
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("FASTMASH_GROUPING", "sort")
        .env("FASTMASH_SORT_TRACE", "1");
    let output = pipe(route, b"b\t2\t3\na\t1\t2\na\t3\t6\n")?;
    expect(&output, b"a\t2\nb\t0\n", "paired operation")?;
    if !matches!(
        output.stderr.as_slice(),
        b"sort route: native sort of the selected fields\n"
            | b"sort route: native sort of whole records\n"
    ) {
        return Err(io::Error::other(format!(
            "paired operation did not report a supported sort route: {:?}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let mut rmdup = Command::new(&binary);
    rmdup
        .args(["-s", "rmdup", "1"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("FASTMASH_SORT_TRACE", "1");
    let rmdup = pipe(rmdup, b"b\t2\na\t1\nb\t3\n")?;
    expect(&rmdup, b"a\t1\nb\t2\n", "sorted rmdup")?;
    if rmdup.stderr != b"sort route: native sort of whole records\n" {
        return Err(io::Error::other(format!(
            "sorted rmdup did not use native sorting: {:?}",
            String::from_utf8_lossy(&rmdup.stderr)
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
