#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> Result<u8, Box<dyn std::error::Error>> {
    use nix::sys::signal::{Signal, kill};
    use nix::sys::stat::{SFlag, fstat};
    use nix::unistd::{close, getpid};
    use std::fs;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::Duration;

    let mut args = std::env::args_os().skip(1);
    let mode = args.next().ok_or("capture fixture requires a mode")?;
    if mode == "nonreading" {
        std::thread::sleep(Duration::from_secs(10));
        return Ok(0);
    }
    if mode == "descendant" || mode == "orphan" {
        let pidfile = args.next().ok_or("descendant mode requires a PID file")?;
        let mut child = Command::new(std::env::current_exe()?)
            .arg("nonreading")
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        fs::write(pidfile, child.id().to_string())?;
        if mode == "descendant" {
            let status = child.wait()?;
            return Ok(u8::try_from(status.code().unwrap_or(1))?);
        }
        return Ok(0);
    }
    if mode == "early-close" || mode == "signal" {
        close(0)?;
        std::io::stdout().write_all(b"\xffearly\n")?;
        std::io::stderr().write_all(b"\x80closed\n")?;
        if mode == "signal" {
            kill(getpid(), Signal::SIGTERM)?;
            return Err("fixture remained alive after SIGTERM".into());
        }
        return Ok(17);
    }
    if mode != "echo" && mode != "slow-echo" && mode != "null" {
        return Err("unknown capture fixture mode".into());
    }
    let stdin = std::io::stdin();
    let kind = SFlag::from_bits_truncate(fstat(&stdin)?.st_mode);
    if mode != "null" && !kind.contains(SFlag::S_IFIFO) {
        return Err("capture fixture requires pipe input".into());
    }
    if mode == "slow-echo" {
        // Let the parent fill the pipe before consuming any bytes.
        std::thread::sleep(Duration::from_millis(100));
    }
    let mut input = stdin.lock();
    let mut output = std::io::stdout().lock();
    let mut buffer = [0; 8192];
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        output.write_all(&buffer[..length])?;
        if mode == "slow-echo" {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    output.flush()?;
    std::io::stderr().write_all(b"pipe-eof\n")?;
    Ok(0)
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    match run() {
        Ok(status) => std::process::ExitCode::from(status),
        Err(error) => {
            eprintln!("capture fixture failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    std::process::ExitCode::FAILURE
}
