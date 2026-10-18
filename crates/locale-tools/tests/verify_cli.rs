#![forbid(unsafe_code)]
#![cfg(unix)]

use fastmash_locale_tools::collation_plan::digit_words;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

const MIXED: &[u8] = include_bytes!("fixtures/unicode15/mixed.json");
const SUCCESS: &[u8] = include_bytes!("fixtures/verification-executor/success.stdout");
const BLOCK_INPUTS: &[(&str, &[u8])] = &[
    ("latin", include_bytes!("fixtures/unicode15/latin.stdin")),
    (
        "vietnamese",
        include_bytes!("fixtures/unicode15/vietnamese.stdin"),
    ),
    ("greek", include_bytes!("fixtures/unicode15/greek.stdin")),
    (
        "cyrillic",
        include_bytes!("fixtures/unicode15/cyrillic.stdin"),
    ),
    (
        "armenian",
        include_bytes!("fixtures/unicode15/armenian.stdin"),
    ),
    ("hebrew", include_bytes!("fixtures/unicode15/hebrew.stdin")),
    ("arabic", include_bytes!("fixtures/unicode15/arabic.stdin")),
    (
        "devanagari",
        include_bytes!("fixtures/unicode15/devanagari.stdin"),
    ),
    (
        "bengali",
        include_bytes!("fixtures/unicode15/bengali.stdin"),
    ),
    (
        "gurmukhi",
        include_bytes!("fixtures/unicode15/gurmukhi.stdin"),
    ),
    (
        "gujarati",
        include_bytes!("fixtures/unicode15/gujarati.stdin"),
    ),
    ("oriya", include_bytes!("fixtures/unicode15/oriya.stdin")),
    ("tamil", include_bytes!("fixtures/unicode15/tamil.stdin")),
    ("telugu", include_bytes!("fixtures/unicode15/telugu.stdin")),
    (
        "kannada",
        include_bytes!("fixtures/unicode15/kannada.stdin"),
    ),
    (
        "malayalam",
        include_bytes!("fixtures/unicode15/malayalam.stdin"),
    ),
    (
        "sinhala",
        include_bytes!("fixtures/unicode15/sinhala.stdin"),
    ),
    ("thai", include_bytes!("fixtures/unicode15/thai.stdin")),
    ("lao", include_bytes!("fixtures/unicode15/lao.stdin")),
    (
        "tibetan",
        include_bytes!("fixtures/unicode15/tibetan.stdin"),
    ),
    (
        "myanmar",
        include_bytes!("fixtures/unicode15/myanmar.stdin"),
    ),
    (
        "georgian",
        include_bytes!("fixtures/unicode15/georgian.stdin"),
    ),
    (
        "ethiopic",
        include_bytes!("fixtures/unicode15/ethiopic.stdin"),
    ),
    ("khmer", include_bytes!("fixtures/unicode15/khmer.stdin")),
    ("thaana", include_bytes!("fixtures/unicode15/thaana.stdin")),
    (
        "hiragana",
        include_bytes!("fixtures/unicode15/hiragana.stdin"),
    ),
    (
        "katakana",
        include_bytes!("fixtures/unicode15/katakana.stdin"),
    ),
    ("hangul", include_bytes!("fixtures/unicode15/hangul.stdin")),
    ("han", include_bytes!("fixtures/unicode15/han.stdin")),
];

struct Run {
    owned: PathBuf,
    root: PathBuf,
    candidates: PathBuf,
    original: Vec<u8>,
    candidate: PathBuf,
    reference: PathBuf,
    sort: PathBuf,
    locpath: PathBuf,
    output: PathBuf,
    evidence: PathBuf,
    observations: PathBuf,
}

