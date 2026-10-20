//! Acknowledged late EIO with exact input and owned terminal job control.
use crate::fixture::{Case, Expected, decode, digest, encode};
use crate::held_output::Streaming;
use crate::process::{CAP, Observed, require_unblocked_signals};
use crate::{Error, Result};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::sys::resource::{Resource, setrlimit};
use nix::sys::signal::{SigSet, SigmaskHow, Signal, killpg, pthread_sigmask};
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::{BorrowedFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
    barrier: Barrier,
    full_output: bool,
}

#[derive(Serialize, Deserialize)]
enum Barrier {
    OutputPrefix(Vec<u8>),
    InputDrained,
}

#[derive(Serialize, Deserialize)]
struct Completed {
    pid: u32,
    returncode: i32,
    acknowledgement: String,
    reaped: bool,
    terminal_reclaimed: bool,
}

struct Running {
    child: Child,
    reaped: bool,
    group: bool,
    cancel: Option<UnixStream>,
    cleanup: PathBuf,
}
impl Running {
    fn status(&mut self) -> Result<Option<rustix::process::WaitIdStatus>> {
        let status = crate::child_status::terminal(&self.child, &mut self.reaped);
        if self.reaped {
            self.group = false;
        }
        let status = status?;
        Ok(status)
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        // Cancel descendants while the leader PID is still owned.
        if self.group && !self.reaped {
            let _ = crate::child_status::cancel_group(&self.child, &mut self.reaped);
            self.group = false;
        }
        if self.reaped {
            return;
        }
        if let Some(peer) = self.cancel.take() {
            drop(peer);
            let grace = Instant::now() + Duration::from_secs(2);
            while Instant::now() < grace {
                if self.status().ok().flatten().is_some() {
                    let _ = crate::child_status::reap(&mut self.child, &mut self.reaped);
                    return;
                }
                if self.reaped {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        let _ = self.child.kill();
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline {
            if self.status().ok().flatten().is_some() {
                let Ok(status) = crate::child_status::reap(&mut self.child, &mut self.reaped)
                else {
                    return;
                };
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
fn pause(started: Instant, cancel: BorrowedFd<'_>) -> Result<()> {
    let mut byte = [0];
    match nix::unistd::read(cancel, &mut byte) {
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

fn stop(running: &mut Running, pid: Pid, started: Instant, cancel: BorrowedFd<'_>) -> Result<bool> {
    match killpg(pid, Signal::SIGSTOP) {
        Ok(()) => {}
        Err(error)
            if (error == Errno::ESRCH || cfg!(target_os = "macos") && error == Errno::EPERM)
                && running.status()?.is_some() =>
        {
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    }
    loop {
        match crate::child_status::observe(
            &running.child,
            &mut running.reaped,
            rustix::process::WaitIdOptions::EXITED | rustix::process::WaitIdOptions::STOPPED,
        ) {
            Ok(Some(status)) if status.stopping_signal() == Some(Signal::SIGSTOP as i32) => {
                return Ok(true);
            }
            Ok(Some(status)) if status.exited() || status.killed() || status.dumped() => {
                return Ok(false);
            }
            Ok(None) => {}
            Ok(Some(_)) => return Err(Error::message("unexpected late-input stop observation")),
            Err(error) => {
                if running.reaped {
                    running.group = false;
                }
                return Err(error.into());
            }
        }
        pause(started, cancel)?;
    }
}

pub fn coordinate(args: impl Iterator<Item = OsString>) -> Result<()> {
    require_unblocked_signals()?;
    let root = root_argument(args)?;
    let plan = plan(&root)?;
    // The full sink takes stdout in the combined fixture. The same duplex
    // cancellation socket then occupies stderr, separate from candidate stderr.
    let cancel = if plan.full_output {
        rustix::stdio::stderr()
    } else {
        rustix::stdio::stdout()
    };
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
    let (mut stream, output, destination) = Streaming::new(!plan.full_output)?;
    let (output, destination) = if plan.full_output {
        // Close the unused pipe writer now so its empty capture reaches EOF.
        drop(output);
        let inherited = File::from(rustix::stdio::stdout().try_clone_to_owned()?);
        let destination = crate::held_output::destination(&inherited)?;
        (Stdio::inherit(), destination)
    } else {
        (output, destination)
    };
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
    let output_acknowledged = matches!(plan.barrier, Barrier::OutputPrefix(_));
    if output_acknowledged {
        fs::write(root.join("release"), b"foreground ready\n")?;
    }
    fs::write(root.join("input-ready"), b"input ready\n")?;
    if !output_acknowledged {
        let length = fs::metadata(root.join("input"))?.len();
        // An empty queue alone could precede delivery through the terminal's
        // line discipline. First observe every byte queued while the exec
        // helper is held at `release` and cannot read stdin.
        loop {
            if running.status()?.is_some() {
                return Err(Error::message("candidate exited before input delivery"));
            }
            let queued = rustix::io::ioctl_fionread(input)?;
            if root.join("input-delivered").try_exists()? && queued == length {
                fs::write(
                    root.join("input-queued.json"),
                    serde_json::to_vec(&json!({
                        "candidate_pid":running.child.id(),"kernel_unread":queued,
                        "exact_input_length":length,"before_exec_release":true,
                    }))?,
                )?;
                break;
            }
            pause(started, cancel)?;
        }
        fs::write(root.join("release"), b"exact input queued\n")?;
    }
    let acknowledgement = match &plan.barrier {
        Barrier::OutputPrefix(prefix) => {
            loop {
                collect(&mut stream, &mut sink, &mut copied)?;
                if running.status()?.is_some() {
                    return Err(Error::message(
                        "candidate exited before late-input acknowledgement",
                    ));
                }
                if stream.bytes.len() >= prefix.len() {
                    if !stream.bytes.starts_with(prefix) {
                        return Err(Error::message("late-input acknowledgement differs"));
                    }
                    break;
                }
                pause(started, cancel)?;
            }
            let acknowledgement = encode(&stream.bytes[..prefix.len()]);
            fs::write(
                root.join("acknowledged.json"),
                serde_json::to_vec(&json!({
                    "acknowledged":acknowledgement,"candidate_live":true,
                    "foreground":rustix::termios::tcgetpgrp(input)?.as_raw_nonzero().get(),
                }))?,
            )?;
            acknowledgement
        }
        Barrier::InputDrained => String::new(),
    };
    let mut ended = false;
    while !root.join("input-delivered").try_exists()? || rustix::io::ioctl_fionread(input)? != 0 {
        collect(&mut stream, &mut sink, &mut copied)?;
        if running.status()?.is_some() {
            if output_acknowledged {
                return Err(Error::message(
                    "candidate exited before late-input selection",
                ));
            }
            ended = true;
            break;
        }
        pause(started, cancel)?;
    }
    // Input drain proves that the candidate read these bytes, not that it
    // parsed them or emitted a result. Never label it an output acknowledgement.
    ended |= running.status()?.is_some();
    if ended && output_acknowledged {
        return Err(Error::message("candidate exited before late-input stop"));
    }
    if !ended && !output_acknowledged {
        fs::write(
            root.join("input-drained.json"),
            serde_json::to_vec(&json!({
                "candidate_pid":running.child.id(),"candidate_live":true,
                "kernel_unread":0,"exact_input_delivered":true,
            }))?,
        )?;
    }
    let terminal_reclaimed = !ended && stop(&mut running, pid, started, cancel)?;
    if !terminal_reclaimed && output_acknowledged {
        return Err(Error::message("candidate ended before stop observation"));
    }
    if terminal_reclaimed {
        fs::write(
            root.join("stopped.json"),
            serde_json::to_vec(&json!({
                "pid":running.child.id(),"signal":"SIGSTOP","kernel_unread":rustix::io::ioctl_fionread(input)?,
                "acknowledgement_before_stop":output_acknowledged,
            }))?,
        )?;
        rustix::termios::tcsetpgrp(input, session)?;
        killpg(pid, Signal::SIGCONT)?;
        fs::write(root.join("resumed"), b"resumed after foreground reclaim\n")?;
    }
    loop {
        collect(&mut stream, &mut sink, &mut copied)?;
        if running.status()?.is_some() {
            break;
        }
        pause(started, cancel)?;
    }
    // Stop descendants before waiting for the terminal's last writer to close.
    crate::child_status::cancel_group(&running.child, &mut running.reaped)?;
    running.group = false;
    let status = crate::child_status::reap(&mut running.child, &mut running.reaped)?;
    while !stream.ended() {
        collect(&mut stream, &mut sink, &mut copied)?;
        pause(started, cancel)?;
    }
    let result = Completed {
        pid: running.child.id(),
        returncode: status
            .code()
            .unwrap_or_else(|| -status.signal().unwrap_or(0)),
        acknowledgement,
        reaped: running.reaped,
        terminal_reclaimed,
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
    let plan = Plan {
        case_id: case.id.clone(),
        binary: binary.to_owned(),
        name: name.to_owned(),
        arguments: args.iter().map(|arg| arg.as_bytes().to_vec()).collect(),
        env: env.clone(),
        barrier: Barrier::OutputPrefix(ack),
        full_output: false,
    };
    execute(plan, &data, launcher, &root, None)
}

/// Deliver exact input to a live terminal reader, then select real EIO after
/// its kernel input queue drains. This does not require or infer parsed output.
/// `root` must not exist. The caller retains its ordinary-EOF controls and exact
/// fault-result assertions, since incomplete records can make those differ.
/// Earlier command errors are returned as observed, without claiming that the
/// terminal was reclaimed or that a read encountered EIO.
pub fn buffered(
    binary: &Path,
    arguments: &[OsString],
    input: &[u8],
    locale: &str,
    launcher: &Path,
    root: &Path,
) -> Result<std::process::Output> {
    buffered_output(binary, arguments, input, locale, launcher, root, None)
}

/// Combine the same actual late-input transport with an already calibrated
/// full stdout. The caller must retain the owner until this invocation finishes.
pub fn buffered_with_full_output(
    binary: &Path,
    arguments: &[OsString],
    input: &[u8],
    locale: &str,
    launcher: &Path,
    root: &Path,
    full: &mut crate::full_output::FullOutput,
) -> Result<std::process::Output> {
    buffered_output(binary, arguments, input, locale, launcher, root, Some(full))
}

fn buffered_output(
    binary: &Path,
    arguments: &[OsString],
    input: &[u8],
    locale: &str,
    launcher: &Path,
    root: &Path,
    mut full: Option<&mut crate::full_output::FullOutput>,
) -> Result<std::process::Output> {
    if input.is_empty() || input.len() > 1024 {
        return Err(Error::message("late-input input bounds differ"));
    }
    let binary = binary.canonicalize()?;
    if let Some(owner) = full.as_deref_mut() {
        let identity = owner.identity()?;
        if identity["executable"] != json!(binary)
            || identity["executable_sha256"] != digest(&fs::read(&binary)?)
        {
            return Err(Error::message("calibrated full-output executable differs"));
        }
    }
    fs::DirBuilder::new().mode(0o700).create(root)?;
    let root = root.canonicalize()?;
    let plan = Plan {
        case_id: "buffered-input".to_owned(),
        binary,
        name: "fastmash".to_owned(),
        arguments: arguments
            .iter()
            .map(|arg| arg.as_bytes().to_vec())
            .collect(),
        env: BTreeMap::from([
            ("LC_ALL".to_owned(), locale.to_owned()),
            ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        ]),
        barrier: Barrier::InputDrained,
        full_output: full.is_some(),
    };
    let destination = full.map(|full| full.destination()).transpose()?;
    let observed = execute(plan, input, launcher, &root, destination)?;
    let status = if observed.returncode >= 0 {
        observed.returncode << 8
    } else {
        -observed.returncode
    };
    Ok(std::process::Output {
        status: std::process::ExitStatus::from_raw(status),
        stdout: decode(&observed.stdout_hex)?,
        stderr: decode(&observed.stderr_hex)?,
    })
}

fn execute(
    plan: Plan,
    data: &[u8],
    launcher: &Path,
    root: &Path,
    full: Option<File>,
) -> Result<Observed> {
    let expected_ack = match &plan.barrier {
        Barrier::OutputPrefix(prefix) => Some(encode(prefix)),
        Barrier::InputDrained => None,
    };
    let args: Vec<OsString> = plan
        .arguments
        .iter()
        .cloned()
        .map(OsString::from_vec)
        .collect();
    fresh(&root.join("input"))?.write_all(data)?;
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
        .arg(root)
        .stdin(Stdio::from(pair.slave));
    if let Some(full) = full {
        command
            .stdout(full)
            .stderr(Stdio::from(OwnedFd::from(peer)));
    } else {
        command
            .stdout(Stdio::from(OwnedFd::from(peer)))
            .stderr(fresh(&root.join("coordinator.stderr"))?);
    }
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
    loop {
        if running.status()?.is_some() {
            break;
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
    }
    let status = crate::child_status::reap(&mut running.child, &mut running.reaped)?;
    if plan.full_output {
        // Only the coordinator inherited this socket as stderr. The candidate's
        // stderr is its capture file, so reaping the coordinator closes the last
        // socket writer even when the candidate spawned descendants.
        let mut diagnostics = Vec::new();
        running
            .cancel
            .as_mut()
            .ok_or_else(|| Error::message("missing coordinator socket"))?
            .take(CAP + 1)
            .read_to_end(&mut diagnostics)?;
        if diagnostics.len() as u64 > CAP {
            return Err(Error::message("late-input coordinator stderr overflow"));
        }
        fresh(&root.join("coordinator.stderr"))?.write_all(&diagnostics)?;
    }
    if !status.success() {
        return Err(Error::message(format!(
            "late-input coordinator failed: {status}; inspect {}",
            root.display()
        )));
    }
    let result: Completed = serde_json::from_slice(&bounded_read(&root.join("completed.json"))?)?;
    if !result.reaped
        || delivered != data.len()
        || result.acknowledgement != expected_ack.as_deref().unwrap_or("")
    {
        return Err(Error::message("late-input completion facts differ"));
    }
    let mut argv = vec![OsStr::new(&plan.name).to_owned()];
    argv.extend(args);
    Ok(Observed {
        argv: argv
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        argv_hex: argv.iter().map(|arg| encode(arg.as_bytes())).collect(),
        executable: plan.binary,
        env: plan.env,
        process_group: result.pid,
        returncode: result.returncode,
        timed_out: false,
        overflow: false,
        cleanup_confirmed: running.reaped && result.reaped,
        stdin_held_until_exit: false,
        held: None,
        destination: None,
        acknowledged: expected_ack.clone(),
        input_error_mechanism: result.terminal_reclaimed.then(|| {
            if expected_ack.is_some() {
                "controlling-terminal-stop-resume-EIO"
            } else {
                "controlling-terminal-input-drained-stop-resume-EIO"
            }
            .to_owned()
        }),
        stdout_hex: encode(&bounded_read(&root.join("stdout"))?),
        stderr_hex: encode(&bounded_read(&root.join("stderr"))?),
    })
}
