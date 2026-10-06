use std::{
    fs,
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

#[path = "support/executable.rs"]
mod executable;

#[path = "support/temp_dir.rs"]
mod temp_dir;

fn invoke(
    arguments: &[impl AsRef<std::ffi::OsStr>],
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
        .envs(variables.iter().copied())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if file {
        let directory =
            temp_dir::TempDir::new(&format!("csv-input-{:?}", std::thread::current().id()));
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
    for file in [false, true] {
        for target in ["67108864", "1"] {
            let output = invoke(
                arguments,
                input,
                file,
                &[("FASTMASH_SORT_MEMORY_BYTES", target)],
            );
            assert!(output.status.success(), "{arguments:?}: {output:?}");
            assert_eq!(output.stdout, expected);
            assert!(output.stderr.is_empty(), "{output:?}");
        }
    }
}

#[test]
fn csv_sorted_groups_compare_decoded_keys_and_keep_stable_records() {
    reports(
        &["--csv", "-H", "-s", "-g", "region\\,name", "--result-name=1:total", "sum", "value", "first", "note", "last", "note"],
        b"\"region,name\",value,note\nb,2,b1\n\"a\",3,\"a,first\"\n\"b\",4,b2\na,5,\"a\nlast\"\n",
        b"\"GroupBy(region,name)\",total,first(note),last(note)\na,8,\"a,first\",\"a\nlast\"\nb,6,b1,b2\n",
    );
}

#[test]
fn csv_calculations_decode_headers_and_multiline_unused_fields() {
    reports(
        &[
            "--csv",
            "-H",
            "--result-name=1:total,\"daily\"",
            "sum",
            "amount\\,raw",
        ],
        b"\"amount,raw\",note\r\n\"2\",\"two\nlines\"\n4,\"a\"\"b\"",
        b"\"total,\"\"daily\"\"\"\n6\n",
    );
}

#[test]
fn csv_empty_records_and_final_fields_remain_distinct_from_absent_fields() {
    for (input, expected) in [
        (b"".as_slice(), b"".as_slice()),
        (b"\n", b"1\n"),
        (b"\r\n", b"1\n"),
        (b"\"\"", b"1\n"),
        (b"\n\r\n\"\"\n", b"3\n"),
        (b"a,\n", b"1\n"),
        (b"a,", b"1\n"),
    ] {
        reports(&["--csv", "count", "1"], input, expected);
    }
    reports(&["--csv", "cut", "2"], b"a,\n\"a\",\"\"", b"\"\"\n\"\"\n");
    reports(&["--csv", "-H", "cut", "2"], b"a,", b"cut()\n");
    reports(&["--csv", "-H", "sum", "missing"], b"", b"");
}

#[test]
fn csv_bytes_spaces_and_internal_endings_survive_decoding_and_encoding() {
    reports(
        &["--csv", "cut", "1-2"],
        b"\" a,\t\"\"b\r\nc\n\xff \",tail\r\n\"\",\"x\r y\"\n",
        b"\" a,\t\"\"b\r\nc\n\xff \",tail\n\"\",\"x\r y\"\n",
    );
    // cut retains its existing NUL display rule; base64 consumes every byte.
    reports(
        &["--csv", "cut", "1", "base64", "1"],
        b"\"a\0b\",unused\n",
        b"a,YQBi\n",
    );
    reports(
        &["--csv", "-H", "cut", "1"],
        b"\xef\xbb\xbfname\nvalue\n",
        b"cut(\xef\xbb\xbfname)\nvalue\n",
    );
    use std::os::unix::ffi::OsStringExt;
    for file in [false, true] {
        let args = [
            "--csv".into(),
            "-H".into(),
            "cut".into(),
            std::ffi::OsString::from_vec(b"\\\xef\\\xbb\\\xbfname".to_vec()),
        ];
        let output = invoke(&args, b"\xef\xbb\xbfname\nvalue\n", file, &[]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"cut(\xef\xbb\xbfname)\nvalue\n");
        assert!(output.stderr.is_empty());
    }
    reports(
        &["--csv", "-H", "sum", "1"],
        b"\"head\nline\",unused\n2,x\n",
        b"\"sum(head\nline)\"\n2\n",
    );
}

#[test]
fn csv_input_only_keeps_text_output_and_flags_are_idempotent() {
    reports(
        &["--csv-in", "cut", "1-2"],
        b"\"a,b\",\"x\ty\"\n",
        b"a,b\tx\ty\n",
    );
    reports(
        &["--csv-in", "--output-delimiter=|", "cut", "1-2"],
        b"\"a,b\",\"x|y\"\n",
        b"a,b|x|y\n",
    );
    for args in [
        &["--csv", "--csv", "sum", "1"][..],
        &["--csv-in", "--csv-in", "--csv-out", "sum", "1"],
        &["--csv-out", "--csv-in", "--csv", "sum", "1"],
    ] {
        reports(args, b"\"2\",unused\r\n4,x", b"6\n");
    }
}

fn errors(args: &[&str], input: &[u8], message: &[u8]) {
    for file in [false, true] {
        let output = invoke(args, input, file, &[]);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(output.stderr, [b"fastmash: ", message].concat());
    }
}

#[test]
fn csv_syntax_errors_validate_unused_fields_and_use_original_byte_positions() {
    for (input, diagnostic) in [
        (b"1,a\"b\n".as_slice(), b"invalid CSV: quote in unquoted field at physical line 1 byte 4; record 1 starts at physical line 1\n".as_slice()),
        (b"1,\"a\" x\n", b"invalid CSV: byte after closing quote at physical line 1 byte 6; record 1 starts at physical line 1\n"),
        (b"1,a\rb\n", b"invalid CSV: bare CR outside quoted field at physical line 1 byte 4; record 1 starts at physical line 1\n"),
        (b"1,a\r", b"invalid CSV: bare CR outside quoted field at physical line 1 byte 4; record 1 starts at physical line 1\n"),
        (b"1,\"unfinished", b"invalid CSV: unexpected end of input in quoted field; record 1 starts at physical line 1\n"),
        (b"1,\"two\nlines\"\r\n2,\"x\ny\"!", b"invalid CSV: byte after closing quote at physical line 4 byte 3; record 2 starts at physical line 3\n"),
        (b"\xef\xbb\xbf\"head\"\n2\n", b"invalid CSV: quote in unquoted field at physical line 1 byte 4; record 1 starts at physical line 1\n"),
    ] {
        errors(&["--csv", "sum", "1"], input, diagnostic);
    }
}

#[test]
fn csv_validation_and_header_errors_include_logical_record_and_start_line() {
    errors(
        &["--csv", "-H", "sum", "missing"],
        b"\"a\nheader\",b\n",
        b"column name 'missing' not found in input file\nCSV record 1 starts at physical line 1\n",
    );
    errors(
        &["--csv", "-H", "sum", "1"],
        b"\"a\nheader\",b\r\n\"bad\",x\n",
        b"invalid numeric value in line 2 field 1: 'bad'\nCSV record 2 starts at physical line 3\n",
    );
    errors(&["--csv", "sum", "2"], b"1,2\r\n\"three\nlines\nlong\"\n",
        b"invalid input: field 2 requested, line 2 has only 1 fields\nCSV record 2 starts at physical line 2\n");
    errors(&["--csv", "--header-out", "--result-name=1:alias", "sum", "2"], b"1\n",
        b"invalid input: field 2 requested, line 1 has only 1 fields\nCSV record 1 starts at physical line 1\n");
    errors(
        &["--csv", "sum", "1"],
        b"\"\"\n",
        b"invalid numeric value in line 1 field 1: ''\nCSV record 1 starts at physical line 1\n",
    );
    errors(
        &["--csv", "-H", "cut", "name"],
        b"\xef\xbb\xbfname\nvalue\n",
        b"column name 'name' not found in input file\nCSV record 1 starts at physical line 1\n",
    );
    errors(
        &["--csv", "debase64", "1"],
        b"\"YSxi\",\"two\nlines\"\n\"!\",unused\n",
        b"invalid base64 value in line 2 field 1: '!'\nCSV record 2 starts at physical line 3\n",
    );
}

#[test]
fn csv_numeric_conversion_is_isolated_and_missing_sides_compact_independently() {
    reports(
        &["--csv", "sum", "1"],
        b"1,1e999999999\n\"2\",malformed-number\n",
        b"3\n",
    );
    // Conversion cannot read the contiguous decoded neighbor or a framing NUL.
    reports(
        &["--csv", "sum", "1"],
        b"\"1\",e999999999\n\"2\",.5\n",
        b"3\n",
    );
    reports(
        &["--csv", "--narm", "count", "1", "sum", "1", "mean", "1"],
        b"NA,x\n\"N/A\",y\nNaN,z\n",
        b"0,0,nan\n",
    );
    reports(
        &["--csv", "--narm", "dotprod", "1:2"],
        b"1,NA\nNA,2\n3,4\n",
        b"14\n",
    );
    reports(&["--csv", "--narm", "dotprod", "1:2"], b"1,NA\n", b"nan\n");
    errors(
        &["--csv", "--narm", "sum", "1"],
        b"\" NA\"\n",
        b"invalid numeric value in line 1 field 1: ' NA'\nCSV record 1 starts at physical line 1\n",
    );
}

#[test]
fn csv_all_aggregate_families_match_unambiguous_text_calculations() {
    let operations = [
        "count",
        "countunique",
        "unique",
        "collapse",
        "first",
        "last",
        "rand",
        "min",
        "max",
        "absmin",
        "absmax",
        "range",
        "sum",
        "mean",
        "geomean",
        "harmmean",
        "ms",
        "rms",
        "median",
        "mode",
        "antimode",
        "q1",
        "q3",
        "iqr",
        "perc",
        "perc:100",
        "trimmean",
        "trimmean:0.25",
        "pvar",
        "svar",
        "pstdev",
        "sstdev",
        "madraw",
        "mad",
        "pskew",
        "sskew",
        "pkurt",
        "skurt",
        "jarque",
        "dpo",
        "pcov",
        "scov",
        "ppearson",
        "spearson",
        "dotprod",
    ];
    let csv = b"\"1\",\"2\"\r\n2,4\n\"3\",6\n4,8\n5,10\n6,12\n7,14\n8,16\n";
    let text = b"1\t2\n2\t4\n3\t6\n4\t8\n5\t10\n6\t12\n7\t14\n8\t16\n";
    for file in [false, true] {
        for operation in operations {
            let fields = if matches!(
                operation,
                "pcov" | "scov" | "ppearson" | "spearson" | "dotprod"
            ) {
                "1:2"
            } else {
                "2,1-2"
            };
            for flags in [
                &[][..],
                &["--full"],
                &["-g", "3"],
                &["--full", "-g", "3"],
                &["-s"],
                &["-s", "--full"],
                &["-s", "-g", "3"],
                &["-s", "--full", "-g", "3"],
            ] {
                let mut text_input = Vec::new();
                let mut csv_input = Vec::new();
                for key in [b"a".as_slice(), b"b", b"a"] {
                    for record in text
                        .split(|&byte| byte == b'\n')
                        .filter(|record| !record.is_empty())
                    {
                        text_input.extend_from_slice(record);
                        text_input.extend_from_slice(b"\t");
                        text_input.extend_from_slice(key);
                        text_input.push(b'\n');
                    }
                    for record in csv
                        .split(|&byte| byte == b'\n')
                        .filter(|record| !record.is_empty())
                    {
                        let record = record.strip_suffix(b"\r").unwrap_or(record);
                        csv_input.extend_from_slice(record);
                        csv_input.push(b',');
                        csv_input.extend_from_slice(key);
                        csv_input.push(b'\n');
                    }
                }
                let mut args = vec!["--seed=13"];
                args.extend(flags);
                args.extend([operation, fields]);
                let expected = invoke(&args, &text_input, file, &[]);
                assert!(expected.status.success(), "{operation}: {expected:?}");
                args.insert(0, "--csv-in");
                let actual = invoke(
                    &args,
                    &csv_input,
                    file,
                    &[("FASTMASH_SORT_MEMORY_BYTES", "1")],
                );
                assert_eq!(
                    actual.status.code(),
                    expected.status.code(),
                    "{operation}: {actual:?}"
                );
                assert_eq!(actual.stdout, expected.stdout, "{operation}, {flags:?}");
                assert_eq!(actual.stderr, expected.stderr, "{operation}, {flags:?}");
            }
        }
    }
    reports(
        &["--csv", "-H", "sum", "b,a,b", "dotprod", "a:b,b:a"],
        b"a,b\n1,2\n3,4\n",
        b"sum(b),sum(a),sum(b),\"dotprod(a,b)\",\"dotprod(b,a)\"\n6,4,6,14,14\n",
    );
    reports(
        &["--csv", "-H", "sum", "a"],
        b"a,a\n1,9\n2,8\n",
        b"sum(a)\n3\n",
    );
}

#[test]
fn csv_per_row_families_share_decoded_fields_and_retain_separate_records() {
    for operations in [
        &["cut", "2,1-2"][..],
        &[
            "round", "1", "floor", "1", "ceil", "1", "trunc", "1", "frac", "1", "bin:2", "1",
        ],
        &[
            "getnum:n", "2", "getnum:i", "2", "getnum:h", "2", "getnum:o", "2", "getnum:p", "2",
            "getnum:d", "2",
        ],
        &[
            "strbin:17",
            "2",
            "base64",
            "2",
            "md5",
            "2",
            "sha1",
            "2",
            "sha224",
            "2",
            "sha256",
            "2",
            "sha384",
            "2",
            "sha512",
            "2",
        ],
        &[
            "dirname", "2", "basename", "2", "extname", "2", "barename", "2",
        ],
    ] {
        for (file, full, sort) in [
            (false, false, false),
            (true, false, false),
            (false, true, false),
            (true, true, false),
            (false, false, true),
            (true, false, true),
            (false, true, true),
            (true, true, true),
        ] {
            let mut args = Vec::new();
            if sort {
                args.push("-s");
            }
            if full {
                args.push("--full");
            }
            args.extend(operations);
            let expected = invoke(&args, b"2.5\t/a/file.txt\n-3.25\t/b/other.gz\n", file, &[]);
            assert!(expected.status.success(), "{operations:?}: {expected:?}");
            args.insert(0, "--csv-in");
            let actual = invoke(
                &args,
                b"\"2.5\",\"/a/file.txt\"\n-3.25,/b/other.gz",
                file,
                &[],
            );
            assert_eq!(
                actual.status.code(),
                expected.status.code(),
                "{operations:?}: {actual:?}"
            );
            assert_eq!(actual.stdout, expected.stdout, "{operations:?}");
            assert_eq!(actual.stderr, expected.stderr, "{operations:?}");
        }
    }
    reports(
        &["--csv", "debase64", "1"],
        b"\"YSxi\"\n\"eCJ5CgB6\"\n",
        b"\"a,b\"\n\"x\"\"y\n\"\n",
    );
    reports(
        &["--csv", "--format=%.2f", "round", "1", "frac", "1"],
        b"2.5\n-3.25\n",
        b"3.00,0.50\n-3.00,-0.25\n",
    );
}

#[test]
fn csv_sort_without_keys_streams_in_source_order_and_ignores_unused_target() {
    for file in [false, true] {
        for target in ["1", "not-a-byte-count"] {
            for (args, expected, warning) in [
                (
                    &["--csv", "-s", "sum", "2"][..],
                    b"6\n".as_slice(),
                    b"".as_slice(),
                ),
                (&["--csv", "--sort", "cut", "1-2"], b"z,2\na,4\n", b""),
                (
                    &["--csv", "-s", "--full", "cut", "2"],
                    b"z,2,2\na,4,4\n",
                    b"",
                ),
                (
                    &["--csv", "-s", "--full", "sum", "2"],
                    b"z,2,6\n",
                    FULL_WARNING,
                ),
            ] {
                let output = invoke(
                    args,
                    b"z,2\na,4\n",
                    file,
                    &[("FASTMASH_SORT_MEMORY_BYTES", target)],
                );
                assert!(output.status.success(), "{args:?}: {output:?}");
                assert_eq!(output.stdout, expected);
                assert_eq!(output.stderr, warning);
            }
        }
    }
}

#[test]
fn csv_sorted_full_records_keep_ragged_width_and_original_bytes() {
    let input =
        b"z,5,\"late\r\nrecord\",tail\r\na,2,\"first,record\"\na,2,\"tie\"\"record\",extra\n";
    for file in [false, true] {
        for (operation, expected) in [
            ("min", b"field-1,field-2,field-3,min(field-2)\na,2,\"first,record\",2\nz,5,\"late\r\nrecord\",tail,5\n".as_slice()),
            ("last", b"field-1,field-2,field-3,last(field-2)\na,2,\"tie\"\"record\",extra,2\nz,5,\"late\r\nrecord\",tail,5\n"),
        ] {
            let output = invoke(&["--csv", "--header-out", "--full", "-s", "-g", "1", operation, "2"], input, file, &[("FASTMASH_SORT_MEMORY_BYTES", "1")]);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, FULL_WARNING);
        }
    }
    // One literal quote in a field holding every byte must be doubled.
    let payload: Vec<u8> = (0..=255).collect();
    let field = [
        b"\"".as_slice(),
        &payload[..34],
        b"\"\"",
        &payload[35..],
        b"\"",
    ]
    .concat();
    let input = [b"b,".as_slice(), &field, b",2\na,tail,1\n"].concat();
    let expected = [b"a,tail,1,1\nb,".as_slice(), &field, b",2,1\n"].concat();
    for file in [false, true] {
        let output = invoke(
            &["--csv", "--full", "-s", "-g", "1", "count", "3"],
            &input,
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "1")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
        assert_eq!(output.stderr, FULL_WARNING);
    }
}

#[test]
fn csv_sorted_final_representatives_keep_multiple_keys_case_and_complete_fields() {
    let input = b"z,b,1,late\nA,b,2,\"first\r\nrecord\",tail\na,a,3,older\nA,b,4,\"last\"\"record\",,\na,a,5,final\nZ,a,6,firstz\n";
    let expected = b"a,a,5,final,3,5\nZ,a,6,firstz,6,6\nA,b,4,\"last\"\"record\",\"\",\"\",2,4\nz,b,1,late,1,1\n";
    let text = b"a\ta\t5\tfinal\t3\t5\nZ\ta\t6\tfirstz\t6\t6\nA\tb\t4\tlast\"record\t\t\t2\t4\nz\tb\t1\tlate\t1\t1\n";
    for language in ["C", "en_US.UTF-8", "de_DE.UTF-8"] {
        for file in [false, true] {
            for target in ["67108864", "4096", "1"] {
                for format in ["--csv", "--csv-in"] {
                    let output = invoke(
                        &[
                            format, "--full", "-i", "-s", "-g", "2,1", "first", "3", "last", "3",
                        ],
                        input,
                        file,
                        &[("LC_ALL", language), ("FASTMASH_SORT_MEMORY_BYTES", target)],
                    );
                    assert!(output.status.success(), "{output:?}");
                    assert_eq!(
                        output.stdout,
                        if format == "--csv" {
                            &expected[..]
                        } else {
                            &text[..]
                        }
                    );
                    assert_eq!(output.stderr, FULL_WARNING);
                }
            }
        }
    }
}

#[test]
fn csv_sort_case_multiple_keys_and_nul_order_keep_distinct_group_equality() {
    reports(
        &["--csv", "-i", "-s", "-g", "1", "collapse", "2"],
        b"[,1\nz,2\nA,3\na,4\n",
        b"A,\"3,4\"\nz,2\n[,1\n",
    );
    reports(
        &["--csv", "-s", "-g", "2,1", "first", "3"],
        b"b,a,1\na,b,2\na,a,3\n",
        b"a,a,3\na,b,1\nb,a,2\n",
    );
    reports(
        &["--csv", "-i", "-s", "-g", "1", "collapse", "2"],
        b"A\0y,1\na\0x,2\na\0yy,3\n",
        b"a\0x,\"2,1\"\na\0yy,3\n",
    );
    reports(
        &["--csv", "-s", "-g", "1", "collapse", "2"],
        b"\xff,1\n\0,2\n\x80,3\n,4\n\"\",5\n",
        b"\"\",\"4,5\"\n\0,2\n\x80,3\n\xff,1\n",
    );
}

#[test]
fn csv_language_sort_reuses_per_key_identity_and_stable_collation_ties() {
    for language in ["en_US.UTF-8", "de_DE.UTF-8"] {
        for file in [false, true] {
            for (args, input, expected) in [
                (
                    &["--csv", "-s", "-g", "1,2", "collapse", "3"][..],
                    "é,a,1\ne\u{301},z,2\ne\u{301},a,3\né,z,4\n",
                    "e\u{301},a,3\ne\u{301},z,2\né,a,1\né,z,4\n",
                ),
                (
                    &["--csv", "-s", "-g", "1", "collapse", "2"],
                    "и\u{306},1\nй,2\nи\u{306},3\n",
                    "и\u{306},1\nй,2\nи\u{306},3\n",
                ),
                (
                    &["--csv", "-i", "-s", "-g", "1", "collapse", "2"],
                    "a,1\nA,2\nä,3\nÄ,4\n",
                    "a,\"1,2\"\nä,3\nÄ,4\n",
                ),
                (
                    &["--csv", "-s", "-g", "1", "sum", "2"],
                    "z,1\n,2\n\"\",3\na,4\n",
                    "\"\",5\na,4\nz,1\n",
                ),
            ] {
                for grouping in ["sort", "hash", "restart:1"] {
                    for target in ["67108864", "4096", "1"] {
                        let output = invoke(
                            args,
                            input.as_bytes(),
                            file,
                            &[
                                ("LC_ALL", language),
                                ("FASTMASH_GROUPING", grouping),
                                ("FASTMASH_PIPE_GROUPING", "hash"),
                                ("FASTMASH_SORT_MEMORY_BYTES", target),
                            ],
                        );
                        assert!(output.status.success(), "{output:?}");
                        assert_eq!(output.stdout, expected.as_bytes());
                        assert!(output.stderr.is_empty());
                    }
                }
            }
        }
    }
    for input in [b"\xff,1\n".as_slice(), b"a\0b,1\n"] {
        for file in [false, true] {
            let output = invoke(
                &["--csv", "-s", "-g", "1", "sum", "2"],
                input,
                file,
                &[("LC_ALL", "en_US.UTF-8")],
            );
            assert_eq!(output.status.code(), Some(77));
            assert!(output.stdout.is_empty());
            assert!(
                output
                    .stderr
                    .starts_with(b"fastmash: language sorting requires")
            );
        }
    }
}

#[test]
fn csv_sorted_field_errors_keep_original_locations_and_deferred_output() {
    for file in [false, true] {
        let output = invoke(
            &["--csv", "-H", "-s", "-g", "key\\\nname", "sum", "value"],
            b"\"key\nname\",value,note\nz,bad,unused\nb,3,\"two\nlines\"\na,2,ok\n",
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "1")],
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            output.stdout,
            b"\"GroupBy(key\nname)\",sum(value)\na,2\nb,3\n"
        );
        assert_eq!(output.stderr, b"fastmash: invalid numeric value in line 2 field 2: 'bad'\nCSV record 2 starts at physical line 3\n");
        let output = invoke(
            &["--csv", "-s", "-g", "1", "sum", "2"],
            b"z\na,2,\"two\nlines\"\nb,3\n",
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "1")],
        );
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(output.stdout, b"a,2\nb,3\n");
        assert_eq!(output.stderr, b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\nCSV record 1 starts at physical line 1\n");
        let output = invoke(
            &["--csv", "-s", "-g", "1,2", "count", "1"],
            b"a,1\nb,2\nz\n",
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "1")],
        );
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"a,1,1\nb,2,1\nz,");
        assert_eq!(output.stderr, b"fastmash: invalid input: field 2 requested, line 3 has only 1 fields\nCSV record 3 starts at physical line 3\n");
    }
}

