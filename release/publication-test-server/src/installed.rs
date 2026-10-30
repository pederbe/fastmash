//! The same qualified-byte and command oracle runs locally or through engine exec.
use crate::receipt::{Receipts, successful};
use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;
use std::process::{Command, Output};

pub enum Target<'a> {
    Host,
    Container { engine: &'a OsStr, id: &'a str },
}

impl Target<'_> {
    pub(crate) fn command(&self, program: impl AsRef<OsStr>) -> Command {
        match self {
            Self::Host => {
                let mut command = Command::new(program);
                command.env("LC_ALL", "C");
                command
            }
            Self::Container { engine, id } => {
                let mut command = Command::new(engine);
                command
                    .args(["exec", "-i", "--env", "LC_ALL=C", id])
                    .arg(program);
                command
            }
        }
    }
}

fn exact(output: &Output, stdout: &[u8], stderr: &[u8], label: &str) -> io::Result<()> {
    successful(output, label)?;
    if output.stdout != stdout || output.stderr != stderr {
        return Err(io::Error::other(format!(
            "unexpected installed output: {label}"
        )));
    }
    Ok(())
}

/// The Sort route a check expects its Command to report.
#[derive(Clone, Copy, PartialEq)]
enum Route {
    /// None: the check does not ask for a route.
    Untraced,
    /// The single executable performs sorting itself.
    Native,
    /// Older macOS archives sort duplicate removal natively without a trace.
    NativeDuplicateRemoval,
    /// A calculation: native in current pairs, the system sort in older ones.
    Calculation,
    /// Sorted rmdup: the system sort through the installed supervisor. Older
    /// pairs report no route for it; their calculation check proves the
    /// supervisor instead.
    SystemSort,
}

fn check(
    target: &Target<'_>,
    bin: &Path,
    args: &[&str],
    input: &[u8],
    expected: &[u8],
    route: Route,
    receipts: &mut Receipts,
) -> io::Result<bool> {
    let mut command = target.command("env");
    command.args(["-i", "PATH=/usr/bin:/bin", "LC_ALL=C"]);
    if route != Route::Untraced {
        command.args(["FASTMASH_GROUPING=sort", "FASTMASH_SORT_TRACE=1"]);
    }
    command.arg(bin.join("fastmash")).args(args);
    let output = receipts.run(args[0].trim_start_matches('-'), &mut command, input)?;
    let native = matches!(
        output.stderr.as_slice(),
        b"sort route: native sort of the selected fields\n"
            | b"sort route: native sort of whole records\n"
    );
    exact(
        &output,
        expected,
        match route {
            Route::Untraced => b"",
            Route::NativeDuplicateRemoval if output.stderr.is_empty() => b"",
            Route::NativeDuplicateRemoval if native => &output.stderr,
            Route::NativeDuplicateRemoval => b"sort route: native sort of whole records\n",
            Route::Native if native => &output.stderr,
            Route::Native => b"sort route: native sort of whole records\n",
            // Older published pairs use the external route. Admit either
            // generation while still checking the complete diagnostic.
            Route::Calculation if native => &output.stderr,
            Route::SystemSort if output.stderr.is_empty() => b"",
            Route::Calculation | Route::SystemSort => b"sort route: system sort\n",
        },
        args[0],
    )?;
    Ok(output.stderr == b"sort route: system sort\n")
}

