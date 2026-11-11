//! Check installed archive bytes and commands without compiling inside a client.
#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> std::io::Result<()> {
    use fastmash_publication_test_server::installed::{Target, verify};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && matches!(args[0].to_str(), Some("--help" | "-h")) {
        println!(
            "Check installed archive bytes, version and representative commands.\n\n\
Usage: fastmash-check-installed VERSION BIN_DIR QUALIFIED_CHECKSUMS\n\n\
BIN_DIR contains the installed executable. QUALIFIED_CHECKSUMS is its independently\n\
supplied checksum manifest. VERSION is the expected executable version number.\n\
Native checksum results go to stdout. Other checks are silent on success.\n\
Set INSTALLATION_TEST_EVIDENCE to a new directory to retain raw command receipts.\n\n\
Example, replacing VERSION and installed paths with the candidate being checked:\n\
  fastmash-check-installed VERSION /absolute/path/to/installed/bin release/delivery.sha256\n\n\
Guide: https://github.com/pederbe/fastmash/blob/main/RELEASING.md"
        );
        return Ok(());
    }
    if args.len() != 3 {
        return Err(std::io::Error::other(
            "usage: fastmash-check-installed VERSION BIN_DIR QUALIFIED_CHECKSUMS",
        ));
    }
    let bin = std::fs::canonicalize(&args[1]).map_err(|error| {
        std::io::Error::other(format!(
            "cannot open installed binary directory {}: {error}",
            std::path::Path::new(&args[1]).display()
        ))
    })?;
    let checksums = std::fs::canonicalize(&args[2]).map_err(|error| {
        std::io::Error::other(format!(
            "cannot open qualified checksum manifest {}: {error}",
            std::path::Path::new(&args[2]).display()
        ))
    })?;
    verify(Target::Host, &args[0], &bin, &checksums)
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("installed checks: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("installed checks require Linux or macOS");
        std::process::ExitCode::FAILURE
    }
}