#[test]
fn csv_sorted_empty_headers_and_oversized_records_keep_command_timing() {
    reports(
        &["--csv", "-H", "-s", "-g", "key", "sum", "value"],
        b"",
        b"",
    );
    reports(
        &["--csv", "-H", "-s", "-g", "key", "sum", "value"],
        b"key,value\n",
        b"GroupBy(key),sum(value)\n",
    );
    reports(
        &["--csv", "--header-out", "-s", "-g", "1", "count", "1"],
        b"\n",
        b"GroupBy(field-1),count(field-1)\n\"\",1\n",
    );
    for file in [false, true] {
        for (input, expected) in [
            (b"b,2\na,1\n".as_slice(), b"a,1\nb,1\n".as_slice()),
            (&[b'\n'; 100], b"\"\",100\n"),
            (&[b','; 10000], b"\"\",1\n"),
        ] {
            let output = invoke(
                &["--csv", "-s", "-g", "1", "count", "1"],
                input,
                file,
                &[("FASTMASH_SORT_MEMORY_BYTES", "128")],
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, expected);
            assert!(output.stderr.is_empty());
        }
        let output = invoke(
            &["--csv", "-s", "-g", "1", "sum", "2"],
            b"b,2\na,1\n",
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "4096")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"a,1\nb,2\n");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn csv_sorted_paired_missing_values_and_numeric_neighbors_keep_field_isolation() {
    reports(
        &[
            "--csv",
            "-H",
            "--narm",
            "-s",
            "-g",
            "key",
            "--result-name=1:product",
            "dotprod",
            "a:b",
        ],
        b"key,a,b,note\nb,1,2,unused\na,1,NA,\"two\nlines\"\na,NA,2,unused\na,3,4,unused\n",
        b"GroupBy(key),product\na,14\nb,2\n",
    );
    reports(
        &["--csv", "-s", "-g", "1", "sum", "2"],
        b"b,1,e9\na,2,malformed-number\n",
        b"a,2\nb,1\n",
    );
}

#[test]
fn csv_sort_large_unused_header_keeps_the_data_retention_budget() {
    let input = [
        b"key,value,".as_slice(),
        &[b'x'; 10000],
        b"\nb,2,unused\na,1,unused\n",
    ]
    .concat();
    for file in [false, true] {
        let output = invoke(
            &["--csv", "--header-in", "-s", "-g", "1", "sum", "2"],
            &input,
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "4096")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"a,1\nb,2\n");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn csv_locale_presentation_and_text_lists_remain_single_results() {
    for file in [false, true] {
        let output = invoke(
            &["--csv", "--format=%.2f", "sum", "1", "mean", "1"],
            b"\"1,5\",unused\n\"2,5\",unused\n",
            file,
            &[("LC_ALL", "de_DE.UTF-8")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"\"4,00\",\"2,00\"\n");
        assert!(output.stderr.is_empty());
    }
    reports(
        &[
            "--csv",
            "-i",
            "--collapse-delimiter=|",
            "unique",
            "1",
            "collapse",
            "1",
        ],
        b"\"B,b\"\nb,B\n\"B,b\"\n",
        b"\"b|B,b\",\"B,b|b|B,b\"\n",
    );
}

#[test]
fn csv_long_and_wide_records_cross_normal_buffers_without_size_caps() {
    let field = vec![b'x'; 100_000];
    let bytes = [b"\"".as_slice(), &field, b"\",tail\r\n"].concat();
    reports(
        &["--csv", "cut", "1"],
        &bytes,
        &[&field[..], b"\n"].concat(),
    );
    reports(
        &["--csv", "--full", "cut", "1"],
        &bytes,
        &[&field[..], b",tail,", &field[..], b"\n"].concat(),
    );
    let mut wide = b"start".to_vec();
    wide.extend(std::iter::repeat_n(b',', 10_000));
    wide.extend_from_slice(b"last");
    reports(
        &["--csv", "cut", "1,10000-10001"],
        &wide,
        b"start,\"\",last\n",
    );
    let mut expected = b"start,".to_vec();
    for _ in 0..9999 {
        expected.extend_from_slice(b"\"\",");
    }
    expected.extend_from_slice(b"last,last\n");
    reports(&["--csv", "--full", "cut", "10001"], &wide, &expected);
}

#[test]
fn csv_adjacent_groups_decode_keys_and_keep_later_repeated_runs_separate() {
    reports(
        &["--csv", "-g", "1", "sum", "2", "collapse", "3"],
        b"\"a,b\",2,\"x\ty\"\r\n\"a,b\",3,\"q\"\"r\"\nother,4,\"two\r\nlines\"\n\"a,b\",7,\n",
        b"\"a,b\",5,\"x\ty,q\"\"r\"\nother,4,\"two\r\nlines\"\n\"a,b\",7,\"\"\n",
    );
    reports(
        &["--csv-in", "groupby", "1", "count", "2"],
        b"a,1\n\"a\",2\nb,3\na,4",
        b"a\t2\nb\t1\na\t1\n",
    );
}

#[test]
fn csv_named_multiple_keys_and_result_names_keep_their_header_positions() {
    reports(
        &[
            "--csv", "-H", "--result-name=2:total", "-g", "key\\,raw,second",
            "count", "value", "sum", "value",
        ],
        b"\"key,raw\",second,value\n\"a,b\",x,1\n\"a,b\",x,3\n\"a,b\",y,5\nc,y,7\n",
        b"\"GroupBy(key,raw)\",GroupBy(second),count(value),total\n\"a,b\",x,2,4\n\"a,b\",y,1,5\nc,y,1,7\n",
    );
    reports(
        &["--csv", "--header-out", "-g", "2,1", "sum", "3"],
        b"a,x,1\na,x,3\nb,x,5\n",
        b"GroupBy(field-2),GroupBy(field-1),sum(field-3)\nx,a,4\nx,b,5\n",
    );
}

#[test]
fn csv_full_per_row_reports_copy_decoded_fields_with_their_actual_width() {
    reports(
        &["--csv", "-H", "--full", "--result-name=2:alias", "cut", "2,1"],
        b"\"key,name\",value\n\"a,b\",\"x\ty\"\n\"\0\xff\",,\"q\"\"r\"\n",
        b"\"key,name\",value,cut(value),alias\n\"a,b\",x\ty,x\ty,\"a,b\"\n\0\xff,\"\",\"q\"\"r\",\"\",\"\"\n",
    );
    reports(
        &["--csv-in", "--full", "--output-delimiter=|", "cut", "2"],
        b"x,\"a,b\"\ny,\"two\nlines\",\n",
        b"x|a,b|a,b\ny|two\nlines||two\nlines\n",
    );
}

const FULL_WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";

#[test]
fn csv_full_aggregates_retain_representatives_ties_and_warning_timing() {
    let input = b"a,3,\"first,record\"\nb,1,\"q\"\"r\"\nc,2,\nd,8,\"two\nlines\",extra\ne,1,tie\n";
    for (operations, expected) in [
        (&["sum", "2"][..], b"a,3,\"first,record\",15\n".as_slice()),
        (&["min", "2", "max", "2"], b"d,8,\"two\nlines\",extra,1,8\n"),
        (&["min", "2"], b"b,1,\"q\"\"r\",1\n"),
        (&["last", "2"], b"e,1,tie,1\n"),
    ] {
        for file in [false, true] {
            let mut args = vec!["--csv", "--full"];
            args.extend(operations);
            let output = invoke(&args, input, file, &[]);
            assert!(output.status.success(), "{operations:?}: {output:?}");
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, FULL_WARNING);
        }
    }
    for file in [false, true] {
        for (args, input, expected) in [
            (
                &["--csv", "--full", "count", "1"][..],
                b"".as_slice(),
                b"".as_slice(),
            ),
            (
                &["--csv", "-H", "--full", "count", "value"],
                b"key,value\n",
                b"key,value,count(value)\n",
            ),
            (
                &["--csv", "--header-out", "--full", "sum", "2"],
                b"a,2\n",
                b"field-1,field-2,sum(field-2)\na,2,2\n",
            ),
        ] {
            let output = invoke(args, input, file, &[]);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, FULL_WARNING);
        }
        let output = invoke(
            &["--csv", "-H", "--full", "sum", "absent"],
            b"key,value\n",
            file,
            &[],
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"fastmash: column name 'absent' not found in input file\nCSV record 1 starts at physical line 1\n");
        for (args, input, expected) in [
            (
                &["--csv", "--full", "--narm", "-g", "1", "first", "2"][..],
                b"a,NA,first\na,3,selected\na,2,later\n".as_slice(),
                b"a,3,selected,3\n".as_slice(),
            ),
            (
                &[
                    "--csv",
                    "--full",
                    "--no-strict",
                    "--filler=X",
                    "-g",
                    "1",
                    "count",
                    "1",
                ],
                b"\n\"\"\n",
                b"\"\",2\n",
            ),
        ] {
            let output = invoke(args, input, file, &[]);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, FULL_WARNING);
        }
    }
}

