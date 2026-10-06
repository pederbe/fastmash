//! Full-output faults for direct native commands, with Linux's device intact.
use std::{
    fs::File,
    process::{Command, Stdio},
};
#[cfg(target_os = "macos")]
#[path = "temp_dir.rs"]
// Integration suites may also use this private fixture helper directly.
#[allow(clippy::duplicate_mod)]
mod temp_dir;

pub trait OutputFault {
    fn full_stdout(&mut self, full: bool) -> &mut Self;
    #[allow(dead_code)]
    fn full_stderr(&mut self) -> &mut Self;
    #[cfg(target_os = "macos")]
    #[allow(dead_code)]
    fn read_error_at_eof(&mut self) -> &mut Self;
}

impl OutputFault for Command {
    fn full_stdout(&mut self, full: bool) -> &mut Self {
        if full {
            full_stream(self, 1);
        } else {
            self.stdout(Stdio::piped());
        }
        self
    }

    fn full_stderr(&mut self) -> &mut Self {
        full_stream(self, 2);
        self
    }

    #[cfg(target_os = "macos")]
    fn read_error_at_eof(&mut self) -> &mut Self {
        native::configure_read(self);
        self
    }
}

fn full_stream(command: &mut Command, fd: i32) {
    #[cfg(target_os = "linux")]
    let destination = File::options().write(true).open("/dev/full").unwrap();
    #[cfg(target_os = "macos")]
    let destination = {
        native::configure(command, fd);
        File::options().write(true).open("/dev/null").unwrap()
    };
    match fd {
        1 => {
            command.stdout(destination);
        }
        2 => {
            command.stderr(destination);
        }
        _ => unreachable!(),
    }
}

