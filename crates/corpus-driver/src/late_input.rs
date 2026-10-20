//! Acknowledged late EIO with exact input and owned terminal job control.
use crate::fixture::{Case, Expected, decode, digest, encode};
use crate::held_output::Streaming;
use crate::process::{CAP, Observed, require_unblocked_signals};
use crate::{Error, Result};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::sys::resource::{Resource, setrlimit};
use nix::sys::signal::{SigSet, SigmaskHow, Signal, killpg, pthread_sigmask};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub const COORDINATE: &str = "__fastmash_late_input_coordinate";
pub const EXEC: &str = "__fastmash_late_input_exec";
const INNER: Duration = Duration::from_secs(6);
const OUTER: Duration = Duration::from_secs(8);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    case_id: String,
    binary: PathBuf,
    name: String,
    arguments: Vec<Vec<u8>>,
    env: BTreeMap<String, String>,
    acknowledgement: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct Completed {
    pid: u32,
    returncode: i32,
    acknowledgement: String,
    reaped: bool,
}

struct Running {
    child: Child,
    reaped: bool,
    group: bool,
    cancel: Option<UnixStream>,
    cleanup: PathBuf,
}
impl Running {
    fn status(&mut self) -> Result<Option<ExitStatus>> {
        let status = match self.child.try_wait() {
            Err(error) if error.raw_os_error() == Some(Errno::ECHILD as i32) => {
                // Ownership loss is an error, not permission to signal an old PID.
                self.reaped = true;
                self.group = false;
                return Err(error.into());
            }
            result => result?,
        };
        self.reaped |= status.is_some();
        Ok(status)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        // A reaped leader may still have descendants holding the terminal.
        if self.group {
            let _ = killpg(Pid::from_raw(self.child.id() as i32), Signal::SIGKILL);
            self.group = false;
        }
        if self.reaped {
            return;
        }
        if let Some(peer) = self.cancel.take() {
            drop(peer);
            let grace = Instant::now() + Duration::from_secs(2);
            while Instant::now() < grace {
                if self.status().ok().flatten().is_some() || self.reaped {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        let _ = self.child.kill();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.status() {
                let _ = fs::write(
                    &self.cleanup,
                    format!("pid={} reaped=true status={status}\n", self.child.id()),
                );
                return;
            }
            if self.reaped {
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        eprintln!("late-input cleanup unconfirmed for {}", self.child.id());
    }
}

fn fresh(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}
fn plan(root: &Path) -> Result<Plan> {
    let bytes = bounded_read(&root.join("plan.json"))?;
    Ok(serde_json::from_slice(&bytes)?)
}
fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(CAP + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > CAP {
        return Err(Error::message("late-input capture overflow"));
    }
    Ok(bytes)
}
fn root_argument(mut args: impl Iterator<Item = OsString>) -> Result<PathBuf> {
    let root = args
        .next()
        .ok_or_else(|| Error::message("late-input root is required"))?;
    if args.next().is_some() {
        return Err(Error::message("unexpected late-input arguments"));
    }
    Ok(PathBuf::from(root))
}
fn pause(started: Instant) -> Result<()> {
    let mut byte = [0];
    match nix::unistd::read(rustix::stdio::stdout(), &mut byte) {
        Ok(0) => {
            return Err(Error::message(
                "late-input coordinator cancellation requested",
            ));
        }
        Ok(_) => return Err(Error::message("unexpected late-input cancellation data")),
        Err(Errno::EAGAIN | Errno::EINTR) => {}
        Err(error) => return Err(error.into()),
    }
    if started.elapsed() >= INNER {
        return Err(Error::message("late-input execution deadline"));
    }
    thread::sleep(Duration::from_millis(2));
    Ok(())
}

pub fn exec(args: impl Iterator<Item = OsString>) -> Result<()> {
    let root = root_argument(args)?;
    let plan = plan(&root)?;
    // The coordinator alone survives terminal hangup to finish cancellation.
    // Restore the candidate's original unblocked hangup disposition.
    let mut hangup = SigSet::empty();
    hangup.add(Signal::SIGHUP);
    pthread_sigmask(SigmaskHow::SIG_UNBLOCK, Some(&hangup), None)?;
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGTTIN);
    mask.add(Signal::SIGTTOU);
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;
    let actual = SigSet::thread_get_mask()?;
    fs::write(
        root.join("exec-mask.json"),
        serde_json::to_vec(&json!({
            "sigttin_blocked":actual.contains(Signal::SIGTTIN),
            "sigttou_blocked":actual.contains(Signal::SIGTTOU),
            "sighup_blocked":actual.contains(Signal::SIGHUP),
        }))?,
    )?;
    setrlimit(Resource::RLIMIT_FSIZE, CAP, CAP)?;
    let started = Instant::now();
    while !root.join("release").try_exists()? {
        if started.elapsed() >= INNER {
            return Err(Error::message("late-input exec release deadline"));
        }
        thread::sleep(Duration::from_millis(2));
    }
    let error = Command::new(plan.binary)
        .arg0(plan.name)
        .args(plan.arguments.into_iter().map(OsString::from_vec))
        .env_clear()
        .envs(plan.env)
        .exec();
    Err(error.into())
}

fn collect(stream: &mut Streaming, sink: &mut File, copied: &mut usize) -> Result<()> {
    stream.drain(CAP as usize)?;
    if stream.bytes.len() as u64 > CAP {
        return Err(Error::message("late-input stdout overflow"));
    }
    sink.write_all(&stream.bytes[*copied..])?;
    *copied = stream.bytes.len();
    Ok(())
}

pub fn coordinate(args: impl Iterator<Item = OsString>) -> Result<()> {
    require_unblocked_signals()?;
    let root = root_argument(args)?;
    let plan = plan(&root)?;
    let started = Instant::now();
    let mut mask = SigSet::empty();
    mask.add(Signal::SIGTTOU);
    // Parent death closes the terminal master as well as the cancellation
    // socket. Keep this owner alive through SIGHUP so it can reap its group.
    mask.add(Signal::SIGHUP);
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&mask), None)?;
    let session = rustix::process::setsid()?;
    let input = rustix::stdio::stdin();
    rustix::process::ioctl_tiocsctty(input)?;
    let (mut stream, output, destination) = Streaming::new(true)?;
    let mut sink = fresh(&root.join("stdout"))?;
    let mut copied = 0;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg(EXEC)
        .arg(&root)
        .stdin(Stdio::inherit())
        .stdout(output)
        .stderr(fresh(&root.join("stderr"))?)
        .current_dir(&root)
        .process_group(0);
    let mut running = Running {
        child: command.spawn()?,
        reaped: false,
        group: true,
        cancel: None,
        cleanup: root.join("candidate-cleanup.txt"),
    };
    drop(command);
    let pid = Pid::from_raw(running.child.id() as i32);
    let native_pid = rustix::process::Pid::from_raw(pid.as_raw())
        .ok_or_else(|| Error::message("invalid late-input PID"))?;
    rustix::termios::tcsetpgrp(input, native_pid)?;
    fs::write(
        root.join("started.json"),
        serde_json::to_vec(&json!({
            "coordinator_pid":std::process::id(),
            "coordinator_sighup_blocked":SigSet::thread_get_mask()?.contains(Signal::SIGHUP),
            "candidate_pid":running.child.id(),"session":session.as_raw_nonzero().get(),
            "candidate_group":rustix::process::getpgid(Some(native_pid))?.as_raw_nonzero().get(),
            "terminal_session":rustix::termios::tcgetsid(input)?.as_raw_nonzero().get(),
            "stdin_is_terminal":nix::unistd::isatty(input)?,"destination":destination,
        }))?,
    )?;
    fs::write(root.join("release"), b"foreground ready\n")?;
    fs::write(root.join("input-ready"), b"input ready\n")?;
    loop {
        collect(&mut stream, &mut sink, &mut copied)?;
        if running.status()?.is_some() {
            return Err(Error::message(
                "candidate exited before late-input acknowledgement",
            ));
        }
        if stream.bytes.len() >= plan.acknowledgement.len() {
            if !stream.bytes.starts_with(&plan.acknowledgement) {
                return Err(Error::message("late-input acknowledgement differs"));
            }
            break;
        }
        pause(started)?;
    }
    let acknowledgement = encode(&stream.bytes[..plan.acknowledgement.len()]);
    fs::write(
        root.join("acknowledged.json"),
        serde_json::to_vec(&json!({
            "acknowledged":acknowledgement,"candidate_live":true,
            "foreground":rustix::termios::tcgetpgrp(input)?.as_raw_nonzero().get(),
        }))?,
    )?;
    while !root.join("input-delivered").try_exists()? || rustix::io::ioctl_fionread(input)? != 0 {
        collect(&mut stream, &mut sink, &mut copied)?;
        if running.status()?.is_some() {
            return Err(Error::message(
                "candidate exited before late-input selection",
            ));
        }
        pause(started)?;
    }
    killpg(pid, Signal::SIGSTOP)?;
    loop {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG | WaitPidFlag::WUNTRACED)) {
            Ok(WaitStatus::Stopped(_, Signal::SIGSTOP)) => break,
            Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => {}
            Ok(WaitStatus::Exited(..) | WaitStatus::Signaled(..)) => {
                running.reaped = true;
                return Err(Error::message(
                    "candidate wait ownership ended before stop observation",
                ));
            }
            Err(Errno::ECHILD) => {
                running.reaped = true;
                running.group = false;
                return Err(Error::message(
                    "candidate wait ownership lost before stop observation",
                ));
            }
            result => {
                return Err(Error::message(format!(
                    "unexpected late-input stop result: {result:?}"
                )));
            }
        }
        pause(started)?;
    }
    fs::write(
        root.join("stopped.json"),
        serde_json::to_vec(&json!({
            "pid":running.child.id(),"signal":"SIGSTOP","kernel_unread":rustix::io::ioctl_fionread(input)?,
            "acknowledgement_before_stop":true,
        }))?,
    )?;
    rustix::termios::tcsetpgrp(input, session)?;
    killpg(pid, Signal::SIGCONT)?;
    fs::write(root.join("resumed"), b"resumed after foreground reclaim\n")?;
    let status = loop {
        collect(&mut stream, &mut sink, &mut copied)?;
        if let Some(status) = running.status()? {
            break status;
        }
        pause(started)?;
    };
    // Stop descendants before waiting for the terminal's last writer to close.
    match killpg(pid, Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => {}
        Err(error) => return Err(error.into()),
    }
    running.group = false;
    while !stream.ended() {
        collect(&mut stream, &mut sink, &mut copied)?;
        pause(started)?;
    }
    let result = Completed {
        pid: running.child.id(),
        returncode: status
            .code()
            .unwrap_or_else(|| -status.signal().unwrap_or(0)),
        acknowledgement,
        reaped: running.reaped,
    };
    let mut output = fresh(&root.join("completed.json"))?;
    serde_json::to_writer(&mut output, &result)?;
    Ok(())
}

