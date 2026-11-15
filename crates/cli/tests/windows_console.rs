//! Native console checks adapted from udpstp's bounded ConPTY test transport.
#![cfg(windows)]
#![forbid(unsafe_code)]
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::{
    ffi::OsString,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

fn console(args: &[OsString], no_color: bool) -> String {
    console_command(
        &std::env::var_os("FASTMASH_TEST_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into()),
        args,
        no_color,
        None,
    )
    .0
}

fn console_command(
    program: &Path,
    args: &[OsString],
    no_color: bool,
    held: Option<(&Path, &Path)>,
) -> (String, Option<String>) {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 500,
            cols: 200,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(program);
    command.args(args);
    command.env("LC_ALL", "C.UTF-8");
    command.env("TERM", "xterm");
    for name in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "CI"] {
        command.env_remove(name);
    }
    if no_color {
        command.env("NO_COLOR", "1");
    }
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (input, requests) = mpsc::channel::<Option<Vec<u8>>>();
    let (input_sent, input_done) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            while let Ok(Some(bytes)) = requests.recv() {
                writer.write_all(&bytes)?;
                writer.flush()?;
            }
            Ok::<_, std::io::Error>(())
        })();
        drop(writer);
        let _ = input_sent.send(result);
    });
    let replies = input.clone();
    let prefix = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&prefix);
    let (sent, output) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = reader.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                let start = bytes.len().saturating_sub(3);
                bytes.extend_from_slice(&buffer[..count]);
                if bytes.len() > 1 << 20 {
                    return Err(std::io::Error::other("console output exceeded 1 MiB"));
                }
                observed.lock().unwrap().extend_from_slice(&buffer[..count]);
                // INHERIT_CURSOR queries the host before process creation.
                // This fresh, otherwise unused terminal starts at row/column 1.
                for word in bytes[start..].windows(4) {
                    if word == b"\x1b[6n" {
                        let _ = replies.send(Some(b"\x1b[1;1R".to_vec()));
                    }
                }
            }
            Ok(bytes)
        })();
        let _ = sent.send(result);
    });
    let (spawned, process) = mpsc::channel();
    std::thread::spawn(move || {
        let result = pair.slave.spawn_command(command);
        drop(pair.slave);
        if let Err(late) = spawned.send(result)
            && let Ok(mut child) = late.0
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    });
    let mut child = process
        .recv_timeout(Duration::from_secs(15))
        .expect("ConPTY spawn deadline")
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut observation_end = None;
    let mut held_bytes = None;
    let status = loop {
        if let Some((ready, release)) = held {
            if ready.exists() {
                observation_end.get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
            }
            if held_bytes.is_none() && observation_end.is_some_and(|end| Instant::now() >= end) {
                held_bytes = Some(prefix.lock().unwrap().clone());
                fs::write(release, b"allow EOF").unwrap();
            }
        }
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (closed, teardown) = mpsc::channel();
    std::thread::spawn(move || {
        drop(pair.master);
        let _ = closed.send(());
    });
    teardown
        .recv_timeout(Duration::from_secs(2))
        .expect("ConPTY teardown deadline");
    let bytes = output
        .recv_timeout(Duration::from_secs(2))
        .expect("ConPTY output deadline")
        .unwrap();
    let _ = input.send(None);
    input_done
        .recv_timeout(Duration::from_secs(2))
        .expect("ConPTY input deadline")
        .unwrap();
    if let Some(root) = std::env::var_os("FASTMASH_CONSOLE_EVIDENCE") {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::path::PathBuf::from(root);
        fs::create_dir_all(&root).unwrap();
        let index = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        fs::write(root.join(format!("{index}.bin")), &bytes).unwrap();
        fs::write(
            root.join(format!("{index}.txt")),
            format!("args={args:?}\nstatus={status:?}\nno_color={no_color}\n"),
        )
        .unwrap();
    }
    assert!(
        status.is_some_and(|status| status.success()),
        "console command failed: {}",
        String::from_utf8_lossy(&bytes)
    );
    (
        String::from_utf8(bytes).expect("console emits valid UTF-8"),
        held_bytes.map(|bytes| String::from_utf8(bytes).expect("held console emits valid UTF-8")),
    )
}

#[test]
fn console_unicode_crosses_application_buffer_boundaries_without_replacement() {
    let root =
        std::env::temp_dir().join(format!("fastmash-console-unicode-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let before = root.join("before é 🦀.tsv");
    let after = root.join("after é 🦀.tsv");
    let key = "é🦀".repeat(2000);
    fs::write(&before, format!("{key}\t2\n")).unwrap();
    fs::write(&after, format!("{key}\t5\n")).unwrap();
    let args = [
        OsString::from("--csv-out"),
        "-g1".into(),
        "compare".into(),
        before.into_os_string(),
        after.into_os_string(),
        "sum".into(),
        "2".into(),
    ];
    let text = console(&args, true);
    fs::remove_dir_all(root).unwrap();
    assert!(!text.contains('\u{fffd}'));
    assert_eq!(
        text.matches('🦀').count(),
        2000,
        "console lost or duplicated Unicode scalars"
    );
}

#[test]
fn automatic_console_color_and_no_color_use_the_actual_terminal() {
    let args = [OsString::from("--help")];
    let colored = console(&args, false);
    let plain = console(&args, true);
    assert!(colored.contains("Usage:"));
    assert!(plain.contains("Usage:"));
    assert!(
        colored.contains("\u{1b}[38;2;189;147;249")
            || colored.contains("\u{1b}[1;38;2;189;147;249"),
        "console heading palette absent: {colored:?}"
    );
    assert!(!plain.contains("189;147;249"));
}

#[test]
fn console_flushes_a_complete_header_before_input_eof() {
    let compiled = PathBuf::from(env!("CARGO_BIN_EXE_fastmash"));
    let binary = std::env::var_os("FASTMASH_TEST_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| compiled.clone());
    let helper = compiled
        .parent()
        .unwrap()
        .join("fastmash-corpus-test-command.exe");
    assert!(
        helper.is_file(),
        "build the corpus driver binaries before this test"
    );
    let root = std::env::temp_dir().join(format!("fastmash-console-held-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let ready = root.join("ready");
    let release = root.join("release");
    let args = [
        "console-held".into(),
        binary.into_os_string(),
        ready.clone().into_os_string(),
        release.clone().into_os_string(),
    ];
    let (complete, held) = console_command(&helper, &args, true, Some((&ready, &release)));
    fs::remove_dir_all(root).unwrap();
    let held = held.expect("console child must remain alive until input EOF");
    assert!(
        held.contains("sum(value)"),
        "header was buffered until EOF: {held:?}"
    );
    assert!(
        !held.contains("\r\n3\r\n"),
        "aggregate completed before EOF: {held:?}"
    );
    assert!(
        complete.contains("\r\n3\r\n"),
        "aggregate missing after EOF: {complete:?}"
    );
}
