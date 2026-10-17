#![forbid(unsafe_code)]
#![cfg(unix)]

use fastmash_locale_tools::collation_plan::{digit_words, punctuation_words};
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const LEGACY: &[u8] = include_bytes!("../../../scripts/verify_collation_locales.py");
const FIXTURE: &[u8] = include_bytes!("support/collation-words-fixture.rs");
const SCOPE: &str = "Fixture-only traffic. No native collation, Unicode planner parity, reference admission or Python retirement is established.\n";

fn selected_path(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name)
            .unwrap_or_else(|| panic!("the opt-in qualification requires {name}")),
    );
    assert!(path.is_absolute(), "{name} must be an absolute path");
    path
}

// The opt-in caller supplies the whole process-tree deadline, including Python
// and every observer invocation. No independent process framework is needed.
fn run(directory: &Path, python: &Path, arguments: &[OsString]) {
    fs::write(
        directory.join("legacy.command.json"),
        serde_json::to_vec_pretty(&json!({
            "program_bytes": python.as_os_str().as_bytes(),
            "arguments_bytes": arguments.iter().map(|arg| arg.as_bytes()).collect::<Vec<_>>(),
            "working_directory_bytes": directory.as_os_str().as_bytes(),
            "environment": {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "TZ": "UTC"},
            "stdin": "null",
            "requires_outer_process_tree_deadline": true,
        }))
        .unwrap(),
    )
    .unwrap();
    let status = Command::new(python)
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            File::create(directory.join("legacy.stdout")).unwrap(),
        ))
        .stderr(Stdio::from(
            File::create(directory.join("legacy.stderr")).unwrap(),
        ))
        .status()
        .unwrap_or_else(|error| {
            fs::write(directory.join("legacy.spawn-error"), error.to_string()).unwrap();
            panic!(
                "legacy command failed to start: {error}. Evidence: {}",
                directory.display()
            );
        });
    fs::write(
        directory.join("legacy.wait.json"),
        serde_json::to_vec_pretty(&json!({
            "raw_status": status.into_raw(), "exit_code": status.code(), "signal": status.signal(),
        }))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(status.code(), Some(0), "Evidence: {}", directory.display());
    assert!(
        fs::read(directory.join("legacy.stderr"))
            .unwrap()
            .is_empty()
    );
}

