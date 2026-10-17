//! Owned direct-child capture shared by the two native tool adapters.

use crate::Result;
use serde_json::json;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
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
    // The only input caller supplies the frozen plan's 139/223-byte payload.
    // It fits in a fresh pipe even at the POSIX minimum capacity. Do not turn
    // this transport into a general streaming writer.
    if input.is_some_and(|bytes| bytes.len() > 223) {
        return Err("comparison input exceeds the frozen 223-byte workload".into());
    }
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
    let mut delivered = false;
    let mut broken_pipe = false;
    let mut input_error = None;
    let mut writer_closed = false;
    if let Some(bytes) = input {
        if let Some(mut writer) = child.stdin.take() {
            match writer.write_all(bytes) {
                Ok(()) => delivered = true,
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => broken_pipe = true,
                Err(error) => input_error = Some(error.to_string()),
            }
            drop(writer);
            writer_closed = true;
        } else {
            input_error = Some("the requested child stdin pipe is unavailable".into());
        }
    }
    // No fallible evidence write may abandon a live child before this wait.
    let waited = wait_bounded(&mut child, started, budget);
    if input.is_some() {
        fs::write(
            directory.join("stdin-delivery.json"),
            serde_json::to_vec_pretty(&json!({
                "delivered": delivered, "broken_pipe": broken_pipe,
                "writer_closed": writer_closed, "error": input_error,
            }))?,
        )?;
    }
    fs::write(
        directory.join("wait.json"),
        serde_json::to_vec_pretty(&json!({
            "pid": pid, "raw_status": waited.status.map(ExitStatusExt::into_raw),
            "exit_code": waited.status.and_then(|status| status.code()),
            "signal": waited.status.and_then(|status| status.signal()),
            "reaped": waited.status.is_some(), "timed_out": waited.timed_out,
            "kill_error": waited.kill_error, "wait_error": waited.wait_error,
            "execution_milliseconds": budget.execution.as_millis(),
            "cleanup_milliseconds": budget.cleanup.as_millis(),
            "timeout_starts_after_spawn": budget.start_after_spawn,
            "input_error": input_error,
        }))?,
    )?;
    if waited.timed_out
        || waited.wait_error.is_some()
        || waited.status.is_none()
        || input_error.is_some()
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

struct Waited {
    status: Option<ExitStatus>,
    timed_out: bool,
    kill_error: Option<String>,
    wait_error: Option<String>,
}

fn wait_bounded(child: &mut Child, started: Instant, budget: Budget) -> Waited {
    let mut result = Waited {
        status: None,
        timed_out: false,
        kill_error: None,
        wait_error: None,
    };
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                result.status = Some(status);
                result.timed_out |= started.elapsed() >= budget.completion;
                return result;
            }
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => {
                // Ownership is uncertain. Never signal a possibly reaped PID.
                result.wait_error = Some(error.to_string());
                return result;
            }
        }
        let elapsed = started.elapsed();
        if !result.timed_out && elapsed >= budget.execution {
            result.timed_out = true;
            if let Err(error) = child.kill() {
                result.kill_error = Some(error.to_string());
            }
        }
        if elapsed >= budget.execution + budget.cleanup {
            result.wait_error =
                Some("direct-child cleanup did not finish within its reserve".into());
            return result;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::DirBuilder;
    use std::os::unix::fs::DirBuilderExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn short_comparison_budget_delivers_input_and_reaps_a_nonreading_child() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "locale-capture-no-read-{}-{stamp}",
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let mut command = Command::new("/bin/sleep");
        command
            .arg("10")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C");
        let input = b"0\t0\n1\t1\n";
        let started = Instant::now();
        let result = capture(
            &mut command,
            Some(input),
            &root,
            Budget::comparison(Duration::from_millis(250)),
        );
        assert!(result.is_err());
        assert!(started.elapsed() >= Duration::from_millis(250));
        let waited: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("wait.json")).unwrap()).unwrap();
        assert_eq!(waited["timed_out"], true);
        assert_eq!(waited["reaped"], true);
        assert!(waited["raw_status"].is_number());
        assert_eq!(waited["signal"], 9);
        assert!(waited["exit_code"].is_null());
        assert_eq!(waited["execution_milliseconds"], 250);
        assert_eq!(waited["cleanup_milliseconds"], 1_000);
        assert_eq!(waited["timeout_starts_after_spawn"], true);
        assert!(waited["wait_error"].is_null());
        let delivery: serde_json::Value =
            serde_json::from_slice(&fs::read(root.join("stdin-delivery.json")).unwrap()).unwrap();
        assert_eq!(delivery["delivered"], true);
        assert_eq!(delivery["writer_closed"], true);
        assert_eq!(delivery["broken_pipe"], false);
        assert!(delivery["error"].is_null());
        assert_eq!(fs::read(root.join("stdin")).unwrap(), input);
        assert!(root.join("stdout").is_file());
        assert!(root.join("stderr").is_file());
        fs::remove_dir_all(root).unwrap();
    }
}
