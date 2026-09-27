use super::{args, candidate, compare_cases, invoke};
use std::{ffi::OsString, fs, os::unix::ffi::OsStringExt};

const WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";

#[test]
fn representative_rows_and_ragged_aggregation() {
    for (a, input, expected) in [
        (
            args(&["-f", "sum", "2"]),
            b"a\t1\nx\t3\n".as_slice(),
            b"a\t1\t4\n".as_slice(),
        ),
        (
            args(&["-f", "min", "2", "max", "2"]),
            b"a\t3\nb\t1\nc\t2\nd\t8\n".as_slice(),
            b"d\t8\t1\t8\n".as_slice(),
        ),
        (
            args(&["-Wf", "--header-out", "last", "2"]),
            b" a  1 \n b  2 \n".as_slice(),
            b"field-1\tfield-2\tfield-3\tlast(field-2)\nb\t2\t\t2\n".as_slice(),
        ),
        (
            args(&["-zf", "--no-strict", "--filler=X", "last", "1"]),
            b"a\tb\0c\0".as_slice(),
            b"c\tc\0".as_slice(),
        ),
    ] {
        let result = invoke(&candidate(), &a, input, false, false);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, expected);
        assert_eq!(result.stderr, WARNING);
    }
    let result = invoke(
        &candidate(),
        &args(&["--no-strict", "-F", "X", "count", "2"]),
        b"a\n",
        false,
        false,
    );
    assert_eq!(result.status.code(), Some(1));
    assert_eq!(
        result.stderr,
        b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\n"
    );
}

#[test]
fn full_text_spill_retains_wide_unselected_fields_and_fails_output() {
    let mut input = b"b\tx\t".to_vec();
    input.extend(vec![0xff; 100_000]);
    input.extend_from_slice(b"\0a\ty\tshort\0");
    let mut expected = b"a\ty\tshort\ty\0b\tx\t".to_vec();
    expected.extend(vec![0xff; 100_000]);
    expected.extend_from_slice(b"\tx\0");
    let a = args(&["-zfs", "-g1", "first", "2"]);
    let result = invoke(&candidate(), &a, &input, true, false);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, expected);
    assert_eq!(result.stderr, WARNING);
    let result = invoke(&candidate(), &a, &input, true, true);
    assert_eq!(result.status.code(), Some(1));
    assert!(result.stderr.starts_with(WARNING));
    assert!(String::from_utf8_lossy(&result.stderr).contains("No space left on device"));
}

#[test]
#[ignore = "requires named GNU reference and fresh full-row evidence directory"]
fn named_reference_full_rows() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_FULL_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for zero in [false, true] {
        let end = if zero { 0 } else { b'\n' };
        for header in [None, Some("-H"), Some("--header-out"), Some("--header-in")] {
            for grouping in [None, Some("-g1"), Some("-sg1")] {
                for operations in [
                    vec!["count", "2"],
                    vec!["sum", "2"],
                    vec!["first", "2"],
                    vec!["last", "2"],
                    vec!["min", "2", "max", "2"],
                    vec!["last", "2", "min", "3"],
                    vec!["pcov", "2:3"],
                    vec!["rand", "2"],
                ] {
                    let mut a = args(&["-f", "-S1"]);
                    if zero {
                        a.push("-z".into());
                    }
                    if let Some(header) = header {
                        a.push(header.into());
                    }
                    if let Some(grouping) = grouping {
                        a.push(grouping.into());
                    }
                    a.extend(args(&operations));
                    let mut input = Vec::new();
                    if matches!(header, Some("-H" | "--header-in")) {
                        input.extend_from_slice(b"key\tx\ty\tunused");
                        input.push(end);
                    }
                    for row in [
                        b"b\t3\t9\tfirst".as_slice(),
                        b"b\t1\t8\tsecond\textra",
                        b"a\t4\t7\tthird",
                        b"a\t2\t6",
                    ] {
                        input.extend_from_slice(row);
                        input.push(end);
                    }
                    cases.push((a, input));
                }
            }
        }
    }
    for header in [None, Some("-H"), Some("--header-in"), Some("--header-out")] {
        for grouping in [None, Some("-g3"), Some("-sg3")] {
            for input in [
                b"".as_slice(),
                b"a\tb\n",
                b"\n",
                b"a\tb\nx\ty\n",
                b"a\0tail\tb\nx\0rest\ty\n",
            ] {
                let mut a = args(&["-f"]);
                if let Some(header) = header {
                    a.push(header.into());
                }
                if let Some(grouping) = grouping {
                    a.push(grouping.into());
                }
                a.extend(args(&["count", "1"]));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for operation in ["first", "last", "min", "max", "absmin", "absmax", "range"] {
        for flags in [vec!["-f", "--narm"], vec!["-fs", "-g1", "--narm"]] {
            let mut a = args(&flags);
            a.extend(args(&[operation, "2"]));
            cases.push((a, b"a\tNA\tfirst\na\t3\tsecond\na\t2\tthird\n".to_vec()));
        }
    }
    for flags in [
        vec!["--no-strict"],
        vec!["-F", "X"],
        vec!["--no-strict", "--filler="],
        vec!["--filler=a", "--filler=b", "--full"],
    ] {
        for input in [b"a\tb\nc\n".as_slice(), b"\n", b"\ta\n\tb\tc\n"] {
            for field in ["1", "2"] {
                let mut a = args(&flags);
                a.extend(args(&["count", field]));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for a in [
        vec!["--full=x", "count", "1"],
        vec!["--no-strict=x", "count", "1"],
        vec!["--filler"],
        vec!["-fH", "sum", "absent"],
        vec!["-fsH", "-gabsent", "sum", "2"],
    ] {
        cases.push((args(&a), b"a\tb\n".to_vec()));
    }
    for flags in [
        vec!["-fW"],
        vec!["-fWH"],
        vec!["-fWs", "-g1"],
        vec!["-fWsH", "-g1"],
    ] {
        let mut a = args(&flags);
        a.extend(args(&["--output-delimiter=|", "collapse", "2"]));
        cases.push((a, b" key  value \n b\tsecond \n a  third \n".to_vec()));
    }
    cases.push((
        vec![
            "-f".into(),
            "-F".into(),
            OsString::from_vec(vec![0xff]),
            "count".into(),
            "1".into(),
        ],
        b"\xff\t\x80\n".to_vec(),
    ));
    compare_cases(&cases, &reference, &evidence);
}
