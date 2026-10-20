#![forbid(unsafe_code)]
//! Bounded executed-command setup and selected child-only resource limits.
#[path = "test_process.rs"]
mod test_process;
use std::{ffi::OsStr, process::Command};
pub use test_process::bounded as native;

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
    let command = native(binary, seconds);
    command
}

/// Capture the file-size fixture through real pipes, with SIGXFSZ defaulted in
/// only the spawned process. `command` is a native launcher with env_clear().
pub fn file_size_output(
    mut command: Command,
    input: std::fs::File,
    bytes: u64,
) -> (nix::sys::wait::WaitStatus, Vec<u8>, Vec<u8>) {
    use nix::{
        fcntl::{FcntlArg, FdFlag, fcntl},
        spawn::{PosixSpawnAttr, PosixSpawnFileActions, PosixSpawnFlags, posix_spawn},
        sys::signal::{SigSet, Signal},
    };
    use std::{
        ffi::CString,
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };
    command.env("FASTMASH_TEST_PROCESS_FILE_SIZE", bytes.to_string());
    let executable = CString::new(command.get_program().as_bytes()).unwrap();
    let args: Vec<_> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|value| CString::new(value.as_bytes()).unwrap())
        .collect();
    let environment: Vec<_> = command
        .get_envs()
        .filter_map(|(key, value)| {
            value.map(|value| {
                let mut entry = key.as_bytes().to_vec();
                entry.push(b'=');
                entry.extend_from_slice(value.as_bytes());
                CString::new(entry).unwrap()
            })
        })
        .collect();
    let (stdout, stdout_writer) = std::io::pipe().unwrap();
    let (stderr, stderr_writer) = std::io::pipe().unwrap();
    assert!(
        FdFlag::from_bits_retain(fcntl(&stdout_writer, FcntlArg::F_GETFD).unwrap())
            .contains(FdFlag::FD_CLOEXEC)
    );
    assert!(
        FdFlag::from_bits_retain(fcntl(&stderr_writer, FcntlArg::F_GETFD).unwrap())
            .contains(FdFlag::FD_CLOEXEC)
    );
    let mut actions = PosixSpawnFileActions::init().unwrap();
    for (source, target) in [
        (input.as_raw_fd(), 0),
        (stdout_writer.as_raw_fd(), 1),
        (stderr_writer.as_raw_fd(), 2),
    ] {
        assert!(source >= 3);
        actions.add_dup2(source, target).unwrap();
    }
    let mut attributes = PosixSpawnAttr::init().unwrap();
    let mut defaults = SigSet::empty();
    defaults.add(Signal::SIGXFSZ);
    attributes.set_sigdefault(&defaults).unwrap();
    let flags = PosixSpawnFlags::POSIX_SPAWN_SETSIGDEF;
    // Darwin closes every descriptor not explicitly inherited by file actions,
    // including another test thread's transient descriptor before CLOEXEC setup.
    #[cfg(target_os = "macos")]
    let flags = flags | PosixSpawnFlags::from_bits_retain(libc::POSIX_SPAWN_CLOEXEC_DEFAULT);
    attributes.set_flags(flags).unwrap();
    let pid = posix_spawn(
        executable.as_c_str(),
        &actions,
        &attributes,
        &args,
        &environment,
    )
    .unwrap();
    let mut child = Running(Some(pid));
    drop((actions, attributes, input, stdout_writer, stderr_writer));
    let stdout = drain(stdout);
    let stderr = drain(stderr);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(35);
    let status = loop {
        if let Some(status) = child.poll() {
            break status;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "file-size fixture exceeded its child alarm and cleanup allowance"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    (status, stdout.join().unwrap(), stderr.join().unwrap())
}

fn drain(mut reader: std::io::PipeReader) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        use std::io::Read;
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).unwrap();
        bytes
    })
}

struct Running(Option<nix::unistd::Pid>);
impl Running {
    fn poll(&mut self) -> Option<nix::sys::wait::WaitStatus> {
        use nix::{
            errno::Errno,
            sys::wait::{WaitPidFlag, WaitStatus, waitpid},
        };
        match waitpid(self.0.unwrap(), Some(WaitPidFlag::WNOHANG)) {
            Ok(status @ (WaitStatus::Exited(..) | WaitStatus::Signaled(..))) => {
                self.0.take();
                Some(status)
            }
            Ok(WaitStatus::StillAlive) | Err(Errno::EINTR) => None,
            result => {
                // An ownership error does not authorize signalling the old PID.
                self.0.take();
                panic!("unexpected file-size child status: {result:?}; cleanup unconfirmed");
            }
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        use nix::{
            errno::Errno,
            sys::{
                signal::{Signal, kill},
                wait::{WaitPidFlag, WaitStatus, waitpid},
            },
        };
        let Some(pid) = self.0.take() else {
            return;
        };
        // The leader remains ours and unreaped until this guard consumes it.
        let observed = loop {
            match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                Err(Errno::EINTR) => continue,
                result => break result,
            }
        };
        if matches!(observed, Ok(WaitStatus::StillAlive)) {
            let _ = kill(pid, Signal::SIGKILL);
            while let Err(Errno::EINTR) = waitpid(pid, None) {}
        }
    }
}
