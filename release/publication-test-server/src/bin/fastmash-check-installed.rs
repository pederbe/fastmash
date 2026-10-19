//! Check installed archive bytes and commands without compiling inside a client.
#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> std::io::Result<()> {
    use fastmash_publication_test_server::installed::{Target, verify};
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(std::io::Error::other(
            "usage: check-installed.sh VERSION BIN_DIR QUALIFIED_CHECKSUMS",
        ));
    }
    let bin = std::fs::canonicalize(&args[1])?;
    let checksums = std::fs::canonicalize(&args[2])?;
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
