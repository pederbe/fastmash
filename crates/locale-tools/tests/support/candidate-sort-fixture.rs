#![forbid(unsafe_code)]

use std::io::{self, Read, Write};
use std::process::ExitCode;

fn run() -> io::Result<ExitCode> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    let executable = std::env::current_exe()?;
    let mode = std::fs::read_to_string(executable.parent().unwrap().join("mode"))?;
    let locale = std::env::var("LC_ALL").unwrap();
    assert_eq!(input, b"a\nA\n");
    assert_eq!(std::env::args_os().count(), 1);
    assert_eq!(std::env::var("PATH").unwrap(), "/usr/bin:/bin");
    assert_eq!(std::env::var("TZ").unwrap(), "UTC");
    assert!(std::path::Path::new(&std::env::var_os("LOCPATH").unwrap()).is_dir());
    assert!(std::env::var_os("FASTMASH_CANDIDATE_POISON").is_none());
    if mode == "diagnostic" {
        eprintln!("reference locale failed");
    }
    let upper = locale == "upper.UTF-8" || mode == "failed";
    let output = if mode == "wrong" {
        b"unexpected\n".as_slice()
    } else if upper {
        b"A\na\n"
    } else {
        b"a\nA\n"
    };
    io::stdout().write_all(output)?;
    Ok(if mode == "failed" {
        ExitCode::from(17)
    } else {
        ExitCode::SUCCESS
    })
}

fn main() -> ExitCode {
    run().unwrap()
}
