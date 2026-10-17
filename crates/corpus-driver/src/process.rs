use crate::fixture::{Case, Expected, decode, encode};
use crate::{Error, Result};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::sys::resource::{Resource, setrlimit};
use nix::sys::signal::{SigSet, Signal, kill, killpg};
use nix::unistd::{Pid, setsid};
use serde::Serialize;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Seek, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

pub const CAP: u64 = 524_288;
const DEADLINE: Duration = Duration::from_secs(8);
pub(crate) const EXEC_MODE: &str = "__fastmash_corpus_exec";

#[derive(Debug, Serialize)]
pub struct Observed {
    pub argv: Vec<String>,
    // Hex arguments retain invalid UTF-8 without relying on a JSON encoder's
    // replacement-character display. Actual launch always uses these raw bytes.
    pub argv_hex: Vec<String>,
    pub executable: PathBuf,
    pub env: BTreeMap<String, String>,
    pub process_group: u32,
    pub returncode: i32,
    pub timed_out: bool,
    pub overflow: bool,
    pub cleanup_confirmed: bool,
    pub stdout_hex: String,
    pub stderr_hex: String,
}

impl Observed {
    pub fn matches(&self, expected: &Expected) -> bool {
        !self.timed_out
            && !self.overflow
            && self.cleanup_confirmed
            && self.returncode == expected.returncode
            && self.stdout_hex == expected.stdout_hex
            && normalize_sort_name(&self.stderr_hex) == expected.stderr_hex
    }
}

pub fn require_unblocked_signals() -> Result<()> {
    if SigSet::thread_get_mask()?.iter().next().is_some() {
        return Err(Error::message(
            "run from a shell with an unblocked signal mask",
        ));
    }
    Ok(())
}

// Run this only in the fresh internal launcher, before creating threads. Doing
// resource/session setup there avoids an unsafe pre_exec closure after fork.
pub(crate) fn exec_target(mut args: impl Iterator<Item = OsString>) -> Result<()> {
    let binary = args
        .next()
        .ok_or_else(|| Error::message("internal launch requires an executable"))?;
    let name = args
        .next()
        .ok_or_else(|| Error::message("internal launch requires argv[0]"))?;
    require_unblocked_signals()?;
    setsid()?;
    setrlimit(Resource::RLIMIT_FSIZE, CAP, CAP)?;
    let error = Command::new(binary).arg0(name).args(args).exec();
    Err(error.into())
}

