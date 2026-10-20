#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Files(PathBuf);

impl Files {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-archive-check-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn check(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-check-archive"))
            .arg("bytes")
            .arg(self.0.join("original"))
            .arg(self.0.join("copy"))
            .output()
            .unwrap()
    }
}

impl Drop for Files {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bytes_cover_empty_large_altered_truncated_missing_and_unreadable_files() {
    let files = Files::new();
    for bytes in [vec![], vec![0xa7; 32769]] {
        fs::write(files.0.join("original"), &bytes).unwrap();
        fs::write(files.0.join("copy"), bytes).unwrap();
        let output = files.check();
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty() && output.stderr.is_empty());
    }
    for bytes in [vec![0xa6; 32769], vec![0xa7; 32768]] {
        fs::write(files.0.join("copy"), bytes).unwrap();
        assert_eq!(files.check().status.code(), Some(1));
    }
    fs::remove_file(files.0.join("copy")).unwrap();
    assert_eq!(files.check().status.code(), Some(1));
    fs::create_dir(files.0.join("copy")).unwrap();
    assert_eq!(files.check().status.code(), Some(1));
}

#[test]
fn deployment_metadata_accepts_only_the_retained_macos_15_forms() {
    for (metadata, valid) in [
        (
            "Load command 9\n      cmd LC_BUILD_VERSION\n    minos 15.0\n      sdk 26.0\n",
            true,
        ),
        ("\tminos 15.0.0\n", true),
        ("minos 15.0", true),
        ("    minos 14.0\n", false),
        ("    minos 15.1\n", false),
        ("    minos 15.0 ", false),
        ("    minos 15.0.0.0", false),
        ("    minos 15.0\r\n", false),
        ("    minos15.0", false),
        ("    minos\t15.0", false),
        ("    minos  15.0", false),
        ("binary-minos 15.0", false),
        ("    sdk 15.0", false),
        ("", false),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-check-archive"))
            .args(["deployment", metadata])
            .output()
            .unwrap();
        assert_eq!(output.status.success(), valid, "{metadata:?}: {output:?}");
        assert!(output.stdout.is_empty());
        if !valid {
            assert_eq!(
                output.stderr,
                b"the binary does not record the macOS 15 deployment target\n"
            );
        }
    }
}

#[test]
fn incomplete_and_unknown_invocations_are_rejected() {
    for args in [vec![], vec!["bytes"], vec!["deployment"], vec!["unknown"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-check-archive"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("usage:"));
    }
}
