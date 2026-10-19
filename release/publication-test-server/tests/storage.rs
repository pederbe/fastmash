//! Exercise the local storage executable with independent files and write counts.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-publication-storage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        let fixture = Self(path);
        fs::create_dir(fixture.0.join("source")).unwrap();
        fs::create_dir(fixture.0.join("live")).unwrap();
        fs::write(fixture.0.join("count"), b"0\n").unwrap();
        fixture
    }

    fn put(&self, area: &str, path: impl AsRef<Path>, bytes: &[u8]) {
        let target = self.0.join(area).join(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(target, bytes).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash-publication-test-storage"));
        command
            .current_dir(self.0.join("source"))
            .env("PUBLICATION_LIVE", self.0.join("live"))
            .env("PUBLICATION_COUNT", self.0.join("count"))
            .env_remove("PUBLICATION_STOP_AFTER")
            .env("LC_ALL", "C");
        command
    }

    fn run(&self, args: &[&str], stop: Option<&str>) -> Output {
        let mut command = self.command();
        command.args(args);
        if let Some(stop) = stop {
            command.env("PUBLICATION_STOP_AFTER", stop);
        }
        command.output().unwrap()
    }

    fn count(&self, expected: &[u8]) {
        assert_eq!(fs::read(self.0.join("count")).unwrap(), expected);
    }

    fn live(&self, path: impl AsRef<Path>, expected: &[u8]) {
        assert_eq!(fs::read(self.0.join("live").join(path)).unwrap(), expected);
    }

    fn no_pending(&self) {
        fn visit(path: &Path) {
            for entry in fs::read_dir(path).unwrap() {
                let entry = entry.unwrap();
                assert!(!entry.file_name().as_encoded_bytes().ends_with(b".pending"));
                if entry.file_type().unwrap().is_dir() {
                    visit(&entry.path());
                }
            }
        }
        visit(&self.0.join("live"));
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn status(output: Output, expected: i32) {
    assert_eq!(output.status.code(), Some(expected), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    if expected == 0 || expected == 75 {
        assert!(output.stderr.is_empty(), "{output:?}");
    } else {
        assert!(!output.stderr.is_empty(), "{output:?}");
    }
}

#[test]
fn sync_preserves_exclusions_nested_bytes_and_unchanged_objects() {
    let fixture = Fixture::new();
    fixture.put("source", "keep/equal.bin", b"\0\xffsame\n");
    fixture.put("live", "keep/equal.bin", b"\0\xffsame\n");
    fixture.put("source", "keep/changed", b"new");
    fixture.put("live", "keep/changed", b"old");
    fixture.put("source", "nested/package.rpm", b"rpm bytes");
    fixture.put("source", "nested/fastmash.asc", b"nested key");
    fixture.put("source", "rpm/mirrorlist.more", b"ordinary object");
    fixture.put("live", "old/snapshot", b"retained");
    fixture.put("live", "fastmash.asc", b"previous key");
    let excluded = [
        "apt/dists/stable/main/binary-amd64/Packages",
        "apt/dists/stable/main/binary-amd64/Packages.gz",
        "apt/dists/stable/Release",
        "apt/dists/stable/Release.gpg",
        "apt/dists/stable/InRelease",
        "rpm/mirrorlist",
        "fastmash.asc",
        "fastmash.gpg",
        "fastmash.repo",
    ];
    for path in excluded {
        fixture.put("source", path, b"excluded new bytes");
    }
    std::os::unix::fs::symlink("keep/changed", fixture.0.join("source/link")).unwrap();
    let args = [
        "s3",
        "sync",
        "--only-show-errors",
        ".",
        "s3://test-bucket",
        "--exclude",
        "apt/dists/stable/main/binary-amd64/Packages*",
        "--exclude",
        "apt/dists/stable/Release*",
        "--exclude",
        "apt/dists/stable/InRelease",
        "--exclude",
        "rpm/mirrorlist",
        "--exclude",
        "fastmash.*",
        "--cache-control",
        "public, max-age=31536000, immutable",
    ];
    status(fixture.run(&args, None), 0);
    fixture.count(b"4\n");
    fixture.live("keep/equal.bin", b"\0\xffsame\n");
    fixture.live("keep/changed", b"new");
    fixture.live("nested/package.rpm", b"rpm bytes");
    fixture.live("nested/fastmash.asc", b"nested key");
    fixture.live("rpm/mirrorlist.more", b"ordinary object");
    fixture.live("old/snapshot", b"retained");
    fixture.live("fastmash.asc", b"previous key");
    for path in excluded.into_iter().filter(|path| *path != "fastmash.asc") {
        assert!(!fixture.0.join("live").join(path).exists());
    }
    assert!(!fixture.0.join("live/link").exists());
    status(fixture.run(&args, None), 0);
    fixture.count(b"4\n");
    fixture.put("source", "keep/changed", b"newer");
    status(fixture.run(&args, None), 0);
    fixture.count(b"5\n");
    fixture.live("keep/changed", b"newer");
    fixture.no_pending();
}

#[test]
fn copy_always_writes_with_persistent_counts_and_absolute_stop() {
    let fixture = Fixture::new();
    fixture.put("source", "one", b"\0complete\xff");
    fixture.put("live", "nested/target", b"\0complete\xff");
    let args = [
        "s3",
        "cp",
        "--cache-control",
        "no-cache",
        "one",
        "s3://test-bucket/nested/target",
        "--only-show-errors",
    ];
    status(fixture.run(&args, None), 0);
    fixture.count(b"1\n");
    status(fixture.run(&args, Some("2")), 75);
    fixture.count(b"2\n");
    fixture.live("nested/target", b"\0complete\xff");
    status(fixture.run(&args, Some("2")), 0);
    fixture.count(b"3\n");
    // The old adapter compares the stop string with the canonical written count.
    status(fixture.run(&args, Some("04")), 0);
    fixture.count(b"4\n");
    fixture.no_pending();
}

#[test]
fn every_sorted_write_boundary_is_visible_and_resumes_without_recounting() {
    let cases: [(&str, &[u8], &[&str]); 3] = [
        ("1", b"1\n", &["a"]),
        ("2", b"2\n", &["a", "nested/b"]),
        ("3", b"3\n", &["a", "nested/b", "z"]),
    ];
    for (stop, count, visible) in cases {
        let fixture = Fixture::new();
        fixture.put("source", "z", b"last");
        fixture.put("source", "nested/b", b"middle");
        fixture.put("source", "a", b"first");
        let args = ["s3", "sync", ".", "s3://test-bucket"];
        status(fixture.run(&args, Some(stop)), 75);
        fixture.count(count);
        for (path, bytes) in [
            ("a", b"first".as_slice()),
            ("nested/b", b"middle".as_slice()),
            ("z", b"last".as_slice()),
        ] {
            if visible.contains(&path) {
                fixture.live(path, bytes);
            } else {
                assert!(!fixture.0.join("live").join(path).exists());
            }
        }
        fixture.no_pending();
        status(fixture.run(&args, None), 0);
        fixture.count(b"3\n");
        fixture.live("a", b"first");
        fixture.live("nested/b", b"middle");
        fixture.live("z", b"last");
        fixture.no_pending();
    }
}

#[test]
fn nul_sorted_paths_preserve_quotes_newlines_and_unicode() {
    let fixture = Fixture::new();
    fixture.put("source", "nested space/Å", b"third\0");
    fixture.put("source", "line\n'\"é", b"second\r\n\xff");
    fixture.put("source", "-dash", b"first\0");
    let args = ["s3", "sync", ".", "s3://test-bucket"];
    status(fixture.run(&args, Some("2")), 75);
    fixture.count(b"2\n");
    fixture.live("-dash", b"first\0");
    fixture.live("line\n'\"é", b"second\r\n\xff");
    assert!(!fixture.0.join("live/nested space/Å").exists());
    status(fixture.run(&args, None), 0);
    fixture.count(b"3\n");
    fixture.live("nested space/Å", b"third\0");
    fixture.no_pending();
}

#[cfg(target_os = "linux")]
#[test]
fn linux_non_utf8_source_and_object_arguments_preserve_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let fixture = Fixture::new();
    let raw = OsString::from_vec(b"raw-\xff\x80".to_vec());
    fixture.put("source", &raw, b"raw body\0\xff");
    fixture.put("source", "a", b"ordinary");
    let args = ["s3", "sync", ".", "s3://test-bucket"];
    status(fixture.run(&args, Some("1")), 75);
    fixture.count(b"1\n");
    fixture.live("a", b"ordinary");
    assert!(!fixture.0.join("live").join(&raw).exists());
    status(fixture.run(&args, None), 0);
    fixture.count(b"2\n");
    fixture.live(&raw, b"raw body\0\xff");
    let destination = OsString::from_vec(b"s3://test-bucket/raw-\xff\x80".to_vec());
    status(
        fixture
            .command()
            .args(["s3", "cp"])
            .arg(&raw)
            .arg(&destination)
            .output()
            .unwrap(),
        0,
    );
    fixture.count(b"3\n");
    fixture.live(&raw, b"raw body\0\xff");
    fixture.no_pending();
}

#[test]
fn invocation_and_write_errors_keep_status_one_and_remove_pending_files() {
    let fixture = Fixture::new();
    fixture.put("source", "one", b"complete new body");
    let invalid: &[&[&str]] = &[
        &[],
        &["s3", "remove", "one", "s3://test-bucket/target"],
        &["s3", "cp", "one", "s3://other/target"],
        &["s3", "cp", "one", "s3://test-bucket/target", "--exclude"],
        &["s3", "sync", ".", "s3://test-bucket", "--cache-control"],
        &[
            "s3",
            "sync",
            ".",
            "s3://test-bucket",
            "--exclude",
            "keep/*/file",
        ],
    ];
    for args in invalid {
        status(fixture.run(args, None), 1);
        fixture.count(b"0\n");
        assert!(!fixture.0.join("live/target").exists());
    }
    fixture.put("live", "blocked/child", b"preserved");
    status(
        fixture.run(&["s3", "cp", "one", "s3://test-bucket/blocked"], None),
        1,
    );
    fixture.live("blocked/child", b"preserved");
    fixture.count(b"0\n");
    fixture.no_pending();
    fs::remove_file(fixture.0.join("count")).unwrap();
    status(
        fixture.run(&["s3", "cp", "one", "s3://test-bucket/visible"], None),
        1,
    );
    // Rename is complete before the counter read fails, as in the existing adapter.
    fixture.live("visible", b"complete new body");
    assert!(!fixture.0.join("count").exists());
    fixture.no_pending();
}
