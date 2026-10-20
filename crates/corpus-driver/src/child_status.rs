//! Observe an owned child without releasing the PID needed for group cleanup.
use rustix::process::{Pid, WaitId, WaitIdOptions, WaitIdStatus, waitid};
use std::io;
use std::process::{Child, ExitStatus};

pub(crate) fn cancel_group(child: &Child, reaped: &mut bool) -> io::Result<()> {
    use nix::errno::Errno;
    use nix::sys::signal::{Signal, killpg};
    if *reaped {
        return Err(io::Error::other("cannot cancel a released child PID"));
    }
    let pid = nix::unistd::Pid::from_raw(child.id() as i32);
    match killpg(pid, Signal::SIGKILL) {
        Ok(()) | Err(Errno::ESRCH) => Ok(()),
        // Darwin excludes zombies from group signalling and returns EPERM when
        // none are eligible. These candidate groups retain inherited credentials
        // and never daemonize. A live descendant is still sent SIGKILL. Keep the
        // leader unreaped and verify its terminal status before accepting this
        // empty-group result. Live or unobserved leaders retain the error.
        Err(Errno::EPERM) if cfg!(target_os = "macos") && terminal(child, reaped)?.is_some() => {
            Ok(())
        }
        Err(error) => Err(io::Error::new(
            io::Error::from(error).kind(),
            format!("cancel owned child group {pid} before reap: {error}"),
        )),
    }
}

pub(crate) fn observe(
    child: &Child,
    reaped: &mut bool,
    events: WaitIdOptions,
) -> io::Result<Option<WaitIdStatus>> {
    if *reaped {
        return Err(io::Error::other("cannot observe a released child PID"));
    }
    let pid = Pid::from_raw(child.id() as i32)
        .ok_or_else(|| io::Error::other("invalid owned child PID"))?;
    match waitid(
        WaitId::Pid(pid),
        events | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
    ) {
        Ok(status) => Ok(status),
        Err(rustix::io::Errno::INTR) => Ok(None),
        Err(error) => {
            if error == rustix::io::Errno::CHILD {
                // Another wait has released ownership. Never signal this number.
                *reaped = true;
            }
            Err(io::Error::new(
                io::Error::from(error).kind(),
                format!("observe child {} without reaping: {error}", child.id()),
            ))
        }
    }
}

pub(crate) fn terminal(child: &Child, reaped: &mut bool) -> io::Result<Option<WaitIdStatus>> {
    observe(child, reaped, WaitIdOptions::EXITED)
}

pub(crate) fn reap(child: &mut Child, reaped: &mut bool) -> io::Result<ExitStatus> {
    let result = child.wait();
    if result.is_ok()
        || result
            .as_ref()
            .is_err_and(|error| error.raw_os_error() == Some(nix::errno::Errno::ECHILD as i32))
    {
        *reaped = true;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use nix::sys::signal::{Signal, killpg};
    use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
    use std::os::unix::process::CommandExt;
    use std::process::Command;
    use std::time::{Duration, Instant};

    #[test]
    fn terminal_observation_keeps_the_pid_owned_until_group_cleanup() {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 17"])
            .process_group(0)
            .spawn()
            .unwrap();
        let mut reaped = false;
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = terminal(&child, &mut reaped).unwrap() {
                assert_eq!(status.exit_status(), Some(17));
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(!reaped);
        assert_eq!(
            terminal(&child, &mut reaped)
                .unwrap()
                .unwrap()
                .exit_status(),
            Some(17)
        );
        // A normal reaping wait must still find this child. With try_wait-based
        // observation it instead returns ECHILD, exposing the ownership race.
        let pid = nix::unistd::Pid::from_raw(child.id() as i32);
        let native = killpg(pid, Signal::SIGKILL);
        assert!(
            native.is_ok() || cfg!(target_os = "macos") && native == Err(nix::errno::Errno::EPERM)
        );
        cancel_group(&child, &mut reaped).unwrap();
        assert_eq!(
            waitpid(pid, Some(WaitPidFlag::WNOHANG)).unwrap(),
            WaitStatus::Exited(pid, 17)
        );
        assert!(terminal(&child, &mut reaped).is_err());
        assert!(reaped);
        assert_eq!(
            child.wait().unwrap_err().raw_os_error(),
            Some(nix::errno::Errno::ECHILD as i32)
        );
    }
}
