#![forbid(unsafe_code)]
#![cfg(unix)]

use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::OsStrExt;
#[cfg(target_os = "linux")]
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

#[rustfmt::skip]
#[path = "fixtures/generated_collation.rs"]
mod generated;

const USAGE: &str =
    "Usage: fastmash-generate-collation-table [--variant N] [--] INPUT_JSON OUTPUT_RS";
const VERIFICATION: &str = r#"{
    "locales": {
        "quote\"name": {"tag": "tag\"quote"},
        "back\\slash": {"tag": "tag\\slash"},
        "carriage\rreturn": {"tag": "tag\rcarriage"},
        "line\nbreak": {"tag": "tag\nline"},
        "fr_é": {"tag": "fr-É"},
        "bad\");compile_error!(\"injected": {"tag": "tag\");compile_error!(\"injected"}
    }
}"#;
const CANDIDATES: &[u8] = br#"{"locales":{
    "z":{"tags":["z0","","z-last"]},
    "a":{"tags":["a0","a-last"]}
}}"#;

fn root(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "locale-collation-cli-{label}-{}-{stamp}",
        std::process::id()
    ));
    DirBuilder::new().mode(0o700).create(&root).unwrap();
    root
}

fn invoke(root: &Path, label: &str, args: &[OsString]) -> Output {
    // These fixed, tiny own-program cases use the caller's process-tree bound.
    let output = Command::new(env!("CARGO_BIN_EXE_fastmash-generate-collation-table"))
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .output()
        .unwrap();
    fs::write(root.join(format!("{label}.stdout")), &output.stdout).unwrap();
    fs::write(root.join(format!("{label}.stderr")), &output.stderr).unwrap();
    fs::write(
        root.join(format!("{label}.status")),
        format!("{}\n", output.status),
    )
    .unwrap();
    output
}

fn success_line(count: usize, path: &Path) -> Vec<u8> {
    let mut expected = format!("{count} locales in ").into_bytes();
    expected.extend_from_slice(path.as_os_str().as_bytes());
    expected.push(b'\n');
    expected
}

