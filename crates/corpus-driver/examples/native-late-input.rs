#![forbid(unsafe_code)]

//! Probe a real late EIO from POSIX controlling-terminal job control.
//! This is mechanism evidence, not a replacement of retained fault tests.

use fastmash_corpus_driver::fixture::digest;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::sys::signal::{SigSet, SigmaskHow, Signal, killpg, pthread_sigmask};
use serde_json::json;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::UnixStream;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const LIMIT: Duration = Duration::from_secs(10);
const PREFIX: usize = 32_768;
const WAKE: &[u8] = b"# wake\n";

fn fresh(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

struct Running {
    child: Child,
    group: bool,
    reaped: bool,
    watch_cancellation: bool,
    cancellation: Option<UnixStream>,
    cleanup_record: Option<PathBuf>,
}
impl Running {
    fn record_cleanup(&self, status: Option<ExitStatus>) {
        if let Some(path) = &self.cleanup_record {
            // Recording may fail on a full filesystem. Cancellation and
            // reaping have already happened and do not depend on this write.
            let _ = serde_json::to_vec_pretty(&json!({
                "pid":self.child.id(), "reaped":self.reaped,
                "wait_status":status.map(|value| format!("{value}")),
                "signal":status.and_then(|value| value.signal()),
            }))
            .map(|bytes| fs::write(path, bytes));
        }
    }
    fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        let status = self.child.try_wait()?;
        self.reaped |= status.is_some();
        Ok(status)
    }
    fn wait(&mut self, started: Instant) -> Result<ExitStatus> {
        loop {
            if self.watch_cancellation && cancellation_requested()? {
                return Err("coordinator cancellation requested".into());
            }
            if let Some(status) = self.status()? {
                return Ok(status);
            }
            if started.elapsed() >= LIMIT {
                return Err("child execution deadline".into());
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        if let Some(channel) = self.cancellation.take() {
            // Closing the established peer cannot fail for lack of space in
            // the capture filesystem. The coordinator observes EOF and owns
            // cancellation and reaping of its separately grouped candidate.
            drop(channel);
            let grace = Instant::now();
            while grace.elapsed() < Duration::from_secs(2) {
                if self.status().ok().flatten().is_some() {
                    return;
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
        if self.group {
            let _ = killpg(
                nix::unistd::Pid::from_raw(self.child.id() as i32),
                Signal::SIGKILL,
            );
        }
        let _ = self.child.kill();
        let cleanup = Instant::now();
        while cleanup.elapsed() < Duration::from_secs(1) {
            if let Some(status) = self.status().ok().flatten() {
                self.record_cleanup(Some(status));
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        eprintln!(
            "native-late-input: child {} did not reap within cleanup reserve",
            self.child.id()
        );
        self.record_cleanup(None);
    }
}

fn cancellation_requested() -> Result<bool> {
    let mut byte = [0];
    // The coordinator's otherwise unused stdout is a full-duplex UnixStream.
    // It has no buffered stdout writes and never passes this stream to the
    // candidate, whose stdout is an independently captured regular file.
    match nix::unistd::read(rustix::stdio::stdout(), &mut byte) {
        Ok(0) => Ok(true),
        Ok(_) => Err("unexpected cancellation channel data".into()),
        Err(Errno::EAGAIN | Errno::EINTR) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn pause(started: Instant) -> Result<()> {
    if started.elapsed() >= LIMIT {
        return Err("probe deadline".into());
    }
    thread::sleep(Duration::from_millis(2));
    Ok(())
}

fn target(executable: &Path, directory: &Path) -> Result<Command> {
    let mut command = Command::new(executable);
    command
        .arg0("fastmash")
        .args(["--skip-comments", "--full", "noop"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdout(fresh(&directory.join("stdout"))?)
        .stderr(fresh(&directory.join("stderr"))?);
    Ok(command)
}

fn ordinary(executable: &Path, root: &Path, prefix: &[u8]) -> Result<()> {
    let directory = root.join("ordinary");
    fs::create_dir(&directory)?;
    let (reader, mut writer) = UnixStream::pair()?;
    writer.set_nonblocking(true)?;
    let mut command = target(executable, &directory)?;
    command
        .stdin(Stdio::from(std::os::fd::OwnedFd::from(reader)))
        .process_group(0);
    let mut running = Running {
        child: command.spawn()?,
        group: true,
        reaped: false,
        watch_cancellation: false,
        cancellation: None,
        cleanup_record: None,
    };
    drop(command);
    let started = Instant::now();
    let mut bytes = prefix.to_vec();
    bytes.extend_from_slice(WAKE);
    let mut delivered = 0;
    while delivered < bytes.len() {
        match writer.write(&bytes[delivered..]) {
            Ok(0) => return Err("ordinary input write made no progress".into()),
            Ok(n) => delivered += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => pause(started)?,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => (),
            Err(e) => return Err(e.into()),
        }
    }
    drop(writer);
    let status = running.wait(started)?;
    let stdout = fs::read(directory.join("stdout"))?;
    let stderr = fs::read(directory.join("stderr"))?;
    fs::write(
        directory.join("observed.json"),
        serde_json::to_vec_pretty(&json!({
            "exit_code":status.code(), "reaped":running.reaped, "delivered":delivered,
            "stdout_sha256":digest(&stdout), "stderr_sha256":digest(&stderr),
            "matched":status.success() && stdout == prefix && stderr.is_empty(),
        }))?,
    )?;
    if !status.success() || stdout != prefix || !stderr.is_empty() {
        return Err("ordinary actual-candidate control differs".into());
    }
    Ok(())
}

fn exec_target(executable: &Path, root: &Path) -> Result<()> {
    let mut set = SigSet::empty();
    set.add(Signal::SIGTTIN);
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&set), None)?;
    let mask = SigSet::thread_get_mask()?;
    fs::write(
        root.join("exec-mask.json"),
        serde_json::to_vec_pretty(&json!({
            "sigttin_blocked":mask.contains(Signal::SIGTTIN),
            "sigttou_blocked":mask.contains(Signal::SIGTTOU),
            "pid":std::process::id(),
        }))?,
    )?;
    let started = Instant::now();
    while !root.join("release").try_exists()? {
        pause(started)?;
    }
    // Stdio is already configured by the coordinator. exec keeps this PID and
    // process group, while the real candidate performs every input/output call.
    let error = Command::new(executable)
        .arg0("fastmash")
        .args(["--skip-comments", "--full", "noop"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .exec();
    Err(error.into())
}

fn coordinate(executable: &Path, root: &Path, cancel: bool) -> Result<()> {
    let started = Instant::now();
    let session = rustix::process::setsid()?;
    let input = rustix::stdio::stdin();
    rustix::process::ioctl_tiocsctty(input)?;
    let mut set = SigSet::empty();
    set.add(Signal::SIGTTOU);
    pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&set), None)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg("--exec")
        .arg(executable)
        .arg(root)
        .stdin(Stdio::inherit())
        .stdout(fresh(&root.join("stdout"))?)
        .stderr(fresh(&root.join("stderr"))?)
        .process_group(0);
    let mut running = Running {
        child: command.spawn()?,
        group: true,
        reaped: false,
        watch_cancellation: true,
        cancellation: None,
        cleanup_record: Some(root.join("candidate-cleanup.json")),
    };
    drop(command);
    let child =
        rustix::process::Pid::from_raw(running.child.id() as i32).ok_or("invalid child pid")?;
    fs::write(
        root.join("started.json"),
        serde_json::to_vec_pretty(&json!({
            "coordinator_pid":std::process::id(), "candidate_pid":running.child.id(),
            "session":session.as_raw_nonzero().get(),
            "candidate_group":rustix::process::getpgid(Some(child))?.as_raw_nonzero().get(),
            "terminal_session":rustix::termios::tcgetsid(input)?.as_raw_nonzero().get(),
            "stdin_is_terminal":nix::unistd::isatty(input)?,
        }))?,
    )?;
    rustix::termios::tcsetpgrp(input, child)?;
    fs::write(root.join("release"), b"foreground ready\n")?;
    fs::write(root.join("input-ready"), b"real terminal input ready\n")?;
    while fs::metadata(root.join("stdout"))?.len() < (PREFIX * 8) as u64 {
        if cancellation_requested()? {
            return Err("coordinator cancellation requested".into());
        }
        if let Some(status) = running.status()? {
            return Err(format!("candidate exited before prefix: {status}").into());
        }
        pause(started)?;
    }
    let prefix = fs::read(root.join("stdout"))?;
    let expected_prefix = fs::read(root.parent().ok_or("missing probe root")?.join("input"))?;
    if !expected_prefix.starts_with(&prefix) {
        return Err("visible pre-fault output differs from supplied prefix".into());
    }
    if running.status()?.is_some() {
        return Err("candidate is not live before selected fault".into());
    }
    fs::write(
        root.join("prefix-visible.json"),
        serde_json::to_vec_pretty(&json!({
            "bytes":prefix.len(), "sha256":digest(&prefix), "candidate_live":true,
            "candidate_pid":child.as_raw_nonzero().get(), "session":session.as_raw_nonzero().get(),
            "foreground":rustix::termios::tcgetpgrp(input)?.as_raw_nonzero().get(),
        }))?,
    )?;
    if cancel {
        fs::write(
            root.join("cancel-ready"),
            b"live candidate prefix observed\n",
        )?;
        let status = running.wait(started)?;
        return Err(format!("candidate exited before cancellation: {status}").into());
    }
    rustix::termios::tcsetpgrp(input, session)?;
    fs::write(root.join("backgrounded"), b"terminal reclaimed\n")?;
    let status = running.wait(started)?;
    fs::write(
        root.join("candidate-status.json"),
        serde_json::to_vec_pretty(&json!({
            "exit_code":status.code(), "wait_status":format!("{status}"), "reaped":running.reaped,
            "foreground_after":rustix::termios::tcgetpgrp(input)?.as_raw_nonzero().get(),
        }))?,
    )?;
    Ok(())
}

fn probe(executable: &Path, root: &Path, cancel: bool) -> Result<()> {
    fs::DirBuilder::new().mode(0o700).create(root)?;
    let executable = fs::canonicalize(executable)?;
    let mut prefix = b"alpha\t2\n".repeat(PREFIX);
    // Starting this record flushes the preceding complete output block. It
    // remains part of the exact final expected output, even if still buffered
    // when the observed prefix establishes successful input before the fault.
    prefix.extend_from_slice(b"omega\t9\n");
    fs::write(root.join("input"), &prefix)?;
    fs::write(
        root.join("plan.json"),
        serde_json::to_vec_pretty(&json!({
            "candidate":executable, "candidate_sha256":digest(&fs::read(&executable)?),
            "input_sha256":digest(&prefix), "input_bytes":prefix.len(),
        "candidate_arguments":["--skip-comments", "--full", "noop"],
        "wakeup_comment_bytes":WAKE,
        "selection":if cancel { "close pre-established cancellation peer after live prefix" } else { "foreground terminal to background group with SIGTTIN blocked" },
            "source":"https://pubs.opengroup.org/onlinepubs/007904975/basedefs/xbd_chap11.html",
            "scope":"mechanism probe only; no retained fault test has been replaced",
        }))?,
    )?;
    ordinary(&executable, root, &prefix)?;
    let directory = root.join("terminal");
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let pty = nix::pty::openpty(None, None)?;
    let mut attributes = nix::sys::termios::tcgetattr(&pty.slave)?;
    nix::sys::termios::cfmakeraw(&mut attributes);
    nix::sys::termios::tcsetattr(&pty.slave, nix::sys::termios::SetArg::TCSANOW, &attributes)?;
    for fd in [&pty.master, &pty.slave] {
        fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
    }
    let flags = OFlag::from_bits_truncate(fcntl(&pty.master, FcntlArg::F_GETFL)?);
    fcntl(&pty.master, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    let master_fd = pty.master.as_raw_fd();
    let slave_fd = pty.slave.as_raw_fd();
    let mut master = File::from(pty.master);
    let (cancellation_reader, cancellation_writer) = UnixStream::pair()?;
    cancellation_reader.set_nonblocking(true)?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .arg(if cancel {
            "--coordinate-cancel"
        } else {
            "--coordinate"
        })
        .arg(&executable)
        .arg(&directory)
        .stdin(Stdio::from(pty.slave))
        .stdout(Stdio::from(std::os::fd::OwnedFd::from(cancellation_reader)))
        .stderr(fresh(&directory.join("coordinator.stderr"))?);
    let mut running = Running {
        child: command.spawn()?,
        group: false,
        reaped: false,
        watch_cancellation: false,
        cancellation: Some(cancellation_writer),
        cleanup_record: Some(directory.join("coordinator-cleanup.json")),
    };
    drop(command);
    let started = Instant::now();
    let mut delivered = 0;
    let mut wake_delivered = 0;
    let mut cancellation_sent = false;
    let coordinator_status = loop {
        if let Some(status) = running.status()? {
            break status;
        }
        if directory.join("input-ready").try_exists()? && delivered < prefix.len() {
            match master.write(&prefix[delivered..]) {
                Ok(n) => delivered += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if cancel && directory.join("cancel-ready").try_exists()? && !cancellation_sent {
            drop(running.cancellation.take());
            cancellation_sent = true;
        }
        if delivered == prefix.len()
            && directory.join("backgrounded").try_exists()?
            && wake_delivered < WAKE.len()
        {
            // Wake a read already pending at selection. The calibrated
            // comment is ignored if returned before the next read fails.
            match master.write(&WAKE[wake_delivered..]) {
                Ok(n) => wake_delivered += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted
                    ) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if started.elapsed() >= LIMIT + Duration::from_secs(5) {
            return Err("outer probe deadline".into());
        }
        thread::sleep(Duration::from_millis(2));
    };
    if cancel {
        let cleanup: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("candidate-cleanup.json"))?)?;
        let started: serde_json::Value =
            serde_json::from_slice(&fs::read(directory.join("started.json"))?)?;
        let stderr = fs::read(directory.join("coordinator.stderr"))?;
        let matched = cancellation_sent
            && coordinator_status.code() == Some(1)
            && cleanup["reaped"] == true
            && cleanup["signal"] == 9
            && cleanup["pid"] == started["candidate_pid"]
            && stderr == b"native-late-input: coordinator cancellation requested\n";
        fs::write(
            directory.join("cancellation-observed.json"),
            serde_json::to_vec_pretty(&json!({
                "matched":matched, "cancellation_sent":cancellation_sent,
                "coordinator_reaped":running.reaped, "coordinator_exit_code":coordinator_status.code(),
                "candidate_cleanup":cleanup,
            }))?,
        )?;
        if !matched {
            return Err("cancellation control differs; inspect captures".into());
        }
        println!("ordinary control and actual-candidate cancellation cleanup match");
        return Ok(());
    }
    if !coordinator_status.success() {
        return Err(
            format!("coordinator failed: {coordinator_status} (see coordinator.stderr)").into(),
        );
    }
    let stdout = fs::read(directory.join("stdout"))?;
    let stderr = fs::read(directory.join("stderr"))?;
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("candidate-status.json"))?)?;
    let matched = status["exit_code"] == 1
        && status["reaped"] == true
        && stderr == b"fastmash: read error: Input/output error\n"
        && stdout == prefix;
    fs::write(
        directory.join("observed.json"),
        serde_json::to_vec_pretty(&json!({
            "matched":matched, "delivered_before_wakeup":delivered, "wakeup_bytes_delivered":wake_delivered,
            "master_fd":master_fd, "slave_fd":slave_fd, "coordinator_reaped":running.reaped,
            "stdout_bytes":stdout.len(), "stdout_sha256":digest(&stdout), "stderr_sha256":digest(&stderr),
            "candidate":status,
        }))?,
    )?;
    if !matched {
        return Err("selected actual-candidate terminal fault differs; inspect captures".into());
    }
    println!("ordinary control and actual late terminal EIO match");
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let result = match args.as_slice() {
        [mode, executable, root] if mode == "--exec" => {
            exec_target(Path::new(executable), Path::new(root))
        }
        [mode, executable, root] if mode == "--coordinate" => {
            coordinate(Path::new(executable), Path::new(root), false)
        }
        [mode, executable, root] if mode == "--coordinate-cancel" => {
            coordinate(Path::new(executable), Path::new(root), true)
        }
        [mode, executable, root] if mode == "--cancel" => {
            probe(Path::new(executable), &PathBuf::from(root), true)
        }
        [executable, root] => probe(Path::new(executable), &PathBuf::from(root), false),
        _ => Err("usage: native-late-input [--cancel] EXECUTABLE FRESH_CAPTURE_DIR".into()),
    };
    if let Err(error) = result {
        eprintln!("native-late-input: {error}");
        std::process::exit(1);
    }
}
