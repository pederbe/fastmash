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
    let error = match args.as_slice() {
        [mode, left, right] if mode == "bytes" => {
            if matches!(same(Path::new(left), Path::new(right)), Ok(true)) {
                return ExitCode::SUCCESS;
            }
            "archive executable bytes differ or could not be read"
        }
        [mode, metadata] if mode == "deployment" => {
            if metadata.to_str().is_some_and(deployment) {
                return ExitCode::SUCCESS;
            }
            "the binary does not record the macOS 15 deployment target"
        }
        _ => "usage: fastmash-check-archive bytes ORIGINAL COPY | deployment VTOOL_OUTPUT",
    };
    eprintln!("{error}");
    ExitCode::FAILURE
}
