//! Destination-specific readable health presentation and its data counterpart.
#[path = "support/terminal.rs"]
mod terminal;
const PROGRAM: terminal::Program = terminal::Program;

#[test]
fn terminal_health_preserves_findings_and_distinguishes_validation() {
    let input = b"reading\n10\n12\noops\n";
    let args = ["--header-in", "health", "type", "reading", "number"];
    let inspection = PROGRAM.invoke(&args, input, &[], true, false);
    assert!(inspection.status.success());
    let mut args = args.to_vec();
    args.push("validate");
    let validation = PROGRAM.invoke(&args, input, &[], true, false);
    assert_eq!(validation.status.code(), Some(1));
    let text = String::from_utf8(terminal::strip_styles(&validation.stdout)).unwrap();
    for expected in [
        "✗ Validation failed",
        "3",
        "header records: 1",
        "accepted records: 4",
        "mixed_types [observation, advisory]\n  1 fields",
        "type_mismatch [declared, violation]\n  1 cells; expected number",
        "accepted record 4, field 1: observed text; sample \"oops\"; truncated no",
        "examples retained 1, omitted 0",
        "Input and reporting",
        "Sample byte limit: 128",
    ] {
        assert!(text.contains(expected), "missing {expected:?}: {text}");
    }
    assert!(
        validation
            .stdout
            .windows(b"38;2;255;92;87".len())
            .any(|s| s == b"38;2;255;92;87")
    );
    assert!(
        validation
            .stdout
            .windows(b"38;2;243;156;18".len())
            .any(|s| s == b"38;2;243;156;18")
    );
    let success = PROGRAM.invoke(&args, b"reading\n10\n12\n", &[], true, false);
    assert!(success.status.success());
    assert!(String::from_utf8_lossy(&success.stdout).contains("✔ Validation passed"));
    let advisory = PROGRAM.invoke(&["--header-in", "health"], input, &[], true, false);
    assert!(advisory.status.success());
    assert!(String::from_utf8_lossy(&advisory.stdout).contains("⚠ Inspection complete"));
}

#[test]
fn redirected_health_remains_plain_even_when_color_is_forced() {
    let input = b"1\nword\n";
    let plain = PROGRAM.invoke(&["health"], input, &[], false, false);
    for mode in ["--color=auto", "--color=always", "--no-color"] {
        let output = PROGRAM.invoke(&[mode, "health"], input, &[], false, true);
        assert_eq!(output.status, plain.status);
        assert_eq!(output.stdout, plain.stdout);
    }
    for environment in [vec![("NO_COLOR", "1")], vec![("TERM", "dumb")]] {
        let output = PROGRAM.invoke(&["health"], input, &environment, true, false);
        assert_eq!(output.stdout, plain.stdout);
    }
}

#[test]
fn terminal_health_keeps_binary_samples_escaped_and_discloses_omissions() {
    let input = b"1\n2\n\x1b[31m\xff\0\r\\\nword\n";
    let output = PROGRAM.invoke(
        &[
            "health", "type", "1", "integer", "examples", "1", "validate",
        ],
        input,
        &[],
        true,
        false,
    );
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(terminal::strip_styles(&output.stdout)).unwrap();
    assert!(text.contains("\\x1B[31m\\xFF\\0\\r\\\\"), "{text}");
    assert!(text.contains("examples retained 1, omitted 1"), "{text}");
}

#[test]
fn health_tsv_keeps_its_bytes_on_a_terminal() {
    let args = ["health", "tsv", "type", "1", "integer", "validate"];
    let plain = PROGRAM.invoke(&args, b"1\nword\n", &[], false, false);
    let output = PROGRAM.invoke(&args, b"1\nword\n", &[], true, false);
    assert_eq!(output.status, plain.status);
    assert_eq!(output.stdout, plain.stdout);
    assert!(!output.stdout.contains(&0x1b));
}
