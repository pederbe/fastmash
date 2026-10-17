#![forbid(unsafe_code)]
#![cfg(unix)]

use fastmash_locale_tools::{comparison::Plan, executor::execute};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "locale-executor-{label}-{}-{stamp}",
        std::process::id()
    ));
    DirBuilder::new().mode(0o700).create(&path).unwrap();
    path
}

fn plan() -> Plan {
    Plan::from_json(
        serde_json::to_vec(&json!({
            "bare_utf8": ["fixture' name\nfr_é"],
            "locales": {"fixture' name\nfr_é": {
                "decimal_point": ".", "thousands_sep": "", "grouping": [-1],
            }},
        }))
        .unwrap()
        .as_slice(),
        false,
    )
    .unwrap()
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn real_fixture_observes_exact_arguments_environment_input_eof_and_raw_tuple() {
    let root = root("ordinary");
    let evidence = root.join("comparison");
    let plan = plan();
    let locpath = OsString::from_vec(b"relative-reference-\xff/path".to_vec());
    let fixture = Path::new(env!("CARGO_BIN_EXE_locale-tools-fixture"));
    let summary = execute(&plan, fixture, fixture, &locpath, &evidence).unwrap();
    assert_eq!(summary.completed_pairs, 21);
    assert_eq!(summary.mismatches, 0);
    assert!(summary.diagnostics.is_empty());
    for (role, argv0) in [("candidate", "fastmash"), ("reference", "datamash")] {
        let directory = evidence.join("pair-000001").join(role);
        assert_eq!(
            fs::read(directory.join("stdout")).unwrap(),
            b"\xffobserved\n"
        );
        let stderr = fs::read(directory.join("stderr")).unwrap();
        assert_eq!(stderr.first(), Some(&0xff));
        assert_eq!(stderr.last(), Some(&0x80));
        let observed: Value = serde_json::from_slice(&stderr[1..stderr.len() - 1]).unwrap();
        assert_eq!(observed["argv0"], argv0);
        assert_eq!(observed["arguments"], json!([b"-g", b"2", b"sum", b"1"]));
        assert_eq!(observed["environment"].as_object().unwrap().len(), 4);
        assert_eq!(
            observed["environment"]["LC_ALL"],
            json!("fixture' name\nfr_é".as_bytes())
        );
        assert_eq!(
            observed["environment"]["LOCPATH"],
            json!(locpath.as_bytes())
        );
        assert_eq!(observed["environment"]["PATH"], json!(b"/usr/bin:/bin"));
        assert_eq!(observed["environment"]["TZ"], json!(b"UTC"));
        assert_eq!(
            observed["cwd"],
            json!(std::env::current_dir().unwrap().as_os_str().as_bytes())
        );
        let expected_input = plan.locales[0].cases().next().unwrap().input;
        assert_eq!(observed["input"], json!(expected_input));
        assert_eq!(fs::read(directory.join("stdin")).unwrap(), expected_input);
        let waited = read_json(&directory.join("wait.json"));
        assert_eq!(waited["exit_code"], 77);
        assert_eq!(waited["reaped"], true);
        assert_eq!(waited["execution_milliseconds"], 30_000);
        assert_eq!(waited["cleanup_milliseconds"], 1_000);
        assert_eq!(waited["timeout_starts_after_spawn"], true);
        let command = read_json(&directory.join("command.json"));
        assert_eq!(command["argv0"], argv0);
        assert_eq!(command["stdin_transport"], "pipe");
        assert_eq!(command["stdout_transport"], "regular-file");
        assert_eq!(command["stderr_transport"], "regular-file");
        let delivery = read_json(&directory.join("stdin-delivery.json"));
        assert_eq!(delivery["delivered"], true);
        assert_eq!(delivery["writer_closed"], true);
    }
    let formatted = fs::read(evidence.join("pair-000007/candidate/stderr")).unwrap();
    let formatted: Value = serde_json::from_slice(&formatted[1..formatted.len() - 1]).unwrap();
    assert_eq!(
        formatted["arguments"],
        json!([b"--format", b"%' 025.3f", b"-g", b"2", b"sum", b"1"])
    );
    let column = fs::read(evidence.join("pair-000018/candidate/stdin")).unwrap();
    assert_eq!(column.len(), 139);
    assert_eq!(column, plan.locales[0].cases().nth(17).unwrap().input);
    assert!(evidence.join("pair-000021/reference/stdout").is_file());
    fs::remove_dir_all(root).unwrap();
}