#[test]
fn csv_group_equality_preserves_case_length_nul_and_locale_conventions() {
    let input = b"\"A\0x\",1\na\0y,2\na\0long,3\n\xff,4\n\"\xff\",5\n,6\n\"\",7\n";
    reports(
        &["--csv", "-i", "-g", "1", "sum", "2"],
        input,
        b"A\0x,3\na\0long,3\n\xff,9\n\"\",13\n",
    );
    reports(
        &["--csv", "-g", "1", "count", "2"],
        b"a,1\nA,2\na,3\n",
        b"a,1\nA,1\na,1\n",
    );
    for file in [false, true] {
        let output = invoke(
            &["--csv", "-i", "-g", "1", "--format=%.2f", "sum", "2"],
            b"A,\"1,5\"\n\"a\",\"2,5\"\n\xc3\xa4,\"3,0\"\n\xc3\xa4,\"1,0\"\n",
            file,
            &[("LC_ALL", "de_DE.UTF-8")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"A,\"4,00\"\n\xc3\xa4,\"4,00\"\n");
        assert!(output.stderr.is_empty());
        let output = invoke(
            &["--csv", "-i", "-g", "1", "count", "2"],
            b"\"malformed",
            file,
            &[("LC_ALL", "tr_TR.UTF-8")],
        );
        assert_eq!(output.status.code(), Some(77));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("-i"));
    }
}

