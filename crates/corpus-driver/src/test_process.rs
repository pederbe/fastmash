#![forbid(unsafe_code)]
//! Child-only native stream setup, followed by exec in the same process.
use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
use nix::unistd::{alarm, dup2_stdout, execve, pipe, write};
use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::process::Command;

const BLOCKED: &str = "FASTMASH_TEST_PROCESS_BLOCKED";
const IGNORE_PIPE: &str = "FASTMASH_TEST_PROCESS_IGNORE_PIPE";
const BROKEN_OUTPUT: &str = "FASTMASH_TEST_PROCESS_BROKEN_OUTPUT";

fn selected(name: &str) -> Result<bool, Box<dyn std::error::Error>> {
    match std::env::var_os(name) {
        None => Ok(false),
        Some(value) if value == "1" => Ok(true),
        _ => Err("invalid test-process switch".into()),
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let seconds: u32 = args
        .next()
        .ok_or("test-process alarm is required")?
        .to_str()
        .ok_or("invalid test-process alarm")?
        .parse()?;
    if !(1..=30).contains(&seconds) {
        return Err("test-process alarm must be between 1 and 30 seconds".into());
    }
    let binary = args.next().ok_or("test-process executable is required")?;
    let arguments: Vec<OsString> = args.collect();
    let ignored = selected(IGNORE_PIPE)?;
    let broken = selected(BROKEN_OUTPUT)?;
    let mut mask = SigSet::empty();
    if let Some(value) = std::env::var_os(BLOCKED) {
        let signal = Signal::try_from(
            value
                .to_str()
                .ok_or("invalid blocked signal")?
                .parse::<i32>()?,
        )?;
        if !matches!(
            signal,
            Signal::SIGPIPE | Signal::SIGINT | Signal::SIGTERM | Signal::SIGHUP
        ) {
            return Err("unsupported test-process blocked signal".into());
        }
        mask.add(signal);
    }
    pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&mask), None)?;
    alarm::set(seconds);
    if broken {
        // This fresh process owns both endpoints. No other test can retain a
        // reader, and the real failed write calibrates Rust's ignored SIGPIPE.
        let (reader, writer) = pipe()?;
        drop(reader);
        if write(&writer, b"x") != Err(nix::errno::Errno::EPIPE) {
            return Err("test-process broken pipe calibration failed".into());
        }
        dup2_stdout(&writer)?;
        drop(writer);
    }
    if ignored {
        // Rust starts with SIGPIPE ignored. A direct safe POSIX exec preserves
        // that disposition. Command::exec instead restores the normal default.
        let executable = CString::new(binary.as_bytes())?;
        let mut argv = vec![CString::new("fastmash")?];
        for arg in arguments {
            argv.push(CString::new(arg.as_bytes())?);
        }
        let mut environment = Vec::new();
        for (key, value) in std::env::vars_os() {
            if [BLOCKED, IGNORE_PIPE, BROKEN_OUTPUT]
                .iter()
                .any(|name| key == *name)
            {
                continue;
            }
            let mut entry = key.as_bytes().to_vec();
            entry.push(b'=');
            entry.extend_from_slice(value.as_bytes());
            environment.push(CString::new(entry)?);
        }
        execve(&executable, &argv, &environment)?;
        unreachable!();
    }
    let error = Command::new(binary)
        .arg0("fastmash")
        .args(arguments)
        .env_remove(BLOCKED)
        .env_remove(IGNORE_PIPE)
        .env_remove(BROKEN_OUTPUT)
        .exec();
    Err(error.into())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("test-process: {error}");
        std::process::exit(2);
    }
}
