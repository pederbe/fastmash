#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;
use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn command(args: &[&str], input: &[u8]) -> Output {
    let binary = std::env::var_os("FASTMASH_SUM_MEAN_TEST_BINARY")
        .or_else(|| std::env::var_os("FASTMASH_TEST_BINARY"))
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
    let mut child = Command::new(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(input);
    child.wait_with_output().unwrap()
}

#[test]
fn sum_and_mean_ignore_wide_unselected_fields() {
    let input = format!("1.25\t{}\n2.75\t{}", "x".repeat(9000), "y".repeat(9000));
    let out = command(&["sum", "1", "mean", "1"], input.as_bytes());
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"4\t2\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn decimal_commands_preserve_order_range_and_special_values() {
    for (input, expected) in [
        ("", ""),
        ("1.25\n", "1.25\t1.25\n"),
        ("  +1.25e2\n-2.5e1", "100\t50\n"),
        (
            "9007199254740992\n1\n-9007199254740992\n",
            "1\t0.33333333333333\n",
        ),
        ("18446744073709551616\n1\n-18446744073709551616\n", "0\t0\n"),
        ("-0\n", "0\t0\n"),
        ("1e4000\n", "1e+4000\t1e+4000\n"),
        ("1e4932\n1e4932\n", "inf\tinf\n"),
        ("0x1p2\n0x1\n", "5\t2.5\n"),
        ("inf\n-inf\n", "nan\tnan\n"),
        ("-nan\n1\n", "-nan\t-nan\n"),
    ] {
        let out = command(&["sum", "1", "mean", "1"], input.as_bytes());
        assert_eq!(
            out.status.code(),
            Some(0),
            "{input:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, expected.as_bytes(), "{input:?}");
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn wide_specials_and_hex_do_not_parse_unselected_storage() {
    for (value, expected) in [
        ("0x1p2", "4\t4\n"),
        ("-nan", "-nan\t-nan\n"),
        ("inf", "inf\tinf\n"),
    ] {
        let out = command(
            &["sum", "1", "mean", "1"],
            format!("{value}\t{}\n", "x".repeat(20000)).as_bytes(),
        );
        assert_eq!(out.status.code(), Some(0));
        assert_eq!(out.stdout, expected.as_bytes());
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn long_decimal_coefficient_and_exponent_cancel_without_a_field_cap() {
    let input = format!("1{}e-10001\n", "0".repeat(10001));
    let out = command(&["sum", "1", "mean", "1"], input.as_bytes());
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(out.stdout, b"1\t1\n");
    assert!(out.stderr.is_empty());
}

#[test]
fn repeated_named_fields_reset_counts_and_preserve_errors() {
    let out = command(
        &[
            "-H", "--narm", "-g", "group", "mean", "value", "sum", "2", "mean", "2",
        ],
        b"group\tvalue\na\t1\na\t3\nb\tNA\nb\tNaN\nc\t5\n",
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(out.stdout, b"GroupBy(group)\tmean(value)\tsum(value)\tmean(value)\na\t2\t4\t2\nb\tnan\t0\tnan\nc\t5\t5\t5\n");
    for input in [
        b"1e5000\tbad\n".as_slice(),
        b"1e-5000\tbad\n",
        b"1e-4950\tbad\n",
        b"1 \tbad\n",
        b"1\0x\tbad\n",
    ] {
        let out = command(&["sum", "1", "sum", "2", "mean", "1"], input);
        assert_eq!(
            out.status.code(),
            Some(1),
            "{input:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
        assert!(String::from_utf8_lossy(&out.stderr).contains("line 1 field 1:"));
    }
    let out = command(&["-t", "e", "sum", "1"], b"1e5000\n");
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("field 1: '1'"));
}

#[test]
fn write_failure_is_not_success() {
    let binary = std::env::var_os("FASTMASH_SUM_MEAN_TEST_BINARY")
        .or_else(|| std::env::var_os("FASTMASH_TEST_BINARY"))
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
    let mut child = Command::new(binary)
        .args(["sum", "1", "mean", "1"])
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .full_stdout(true)
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"1\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("write error"));
}

#[test]
#[ignore = "requires the installed CLI and sorter pair via FASTMASH_SUM_MEAN_TEST_BINARY"]
fn sorted_wide_headers_comments_and_named_fields() {
    assert!(std::env::var_os("FASTMASH_SUM_MEAN_TEST_BINARY").is_some());
    let padding = "x".repeat(20000);
    let input = format!("#{padding}\ngroup\tvalue\t{padding}\nb\t3\t{padding}\na\t1\t{padding}");
    let out = command(
        &[
            "--skip-comments",
            "-H",
            "-s",
            "-g",
            "group",
            "sum",
            "value",
            "mean",
            "2",
        ],
        input.as_bytes(),
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        out.stdout,
        b"GroupBy(group)\tsum(value)\tmean(value)\na\t1\t1\nb\t3\t3\n"
    );
    assert!(out.stderr.is_empty());
}

/// Sorted numeric jobs keep only the selected fields, yet read each number
/// as GNU does, from the field on through the rest of the record: the parse
/// runs past a field of blanks into the next field, and past an exponent
/// letter into a sign separator (GNU datamash 1.9, field-ops.c:378-405).
#[test]
fn sorted_numbers_read_on_past_their_field_as_gnu_does() {
    let blanks = " ".repeat(600);
    let blanks_exponent = format!("{}1e", " ".repeat(510));
    for (args, input, status, stdout, stderr) in [
        (
            vec!["-s", "-g", "1", "sum", "2"],
            format!("a\t{blanks}\t5\n"),
            1,
            "",
            "internal error: input field too long (600)\n",
        ),
        (
            vec!["-s", "-g", "1", "sum", "2"],
            "a\t \t5\n".to_string(),
            1,
            "",
            "invalid numeric value in line 1 field 2: ' '\n",
        ),
        (
            vec!["-t", "+", "-s", "-g", "1", "sum", "2"],
            format!("a+{blanks_exponent}+5\n"),
            1,
            "",
            "internal error: input field too long (512)\n",
        ),
        (
            vec!["-t", ",", "-s", "-g", "1", "sum", "2", "max", "2"],
            "b,2\na,1e5\nb,-3\n".to_string(),
            0,
            "a,100000,100000\nb,-1,2\n",
            "",
        ),
    ] {
        let out = command(&args, input.as_bytes());
        assert_eq!(out.status.code(), Some(status), "{args:?}");
        assert_eq!(String::from_utf8_lossy(&out.stdout), stdout, "{args:?}");
        // After the program name, which is the binary's path here.
        let message = String::from_utf8_lossy(&out.stderr);
        let message = message.split_once(": ").map_or(&*message, |(_, rest)| rest);
        assert_eq!(message, stderr, "{args:?}");
    }
}
