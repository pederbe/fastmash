//! Focused development checks. The full Windows command corpus is a later gate.
#![cfg(windows)]
#![forbid(unsafe_code)]

#[path = "../src/console_utf8.rs"]
mod console_utf8;

use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::ffi::OsStringExt,
    path::PathBuf,
    process::{Child, Command, Output, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "fastmash-windows-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn command(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash"));
    command
        .args(args)
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}
fn finish(mut child: Child) -> Output {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("command timed out: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn pipe(args: &[&str], bytes: &[u8]) -> Output {
    let mut child = command(args).stdin(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    finish(child)
}

#[test]
fn redirected_bytes_are_binary_and_unicode_paths_work() {
    let fixture = Fixture::new();
    let bytes = b"a\0\r\x1a\xff\xc3\xa9\n";
    let input = fixture.file("input é 🦀.tsv", bytes);
    let output = fixture.0.join("output é 🦀.tsv");
    let child = command(&["cut", "1"])
        .stdin(File::open(input).unwrap())
        .stdout(File::create(&output).unwrap())
        .spawn()
        .unwrap();
    let result = finish(child);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(fs::read(output).unwrap(), bytes);
    let result = pipe(&["cut", "1"], bytes);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, bytes);
}

#[test]
fn pipe_and_disk_grouping_agree() {
    let fixture = Fixture::new();
    let bytes = b"a\t1\nb\t2\na\t3\n";
    let input = fixture.file("groups", bytes);
    let args = ["-g", "1", "sum", "2"];
    let piped = pipe(&args, bytes);
    let disk = finish(
        command(&args)
            .stdin(File::open(input).unwrap())
            .spawn()
            .unwrap(),
    );
    assert!(piped.status.success(), "{piped:?}");
    assert!(disk.status.success(), "{disk:?}");
    assert_eq!(disk.stdout, b"a\t4\nb\t2\n");
    assert_eq!(piped.stdout, disk.stdout);
}

#[test]
fn wrong_access_and_broken_output_are_errors() {
    let fixture = Fixture::new();
    let path = fixture.file("wrong-access", b"1\n");
    let output = finish(
        command(&["sum", "1"])
            .stdin(OpenOptions::new().write(true).open(&path).unwrap())
            .spawn()
            .unwrap(),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!output.stderr.is_empty());
    let output = finish(
        command(&["sum", "1"])
            .stdin(File::open(&path).unwrap())
            .stdout(File::open(&path).unwrap())
            .spawn()
            .unwrap(),
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!output.stderr.is_empty());
    let mut child = command(&["sum", "1"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // The calculation cannot finish before input EOF, so close its reader first.
    drop(child.stdout.take());
    child.stdin.take().unwrap().write_all(b"1\n").unwrap();
    let output = finish(child);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!output.stderr.is_empty());
}

#[test]
fn unpaired_utf16_arguments_are_refused() {
    let output = command(&["sum"])
        .arg(OsString::from_wide(&[0xd800]))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(77), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("non-Unicode"));
}

#[test]
fn forced_spill_matches_memory_sort_and_cleans_up() {
    let fixture = Fixture::new();
    let bytes = (0..2000)
        .rev()
        .map(|n| format!("{}\t1\n", n % 37))
        .collect::<String>();
    let input = fixture.file("input", bytes.as_bytes());
    let spill = fixture.0.join("spill");
    fs::create_dir(&spill).unwrap();
    let args = ["-s", "-g", "1", "sum", "2"];
    let expected = pipe(&args, bytes.as_bytes());
    assert!(expected.status.success(), "{expected:?}");
    let actual = finish(
        command(&args)
            .env("TEMP", &spill)
            .env("TMP", &spill)
            .env("FASTMASH_SORT_MEMORY_BYTES", "4096")
            .stdin(File::open(&input).unwrap())
            .spawn()
            .unwrap(),
    );
    assert!(actual.status.success(), "{actual:?}");
    assert_eq!(actual.stdout, expected.stdout);
    assert_eq!(fs::read_dir(&spill).unwrap().count(), 0);
    // A missing temporary directory must fail rather than discard spill data.
    let failed = finish(
        command(&args)
            .env("TEMP", spill.join("absent"))
            .env("TMP", spill.join("absent"))
            .env("FASTMASH_SORT_MEMORY_BYTES", "4096")
            .stdin(File::open(input).unwrap())
            .spawn()
            .unwrap(),
    );
    assert!(!failed.status.success(), "{failed:?}");
    assert!(failed.stdout.is_empty());
}

#[test]
fn termination_removes_open_spill_files() {
    let fixture = Fixture::new();
    let mut child = command(&["-s", "-g", "1", "sum", "2"])
        .env("TEMP", &fixture.0)
        .env("TMP", &fixture.0)
        .env("FASTMASH_SORT_MEMORY_BYTES", "4096")
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    // Enough complete records to spill, while keeping input open to hold the process.
    input.write_all(&b"a\t1\n".repeat(4096)).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while fs::read_dir(&fixture.0).unwrap().count() == 0 {
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("no spill file observed");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let path = fs::read_dir(&fixture.0)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert!(
        File::open(path).is_err(),
        "spill file permits a second opener"
    );
    child.kill().unwrap();
    child.wait().unwrap();
    drop(input);
    assert_eq!(fs::read_dir(&fixture.0).unwrap().count(), 0);
}
