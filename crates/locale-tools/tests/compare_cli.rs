#![forbid(unsafe_code)]
#![cfg(unix)]

use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USAGE: &str = "Usage: fastmash-compare-numeric-locales [--all] [--evidence PATH] [--] CANDIDATE REFERENCE LOCPATH TABLE_JSON";
const TABLE: &[u8] = br#"{
    "bare_utf8": ["a"],
    "locales": {
        "a": {"decimal_point": ".", "thousands_sep": "", "grouping": [-1]},
        "b": {"decimal_point": ".", "thousands_sep": "", "grouping": [-1]}
    }
}"#;
const FIRST_INPUT: &[u8] = b"0\t0\n1\t1\n-1\t2\n12\t3\n123\t4\n1234\t5\n12345\t6\n123456\t7\n1234567\t8\n12345678\t9\n123456789\t10\n1234567890\t11\n98765432109876\t12\n-4321987.5\t13\n0.125\t14\n1e20\t15\n-2.5e-7\t16\n999999.995\t17\n1000000\t18\n-0.5\t19\n123456789012345678901234567890\t20\n";

fn root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "locale-compare-cli-{label}-{}-{stamp}",
        std::process::id()
    ));
    DirBuilder::new().mode(0o700).create(&root).unwrap();
    DirBuilder::new()
        .mode(0o700)
        .create(root.join("tmp"))
        .unwrap();
    // macOS may spell its temporary directory through a symlink.
    fs::canonicalize(root).unwrap()
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_locale-tools-fixture"))
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn observation(evidence: &Path, pair: &str, role: &str) -> Value {
    let raw = fs::read(evidence.join(pair).join(role).join("stderr")).unwrap();
    assert_eq!(raw.first(), Some(&0xff));
    assert_eq!(raw.last(), Some(&0x80));
    serde_json::from_slice(&raw[1..raw.len() - 1]).unwrap()
}

