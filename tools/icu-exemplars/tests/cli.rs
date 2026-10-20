#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-icu-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("numeric.json"),
            r#"{"locales":{"en_AA":{},"es_MX":{},"zz_ZZ":{}}}"#,
        )
        .unwrap();
        Self(path)
    }

    fn invoke(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_fastmash-icu-exemplars"))
            .current_dir(&self.0)
            .args(["numeric.json", "out.json"])
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
fn native_cli_identifies_or_refuses_icu_and_never_overwrites_output() {
    let fixture = Fixture::new();
    let output = fixture.invoke();
    if fastmash_icu_exemplars::supported() {
        assert!(output.status.success(), "{output:?}");
        let path = fixture.0.join("out.json");
        let bytes = fs::read(&path).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["icu"], "78");
        assert_eq!(json["locales"]["en_AA"]["alphabet"][0], "a");
        assert!(
            json["locales"]["es_MX"]["alphabet"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("ñ"))
        );
        assert!(json["locales"]["zz_ZZ"]["alphabet"].is_null());
        assert_eq!(fixture.invoke().status.code(), Some(1));
        assert_eq!(fs::read(path).unwrap(), bytes);
    } else {
        assert_eq!(output.status.code(), Some(77));
        assert!(output.stdout.is_empty());
        assert!(!fixture.0.join("out.json").exists());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains(fastmash_icu_exemplars::ICU_VERSION)
        );
    }
}

#[test]
fn help_and_usage_do_not_query_resources_or_create_output() {
    let help = Command::new(env!("CARGO_BIN_EXE_fastmash-icu-exemplars"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(help.stderr.is_empty());
    assert!(help.stdout.starts_with(b"Usage: fastmash-icu-exemplars"));
    let usage = Command::new(env!("CARGO_BIN_EXE_fastmash-icu-exemplars"))
        .output()
        .unwrap();
    assert_eq!(usage.status.code(), Some(1));
    let fixture = Fixture::new();
    if fastmash_icu_exemplars::supported() {
        for invalid in [
            "{}",
            r#"{"locales":{}}"#,
            r#"{"locales":{"bad\u0000name":{}}}"#,
        ] {
            fs::write(fixture.0.join("numeric.json"), invalid).unwrap();
            assert_eq!(fixture.invoke().status.code(), Some(1));
            assert!(!fixture.0.join("out.json").exists());
        }
    }
}
