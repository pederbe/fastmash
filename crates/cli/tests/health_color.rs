//! Readable Health styling through the executable's existing Command seam.
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;
use std::{fs, process::Output};

const PROGRAM: terminal::Program = terminal::Program;

fn contains(bytes: &[u8], expected: &[u8]) -> bool {
    bytes.windows(expected.len()).any(|part| part == expected)
}

fn pair(args: &[&str], input: &[u8], status: i32) -> Output {
    let mut controls = vec!["--color=never"];
    controls.extend(args);
    let plain = PROGRAM.invoke(&controls, input, &[], false, false);
    controls[0] = "--color=always";
    let styled = PROGRAM.invoke(&controls, input, &[], false, false);
    assert_eq!(plain.status.code(), Some(status), "{plain:?}");
    assert_eq!(styled.status, plain.status, "{styled:?}");
    assert!(plain.stderr.is_empty() && styled.stderr.is_empty());
    assert_eq!(terminal::strip_styles(&styled.stdout), plain.stdout);
    styled
}

#[test]
fn readable_roles_follow_findings_and_leave_observations_plain() {
    let styled = pair(&["health"], b"1\n2\nword\nNA\n\n", 0);
    for heading in [
        b"\x1b[1mTable health report\x1b[0m\nData records: 5".as_slice(),
        b"\x1b[1mField observations (data-record cells):\x1b[0m\n",
        b"\x1b[1mFindings (counts may overlap):\x1b[0m\n",
    ] {
        assert!(contains(&styled.stdout, heading), "missing {heading:?}");
    }
    for finding in [
        b"width_mismatch [inferred, \x1b[33madvisory\x1b[0m]: 1 data_records".as_slice(),
        b"absent_field [observation, \x1b[33madvisory\x1b[0m]: 1 cells",
        b"missing_indicator [observation, \x1b[33madvisory\x1b[0m]: 1 cells",
        b"mixed_types [observation, \x1b[33madvisory\x1b[0m]: 1 fields",
        b"type_mismatch [inferred, \x1b[33madvisory\x1b[0m]: 1 cells",
    ] {
        assert!(contains(&styled.stdout, finding), "missing {finding:?}");
    }
    assert!(!contains(&styled.stdout, b"\x1b[31m"));
    for line in styled.stdout.split(|byte| *byte == b'\n') {
        if line.starts_with(b"  field ") || line.starts_with(b"    ") {
            assert!(
                !line.contains(&0x1b),
                "styled observation or example: {line:?}"
            );
        }
    }
}

#[test]
fn validation_color_distinguishes_advice_from_declared_failures() {
    let advisory = pair(
        &["health", "width", "1", "validate"],
        b"1\n2\nword\nNA\n",
        0,
    );
    assert!(contains(&advisory.stdout, b"\x1b[33madvisory\x1b[0m"));
    assert!(!contains(&advisory.stdout, b"\x1b[31m"));
    let controls = ["health", "type", "1", "integer", "nonmissing", "1"];
    let inspection = pair(&controls, b"1\n2\nword\nNA\n", 0);
    let mut validating = controls.to_vec();
    validating.push("validate");
    let violation = pair(&validating, b"1\n2\nword\nNA\n", 1);
    assert_eq!(inspection.stdout, violation.stdout);
    for finding in [
        b"mixed_types [observation, \x1b[33madvisory\x1b[0m]: 1 fields".as_slice(),
        b"type_mismatch [declared, \x1b[31mviolation\x1b[0m]: 1 cells",
        b"presence_mismatch [declared, \x1b[31mviolation\x1b[0m]: 1 cells",
    ] {
        assert!(contains(&violation.stdout, finding), "missing {finding:?}");
    }
    assert!(!contains(&violation.stdout, b"type_mismatch [inferred"));
    let width = pair(
        &["-H", "health", "width", "1", "validate"],
        b"a\tb\n1\t2\n",
        1,
    );
    for category in ["header_width_mismatch", "width_mismatch"] {
        assert!(contains(
            &width.stdout,
            format!("{category} [declared, \x1b[31mviolation\x1b[0m]: 1").as_bytes()
        ));
    }
}

#[test]
fn no_findings_adds_only_the_existing_heading_styles() {
    for input in [b"".as_slice(), b"1\n2\n"] {
        let styled = pair(&["health"], input, 0);
        assert!(styled.stdout.ends_with(b"\x1b[0m\n  none\n"));
        assert!(!contains(&styled.stdout, b"\x1b[31m"));
        assert!(!contains(&styled.stdout, b"\x1b[33m"));
        assert!(!contains(&styled.stdout, b"\x1b[36m"));
    }
    pair(&["health", "width", "1", "validate"], b"1\n2\n", 0);
}

