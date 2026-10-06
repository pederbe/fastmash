#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;

use std::{
    ffi::OsString,
    fs,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
    time::Instant,
};

fn candidate() -> OsString {
    std::env::var_os("FASTMASH_SELECTOR_BINARY")
        .or_else(|| std::env::var_os("FASTMASH_TEST_BINARY"))
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into())
}
fn args(values: &[&str]) -> Vec<OsString> {
    values.iter().map(OsString::from).collect()
}
fn invoke(binary: &std::ffi::OsStr, arguments: &[OsString], input: &[u8]) -> Output {
    invoke_output(binary, arguments, input, false)
}
fn invoke_output(
    binary: &std::ffi::OsStr,
    arguments: &[OsString],
    input: &[u8],
    full: bool,
) -> Output {
    let path = std::env::temp_dir().join(format!(
        "selector-{}-{:?}.tsv",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let output = Command::new(binary)
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdin(fs::File::open(&path).unwrap())
        .full_stdout(full)
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    output
}

#[test]
fn expansion_refusal_and_large_header_write_failure_are_explicit() {
    let output = invoke(
        &candidate(),
        &args(&["count", "1-9223372036854775807"]),
        b"x\n",
    );
    assert_eq!(output.status.code(), Some(77));
    assert_eq!(
        output.stderr,
        b"fastmash: command memory allocation failed\n"
    );
    assert!(output.stdout.is_empty());
    let (arguments, input, _) = wide_job();
    let output = invoke_output(&candidate(), &arguments, &input, true);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("No space left on device"));
}

fn wide_job() -> (Vec<OsString>, Vec<u8>, Vec<u8>) {
    let names: Vec<_> = (0..512)
        .map(|n| format!("column_{n:04}_long_name"))
        .collect();
    let header = names.join("\t");
    let data = vec!["1"; 512].join("\t");
    let input = format!("{header}\n{data}\n{data}\n").into_bytes();
    let arguments = args(&["-H", "count", "1-512"]);
    let expected = format!(
        "{}\n{}\n",
        names
            .iter()
            .map(|s| format!("count({s})"))
            .collect::<Vec<_>>()
            .join("\t"),
        vec!["2"; 512].join("\t")
    )
    .into_bytes();
    (arguments, input, expected)
}

fn distinct_sum_job() -> (Vec<OsString>, Vec<u8>, Vec<u8>) {
    let names: Vec<_> = (0..512)
        .map(|n| format!("column_{n:04}_long_name"))
        .collect();
    let row = (1..=512)
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join("\t")
        + "\n";
    let input = (names.join("\t") + "\n" + &row.repeat(64)).into_bytes();
    let expected = ((1..=512)
        .map(|n| (n * 64).to_string())
        .collect::<Vec<_>>()
        .join("\t")
        + "\n")
        .into_bytes();
    (
        args(&["--header-in", "sum", &names.join(",")]),
        input,
        expected,
    )
}

#[test]
fn wide_headers_and_many_calculations_are_useful() {
    let (arguments, input, expected) = wide_job();
    let start = Instant::now();
    let output = invoke(&candidate(), &arguments, &input);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(output.stdout, expected);
    eprintln!("512-column header/range elapsed: {:?}", start.elapsed());
    let mut arguments = Vec::new();
    for _ in 0..1200 {
        arguments.extend(args(&["count", "1"]));
    }
    let output = invoke(&candidate(), &arguments, b"a\nb\n");
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        output.stdout,
        format!("{}\n", vec!["2"; 1200].join("\t")).as_bytes()
    );
    let (arguments, input, expected) = distinct_sum_job();
    let start = Instant::now();
    let output = invoke(&candidate(), &arguments, &input);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(output.stdout, expected);
    eprintln!(
        "512 distinct named sums, 64 rows elapsed: {:?}",
        start.elapsed()
    );
}