#[test]
fn csv_group_errors_keep_the_origin_of_current_and_retained_records() {
    for (args, input, diagnostic) in [
        (&["--csv", "-g", "1,3", "count", "1"][..], b"\"a\nkey\",x\n\"a\nkey\",y,z\n".as_slice(), b"invalid input: field 3 requested, line 1 has only 2 fields\nCSV record 1 starts at physical line 1\n".as_slice()),
        (&["--csv", "-g", "1,3", "count", "1"], b"a,x,z\n\"a\",\"two\nlines\"\n", b"invalid input: field 3 requested, line 2 has only 2 fields\nCSV record 2 starts at physical line 2\n"),
        (&["--csv", "-g", "3", "count", "1"], b"\"two\nlines\",x\n", b"invalid input: field 3 requested, line 1 has only 2 fields\nCSV record 1 starts at physical line 1\n"),
        (&["--csv", "-g", "1", "sum", "2", "last", "3"], b"a,1,\"two\nlines\"\na,2,x\na,bad,y\n", b"invalid numeric value in line 3 field 2: 'bad'\nCSV record 3 starts at physical line 4\n"),
        (&["--csv", "-H", "-g", "missing", "count", "1"], b"\"head\nline\",value\n", b"column name 'missing' not found in input file\nCSV record 1 starts at physical line 1\n"),
    ] {
        errors(args, input, diagnostic);
    }
    // A different first key ends the old Group without inspecting its later
    // missing key. Full output copies what exists, with no padding or filler.
    for file in [false, true] {
        let output = invoke(
            &["--csv", "--full", "-g", "1,3", "count", "1"],
            b"a,x\nb,y,z\n",
            file,
            &[],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"a,x,1\nb,y,z,1\n");
        assert_eq!(output.stderr, FULL_WARNING);
    }
}

