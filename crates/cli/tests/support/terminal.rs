#![forbid(unsafe_code)]

//! Independent stdout/stderr pseudo-terminals for executable presentation tests.
#[path = "executable.rs"]
mod executable;
use nix::{
    fcntl::{FcntlArg, FdFlag, fcntl},
    pty::openpty,
    sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr},
};
use std::{
    fs::File,
    io::{Read, Write},
    os::unix::process::CommandExt,
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

#[allow(dead_code)]
pub fn pair() -> (File, File) {
    let pty = openpty(None, None).unwrap();
    let pair = (File::from(pty.master), File::from(pty.slave));
    for file in [&pair.0, &pair.1] {
        assert_eq!(fcntl(file, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC)), Ok(0));
    }
    let mut attributes = tcgetattr(&pair.1).unwrap();
    // Raw mode prevents terminal newline conversion from changing captured bytes.
    cfmakeraw(&mut attributes);
    tcsetattr(&pair.1, SetArg::TCSANOW, &attributes).unwrap();
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
                // Linux reports EIO when every slave descriptor has closed;
                // Darwin reports EOF, handled above.
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
    let stdout = stdout_terminal.then(pair);
    let stderr = stderr_terminal.then(pair);
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

#[allow(dead_code)]
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
