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

fn invoke(arguments: &[OsString], input: &[u8], file: bool) -> Output {
    invoke_with(arguments, input, file, &[])
}

fn invoke_with(
    arguments: &[OsString],
    input: &[u8],
    file: bool,
    variables: &[(&str, &str)],
) -> Output {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.envs(variables.iter().copied());
    if file {
        let directory =
            temp_dir::TempDir::new(&format!("result-names-{:?}", std::thread::current().id()));
        let path = directory.0.join("input");
        fs::write(&path, input).unwrap();
        command
            .stdin(fs::File::open(&path).unwrap())
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
        let output = invoke(&arguments, input, file);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}

#[test]
fn result_names_replace_selected_labels_in_text_reports() {
    reports(
        &[
            "--header-out",
            "--result-name=2:average:daily",
            "--result-name",
            "1:total",
            "sum",
            "1",
            "mean",
            "2",
            "count",
            "1",
        ],
        b"1\t2\n3\t4\n",
        b"total\taverage:daily\tcount(field-1)\n4\t3\t2\n",
    );
}

#[test]
fn invalid_result_names_fail_before_emitting_any_header() {
    let invalid = "--result-name requires a positive INDEX:NAME\n";
    for (value, diagnostic) in [
        ("0:x", invalid),
        ("-1:x", invalid),
        ("+1:x", invalid),
        ("1x:x", invalid),
        (" 1:x", invalid),
        (":x", invalid),
        ("1", invalid),
        ("1:", invalid),
        ("18446744073709551616:x", invalid),
        (
            "2:x",
            "--result-name target exceeds the number of results\n",
        ),
        (
            "1:a\tb",
            "--result-name contains the text output delimiter or record terminator\n",
        ),
        (
            "1:a\nb",
            "--result-name contains the text output delimiter or record terminator\n",
        ),
    ] {
        for file in [false, true] {
            let output = invoke(
                &[
                    "--header-out".into(),
                    "--result-name".into(),
                    value.into(),
                    "sum".into(),
                    "1".into(),
                ],
                b"1\n",
                file,
            );
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty(), "{output:?}");
            assert_eq!(output.stderr, format!("fastmash: {diagnostic}").as_bytes());
        }
    }
    for arguments in [
        vec!["--result-name=1:x", "sum", "1"],
        vec!["--header-in", "--result-name=1:x", "sum", "1"],
        vec!["--vnlog", "--result-name=1:x", "sum", "x"],
    ] {
        let output = invoke(
            &arguments.iter().map(OsString::from).collect::<Vec<_>>(),
            b"",
            false,
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            b"fastmash: --result-name requires --header-out or -H\n"
        );
    }
    for (header, input) in [
        ("--header-out", b"".as_slice()),
        ("-H", b"value\n".as_slice()),
    ] {
        let arguments: Vec<OsString> = [header, "--result-name=2:x", "sum", "1"]
            .iter()
            .map(Into::into)
            .collect();
        for file in [false, true] {
            let output = invoke(&arguments, input, file);
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr,
                b"fastmash: --result-name target exceeds the number of results\n"
            );
        }
    }
    let output = invoke(
        &[
            "-H".into(),
            "--result-name=1:x".into(),
            "--result-name=01:y".into(),
            "sum".into(),
            "1".into(),
        ],
        b"header\n1\n",
        false,
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(
        output.stderr,
        b"fastmash: --result-name target is repeated\n"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn result_names_refuse_excluded_modes_before_reading_input() {
    for mode in [
        &["crosstab", "1,2"][..],
        &["transpose"],
        &["check"],
        &["rmdup", "1"],
        &["reverse"],
        &["noop"],
    ] {
        let mut command = Command::new(executable::fastmash());
        command
            .arg0("fastmash")
            .args(["--header-out", "--result-name=1:result"])
            .args(mode)
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
        assert_eq!(output.status.code(), Some(77), "{mode:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            b"fastmash: --result-name is supported only for aggregate and per-row operations\n"
        );
    }
}

#[test]
fn result_names_follow_expanded_results_and_allow_duplicate_values() {
    reports(
        &[
            "-H",
            "--result-name=6:total",
            "--result-name=1:total",
            "--result-name=4:pair",
            "sum",
            "2,1-2",
            "dotprod",
            "1:2",
            "sum",
            "2,1",
        ],
        b"left\tright\n1\t2\n3\t4\n",
        b"total\tsum(left)\tsum(right)\tpair\tsum(right)\ttotal\n6\t4\t6\t14\t6\t4\n",
    );
    reports(
        &[
            "-H",
            "--result-name=3:reading",
            "--result-name=1:copy",
            "cut",
            "2,1-2",
            "round",
            "1",
        ],
        b"left\tright\n1.5\t2.5\n3.5\t4.5\n",
        b"copy\tcut(left)\treading\tround(left)\n2.5\t1.5\t2.5\t2\n4.5\t3.5\t4.5\t4\n",
    );
}

#[test]
fn grouping_keys_and_full_prefixes_do_not_consume_result_name_indices() {
    reports(
        &[
            "-H",
            "--result-name=1:total",
            "-g",
            "key",
            "sum",
            "value",
            "count",
            "value",
        ],
        b"key\tvalue\na\t1\na\t3\nb\t2\n",
        b"GroupBy(key)\ttotal\tcount(value)\na\t4\t2\nb\t2\t1\n",
    );
    reports(
        &["-H", "--full", "--result-name=1:reading", "cut", "2,1"],
        b"key\tvalue\na\t1\nb\t2\n",
        b"key\tvalue\treading\tcut(key)\na\t1\t1\ta\nb\t2\t2\tb\n",
    );
    for file in [false, true] {
        let output = invoke(
            &[
                "-H",
                "--full",
                "--result-name=1:total",
                "-g",
                "key",
                "sum",
                "value",
            ]
            .iter()
            .map(OsString::from)
            .collect::<Vec<_>>(),
            b"key\tvalue\na\t1\na\t3\nb\t2\n",
            file,
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"key\tvalue\ttotal\na\t1\t4\nb\t2\t2\n");
        assert_eq!(output.stderr, b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n");
    }
}

#[test]
fn result_names_reach_sorted_grouping_routes() {
    for (headers, input, operation, selector, expected) in [
        (
            &["--header-out"][..],
            b"b\t4\na\t1\nb\t4\na\t1\n".as_slice(),
            "sum",
            "2",
            b"GroupBy(field-1)\ttotal\na\t2\nb\t8\n".as_slice(),
        ),
        (
            &["-H"][..],
            b"key\tvalue\nb\t4\na\t1\nb\t4\na\t1\n",
            "geomean",
            "value",
            b"GroupBy(key)\ttotal\na\t1\nb\t4\n",
        ),
    ] {
        let mut arguments: Vec<OsString> = headers.iter().map(Into::into).collect();
        arguments.extend(
            [
                "-s",
                "-g",
                if headers == ["-H"] { "key" } else { "1" },
                "--result-name=1:total",
                operation,
                selector,
            ]
            .iter()
            .map(OsString::from),
        );
        for file in [false, true] {
            for variables in [&[][..], &[("FASTMASH_GROUPING", "sort")][..]] {
                let output = invoke_with(&arguments, input, file, variables);
                assert!(output.status.success(), "{variables:?} {output:?}");
                assert_eq!(output.stdout, expected);
                assert!(output.stderr.is_empty(), "{output:?}");
            }
        }
    }
}

#[test]
fn names_preserve_header_timing_and_required_field_validation() {
    reports(&["--header-out", "--result-name=1:x", "sum", "1"], b"", b"");
    reports(
        &["-H", "--result-name=1:x", "sum", "value"],
        b"value\n",
        b"x\n",
    );
    for (arguments, input, stdout, stderr) in [
        (
            &["--header-out", "--result-name=1:x", "sum", "1", "sum", "2"][..],
            b"1\n".as_slice(),
            b"x\t".as_slice(),
            b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\n".as_slice(),
        ),
        (
            &["-H", "--result-name=1:x", "dotprod", "1:2"][..],
            b"one\n",
            b"",
            b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\n",
        ),
        (
            &["-H", "--result-name=1:x", "sum", "absent"][..],
            b"value\n1\n",
            b"",
            b"fastmash: column name 'absent' not found in input file\n",
        ),
        (
            &["--header-out", "--result-name=1:x", "sum", "1"][..],
            b"bad\n",
            b"x\n",
            b"fastmash: invalid numeric value in line 1 field 1: 'bad'\n",
        ),
    ] {
        let arguments: Vec<OsString> = arguments.iter().map(Into::into).collect();
        for file in [false, true] {
            let output = invoke(&arguments, input, file);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert_eq!(output.stdout, stdout);
            assert_eq!(output.stderr, stderr);
        }
    }
}

#[test]
fn naming_uses_effective_text_delimiters_and_preserves_argument_bytes() {
    use std::os::unix::ffi::OsStringExt;
    let arguments = [
        "--header-out".into(),
        "--output-delimiter=|".into(),
        OsString::from_vec(b"--result-name=1:\xff\tname\r\"".to_vec()),
        "sum".into(),
        "1".into(),
    ];
    for file in [false, true] {
        let output = invoke(&arguments, b"2\n3\n", file);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"\xff\tname\r\"\n5\n");
        assert!(output.stderr.is_empty());
    }
    reports(
        &[
            "--header-out",
            "--result-name=1:line\nbreak",
            "-z",
            "sum",
            "1",
        ],
        b"2\0",
        b"line\nbreak\x002\0",
    );
    for arguments in [
        vec!["--header-out", "--result-name=1:with,comma", "-t,"],
        vec!["--header-out", "-t,", "--result-name=1:with,comma"],
        vec![
            "--header-out",
            "--result-name=1:with|bar",
            "--output-delimiter=|",
            "-t,",
        ],
    ] {
        let arguments: Vec<OsString> = arguments
            .into_iter()
            .chain(["sum", "1"])
            .map(Into::into)
            .collect();
        let output = invoke(&arguments, b"1\n", false);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            b"fastmash: --result-name contains the text output delimiter or record terminator\n"
        );
    }
}

#[test]
fn extension_spelling_preserves_gnu_abbreviations_and_ambiguity() {
    reports(
        &["--header-o", "--result-name=1:total", "--r=2", "sum", "1"],
        b"1\n2\n",
        b"total\n3.00\n",
    );
    for spelling in ["--result", "--result-n", "--result-names"] {
        let arguments: Vec<OsString> = ["--header-out", spelling, "1:total", "sum", "1"]
            .iter()
            .map(Into::into)
            .collect();
        let output = invoke(&arguments, b"1\n", false);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, format!("fastmash: unrecognized option '{spelling}'\nTry 'fastmash --help' for more information.\n").as_bytes());
    }
    let output = invoke(&["--h".into()], b"", false);
    assert_eq!(output.stderr, b"fastmash: option '--h' is ambiguous; possibilities: '--header-in' '--header-out' '--headers' '--help'\nTry 'fastmash --help' for more information.\n");
    let output = invoke(
        &["sum".into(), "1".into(), "--result-name".into()],
        b"",
        false,
    );
    assert_eq!(output.stderr, b"fastmash: option '--result-name' requires an argument\nTry 'fastmash --help' for more information.\n");
}