#[test]
fn actual_cli_help_and_invalid_arguments_create_no_output() {
    let root = root("usage");
    let output_path = Path::new("never-created.rs");
    for (index, args) in [
        vec!["--help".into()],
        vec!["-h".into()],
        vec![
            "source.json".into(),
            output_path.as_os_str().into(),
            "--help".into(),
        ],
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
        vec!["source.json".into()],
        vec![
            "source.json".into(),
            output_path.as_os_str().into(),
            "extra".into(),
        ],
        vec![
            "source.json".into(),
            output_path.as_os_str().into(),
            "--unknown".into(),
        ],
        vec![
            "source.json".into(),
            output_path.as_os_str().into(),
            "--variant".into(),
        ],
        vec![
            "source.json".into(),
            output_path.as_os_str().into(),
            "--variant".into(),
            "bad".into(),
        ],
        vec![
            "source.json".into(),
            output_path.as_os_str().into(),
            "--variant".into(),
            "9223372036854775808".into(),
        ],
        vec![
            "--variant".into(),
            "0".into(),
            "source.json".into(),
            output_path.as_os_str().into(),
            "--variant".into(),
            "1".into(),
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
        assert!(!root.join(output_path).exists());
    }
    assert!(!root.join(output_path).exists());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_default_writes_exact_compiled_golden_for_unusual_data_and_paths() {
    let root = root("golden");
    let input = Path::new("verification ' é\ninput.json");
    let output_path = Path::new("generated ' é\noutput.rs");
    fs::write(root.join(input), VERIFICATION.as_bytes()).unwrap();
    let output = invoke(
        &root,
        "golden",
        &[input.as_os_str().into(), output_path.as_os_str().into()],
    );
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, success_line(6, output_path));
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read(root.join(output_path)).unwrap(),
        include_bytes!("fixtures/generated_collation.rs")
    );
    let expected: &[(&[u8], &str)] = &[
        (b"back\\slash", "tag\\slash"),
        (
            b"bad\");compile_error!(\"injected",
            "tag\");compile_error!(\"injected",
        ),
        (b"carriage\rreturn", "tag\rcarriage"),
        ("fr_é".as_bytes(), "fr-É"),
        (b"line\nbreak", "tag\nline"),
        (b"quote\"name", "tag\"quote"),
    ];
    assert_eq!(generated::COLLATION, expected);
    assert_eq!(fs::read(root.join(input)).unwrap(), VERIFICATION.as_bytes());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_candidate_variants_keep_literal_rows_and_signed_headers() {
    let root = root("variants");
    let input = Path::new("candidates.json");
    fs::write(root.join(input), CANDIDATES).unwrap();
    for (variant, rows) in [
        (
            0_i64,
            "    (b\"\\x61\", \"a0\"),\n    (b\"\\x7a\", \"z0\"),",
        ),
        (1, "    (b\"\\x61\", \"a-last\"),\n    (b\"\\x7a\", \"\"),"),
        (
            -1,
            "    (b\"\\x61\", \"a-last\"),\n    (b\"\\x7a\", \"z-last\"),",
        ),
        (-2, "    (b\"\\x61\", \"a0\"),\n    (b\"\\x7a\", \"\"),"),
    ] {
        let output_path = PathBuf::from(format!("variant-{variant}.rs"));
        let mut args = vec![input.as_os_str().into(), output_path.as_os_str().into()];
        let position = match variant {
            0 => 0,
            1 | -2 => 1,
            _ => 2,
        };
        args.insert(position, "--variant".into());
        args.insert(position + 1, variant.to_string().into());
        let output = invoke(&root, &format!("variant-{variant}"), &args);
        assert!(output.status.success(), "CLI evidence: {}", root.display());
        assert_eq!(output.stdout, success_line(2, &output_path));
        assert!(output.stderr.is_empty());
        let expected = format!(
            "//! MEASUREMENT BUILD of candidate {variant}; do not commit.\n\
/// Sorted by glibc name for binary search.\n\
pub(super) static COLLATION: &[(&[u8], &str)] = &[\n{rows}\n];\n"
        );
        assert_eq!(
            fs::read(root.join(output_path)).unwrap(),
            expected.as_bytes()
        );
    }
    assert_eq!(fs::read(root.join(input)).unwrap(), CANDIDATES);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn actual_cli_failures_preserve_inputs_and_existing_outputs_and_accept_terminated_paths() {
    let root = root("failures");
    for (index, (raw, variant)) in [
        ("{", None),
        ("{}", None),
        (r#"{"locales":{"a":{}}}"#, None),
        (r#"{"locales":{"a":{"tag":true}}}"#, None),
        (r#"{"locales":{"a":{"tags":["a0","a1"]}}}"#, Some(2_i64)),
        (r#"{"locales":{"a":{"tags":["a0","a1"]}}}"#, Some(-3)),
        (r#"{"locales":{"a":{"tags":[]}}}"#, Some(0)),
    ]
    .into_iter()
    .enumerate()
    {
        let input = PathBuf::from(format!("invalid-{index}.json"));
        let output_path = PathBuf::from(format!("invalid-{index}.rs"));
        fs::write(root.join(&input), raw.as_bytes()).unwrap();
        let mut args = vec![input.as_os_str().into(), output_path.as_os_str().into()];
        if let Some(variant) = variant {
            args.extend(["--variant".into(), variant.to_string().into()]);
        }
        let output = invoke(&root, &format!("schema-{index}"), &args);
        assert_eq!(
            output.status.code(),
            Some(1),
            "CLI evidence: {}",
            root.display()
        );
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert!(!root.join(output_path).exists());
        assert_eq!(fs::read(root.join(input)).unwrap(), raw.as_bytes());
    }
    let input = Path::new("valid.json");
    fs::write(root.join(input), VERIFICATION).unwrap();
    let existing = Path::new("existing.rs");
    fs::write(root.join(existing), b"keep existing bytes\n").unwrap();
    for (label, source, destination) in [
        ("existing", input, existing),
        ("same-input", input, input),
        (
            "missing-input",
            Path::new("missing.json"),
            Path::new("missing.rs"),
        ),
        (
            "missing-parent",
            input,
            Path::new("missing-parent/generated.rs"),
        ),
    ] {
        let output = invoke(
            &root,
            label,
            &[source.as_os_str().into(), destination.as_os_str().into()],
        );
        assert_eq!(
            output.status.code(),
            Some(1),
            "CLI evidence: {}",
            root.display()
        );
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
        assert_eq!(fs::read(root.join(input)).unwrap(), VERIFICATION.as_bytes());
        assert_eq!(
            fs::read(root.join(existing)).unwrap(),
            b"keep existing bytes\n"
        );
    }
    assert!(!root.join("missing.rs").exists());
    assert!(!root.join("missing-parent").exists());

    let input = Path::new("-verification.json");
    let output_path = Path::new("--variant");
    fs::write(root.join(input), VERIFICATION).unwrap();
    let output = invoke(
        &root,
        "terminated",
        &[
            "--".into(),
            input.as_os_str().into(),
            output_path.as_os_str().into(),
        ],
    );
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, success_line(6, output_path));
    assert_eq!(
        fs::read(root.join(output_path)).unwrap(),
        include_bytes!("fixtures/generated_collation.rs")
    );
    fs::remove_dir_all(root).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn actual_cli_preserves_non_utf8_input_output_paths_and_summary_bytes() {
    let root = root("linux-bytes");
    let input = PathBuf::from(OsString::from_vec(b"verification-\xff.json".to_vec()));
    let output_path = PathBuf::from(OsString::from_vec(b"generated-\xfe.rs".to_vec()));
    fs::write(root.join(&input), VERIFICATION).unwrap();
    let output = invoke(
        &root,
        "raw-paths",
        &[input.as_os_str().into(), output_path.as_os_str().into()],
    );
    assert!(output.status.success(), "CLI evidence: {}", root.display());
    assert_eq!(output.stdout, success_line(6, &output_path));
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read(root.join(output_path)).unwrap(),
        include_bytes!("fixtures/generated_collation.rs")
    );
    assert_eq!(fs::read(root.join(input)).unwrap(), VERIFICATION.as_bytes());
    fs::remove_dir_all(root).unwrap();
}
