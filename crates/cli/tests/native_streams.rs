#![forbid(unsafe_code)]
//! Executed-command checks for inherited streams and native terminal behavior.
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;
#[path = "support/test_process.rs"]
mod test_process;

use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsFd, OwnedFd},
        unix::{
            ffi::OsStringExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn command(args: &[&str]) -> Command {
    let target = terminal::Program.command(args, &[]);
    let mut command = test_process::bounded(target.get_program(), 5);
    command
        .args(target.get_args())
        .arg0("fastmash")
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn argument_initialization_preserves_non_utf8_bytes() {
    let mut child = command(&["-t"])
        .arg(OsString::from_vec(vec![0xff]))
        .args(["cut", "2,1"])
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"left\xffright\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"right\xffleft\n");
    assert!(output.stderr.is_empty());
}

fn signal_mask(command: &mut Command, blocked: i32, ignored: Option<i32>) {
    command.env("FASTMASH_TEST_PROCESS_BLOCKED", blocked.to_string());
    if let Some(signal) = ignored {
        assert_eq!(signal, libc::SIGPIPE);
        command.env("FASTMASH_TEST_PROCESS_IGNORE_PIPE", "1");
    }
}

#[test]
fn blocked_sigpipe_refuses_and_unrelated_masks_allow_ordinary_commands() {
    for signal in [libc::SIGPIPE, libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        let mut command = command(&["count", "1"]);
        signal_mask(&mut command, signal, None);
        let mut child = command.spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(b"a\nb\n");
        let output = child.wait_with_output().unwrap();
        if signal == libc::SIGPIPE {
            assert_eq!(output.status.code(), Some(77), "{output:?}");
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, b"fastmash: blocked SIGPIPE is unsupported\n");
        } else {
            assert!(output.status.success(), "signal={signal}: {output:?}");
            assert_eq!(output.stdout, b"2\n");
            assert!(output.stderr.is_empty());
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn denied_signal_mask_query_refuses() {
    let output = command(&["count", "1"])
        .env("FASTMASH_TEST_PROCESS_DENY_MASK_QUERY", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(77), "{output:?}");
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"fastmash: unable to inspect runtime signal mask\n"
    );
}

#[test]
fn ordinary_broken_pipes_restore_sigpipe_even_when_inherited_ignored() {
    for ignored in [false, true] {
        let mut command = command(&["count", "1"]);
        command.stdout(Stdio::null());
        command.env("FASTMASH_TEST_PROCESS_BROKEN_OUTPUT", "1");
        signal_mask(
            &mut command,
            libc::SIGTERM,
            ignored.then_some(libc::SIGPIPE),
        );
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"a\n").unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            output.status.signal(),
            Some(libc::SIGPIPE),
            "ignored={ignored}: {output:?}"
        );
        assert!(output.stderr.is_empty());
    }
}

fn readable(file: &impl AsFd, timeout: Duration) -> bool {
    use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
    let mut descriptor = [PollFd::new(file.as_fd(), PollFlags::POLLIN)];
    poll(&mut descriptor, PollTimeout::try_from(timeout).unwrap()).unwrap() != 0
}

fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("command did not complete within five seconds");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn terminal_lines_are_visible_while_pipe_output_waits_for_eof() {
    for tty in [false, true] {
        let (mut reader, output): (File, Stdio) = if tty {
            let (master, slave) = terminal::pair();
            (master, slave.into())
        } else {
            let (reader, writer) = std::io::pipe().unwrap();
            (File::from(OwnedFd::from(reader)), writer.into())
        };
        let mut child = command(&["reverse"]).stdout(output).spawn().unwrap();
        let mut input = child.stdin.take().unwrap();
        input.write_all(b"visible\n").unwrap();
        assert_eq!(
            readable(&reader, Duration::from_millis(500)),
            tty,
            "tty={tty}"
        );
        assert!(
            child.try_wait().unwrap().is_none(),
            "input must still be open"
        );
        let mut output = Vec::new();
        if tty {
            let mut line = [0; 8];
            reader.read_exact(&mut line).unwrap();
            output.extend_from_slice(&line);
        }
        drop(input);
        assert!(wait(&mut child).success());
        let mut tail = Vec::new();
        match reader.read_to_end(&mut tail) {
            Ok(_) => (),
            Err(error) if tty && error.raw_os_error() == Some(libc::EIO) => (),
            Err(error) => panic!("output read failed: {error}"),
        }
        output.extend(tail);
        assert_eq!(output, b"visible\n", "tty={tty}");
    }
}

#[test]
fn interrupt_after_visible_terminal_output_ends_the_command() {
    let (mut master, slave) = terminal::pair();
    let mut child = command(&["reverse"]).stdout(slave).spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(b"ready\n").unwrap();
    assert!(readable(&master, Duration::from_secs(2)));
    let mut line = [0; 6];
    master.read_exact(&mut line).unwrap();
    assert_eq!(&line, b"ready\n");
    assert_eq!(
        kill(Pid::from_raw(child.id() as i32), Signal::SIGINT),
        Ok(())
    );
    assert_eq!(wait(&mut child).signal(), Some(libc::SIGINT));
    drop(input);
    let mut error = Vec::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_end(&mut error)
        .unwrap();
    assert!(error.is_empty());
}
