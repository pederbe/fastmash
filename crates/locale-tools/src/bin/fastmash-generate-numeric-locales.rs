#![forbid(unsafe_code)]

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use fastmash_locale_tools::native::{USAGE, generate, parse_args};
    use std::process::ExitCode;

    let config = match parse_args(std::env::args_os().skip(1)) {
        Ok(Some(config)) => config,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("fastmash-generate-numeric-locales: {error}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match generate(&config) {
        Ok(summary) => {
            println!("{} locales, {} refused", summary.locales, summary.refused);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("fastmash-generate-numeric-locales: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("fastmash-generate-numeric-locales: native glibc queries require Unix");
    std::process::ExitCode::FAILURE
}
