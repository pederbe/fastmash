//! Linux-only layout fixture for the existing Darwin archive-layout tests.
//! Native macOS execution remains a separate workflow check.
#![forbid(unsafe_code)]

fn main() -> std::process::ExitCode {
    use std::ffi::OsStr;
    let args: Vec<_> = std::env::args_os().collect();
    match std::path::Path::new(&args[0])
        .file_name()
        .and_then(OsStr::to_str)
    {
        Some("uname") if args.get(1).is_some_and(|arg| arg == "-s") && args.len() == 2 => {
            println!("Darwin");
            std::process::ExitCode::SUCCESS
        }
        Some("shasum")
            if args.get(1).is_some_and(|arg| arg == "-a")
                && args.get(2).is_some_and(|arg| arg == "256") =>
        {
            match std::process::Command::new("/usr/bin/sha256sum")
                .args(&args[3..])
                .status()
            {
                Ok(status) if status.success() => std::process::ExitCode::SUCCESS,
                Ok(_) => std::process::ExitCode::FAILURE,
                Err(error) => {
                    eprintln!("layout checksum fixture: {error}");
                    std::process::ExitCode::FAILURE
                }
            }
        }
        _ => {
            eprintln!("expected a uname or shasum layout-fixture invocation");
            std::process::ExitCode::FAILURE
        }
    }
}