pub(crate) fn invoke(
    case: &Case,
    binary: &Path,
    launcher: &Path,
    directory: &Path,
    stdin_file: bool,
) -> Result<Observed> {
    require_unblocked_signals()?;
    let args = case.arguments()?;
    let name = case.name.as_deref().unwrap_or("fastmash");
    let data = decode(&case.input_hex)?;
    let mut stdout = capture(directory, "stdout")?;
    let mut stderr = capture(directory, "stderr")?;
    let input = if stdin_file {
        let mut input = capture(directory, "stdin")?;
        input.write_all(&data)?;
        input.rewind()?;
        Stdio::from(input)
    } else {
        Stdio::piped()
    };
    let mut env = BTreeMap::from([
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ("LC_ALL".to_owned(), case.locale.clone()),
        ("TZ".to_owned(), "UTC".to_owned()),
    ]);
    if let Some(value) = &case.posixly_correct {
        env.insert("POSIXLY_CORRECT".to_owned(), value.clone());
    }
    let started = Instant::now();
    let child = Command::new(launcher)
        .arg(EXEC_MODE)
        .arg(binary)
        .arg(name)
        .args(&args)
        .current_dir(directory)
        .env_clear()
        .envs(&env)
        .stdin(input)
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .spawn()?;
    let mut child = RunningChild {
        child,
        reaped: false,
        terminated: false,
    };
    let process_group = child.child.id();
    let mut input = child.child.stdin.take();
    if let Some(input) = &input {
        let flags = OFlag::from_bits_truncate(fcntl(input, FcntlArg::F_GETFL)?);
        fcntl(input, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    }
    let mut written = 0;
    let mut status = None;
    let mut timed_out = false;
    let mut overflow = false;
    loop {
        if let Some(stdin) = &mut input {
            if written < data.len() {
                match stdin.write(&data[written..]) {
                    Ok(0) => return Err(Error::message("zero-length write to command input")),
                    Ok(length) => written += length,
                    // Early version/help/error actions need not consume input.
                    Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => {
                        written = data.len()
                    }
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                        ) => {}
                    Err(error) => return Err(error.into()),
                }
            }
            if written == data.len() {
                input = None;
            }
        }
        if status.is_none() {
            status = child.child.try_wait()?;
            child.reaped = status.is_some();
        }
        overflow |= stdout.metadata()?.len() > CAP || stderr.metadata()?.len() > CAP;
        if status.is_some() && input.is_none() {
            break;
        }
        if started.elapsed() >= DEADLINE {
            timed_out = true;
            break;
        }
        if overflow {
            break;
        }
        thread::sleep(Duration::from_millis(2));
    }
    // Kill the whole session's group even if its leader has already exited.
    // A descendant may still hold stdin open or be writing a capture file.
    // The direct child kill also covers launch failure before setsid completes.
    child.terminate()?;
    drop(input);
    let status = match status {
        Some(status) => status,
        None => child.child.wait()?,
    };
    child.reaped = true;
    stdout.rewind()?;
    stderr.rewind()?;
    let mut out = Vec::new();
    let mut err = Vec::new();
    stdout.take(CAP + 1).read_to_end(&mut out)?;
    stderr.take(CAP + 1).read_to_end(&mut err)?;
    overflow |= out.len() as u64 > CAP
        || err.len() as u64 > CAP
        || status.signal() == Some(Signal::SIGXFSZ as i32);
    let mut argv = vec![OsString::from(name)];
    argv.extend(args);
    use std::os::unix::ffi::OsStrExt;
    Ok(Observed {
        argv: argv
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        argv_hex: argv.iter().map(|arg| encode(arg.as_bytes())).collect(),
        executable: binary.to_owned(),
        env,
        process_group,
        returncode: returncode(status),
        timed_out,
        overflow,
        cleanup_confirmed: true,
        stdout_hex: encode(&out),
        stderr_hex: encode(&err),
    })
}

struct RunningChild {
    child: Child,
    reaped: bool,
    terminated: bool,
}

impl RunningChild {
    fn terminate(&mut self) -> Result<()> {
        let pid = Pid::from_raw(
            i32::try_from(self.child.id())
                .map_err(|_| Error::message("child PID exceeds the native process-ID range"))?,
        );
        match killpg(pid, Signal::SIGKILL) {
            Ok(()) | Err(Errno::ESRCH) => {}
            Err(error) => return Err(error.into()),
        }
        // killpg can see no group while the launcher is still before setsid.
        // A live leader is ours until wait has reaped it.
        if !self.reaped {
            match kill(pid, Signal::SIGKILL) {
                Ok(()) | Err(Errno::ESRCH) => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.terminated = true;
        Ok(())
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        if !self.terminated {
            let _ = self.terminate();
        }
        if !self.reaped {
            let _ = self.child.wait();
        }
    }
}

fn returncode(status: ExitStatus) -> i32 {
    status
        .code()
        .unwrap_or_else(|| -status.signal().unwrap_or(0))
}

fn capture(directory: &Path, name: &str) -> Result<File> {
    let path = directory.join(name);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    fs::remove_file(path)?;
    Ok(file)
}

pub(crate) struct Scratch {
    pub path: PathBuf,
}

impl Scratch {
    pub fn new() -> Result<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "fastmash-corpus-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
    }

    pub fn remove(self) -> Result<()> {
        fs::remove_dir_all(&self.path)?;
        Ok(())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn normalize_sort_name(stderr: &str) -> String {
    // Exactly the existing multiline prefix normalization, with no lossy UTF-8
    // conversion or replacement inside a diagnostic's body.
    let Ok(bytes) = decode(stderr) else {
        return stderr.to_owned();
    };
    let mut result = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"/usr/bin/sort: ") {
            result.extend_from_slice(b"sort: ");
            result.extend_from_slice(rest);
        } else {
            result.extend_from_slice(line);
        }
    }
    encode(&result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_only_changes_the_existing_start_of_line_prefix() {
        assert_eq!(
            decode(&normalize_sort_name(&encode(
                b"/usr/bin/sort: bad\nx/usr/bin/sort: raw\n/usr/bin/sort: last"
            )))
            .unwrap(),
            b"sort: bad\nx/usr/bin/sort: raw\nsort: last"
        );
    }
}
