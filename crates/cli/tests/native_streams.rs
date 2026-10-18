//! Executed-command checks for inherited streams and native terminal behavior.
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;

use std::{
    ffi::OsString,
    fs::File,
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
        let mut command = command(&["count", "1"]);
        command.stdout(Stdio::null());
        // Create the pipe after fork so concurrent test children cannot retain
        // its reader before their own exec and make the write succeed.
        // SAFETY: the callback uses only async-signal-safe libc operations and
        // stack-local storage. Command has installed open standard streams,
        // so the new descriptors are distinct from stdout and belong only to
        // this child. Both are closed after duplicating the writer onto stdout.
        unsafe {
            command.pre_exec(|| {
                let mut pipe = [-1; 2];
                if libc::pipe(pipe.as_mut_ptr()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::close(pipe[0]) != 0 {
                    let error = std::io::Error::last_os_error();
                    libc::close(pipe[1]);
                    return Err(error);
                }
                if libc::dup2(pipe[1], libc::STDOUT_FILENO) < 0 {
                    let error = std::io::Error::last_os_error();
                    libc::close(pipe[1]);
                    return Err(error);
                }
                if libc::close(pipe[1]) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
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
