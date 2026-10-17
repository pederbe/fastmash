#![forbid(unsafe_code)]
#![cfg(all(target_os = "linux", target_env = "gnu"))]

use fastmash_locale_tools::{Numeric, Reference, collect};
use serde_json::json;
use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn actual_cli_preserves_usage_error_status_and_successful_help() {
    let binary = env!("CARGO_BIN_EXE_fastmash-generate-numeric-locales");
    let invalid = Command::new(binary).output().unwrap();
    assert_eq!(invalid.status.code(), Some(1));
    assert!(invalid.stdout.is_empty());
    assert!(
        String::from_utf8(invalid.stderr)
            .unwrap()
            .contains("expected LOCPATH OUT_JSON OUT_RS")
    );
    let help = Command::new(binary).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert_eq!(
        help.stdout,
        format!("{}\n", fastmash_locale_tools::native::USAGE).as_bytes()
    );
}

fn private_root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "fastmash-locale-cli-{label}-{}-{stamp}",
        std::process::id()
    ));
    DirBuilder::new().mode(0o700).create(&root).unwrap();
    root
}

fn invoke(root: &Path, name: &str) -> (Output, PathBuf, PathBuf, PathBuf) {
    let locpath = root.join("selected-locales");
    fs::create_dir(&locpath).unwrap();
    fs::create_dir(locpath.join(name)).unwrap();
    let raw_path = root.join("chosen-numeric.json");
    let rust_path = root.join("chosen-numeric.rs");
    let query_temp = root.join("query-evidence");
    DirBuilder::new().mode(0o700).create(&query_temp).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-generate-numeric-locales"))
        .arg(&locpath)
        .arg(&raw_path)
        .arg(&rust_path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TMPDIR", &query_temp)
        .output()
        .unwrap();
    fs::write(root.join("stdout"), &output.stdout).unwrap();
    fs::write(root.join("stderr"), &output.stderr).unwrap();
    fs::write(root.join("status"), format!("{}\n", output.status)).unwrap();
    (output, raw_path, rust_path, query_temp)
}

#[test]
fn actual_cli_queries_builtin_posix_into_chosen_files() {
    // An ordinary installed-glibc control, not the frozen Ubuntu reference.
    let root = private_root("posix-test");
    let (output, raw_path, rust_path, query_temp) = invoke(&root, "POSIX");
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, b"1 locales, 0 refused\n");
    assert!(output.stderr.is_empty());
    let raw: serde_json::Value = serde_json::from_slice(&fs::read(raw_path).unwrap()).unwrap();
    let glibc = raw["glibc"].as_str().unwrap();
    let machine = raw["machine"].as_str().unwrap();
    assert!(!glibc.is_empty());
    assert!(!machine.is_empty());
    assert_eq!(
        raw,
        json!({
            "glibc": glibc,
            "machine": machine,
            "bare_utf8": ["POSIX"],
            "locales": {
                "POSIX": {"decimal_point": ".", "thousands_sep": "", "grouping": [-1]},
            },
        })
    );
    let expected = collect(
        Reference {
            glibc: glibc.into(),
            machine: machine.into(),
        },
        vec!["POSIX".into()],
        |_| {
            Ok(Numeric {
                decimal_point: ".".into(),
                thousands_sep: String::new(),
                grouping: vec![-1],
            })
        },
    )
    .unwrap();
    // Membership comes from the typed expectation, never parsed Rust source.
    assert_eq!(
        fs::read_to_string(rust_path).unwrap(),
        expected.rust_source().unwrap()
    );
    assert_eq!(fs::read_dir(query_temp).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_refuses_invalid_locale_and_retains_raw_query_evidence() {
    let root = private_root("invalid-test");
    let (output, raw_path, rust_path, query_temp) =
        invoke(&root, "fastmash_missing_numeric_locale");
    assert_eq!(
        output.status.code(),
        Some(1),
        "CLI evidence: {}",
        root.display()
    );
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
    assert!(!raw_path.exists());
    assert!(!rust_path.exists());
    let retained: Vec<_> = fs::read_dir(query_temp)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(retained.len(), 1);
    let query = retained[0].join("query-0003");
    let declaration: serde_json::Value =
        serde_json::from_slice(&fs::read(query.join("command.json")).unwrap()).unwrap();
    assert_eq!(
        declaration["environment"]["LC_ALL"],
        "fastmash_missing_numeric_locale"
    );
    let waited: serde_json::Value =
        serde_json::from_slice(&fs::read(query.join("wait.json")).unwrap()).unwrap();
    assert_eq!(waited["reaped"], true);
    assert_eq!(waited["timed_out"], false);
    assert!(waited["raw_status"].is_number());
    assert!(!fs::read(query.join("stderr")).unwrap().is_empty());
    assert!(query.join("stdout").is_file());
    assert!(retained[0].join("failure.txt").is_file());
    eprintln!("Retained invalid-locale CLI evidence: {}", root.display());
}
