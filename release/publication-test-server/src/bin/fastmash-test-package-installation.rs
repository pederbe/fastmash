//! Host orchestration for actual isolated package installation and removal.
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match fastmash_publication_test_server::packages::run(
        &std::env::args_os().skip(1).collect::<Vec<_>>(),
    ) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("package installation checks: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("package installation checks require native Unix package clients");
        std::process::ExitCode::FAILURE
    }
}
