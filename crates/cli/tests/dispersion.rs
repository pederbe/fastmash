use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

fn candidate() -> OsString {
    std::env::var_os("FASTMASH_DISPERSION_BINARY")
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
        "dispersion-{}-{:?}.tsv",
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
        .stdout(if full {
            Stdio::from(
                fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .unwrap(),
            )
        } else {
            Stdio::piped()
        })
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
            ["pvar", "svar", "pstdev", "sstdev"]
                .into_iter()
                .flat_map(|op| [op.to_string(), field.to_string()]),
        )
        .collect()
}
#[test]
fn exact_translated_measurements_and_useful_scale() {
    // Translation does not change these exact representable differences.
    for input in [
        b"0\n2\n".as_slice(),
        b"1000000000000000000\n1000000000000000002\n".as_slice(),
    ] {
        let out = invoke(&candidate(), &arguments(&[], "1"), input, false);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, b"1\t2\t1\t1.4142135623731\n");
    }
    // Ideal variance is 1; binary80 ordered mean rounds a half-ulp at 2^64,
    // so the GNU-compatible two-pass result is population variance 2.
    let out = invoke(
        &candidate(),
        &arguments(&[], "1"),
        b"18446744073709551616\n18446744073709551618\n",
        false,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"2\t4\t1.4142135623731\t2\n");
    let input = b"0\n2\n".repeat(500_000);
    let args = ["pvar", "1", "pstdev", "1", "pvar", "1"].map(str::to_string);
    let start = std::time::Instant::now();
    let out = invoke(&candidate(), &args, &input, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"1\t1\t1\n");
    eprintln!(
        "million rows, shared variance/stddev: {:?}",
        start.elapsed()
    );
    let wide = format!("{}\t0\n{}\t2\n", "x".repeat(20000), "x".repeat(20000));
    let out = invoke(&candidate(), &arguments(&[], "2"), wide.as_bytes(), false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"1\t2\t1\t1.4142135623731\n");
}
#[test]
fn grouped_spill_and_io_failures() {
    let args = arguments(&["--narm", "-s", "-g", "1"], "2");
    let input = b"b\t0\na\t4\nb\t2\na\t4\nc\tNA\nd\t1\n";
    let out = invoke_at(&candidate(), &args, input, false, "1", None);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        out.stdout,
        b"a\t0\t0\t0\t0\nb\t1\t2\t1\t1.4142135623731\nc\tnan\tnan\tnan\tnan\nd\t0\tnan\t0\tnan\n"
    );
    let bad = std::env::temp_dir().join(format!("dispersion-bad-temp-{}", std::process::id()));
    fs::write(&bad, b"not a directory").unwrap();
    let out = invoke_at(&candidate(), &args, input, false, "1", Some(&bad));
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("sort temporary I/O error"));
    fs::remove_file(bad).unwrap();
    let out = invoke(&candidate(), &arguments(&[], "1"), b"0\n2\n", true);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
}
#[test]
#[ignore = "requires named GNU reference, FASTMASH_WINE_CSV and fresh evidence directory"]
fn named_reference_dispersion() {
    let reference = std::env::var_os("FASTMASH_DISPERSION_REFERENCE").unwrap();
    let evidence =
        std::path::PathBuf::from(std::env::var_os("FASTMASH_DISPERSION_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases: Vec<(Vec<String>, Vec<u8>)> = Vec::new();
    for input in [
        "",
        "1\n",
        "-0\n0\n",
        "0\n2\n",
        "4\n4\n4\n",
        "NA\nN/A\nnan\n",
        "-nan\n1\n",
        "1\nnan\n",
        "inf\n1\n",
        "-inf\ninf\n",
        "1e4932\n1e4932\n",
        "-1e2500\n1e2500\n",
        "1e-2400\n2e-2400\n",
        "0\n0x1p-8222\n",
        "0\n0x1p-8223\n",
        "0\n0x1p-100\n",
        "1000000000000000000\n1000000000000000002\n",
        "1e20\n1\n-1e20\n",
        "1e20\n-1e20\n1\n",
        "1.0000000000000000001\n1\n",
        "1e-5000\n",
        "1\nbad\n",
        "1\n\n",
        "1\t2\n3\n",
    ] {
        for prefix in [vec![], vec!["--narm"]] {
            cases.push((arguments(&prefix, "1"), input.as_bytes().to_vec()));
        }
        for op in ["pvar", "svar", "pstdev", "sstdev"] {
            cases.push((vec![op.into(), "1".into()], input.as_bytes().to_vec()));
        }
    }
    for prefix in [
        vec!["-g", "1"],
        vec!["-s", "-g", "1"],
        vec!["--narm", "-s", "-g", "1"],
    ] {
        for input in [
            "a\t0\na\t2\nb\t4\nb\t4\nc\tNA\n",
            "b\t2\na\t3\nb\t0\na\t1\n",
            "a\t1\nb\tbad\n",
        ] {
            cases.push((arguments(&prefix, "2"), input.as_bytes().to_vec()));
        }
    }
    for prefix in [vec!["-H"], vec!["-H", "-s", "-g", "group"]] {
        cases.push((
            arguments(&prefix, "value"),
            b"group\tvalue\nb\t2\na\t4\nb\t0\na\t4\n".to_vec(),
        ));
    }
    for operations in [
        vec![
            "pvar", "1", "median", "1", "svar", "1", "q1", "1", "pstdev", "1", "sum", "1",
        ],
        vec![
            "median", "1", "pvar", "1", "pstdev", "1", "sstdev", "1", "pvar", "1",
        ],
        vec!["svar", "1", "pvar", "1", "sstdev", "1", "pstdev", "1"],
    ] {
        cases.push((
            operations.into_iter().map(str::to_string).collect(),
            b"2\n0\n4\n".to_vec(),
        ));
    }
    cases.push((arguments(&[], "1"), b"0\n2\n".repeat(50000)));
    cases.push((
        arguments(&["-s", "-g", "1", "-t0"], "2"),
        b"a010.5\na020.5\nb030.5\n".to_vec(),
    ));
    cases.push((
        arguments(&[], "1"),
        b"18446744073709551616\n18446744073709551618\n".to_vec(),
    ));
    cases.push((
        ["pvar", "1", "geomean", "1", "pstdev", "1"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        b"1\n4\n".to_vec(),
    ));
    let wine = fs::read(
        std::env::var_os("FASTMASH_WINE_CSV")
            .expect("FASTMASH_WINE_CSV names winequality-white.csv"),
    )
    .unwrap();
    for prefix in [vec!["-H", "-t;"], vec!["-H", "-t;", "-s", "-g", "12"]] {
        cases.push((arguments(&prefix, "9"), wine.clone()));
    }
    let groups = (0..1000)
        .rev()
        .map(|n| format!("g{n:04}\t0\ng{n:04}\t2\n"))
        .collect::<String>();
    cases.push((arguments(&["-s", "-g", "1"], "2"), groups.into_bytes()));
    let mut failed = Vec::new();
    let mut exact = 0;
    let mut portable = 0;
    for (i, (args, input)) in cases.iter().enumerate() {
        fs::write(evidence.join(format!("{i:03}.args")), format!("{args:?}")).unwrap();
        fs::write(evidence.join(format!("{i:03}.input")), input).unwrap();
        let expected = invoke(&reference, args, input, false);
        let actual = invoke(&candidate(), args, input, false);
        for (label, out) in [("gnu", &expected), ("candidate", &actual)] {
            fs::write(evidence.join(format!("{i:03}.{label}.stdout")), &out.stdout).unwrap();
            fs::write(evidence.join(format!("{i:03}.{label}.stderr")), &out.stderr).unwrap();
            fs::write(
                evidence.join(format!("{i:03}.{label}.status")),
                out.status.to_string(),
            )
            .unwrap();
        }
        let same = actual.status == expected.status && actual.stderr == expected.stderr;
        if same && actual.stdout == expected.stdout {
            exact += 1;
        } else if same
            && input.windows(3).any(|x| x == b"inf")
            && actual.stdout
                == String::from_utf8(expected.stdout.clone())
                    .unwrap()
                    .replace("-nan", "nan")
                    .as_bytes()
        {
            portable += 1;
            fs::write(
                evidence.join(format!("{i:03}.portable-policy")),
                b"Existing invalid-operation positive NaN policy\n",
            )
            .unwrap();
        } else {
            failed.push(i);
        }
    }
    eprintln!(
        "{} observations: {exact} exact, {portable} retained NaN policy",
        cases.len()
    );
    assert!(failed.is_empty(), "mismatches: {failed:?}");
}

#[test]
#[ignore = "requires installed baseline, named GNU reference, FASTMASH_WINE_CSV and fresh timing directory"]
fn timed_scientific_jobs() {
    let reference = std::env::var_os("FASTMASH_DISPERSION_REFERENCE").unwrap();
    let baseline = std::env::var_os("FASTMASH_DISPERSION_BASELINE").unwrap();
    let dir = std::path::PathBuf::from(std::env::var_os("FASTMASH_DISPERSION_TIMINGS").unwrap());
    fs::create_dir(&dir).unwrap();
    let wine = fs::read(
        std::env::var_os("FASTMASH_WINE_CSV")
            .expect("FASTMASH_WINE_CSV names winequality-white.csv"),
    )
    .unwrap();
    for (job, args, input) in [
        ("tiny", arguments(&[], "1"), b"0\n2\n".to_vec()),
        ("wine", arguments(&["-H", "-t;"], "9"), wine),
    ] {
        let expected = invoke(&reference, &args, &input, false);
        assert!(expected.status.success());
        fs::write(dir.join(format!("{job}.args")), format!("{args:?}")).unwrap();
        fs::write(dir.join(format!("{job}.input")), &input).unwrap();
        for round in 0..6 {
            let products = [
                ("gnu", &reference),
                ("baseline", &baseline),
                ("candidate", &candidate()),
            ];
            for index in 0..3 {
                let (name, binary) = products[(index + round) % 3];
                let start = std::time::Instant::now();
                let actual = invoke(binary, &args, &input, false);
                let elapsed = start.elapsed().as_nanos();
                let stem = format!("{job}.{round}.{name}");
                fs::write(dir.join(format!("{stem}.ns")), elapsed.to_string()).unwrap();
                fs::write(dir.join(format!("{stem}.stdout")), &actual.stdout).unwrap();
                fs::write(dir.join(format!("{stem}.stderr")), &actual.stderr).unwrap();
                fs::write(
                    dir.join(format!("{stem}.status")),
                    actual.status.to_string(),
                )
                .unwrap();
                assert_eq!(actual.status, expected.status, "{stem}");
                assert_eq!(actual.stdout, expected.stdout, "{stem}");
                assert_eq!(actual.stderr, expected.stderr, "{stem}");
            }
        }
    }
}