#[test]
#[ignore = "requires the named GNU reference and fresh evidence directory"]
fn named_reference_selectors_and_headers() {
    use std::os::unix::ffi::OsStringExt;
    let reference = std::env::var_os("FASTMASH_SELECTOR_REFERENCE").unwrap();
    let evidence =
        std::path::PathBuf::from(std::env::var_os("FASTMASH_SELECTOR_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases: Vec<(Vec<OsString>, Vec<u8>)> = Vec::new();
    cases.push((
        args(&[
            "-H",
            "-t|",
            "--output-delimiter=;",
            "sum",
            "b,a",
            "first",
            "a",
        ]),
        b"a|b\n1|2\n3|4\n".to_vec(),
    ));
    cases.push((
        args(&["-WH", "--output-delimiter=,", "sum", "b,a", "count", "a"]),
        b" a  b \n1  2\n 3\t4 \n".to_vec(),
    ));
    cases.push((
        args(&["-H", "-t|", "sum", r"a\ b,c\-d"]),
        b"a b|c-d\n1|2\n3|4\n".to_vec(),
    ));
    for flags in [
        &[][..],
        &["--header-in"][..],
        &["--header-out"][..],
        &["-H"][..],
    ] {
        for operations in [
            &["count", "3,1-2,1"][..],
            &["first", "2", "last", "1"][..],
            &["sum", "1", "mean", "2"][..],
            &["pcov", "1:2,2:3"][..],
        ] {
            let mut a = args(flags);
            a.extend(args(operations));
            cases.push((a, b"1\t2\t3\n2\t4\t6\n3\t6\t9\n".to_vec()));
        }
    }
    for a in [
        vec!["-H", "count", "a,b,a"],
        vec!["--header-in", "sum", "a", "mean", "b"],
        vec!["-H", "pcov", "a:b,b:a"],
        vec!["-H", "count", "missing,a"],
        vec!["-H", "count", "a,missing"],
        vec!["count", "a"],
        vec!["-H", "count", "a-b"],
        vec!["-H", "pcov", "a:b,c:a"],
        vec!["-H", "-g", "a", "count", "b", "first", "a"],
        vec!["-H", "-s", "-g", "a", "count", "b", "first", "a"],
        vec!["-H", "-t|", "--output-delimiter=;", "first", "b,a"],
        vec!["-WH", "first", "b,a"],
    ] {
        cases.push((args(&a), b"a\tb\ta\n1\t2\t3\n1\t4\t6\n".to_vec()));
    }
    for selector in [
        "", "0", "-1", "1-1", "2-1", "1-", "1,,2", "1,", ":", "1::2", "1:", "1:2", "a-2", "1-a",
        "a\\", "a\\ b", "\\1", "a\\-b",
    ] {
        for operation in ["count", "pcov"] {
            cases.push((args(&["-H", operation, selector]), b"a\tb\na\tb\n".to_vec()));
        }
    }
    for a in [
        vec!["--", "--help"],
        vec!["--", "-"],
        vec![" "],
        vec!["!"],
        vec!["bogus", "1"],
        vec![r"c\ount", "1"],
        vec!["count", "1", r"f\irst", "2"],
    ] {
        cases.push((args(&a), b"a\tb\n".to_vec()));
    }
    for length in [511, 512] {
        let name = "a".repeat(length);
        cases.push((
            args(&["-H", "count", &name]),
            format!("{name}\nx\n").into_bytes(),
        ));
    }
    cases.push((
        vec![
            "-H".into(),
            "count".into(),
            OsString::from_vec(b"\\\xff".to_vec()),
        ],
        b"\xff\nx\n".to_vec(),
    ));
    for input in [
        b"".as_slice(),
        b"a\tb\n",
        b"a\ta\n1\t2\n",
        b"a\0tail\tb\n1\t2\n",
    ] {
        cases.push((args(&["-H", "count", "a,a"]), input.to_vec()));
    }
    let (a, input, _) = wide_job();
    cases.push((a, input));
    let (a, input, _) = distinct_sum_job();
    cases.push((a, input));
    cases.push((
        args(&["count", &format!("{}1", "0".repeat(20_000))]),
        b"a\nb\n".to_vec(),
    ));
    for a in [
        vec!["-H", "pcov", "1:b,b:2"],
        vec!["-H", "count", "a", "sum", "4"],
        vec!["count", "1-512,0"],
        vec!["-H", r"g\b", "a", "count", "b"],
    ] {
        cases.push((args(&a), b"a\tb\n1\t2\n3\t4\n".to_vec()));
    }
    let mut a = Vec::new();
    for _ in 0..1200 {
        a.extend(args(&["count", "1"]));
    }
    cases.push((a, b"a\nb\n".to_vec()));
    let mut failed = Vec::new();
    for (index, (arguments, input)) in cases.iter().enumerate() {
        fs::write(
            evidence.join(format!("{index:03}.args")),
            format!("{arguments:?}"),
        )
        .unwrap();
        fs::write(evidence.join(format!("{index:03}.input")), input).unwrap();
        let expected = invoke(&reference, arguments, input);
        let actual = invoke(&candidate(), arguments, input);
        for (label, output) in [("gnu", &expected), ("candidate", &actual)] {
            fs::write(
                evidence.join(format!("{index:03}.{label}.stdout")),
                &output.stdout,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{index:03}.{label}.stderr")),
                &output.stderr,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{index:03}.{label}.status")),
                output.status.to_string(),
            )
            .unwrap();
        }
        if actual.status != expected.status
            || actual.stdout != expected.stdout
            || actual.stderr != expected.stderr
        {
            failed.push(index);
        }
    }
    assert!(failed.is_empty(), "mismatches: {failed:?}");
}
