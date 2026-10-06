//! Executed-command checks for inherited streams and native terminal behavior.
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;

use std::{
    ffi::OsString,
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, OwnedFd},
        unix::{
            ffi::OsStringExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

fn command(args: &[&str]) -> Command {
    let mut command = terminal::Program.command(args, &[]);
    command
        .arg0("fastmash")
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: alarm is async-signal-safe and bounds any unexpected wait in the
    // child, including assertion failures before the parent can reap it.
    unsafe {
        command.pre_exec(|| {
            libc::alarm(5);
            Ok(())
        });
    }
    command
}

#[test]
fn closed_standard_stream_combinations_keep_their_original_failure() {
    let directory = temp_dir::TempDir::new("closed-stream-combinations");
    let input = directory.0.join("input");
    fs::write(&input, b"a\nb\n").unwrap();
    for closed in 1u8..8 {
        let mut command = command(&["count", "1"]);
        command.stdin(File::open(&input).unwrap());
        // SAFETY: close is async-signal-safe and touches only the child's
        // standard streams, after Command has installed its owned handles.
        unsafe {
            command.pre_exec(move || {
                for fd in 0..3 {
                    if closed & (1 << fd) != 0 && libc::close(fd) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let output = command.output().unwrap();
        let expected = if closed & 3 == 0 { 0 } else { 1 };
        assert_eq!(
            output.status.code(),
            Some(expected),
            "closed={closed}: {output:?}"
        );
        assert_eq!(
            output.stdout,
            if closed & 3 == 0 {
                b"2\n".as_slice()
            } else {
                b""
            },
            "closed={closed}"
        );
        let error: &[u8] = if closed & 4 != 0 {
            b""
        } else if closed & 1 != 0 {
            b"fastmash: read error: Bad file descriptor\n"
        } else {
            b"fastmash: write error: Bad file descriptor\n"
        };
        assert_eq!(output.stderr, error, "closed={closed}");
    }
}

#[test]
fn wrong_access_stream_combinations_keep_their_original_failure() {
    let directory = temp_dir::TempDir::new("wrong-access-combinations");
    let input = directory.0.join("input");
    fs::write(&input, b"a\nb\n").unwrap();
    for wrong in 1u8..8 {
        let mut command = command(&["count", "1"]);
        command.stdin(if wrong & 1 == 0 {
            File::open(&input).unwrap()
        } else {
            File::options().write(true).open("/dev/null").unwrap()
        });
        if wrong & 2 != 0 {
            command.stdout(File::open("/dev/null").unwrap());
        }
        if wrong & 4 != 0 {
            command.stderr(File::open("/dev/null").unwrap());
        }
        let output = command.output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(if wrong & 3 == 0 { 0 } else { 1 }),
            "wrong={wrong}: {output:?}"
        );
        assert_eq!(
            output.stdout,
            if wrong & 3 == 0 {
                b"2\n".as_slice()
            } else {
                b""
            },
            "wrong={wrong}"
        );
        let error: &[u8] = if wrong & 4 != 0 {
            b""
        } else if wrong & 1 != 0 {
            b"fastmash: read error: Bad file descriptor\n"
        } else {
            b"fastmash: write error: Bad file descriptor\n"
        };
        assert_eq!(output.stderr, error, "wrong={wrong}");
    }
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
    // SAFETY: these signal operations use stack-local sigset_t storage and
    // async-signal-safe libc calls; they change only the child before exec.
    unsafe {
        command.pre_exec(move || {
            let mut mask = std::mem::zeroed();
            if libc::sigemptyset(&mut mask) != 0
                || libc::sigaddset(&mut mask, blocked) != 0
                || libc::sigprocmask(libc::SIG_SETMASK, &mask, std::ptr::null_mut()) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            if let Some(signal) = ignored
                && libc::signal(signal, libc::SIG_IGN) == libc::SIG_ERR
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
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

#[test]
fn ordinary_broken_pipes_restore_sigpipe_even_when_inherited_ignored() {
    for ignored in [false, true] {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        let mut command = command(&["count", "1"]);
        command.stdout(writer);
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

fn readable(file: &impl AsRawFd, timeout: Duration) -> bool {
    let mut descriptor = libc::pollfd {
        fd: file.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: poll reads and updates one live pollfd for an owned descriptor.
    let result = unsafe {
        libc::poll(
            &mut descriptor,
            1,
            timeout.as_millis().min(i32::MAX as u128) as i32,
        )
    };
    assert!(result >= 0, "poll: {}", std::io::Error::last_os_error());
    result != 0
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
    // SAFETY: kill sends SIGINT to the child PID returned by Command.
    assert_eq!(unsafe { libc::kill(child.id() as i32, libc::SIGINT) }, 0);
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
