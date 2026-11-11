#![forbid(unsafe_code)]

use fastmash_license_notices::{Result, Target, write_notices};
use std::{
    io,
    path::Path,
    process::{Command, ExitCode, Stdio},
};

const USAGE: &str = "Usage: fastmash-license-notices [--target TARGET]\n\n\
Targets: x86_64-unknown-linux-gnu (default), x86_64-unknown-linux-musl, aarch64-apple-darwin";

fn arguments() -> Result<Option<Target>> {
    let mut target = Target::Linux;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => {
                println!(
                    "Write third-party license notices as Markdown on standard output.\n\n{USAGE}\n\n\
Uses Cargo's locked, target-filtered normal dependency graph for this checkout.\n\
Choose the target of the archive you are preparing. Diagnostics go to stderr.\n\
Check the exit status before using redirected output, which may be incomplete on failure.\n\n\
Example, writing notices for the Linux musl archive:\n\
  fastmash-license-notices --target x86_64-unknown-linux-musl > THIRD-PARTY-LICENSES.md\n\n\
Release guide: https://github.com/pederbe/fastmash/blob/main/RELEASING.md"
                );
                return Ok(None);
            }
            "--target" => {
                target = Target::parse(&arguments.next().ok_or("--target needs a value")?)?;
            }
            value if value.starts_with("--target=") => target = Target::parse(&value[9..])?,
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    Ok(Some(target))
}

fn generate(target: Target) -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--filter-platform",
            target.triple(),
        ])
        .current_dir(root)
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        return Err(format!("cargo metadata failed: {}", output.status).into());
    }
    let metadata = serde_json::from_slice(&output.stdout)?;
    write_notices(&metadata, target, &mut io::stdout().lock())
}

fn main() -> ExitCode {
    let target = match arguments() {
        Ok(Some(target)) => target,
        Ok(None) => return ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match generate(target) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