#[test]
fn styling_preserves_binary_escaping_truncation_locations_and_omissions() {
    let mut input =
        b"violation\tadvisory\tadvisory\t\n1\t2\tNA\t\n3\t4\t\x1b[31m\xff\0\r\\\t\n5\t6\t".to_vec();
    input.extend([b'x'; 160]);
    input.extend_from_slice(b"\t\n7\t8\tother\t\n9\n");
    let styled = pair(
        &[
            "-H", "health", "type", "3", "integer", "examples", "2", "validate",
        ],
        &input,
        1,
    );
    for expected in [
        b"duplicate_header [observation, \x1b[33madvisory\x1b[0m]: 1 header_fields".as_slice(),
        b"blank_header [observation, \x1b[33madvisory\x1b[0m]: 1 header_fields",
        b"empty_field [observation, \x1b[33madvisory\x1b[0m]",
        b"name \"violation\"",
        b"name \"advisory\"",
        b"accepted record 3, field 3: observed text; sample \"\\x1B[31m\\xFF\\0\\r\\\\\"; truncated no",
        b"type_mismatch [declared, \x1b[31mviolation\x1b[0m]: 3 cells; expected integer; field 3, name \"advisory\"; examples retained 2, omitted 1",
        b"\"; truncated yes",
    ] {
        assert!(contains(&styled.stdout, expected), "missing {expected:?}");
    }
    let nul = pair(&["-z", "health"], b"1\x002\0line\nwith\r\tbytes\0", 0);
    assert!(contains(&nul.stdout, b"line\\nwith\\r\\tbytes"));
}

#[test]
fn automatic_health_checks_stdout_and_overrides_including_redirected_files() {
    let input = b"1\nword\n";
    let plain = PROGRAM.invoke(&["health"], input, &[], false, false);
    for stdout_terminal in [false, true] {
        for stderr_terminal in [false, true] {
            for (environment, eligible) in [
                (vec![], true),
                (vec![("NO_COLOR", "")], true),
                (vec![("NO_COLOR", "1")], false),
                (vec![("TERM", "dumb")], false),
            ] {
                let output = PROGRAM.invoke(
                    &["health"],
                    input,
                    &environment,
                    stdout_terminal,
                    stderr_terminal,
                );
                assert!(output.status.success());
                assert_eq!(output.stdout.contains(&0x1b), stdout_terminal && eligible);
                assert_eq!(terminal::strip_styles(&output.stdout), plain.stdout);
                assert!(output.stderr.is_empty());
            }
        }
    }
    for (controls, styled) in [
        (vec!["--color", "always"], true),
        (vec!["--color=always", "--no-color"], false),
        (vec!["--no-color", "--color=always"], true),
        (vec!["--color=never"], false),
        (vec!["--color=always", "--color=auto"], false),
    ] {
        let mut args = controls;
        args.push("health");
        let output = PROGRAM.invoke(
            &args,
            input,
            &[("NO_COLOR", "1"), ("TERM", "dumb")],
            true,
            false,
        );
        assert_eq!(output.stdout.contains(&0x1b), styled);
        assert_eq!(terminal::strip_styles(&output.stdout), plain.stdout);
    }
    let directory = temp_dir::TempDir::new("health-color-redirect");
    let source = directory.0.join("input");
    let destination = directory.0.join("report");
    fs::write(&source, input).unwrap();
    for mode in [None, Some("--color=always"), Some("--no-color")] {
        let mut args: Vec<_> = mode.into_iter().collect();
        args.push("health");
        let output = PROGRAM
            .command(&args, &[])
            .stdin(fs::File::open(&source).unwrap())
            .stdout(fs::File::create(&destination).unwrap())
            .output()
            .unwrap();
        assert!(output.status.success());
        let report = fs::read(&destination).unwrap();
        assert_eq!(report.contains(&0x1b), mode == Some("--color=always"));
        assert_eq!(terminal::strip_styles(&report), plain.stdout);
    }
}

#[test]
fn tsv_stays_byte_identical_for_every_mode_and_validation_outcome() {
    for (input, status) in [
        (b"1\n2\n".as_slice(), 0),
        (b"1\nword\nNA\n\x1b[31m\xff\n", 1),
        (b"", 0),
    ] {
        let args = [
            "health",
            "tsv",
            "type",
            "1",
            "integer",
            "nonmissing",
            "1",
            "validate",
        ];
        let plain = PROGRAM.invoke(&args, input, &[], false, false);
        assert_eq!(plain.status.code(), Some(status));
        for mode in [
            "--color=auto",
            "--color=always",
            "--color=never",
            "--no-color",
        ] {
            let mut controls = vec![mode];
            controls.extend(args);
            let output = PROGRAM.invoke(&controls, input, &[], true, true);
            assert_eq!(output.status, plain.status);
            assert_eq!(output.stdout, plain.stdout);
            assert!(output.stderr.is_empty());
            assert!(!output.stdout.contains(&0x1b));
        }
    }
}
