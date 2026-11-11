//! Verify native archive build identities and deployment metadata.
#![forbid(unsafe_code)]

use fastmash_publication_test_server::bytes::same;
use std::path::Path;
use std::process::ExitCode;

fn deployment(metadata: &str) -> bool {
    metadata.split('\n').any(|line| {
        let Some(rest) = line
            .trim_start_matches(|ch: char| ch.is_ascii_whitespace())
            .strip_prefix("minos ")
        else {
            return false;
        };
        matches!(rest, "15.0" | "15.0.0")
    })
}

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && matches!(args[0].to_str(), Some("--help" | "-h")) {
        println!(
            "Check archive executable identity or recorded macOS deployment metadata.\n\n\
Usage: fastmash-check-archive bytes ORIGINAL COPY\n\
       fastmash-check-archive deployment VTOOL_OUTPUT\n\n\
bytes compares the contents of two executable files. deployment checks literal\n\
vtool output text for the macOS 15 deployment target, not a metadata file path.\n\
Successful checks are silent. A failed check exits with status 1.\n\n\
Examples, replacing executable paths with the archive's programs:\n\
  fastmash-check-archive bytes /path/to/original /path/to/extracted/fastmash\n\
  fastmash-check-archive deployment \"$(xcrun vtool -show-build /path/to/fastmash)\"\n\n\
Guide: https://github.com/pederbe/fastmash/blob/main/RELEASING.md"
        );
        return ExitCode::SUCCESS;
    }
    let error = match args.as_slice() {
        [mode, left, right] if mode == "bytes" => match same(Path::new(left), Path::new(right)) {
            Ok(true) => return ExitCode::SUCCESS,
            Ok(false) => format!(
                "archive executable bytes differ: {} and {}",
                Path::new(left).display(),
                Path::new(right).display()
            ),
            Err(error) => format!(
                "cannot compare archive executables {} and {}: {error}",
                Path::new(left).display(),
                Path::new(right).display()
            ),
        },
        [mode, metadata] if mode == "deployment" => {
            if metadata.to_str().is_some_and(deployment) {
                return ExitCode::SUCCESS;
            }
            "the binary does not record the macOS 15 deployment target".to_owned()
        }
        _ => {
            "usage: fastmash-check-archive bytes ORIGINAL COPY | deployment VTOOL_OUTPUT".to_owned()
        }
    };
    eprintln!("{error}");
    ExitCode::FAILURE
}
