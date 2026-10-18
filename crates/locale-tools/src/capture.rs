//! Owned capture shared by the native tool adapters.

use crate::Result;
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

pub(crate) struct Captured {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Clone, Copy)]
pub(crate) struct Budget {
    pub execution: Duration,
    pub cleanup: Duration,
    completion: Duration,
    start_after_spawn: bool,
}

impl Budget {
    pub fn query(total: Duration) -> Self {
        let cleanup = Duration::from_secs(1).min(total / 4);
        Self {
            execution: total - cleanup,
            cleanup,
            completion: total,
            start_after_spawn: false,
        }
    }

    pub fn comparison(execution: Duration) -> Self {
        Self {
            execution,
            cleanup: Duration::from_secs(1),
            completion: execution,
            start_after_spawn: true,
        }
    }
}

pub(crate) fn capture(
    command: &mut Command,
    input: Option<&[u8]>,
    directory: &Path,
    budget: Budget,
) -> Result<Captured> {
    // Numeric comparison keeps its frozen workload and direct-child contract.
    if input.is_some_and(|bytes| bytes.len() > 223) {
        return Err("comparison input exceeds the frozen 223-byte workload".into());
    }
    capture_pipe(command, input, directory, budget)
}

/// Larger supplied workloads use nonblocking pipe writes and an owned process
/// group. The frozen small-input path retains its original delivery receipts.
pub(crate) fn capture_pipe(
    command: &mut Command,
    input: Option<&[u8]>,
    directory: &Path,
    budget: Budget,
) -> Result<Captured> {
    let large_input = input.is_some_and(|bytes| bytes.len() > 223);
    if let Some(bytes) = input {
        fs::write(directory.join("stdin"), bytes)?;
    }
    let file = |name| {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(directory.join(name))
    };
    let stdout = file("stdout")?;
    let stderr = file("stderr")?;
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    if large_input {
        command.process_group(0);
    }
    let mut started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            fs::write(
                directory.join("wait.json"),
                serde_json::to_vec_pretty(&json!({
                    "spawn_error": error.to_string(), "pid": null, "raw_status": null,
                    "exit_code": null, "signal": null, "reaped": false, "timed_out": false,
                    "kill_error": null, "wait_error": null,
                    "execution_milliseconds": budget.execution.as_millis(),
                    "cleanup_milliseconds": budget.cleanup.as_millis(),
                }))?,
            )?;
            return Err(error.into());
        }
    };
    if budget.start_after_spawn {
        started = Instant::now();
    }
    let pid = child.id();
    let mut delivery = input.map(|bytes| Delivery::new(bytes, child.stdin.take()));
    if let Some(delivery) = &mut delivery {
        if large_input {
            delivery.nonblocking();
        } else {
            // At most 223 bytes fit even in a fresh minimum-capacity POSIX pipe.
            delivery.write_small();
        }
    }
    // No fallible evidence write may abandon a live child before this wait.
    let waited = wait_bounded(
        &mut child,
        started,
        budget,
        delivery.as_mut().filter(|_| large_input),
    );
    let input_error = delivery.as_ref().and_then(|input| input.error.as_ref());
    if let Some(delivery) = &delivery {
        let mut receipt = json!({
            "delivered": delivery.delivered, "broken_pipe": delivery.broken_pipe,
            "writer_closed": delivery.writer_closed, "error": delivery.error,
        });
        if large_input {
            receipt["bytes_written"] = json!(delivery.written);
            receipt["would_block"] = json!(delivery.would_block);
        }
        fs::write(
            directory.join("stdin-delivery.json"),
            serde_json::to_vec_pretty(&receipt)?,
        )?;
    }
    let mut receipt = json!({
        "pid": pid, "raw_status": waited.status.map(ExitStatusExt::into_raw),
        "exit_code": waited.status.and_then(|status| status.code()),
        "signal": waited.status.and_then(|status| status.signal()),
        "reaped": waited.status.is_some(), "timed_out": waited.timed_out,
        "kill_error": waited.kill_error, "wait_error": waited.wait_error,
        "execution_milliseconds": budget.execution.as_millis(),
        "cleanup_milliseconds": budget.cleanup.as_millis(),
        "timeout_starts_after_spawn": budget.start_after_spawn,
        "input_error": input_error,
    });
    if large_input {
        receipt["process_group"] = json!(pid);
        receipt["group_signal"] = json!(waited.group_signal);
    }
    fs::write(
        directory.join("wait.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    if waited.timed_out
        || waited.wait_error.is_some()
        || waited.status.is_none()
        || input_error.is_some()
        || (large_input && waited.kill_error.is_some())
    {
        let label = if input.is_none() {
            "native query"
        } else {
            "native invocation"
        };
        return Err(format!(
            "{label} did not complete cleanly (reaped: {})",
            waited.status.is_some()
        )
        .into());
    }
    Ok(Captured {
        status: waited
            .status
            .ok_or("native invocation has no terminal status")?,
        stdout: fs::read(directory.join("stdout"))?,
        stderr: fs::read(directory.join("stderr"))?,
    })
}

struct Delivery<'a> {
    bytes: &'a [u8],
    writer: Option<ChildStdin>,
    written: usize,
    delivered: bool,
    broken_pipe: bool,
    writer_closed: bool,
    would_block: bool,
    error: Option<String>,
}

