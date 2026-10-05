use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

#[path = "support/executable.rs"]
mod executable;

#[path = "support/temp_dir.rs"]
mod temp_dir;

fn invoke(arguments: &[OsString], input: &[u8], file: bool, variables: &[(&str, &str)]) -> Output {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .envs(variables.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if file {
        let directory =
            temp_dir::TempDir::new(&format!("csv-output-{:?}", std::thread::current().id()));
        let path = directory.0.join("input");
        fs::write(&path, input).unwrap();
        command
            .stdin(fs::File::open(path).unwrap())
            .output()
            .unwrap()
    } else {
        let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(input);
        child.wait_with_output().unwrap()
    }
}

fn reports(arguments: &[&str], input: &[u8], expected: &[u8]) {
    let arguments: Vec<OsString> = arguments.iter().map(Into::into).collect();
    for file in [false, true] {
        let output = invoke(&arguments, input, file, &[]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}

#[test]
fn csv_output_encodes_results_and_does_not_imply_headers() {
    reports(
        &["--csv-out", "--csv-out", "cut", "1-5"],
        b"a,b\ta\"b\ta\rb\t\t\0\xff\n",
        b"\"a,b\",\"a\"\"b\",\"a\rb\",\"\",\"\"\n",
    );
    reports(
        &["--csv-out", "sum", "1", "count", "1"],
        b"2\n4\n",
        b"6,2\n",
    );
}

#[test]
fn csv_headers_encode_complete_generated_labels_and_custom_names() {
    reports(
        &[
            "--csv-out",
            "-H",
            "--result-name=2:total,\"daily\":\r\nreading",
            "dotprod",
            "1:2",
            "sum",
            "2",
        ],
        b"left,side\tright\"side\n2\t3\n4\t5\n",
        b"\"dotprod(left,side,right\"\"side)\",\"total,\"\"daily\"\":\r\nreading\"\n26,8\n",
    );
}

fn before_input(arguments: &[&str], status: i32, diagnostic: &[u8]) {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: close is async-signal-safe between fork and exec.
    unsafe {
        command.pre_exec(|| {
            libc::close(0);
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(status),
        "{arguments:?}: {output:?}"
    );
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(
        output.stderr,
        [b"fastmash: ", diagnostic].concat(),
        "{arguments:?}"
    );
}

#[test]
fn csv_options_reject_conflicts_and_exclusions_before_input_in_either_order() {
    for flag in [
        "--output-delimiter=,",
        "--output-delim=|",
        "-z",
        "--zero",
        "--vnlog",
    ] {
        for arguments in [
            vec!["--csv-out", flag, "count", "1"],
            vec![flag, "--csv-out", "count", "1"],
        ] {
            let diagnostic: &[u8] = if flag.starts_with("--output") {
                b"CSV output conflicts with --output-delimiter\n".as_slice()
            } else if flag == "--vnlog" {
                b"CSV conflicts with --vnlog\n"
            } else {
                b"CSV conflicts with --zero-terminated\n"
            };
            before_input(&arguments, 1, diagnostic);
        }
    }
    for mode in [
        &["crosstab", "1,2"][..],
        &["transpose"],
        &["check"],
        &["rmdup", "1"],
        &["reverse"],
        &["noop"],
    ] {
        for flag in ["--csv-out", "--csv-in", "--csv"] {
            let mut arguments = vec![flag];
            arguments.extend(mode);
            before_input(
                &arguments,
                77,
                b"CSV is supported only for aggregate and per-row operations\n",
            );
        }
    }
    for flag in ["--csv-in", "--csv"] {
        for option in ["-z", "--vnlog"] {
            let diagnostic = if option == "-z" {
                b"CSV conflicts with --zero-terminated\n".as_slice()
            } else {
                b"CSV conflicts with --vnlog\n"
            };
            before_input(&[flag, option, "count", "1"], 1, diagnostic);
            before_input(&[option, flag, "count", "1"], 1, diagnostic);
        }
    }
    for flag in ["-t,", "--field-sep=,", "-W", "--whitesp", "-C", "--skip-c"] {
        let name = if flag == "-W" || flag == "--whitesp" {
            "whitespace"
        } else if flag == "-C" || flag == "--skip-c" {
            "skip-comments"
        } else {
            "field-separator"
        };
        let diagnostic = format!("CSV input conflicts with --{name}\n");
        before_input(&["--csv-in", flag, "count", "1"], 1, diagnostic.as_bytes());
        before_input(&[flag, "--csv-in", "count", "1"], 1, diagnostic.as_bytes());
    }
    for flag in ["--csv-in=yes", "--csv-out=yes", "--csv=yes"] {
        let name = flag.split('=').next().unwrap();
        let diagnostic = format!(
            "option '{name}' doesn't allow an argument\nTry 'fastmash --help' for more information.\n"
        );
        before_input(&[flag, "count", "1"], 1, diagnostic.as_bytes());
    }
    for flag in ["--csv-in", "--csv-out", "--csv"] {
        let diagnostic = format!("health does not support calculation option '{flag}'\n");
        before_input(&[flag, "health"], 1, diagnostic.as_bytes());
        before_input(&["health", "-H", flag], 1, diagnostic.as_bytes());
    }
    for flag in ["--csv-o", "--csv-i", "--cs"] {
        let diagnostic =
            format!("unrecognized option '{flag}'\nTry 'fastmash --help' for more information.\n");
        before_input(&[flag, "count", "1"], 1, diagnostic.as_bytes());
    }
}

#[test]
fn csv_names_preserve_argument_bytes_and_retain_mapping_errors() {
    use std::os::unix::ffi::OsStringExt;
    for file in [false, true] {
        let args = [
            "--csv-out".into(),
            "--header-out".into(),
            OsString::from_vec(b"--result-name=1:\xff:\"name,\n".to_vec()),
            "sum".into(),
            "1".into(),
        ];
        let output = invoke(&args, b"2\n4\n", file, &[]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"\"\xff:\"\"name,\n\"\n6\n");
        assert!(output.stderr.is_empty());
    }
    for (flags, diagnostic) in [
        (
            &["--result-name=2:x"][..],
            b"--result-name target exceeds the number of results\n".as_slice(),
        ),
        (
            &["--result-name=1:x", "--result-name=01:y"][..],
            b"--result-name target is repeated\n".as_slice(),
        ),
        (
            &["--result-name=0:x"][..],
            b"--result-name requires a positive INDEX:NAME\n".as_slice(),
        ),
        (
            &["--result-name=1:"][..],
            b"--result-name requires a positive INDEX:NAME\n".as_slice(),
        ),
    ] {
        let mut args = vec!["--csv-out", "--header-out"];
        args.extend(flags);
        args.extend(["sum", "1"]);
        before_input(&args, 1, diagnostic);
    }
    before_input(
        &["--csv-out", "--result-name=1:x", "sum", "1"],
        1,
        b"--result-name requires --header-out or -H\n",
    );
    let args = [
        "--csv-out".into(),
        OsString::from_vec(b"--output-delimiter=\xff".to_vec()),
        "count".into(),
        "1".into(),
    ];
    let output = invoke(&args, b"1\n", false, &[]);
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"fastmash: CSV output conflicts with --output-delimiter\n"
    );
    assert!(output.stdout.is_empty());
    reports(
        &["--csv-out", "--header-o", "--r=2", "sum", "1"],
        b"1\n2\n",
        b"sum(field-1)\n3.00\n",
    );
}

#[test]
fn csv_grouping_and_full_rows_keep_logical_fields_and_raw_bytes() {
    reports(
        &["--csv-out", "-H", "-g", "1", "sum", "2"],
        b"key,\"name\tvalue\na,\"b\t2\na,\"b\t3\nc\0\xff\t4\n",
        b"\"GroupBy(key,\"\"name)\",sum(value)\n\"a,\"\"b\",5\nc\0\xff,4\n",
    );
    reports(
        &["--csv-out", "-H", "--full", "cut", "2"],
        b"key,\"name\tvalue\n\xef\xbb\xbfa,\"b\t\n\0\xff\tx\ty,z\n",
        b"\"key,\"\"name\",value,cut(value)\n\"\xef\xbb\xbfa,\"\"b\",\"\",\"\"\n\0\xff,x,\"y,z\",x\n",
    );
    reports(
        &["--csv-out", "--full", "cut", "1"],
        b"\xef\xbb\xbfvalue\n",
        b"\xef\xbb\xbfvalue,\xef\xbb\xbfvalue\n",
    );
    reports(&["--csv-out", "-t\n", "cut", "1,1"], b"x\n", b"x,x\n");
    reports(
        &["--csv-out", "-WC", "cut", "2"],
        b"# comment\na b,c\n",
        b"\"b,c\"\n",
    );
    let args: Vec<OsString> = [
        "--csv-out",
        "-H",
        "--full",
        "--result-name=1:total",
        "-g",
        "1",
        "sum",
        "2",
    ]
    .iter()
    .map(Into::into)
    .collect();
    for file in [false, true] {
        let output = invoke(&args, b"key\tvalue\na,\"b\t1\na,\"b\t3\nc\t2\n", file, &[]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"key,value,total\n\"a,\"\"b\",1,4\nc,2,2\n");
        assert_eq!(output.stderr, b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n");
    }
}

#[test]
fn csv_list_results_and_decimal_comma_presentation_stay_in_one_field() {
    reports(
        &["--csv-out", "--header-out", "collapse", "1", "unique", "1"],
        b"b\na\nb\n",
        b"collapse(field-1),unique(field-1)\n\"b,a,b\",\"a,b\"\n",
    );
    reports(
        &["--csv-out", "-c|", "collapse", "1", "unique", "1"],
        b"b\na\n",
        b"b|a,a|b\n",
    );
    for file in [false, true] {
        let arguments: Vec<OsString> = ["--csv-out", "-H", "--round=2", "mean", "1", "sum", "1"]
            .iter()
            .map(Into::into)
            .collect();
        let output = invoke(
            &arguments,
            b"value\n1,5\n2,5\n",
            file,
            &[("LC_ALL", "de_DE.UTF-8")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"mean(value),sum(value)\n\"2,00\",\"4,00\"\n"
        );
        assert!(output.stderr.is_empty());
    }
    reports(
        &[
            "--csv-out",
            "--header-out",
            "trimmean:0.2",
            "1",
            "perc:50",
            "1",
        ],
        b"1\n3\n",
        b"trimmean:0.2(field-1),perc:50(field-1)\n2,2\n",
    );
}

#[test]
fn csv_sorted_text_reports_reach_existing_grouping_routes() {
    for (headers, operation, expected) in [
        (
            &["--header-out"][..],
            "sum",
            b"GroupBy(field-1),sum(field-2)\n\"a,\"\"b\",2\n\"c\r\",8\n".as_slice(),
        ),
        (
            &["-H"][..],
            "geomean",
            b"GroupBy(key),geomean(value)\n\"a,\"\"b\",1\n\"c\r\",4\n".as_slice(),
        ),
    ] {
        let mut arguments: Vec<OsString> = headers.iter().map(Into::into).collect();
        arguments.extend(
            ["--csv-out", "-s", "-g", "1", operation, "2"]
                .iter()
                .map(OsString::from),
        );
        let input = if headers == ["-H"] {
            b"key\tvalue\nc\r\t4\na,\"b\t1\nc\r\t4\na,\"b\t1\n".as_slice()
        } else {
            b"c\r\t4\na,\"b\t1\nc\r\t4\na,\"b\t1\n".as_slice()
        };
        for file in [false, true] {
            for variables in [&[][..], &[("FASTMASH_GROUPING", "sort")][..]] {
                let output = invoke(&arguments, input, file, variables);
                assert!(output.status.success(), "{variables:?}: {output:?}");
                assert_eq!(output.stdout, expected);
                assert!(output.stderr.is_empty());
            }
        }
    }
}

#[test]
fn text_full_prefixes_keep_their_fields_through_raw_and_sorted_grouping() {
    let input = b"key\tvalue\tnote\na,\"b\t3\tfirst\na,\"b\t1\t\xff\0raw\nc\t2\tlast\n";
    let expected = b"key,value,note,min(value)\n\"a,\"\"b\",1,\xff\0raw,1\nc,2,last,2\n";
    for sorted in [false, true] {
        let mut args = vec!["--csv-out", "-H", "--full", "-g", "1", "min", "2"];
        if sorted {
            args.insert(0, "-s");
        }
        let args: Vec<OsString> = args.into_iter().map(Into::into).collect();
        for file in [false, true] {
            for variables in [
                &[][..],
                &[
                    ("FASTMASH_GROUPING", "sort"),
                    ("FASTMASH_SORT_MEMORY_BYTES", "1"),
                ][..],
            ] {
                let output = invoke(&args, input, file, variables);
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, expected);
                assert_eq!(output.stderr, b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n");
            }
        }
    }
}

#[test]
fn csv_headers_keep_empty_input_and_partial_error_timing() {
    reports(&["--csv-out", "--header-out", "sum", "1"], b"", b"");
    reports(
        &["--csv-out", "-H", "sum", "value"],
        b"value\n",
        b"sum(value)\n",
    );
    reports(
        &["--csv-out", "-H", "--full", "cut", "1"],
        b"\t\n",
        b"\"\",\"\",cut()\n",
    );
    for (arguments, input, stdout, stderr) in [
        (
            &["--csv-out", "cut", "1"][..],
            b"\n".as_slice(),
            b"".as_slice(),
            b"fastmash: invalid input: field 1 requested, line 1 has only 0 fields\n".as_slice(),
        ),
        (
            &["--csv-out", "--header-out", "sum", "1", "sum", "2"][..],
            b"1\n".as_slice(),
            b"sum(field-1),".as_slice(),
            b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\n".as_slice(),
        ),
        (
            &["--csv-out", "-H", "--result-name=1:x,y", "dotprod", "1:2"],
            b"one\n",
            b"",
            b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\n",
        ),
        (
            &["--csv-out", "--header-out", "sum", "1"],
            b"bad\n",
            b"sum(field-1)\n",
            b"fastmash: invalid numeric value in line 1 field 1: 'bad'\n",
        ),
        (
            &["--csv-out", "-H", "sum", "absent"],
            b"value\n1\n",
            b"",
            b"fastmash: column name 'absent' not found in input file\n",
        ),
    ] {
        let arguments: Vec<OsString> = arguments.iter().map(Into::into).collect();
        for file in [false, true] {
            let output = invoke(&arguments, input, file, &[]);
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(output.stdout, stdout);
            assert_eq!(output.stderr, stderr);
        }
    }
}

#[test]
fn established_byte_reader_recovers_canonical_csv_fields() {
    let args: Vec<OsString> = [
        "--csv-out",
        "-H",
        "--full",
        "--result-name=2:line\r\nbreak",
        "cut",
        "1,2",
        "debase64",
        "3",
    ]
    .iter()
    .map(Into::into)
    .collect();
    let output = invoke(
        &args,
        b"key\tvalue\tencoded\na,\"b\t\xff\tYQ0KYg==\n",
        false,
        &[],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"key,value,encoded,cut(key),\"line\r\nbreak\",debase64(encoded)\n\"a,\"\"b\",\xff,YQ0KYg==,\"a,\"\"b\",\xff,\"a\r\nb\"\n");
    let mut reader = csv_core::Reader::new();
    let mut remaining = output.stdout.as_slice();
    let mut buffer = vec![0; output.stdout.len()];
    let mut fields = Vec::new();
    let mut rows = Vec::new();
    loop {
        let (result, consumed, written) = reader.read_field(remaining, &mut buffer);
        remaining = &remaining[consumed..];
        match result {
            csv_core::ReadFieldResult::Field { record_end } => {
                fields.push(buffer[..written].to_vec());
                if record_end {
                    rows.push(std::mem::take(&mut fields));
                }
            }
            csv_core::ReadFieldResult::End => break,
            other => panic!("complete small input should produce a field: {other:?}"),
        }
    }
    assert_eq!(
        rows,
        vec![
            vec![
                b"key".to_vec(),
                b"value".to_vec(),
                b"encoded".to_vec(),
                b"cut(key)".to_vec(),
                b"line\r\nbreak".to_vec(),
                b"debase64(encoded)".to_vec()
            ],
            vec![
                b"a,\"b".to_vec(),
                b"\xff".to_vec(),
                b"YQ0KYg==".to_vec(),
                b"a,\"b".to_vec(),
                b"\xff".to_vec(),
                b"a\r\nb".to_vec()
            ],
        ]
    );
}

#[test]
fn a_single_empty_csv_result_is_an_explicit_record() {
    reports(&["--csv-out", "debase64", "1"], b"AA==\n", b"\"\"\n");
    let args = ["--csv-out".into(), "debase64".into(), "1".into()];
    let output = invoke(&args, b"AA==\n", false, &[]);
    assert!(output.status.success());
    let mut reader = csv_core::Reader::new();
    let (result, consumed, written) = reader.read_field(&output.stdout, &mut [0; 1]);
    assert_eq!(
        result,
        csv_core::ReadFieldResult::Field { record_end: true }
    );
    assert_eq!((consumed, written), (3, 0));
}
