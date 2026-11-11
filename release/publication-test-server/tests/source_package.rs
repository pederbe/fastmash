#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "support/copy_executable.rs"]
mod copy_executable;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-source-package-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        copy_executable::copy_executable(
            env!("CARGO_BIN_EXE_fastmash-source-package-fixture"),
            path.join("fastmash"),
        );
        Self(path)
    }

    fn run(&self, mode: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-test-source-package"))
            .arg(&self.0)
            .env("SOURCE_PACKAGE_FIXTURE", mode)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn executable_copy_preserves_opaque_paths_and_reports_errors() {
    use std::os::unix::ffi::OsStringExt;

    let fixture = Fixture::new();
    // macOS filesystems require Unicode names. Linux also accepts opaque bytes.
    let (source_name, destination_name): (&[u8], &[u8]) = if cfg!(target_os = "linux") {
        (b"source-\xff", b"copy-\xfe")
    } else {
        ("source-é".as_bytes(), "copy-é".as_bytes())
    };
    let source = fixture
        .0
        .join(std::ffi::OsString::from_vec(source_name.to_vec()));
    let destination = fixture
        .0
        .join(std::ffi::OsString::from_vec(destination_name.to_vec()));
    fs::rename(fixture.0.join("fastmash"), &source).unwrap();
    copy_executable::copy_executable(&source, &destination);
    assert_eq!(fs::read(&source).unwrap(), fs::read(&destination).unwrap());
    assert_eq!(
        fs::metadata(&source).unwrap().permissions().mode(),
        fs::metadata(&destination).unwrap().permissions().mode()
    );
    let output = Command::new(&destination)
        .arg("--version")
        .env_clear()
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fastmash fixture\n");

    let missing = fixture.0.join("missing");
    let failed = Command::new(env!("CARGO_BIN_EXE_fastmash-source-package-fixture"))
        .arg("--copy-executable")
        .arg(&missing)
        .arg(fixture.0.join("failed"))
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2), "{failed:?}");
    assert!(failed.stdout.is_empty(), "{failed:?}");
    assert!(
        String::from_utf8_lossy(&failed.stderr).contains("missing"),
        "{failed:?}"
    );
    assert!(!fixture.0.join("failed").exists());
}

#[test]
#[cfg(target_os = "linux")]
fn copied_release_fixture_starts_during_parallel_launches() {
    use std::sync::atomic::AtomicBool;

    let stop = AtomicBool::new(false);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let stop = &stop;
            scope.spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    let output =
                        Command::new(env!("CARGO_BIN_EXE_fastmash-source-package-fixture"))
                            .arg("--version")
                            .env_clear()
                            .output()
                            .unwrap();
                    assert!(output.status.success(), "{output:?}");
                }
            });
        }
        let result = std::panic::catch_unwind(|| {
            for attempt in 0..1000 {
                let fixture = Fixture::new();
                let output = Command::new(fixture.0.join("fastmash"))
                    .arg("--version")
                    .env_clear()
                    .output()
                    .unwrap_or_else(|error| panic!("fixture startup attempt={attempt}: {error}"));
                assert!(output.status.success(), "attempt={attempt}: {output:?}");
                assert_eq!(output.stdout, b"fastmash fixture\n");
                assert!(output.stderr.is_empty(), "attempt={attempt}: {output:?}");
            }
        });
        stop.store(true, Ordering::Relaxed);
        if let Err(error) = result {
            std::panic::resume_unwind(error);
        }
    });
}

#[test]
fn successful_source_package_checks_keep_version_output() {
    let fixture = Fixture::new();
    let output = fixture.run("");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"fastmash fixture\n");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn wrong_bytes_and_failed_commands_with_matching_bytes_are_rejected() {
    let fixture = Fixture::new();
    for mode in ["wrong-output", "failed-command"] {
        let output = fixture.run(mode);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(output.stdout, b"fastmash fixture\n");
        assert!(!output.stderr.is_empty());
    }
}

#[test]
fn missing_and_nonexecutable_commands_are_rejected() {
    for missing in [true, false] {
        let fixture = Fixture::new();
        let supervisor = fixture.0.join("fastmash");
        if missing {
            fs::remove_file(&supervisor).unwrap();
        } else {
            fs::set_permissions(&supervisor, fs::Permissions::from_mode(0o644)).unwrap();
        }
        let output = fixture.run("");
        assert_eq!(output.status.code(), Some(1));
        let text = String::from_utf8_lossy(&output.stderr);
        assert!(text.contains(&supervisor.display().to_string()));
        assert!(text.contains(if missing {
            "cannot inspect installed executable"
        } else {
            "not an executable file"
        }));
    }
}

#[test]
fn failed_version_and_missing_program_are_rejected() {
    let fixture = Fixture::new();
    assert_eq!(fixture.run("failed-version").status.code(), Some(1));
    fs::remove_file(fixture.0.join("fastmash")).unwrap();
    assert_eq!(fixture.run("").status.code(), Some(1));
}

#[test]
fn an_explicit_binary_directory_is_required() {
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-test-source-package"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("BIN_DIR"));
}