#[test]
fn csv_grouped_full_headers_and_aliases_preserve_copied_labels_and_bytes() {
    for file in [false, true] {
        let output = invoke(
            &["--csv", "-H", "--full", "-g", "key\\,raw", "--result-name=2:pair,\"value\"", "--result-name=1:total", "sum", "2", "dotprod", "2:3", "last", "4"],
            b"\"key,raw\",value,other,\"note\ntext\"\n\"a,b\",1,2,\"x\ty\"\n\"a,b\",3,4,\"q\"\"r\"\n",
            file,
            &[],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"\"key,raw\",value,other,\"note\ntext\",total,\"pair,\"\"value\"\"\",\"last(note\ntext)\"\n\"a,b\",3,4,\"q\"\"r\",4,14,\"q\"\"r\"\n");
        assert_eq!(output.stderr, FULL_WARNING);
        let output = invoke(
            &["--csv", "-H", "--full", "--result-name=1:alias", "sum", "2"],
            b"only\n",
            file,
            &[],
        );
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"only,");
        assert_eq!(output.stderr, [FULL_WARNING, b"fastmash: invalid input: field 2 requested, line 1 has only 1 fields\nCSV record 1 starts at physical line 1\n"].concat());
    }
    reports(
        &["--csv", "-H", "-g", "key", "count", "1"],
        b"\"key\0suffix\",key\na,unused\n\"a\",different\n",
        b"GroupBy(key),count(key)\na,2\n",
    );
}

