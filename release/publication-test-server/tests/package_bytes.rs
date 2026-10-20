//! Exercise the package byte oracle through its actual command interface.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const PROGRAMS: [&str; 2] = ["fastmash", "fastmash-sort-supervisor"];

struct Fixture {
    root: PathBuf,
    bin: PathBuf,
    package: PathBuf,
}

impl Fixture {
    fn new(bin_name: impl AsRef<Path>, package_name: impl AsRef<Path>) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fastmash-package-bytes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let fixture = Self {
            bin: root.join(bin_name),
            package: root.join(package_name),
            root,
        };
        fs::create_dir(&fixture.bin).unwrap();
        fs::create_dir_all(fixture.package.join("usr/bin")).unwrap();
        for (program, bytes) in [
            (PROGRAMS[0], b"\0\xffqualified CLI\n".repeat(1400)),
            (PROGRAMS[1], b"sort\0supervisor\xfe\n".repeat(1600)),
        ] {
            fs::write(fixture.bin.join(program), &bytes).unwrap();
            fs::write(fixture.packaged(program), &bytes).unwrap();
        }
        fixture
    }

    fn packaged(&self, program: &str) -> PathBuf {
        self.package.join("usr/bin").join(program)
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-check-package-bytes"))
            .arg(&self.bin)
            .arg(&self.package)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn changed(output: Output, program: &str) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(
        output.stderr,
        format!("{program} changed during packaging\n").as_bytes()
    );
}

#[test]
fn equal_binary_files_succeed_silently_across_buffer_boundaries() {
    let fixture = Fixture::new("qualified", "package");
    let output = fixture.run();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn changed_truncated_and_extended_programs_fail() {
    for program in PROGRAMS {
        for change in ["changed", "truncated", "extended"] {
            let fixture = Fixture::new("qualified", "package");
            let path = fixture.packaged(program);
            let mut bytes = fs::read(&path).unwrap();
            match change {
                "changed" => bytes[8193] ^= 1,
                "truncated" => {
                    bytes.pop().unwrap();
                }
                "extended" => bytes.push(0),
                _ => unreachable!(),
            }
            fs::write(path, bytes).unwrap();
            changed(fixture.run(), program);
        }
    }
}

#[test]
fn missing_qualified_or_packaged_programs_fail() {
    for program in PROGRAMS {
        for qualified in [true, false] {
            let fixture = Fixture::new("qualified", "package");
            let path = if qualified {
                fixture.bin.join(program)
            } else {
                fixture.packaged(program)
            };
            fs::remove_file(path).unwrap();
            changed(fixture.run(), program);
        }
    }
}

#[test]
fn a_directory_in_place_of_a_program_is_a_read_failure() {
    for program in PROGRAMS {
        let fixture = Fixture::new("qualified", "package");
        let path = fixture.packaged(program);
        fs::remove_file(&path).unwrap();
        fs::create_dir(path).unwrap();
        changed(fixture.run(), program);
    }
}

#[test]
fn path_arguments_preserve_spaces_newlines_and_non_utf8_bytes() {
    let fixture = Fixture::new(
        OsString::from_vec(b"qualified \n\xff".to_vec()),
        OsString::from_vec(b"package \t\xfe".to_vec()),
    );
    let output = fixture.run();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn the_cli_requires_exactly_two_paths() {
    for args in [vec![], vec!["only-one"], vec!["bin", "root", "extra"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-check-package-bytes"))
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty(), "{output:?}");
        assert_eq!(
            output.stderr,
            b"usage: fastmash-check-package-bytes BIN_DIR PACKAGE_ROOT\n"
        );
    }
}
