//! Check staged and extracted package programs against the qualified inputs.
#![forbid(unsafe_code)]

use fastmash_publication_test_server::bytes::same;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && matches!(args[0].to_str(), Some("--help" | "-h")) {
        println!(
            "Check that packaging preserved the qualified Fastmash executable bytes.\n\n\
Usage: fastmash-check-package-bytes BIN_DIR PACKAGE_ROOT\n\n\
Compares BIN_DIR/fastmash with PACKAGE_ROOT/usr/bin/fastmash.\n\
Successful checks are silent. Changed bytes or unreadable files exit with status 1.\n\n\
Example, with the qualified binary directory and extracted package tree:\n\
  fastmash-check-package-bytes /path/to/qualified/bin /path/to/extracted-package\n\n\
Guide: https://github.com/pederbe/fastmash/blob/main/RELEASING.md"
        );
        return ExitCode::SUCCESS;
    }
    if args.len() != 2 {
        eprintln!("usage: fastmash-check-package-bytes BIN_DIR PACKAGE_ROOT");
        return ExitCode::FAILURE;
    }
    let bin = Path::new(&args[0]);
    let packaged = Path::new(&args[1]).join("usr/bin");
    let qualified = bin.join("fastmash");
    let packaged = packaged.join("fastmash");
    match same(&qualified, &packaged) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("fastmash changed during packaging");
            ExitCode::FAILURE
        }
        Err(error) => {
            eprintln!(
                "cannot compare qualified executable {} with packaged executable {}: {error}. Check that both paths contain readable files",
                qualified.display(),
                packaged.display()
            );
            ExitCode::FAILURE
        }
    }
}
