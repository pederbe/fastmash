#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;

use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

fn candidate() -> OsString {
    std::env::var_os("FASTMASH_EXTREMA_BINARY")
        .or_else(|| std::env::var_os("FASTMASH_TEST_BINARY"))
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into())
}
fn invoke(binary: &OsStr, args: &[String], input: &[u8], full: bool) -> Output {
    invoke_at(binary, args, input, full, "1048576", None)
}
fn invoke_at(
    binary: &OsStr,
    args: &[String],
    input: &[u8],
    full: bool,
    memory: &str,
    temp: Option<&std::path::Path>,
) -> Output {
    let path = std::env::temp_dir().join(format!(
        "extrema-{}-{:?}.tsv",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let mut command = Command::new(binary);
    command
        .arg0("fastmash")
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .env("FASTMASH_SORT_MEMORY_BYTES", memory)
        .stdin(fs::File::open(&path).unwrap())
        .full_stdout(full)
        .stderr(Stdio::piped());
    if let Some(temp) = temp {
        command.env("TMPDIR", temp);
    }
    let result = command.output().unwrap();
    fs::remove_file(path).unwrap();
    result
}
fn arguments(prefix: &[&str], field: &str) -> Vec<String> {
    prefix
        .iter()
        .map(|s| s.to_string())
        .chain(
            ["min", "max", "absmin", "absmax", "range"]
                .into_iter()
                .flat_map(|op| [op.to_string(), field.to_string()]),
        )
        .collect()
}
#[test]
fn useful_scale_and_wide_records() {
    let input = b"-3.25\n2.5\n0.125\n".repeat(100_000);
    let start = std::time::Instant::now();
    let out = invoke(&candidate(), &arguments(&[], "1"), &input, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"-3.25\t2.5\t0.125\t-3.25\t5.75\n");
    eprintln!("300000 rows, five summaries: {:?}", start.elapsed());
    let input = format!("{}\t-3.25\n{}\t2.5\n", "x".repeat(20000), "x".repeat(20000));
    for prefix in [vec![], vec!["-s", "-g", "1"]] {
        let out = invoke(
            &candidate(),
            &arguments(&prefix, "2"),
            input.as_bytes(),
            false,
        );
        assert!(out.status.success(), "{:?}", out.stderr);
        let expected = if prefix.is_empty() {
            "-3.25\t2.5\t2.5\t-3.25\t5.75\n".to_string()
        } else {
            format!("{}\t-3.25\t2.5\t2.5\t-3.25\t5.75\n", "x".repeat(20000))
        };
        assert_eq!(out.stdout, expected.as_bytes());
    }
}
#[test]
fn output_failure_is_reported() {
    let out = invoke(&candidate(), &arguments(&[], "1"), b"1\n2\n", true);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
    for prefix in [vec![], vec!["-s"]] {
        let out = Command::new(candidate())
            .arg0("fastmash")
            .args(arguments(&prefix, "1"))
            .env_clear()
            .env("LC_ALL", "C")
            .env("PATH", "/usr/bin:/bin")
            .stdin(fs::File::open("/tmp").unwrap())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("Is a directory"));
    }
}

#[test]
fn sorted_spill_preserves_ties_and_reports_temporary_failure() {
    let args = arguments(&["-s", "-g", "1"], "2");
    let input = b"b\t4\na\t-0\nb\t-4\na\t0\nc\t-2\nc\t1\n";
    let out = invoke_at(&candidate(), &args, input, false, "1", None);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        out.stdout,
        b"a\t-0\t-0\t-0\t-0\t0\nb\t-4\t4\t4\t4\t8\nc\t-2\t1\t1\t-2\t3\n"
    );
    let path = std::env::temp_dir().join(format!("extrema-bad-temp-{}", std::process::id()));
    fs::write(&path, b"not a directory").unwrap();
    let out = invoke_at(&candidate(), &args, input, false, "1", Some(&path));
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("sort temporary I/O error"));
    let out = invoke_at(&candidate(), &args, input, false, "1048576", Some(&path));
    assert!(out.status.success(), "{out:?}");
    fs::remove_file(path).unwrap();
}
#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn reference_extrema_boundaries() {
    let reference = std::env::var_os("FASTMASH_EXTREMA_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_EXTREMA_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
    for input in [
        "",
        "1\n",
        "-0\n0\n",
        "0\n-0\n",
        "-4\n4\n-2\n2\n",
        "4\n-4\n2\n-2\n",
        "nan\n1\n-2\n",
        "1\nnan\n-2\n",
        "-nan\n1\n",
        "inf\ninf\n",
        "-inf\n-inf\n",
        "-inf\ninf\n",
        "nan\n-nan\n",
        "NA\nN/A\nNaN\n",
        "1.0000000000000000001\n1\n",
        "0x1.0000000000000002p0\n0x1p0\n",
        "-0x1p-64\n1\n",
        "-0x1p-65\n1\n",
        "-1e4932\n1e4932\n",
        "0x1p-16445\n0x2p-16445\n",
        "1e-5000\n",
        "1e5000\n",
        "1\ninvalid\n",
        "1\n\n",
        "1\t2\n3\n",
        "0x1p0\n-0x1p0\n",
        "-0\n-0\n",
        "2\n1\n2\n",
        "1.234567890123456789\n-9.876543210987654321\n",
    ] {
        for prefix in [vec![], vec!["--narm"]] {
            cases.push((arguments(&prefix, "1"), input.as_bytes().to_vec()));
        }
        // Independent operations expose early output/error sequencing hidden by a combined command.
        for op in ["min", "max", "absmin", "absmax", "range"] {
            cases.push((vec![op.into(), "1".into()], input.as_bytes().to_vec()));
        }
    }
    for prefix in [
        vec!["-g", "1"],
        vec!["-s", "-g", "1"],
        vec!["-s", "-g", "1", "--narm"],
    ] {
        for input in [
            "a\t-0\na\t0\nb\t4\nb\t-4\nc\tNA\n",
            "b\t2\na\t-4\nb\t-2\na\t4\n",
            "a\tnan\na\t1\nb\t1\nb\tnan\n",
            "a\t1\nb\tbad\n",
        ] {
            cases.push((arguments(&prefix, "2"), input.as_bytes().to_vec()));
        }
    }
    for prefix in [
        vec!["-H"],
        vec!["--header-in"],
        vec!["-H", "-s", "-g", "group"],
    ] {
        cases.push((
            arguments(&prefix, "value"),
            b"group\tvalue\na\t-3.25\na\t2.5\nb\t0\n".to_vec(),
        ));
    }
    for (prefix, input) in [
        (vec!["-t|"], "1.5|x\n-2.25|y\n"),
        (vec!["-W"], "1.5 x\n-2.25 y\n"),
        (vec!["-t0"], "10.5\n20.5\n"),
    ] {
        cases.push((arguments(&prefix, "1"), input.as_bytes().to_vec()));
    }
    cases.push((
        arguments(&[], "2"),
        format!("{}\t-3\n{}\t2\n", "x".repeat(20000), "x".repeat(20000)).into_bytes(),
    ));
    cases.push((
        arguments(&[], "1"),
        format!("{}1\n2\n", "0".repeat(20000)).into_bytes(),
    ));
    for value in ["inf\n", "-inf\n", "nan\n", "-nan\n"] {
        cases.push((arguments(&[], "1"), value.as_bytes().to_vec()));
    }
    // A numeric delimiter requires original-record retry semantics.
    cases.push((
        arguments(&["-s", "-g", "1", "-t0"], "2"),
        b"a010.5\na020.5\nb030.5\n".to_vec(),
    ));
    cases.push((
        arguments(&["-s", "-g", "1"], "2"),
        format!("{}\t-3\n{}\t2\n", "x".repeat(20000), "x".repeat(20000)).into_bytes(),
    ));
    for input in [b"-3.25\n2.5\n0.125\n".as_slice(), b"1\nbad\n".as_slice()] {
        cases.push((
            ["range", "1", "pstdev", "1", "absmax", "1", "sum", "1"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            input.to_vec(),
        ));
    }
    let mut failures = Vec::new();
    for (i, (args, input)) in cases.iter().enumerate() {
        fs::write(evidence.join(format!("{i:03}.args")), format!("{args:?}")).unwrap();
        fs::write(evidence.join(format!("{i:03}.input")), input).unwrap();
        let expected = invoke(&reference, args, input, false);
        let actual = invoke(&candidate(), args, input, false);
        for (label, output) in [("gnu", &expected), ("candidate", &actual)] {
            fs::write(
                evidence.join(format!("{i:03}.{label}.stdout")),
                &output.stdout,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{i:03}.{label}.stderr")),
                &output.stderr,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{i:03}.{label}.status")),
                output.status.to_string(),
            )
            .unwrap();
        }
        let portable_invalid = matches!(
            input.as_slice(),
            b"inf\ninf\n" | b"-inf\n-inf\n" | b"inf\n" | b"-inf\n"
        ) && args.iter().any(|arg| arg == "range");
        let stdout = if portable_invalid {
            // Retained portable policy: invalid subtraction creates positive NaN.
            let adapted = String::from_utf8(expected.stdout.clone())
                .unwrap()
                .replace("-nan", "nan")
                .into_bytes();
            fs::write(evidence.join(format!("{i:03}.portable.stdout")), &adapted).unwrap();
            adapted
        } else {
            expected.stdout.clone()
        };
        if expected.status != actual.status
            || stdout != actual.stdout
            || expected.stderr != actual.stderr
        {
            failures.push(i);
        }
    }
    eprintln!("{} reference observations", cases.len());
    assert!(failures.is_empty(), "mismatches: {failures:?}");
}