impl Run {
    fn new(label: &str, candidates: &[u8]) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let owned =
            std::env::temp_dir().join(format!("verify-cli-{label}-{}-{stamp}", std::process::id()));
        DirBuilder::new().mode(0o700).create(&owned).unwrap();
        let root = owned.join("via-link");
        symlink(".", &root).unwrap();
        let candidate = root.join(filesystem_name("candidate ' "));
        let reference = root.join(filesystem_name("reference ' "));
        symlink(env!("CARGO_BIN_EXE_collation-words-fixture"), &candidate).unwrap();
        symlink(env!("CARGO_BIN_EXE_collation-words-fixture"), &reference).unwrap();
        // A leading option-shaped relative filename requires the CLI's --.
        let input = PathBuf::from("--candidates.json");
        fs::write(root.join(&input), candidates).unwrap();
        let observations = root.join("observations");
        fs::create_dir(&observations).unwrap();
        let run = Self {
            candidates: input,
            original: candidates.to_vec(),
            candidate,
            reference,
            sort: PathBuf::from(OsString::from_vec(b"sort ' \xff\n".to_vec())),
            locpath: PathBuf::from(OsString::from_vec(b"locales ' \xfe\n".to_vec())),
            output: root.join("report.json"),
            evidence: root.join("evidence"),
            observations,
            owned,
            root,
        };
        run.observer(json!({}));
        run
    }

    fn observer(&self, replies: Value) {
        fs::write(
            self.root.join("observer-config.json"),
            serde_json::to_vec(&json!({
                "observations": self.observations.as_os_str().as_bytes(), "replies": replies,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn fixed_replies(&mut self, locales: Value) {
        self.candidate = env!("CARGO_BIN_EXE_collation-punctuation-fixture").into();
        self.reference = self.candidate.clone();
        fs::write(
            self.root.join("punctuation-fixture.json"),
            serde_json::to_vec(&json!({
                "observations": self.root.join("calls.jsonl").as_os_str().as_bytes(),
                "locales": locales,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    fn arguments(&self, locales: &[&str], variant: i64) -> Vec<OsString> {
        let mut args = vec![
            "--variant".into(),
            variant.to_string().into(),
            "--evidence".into(),
            self.evidence.as_os_str().to_owned(),
        ];
        for locale in locales {
            args.extend(["--locale".into(), (*locale).into()]);
        }
        args.push("--".into());
        args.extend(
            [
                &self.candidates,
                &self.candidate,
                &self.reference,
                &self.sort,
                &self.locpath,
                &self.output,
            ]
            .map(|path| path.as_os_str().to_owned()),
        );
        args
    }

    fn invoke(&self, args: &[OsString]) -> Output {
        // These fixed local fixtures use the caller's process-tree bound.
        // They do not run a native collator or a timing comparison.
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash-verify-collation-locales"))
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("VERIFICATION_PARENT_ONLY", "must not reach a role")
            .output()
            .unwrap();
        fs::write(self.root.join("cli.stdout"), &output.stdout).unwrap();
        fs::write(self.root.join("cli.stderr"), &output.stderr).unwrap();
        fs::write(self.root.join("cli.status"), format!("{}\n", output.status)).unwrap();
        output
    }

    fn count(&self, role: &str) -> usize {
        match fs::read_to_string(self.observations.join(format!("{role}.count"))) {
            Ok(raw) => raw.trim_end().parse().unwrap(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
            Err(error) => panic!("cannot read observer count: {error}"),
        }
    }

    fn fixed_calls(&self) -> Vec<Value> {
        fs::read_to_string(self.root.join("calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn role_directory(&self, occurrence: usize, index: usize, label: &str, role: &str) -> PathBuf {
        self.evidence.join(format!(
            "locale-{occurrence:06}/case-{index:02}-{label}/{role}"
        ))
    }

    fn finish(self) {
        assert_eq!(
            fs::read(self.root.join(&self.candidates)).unwrap(),
            self.original
        );
        fs::remove_dir_all(self.owned).unwrap();
    }
}

fn filesystem_name(prefix: &str) -> OsString {
    let mut bytes = prefix.as_bytes().to_vec();
    #[cfg(target_os = "linux")]
    bytes.push(0xff);
    #[cfg(not(target_os = "linux"))]
    bytes.extend_from_slice("é".as_bytes());
    bytes.push(b'\n');
    OsString::from_vec(bytes)
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn candidates(entries: Value) -> Vec<u8> {
    serde_json::to_vec(&json!({"icu": "fixture", "glibc": "fixture", "locales": entries})).unwrap()
}

fn entry(tag: &str, alphabet: Value) -> Value {
    json!({"tags": [tag], "alphabet": alphabet, "auxiliary": []})
}

fn reply(stdout: &[u8], stderr: &[u8], status: u8) -> Value {
    json!({"stdout": stdout, "stderr": stderr, "status": status})
}

fn assert_success(run: &Run, output: &Output, stdout: &[u8]) {
    assert!(
        output.status.success(),
        "CLI evidence: {}",
        run.root.display()
    );
    assert_eq!(output.stdout, stdout);
    assert!(
        output.stderr.is_empty(),
        "CLI evidence: {}",
        run.root.display()
    );
}

fn assert_failure(run: &Run, output: &Output, message: &str) {
    assert!(
        !output.status.success(),
        "CLI evidence: {}",
        run.root.display()
    );
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "CLI evidence: {}",
        run.root.display()
    );
    assert!(!run.output.exists());
}

fn assert_contract(
    run: &Run,
    occurrence: usize,
    index: usize,
    label: &str,
    name: &str,
    input: &[u8],
) {
    let cwd = fs::canonicalize(&run.root).unwrap();
    for (role, argv0, program) in [
        ("candidate", "fastmash", &run.candidate),
        ("reference", "datamash", &run.reference),
    ] {
        let global_index = (occurrence - 1) * 31 + index;
        let observed = read_json(
            &run.observations
                .join(format!("{role}-{global_index:02}.json")),
        );
        let mut arguments = vec![argv0.as_bytes().to_vec()];
        if role == "reference" {
            arguments.extend([
                b"--sort-cmd".to_vec(),
                run.sort.as_os_str().as_bytes().to_vec(),
            ]);
        }
        arguments.extend([b"-s".as_slice(), b"-g", b"1", b"count", b"1"].map(<[u8]>::to_vec));
        assert_eq!(observed["arguments"], json!(arguments));
        assert_eq!(
            observed["environment"],
            json!([
                [b"LC_ALL".as_slice(), format!("{name}.UTF-8").as_bytes()],
                [b"LOCPATH".as_slice(), run.locpath.as_os_str().as_bytes()],
                [b"PATH".as_slice(), b"/usr/bin:/bin".as_slice()],
                [b"TZ".as_slice(), b"UTC".as_slice()],
            ])
        );
        assert_eq!(
            observed["working_directory"],
            json!(cwd.as_os_str().as_bytes())
        );
        assert_eq!(
            fs::read(
                run.observations
                    .join(format!("{role}-{global_index:02}.stdin"))
            )
            .unwrap(),
            input
        );
        let directory = run.role_directory(occurrence, index, label, role);
        let command = read_json(&directory.join("command.json"));
        assert_eq!(
            command["program_bytes"],
            json!(program.as_os_str().as_bytes())
        );
        assert_eq!(command["argv0"], argv0);
        assert_eq!(command["arguments_bytes"], json!(&arguments[1..]));
        assert_eq!(command["locale_key"], name);
        assert_eq!(command["environment"]["PATH"], "/usr/bin:/bin");
        assert_eq!(command["environment"]["LC_ALL"], format!("{name}.UTF-8"));
        assert_eq!(
            command["environment"]["LOCPATH_bytes"],
            json!(run.locpath.as_os_str().as_bytes())
        );
        assert_eq!(command["environment"]["TZ"], "UTC");
        assert_eq!(
            command["working_directory_bytes"],
            observed["working_directory"]
        );
        assert_eq!(command["environment_cleared"], true);
        assert_eq!(command["stdin_transport"], "pipe");
        assert_eq!(command["stdout_transport"], "regular-file");
        assert_eq!(command["stderr_transport"], "regular-file");
        assert_eq!(command["legacy_output_transport"], "pipe");
        assert_eq!(fs::read(directory.join("stdin")).unwrap(), input);
        assert_eq!(
            fs::read(directory.join("stdout")).unwrap(),
            fs::read(
                run.observations
                    .join(format!("{role}-{global_index:02}.stdout"))
            )
            .unwrap()
        );
        let delivered = read_json(&directory.join("stdin-delivery.json"));
        assert_eq!(delivered["delivered"], true);
        assert_eq!(delivered["writer_closed"], true);
        assert_eq!(delivered["broken_pipe"], false);
        assert!(delivered["error"].is_null());
        if index == 0 {
            assert_eq!(delivered["bytes_written"], input.len());
        }
        let waited = read_json(&directory.join("wait.json"));
        assert_eq!(waited["reaped"], true);
        assert_eq!(waited["timed_out"], false);
        assert_eq!(waited["execution_milliseconds"], 300_000);
        assert_eq!(waited["cleanup_milliseconds"], 1_000);
        assert_eq!(waited["timeout_starts_after_spawn"], true);
        assert_eq!(waited["exit_code"], 0);
        assert!(waited["raw_status"].is_number());
        assert!(waited["signal"].is_null());
        assert!(waited["input_error"].is_null());
        if index == 0 {
            // Signal receipts do not establish synchronous descendant reaping.
            assert!(matches!(
                waited["group_signal"].as_str(),
                Some("sent" | "already_absent")
            ));
        }
        assert!(input.ends_with(b"\n"));
    }
}

#[test]
fn full_unicode15_inputs_and_success_reports_match_independent_captures() {
    for (label, candidates, alphabet, expected) in [
        (
            "mixed",
            MIXED,
            include_bytes!("fixtures/unicode15/alphabet-mixed.stdin").as_slice(),
            include_bytes!("fixtures/verification-executor/mixed.json").as_slice(),
        ),
        (
            "over120",
            include_bytes!("fixtures/unicode15/over120.json").as_slice(),
            include_bytes!("fixtures/unicode15/alphabet-over120.stdin").as_slice(),
            include_bytes!("fixtures/verification-executor/over120.json").as_slice(),
        ),
    ] {
        let run = Run::new(label, candidates);
        // Nonempty stderr from successful roles is retained and accepted.
        run.observer(
            json!({"candidate": [{"stderr": b"candidate warning", "status": 0}, null],
            "reference": [{"stderr": b"reference warning", "status": 0}, null]}),
        );
        let output = run.invoke(&run.arguments(&[], 0));
        assert_success(&run, &output, SUCCESS);
        assert_eq!(fs::read(&run.output).unwrap(), expected);
        assert_eq!(run.count("candidate"), 31);
        assert_eq!(run.count("reference"), 31);
        assert_contract(&run, 1, 0, "alphabet", "en_US", alphabet);
        // Digit planning has independent fixed-list coverage. Here it checks
        // that this runner delivers that owner's entire list unchanged.
        let digits = format!("{}\n", digit_words().join("\n"));
        assert_contract(&run, 1, 1, "digits", "en_US", digits.as_bytes());
        for (index, (name, input)) in BLOCK_INPUTS.iter().enumerate() {
            assert_contract(&run, 1, index + 2, name, "en_US", input);
        }
        for (role, expected) in [
            ("candidate", b"candidate warning".as_slice()),
            ("reference", b"reference warning".as_slice()),
        ] {
            assert_eq!(
                fs::read(run.role_directory(1, 0, "alphabet", role).join("stderr")).unwrap(),
                expected
            );
        }
        run.finish();
    }
}

#[test]
fn either_native_failure_continues_all_cases_and_matches_original_error_outputs() {
    let run = Run::new(
        "errors",
        include_bytes!("fixtures/verification-console/candidates.json"),
    );
    let candidate_alphabet =
        include_bytes!("fixtures/verification-console/candidate-alphabet.stderr");
    let reference_alphabet =
        include_bytes!("fixtures/verification-console/reference-alphabet.stderr");
    let candidate_digits = include_bytes!("fixtures/verification-console/candidate-digits.stderr");
    let reference_digits = include_bytes!("fixtures/verification-console/reference-digits.stderr");
    run.observer(json!({
        "candidate": [{"stderr": candidate_alphabet.as_slice(), "status": 77},
            {"stderr": candidate_digits.as_slice(), "status": 0}],
        "reference": [{"stderr": reference_alphabet.as_slice(), "status": 0},
            {"stderr": reference_digits.as_slice(), "status": 23}],
    }));
    let output = run.invoke(&run.arguments(&[], 0));
    assert_success(
        &run,
        &output,
        include_bytes!("fixtures/verification-console/errors.stdout"),
    );
    assert_eq!(
        fs::read(&run.output).unwrap(),
        include_bytes!("fixtures/verification-console/errors.json")
    );
    assert_eq!(run.count("candidate"), 31);
    assert_eq!(run.count("reference"), 31);
    for (index, label, role, stderr, status) in [
        (
            0,
            "alphabet",
            "candidate",
            candidate_alphabet.as_slice(),
            77,
        ),
        (0, "alphabet", "reference", reference_alphabet.as_slice(), 0),
        (1, "digits", "candidate", candidate_digits.as_slice(), 0),
        (1, "digits", "reference", reference_digits.as_slice(), 23),
    ] {
        let directory = run.role_directory(1, index, label, role);
        assert_eq!(fs::read(directory.join("stderr")).unwrap(), stderr);
        assert_eq!(read_json(&directory.join("wait.json"))["exit_code"], status);
    }
    run.finish();
}

#[test]
fn selection_keeps_default_sort_explicit_sequence_duplicates_and_unfiltered_names() {
    let raw = candidates(json!({"cmn_RAW_more": entry("han", Value::Null),
        "en_US": entry("", json!([])), "a_AA": entry("a", json!([]))}));
    let run = Run::new("default", &raw);
    let output = run.invoke(&run.arguments(&[], 0));
    assert_success(&run, &output, b"a_AA: verified\ncmn_RAW_more: verified\nen_US: verified\n3 of 3 locales verified with candidate 0; 2 locales verified overall\n");
    assert_eq!(
        read_json(&run.evidence.join("selection.json"))["names"],
        json!(["a_AA", "cmn_RAW_more", "en_US"])
    );
    for (index, name) in ["a_AA", "cmn_RAW_more", "en_US"].into_iter().enumerate() {
        let observed = read_json(
            &run.observations
                .join(format!("candidate-{:02}.json", index * 31)),
        );
        assert_eq!(
            observed["environment"][0][1],
            json!(format!("{name}.UTF-8").as_bytes())
        );
    }
    assert_eq!(run.count("candidate"), 93);
    assert_eq!(run.count("reference"), 93);
    let alphabet = fs::read(run.observations.join("candidate-31.stdin")).unwrap();
    assert!(
        alphabet
            .split(|&byte| byte == b'\n')
            .any(|word| word == "\u{3400}".as_bytes())
    );
    let report = read_json(&run.output);
    assert_eq!(report["locales"]["cmn_RAW_more"]["alphabet_data"], false);
    assert_eq!(report["locales"]["en_US"]["alphabet_data"], true);
    run.finish();

    let raw = candidates(json!({
        "nan-variant_RAW.UTF-8": {"tags": ["z0", "last"], "alphabet": [], "auxiliary": []},
        "en_US": {"tags": ["en0", "en-last"], "alphabet": [], "auxiliary": []},
        // An unselected entry is not indexed at the chosen variant or planned.
        "unselected": {"tags": [], "alphabet": []},
    }));
    let run = Run::new("explicit", &raw);
    let output =
        run.invoke(&run.arguments(&["nan-variant_RAW.UTF-8", "nan-variant_RAW.UTF-8"], -1));
    assert_success(&run, &output, b"en_US: verified\nnan-variant_RAW.UTF-8: verified\nnan-variant_RAW.UTF-8: verified\n2 of 2 locales verified with candidate -1; 0 locales verified overall\n");
    assert_eq!(
        read_json(&run.evidence.join("selection.json"))["names"],
        json!(["nan-variant_RAW.UTF-8", "nan-variant_RAW.UTF-8", "en_US"])
    );
    for (index, name) in ["nan-variant_RAW.UTF-8", "nan-variant_RAW.UTF-8", "en_US"]
        .into_iter()
        .enumerate()
    {
        let observed = read_json(
            &run.observations
                .join(format!("candidate-{:02}.json", index * 31)),
        );
        assert_eq!(
            observed["environment"][0][1],
            json!(format!("{name}.UTF-8").as_bytes())
        );
    }
    assert_eq!(run.count("candidate"), 93);
    assert_eq!(run.count("reference"), 93);
    let alphabet = fs::read(run.observations.join("candidate-00.stdin")).unwrap();
    assert!(
        !alphabet
            .split(|&byte| byte == b'\n')
            .any(|word| word == "\u{3400}".as_bytes())
    );
    let report = read_json(&run.output);
    assert_eq!(
        report["variants"]["-1"]["locales"]["nan-variant_RAW.UTF-8"]["tag"],
        "last"
    );
    assert!(report["locales"]["unselected"]["tag"].is_null());
    run.finish();
}

#[test]
fn prior_history_signed_variant_and_empty_word_survive_the_cli() {
    let mut raw: Value = serde_json::from_slice(MIXED).unwrap();
    raw["locales"]["en_US"]["alphabet"]
        .as_array_mut()
        .unwrap()
        .push(json!(""));
    raw["locales"]["en_US"]["tags"]
        .as_array_mut()
        .unwrap()
        .push(json!("last"));
    let run = Run::new("prior-empty", &serde_json::to_vec(&raw).unwrap());
    let prior_bytes = include_bytes!("fixtures/verification-executor/mixed.json");
    let prior = run.root.join("prior.json");
    fs::write(&prior, prior_bytes).unwrap();
    let mut args = run.arguments(&[], -1);
    args.splice(
        0..0,
        [OsString::from("--prior"), prior.as_os_str().to_owned()],
    );
    let output = run.invoke(&args);
    assert_success(
        &run,
        &output,
        b"en_US: verified\n1 of 1 locales verified with candidate -1; 1 locales verified overall\n",
    );
    let report = read_json(&run.output);
    let original: Value = serde_json::from_slice(prior_bytes).unwrap();
    assert_eq!(report["variants"]["0"], original["variants"]["0"]);
    assert_eq!(report["locales"], original["locales"]);
    assert_eq!(report["variants"]["-1"]["locales"]["en_US"]["tag"], "last");
    assert!(
        fs::read(run.observations.join("candidate-00.stdin"))
            .unwrap()
            .starts_with(b"\n")
    );
    assert_eq!(fs::read(&prior).unwrap(), prior_bytes);
    assert_eq!(
        fs::read(run.evidence.join("prior.json")).unwrap(),
        prior_bytes
    );
    run.finish();
}

#[test]
fn successful_projection_accepts_invalid_tail_bytes_and_retains_unequal_zero() {
    for (label, candidate, reference, stdout) in [
        ("tails", b"a\t\x1c\x1d\x1e\xc2\x85\xe2\x80\xa8\xe2\x80\xa9\xff\r\nb\t1\n".as_slice(), b"a\t2\rb\t\xfe\r\n".as_slice(), SUCCESS),
        ("unequal-zero", b"a\t1\n".as_slice(), b"a\t1\nb\t1\n".as_slice(),
            b"en_US: differs alphabet:0 digits:0\n0 of 1 locales verified with candidate 0; 0 locales verified overall\n".as_slice()),
    ] {
        let mut run = Run::new(label, MIXED);
        run.fixed_replies(json!({"en_US.UTF-8": {"candidate": reply(candidate, b"success stderr", 0),
            "reference": reply(reference, b"", 0)}}));
        let output = run.invoke(&run.arguments(&[], 0));
        assert_success(&run, &output, stdout);
        let calls = run.fixed_calls();
        assert_eq!(calls.len(), 62);
        for pair in calls.as_chunks::<2>().0 {
            assert_eq!(pair[0]["role"], "candidate");
            assert_eq!(pair[1]["role"], "reference");
            for call in pair {
                assert_eq!(call["input_kind"], "pipe");
                assert_eq!(call["eof"], true);
            }
        }
        let report = read_json(&run.output);
        if label == "unequal-zero" {
            assert_eq!(report["variants"]["0"]["locales"]["en_US"]["failures"],
                json!({"alphabet": {"displaced": 0, "first": []}, "digits": {"displaced": 0, "first": []}}));
        } else {
            assert_eq!(fs::read(&run.output).unwrap(), include_bytes!("fixtures/verification-executor/mixed.json"));
        }
        run.finish();
    }
}

#[test]
fn block_failures_promote_only_null_alphabets_and_never_replace_a_failed_baseline() {
    let raw = candidates(json!({"null": entry("null", Value::Null),
        "empty": entry("empty", json!([])), "en_US": entry("en", json!([]))}));
    let mut run = Run::new("block-failures", &raw);
    let success = reply(b"a\t1\n", b"", 0);
    // Failed native output must never enter the strict successful parser.
    let failure = reply(b"\xff\n", b"native failure", 77);
    run.fixed_replies(json!({
        "null.UTF-8": {"candidate": failure, "reference": success},
        "empty.UTF-8": {"candidate": failure, "reference": success},
        "en_US.UTF-8": {"candidate": success, "reference": success},
    }));
    let output = run.invoke(&run.arguments(&[], 0));
    assert!(
        output.status.success(),
        "CLI evidence: {}",
        run.root.display()
    );
    let report = read_json(&run.output);
    let records = &report["variants"]["0"]["locales"];
    assert_eq!(records["empty"]["failures"].as_object().unwrap().len(), 2);
    assert_eq!(records["null"]["failures"].as_object().unwrap().len(), 31);
    for (block, _) in BLOCK_INPUTS {
        assert_eq!(
            records["null"]["failures"][format!("block:{block}")],
            "error"
        );
        assert_eq!(records["empty"]["other_scripts"][*block], "error");
    }
    assert_eq!(run.fixed_calls().len(), 186);
    run.finish();

    let mut run = Run::new("baseline-failure", MIXED);
    run.fixed_replies(json!({"en_US.UTF-8": {
        "candidate": reply(b"a\t1\n", b"baseline failed", 77),
        "reference": reply(b"a\t1\n", b"", 0),
    }}));
    let output = run.invoke(&run.arguments(&[], 0));
    assert_failure(&run, &output, "en_US baseline block");
    assert!(run.evidence.join("failure.txt").exists());
    assert_eq!(run.fixed_calls().len(), 62);
    assert_eq!(
        read_json(
            &run.role_directory(1, 30, "han", "candidate")
                .join("wait.json")
        )["exit_code"],
        77
    );
    run.finish();
}

#[test]
fn transport_and_successful_decode_errors_stop_with_honest_partial_evidence() {
    for role in ["candidate", "reference"] {
        let mut run = Run::new(role, MIXED);
        if role == "candidate" {
            run.candidate = run.root.join("missing");
        } else {
            run.reference = run.root.join("missing");
        }
        let output = run.invoke(&run.arguments(&[], 0));
        assert_failure(&run, &output, "verification alphabet");
        let waited = read_json(&run.role_directory(1, 0, "alphabet", role).join("wait.json"));
        assert!(waited["spawn_error"].is_string());
        assert_eq!(waited["reaped"], false);
        assert!(waited["pid"].is_null());
        assert!(waited["raw_status"].is_null());
        assert_eq!(run.count("candidate"), usize::from(role == "reference"));
        assert_eq!(run.count("reference"), 0);
        assert!(!run.evidence.join("locale-000001/case-01-digits").exists());
        if role == "candidate" {
            assert!(!run.role_directory(1, 0, "alphabet", "reference").exists());
        }
        run.finish();
    }
    for (label, candidate) in [
        ("strict-utf8", b"\xff\n".as_slice()),
        ("truncated-first-field", b"\xc2\t\x85\n".as_slice()),
        ("missing-rank", b"other\n".as_slice()),
    ] {
        let mut run = Run::new(label, MIXED);
        run.fixed_replies(json!({"en_US.UTF-8": {
            "candidate": reply(candidate, b"", 0), "reference": reply(b"a\t1\n", b"", 0),
        }}));
        let output = run.invoke(&run.arguments(&[], 0));
        assert_failure(&run, &output, "cannot compare alphabet");
        assert_eq!(run.fixed_calls().len(), 2);
        for role in ["candidate", "reference"] {
            let waited = read_json(&run.role_directory(1, 0, "alphabet", role).join("wait.json"));
            assert_eq!(waited["reaped"], true);
            assert_eq!(waited["exit_code"], 0);
        }
        assert!(!run.evidence.join("locale-000001/case-01-digits").exists());
        run.finish();
    }
}

#[test]
fn invalid_selection_and_missing_or_invalid_exemplars_never_launch_roles() {
    for (label, raw, requested, message) in [
        (
            "missing-name",
            MIXED.to_vec(),
            vec!["missing"],
            "not a candidate",
        ),
        (
            "missing-baseline",
            candidates(json!({"only": entry("only", json!([]))})),
            vec![],
            "not a candidate",
        ),
        (
            "missing-alphabet",
            candidates(json!({"en_US": {"tags": ["en"], "auxiliary": []}})),
            vec![],
            "no alphabet field",
        ),
        (
            "missing-auxiliary",
            candidates(json!({"en_US": {"tags": ["en"], "alphabet": []}})),
            vec![],
            "no auxiliary field",
        ),
        (
            "bad-exemplar",
            candidates(json!({"en_US": {"tags": ["en"], "alphabet": [1], "auxiliary": null}})),
            vec![],
            "must contain strings",
        ),
    ] {
        let run = Run::new(label, &raw);
        let output = run.invoke(&run.arguments(&requested, 0));
        assert_failure(&run, &output, message);
        assert_eq!(run.count("candidate"), 0);
        assert_eq!(run.count("reference"), 0);
        assert!(run.evidence.join("failure.txt").exists());
        run.finish();
    }
}

#[test]
fn existing_paths_help_and_argument_errors_leave_inputs_untouched() {
    let run = Run::new("existing-output", MIXED);
    fs::write(&run.output, b"keep original report").unwrap();
    let output = run.invoke(&run.arguments(&[], 0));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("output already exists"));
    assert_eq!(fs::read(&run.output).unwrap(), b"keep original report");
    assert!(!run.evidence.exists());
    assert_eq!(run.count("candidate"), 0);
    run.finish();

    let run = Run::new("existing-evidence", MIXED);
    fs::create_dir(&run.evidence).unwrap();
    fs::write(run.evidence.join("keep"), b"untouched").unwrap();
    let output = run.invoke(&run.arguments(&[], 0));
    assert_failure(&run, &output, "cannot create verification evidence");
    assert_eq!(fs::read(run.evidence.join("keep")).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(&run.evidence).unwrap().count(), 1);
    assert_eq!(run.count("candidate"), 0);
    run.finish();

    let run = Run::new("arguments", MIXED);
    let output = run.invoke(&["--help".into()]);
    assert!(output.status.success());
    assert!(
        output
            .stdout
            .starts_with(b"Usage: fastmash-verify-collation-locales ")
    );
    assert!(output.stderr.is_empty());
    for args in [
        vec![OsString::from("--variant")],
        vec!["--unknown".into()],
        vec!["--locale".into(), OsString::from_vec(vec![0xff])],
    ] {
        let output = run.invoke(&args);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    assert!(!run.output.exists());
    assert!(!run.evidence.exists());
    assert_eq!(run.count("candidate"), 0);
    run.finish();
}
