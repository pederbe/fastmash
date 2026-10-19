//! Exercise the frozen native-indexer adapter and pre-native repair failures.
#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-fixture-repair-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn indexer_cli_copies_frozen_metadata_and_removes_only_its_signature() {
    let fixture = Fixture::new();
    let reference = fixture.0.join("reference");
    fs::create_dir_all(reference.join("repodata")).unwrap();
    fs::write(reference.join("repodata/repomd.xml"), b"frozen index\n").unwrap();
    fs::write(
        reference.join("repodata/content-primary.xml.gz"),
        b"\0\xffprimary\n",
    )
    .unwrap();
    fs::write(
        reference.join("repodata/repomd.xml.asc"),
        b"original signature",
    )
    .unwrap();
    let indexer = fixture.0.join("createrepo_c");
    symlink(
        env!("CARGO_BIN_EXE_fastmash-test-package-fixture-repair"),
        &indexer,
    )
    .unwrap();
    let destination = fixture.0.join("output with spaces");
    let output = Command::new(&indexer)
        .args(["--quiet", "--location-prefix", "../../", "--outputdir"])
        .arg(&destination)
        .arg(fixture.0.join("unused-rpm-directory"))
        .env("PUBLICATION_REFERENCE", &reference)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        fs::read(destination.join("repodata/repomd.xml")).unwrap(),
        b"frozen index\n"
    );
    assert_eq!(
        fs::read(destination.join("repodata/content-primary.xml.gz")).unwrap(),
        b"\0\xffprimary\n"
    );
    assert!(!destination.join("repodata/repomd.xml.asc").exists());
    assert_eq!(
        fs::read(reference.join("repodata/repomd.xml.asc")).unwrap(),
        b"original signature"
    );
}

#[test]
fn missing_primary_cleans_owned_partial_and_preserves_preexisting_partial() {
    let fixture = Fixture::new();
    let generation = "b".repeat(64);
    let metadata = fixture
        .0
        .join("new/rpm/snapshots")
        .join(&generation)
        .join("repodata");
    fs::create_dir_all(&metadata).unwrap();
    fs::write(
        fixture.0.join("new/rpm/mirrorlist"),
        format!("https://packages.fastmash.io/rpm/snapshots/{generation}/\n"),
    )
    .unwrap();
    fs::write(metadata.join("repomd.xml.asc"), b"unchanged signature").unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_fastmash-test-package-fixture-repair"))
            .arg(&fixture.0)
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../package-repository.sh"))
            .env("PATH", "/nonexistent-fixture-repair-tools")
            .output()
            .unwrap()
    };
    let output = run();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("no primary metadata"),
        "{output:?}"
    );
    assert!(!fixture.0.join("partial").exists());
    assert!(!fixture.0.join("build").exists());
    assert_eq!(
        fs::read(metadata.join("repomd.xml.asc")).unwrap(),
        b"unchanged signature"
    );

    fs::create_dir(fixture.0.join("partial")).unwrap();
    fs::write(
        fixture.0.join("partial/retained"),
        b"not owned by the new invocation",
    )
    .unwrap();
    let output = run();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        fs::read(fixture.0.join("partial/retained")).unwrap(),
        b"not owned by the new invocation"
    );
}
