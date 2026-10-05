//! Independent stdout/stderr pseudo-terminals for executable presentation tests.
#[path = "executable.rs"]
mod executable;
use std::{
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::process::CommandExt,
    },
    process::{Command, Output, Stdio},
    thread,
};

// Unit failure tests use their own Commands; integration suites bind the
// executable here so their environment and stream setup stays in one place.
#[allow(dead_code)]
pub struct Program;

#[allow(dead_code)]
impl Program {
    pub fn command(&self, args: &[&str], environment: &[(&str, &str)]) -> Command {
        let mut command = Command::new(executable::fastmash());
        command
            .arg0("fastmash")
            .args(args)
            .env_clear()
            .env("LC_ALL", "C")
            .env("TERM", "xterm")
            .env("PATH", "/usr/bin:/bin")
            .envs(environment.iter().copied());
        command
    }

    pub fn invoke(
        &self,
        args: &[&str],
        input: &[u8],
        environment: &[(&str, &str)],
        stdout_terminal: bool,
        stderr_terminal: bool,
    ) -> Output {
        output(
            &mut self.command(args, environment),
            input,
            stdout_terminal,
            stderr_terminal,
        )
    }
}

fn terminal() -> (File, File) {
    let (mut master, mut slave) = (-1, -1);
    assert_eq!(
        // SAFETY: openpty initializes both descriptors; null optional arguments
        // request the default terminal settings and no copied device name.
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0
    );
    // SAFETY: successful openpty transfers two valid, distinct descriptors.
    let pair = unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) };
    for file in [&pair.0, &pair.1] {
        assert_eq!(
            // SAFETY: F_SETFD accepts integer flags and retains descriptor ownership.
            unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) },
            0
        );
    }
    let mut attributes = std::mem::MaybeUninit::uninit();
    assert_eq!(
        // SAFETY: tcgetattr initializes attributes for this valid terminal.
        unsafe { libc::tcgetattr(slave, attributes.as_mut_ptr()) },
        0
    );
    // SAFETY: the successful tcgetattr initialized the value above.
    let mut attributes = unsafe { attributes.assume_init() };
    // SAFETY: cfmakeraw edits a valid termios value; tcsetattr copies it. Raw
    // mode prevents terminal newline conversion from changing captured bytes.
    unsafe {
        libc::cfmakeraw(&mut attributes);
        assert_eq!(libc::tcsetattr(slave, libc::TCSANOW, &attributes), 0);
    }
    pair
}

fn drain(mut master: File) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => output.extend_from_slice(&buffer[..count]),
                // Linux reports EIO when every slave descriptor has closed.
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => panic!("terminal read failed: {error}"),
            }
        }
        output
    })
}

pub fn output(
    command: &mut Command,
    input: &[u8],
    stdout_terminal: bool,
    stderr_terminal: bool,
) -> Output {
    let stdout = stdout_terminal.then(terminal);
    let stderr = stderr_terminal.then(terminal);
    command.stdin(Stdio::piped());
    command.stdout(
        stdout
            .as_ref()
            .map_or_else(Stdio::piped, |(_, slave)| slave.try_clone().unwrap().into()),
    );
    command.stderr(
        stderr
            .as_ref()
            .map_or_else(Stdio::piped, |(_, slave)| slave.try_clone().unwrap().into()),
    );
    let mut child = command.spawn().unwrap();
    // Command keeps its configured File handles so it can spawn again. Release
    // those parent-side slave copies before waiting for terminal EOF.
    command.stdout(Stdio::null()).stderr(Stdio::null());
    let stdout = stdout.map(|(master, slave)| {
        drop(slave);
        drain(master)
    });
    let stderr = stderr.map(|(master, slave)| {
        drop(slave);
        drain(master)
    });
    let _ = child.stdin.take().unwrap().write_all(input);
    let mut output = child.wait_with_output().unwrap();
    if let Some(reader) = stdout {
        output.stdout = reader.join().unwrap();
    }
    if let Some(reader) = stderr {
        output.stderr = reader.join().unwrap();
    }
    output
}

pub fn strip_styles(bytes: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at..].starts_with(b"\x1b[") {
            let end = bytes[at + 2..]
                .iter()
                .position(|byte| *byte == b'm')
                .unwrap();
            let code = &bytes[at + 2..at + 2 + end];
            assert!(
                [b"0".as_slice(), b"1", b"31", b"33", b"36"].contains(&code),
                "unexpected generated style: {code:?}"
            );
            at += end + 3;
        } else {
            output.push(bytes[at]);
            at += 1;
        }
    }
    output
}
