#![forbid(unsafe_code)]

#[cfg(unix)]
fn observe() -> Result<u8, Box<dyn std::error::Error>> {
    use serde::Deserialize;
    use serde_json::json;
    use std::collections::BTreeMap;
    use std::ffi::OsString;
    use std::fs::{self, OpenOptions};
    use std::io::{Read, Write};
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::path::{Path, PathBuf};

    #[derive(Deserialize)]
    struct Reply {
        stderr: Vec<u8>,
        status: u8,
    }

    #[derive(Deserialize)]
    struct Config {
        observations: Vec<u8>,
        // Only alphabet and digit ordinals can override a role's stderr/status.
        #[serde(default)]
        replies: BTreeMap<String, [Option<Reply>; 2]>,
    }

    fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?
            .write_all(bytes)
    }

    let config: Config = serde_json::from_slice(&fs::read("observer-config.json")?)?;
    let directory = PathBuf::from(OsString::from_vec(config.observations));
    if !directory.is_absolute() {
        return Err("observer observations path must be absolute".into());
    }
    let arguments: Vec<Vec<u8>> = std::env::args_os()
        .map(|arg| arg.as_bytes().to_vec())
        .collect();
    let role = match arguments.first().map(Vec::as_slice) {
        Some(b"fastmash") => "candidate",
        Some(b"datamash") => "reference",
        _ => return Err("observer requires the declared candidate or reference argv[0]".into()),
    };
    // Each selected legacy mode has one locale worker and calls each role
    // sequentially. Its private counters therefore need no locking protocol.
    let counter = directory.join(format!("{role}.count"));
    let index: usize = match fs::read_to_string(&counter) {
        Ok(raw) => raw.trim_end_matches('\n').parse()?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => return Err(error.into()),
    };
    let reply = config
        .replies
        .get(role)
        .and_then(|replies| replies.get(index))
        .and_then(Option::as_ref);
    let mut input = Vec::new();
    std::io::stdin().read_to_end(&mut input)?;
    let mut output = Vec::new();
    for line in input.split_inclusive(|byte| *byte == b'\n') {
        let word = line
            .strip_suffix(b"\n")
            .ok_or("observer requires LF-terminated words")?;
        output.extend_from_slice(word);
        output.extend_from_slice(b"\t1\n");
    }
    let mut environment: Vec<(Vec<u8>, Vec<u8>)> = std::env::vars_os()
        .map(|(key, value)| (key.as_bytes().to_vec(), value.as_bytes().to_vec()))
        .collect();
    environment.sort();
    let mut receipt = json!({
        "role": role,
        "index": index,
        "arguments": arguments,
        "environment": environment,
        "working_directory": std::env::current_dir()?.as_os_str().as_bytes(),
        "input_bytes": input.len(),
        "stdout_bytes": output.len(),
    });
    if let Some(reply) = reply {
        receipt["configured_reply"] = json!({
            "stderr_bytes": reply.stderr.len(),
            "status": reply.status,
        });
    }
    let prefix = format!("{role}-{index:02}");
    write_new(&directory.join(format!("{prefix}.stdin")), &input)?;
    write_new(&directory.join(format!("{prefix}.stdout")), &output)?;
    if let Some(reply) = reply {
        write_new(&directory.join(format!("{prefix}.stderr")), &reply.stderr)?;
    }
    write_new(
        &directory.join(format!("{prefix}.json")),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output)?;
    stdout.flush()?;
    if let Some(reply) = reply {
        let mut stderr = std::io::stderr().lock();
        stderr.write_all(&reply.stderr)?;
        stderr.flush()?;
    }
    fs::write(counter, format!("{}\n", index + 1))?;
    Ok(reply.map_or(0, |reply| reply.status))
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    match observe() {
        Ok(status) => std::process::ExitCode::from(status),
        Err(error) => {
            eprintln!("collation word observer failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
