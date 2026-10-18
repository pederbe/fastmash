#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> Result<u8, Box<dyn std::error::Error>> {
    use nix::sys::signal::{Signal, kill};
    use nix::sys::stat::{SFlag, fstat};
    use nix::unistd::getpid;
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs::{self, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::OpenOptionsExt;
    use std::path::PathBuf;

    #[derive(Deserialize)]
    struct Reply {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
        status: u8,
        #[serde(default)]
        signal: bool,
    }

    #[derive(Deserialize)]
    struct Config {
        observations: Vec<u8>,
        locales: BTreeMap<String, BTreeMap<String, Reply>>,
    }

    let config: Config = serde_json::from_slice(&fs::read("punctuation-fixture.json")?)?;
    let arguments: Vec<Vec<u8>> = std::env::args_os()
        .map(|argument| argument.as_bytes().to_vec())
        .collect();
    let role = match arguments.first().map(Vec::as_slice) {
        Some(b"fastmash") => "candidate",
        Some(b"datamash") => "reference",
        _ => return Err("fixture requires a declared comparison argv[0]".into()),
    };
    let stdin = std::io::stdin();
    if !SFlag::from_bits_truncate(fstat(&stdin)?.st_mode).contains(SFlag::S_IFIFO) {
        return Err("punctuation fixture requires pipe input".into());
    }
    let mut input = Vec::new();
    stdin.lock().read_to_end(&mut input)?;
    let environment: BTreeMap<String, Vec<u8>> = std::env::vars_os()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.as_bytes().to_vec(),
            )
        })
        .collect();
    let observed = json!({
        "role": role,
        "arguments": arguments,
        "environment": environment,
        "working_directory": std::env::current_dir()?.as_os_str().as_bytes(),
        "input": input,
        "input_kind": "pipe",
        "eof": true,
    });
    let observations = PathBuf::from(OsString::from_vec(config.observations));
    let mut output = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .open(observations)?;
    serde_json::to_writer(&mut output, &observed)?;
    output.write_all(b"\n")?;
    let locale = std::env::var("LC_ALL")?;
    let reply = config
        .locales
        .get(&locale)
        .and_then(|roles| roles.get(role))
        .ok_or("fixture has no supplied reply for this locale and role")?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&reply.stdout)?;
    stdout.flush()?;
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&reply.stderr)?;
    stderr.flush()?;
    if reply.signal {
        kill(getpid(), Signal::SIGTERM)?;
        return Err("punctuation fixture remained alive after SIGTERM".into());
    }
    Ok(reply.status)
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    match run() {
        Ok(status) => std::process::ExitCode::from(status),
        Err(error) => {
            eprintln!("punctuation fixture failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
