#![forbid(unsafe_code)]
#![cfg(unix)]

use fastmash_locale_tools::collation_plan::punctuation_words;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const USAGE: &str = "Usage: fastmash-compare-collation-punctuation --evidence PATH [--locale NAME]... [--] VERIFICATION_JSON CANDIDATE REFERENCE SORT LOCPATH OUTPUT_JSON";

struct Run {
    owned_directory: PathBuf,
    root: PathBuf,
    verification: PathBuf,
    original: Vec<u8>,
    candidate: PathBuf,
    reference: PathBuf,
    sort: PathBuf,
    locpath: PathBuf,
    report: PathBuf,
    evidence: PathBuf,
}

impl Run {
    fn new(label: &str, verification: &[u8]) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "locale-punctuation-{label}-{}-{stamp}",
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let owned_directory = root;
        let root = owned_directory.join("via-link");
        // Exercise physical cwd observations through a supplied symlink path.
        symlink(".", &root).unwrap();
        let verification_path = root.join("verification.json");
        fs::write(&verification_path, verification).unwrap();
        Self {
            owned_directory,
            verification: verification_path,
            original: verification.to_vec(),
            candidate: env!("CARGO_BIN_EXE_collation-punctuation-fixture").into(),
            reference: env!("CARGO_BIN_EXE_collation-punctuation-fixture").into(),
            sort: root.join("selected sort ' é"),
            locpath: root.join("compiled locales ' é"),
            report: root.join("report.json"),
            evidence: root.join("evidence"),
            root,
        }
    }

    fn configure(&self, replies: &[(&str, Value, Value)]) {
        let locales: BTreeMap<_, _> = replies
            .iter()
            .map(|(name, candidate, reference)| {
                (
                    format!("{name}.UTF-8"),
                    json!({"candidate": candidate, "reference": reference}),
                )
            })
            .collect();
        fs::write(
            self.root.join("punctuation-fixture.json"),
            serde_json::to_vec(&json!({
                "observations": self.root.join("observations.jsonl").as_os_str().as_bytes(),
                "locales": locales,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn arguments(&self, locales: &[&str]) -> Vec<OsString> {
        let mut args = vec!["--evidence".into(), self.evidence.as_os_str().into()];
        for locale in locales {
            args.extend(["--locale".into(), (*locale).into()]);
        }
        args.push("--".into());
        args.extend(
            [
                &self.verification,
                &self.candidate,
                &self.reference,
                &self.sort,
                &self.locpath,
                &self.report,
            ]
            .map(|path| path.as_os_str().to_owned()),
        );
        args
    }

    fn invoke(&self, label: &str, args: &[OsString]) -> Output {
        // Fixed local fixtures use the caller's process-tree bound. They do not
        // run a native collator, GNU reference or timing comparison.
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-compare-collation-punctuation"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("PUNCTUATION_PARENT_ONLY", "must not reach either role")
            .output()
            .unwrap();
        fs::write(self.root.join(format!("{label}.stdout")), &output.stdout).unwrap();
        fs::write(self.root.join(format!("{label}.stderr")), &output.stderr).unwrap();
        fs::write(
            self.root.join(format!("{label}.status")),
            format!("{}\n", output.status),
        )
        .unwrap();
        output
    }

    fn observations(&self) -> Vec<Value> {
        let path = self.root.join("observations.jsonl");
        match fs::read_to_string(path) {
            Ok(raw) => raw
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot read fixture observations: {error}"),
        }
    }

    fn finish(self) {
        assert_eq!(
            fs::read(self.root.join(&self.verification)).unwrap(),
            self.original
        );
        fs::remove_dir_all(self.owned_directory).unwrap();
    }
}

fn reply(stdout: &[u8], stderr: &[u8], status: u8) -> Value {
    json!({"stdout": stdout, "stderr": stderr, "status": status})
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn filesystem_name(prefix: &str) -> OsString {
    let mut name = prefix.as_bytes().to_vec();
    // Use Unicode filenames on macOS, retaining non-UTF-8 names on Linux.
    // Raw SORT and LOCPATH arguments below exercise arbitrary bytes on both.
    #[cfg(target_os = "linux")]
    name.push(0xff);
    #[cfg(not(target_os = "linux"))]
    name.extend_from_slice("é".as_bytes());
    name.push(b'\n');
    OsString::from_vec(name)
}

fn expected_input() -> Vec<u8> {
    // Word-list parity has separate retained legacy coverage. This test checks
    // that the executor sends the existing planner's entire payload through EOF.
    let words = punctuation_words();
    assert_eq!(words.len(), 1_983);
    let input = format!("{}\n", words.join("\n")).into_bytes();
    assert_eq!(input.len(), 7_786);
    input
}

fn assert_success(output: &Output, summary: &str, root: &Path) {
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, format!("{summary}\n").as_bytes());
    assert!(output.stderr.is_empty(), "CLI evidence: {}", root.display());
}

fn assert_observed_pair(run: &Run, observations: &[Value], index: usize, name: &str) {
    let expected_input = expected_input();
    let working_directory = fs::canonicalize(&run.root).unwrap();
    assert_ne!(working_directory, run.root);
    for (offset, role, argv0, program) in [
        (0, "candidate", "fastmash", &run.candidate),
        (1, "reference", "datamash", &run.reference),
    ] {
        let observed = &observations[index * 2 + offset];
        assert_eq!(observed["role"], role);
        let mut arguments = vec![argv0.as_bytes().to_vec()];
        if role == "reference" {
            arguments.extend([
                b"--sort-cmd".to_vec(),
                run.sort.as_os_str().as_bytes().to_vec(),
            ]);
        }
        arguments.extend([b"-s".as_slice(), b"-g", b"1", b"count", b"1"].map(<[u8]>::to_vec));
        assert_eq!(observed["arguments"], json!(arguments));
        assert_eq!(observed["input_kind"], "pipe");
        assert_eq!(observed["eof"], true);
        assert_eq!(observed["input"], json!(expected_input));
        assert_eq!(
            observed["working_directory"],
            json!(working_directory.as_os_str().as_bytes())
        );
        assert_eq!(observed["environment"].as_object().unwrap().len(), 4);
        assert_eq!(observed["environment"]["PATH"], json!(b"/usr/bin:/bin"));
        assert_eq!(
            observed["environment"]["LC_ALL"],
            json!(format!("{name}.UTF-8").as_bytes())
        );
        assert_eq!(
            observed["environment"]["LOCPATH"],
            json!(run.locpath.as_os_str().as_bytes())
        );
        assert_eq!(observed["environment"]["TZ"], json!(b"UTC"));
        let directory = run.evidence.join(format!("locale-{:06}/{role}", index + 1));
        let command = read_json(&directory.join("command.json"));
        assert_eq!(
            command["program_bytes"],
            json!(program.as_os_str().as_bytes())
        );
        assert_eq!(command["argv0"], argv0);
        assert_eq!(command["arguments_bytes"], json!(&arguments[1..]));
        assert_eq!(command["locale_key"], name);
        assert_eq!(command["environment_cleared"], true);
        assert_eq!(
            command["environment"]["LOCPATH_bytes"],
            json!(run.locpath.as_os_str().as_bytes())
        );
        assert_eq!(
            command["working_directory_bytes"],
            observed["working_directory"]
        );
        assert_eq!(command["stdin_transport"], "pipe");
        assert_eq!(command["stdout_transport"], "regular-file");
        assert_eq!(command["stderr_transport"], "regular-file");
        assert_eq!(command["legacy_output_transport"], "pipe");
        assert_eq!(fs::read(directory.join("stdin")).unwrap(), expected_input);
        let delivery = read_json(&directory.join("stdin-delivery.json"));
        assert_eq!(delivery["delivered"], true);
        assert_eq!(delivery["bytes_written"], 7_786);
        assert_eq!(delivery["writer_closed"], true);
        assert_eq!(delivery["broken_pipe"], false);
        assert!(delivery["error"].is_null());
        let waited = read_json(&directory.join("wait.json"));
        assert_eq!(waited["reaped"], true);
        assert_eq!(waited["timed_out"], false);
        assert_eq!(waited["execution_milliseconds"], 300_000);
        assert_eq!(waited["cleanup_milliseconds"], 1_000);
        assert_eq!(waited["timeout_starts_after_spawn"], true);
        assert!(waited["raw_status"].is_number());
        assert!(waited["kill_error"].is_null());
        assert!(waited["wait_error"].is_null());
        assert!(waited["input_error"].is_null());
        assert!(matches!(
            waited["group_signal"].as_str(),
            Some("sent" | "already_absent")
        ));
    }
}

#[test]
fn default_selection_keeps_final_tags_raw_names_and_exact_native_contracts() {
    let run = Run::new(
        "selection",
        br#"{"locales":{
        "z.UTF-8":{"tag":"z"}, "empty":{"tag":"","verified":true},
        "a@modifier":{"tag":"a"}, "null":{"tag":null},
        "A":{"tag":"A","verified":false}
    }}"#,
    );
    let native = b"a\t\xff\nb\t\x80\r\n";
    run.configure(&["A", "a@modifier", "z.UTF-8"].map(|name| {
        (
            name,
            reply(native, b"\xffcandidate warning\n", 0),
            reply(native, b"reference warning\n", 0),
        )
    }));
    let output = run.invoke("run", &run.arguments(&[]));
    assert_success(
        &output,
        "3 locales, 1983 words; locales by displaced words: 0: 3",
        &run.root,
    );
    let expected = b"{\n \"words\": 1983,\n \"locales\": {\n  \"A\": {\n   \"displaced\": 0,\n   \"words\": []\n  },\n  \"a@modifier\": {\n   \"displaced\": 0,\n   \"words\": []\n  },\n  \"z.UTF-8\": {\n   \"displaced\": 0,\n   \"words\": []\n  }\n }\n}\n";
    assert_eq!(fs::read(&run.report).unwrap(), expected);
    assert_eq!(
        fs::metadata(&run.evidence).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let observations = run.observations();
    assert_eq!(observations.len(), 6);
    for (index, name) in ["A", "a@modifier", "z.UTF-8"].into_iter().enumerate() {
        assert_observed_pair(&run, &observations, index, name);
        for role in ["candidate", "reference"] {
            let directory = run.evidence.join(format!("locale-{:06}/{role}", index + 1));
            assert_eq!(fs::read(directory.join("stdout")).unwrap(), native);
            let waited = read_json(&directory.join("wait.json"));
            assert_eq!(waited["exit_code"], 0);
            assert!(waited["signal"].is_null());
        }
    }
    assert_eq!(
        fs::read(run.evidence.join("locale-000001/candidate/stderr")).unwrap(),
        b"\xffcandidate warning\n"
    );
    assert_eq!(
        fs::read(run.evidence.join("locale-000001/reference/stderr")).unwrap(),
        b"reference warning\n"
    );
    run.finish();
}

#[test]
fn representative_subset_is_sorted_deduplicated_and_preserves_os_byte_paths() {
    let raw = serde_json::to_vec(&json!({"locales": {
        "z": {"tag": "z"}, "quote'\nfr_é": {"tag": "q"}, "a": {"tag": "a"}
    }}))
    .unwrap();
    let mut run = Run::new("raw-paths", &raw);
    let verification = PathBuf::from(filesystem_name("-verification-'"));
    fs::rename(&run.verification, run.root.join(&verification)).unwrap();
    run.verification = verification;
    run.report = filesystem_name("-report-'").into();
    run.evidence = run.root.join(filesystem_name("evidence-'"));
    run.sort = OsString::from_vec(b"-sort-'\xff\n".to_vec()).into();
    run.locpath = OsString::from_vec(b"relative-locales-'\x80\n".to_vec()).into();
    for role in ["candidate", "reference"] {
        let path = run.root.join(filesystem_name(&format!("{role}-'")));
        symlink(env!("CARGO_BIN_EXE_collation-punctuation-fixture"), &path).unwrap();
        if role == "candidate" {
            run.candidate = path;
        } else {
            run.reference = path;
        }
    }
    run.configure(
        &["quote'\nfr_é", "z"]
            .map(|name| (name, reply(b"a\t1\n", b"", 0), reply(b"a\t1\n", b"", 0))),
    );
    let output = run.invoke("run", &run.arguments(&["z", "quote'\nfr_é", "z"]));
    assert_success(
        &output,
        "2 locales, 1983 words; locales by displaced words: 0: 2",
        &run.root,
    );
    let observations = run.observations();
    assert_eq!(observations.len(), 4);
    assert_observed_pair(&run, &observations, 0, "quote'\nfr_é");
    assert_observed_pair(&run, &observations, 1, "z");
    let report = read_json(&run.root.join(&run.report));
    assert_eq!(
        report["locales"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["quote'\nfr_é", "z"]
    );
    run.finish();
}

#[test]
fn retained_negative_words_remain_negative_and_the_report_bytes_are_unchanged() {
    let run = Run::new(
        "retained-negative",
        br#"{"locales":{"tk_TM":{"tag":"tk"},"en_US":{"tag":"en"}}}"#,
    );
    let retained = read_json(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/locales/collation-punctuation.json"),
    );
    let anchors: Vec<String> = (0..20).map(|index| format!("anchor-{index:02}")).collect();
    let tail = String::from("tail");
    let mut replies = Vec::new();
    for name in ["en_US", "tk_TM"] {
        let moved: Vec<String> = retained["locales"][name]["words"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| word.as_str().unwrap().to_owned())
            .collect();
        let candidate = anchors
            .iter()
            .chain(&moved)
            .chain(std::iter::once(&tail))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let reference = moved
            .iter()
            .chain(&anchors)
            .chain(std::iter::once(&tail))
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        replies.push((
            name,
            reply(candidate.as_bytes(), b"", 0),
            reply(reference.as_bytes(), b"", 0),
        ));
    }
    run.configure(&replies);
    let output = run.invoke("run", &run.arguments(&[]));
    assert_success(
        &output,
        "2 locales, 1983 words; locales by displaced words: 11: 1, 13: 1",
        &run.root,
    );
    assert_eq!(
        fs::read(&run.report).unwrap(),
        include_bytes!("fixtures/punctuation_report.json")
    );
    run.finish();
}

#[test]
fn either_native_failure_keeps_both_raw_tuples_and_replacement_decoded_diagnostics() {
    for (label, candidate_status, reference_status, signal) in [
        ("candidate", 17, 0, false),
        ("reference", 0, 23, false),
        ("both", 17, 23, false),
        ("signal", 0, 0, true),
    ] {
        let run = Run::new(label, br#"{"locales":{"b":{"tag":"b"},"a":{"tag":"a"}}}"#);
        let mut candidate = reply(
            b"\xffcandidate output\n",
            b"\xffcandidate diagnostic\n",
            candidate_status,
        );
        candidate["signal"] = json!(signal);
        run.configure(&[
            (
                "a",
                candidate,
                reply(
                    b"\x80reference output\n",
                    b"\x80reference diagnostic\n",
                    reference_status,
                ),
            ),
            ("b", reply(b"a\n", b"", 0), reply(b"a\n", b"", 0)),
        ]);
        let output = run.invoke("run", &run.arguments(&[]));
        assert_success(
            &output,
            "2 locales, 1983 words; locales by displaced words: error: 1, 0: 1",
            &run.root,
        );
        let expected = "{\n \"words\": 1983,\n \"locales\": {\n  \"a\": {\n   \"error\": [\n    \"�candidate diagnostic\\n\",\n    \"�reference diagnostic\\n\"\n   ]\n  },\n  \"b\": {\n   \"displaced\": 0,\n   \"words\": []\n  }\n }\n}\n";
        assert_eq!(fs::read(&run.report).unwrap(), expected.as_bytes());
        let observations = run.observations();
        assert_eq!(observations.len(), 4);
        assert_observed_pair(&run, &observations, 0, "a");
        assert_observed_pair(&run, &observations, 1, "b");
        for (role, status, stdout, stderr) in [
            (
                "candidate",
                candidate_status,
                b"\xffcandidate output\n".as_slice(),
                b"\xffcandidate diagnostic\n".as_slice(),
            ),
            (
                "reference",
                reference_status,
                b"\x80reference output\n",
                b"\x80reference diagnostic\n",
            ),
        ] {
            let directory = run.evidence.join(format!("locale-000001/{role}"));
            assert_eq!(fs::read(directory.join("stdout")).unwrap(), stdout);
            assert_eq!(fs::read(directory.join("stderr")).unwrap(), stderr);
            let waited = read_json(&directory.join("wait.json"));
            if role == "candidate" && signal {
                assert_eq!(waited["signal"], 15);
                assert!(waited["exit_code"].is_null());
            } else {
                assert_eq!(waited["exit_code"], status);
                assert!(waited["signal"].is_null());
            }
        }
        run.finish();
    }
}

#[test]
fn failed_spawn_stops_without_inventing_a_report_or_an_unstarted_native_wait() {
    for failed_role in ["candidate", "reference"] {
        let mut run = Run::new(
            failed_role,
            br#"{"locales":{"a":{"tag":"a"},"b":{"tag":"b"}}}"#,
        );
        run.configure(&[(
            "a",
            reply(b"\xffnot parsed\n", b"native error", 17),
            reply(b"a\n", b"", 0),
        )]);
        if failed_role == "candidate" {
            run.candidate = run.root.join("missing");
        } else {
            run.reference = run.root.join("missing");
        }
        let output = run.invoke("run", &run.arguments(&[]));
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!run.report.exists());
        assert!(run.evidence.join("failure.txt").is_file());
        assert!(!run.evidence.join("locale-000002").exists());
        let failed = run.evidence.join(format!("locale-000001/{failed_role}"));
        let waited = read_json(&failed.join("wait.json"));
        assert!(waited["spawn_error"].is_string());
        assert_eq!(waited["reaped"], false);
        assert!(waited["pid"].is_null());
        assert!(waited["raw_status"].is_null());
        assert!(!failed.join("stdin-delivery.json").exists());
        assert!(failed.join("command.json").is_file());
        assert_eq!(fs::read(failed.join("stdin")).unwrap(), expected_input());
        let observations = run.observations();
        assert_eq!(observations.len(), usize::from(failed_role == "reference"));
        if failed_role == "candidate" {
            assert!(!run.evidence.join("locale-000001/reference").exists());
        } else {
            assert_eq!(
                read_json(&run.evidence.join("locale-000001/candidate/wait.json"))["exit_code"],
                17
            );
        }
        run.finish();
    }
}

#[test]
fn successful_invalid_first_fields_abort_after_both_roles_with_partial_evidence() {
    for failed_role in ["candidate", "reference"] {
        let run = Run::new(
            failed_role,
            br#"{"locales":{"a":{"tag":"a"},"b":{"tag":"b"}}}"#,
        );
        let valid = reply(b"a\t\xff\n", b"warning", 0);
        let invalid = reply(b"\xff\t1\n", b"warning", 0);
        let pair = if failed_role == "candidate" {
            (invalid, valid)
        } else {
            (valid, invalid)
        };
        run.configure(&[("a", pair.0, pair.1)]);
        let output = run.invoke("run", &run.arguments(&[]));
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(!run.report.exists());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("cannot compare punctuation")
        );
        assert_eq!(run.observations().len(), 2);
        assert!(!run.evidence.join("locale-000002").exists());
        assert!(run.evidence.join("failure.txt").is_file());
        for role in ["candidate", "reference"] {
            assert_eq!(
                read_json(&run.evidence.join(format!("locale-000001/{role}/wait.json")))["exit_code"],
                0
            );
        }
        run.finish();
    }
}

#[test]
fn existing_report_or_evidence_is_refused_without_overwriting_inputs_or_running_roles() {
    for existing in ["report", "evidence", "input-as-output", "dangling-output"] {
        let mut run = Run::new(existing, br#"{"locales":{"a":{"tag":"a"}}}"#);
        match existing {
            "report" => fs::write(&run.report, b"keep existing report").unwrap(),
            "evidence" => {
                fs::create_dir(&run.evidence).unwrap();
                fs::write(run.evidence.join("keep"), b"existing evidence").unwrap();
            }
            "input-as-output" => run.report = run.verification.clone(),
            "dangling-output" => symlink("missing-target", &run.report).unwrap(),
            _ => unreachable!(),
        }
        let output = run.invoke("run", &run.arguments(&[]));
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(run.observations().is_empty());
        if existing == "evidence" {
            assert_eq!(
                fs::read(run.evidence.join("keep")).unwrap(),
                b"existing evidence"
            );
            assert_eq!(fs::read_dir(&run.evidence).unwrap().count(), 1);
            assert!(!run.report.exists());
        } else {
            assert!(!run.evidence.exists());
            if existing == "report" {
                assert_eq!(fs::read(&run.report).unwrap(), b"keep existing report");
            }
            if existing == "dangling-output" {
                assert_eq!(
                    fs::read_link(&run.report).unwrap(),
                    Path::new("missing-target")
                );
            }
        }
        run.finish();
    }
}

#[test]
fn help_argument_and_selection_errors_have_no_native_side_effects() {
    let run = Run::new(
        "usage",
        br#"{"locales":{"a":{"tag":"a"},"empty":{"tag":""},"null":{"tag":null}}}"#,
    );
    for option in ["--help", "-h"] {
        let output = run.invoke(option.trim_start_matches('-'), &[option.into()]);
        assert_success(&output, USAGE, &run.root);
    }
    let mut invalid = vec![
        vec![],
        vec!["--evidence".into()],
        vec!["--locale".into()],
        vec!["--unknown".into()],
        vec!["--locale".into(), OsString::from_vec(b"bad\xff".to_vec())],
    ];
    invalid.push(run.arguments(&[])[2..].to_vec());
    let mut duplicate_evidence = vec!["--evidence".into(), "extra-evidence".into()];
    duplicate_evidence.extend(run.arguments(&[]));
    invalid.push(duplicate_evidence);
    for (index, args) in invalid.into_iter().enumerate() {
        let output = run.invoke(&format!("invalid-{index}"), &args);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(USAGE));
    }
    for name in ["unknown", "empty", "null"] {
        let output = run.invoke(name, &run.arguments(&[name]));
        assert_eq!(output.status.code(), Some(1));
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("no nonempty verification tag")
        );
    }
    assert!(!run.evidence.exists());
    assert!(!run.report.exists());
    assert!(run.observations().is_empty());
    run.finish();
}
