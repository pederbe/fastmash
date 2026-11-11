#![forbid(unsafe_code)]

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use fastmash_locale_tools::native::{USAGE, generate, parse_args};
    use std::process::ExitCode;

    let config = match parse_args(std::env::args_os().skip(1)) {
        Ok(Some(config)) => config,
        Ok(None) => {
            println!(
                "Generate numeric locale JSON and Rust tables from an identified glibc reference.\n\n{USAGE}\n\n\
LOCPATH is a tree of compiled UTF-8 locales. Tool paths default to /usr/bin/ldd,\n\
/usr/bin/locale and /usr/bin/uname. Run with matching reference tools and locale data.\n\
OUT_JSON and OUT_RS are overwritten. Keep them separate from committed product tables.\n\
The summary counts locales with one-byte decimal separators and excluded locales.\n\n\
Example, with paths to your reference environment:\n\
  fastmash-generate-numeric-locales --ldd /path/to/reference/bin/ldd \\\n    --locale /path/to/reference/bin/locale --uname /path/to/reference/bin/uname \\\n    -- /path/to/compiled-locales /tmp/fastmash-numeric.json /tmp/fastmash-numeric.rs\n\n\
Guide: https://fastmash.io/contributing/testing.html#locale-tools"
            );
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
