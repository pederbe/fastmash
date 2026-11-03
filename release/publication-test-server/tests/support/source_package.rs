//! Deliberately incorrect command outcomes for the package smoke checker.
#![forbid(unsafe_code)]

use std::io::{self, Read};
use std::os::fd::AsFd;
use std::process::ExitCode;

fn main() -> ExitCode {
    let raw_args: Vec<_> = std::env::args_os().skip(1).collect();
    if raw_args
        .first()
        .is_some_and(|arg| arg == "--copy-executable")
    {
        let [_, source, destination] = raw_args.as_slice() else {
            eprintln!("copy requires source and destination");
            return ExitCode::from(2);
        };
        return match std::fs::copy(source, destination) {
            Ok(_) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("copy {source:?} to {destination:?}: {error}");
                ExitCode::from(2)
            }
        };
    }
    let args: Vec<_> = raw_args
        .into_iter()
        .map(|arg| arg.into_string().unwrap())
        .collect();
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
        let native = mode != "wrong-route";
        eprintln!(
            "sort route: {}",
            if native {
                "native sort of the selected fields"
            } else {
                "system sort"
            }
        );
        if mode == "failed-route" {
            return ExitCode::from(17);
        }
    } else if args == ["-s", "rmdup", "1"] {
        if input != b"b\t2\na\t1\nb\t3\n" {
            return ExitCode::from(20);
        }
        print!("a\t1\nb\t2\n");
        eprintln!("sort route: native sort of whole records");
    } else {
        return ExitCode::from(19);
    }
    ExitCode::SUCCESS
}
