//! Command outcomes and transport checks for Linux source-package assertions.
#![forbid(unsafe_code)]

use std::fs::File;
use std::io::{self, Read};
use std::os::fd::AsFd;
use std::process::ExitCode;

fn main() -> ExitCode {
    let executable = std::env::current_exe().unwrap();
    let setting = std::fs::read_to_string(executable.parent().unwrap().join("mode")).unwrap();
    let (recipe, mode) = setting.split_once(':').unwrap();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("fastmash fixture");
        return if mode == "failed-version" {
            ExitCode::from(17)
        } else {
            ExitCode::SUCCESS
        };
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
        let stdin = File::from(io::stdin().as_fd().try_clone_to_owned().unwrap());
        let path = std::env::var_os("PATH");
        if input != b"b\t2\t3\na\t1\t2\na\t3\t6\n"
            || stdin.metadata().unwrap().is_file()
            || std::env::var("LC_ALL").as_deref() != Ok("C")
            || std::env::var("FASTMASH_GROUPING").as_deref() != Ok("sort")
            || std::env::var("FASTMASH_SORT_TRACE").as_deref() != Ok("1")
            || std::env::var_os("SOURCE_PACKAGE_SHOULD_BE_CLEARED").is_some()
            || if recipe == "fedora" {
                path.as_deref() != Some(std::ffi::OsStr::new("/usr/bin:/bin"))
            } else {
                path.is_some()
            }
        {
            return ExitCode::from(20);
        }
        print!(
            "{}",
            if mode == "wrong-paired" {
                "a\t3\nb\t0\n"
            } else {
                "a\t2\nb\t0\n"
            }
        );
        eprintln!(
            "sort route: {} sort",
            if mode == "wrong-route" {
                "native"
            } else {
                "system"
            }
        );
        if mode == "failed-route" {
            return ExitCode::from(17);
        }
    } else {
        return ExitCode::from(19);
    }
    ExitCode::SUCCESS
}