pub fn invoke(
    case: &Case,
    expected: &Expected,
    binary: &Path,
    launcher: &Path,
    base: &Path,
) -> Result<Observed> {
    let data = decode(&case.input_hex)?;
    let ack = decode(
        case.ack_hex
            .as_deref()
            .ok_or_else(|| Error::message("late-input acknowledgement is required"))?,
    )?;
    if data.is_empty() || data.len() > 1024 || ack.is_empty() || ack.len() as u64 > CAP {
        return Err(Error::message(
            "late-input input or acknowledgement bounds differ",
        ));
    }
    fs::create_dir_all(base)?;
    let root = base.join(digest(case.id.as_bytes()));
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    let root = root.canonicalize()?;
    let mut ordinary = case.clone();
    ordinary.io = "normal".to_owned();
    let control = crate::process::invoke(&ordinary, binary, launcher, &root, false, None)?;
    let mut normal_expected = expected.clone();
    normal_expected.returncode = 0;
    normal_expected.stderr_hex.clear();
    fs::write(
        root.join("ordinary-control.json"),
        serde_json::to_vec_pretty(&json!({
            "observed":control,"expected":normal_expected,"matched":control.matches(&normal_expected),
        }))?,
    )?;
    if !control.matches(&normal_expected) {
        return Err(Error::message(
            "late-input ordinary candidate control differs",
        ));
    }
    let args = case.arguments()?;
    let name = case.name.as_deref().unwrap_or("fastmash");
    let env = control.env;
    fresh(&root.join("input"))?.write_all(&data)?;
    let plan = Plan {
        case_id: case.id.clone(),
        binary: binary.to_owned(),
        name: name.to_owned(),
        arguments: args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
        env: env.clone(),
        acknowledgement: ack,
    };
    let mut file = fresh(&root.join("plan.json"))?;
    serde_json::to_writer(&mut file, &plan)?;
    drop(file);
    let pair = nix::pty::openpty(None, None)?;
    let mut attributes = nix::sys::termios::tcgetattr(&pair.slave)?;
    nix::sys::termios::cfmakeraw(&mut attributes);
    nix::sys::termios::tcsetattr(&pair.slave, nix::sys::termios::SetArg::TCSANOW, &attributes)?;
    for fd in [&pair.master, &pair.slave] {
        fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    }
    let flags = OFlag::from_bits_truncate(fcntl(&pair.master, FcntlArg::F_GETFL)?);
    fcntl(&pair.master, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    let mut master = File::from(pair.master);
    let (peer, cancel) = UnixStream::pair()?;
    peer.set_nonblocking(true)?;
    let mut command = Command::new(launcher);
    command
        .arg(COORDINATE)
        .arg(&root)
        .stdin(Stdio::from(pair.slave))
        .stdout(Stdio::from(OwnedFd::from(peer)))
        .stderr(fresh(&root.join("coordinator.stderr"))?);
    let mut running = Running {
        child: command.spawn()?,
        reaped: false,
        group: false,
        cancel: Some(cancel),
        cleanup: root.join("coordinator-cleanup.txt"),
    };
    drop(command);
    let started = Instant::now();
    let mut delivered = 0;
    let status = loop {
        if let Some(status) = running.status()? {
            break status;
        }
        if root.join("input-ready").try_exists()? && delivered < data.len() {
            match master.write(&data[delivered..]) {
                Ok(0) => return Err(Error::message("late-input write made no progress")),
                Ok(n) => delivered += n,
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error.into()),
            }
            if delivered == data.len() {
                fs::write(root.join("input-delivered"), b"exact input delivered\n")?;
            }
        }
        if started.elapsed() >= OUTER {
            return Err(Error::message("late-input coordinator deadline"));
        }
        thread::sleep(Duration::from_millis(2));
    };
    if !status.success() {
        return Err(Error::message(format!(
            "late-input coordinator failed: {status}; inspect {}",
            root.display()
        )));
    }
    let result: Completed = serde_json::from_slice(&bounded_read(&root.join("completed.json"))?)?;
    if !result.reaped
        || delivered != data.len()
        || result.acknowledgement != encode(&plan.acknowledgement)
    {
        return Err(Error::message("late-input completion facts differ"));
    }
    let mut argv = vec![OsStr::new(name).to_owned()];
    argv.extend(args);
    Ok(Observed {
        argv: argv
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        argv_hex: argv.iter().map(|arg| encode(arg.as_bytes())).collect(),
        executable: binary.to_owned(),
        env,
        process_group: result.pid,
        returncode: result.returncode,
        timed_out: false,
        overflow: false,
        cleanup_confirmed: running.reaped && result.reaped,
        stdin_held_until_exit: false,
        held: None,
        destination: None,
        acknowledged: Some(result.acknowledgement),
        input_error_mechanism: Some("controlling-terminal-stop-resume-EIO".to_owned()),
        stdout_hex: encode(&bounded_read(&root.join("stdout"))?),
        stderr_hex: encode(&bounded_read(&root.join("stderr"))?),
    })
}
