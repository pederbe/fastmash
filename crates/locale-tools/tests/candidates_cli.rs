#![forbid(unsafe_code)]
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "fastmash-candidates-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        fs::write(
            root.join("numeric.json"),
            r#"{"glibc":"reference","locales":{"upper":{},"lower":{}}}"#,
        )
        .unwrap();
        fs::write(root.join("exemplars.json"), r#"{"icu":"78","locales":{"upper":{"alphabet":["ch","é"],"auxiliary":[]},"lower":{"alphabet":null,"auxiliary":null}}}"#).unwrap();
        fs::copy(
            env!("CARGO_BIN_EXE_collation-candidate-sort-fixture"),
            root.join("sort"),
        )
        .unwrap();
        fs::write(root.join("mode"), "normal").unwrap();
        Self(root)
    }

    fn run(&self) -> Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-generate-collation-candidates"))
            .current_dir(&self.0)
            .env("FASTMASH_CANDIDATE_POISON", "must be cleared")
            .args([
                "--evidence",
                "captures",
                "numeric.json",
                "exemplars.json",
                "sort",
                ".",
                "output.json",
            ])
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}

#[test]
fn actual_cli_retains_original_output_format_and_native_probe_contract() {
    let fixture = Fixture::new();
    let output = fixture.run();
    assert!(
        output.status.success(),
        "{}: {:?}",
        fixture.0.display(),
        output
    );
    assert_eq!(
        output.stdout,
        b"2 candidates, 1 uppercase first, 1 without ICU data\n"
    );
    assert!(output.stderr.is_empty());
    let expected = "{\n \"icu\": \"78\",\n \"glibc\": \"reference\",\n \"locales\": {\n  \"lower\": {\n   \"tags\": [\n    \"lower\",\n    \"und\"\n   ],\n   \"alphabet\": null,\n   \"auxiliary\": null\n  },\n  \"upper\": {\n   \"tags\": [\n    \"upper-u-kf-upper\",\n    \"und-u-kf-upper\"\n   ],\n   \"alphabet\": [\n    \"ch\",\n    \"é\"\n   ],\n   \"auxiliary\": []\n  }\n }\n}\n";
    assert_eq!(
        fs::read(fixture.0.join("output.json")).unwrap(),
        expected.as_bytes()
    );
    for index in [1, 2] {
        let evidence = fixture.0.join(format!("captures/locale-{index:06}"));
        assert_eq!(fs::read(evidence.join("stdin")).unwrap(), b"a\nA\n");
        assert!(evidence.join("wait.json").is_file());
        let command: serde_json::Value =
            serde_json::from_slice(&fs::read(evidence.join("command.json")).unwrap()).unwrap();
        assert_eq!(command["execution_milliseconds"], 300_000);
    }
}

#[test]
fn failed_reference_with_matching_output_wrong_output_or_diagnostic_creates_no_table() {
    for mode in ["failed", "wrong", "diagnostic"] {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("mode"), mode).unwrap();
        let output = fixture.run();
        assert_eq!(output.status.code(), Some(1), "{mode}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(!fixture.0.join("output.json").exists());
        assert!(fixture.0.join("captures/locale-000001/stdout").is_file());
        assert!(fixture.0.join("captures/locale-000001/wait.json").is_file());
    }
}

#[test]
fn invalid_reference_and_existing_output_are_preserved_before_probing() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("output.json"), "preserve").unwrap();
    assert_eq!(fixture.run().status.code(), Some(1));
    assert_eq!(
        fs::read(fixture.0.join("output.json")).unwrap(),
        b"preserve"
    );
    assert!(!fixture.0.join("captures").exists());
    fs::remove_file(fixture.0.join("output.json")).unwrap();
    fs::write(
        fixture.0.join("exemplars.json"),
        r#"{"icu":"77","locales":{}}"#,
    )
    .unwrap();
    assert_eq!(fixture.run().status.code(), Some(1));
    assert!(!fixture.0.join("captures").exists());
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-generate-collation-candidates"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
}
