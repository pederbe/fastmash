//! Deliberately incorrect command outcomes for the package smoke checker.
#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::os::fd::AsFd;
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
        println!(
            "{}",
            if mode == "wrong-geomean" {
                "incorrect"
            } else {
                "1.8171205928321"
            }
        );
    } else if args == ["-sg1", "pcov", "2:3"] {
        if input != b"b\t2\t3\na\t1\t2\na\t3\t6\n"
            || std::env::var("LC_ALL").as_deref() != Ok("C")
            || std::env::var("FASTMASH_GROUPING").as_deref() != Ok("sort")
            || std::env::var("FASTMASH_SORT_TRACE").as_deref() != Ok("1")
            || !std::fs::File::from(io::stdin().as_fd().try_clone_to_owned().unwrap())
                .metadata()
                .unwrap()
                .is_file()
        {
            return ExitCode::from(20);
        }
        print!(
            "{}",
            if mode == "wrong-paired-output" {
                "a\t3\nb\t0\n"
            } else {
                "a\t2\nb\t0\n"
            }
        );
        let native = cfg!(target_os = "macos") != (mode == "wrong-route");
        eprintln!(
            "sort route: {} sort",
            if native { "native" } else { "system" }
        );
        if mode == "failed-route" {
            return ExitCode::from(17);
        }
    } else if args == ["-s", "rmdup", "1"] {
        if input != b"b\t2\na\t1\nb\t3\n" {
            return ExitCode::from(20);
        }
        print!("a\t1\nb\t2\n");
    } else {
        return ExitCode::from(19);
    }
    ExitCode::SUCCESS
}