pub(crate) fn verify_with_receipts(
    target: Target<'_>,
    version: &OsStr,
    bin: &Path,
    checksums: &Path,
    receipts: &mut Receipts,
) -> io::Result<()> {
    let platform = receipts.run("platform", target.command("uname").arg("-s"), b"")?;
    successful(&platform, "release platform")?;
    // Frozen Linux releases contain two executables. The supplied identity
    // manifest selects that legacy layout without making it the new default.
    let manifest = receipts.run(
        "checksum-manifest",
        target.command("cat").arg(checksums),
        b"",
    )?;
    successful(&manifest, "release checksum manifest")?;
    let legacy = manifest
        .stdout
        .split(|byte| *byte == b'\n')
        .any(|line| line.ends_with(b" fastmash-sort-supervisor"));
    let (programs, checksum): (&[&str], &[&str]) = match platform.stdout.as_slice() {
        b"Linux\n" if legacy => (&["fastmash", "fastmash-sort-supervisor"], &["sha256sum"]),
        b"Linux\n" => (&["fastmash"], &["sha256sum"]),
        b"Darwin\n" => (&["fastmash"], &["shasum", "-a", "256"]),
        _ => return Err(io::Error::other("unsupported release platform")),
    };
    for program in programs {
        let output = receipts.run(
            "executable",
            target.command("test").arg("-x").arg(bin.join(program)),
            b"",
        )?;
        if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
            return Err(io::Error::other(format!(
                "{program} is not installed and executable"
            )));
        }
        successful(&output, "installed executable admission")?;
    }
    // env --chdir is not available on every supported native platform. The
    // fixed shell only changes directory and invokes the native checksum tool.
    let mut checksum_command = target.command("bash");
    checksum_command
        .args([
            "-c",
            "cd -- \"$1\" && shift && exec \"$@\"",
            "check-installed",
        ])
        .arg(bin)
        .args(checksum)
        .arg("-c")
        .arg(checksums);
    let checked = receipts.run("checksums", &mut checksum_command, b"")?;
    io::stdout().write_all(&checked.stdout)?;
    io::stderr().write_all(&checked.stderr)?;
    successful(&checked, "qualified checksums")?;
    let mut expected_version = OsString::from("fastmash ");
    expected_version.push(version);
    expected_version.push("\n");
    check(
        &target,
        bin,
        &["--version"],
        b"",
        expected_version.as_encoded_bytes(),
        Route::Untraced,
        receipts,
    )?;
    check(
        &target,
        bin,
        &["sum", "1", "mean", "2"],
        b"1\t2\n3\t4\n",
        b"4\t3\n",
        Route::Untraced,
        receipts,
    )?;
    check(
        &target,
        bin,
        &["median", "1"],
        b"1\n2\n3\n",
        b"2\n",
        Route::Untraced,
        receipts,
    )?;
    let mut supervised = false;
    if programs.len() == 1 {
        let output = receipts.run(
            "no-supervisor",
            target
                .command("test")
                .arg("!")
                .arg("-e")
                .arg(bin.join("fastmash-sort-supervisor")),
            b"",
        )?;
        successful(&output, "native archive must not contain a supervisor")?;
    }
    if programs.len() == 2 {
        // Calculations no longer exercise the Linux supervisor. Sorted rmdup
        // still does, so check that the installed pair runs the system sort.
        supervised = check(
            &target,
            bin,
            &["-s", "rmdup", "1"],
            b"b\t2\na\t1\nb\t3\n",
            b"a\t1\nb\t2\n",
            Route::SystemSort,
            receipts,
        )?;
    } else {
        check(
            &target,
            bin,
            &["-s", "rmdup", "1"],
            b"b\t2\na\t1\nb\t3\n",
            b"a\t1\nb\t2\n",
            if platform.stdout == b"Darwin\n" {
                Route::NativeDuplicateRemoval
            } else {
                Route::Native
            },
            receipts,
        )?;
    }
    supervised |= check(
        &target,
        bin,
        &["-sg1", "pcov", "2:3"],
        b"b\t2\t3\na\t1\t2\na\t3\t6\n",
        b"a\t2\nb\t0\n",
        if programs.len() == 2 {
            Route::Calculation
        } else {
            Route::Native
        },
        receipts,
    )?;
    if programs.len() == 2 && !supervised {
        return Err(io::Error::other(
            "no Command used the system sort through the installed supervisor",
        ));
    }
    Ok(())
}

pub fn verify(target: Target<'_>, version: &OsStr, bin: &Path, checksums: &Path) -> io::Result<()> {
    verify_with_receipts(target, version, bin, checksums, &mut Receipts::from_env()?)
}

#[cfg(test)]
mod tests {
    use super::exact;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    #[test]
    fn installed_output_requires_status_and_both_exact_streams() {
        let mut output = Output {
            status: ExitStatus::from_raw(0),
            stdout: b"a\t2\nb\t0\n".to_vec(),
            stderr: b"sort route: system sort\n".to_vec(),
        };
        assert!(
            exact(
                &output,
                b"a\t2\nb\t0\n",
                b"sort route: system sort\n",
                "covariance"
            )
            .is_ok()
        );
        output.status = ExitStatus::from_raw(1 << 8);
        assert!(exact(&output, &output.stdout, &output.stderr, "covariance").is_err());
        output.status = ExitStatus::from_raw(0);
        assert!(exact(&output, b"a\t2\nb\t0\n", b"", "covariance").is_err());
        assert!(exact(&output, b"a\t2\nb\t0", &output.stderr, "covariance").is_err());
    }
}
