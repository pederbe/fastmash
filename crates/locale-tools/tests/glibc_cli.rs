#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "glibc-generator-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    root
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fastmash-generate-glibc-collation"))
}

#[test]
fn help_and_selection_errors_leave_outputs_and_captures_untouched() {
    assert!(command().arg("--help").output().unwrap().status.success());
    let root = root();
    let paths = [
        root.join("sort"),
        root.join("locale"),
        root.join("locpath"),
        root.join("common"),
        root.join("data.json"),
        root.join("table.rs"),
    ];
    let captures = root.join("captures");
    for options in [
        vec!["--locale", "outside_the_sorting_table"],
        vec!["--locale", "en_US", "--locale", "en_US"],
    ] {
        let output = command()
            .arg("--evidence")
            .arg(&captures)
            .args(options)
            .arg("--")
            .args(&paths)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(!captures.exists());
        assert!(!paths[4].exists());
        assert!(!paths[5].exists());
    }
    fs::write(&paths[4], b"preserve this existing output").unwrap();
    assert!(
        !command()
            .arg("--evidence")
            .arg(&captures)
            .arg("--")
            .args(&paths)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read(&paths[4]).unwrap(),
        b"preserve this existing output"
    );
    assert!(!captures.exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rejected_native_admission_keeps_inputs_and_writes_no_completed_artifact() {
    let root = root();
    let common = root.join("source-common");
    let input = b"<U0020> IGNORE;IGNORE;IGNORE;<U0020>\ncollating-element <example> from \"<U0041><U030A>\" % decomposition of LATIN CAPITAL LETTER A WITH RING ABOVE\n";
    fs::write(&common, input).unwrap();
    let captures = root.join("captures");
    let output_json = root.join("data.json");
    let output_rust = root.join("table.rs");
    let output = command()
        .arg("--evidence")
        .arg(&captures)
        .args(["--locale", "en_US", "--", "/usr/bin/sort", "/usr/bin/false"])
        .arg(&root)
        .arg(&common)
        .arg(&output_json)
        .arg(&output_rust)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cannot admit collation locale"));
    assert_eq!(fs::read(captures.join("common")).unwrap(), input);
    let selected: serde_json::Value =
        serde_json::from_slice(&fs::read(captures.join("sorting-locales.json")).unwrap()).unwrap();
    assert_eq!(selected.as_array().unwrap().len(), 1);
    assert_eq!(selected[0][0], "en_US");
    assert!(
        captures
            .join("locale-0000/locale-admission/command.json")
            .exists()
    );
    assert!(!output_json.exists());
    assert!(!output_rust.exists());
    fs::remove_dir_all(root).unwrap();
}
