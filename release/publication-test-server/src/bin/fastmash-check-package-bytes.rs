//! Check staged and extracted package programs against the qualified inputs.
#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::Path;
use std::process::ExitCode;

fn same(left: &Path, right: &Path) -> io::Result<bool> {
    let mut left = BufReader::new(File::open(left)?);
    let mut right = BufReader::new(File::open(right)?);
    loop {
        let left_bytes = left.fill_buf()?;
        let right_bytes = right.fill_buf()?;
        if left_bytes.is_empty() || right_bytes.is_empty() {
            return Ok(left_bytes.is_empty() && right_bytes.is_empty());
        }
        let length = left_bytes.len().min(right_bytes.len());
        if left_bytes[..length] != right_bytes[..length] {
            return Ok(false);
        }
        left.consume(length);
        right.consume(length);
    }
}

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