#[cfg(target_os = "macos")]
mod native {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        collections::HashSet,
        ffi::OsString,
        fs,
        io::Write,
        os::{
            fd::OwnedFd,
            unix::{net::UnixStream, process::CommandExt},
        },
        path::{Path, PathBuf},
        sync::{Mutex, OnceLock},
    };

    struct Fixture {
        directory: Option<temp_dir::TempDir>,
        library: PathBuf,
        probe: PathBuf,
    }
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    static CALIBRATED: Mutex<Option<HashSet<OsString>>> = Mutex::new(None);
    static READ_CALIBRATED: Mutex<Option<HashSet<OsString>>> = Mutex::new(None);

    fn bounded(command: &mut Command) {
        command.process_group(0);
        // SAFETY: alarm is async-signal-safe and bounds each test child before
        // exec. Command's output method always waits for and reaps that child.
        unsafe {
            command.pre_exec(|| {
                libc::alarm(30);
                Ok(())
            });
        }
    }

    fn output(command: &mut Command) -> std::process::Output {
        bounded(command);
        command.output().unwrap()
    }

    fn input(command: &mut Command, bytes: &[u8]) {
        let (reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(bytes).unwrap();
        drop(writer);
        command.stdin(OwnedFd::from(reader));
    }

    extern "C" fn cleanup() {
        if let Some(fixture) = FIXTURE.get()
            && let Some(directory) = &fixture.directory
        {
            let _ = fs::remove_dir_all(&directory.0);
        }
    }

    fn compile(source: &str, destination: &Path, library: bool) {
        let mut command = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
        command
            .arg("--edition=2024")
            .args(["-C", "panic=abort", "-C", "opt-level=2"])
            .arg(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("tests/support")
                    .join(source),
            )
            .arg("-o")
            .arg(destination)
            .env_remove("DYLD_INSERT_LIBRARIES")
            .env_remove("FASTMASH_TEST_FULL_FD")
            .env_remove("FASTMASH_TEST_READ_ERROR");
        if library {
            command.arg("--crate-type=cdylib");
        }
        let output = output(&mut command);
        assert!(
            output.status.success(),
            "native output-fault helper build: {output:?}"
        );
    }

    fn fixture() -> &'static Fixture {
        FIXTURE.get_or_init(|| {
            let fixture =
                if let Some(library) = std::env::var_os("FASTMASH_TEST_FULL_OUTPUT_LIBRARY") {
                    let library = fs::canonicalize(library).expect("native output-fault library");
                    let probe = std::env::var_os("FASTMASH_TEST_FULL_OUTPUT_PROBE")
                        .map(PathBuf::from)
                        .unwrap_or_else(|| library.with_file_name("full-output-probe"));
                    Fixture {
                        directory: None,
                        library,
                        probe: fs::canonicalize(probe).expect("native output-fault probe"),
                    }
                } else {
                    let directory = temp_dir::TempDir::new("output-interposer");
                    let library = directory.0.join("full-output.dylib");
                    let probe = directory.0.join("full-output-probe");
                    compile("full_output_interposer.rs", &library, true);
                    compile("full_output_probe.rs", &probe, false);
                    // SAFETY: this zero-argument callback removes only the fixture
                    // directory we own; no pointers or parent environment change.
                    assert_eq!(unsafe { libc::atexit(cleanup) }, 0);
                    Fixture {
                        directory: Some(directory),
                        library,
                        probe,
                    }
                };
            for path in [&fixture.library, &fixture.probe] {
                let hash: String = Sha256::digest(fs::read(path).unwrap())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect();
                eprintln!(
                    "native output-fault helper {} sha256={hash}",
                    path.display()
                );
            }
            for (fd, loaded) in [(0, false), (0, true), (1, true), (2, true)] {
                let mut command = Command::new(&fixture.probe);
                command.arg(fd.to_string()).env_clear();
                if loaded {
                    command.env("DYLD_INSERT_LIBRARIES", &fixture.library);
                }
                if fd != 0 {
                    inject(&mut command, &fixture.library, fd);
                }
                let output = output(&mut command);
                assert!(
                    output.status.success(),
                    "native write interposition calibration failed for fd {fd}: {output:?}"
                );
                assert_eq!(
                    output.stdout,
                    if fd == 1 { b"".as_slice() } else { b"stdout\n" }
                );
                assert_eq!(
                    output.stderr,
                    if fd == 2 { b"".as_slice() } else { b"stderr\n" }
                );
            }
            for selector in [None, Some("0"), Some("11"), Some("1")] {
                let mut command = Command::new(&fixture.probe);
                command
                    .args([
                        "read",
                        if selector == Some("1") {
                            "error"
                        } else {
                            "eof"
                        },
                    ])
                    .env_clear();
                command.env("DYLD_INSERT_LIBRARIES", &fixture.library);
                if let Some(selector) = selector {
                    command.env("FASTMASH_TEST_READ_ERROR", selector);
                }
                input(&mut command, b"complete\npartial");
                let result = output(&mut command);
                assert!(
                    result.status.success(),
                    "native read fault calibration failed for {selector:?}: {result:?}"
                );
                assert_eq!(result.stdout, b"complete\npartial");
                assert!(result.stderr.is_empty());
            }
            let mut access = Command::new(&fixture.probe);
            access
                .arg("read-access")
                .env_clear()
                .env("DYLD_INSERT_LIBRARIES", &fixture.library)
                .env("FASTMASH_TEST_READ_ERROR", "1")
                .stdin(File::options().write(true).open("/dev/null").unwrap());
            let result = output(&mut access);
            assert!(
                result.status.success() && result.stdout.is_empty() && result.stderr.is_empty(),
                "selected native read errors must remain unchanged: {result:?}"
            );
            fixture
        })
    }

    fn inject(command: &mut Command, library: &Path, fd: i32) {
        assert!(matches!(fd, 1 | 2));
        command
            .env("DYLD_INSERT_LIBRARIES", library)
            .env("FASTMASH_TEST_FULL_FD", fd.to_string());
    }

    fn calibrate(binary: &std::ffi::OsStr, library: &Path) {
        // Only a named candidate can use this mechanism. Never inject into a
        // protected system utility, compiler or separately observed GNU tool.
        let mut baseline = Command::new(binary);
        baseline
            .arg0("fastmash")
            .arg("--version")
            .env_clear()
            .env("LC_ALL", "C");
        let version = output(&mut baseline);
        assert!(
            version.status.success() && version.stdout.starts_with(b"fastmash "),
            "native output-fault target is not Fastmash: {version:?}"
        );
        let mut null = Command::new(binary);
        null.arg0("fastmash")
            .arg("--version")
            .env_clear()
            .env("LC_ALL", "C")
            .stdout(Stdio::null());
        let ordinary_null = output(&mut null);
        assert!(
            ordinary_null.status.success() && ordinary_null.stderr.is_empty(),
            "ordinary /dev/null calibration: {ordinary_null:?}"
        );
        let mut full = Command::new(binary);
        full.arg0("fastmash")
            .arg("--version")
            .env_clear()
            .env("LC_ALL", "C")
            .stdout(Stdio::null());
        inject(&mut full, library, 1);
        let full_output = output(&mut full);
        assert_eq!(
            full_output.status.code(),
            Some(1),
            "native full stdout: {full_output:?}"
        );
        assert_eq!(
            full_output.stderr,
            b"fastmash: write error: No space left on device\n"
        );
        let mut diagnostic = Command::new(binary);
        diagnostic
            .arg0("fastmash")
            .arg("--bad")
            .env_clear()
            .env("LC_ALL", "C");
        let ordinary = output(&mut diagnostic);
        assert_eq!(ordinary.status.code(), Some(1));
        assert!(!ordinary.stderr.is_empty());
        inject(&mut diagnostic, library, 1);
        let delegated = output(&mut diagnostic);
        assert_eq!(delegated.status, ordinary.status);
        assert_eq!(
            delegated.stderr, ordinary.stderr,
            "unselected stderr must delegate exactly"
        );
        inject(&mut diagnostic, library, 2);
        let failed = output(&mut diagnostic);
        assert_eq!(failed.status.code(), Some(1));
        assert!(
            failed.stdout.is_empty() && failed.stderr.is_empty(),
            "native full stderr: {failed:?}"
        );
        let mut unselected = Command::new(binary);
        unselected
            .arg0("fastmash")
            .arg("--version")
            .env_clear()
            .env("LC_ALL", "C");
        inject(&mut unselected, library, 2);
        let delegated = output(&mut unselected);
        assert_eq!(delegated.status, version.status);
        assert_eq!(
            delegated.stdout, version.stdout,
            "unselected stdout must delegate exactly"
        );
        assert!(delegated.stderr.is_empty());
    }

    pub(super) fn configure(command: &mut Command, fd: i32) {
        let fixture = fixture();
        let binary = command.get_program().to_os_string();
        let mut completed = CALIBRATED.lock().unwrap();
        let completed = completed.get_or_insert_with(HashSet::new);
        if !completed.contains(&binary) {
            calibrate(&binary, &fixture.library);
            completed.insert(binary);
        }
        inject(command, &fixture.library, fd);
        bounded(command);
    }

    pub(super) fn configure_read(command: &mut Command) {
        let fixture = fixture();
        let binary = command.get_program().to_os_string();
        let mut completed = READ_CALIBRATED.lock().unwrap();
        let completed = completed.get_or_insert_with(HashSet::new);
        if !completed.contains(&binary) {
            calibrate(&binary, &fixture.library);
            let mut control = Command::new(&binary);
            control
                .arg0("fastmash")
                .arg("reverse")
                .env_clear()
                .env("LC_ALL", "C");
            input(&mut control, b"complete\npartial");
            let ordinary = output(&mut control);
            assert!(ordinary.status.success(), "ordinary EOF: {ordinary:?}");
            assert_eq!(ordinary.stdout, b"complete\npartial\n");
            assert!(ordinary.stderr.is_empty());
            control.env("DYLD_INSERT_LIBRARIES", &fixture.library);
            input(&mut control, b"complete\npartial");
            let disabled = output(&mut control);
            assert_eq!(
                (disabled.status, disabled.stdout, disabled.stderr),
                (ordinary.status, ordinary.stdout, ordinary.stderr),
                "library-loaded disabled EOF must remain unchanged"
            );
            control.env("FASTMASH_TEST_READ_ERROR", "1");
            input(&mut control, b"complete\npartial");
            let failed = output(&mut control);
            assert_eq!(
                failed.status.code(),
                Some(1),
                "native read error: {failed:?}"
            );
            assert_eq!(failed.stdout, b"complete\n");
            assert_eq!(failed.stderr, b"fastmash: read error: Input/output error\n");
            inject(&mut control, &fixture.library, 1);
            input(&mut control, b"complete\npartial");
            let combined = output(&mut control);
            assert_eq!(
                combined.status.code(),
                Some(1),
                "combined native faults: {combined:?}"
            );
            assert!(combined.stdout.is_empty());
            assert_eq!(
                combined.stderr,
                b"fastmash: read error: Input/output error\nfastmash: write error\n"
            );
            completed.insert(binary);
        }
        command
            .env("DYLD_INSERT_LIBRARIES", &fixture.library)
            .env("FASTMASH_TEST_READ_ERROR", "1");
        bounded(command);
    }
}