impl<'a> Delivery<'a> {
    fn new(bytes: &'a [u8], writer: Option<ChildStdin>) -> Self {
        let error = writer
            .is_none()
            .then(|| "the requested child stdin pipe is unavailable".into());
        Self {
            bytes,
            writer,
            written: 0,
            delivered: false,
            broken_pipe: false,
            writer_closed: false,
            would_block: false,
            error,
        }
    }

    fn close(&mut self) {
        if self.writer.take().is_some() {
            self.writer_closed = true;
        }
    }

    fn write_small(&mut self) {
        if let Some(writer) = &mut self.writer {
            match writer.write_all(self.bytes) {
                Ok(()) => self.delivered = true,
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => self.broken_pipe = true,
                Err(error) => self.error = Some(error.to_string()),
            }
            self.close();
        }
    }

    fn nonblocking(&mut self) {
        if let Some(writer) = &self.writer {
            let configure = || -> nix::Result<()> {
                let flags = OFlag::from_bits_truncate(fcntl(writer, FcntlArg::F_GETFL)?);
                fcntl(writer, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
                Ok(())
            };
            if let Err(error) = configure() {
                self.error = Some(format!("cannot make child input nonblocking: {error}"));
                self.close();
            }
        }
    }

    fn advance(&mut self) {
        let Some(writer) = &mut self.writer else {
            return;
        };
        match writer.write(&self.bytes[self.written..]) {
            Ok(0) => self.error = Some("zero-length write to child input".into()),
            Ok(length) => {
                self.written += length;
                self.delivered = self.written == self.bytes.len();
            }
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => self.broken_pipe = true,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => self.would_block = true,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => self.error = Some(error.to_string()),
        }
        if self.delivered || self.broken_pipe || self.error.is_some() {
            self.close();
        }
    }
}

struct Waited {
    status: Option<ExitStatus>,
    timed_out: bool,
    kill_error: Option<String>,
    wait_error: Option<String>,
    group_signal: Option<&'static str>,
}

fn wait_bounded(
    child: &mut Child,
    started: Instant,
    budget: Budget,
    mut delivery: Option<&mut Delivery<'_>>,
) -> Waited {
    let owned_group = delivery.is_some();
    let mut result = Waited {
        status: None,
        timed_out: false,
        kill_error: None,
        wait_error: None,
        group_signal: None,
    };
    let mut terminated = false;
    loop {
        if !terminated && let Some(input) = &mut delivery {
            input.advance();
        }
        if result.status.is_none() {
            match child.try_wait() {
                Ok(status) => result.status = status,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => {
                    // Ownership is uncertain. Never signal a possibly reaped PID.
                    result.wait_error = Some(error.to_string());
                    if let Some(input) = &mut delivery {
                        input.close();
                    }
                    if owned_group && !terminated {
                        terminate_group(child, &mut result);
                    }
                    return result;
                }
            }
        }
        let elapsed = started.elapsed();
        if result.status.is_some() && delivery.as_ref().is_none_or(|input| input.writer.is_none()) {
            result.timed_out |= elapsed >= budget.completion;
            if owned_group && !terminated {
                terminate_group(child, &mut result);
            }
            return result;
        }
        let input_error = delivery.as_ref().is_some_and(|input| input.error.is_some());
        if !terminated && (elapsed >= budget.execution || input_error) {
            result.timed_out |= elapsed >= budget.execution;
            if let Some(input) = &mut delivery {
                input.close();
            }
            if owned_group {
                terminate_group(child, &mut result);
            }
            // The direct-child fallback also covers a failed group signal.
            if result.status.is_none()
                && let Err(error) = child.kill()
            {
                result.kill_error = Some(match result.kill_error {
                    Some(group_error) => format!("{group_error}; direct child: {error}"),
                    None => error.to_string(),
                });
            }
            terminated = true;
        }
        if result.status.is_some() && terminated {
            return result;
        }
        if elapsed >= budget.execution + budget.cleanup {
            result.wait_error =
                Some("direct-child cleanup did not finish within its reserve".into());
            return result;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn terminate_group(child: &Child, result: &mut Waited) {
    let pid = match i32::try_from(child.id()) {
        Ok(pid) => Pid::from_raw(pid),
        Err(error) => {
            result.kill_error = Some(error.to_string());
            result.group_signal = Some("error");
            return;
        }
    };
    // Even a reaped leader can leave descendants holding its input or outputs.
    match killpg(pid, Signal::SIGKILL) {
        Ok(()) => result.group_signal = Some("sent"),
        Err(Errno::ESRCH) => result.group_signal = Some("already_absent"),
        Err(error) => {
            result.group_signal = Some("error");
            result.kill_error = Some(format!("process group: {error}"));
        }
    }
}