fn value(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn payload(words: Vec<String>) -> Vec<u8> {
    words
        .into_iter()
        .flat_map(|word| word.into_bytes().into_iter().chain(*b"\n"))
        .collect()
}

fn check_observations(directory: &Path, locpath: &Path, unused_sort: &Path, calls: usize) {
    let observations = directory.join("observations");
    assert_eq!(fs::read_dir(&observations).unwrap().count(), 2 + calls * 6);
    let environment = json!([
        [b"LC_ALL".as_slice(), b"en_US.UTF-8".as_slice()],
        [b"LOCPATH".as_slice(), locpath.as_os_str().as_bytes()],
        [b"PATH".as_slice(), b"/usr/bin:/bin".as_slice()],
        [b"TZ".as_slice(), b"UTC".as_slice()],
    ]);
    for (role, argv0) in [("candidate", "fastmash"), ("reference", "datamash")] {
        assert_eq!(
            fs::read(observations.join(format!("{role}.count"))).unwrap(),
            format!("{calls}\n").as_bytes()
        );
        let mut arguments = vec![argv0.as_bytes().to_vec()];
        if role == "reference" {
            arguments.extend([
                b"--sort-cmd".to_vec(),
                unused_sort.as_os_str().as_bytes().to_vec(),
            ]);
        }
        arguments.extend(["-s", "-g", "1", "count", "1"].map(|arg| arg.as_bytes().to_vec()));
        for index in 0..calls {
            let prefix = format!("{role}-{index:02}");
            let receipt = value(observations.join(format!("{prefix}.json")));
            let input = fs::read(observations.join(format!("{prefix}.stdin"))).unwrap();
            let output = fs::read(observations.join(format!("{prefix}.stdout"))).unwrap();
            assert_eq!(receipt["role"], role);
            assert_eq!(receipt["index"], index);
            assert_eq!(receipt["arguments"], json!(arguments));
            assert_eq!(receipt["environment"], environment);
            assert_eq!(
                receipt["working_directory"],
                json!(directory.as_os_str().as_bytes())
            );
            assert_eq!(receipt["input_bytes"], input.len());
            assert_eq!(receipt["stdout_bytes"], output.len());
            assert!(input.ends_with(b"\n"));
            let input_lines: Vec<_> = input.split_inclusive(|byte| *byte == b'\n').collect();
            let output_lines: Vec<_> = output.split_inclusive(|byte| *byte == b'\n').collect();
            assert_eq!(input_lines.len(), output_lines.len());
            for (input, output) in input_lines.into_iter().zip(output_lines) {
                assert_eq!(
                    output.strip_suffix(b"\t1\n").unwrap(),
                    input.strip_suffix(b"\n").unwrap()
                );
            }
            let peer = if role == "candidate" {
                "reference"
            } else {
                "candidate"
            };
            assert_eq!(
                input,
                fs::read(observations.join(format!("{peer}-{index:02}.stdin"))).unwrap()
            );
        }
    }
}

#[test]
#[ignore = "requires explicit Python path, new evidence directory and an outer process-tree deadline"]
fn fixed_word_lists_match_captured_legacy_cli_inputs() {
    let python = selected_path("FASTMASH_COLLATION_PLAN_LEGACY_PYTHON");
    let evidence = selected_path("FASTMASH_COLLATION_PLAN_LEGACY_EVIDENCE");
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_collation-words-fixture"));
    assert!(fixture.is_absolute());
    DirBuilder::new().mode(0o700).create(&evidence).unwrap();
    let evidence = evidence.canonicalize().unwrap();
    let script = evidence.join("legacy.py");
    fs::write(&script, LEGACY).unwrap();
    fs::write(evidence.join("observer-source.rs"), FIXTURE).unwrap();
    fs::write(evidence.join("scope.txt"), SCOPE).unwrap();

    for (mode, calls, words, selected_index) in
        [("punctuation", 1, 1983, 0), ("verify", 31, 133, 1)]
    {
        let directory = evidence.join(mode);
        DirBuilder::new().mode(0o700).create(&directory).unwrap();
        fs::create_dir_all(directory.join("data/locales")).unwrap();
        let observations = directory.join("observations");
        DirBuilder::new().mode(0o700).create(&observations).unwrap();
        fs::write(
            directory.join("observer-config.json"),
            serde_json::to_vec_pretty(&json!({
                "observations": observations.as_os_str().as_bytes(),
            }))
            .unwrap(),
        )
        .unwrap();
        fs::write(directory.join("scope.txt"), SCOPE).unwrap();
        let locpath = directory.join("empty-locale-root");
        fs::create_dir(&locpath).unwrap();
        let unused_sort = directory.join("unused-sort-not-executable");
        fs::write(
            &unused_sort,
            b"This fixture must never execute a sort command.\n",
        )
        .unwrap();
        assert_eq!(
            fs::metadata(&unused_sort).unwrap().permissions().mode() & 0o111,
            0
        );
        let mut arguments = vec!["-I".into(), script.as_os_str().into(), mode.into()];
        if mode == "punctuation" {
            fs::write(
                directory.join("data/locales/collation-verification.json"),
                serde_json::to_vec_pretty(
                    &json!({"locales": {"en_US": {"tag": "fixture-control"}}}),
                )
                .unwrap(),
            )
            .unwrap();
        } else {
            fs::write(directory.join("data/locales/collation-candidates.json"),
                serde_json::to_vec_pretty(&json!({
                    "icu": "fixture-only-not-native-reference", "glibc": "fixture-only-not-native-reference",
                    "locales": {"en_US": {"tags": ["fixture-control"], "alphabet": [], "auxiliary": []}},
                })).unwrap()).unwrap();
            arguments.push("0".into());
        }
        arguments.extend([
            fixture.as_os_str().into(),
            fixture.as_os_str().into(),
            unused_sort.as_os_str().into(),
            locpath.as_os_str().into(),
        ]);
        if mode == "verify" {
            arguments.push("en_US".into());
        }
        run(&directory, &python, &arguments);
        let expected_stdout = if mode == "punctuation" {
            "1 locales, 1983 words; locales by displaced words: 0: 1\n"
        } else {
            "en_US: verified\n1 of 1 locales verified with candidate 0; 1 locales verified overall\n"
        };
        assert_eq!(
            fs::read(directory.join("legacy.stdout")).unwrap(),
            expected_stdout.as_bytes()
        );
        check_observations(&directory, &locpath, &unused_sort, calls);
        if mode == "punctuation" {
            assert_eq!(
                value(directory.join("data/locales/collation-punctuation.json")),
                json!({
                    "words": 1983, "locales": {"en_US": {"displaced": 0, "words": []}},
                })
            );
        } else {
            let report = value(directory.join("data/locales/collation-verification.json"));
            assert_eq!(report["icu"], "fixture-only-not-native-reference");
            assert_eq!(report["glibc"], "fixture-only-not-native-reference");
            assert_eq!(
                report["locales"],
                json!({"en_US": {"tag": "fixture-control", "alphabet_data": true}})
            );
            assert_eq!(
                report["variants"]["0"]["locales"],
                json!({"en_US": {
                    "verified": true, "tag": "fixture-control", "failures": {}, "other_scripts": {},
                }})
            );
            let baseline = report["variants"]["0"]["baseline_displaced"]
                .as_object()
                .unwrap();
            assert_eq!(baseline.len(), 29);
            assert!(baseline.values().all(|words| words == &json!([])));
        }
        // The old CLI has completed before constructing either Rust payload.
        let rust_input = payload(if mode == "punctuation" {
            punctuation_words()
        } else {
            digit_words()
        });
        assert_eq!(
            rust_input.iter().filter(|byte| **byte == b'\n').count(),
            words
        );
        fs::write(directory.join("rust-fixed-words.stdin"), &rust_input).unwrap();
        for role in ["candidate", "reference"] {
            assert_eq!(
                rust_input,
                fs::read(observations.join(format!("{role}-{selected_index:02}.stdin"))).unwrap(),
                "fixed {mode} words differ. Evidence: {}",
                directory.display()
            );
        }
        assert!(fs::read_dir(&locpath).unwrap().next().is_none());
        assert_eq!(fs::read(&script).unwrap(), LEGACY);
    }
    fs::write(evidence.join("passed.txt"), b"Fixed digit (133 words) and punctuation (1983 words) LF payloads match both independently captured legacy CLI inputs. No native ordering, Unicode planner parity, reference admission or Python retirement is established.\n").unwrap();
    println!(
        "Fixed collation word-list parity evidence: {}",
        evidence.display()
    );
}
