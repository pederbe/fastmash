//! Bounded executed-command setup without a native utility dependency.
use std::{ffi::OsStr, process::Command};

pub fn bounded(binary: impl AsRef<OsStr>, seconds: u32) -> Command {
    let binary = binary.as_ref();
    #[cfg(target_os = "linux")]
    let command = {
        let mut command = Command::new("/usr/bin/timeout");
        command
            .arg("--kill-after=2s")
            .arg(format!("{seconds}s"))
            .arg(binary);
        command
    };
    #[cfg(target_os = "macos")]
    let command = {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new(binary);
        // SAFETY: alarm is async-signal-safe and changes only the child's timer.
        unsafe {
            command.pre_exec(move || {
                libc::alarm(seconds);
                Ok(())
            });
        }
        command
    };
    command
}
