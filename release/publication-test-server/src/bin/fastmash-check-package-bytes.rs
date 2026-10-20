//! Check staged and extracted package programs against the qualified inputs.
#![forbid(unsafe_code)]

use fastmash_publication_test_server::bytes::same;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: fastmash-check-package-bytes BIN_DIR PACKAGE_ROOT");
        return ExitCode::FAILURE;
    }
    let bin = Path::new(&args[0]);
    let packaged = Path::new(&args[1]).join("usr/bin");
    for program in ["fastmash", "fastmash-sort-supervisor"] {
        if !matches!(same(&bin.join(program), &packaged.join(program)), Ok(true)) {
            eprintln!("{program} changed during packaging");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