fn invoke(root: &Path, label: &str, args: &[OsString]) -> Output {
    let stdout = root.join(format!("{label}.stdout"));
    let stderr = root.join(format!("{label}.stderr"));
    fs::write(
        root.join(format!("{label}.invocation.json")),
        serde_json::to_vec_pretty(&json!({
            "arguments": args.iter().map(|arg| arg.as_bytes()).collect::<Vec<_>>(),
            "working_directory_bytes": root.as_os_str().as_bytes(),
            "execution_milliseconds": 45_000, "cleanup_milliseconds": 2_000,
        }))
        .unwrap(),
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_fastmash-compare-numeric-locales"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TMPDIR", root.join("tmp"))
        .stdin(Stdio::null())
        .stdout(Stdio::from(File::create(&stdout).unwrap()))
        .stderr(Stdio::from(File::create(&stderr).unwrap()))
        .spawn()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() >= Duration::from_secs(45) {
            let killed = child.kill();
            let cleanup = Instant::now();
            let mut reaped = false;
            while cleanup.elapsed() < Duration::from_secs(2) {
                if child.try_wait().unwrap().is_some() {
                    reaped = true;
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            fs::write(
                root.join(format!("{label}.timeout")),
                format!("kill: {killed:?}\nreaped: {reaped}\n"),
            )
            .unwrap();
            panic!(
                "CLI exceeded its test deadline. Evidence: {}",
                root.display()
            );
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    fs::write(root.join(format!("{label}.status")), format!("{status}\n")).unwrap();
    Output {
        status,
        stdout: fs::read(stdout).unwrap(),
        stderr: fs::read(stderr).unwrap(),
    }
}

fn arguments(candidate: &Path, reference: &Path, table: &Path) -> Vec<OsString> {
    vec![
        candidate.as_os_str().into(),
        reference.as_os_str().into(),
        "relative-locale-root".into(),
        table.as_os_str().into(),
    ]
}

#[test]
fn actual_cli_help_usage_and_unknown_flags_create_no_evidence() {
    let root = root("usage");
    for (index, args) in [
        vec![OsString::from("--help")],
        vec!["--evidence".into(), "never-created".into(), "-h".into()],
    ]
    .into_iter()
    .enumerate()
    {
        let output = invoke(&root, &format!("help-{index}"), &args);
        assert!(output.status.success(), "CLI evidence: {}", root.display());
        assert_eq!(output.stdout, format!("{USAGE}\n").as_bytes());
        assert!(output.stderr.is_empty());
    }
    let invalid: Vec<Vec<OsString>> = vec![
        vec![],
        vec!["--unknown".into()],
        vec!["--evidence".into()],
        vec!["candidate".into(), "reference".into(), "locpath".into()],
        vec![
            "one".into(),
            "two".into(),
            "three".into(),
            "four".into(),
            "five".into(),
        ],
        vec![
            "--evidence".into(),
            "never-created".into(),
            "--unknown".into(),
        ],
        vec![
            "--evidence".into(),
            "never-created".into(),
            "--evidence".into(),
            "other".into(),
        ],
    ];
    for (index, args) in invalid.into_iter().enumerate() {
        let output = invoke(&root, &format!("invalid-{index}"), &args);
        assert_eq!(
            output.status.code(),
            Some(1),
            "CLI evidence: {}",
            root.display()
        );
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(USAGE));
    }
    assert!(!root.join("never-created").exists());
    assert!(!root.join("other").exists());
    assert_eq!(fs::read_dir(root.join("tmp")).unwrap().count(), 0);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_selects_representatives_or_all_and_retains_observer_receipts() {
    let root = root("selection");
    let table = root.join("table.json");
    fs::write(&table, TABLE).unwrap();
    let args = arguments(&fixture(), &fixture(), &table);
    let output = invoke(&root, "representatives", &args);
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, b"1 locales, 21 cases, 0 mismatches\n");
    let retained: Vec<_> = fs::read_dir(root.join("tmp"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(retained.len(), 1);
    let evidence = &retained[0];
    assert_eq!(
        fs::metadata(evidence).unwrap().permissions().mode() & 0o077,
        0
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains(&evidence.display().to_string())
    );
    assert_eq!(
        read_json(&evidence.join("summary.json")),
        json!({
            "completed_pairs": 21, "mismatches": 0, "diagnostics": [],
        })
    );
    for (role, argv0) in [("candidate", "fastmash"), ("reference", "datamash")] {
        let directory = evidence.join("pair-000001").join(role);
        assert_eq!(
            fs::read(directory.join("stdout")).unwrap(),
            b"\xffobserved\n"
        );
        let observed = observation(evidence, "pair-000001", role);
        assert_eq!(observed["argv0"], argv0);
        assert_eq!(observed["arguments"], json!([b"-g", b"2", b"sum", b"1"]));
        assert_eq!(
            observed["environment"],
            json!({
                "PATH": b"/usr/bin:/bin", "LC_ALL": b"a",
                "LOCPATH": b"relative-locale-root", "TZ": b"UTC",
            })
        );
        assert_eq!(observed["input"], json!(FIRST_INPUT));
        assert_eq!(fs::read(directory.join("stdin")).unwrap(), FIRST_INPUT);
        let waited = read_json(&directory.join("wait.json"));
        assert_eq!(waited["exit_code"], 77);
        assert_eq!(waited["reaped"], true);
        assert_eq!(waited["timed_out"], false);
        let command = read_json(&directory.join("command.json"));
        assert_eq!(command["argv0"], argv0);
        assert_eq!(command["stdin_transport"], "pipe");
    }
    assert!(evidence.join("pair-000021/reference/stdout").is_file());
    let all = root.join("all-evidence");
    let mut args = arguments(&fixture(), &fixture(), &table);
    args.insert(1, "--all".into());
    args.extend(["--evidence".into(), all.as_os_str().into()]);
    let output = invoke(&root, "all", &args);
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, b"2 locales, 42 cases, 0 mismatches\n");
    assert_eq!(
        read_json(&all.join("summary.json")),
        json!({
            "completed_pairs": 42, "mismatches": 0, "diagnostics": [],
        })
    );
    assert_eq!(
        observation(&all, "pair-000022", "candidate")["environment"]["LC_ALL"],
        json!(b"b.UTF-8")
    );
    assert!(all.join("pair-000042/reference/stdout").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_counts_all_mismatches_and_prints_only_first_twenty_five() {
    let root = root("mismatches");
    let echo = Path::new("/bin/echo");
    assert!(
        echo.is_file(),
        "the independent /bin/echo control must be installed"
    );
    assert_ne!(fs::metadata(echo).unwrap().permissions().mode() & 0o111, 0);
    let table = root.join("table.json");
    fs::write(&table, TABLE).unwrap();
    let evidence = root.join("comparison");
    let mut args = arguments(&fixture(), echo, &table);
    args.extend([
        "--all".into(),
        "--evidence".into(),
        evidence.as_os_str().into(),
    ]);
    let output = invoke(&root, "mismatches", &args);
    assert_eq!(
        output.status.code(),
        Some(1),
        "CLI evidence: {}",
        root.display()
    );
    let summary = read_json(&evidence.join("summary.json"));
    assert_eq!(summary["completed_pairs"], 42);
    assert_eq!(summary["mismatches"], 42);
    let diagnostics = summary["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 25);
    let text = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = text.lines().collect();
    assert_eq!(lines.len(), 26);
    for (line, diagnostic) in lines.iter().zip(diagnostics) {
        assert_eq!(*line, diagnostic.as_str().unwrap());
    }
    assert!(lines[0].starts_with("Mismatch a case 0:"));
    assert!(lines[24].starts_with("Mismatch b.UTF-8 case 3:"));
    assert_eq!(lines[25], "2 locales, 42 cases, 42 mismatches");
    assert!(text.ends_with("2 locales, 42 cases, 42 mismatches\n"));
    assert_eq!(
        read_json(&evidence.join("pair-000001/candidate/wait.json"))["exit_code"],
        77
    );
    assert_eq!(
        read_json(&evidence.join("pair-000001/reference/wait.json"))["exit_code"],
        0
    );
    assert!(evidence.join("pair-000042/reference/stdout").is_file());
    assert!(!evidence.join("failure.txt").exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_abnormal_candidate_stops_before_reference_and_keeps_failure() {
    let root = root("missing-candidate");
    let table = root.join("table.json");
    fs::write(&table, TABLE).unwrap();
    let evidence = root.join("comparison");
    let mut args = arguments(&root.join("missing-program"), &fixture(), &table);
    args.extend(["--evidence".into(), evidence.as_os_str().into()]);
    let output = invoke(&root, "missing", &args);
    assert_eq!(
        output.status.code(),
        Some(1),
        "CLI evidence: {}",
        root.display()
    );
    assert!(output.stdout.is_empty());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("stopped after 0 completed pairs"));
    assert!(error.contains(&evidence.display().to_string()));
    assert!(!error.contains("0 mismatches"));
    assert_eq!(
        read_json(&evidence.join("summary.json")),
        json!({
            "completed_pairs": 0, "mismatches": 0, "diagnostics": [],
        })
    );
    assert!(evidence.join("failure.txt").is_file());
    let directory = evidence.join("pair-000001/candidate");
    assert!(directory.join("command.json").is_file());
    assert!(directory.join("stdout").is_file());
    assert!(directory.join("stderr").is_file());
    let waited = read_json(&directory.join("wait.json"));
    assert!(waited["spawn_error"].is_string());
    assert!(waited["raw_status"].is_null());
    assert_eq!(waited["reaped"], false);
    assert!(!evidence.join("pair-000001/reference").exists());

    // Reusing an evidence path must retain the first failure unchanged.
    let original = fs::read(evidence.join("failure.txt")).unwrap();
    let output = invoke(&root, "existing-evidence", &args);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(evidence.join("failure.txt")).unwrap(), original);
    eprintln!(
        "Retained abnormal-comparison CLI evidence: {}",
        root.display()
    );
}

#[test]
fn actual_cli_preserves_terminated_unusual_paths_and_non_utf8_locpath() {
    let root = root("unusual-paths");
    let programs = Path::new("-programs ' é");
    fs::create_dir(root.join(programs)).unwrap();
    let candidate = programs.join("candidate\none");
    let reference = programs.join("reference");
    symlink(fixture(), root.join(&candidate)).unwrap();
    symlink(fixture(), root.join(&reference)).unwrap();
    let table = Path::new("--all");
    fs::write(root.join(table), TABLE).unwrap();
    let evidence = Path::new("-evidence ' é\nchosen");
    // LOCPATH is an environment value. It need not name a filesystem entry.
    let locpath = OsString::from_vec(b"relative-\xff/path".to_vec());
    let args = vec![
        "--evidence".into(),
        evidence.as_os_str().into(),
        "--".into(),
        candidate.as_os_str().into(),
        reference.as_os_str().into(),
        locpath.clone(),
        table.as_os_str().into(),
    ];
    let output = invoke(&root, "unusual", &args);
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, b"1 locales, 21 cases, 0 mismatches\n");
    let evidence = root.join(evidence);
    let observed = observation(&evidence, "pair-000001", "candidate");
    assert_eq!(
        observed["environment"]["LOCPATH"],
        json!(locpath.as_bytes())
    );
    assert_eq!(observed["cwd"], json!(root.as_os_str().as_bytes()));
    assert_eq!(observed["argv0"], "fastmash");
    let command = read_json(&evidence.join("pair-000001/candidate/command.json"));
    assert_eq!(
        command["program_bytes"],
        json!(candidate.as_os_str().as_bytes())
    );
    assert_eq!(
        command["environment"]["LOCPATH_bytes"],
        json!(locpath.as_bytes())
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn actual_cli_preserves_non_utf8_filesystem_paths() {
    let root = root("linux-path-bytes");
    let programs = PathBuf::from(OsString::from_vec(b"program-\xff".to_vec()));
    fs::create_dir(root.join(&programs)).unwrap();
    let candidate = programs.join("observer");
    symlink(fixture(), root.join(&candidate)).unwrap();
    let table = PathBuf::from(OsString::from_vec(b"numeric-\xfe.json".to_vec()));
    fs::write(root.join(&table), TABLE).unwrap();
    let evidence = PathBuf::from(OsString::from_vec(b"evidence-\xfd".to_vec()));
    let args = vec![
        candidate.as_os_str().into(),
        fixture().into_os_string(),
        "relative-locale-root".into(),
        table.as_os_str().into(),
        "--evidence".into(),
        evidence.as_os_str().into(),
    ];
    let output = invoke(&root, "raw-paths", &args);
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, b"1 locales, 21 cases, 0 mismatches\n");
    let evidence = root.join(evidence);
    let command = read_json(&evidence.join("pair-000001/candidate/command.json"));
    assert_eq!(
        command["program_bytes"],
        json!(candidate.as_os_str().as_bytes())
    );
    assert_eq!(
        observation(&evidence, "pair-000001", "candidate")["argv0"],
        "fastmash"
    );
    assert!(evidence.join("pair-000021/reference/stdout").is_file());
    fs::remove_dir_all(root).unwrap();
}