#[test]
fn csv_spill_multiple_merge_levels_keep_source_order_and_exact_aggregates() {
    use std::fmt::Write as _;
    let mut input = String::from("key,value,note\n");
    for at in 0..512 {
        let key = if at % 2 == 0 { "b" } else { "\"a\"" };
        writeln!(&mut input, "{key},{},note{at}", at + 1).unwrap();
    }
    for file in [false, true] {
        for target in ["1", "4096", "65536", "67108864"] {
            let output = invoke(
                &[
                    "--csv",
                    "--seed=13",
                    "-H",
                    "-s",
                    "-g",
                    "key",
                    "--result-name=2:total",
                    "count",
                    "value",
                    "sum",
                    "value",
                    "first",
                    "note",
                    "last",
                    "note",
                    "rand",
                    "value",
                ],
                input.as_bytes(),
                file,
                &[("FASTMASH_SORT_MEMORY_BYTES", target)],
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"GroupBy(key),count(value),total,first(note),last(note),rand(value)\na,256,65792,note1,note511,438\nb,256,65536,note0,note510,11\n");
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn csv_spill_late_syntax_errors_keep_original_location_and_header_timing() {
    for file in [false, true] {
        for header in [false, true] {
            let mut input = Vec::new();
            let mut args = vec!["--csv", "-s", "-g", "1", "sum", "2"];
            if header {
                input.extend_from_slice(b"key,value,note\n");
                args.push("-H");
            }
            for _ in 0..16 {
                input.extend_from_slice(b"b,1,\"two\r\nlines\"\r\n");
            }
            input.extend_from_slice(b"a,2,\"bad\"!\n");
            let output = invoke(&args, &input, file, &[("FASTMASH_SORT_MEMORY_BYTES", "1")]);
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(
                output.stdout,
                if header {
                    b"GroupBy(key),sum(value)\n".as_slice()
                } else {
                    b""
                }
            );
            let (record, line) = if header { (18, 34) } else { (17, 33) };
            assert_eq!(output.stderr, format!("fastmash: invalid CSV: byte after closing quote at physical line {line} byte 10; record {record} starts at physical line {line}\n").as_bytes());
        }
    }
}

#[test]
fn csv_spill_temporary_failures_reclaim_runs_without_touching_other_files() {
    let directory = temp_dir::TempDir::new("csv-spill-cleanup");
    let sentinel = directory.0.join("sentinel");
    fs::write(&sentinel, b"untouched").unwrap();
    let input = directory.0.join("input");
    fs::write(&input, b"b,1\na,2\nb,3\na,4\n").unwrap();
    for limit in [false, true] {
        let mut command = Command::new(executable::fastmash());
        command
            .arg0("fastmash")
            .args(["--csv", "-s", "-g", "1", "sum", "2"])
            .env_clear()
            .env("LC_ALL", "C")
            .env("FASTMASH_SORT_MEMORY_BYTES", "1")
            .env("TMPDIR", &directory.0)
            .stdin(fs::File::open(&input).unwrap());
        if limit {
            // SAFETY: setrlimit and signal are async-signal-safe; only this child
            // receives the file-size limit and ignores its notification signal.
            unsafe {
                command.pre_exec(|| {
                    let limit = libc::rlimit {
                        rlim_cur: 0,
                        rlim_max: 0,
                    };
                    if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    if libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let output = command.output().unwrap();
        if limit {
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(
                output
                    .stderr
                    .starts_with(b"fastmash: sort temporary I/O error:")
            );
        } else {
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"a,6\nb,4\n");
        }
        assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
        let mut names: Vec<_> = fs::read_dir(&directory.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["input", "sentinel"]);
    }
    let missing = directory.0.join("missing");
    let output = invoke(
        &["--csv", "-s", "-g", "1", "sum", "2"],
        b"a,1\n",
        false,
        &[
            ("FASTMASH_SORT_MEMORY_BYTES", "1"),
            ("TMPDIR", missing.to_str().unwrap()),
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(
        output
            .stderr
            .starts_with(b"fastmash: sort temporary I/O error:")
    );
    assert!(!missing.exists());
}
