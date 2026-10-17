#![forbid(unsafe_code)]

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::os::unix::ffi::OsStrExt;
    fn observe() -> Result<(), Box<dyn std::error::Error>> {
        let mut input = Vec::new();
        std::io::stdin().read_to_end(&mut input)?;
        let arguments: Vec<Vec<u8>> = std::env::args_os()
            .skip(1)
            .map(|arg| arg.as_bytes().to_vec())
            .collect();
        let environment: BTreeMap<String, Vec<u8>> = std::env::vars_os()
            .map(|(key, value)| {
                (
                    key.to_string_lossy().into_owned(),
                    value.as_bytes().to_vec(),
                )
            })
            .collect();
        let observed = serde_json::to_vec(&serde_json::json!({
            "argv0": std::env::args().next(), "arguments": arguments,
            "environment": environment, "input": input,
            "cwd": std::env::current_dir()?.as_os_str().as_bytes(),
        }))?;
        std::io::stdout().write_all(b"\xffobserved\n")?;
        let mut stderr = std::io::stderr();
        stderr.write_all(b"\xff")?;
        stderr.write_all(&observed)?;
        stderr.write_all(b"\x80")?;
        Ok(())
    }
    if let Err(error) = observe() {
        eprintln!("fixture observation failed: {error}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::from(77)
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
