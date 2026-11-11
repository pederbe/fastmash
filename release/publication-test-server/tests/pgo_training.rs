//! A failed training command must never supply a usable compiler profile.
#![cfg(target_os = "linux")]

use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn rejects_wrong_stdout_and_unsuccessful_training_commands() {
    let root = PathBuf::from("/var/tmp").join(format!(
        "fastmash-pgo-training-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    for program in ["true", "false"] {
        let destination = root.join(program);
        fs::create_dir(&destination).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-pgo-train"))
            .arg(format!("/usr/bin/{program}"))
            .arg(destination.join("training"))
            .arg(destination.join("raw"))
            .output()
            .unwrap();
        assert!(!output.status.success(), "{program}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("PGO training tiny changed"),
            "{output:?}"
        );
        assert!(destination.join("training/tiny-0.stdout").is_file());
        assert!(!destination.join("training/tiny-1.stdout").exists());
        let manifest = fs::read_to_string(destination.join("training/manifest.tsv")).unwrap();
        assert_eq!(manifest.lines().count(), 3);
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_profile_data_from_an_earlier_attempt() {
    let root = PathBuf::from("/var/tmp").join(format!(
        "fastmash-pgo-stale-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::create_dir(root.join("raw")).unwrap();
    fs::write(root.join("raw/stale.profraw"), b"old compiler profile").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-pgo-train"))
        .arg("/usr/bin/true")
        .arg(root.join("training"))
        .arg(root.join("raw"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!root.join("training/manifest.tsv").exists());
    assert_eq!(
        fs::read(root.join("raw/stale.profraw")).unwrap(),
        b"old compiler profile"
    );
    fs::remove_dir_all(root).unwrap();
}
