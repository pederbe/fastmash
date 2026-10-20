//! Deliberately incorrect command outcomes for the package smoke checker.
#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let mode = std::env::var("SOURCE_PACKAGE_FIXTURE").unwrap_or_default();
    if args == ["--version"] {
        println!("fastmash fixture");
        if mode == "failed-version" {
            return ExitCode::from(17);
        }
        return ExitCode::SUCCESS;
    }
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input).unwrap();
    if args == ["sum", "1", "mean", "1"] {
        if input != b"1\n2\n3\n" {
            return ExitCode::from(20);
        }
        println!(
            "{}",
            if mode == "wrong-output" {
                "incorrect"
            } else {
                "6\t2"
            }
        );
        if mode == "failed-command" {
            return ExitCode::from(17);
        }
    } else if args == ["-s", "-g", "1", "sum", "2"] {
        if input != b"b\t2\na\t1\na\t3\n" {
            return ExitCode::from(20);
        }
        print!("a\t4\nb\t2\n");
    } else if args == ["geomean", "1"] {
        if input != b"1\n2\n3\n" {
            return ExitCode::from(20);
        }
        println!("1.8171205928321");
    } else {
        return ExitCode::from(19);
    }
    ExitCode::SUCCESS
}
