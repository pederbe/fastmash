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

fn invoke(args: &[&str], input: &[u8], file: bool) -> Output {
    invoke_ctype(args, input, file, None)
}

fn invoke_ctype(args: &[&str], input: &[u8], file: bool, ctype: Option<&str>) -> Output {
    let environment = ctype.map_or_else(Vec::new, |ctype| {
        vec![
            ("LC_ALL", ""),
            ("LC_NUMERIC", "C"),
            ("LC_COLLATE", "C"),
            ("LC_CTYPE", ctype),
        ]
    });
    invoke_in(args, input, file, &environment)
}

fn invoke_in(args: &[&str], input: &[u8], file: bool, environment: &[(&str, &str)]) -> Output {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.envs(environment.iter().copied());
    let temporary = temp_dir::TempDir::new(&format!("selection-{:?}", std::thread::current().id()));
    if file {
        let path = temporary.0.join("input");
        fs::write(&path, input).unwrap();
        command.stdin(fs::File::open(path).unwrap());
        command.output().unwrap()
    } else {
        command.stdin(Stdio::piped());
        let mut child = command.spawn().unwrap();
        if let Err(error) = child.stdin.take().unwrap().write_all(input) {
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
        }
        child.wait_with_output().unwrap()
    }
}

fn selected(args: &[&str], input: &[u8], expected: &[u8]) {
    for file in [false, true] {
        let output = invoke(args, input, file);
        assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
        assert_eq!(output.stdout, expected, "{args:?}, file={file}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }
}

fn selected_sorted(args: &[&str], input: &[u8], expected: &[u8], locale: &str) {
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            let output = invoke_in(
                args,
                input,
                file,
                &[("LC_ALL", locale), ("FASTMASH_SORT_MEMORY_BYTES", memory)],
            );
            assert_eq!(output.status.code(), Some(0), "{args:?}: {output:?}");
            assert_eq!(
                output.stdout, expected,
                "{args:?}, {locale}, {memory}, file={file}"
            );
            assert!(output.stderr.is_empty(), "{output:?}");
        }
    }
}

#[test]
fn csv_sorted_groups_rank_complete_records_with_original_cutoff_ties() {
    let input = b"key,value,note,tail\r\nB,1,first,\n\"A\",10,\"two\nlines\",\nB,3,best,\nA,10,\"a\"\"second\",\nA,10,late,\n";
    selected_sorted(
        &["--csv", "-H", "-s", "-g", "key", "top:2", "value"],
        input,
        b"key,value,note,tail\nA,10,\"two\nlines\",\"\"\nA,10,\"a\"\"second\",\"\"\nB,3,best,\"\"\nB,1,first,\"\"\n",
        "C",
    );
    selected_sorted(
        &["--csv-in", "-H", "-s", "-g", "key", "bottom:1", "value"],
        input,
        b"key\tvalue\tnote\ttail\nA\t10\ttwo\nlines\t\nB\t1\tfirst\t\n",
        "C",
    );
}

#[test]
fn csv_sorted_selection_replacements_keep_wide_multiline_and_final_group_records() {
    let note = [b"\"\0\xff,\r\n\"\"".as_slice(), &vec![b'x'; 4097], b"\""].concat();
    let tail = b",tail".repeat(257);
    let record = |ordinal: usize| {
        let key = if ordinal.is_multiple_of(2) { "z" } else { "a" };
        [format!("{key},{ordinal},").as_bytes(), &note, &tail, b",\n"].concat()
    };
    let output_record = |ordinal: usize| {
        let key = if ordinal.is_multiple_of(2) { "z" } else { "a" };
        [
            format!("{key},{ordinal},").as_bytes(),
            &note,
            &tail,
            b",\"\"\n",
        ]
        .concat()
    };
    let input: Vec<u8> = (0..72).flat_map(record).collect();
    for locale in ["C", "en_US.UTF-8", "de_DE.UTF-8"] {
        for (operation, winners) in [("top:2", [71, 69, 70, 68]), ("bottom:2", [1, 3, 0, 2])] {
            let expected: Vec<u8> = winners.into_iter().flat_map(output_record).collect();
            selected_sorted(
                &["--csv", "-s", "-g", "1", operation, "2"],
                &input,
                &expected,
                locale,
            );
        }
    }
}

#[test]
fn highest_and_lowest_records_keep_source_order_cutoff_ties() {
    let input = b"A\t8\tfirst\nB\t10\tsecond\nC\t10\tthird\nD\t9\tfourth\n";
    selected(&["top:2", "2"], input, b"B\t10\tsecond\nC\t10\tthird\n");
    selected(&["bottom:2", "2"], input, b"A\t8\tfirst\nD\t9\tfourth\n");
}

#[test]
fn text_selection_encodes_complete_fields_as_csv_in_every_grouping_path() {
    let input = b"key\tvalue\tnote\ttail\nB\t1\tb,first\t\nA\t10\ta\"first\0\xff\t\nB\t3\tb,best\t\nA\t10\ta-second\t\n";
    selected(
        &["--csv-out", "-H", "top:2", "value"],
        input,
        b"key,value,note,tail\nA,10,\"a\"\"first\0\xff\",\"\"\nA,10,a-second,\"\"\n",
    );
    selected(
        &["--csv-out", "-H", "-g", "key", "bottom:1", "value"],
        input,
        b"key,value,note,tail\nB,1,\"b,first\",\"\"\nA,10,\"a\"\"first\0\xff\",\"\"\nB,3,\"b,best\",\"\"\nA,10,a-second,\"\"\n",
    );
    selected_sorted(
        &["--csv-out", "-s", "-H", "-g", "key", "top:2", "value"],
        input,
        b"key,value,note,tail\nA,10,\"a\"\"first\0\xff\",\"\"\nA,10,a-second,\"\"\nB,3,\"b,best\",\"\"\nB,1,\"b,first\",\"\"\n",
        "C",
    );
}

#[test]
fn cutoffs_grow_only_with_arriving_records_and_keep_duplicate_observations() {
    let input = b"A\t2\nB\t2\nB\t2\nC\t1\n";
    for (count, top, bottom) in [
        ("1", b"A\t2\n".as_slice(), b"C\t1\n".as_slice()),
        ("0002", b"A\t2\nB\t2\n", b"C\t1\nA\t2\n"),
        (
            "4",
            b"A\t2\nB\t2\nB\t2\nC\t1\n",
            b"C\t1\nA\t2\nB\t2\nB\t2\n",
        ),
        (
            "18446744073709551615",
            b"A\t2\nB\t2\nB\t2\nC\t1\n",
            b"C\t1\nA\t2\nB\t2\nB\t2\n",
        ),
    ] {
        selected(&[&format!("top:{count}"), "2"], input, top);
        selected(&[&format!("bottom:{count}"), "2"], input, bottom);
    }
    selected(&["top:1", "1"], b"", b"");
}

#[test]
fn ranking_uses_admitted_precision_and_copies_numeric_spelling() {
    selected(
        &["top:3", "2"],
        b"A\t9007199254740992\nB\t9007199254740993\nC\t9.007199254740993e15\nD\t9007199254740992.9999\n",
        b"B\t9007199254740993\nC\t9.007199254740993e15\nD\t9007199254740992.9999\n",
    );
    selected(
        &["bottom:3", "2"],
        b"A\t1.00000000000000000001\nB\t1\nC\t1.0000000000000000001\nD\t0x1.0000000000000002p0\n",
        b"A\t1.00000000000000000001\nB\t1\nC\t1.0000000000000000001\n",
    );
    selected(
        &["top:4", "1"],
        b"-2.5\tlow\n-1e0\tnegative\n0x1p-1\thalf\n1.25\thigh\n",
        b"1.25\thigh\n0x1p-1\thalf\n-1e0\tnegative\n-2.5\tlow\n",
    );
}

#[test]
fn signed_zeros_and_same_sign_infinities_tie() {
    let input = b"A\t-0\nB\t0\nC\t-inf\nD\tINF\nE\t+infinity\nF\t-infinity\n";
    selected(
        &["top:6", "2"],
        input,
        b"D\tINF\nE\t+infinity\nA\t-0\nB\t0\nC\t-inf\nF\t-infinity\n",
    );
    selected(
        &["bottom:6", "2"],
        input,
        b"C\t-inf\nF\t-infinity\nA\t-0\nB\t0\nD\tINF\nE\t+infinity\n",
    );
}

#[test]
fn missing_value_removal_applies_only_to_whole_ranking_tokens() {
    selected(
        &["--narm", "top:8", "2"],
        b"A\tna\tkeep?\nB\tN/a\nC\tNaN\nD\t2\tNA\nE\t1\tNaN\n",
        b"D\t2\tNA\nE\t1\tNaN\n",
    );
    selected(&["--narm", "bottom:1", "1"], b"NA\nN/A\nNAN\n", b"");
    for token in ["nan", "+nan", "-nan", "nan(42)", "-nan(0x12)"] {
        for prefix in [vec![], vec!["--narm"]] {
            if token == "nan" && !prefix.is_empty() {
                continue;
            }
            let mut args = prefix;
            args.extend(["top:1", "2"]);
            let output = invoke(&args, format!("A\tinf\nB\t{token}\n").as_bytes(), false);
            assert_eq!(output.status.code(), Some(1), "{token}: {output:?}");
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr,
                b"fastmash: invalid input: field 2 in line 2 has unordered NaN ranking value\n"
            );
        }
    }
    for record in [
        b"B\t".as_slice(),
        b"B",
        b"B\t NA",
        b"B\tNaN ",
        b"B\tbad",
        b"B\t1e5000",
        b"B\t1e-5000",
    ] {
        let input = [b"A\tinf\n".as_slice(), record, b"\n"].concat();
        let output = invoke(&["--narm", "top:1", "2"], &input, false);
        assert_eq!(output.status.code(), Some(1), "{record:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(output.stderr.windows(7).any(|part| part == b"field 2"));
        assert!(output.stderr.windows(6).any(|part| part == b"line 2"));
    }
}

#[test]
fn complete_records_preserve_ragged_fields_bytes_and_framing() {
    selected(
        &["--full", "top:2", "2"],
        b"A\t1\textra\t\nB\t03\t\xff\0tail\t\nC\t2",
        b"B\t03\t\xff\0tail\t\nC\t2\n",
    );
    selected(
        &["-W", "--output-delimiter", "|", "bottom:2", "2"],
        b"  A  2\t\tx\nB\t1\t\nC 3\n",
        b"B|1|\nA|2|x\n",
    );
    selected(
        &["-z", "-t", ",", "top:2", "2"],
        b"A,1,line\nbreak\0B,2,\xff,\0",
        b"B,2,\xff,\0A,1,line\nbreak\0",
    );
    selected(
        &["-C", "bottom:1", "2"],
        b"# prologue\nA\t2\n ;skip\nB\t1\n",
        b"B\t1\n",
    );
    let failed = invoke(
        &["-C", "top:1", "2"],
        b"# skip\nA\tinf\n; skip\nB\tbad\n",
        false,
    );
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    assert_eq!(
        failed.stderr,
        b"fastmash: invalid numeric value in line 2 field 2: 'bad'\n"
    );
    selected(&["-s", "top:1", "2"], b"A\t2\nB\t2\n", b"A\t2\n");
}

#[test]
fn malformed_requests_fail_and_later_capabilities_refuse_before_reading() {
    for request in [
        "top",
        "top:",
        "top:0",
        "top:000",
        "top:+1",
        "top:-1",
        "top:1.0",
        "top:1e2",
        "top:2x",
        "top:18446744073709551616",
        "bottom:",
        "bottom:0",
    ] {
        let output = invoke(&[request, "1"], b"", false);
        assert_eq!(output.status.code(), Some(1), "{request}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    for args in [
        vec!["top:1"],
        vec!["top:1", "0"],
        vec!["top:1", "1,1"],
        vec!["top:1", "1-1"],
        vec!["top:1", "1:1"],
        vec!["top:1", "1", "2"],
        vec!["top:1", "1", "bottom:1", "1"],
        vec!["top:1", "1", "sum", "1"],
        vec!["sum", "1", "top:1", "1"],
        vec!["reverse", "top:1", "1"],
        vec!["top:1", "name"],
    ] {
        let output = invoke(&args, b"", false);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    for control in [
        vec!["--result-name", "1:rank"],
        vec!["--format", "%g"],
        vec!["--round", "1"],
        vec!["--collapse-delimiter", ","],
        vec!["--filler", "N/A"],
        vec!["--seed", "0"],
        vec!["--no-strict"],
        vec!["--vnlog"],
        vec!["--sort-cmd", "sort"],
    ] {
        for trailing in [false, true] {
            let mut args = if trailing { vec!["top:1", "1"] } else { vec![] };
            args.extend(control.iter().copied());
            if !trailing {
                args.extend(["top:1", "1"]);
            }
            let output = invoke(&args, b"", false);
            assert_eq!(output.status.code(), Some(77), "{args:?}: {output:?}");
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
        }
    }
    for args in [
        vec!["--csv-out", "--output-delimiter", ",", "top:1", "1"],
        vec!["--csv-in", "-t", ",", "top:1", "1"],
        vec!["--csv-out", "-z", "top:1", "1"],
        vec!["--format", "%Q", "top:1", "1"],
        vec!["--result-name", "bad", "top:1", "1"],
    ] {
        let output = invoke(&args, b"", false);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    selected(
        &["top:1", "2", "-t", ",", "--out", "|"],
        b"A,2\nB,1\n",
        b"A|2\n",
    );
    selected(&["--", "top:1", "1"], b"2\n1\n", b"2\n");
    for info in ["--help", "--version"] {
        let output = invoke(&["top:bad", "name", info], b"", false);
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.is_empty());
    }
}

#[test]
fn generated_integer_fixtures_match_an_independent_full_order_oracle() {
    let mut state = 0x1234_5678u64;
    for length in [1, 2, 7, 31] {
        for _ in 0..4 {
            let values: Vec<i32> = (0..length)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((state >> 32) % 11) as i32 - 5
                })
                .collect();
            let records: Vec<String> = values
                .iter()
                .enumerate()
                .map(|(index, value)| format!("r{index}\t{value}\tcontext-{index}\n"))
                .collect();
            let input = records.concat();
            for direction in ["top", "bottom"] {
                let mut full_order: Vec<usize> = (0..length).collect();
                full_order.sort_by_key(|&index| {
                    (
                        if direction == "top" {
                            -values[index]
                        } else {
                            values[index]
                        },
                        index,
                    )
                });
                for count in [1, 2, length, length + 3] {
                    let expected = full_order
                        .iter()
                        .take(count)
                        .map(|&index| records[index].as_str())
                        .collect::<String>();
                    let output = invoke(
                        &[&format!("{direction}:{count}"), "2"],
                        input.as_bytes(),
                        false,
                    );
                    assert_eq!(output.status.code(), Some(0), "{output:?}");
                    assert_eq!(
                        output.stdout,
                        expected.as_bytes(),
                        "{direction}, N={count}, {values:?}"
                    );
                    assert!(output.stderr.is_empty());
                }
            }
        }
    }
}

#[test]
fn copied_headers_bind_escaped_names_and_first_duplicate_labels() {
    selected(
        &["-H", "top:1", r"rea\,ding"],
        b"id\trea,ding\trea,ding\nA\t2\t9\nB\t3\t1\n",
        b"id\trea,ding\trea,ding\nB\t3\t1\n",
    );
    selected(
        &["--header-in", "bottom:1", "reading"],
        b"id\treading\nA\t2\nB\t1\n",
        b"B\t1\n",
    );
    selected(
        &["-H", "--full", "top:1", "4"],
        b"id\tvalue\0hidden\nA\tx\ttext\t2\nB\ty\tmore\t3\t\n",
        b"id\tvalue\nB\ty\tmore\t3\t\n",
    );
    for args in [
        vec!["-H", "top:1", "absent"],
        vec!["-H", "-g", "absent", "top:1", "reading"],
        vec!["-H", "-g", "id", "top:1", "absent"],
    ] {
        for file in [false, true] {
            let failed = invoke(&args, b"id\treading\nA\t3\n", file);
            assert_eq!(failed.status.code(), Some(1));
            assert!(failed.stdout.is_empty());
            assert_eq!(
                failed.stderr,
                b"fastmash: column name 'absent' not found in input file\n"
            );
        }
    }
    for args in [vec!["-g", "id", "top:1", "2"], vec!["top:1", "reading"]] {
        let failed = invoke(&args, b"", false);
        assert_eq!(failed.status.code(), Some(1));
        assert_eq!(
            failed.stderr,
            b"fastmash: -H or --header-in must be used with named columns\n"
        );
    }
}

#[test]
fn copied_header_width_comes_from_original_first_record_even_if_omitted() {
    selected(
        &["--header-out", "--narm", "top:1", "2"],
        b"first\tNA\textra\t\nshort\t1\nwinner\t3\n",
        b"field-1\tfield-2\tfield-3\tfield-4\nwinner\t3\n",
    );
    selected(
        &["--header-out", "bottom:1", "2"],
        b"first\t9\nsecond\t2\textra\t\n",
        b"field-1\tfield-2\nsecond\t2\textra\t\n",
    );
    for args in [vec!["-H"], vec!["--header-out"]] {
        let mut args = args;
        args.extend(["top:1", "2"]);
        selected(&args, b"", b"");
    }
    selected(&["-H", "top:1", "2"], b"id\tvalue\n", b"id\tvalue\n");
    selected(&["-H", "top:1", "1"], b"\n", b"\n");
    selected(
        &["-H", "--narm", "top:1", "value"],
        b"id\tvalue\nA\tNA\nB\tN/A\n",
        b"id\tvalue\n",
    );
    selected(
        &["--header-out", "--narm", "bottom:1", "2"],
        b"A\tNA\nB\tNaN\n",
        b"field-1\tfield-2\n",
    );
}

#[test]
fn adjacent_groups_get_independent_cutoffs_with_no_key_prefix() {
    let input = b"A\t8\tfirst\nA\t10\twinner\nA\t10\twinner\nA\t9\tlast\nB\t2\tonly\nA\t1\tlater\n";
    selected(
        &["-g", "1", "top:2", "2"],
        input,
        b"A\t10\twinner\nA\t10\twinner\nB\t2\tonly\nA\t1\tlater\n",
    );
    selected(
        &["groupby", "1", "bottom:2", "2"],
        input,
        b"A\t8\tfirst\nA\t9\tlast\nB\t2\tonly\nA\t1\tlater\n",
    );
    selected(
        &["-H", "-g", "key", "--narm", "top:1", "value"],
        b"key\tid\tvalue\nA\tfirst\t2\nA\tsecond\t3\nB\tomitted\tNA\nA\tlater\t1\n",
        b"key\tid\tvalue\nA\tsecond\t3\nA\tlater\t1\n",
    );
    selected(
        &["-g", "1,2", "top:1", "3"],
        b"\tx\t1\n\tx\t2\n\ty\t3\nB\ty\t4\n",
        b"\tx\t2\n\ty\t3\nB\ty\t4\n",
    );
}

#[test]
fn adjacent_key_equality_case_and_text_controls_match_existing_groups() {
    let input = b"a\tx\t2\nA\tx\t3\nA\tX\t1\na\tx\t1\n";
    selected(
        &["-i", "-g", "1,2", "top:2", "3"],
        input,
        b"A\tx\t3\na\tx\t2\n",
    );
    selected(&["-g", "1,2", "top:1", "3"], input, input);
    selected(&["-i", "-s", "top:1", "3"], input, b"A\tx\t3\n");
    selected(
        &["-g", "1", "--full", "top:1", "2"],
        b"a\0x\t2\t\xff\0tail\t\na\0y\t3\t\xff\0tail\t\na\0yy\t4\n",
        b"a\0y\t3\t\xff\0tail\t\na\0yy\t4\n",
    );
    selected(
        &[
            "-W", "-C", "-H", "--out", "|", "-g", "key", "bottom:1", "value",
        ],
        b"# skip\n key  value note\n A 2 extra\n; skip\nA 1\t\nB 3 end",
        b"key|value|note\nA|1|\nB|3|end\n",
    );
    selected(
        &["-z", "-t", ",", "-H", "-g", "key", "top:1", "value"],
        b"key,value,note\0A,1,line\nbreak\0A,2,\xff,\0B,3,tail\0",
        b"key,value,note\0A,2,\xff,\0B,3,tail\0",
    );
}

#[test]
fn grouped_case_locale_refusal_leaves_output_empty_and_no_key_case_unchanged() {
    for file in [false, true] {
        let failed = invoke_ctype(
            &["-H", "-i", "-g", "key", "top:1", "value"],
            b"key\tvalue\ni\t2\nI\t3\n",
            file,
            Some("tr_TR.UTF-8"),
        );
        assert_eq!(failed.status.code(), Some(77));
        assert!(failed.stdout.is_empty());
        assert!(
            failed.stderr.starts_with(
                "fastmash: unsupported -i under character type ‘tr_TR.UTF-8’ from LC_CTYPE\n"
                    .as_bytes()
            )
        );
        let unchanged = invoke_ctype(
            &["-H", "-i", "top:1", "value"],
            b"key\tvalue\ni\t2\nI\t3\n",
            file,
            Some("tr_TR.UTF-8"),
        );
        assert_eq!(unchanged.status.code(), Some(0));
        assert_eq!(unchanged.stdout, b"key\tvalue\nI\t3\n");
        assert!(unchanged.stderr.is_empty());
    }
}

#[test]
fn grouped_late_failures_report_source_locations_and_keep_only_completed_groups() {
    for file in [false, true] {
        let failed = invoke(
            &["-C", "-H", "-g", "key", "top:1", "value"],
            b"# ignore\nkey\tvalue\nA\t3\nB\tinf\n; ignore\nB\tbad\n",
            file,
        );
        assert_eq!(failed.status.code(), Some(1));
        assert_eq!(failed.stdout, b"key\tvalue\nA\t3\n");
        assert_eq!(
            failed.stderr,
            b"fastmash: invalid numeric value in line 4 field 2: 'bad'\n"
        );
        let failed = invoke(
            &["-H", "-g", "1,3", "--narm", "top:1", "2"],
            b"key\tvalue\textra\nA\t3\tx\nB\tNA\n",
            file,
        );
        assert_eq!(failed.status.code(), Some(1));
        assert_eq!(failed.stdout, b"key\tvalue\textra\n");
        assert_eq!(
            failed.stderr,
            b"fastmash: invalid input: field 3 requested, line 3 has only 2 fields\n"
        );
    }
}

#[test]
fn adjacent_integer_fixtures_match_independent_full_order_per_group() {
    let mut state = 0x1928_3746u64;
    for length in [2, 7, 31] {
        let values: Vec<i32> = (0..length)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((state >> 32) % 7) as i32 - 3
            })
            .collect();
        for width in [1, 3, 5] {
            let records: Vec<String> = values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    format!(
                        "g{}\tr{index}\t{value}\tcontext-{index}\n",
                        (index / width) % 3
                    )
                })
                .collect();
            let input = records.concat();
            for direction in ["top", "bottom"] {
                for count in [1, 2, 4] {
                    let mut expected = String::new();
                    for group in (0..length).collect::<Vec<_>>().chunks(width) {
                        let mut order = group.to_vec();
                        order.sort_by_key(|&index| {
                            (
                                if direction == "top" {
                                    -values[index]
                                } else {
                                    values[index]
                                },
                                index,
                            )
                        });
                        for &index in order.iter().take(count) {
                            expected.push_str(&records[index]);
                        }
                    }
                    selected(
                        &["-g", "1", &format!("{direction}:{count}"), "3"],
                        input.as_bytes(),
                        expected.as_bytes(),
                    );
                }
            }
        }
    }
}

#[test]
fn sorted_groups_keep_complete_wide_records_and_stable_cutoff_ties() {
    let wide = "context".repeat(2000);
    let input = format!(
        "B\t8\tb-first\nA\t10\t{wide}\t\nB\t9\tb-best\nA\t10\ta-second\nA\t10\ta-third\nB\t8\tb-first\nA\t1\tlow\nA\t10\ta-fourth\nB\t8\tb-later\nA\t10\ta-fifth\n"
    );
    let top = format!("A\t10\t{wide}\t\nA\t10\ta-second\nB\t9\tb-best\nB\t8\tb-first\n");
    let bottom = format!("A\t1\tlow\nA\t10\t{wide}\t\nB\t8\tb-first\nB\t8\tb-first\n");
    for full in [false, true] {
        let mut args = vec!["-s", "-g", "1"];
        if full {
            args.push("--full");
        }
        for (request, expected) in [("top:2", &top), ("bottom:2", &bottom)] {
            let mut args = args.clone();
            args.extend([request, "2"]);
            selected_sorted(&args, input.as_bytes(), expected.as_bytes(), "C");
        }
    }
    selected_sorted(
        &["-s", "-t", ",", "-g", "1", "top:1", "2"],
        b"B,1,unused\nA,2,short\nB,3,\xff\0tail,\nA,4,\xff,",
        b"A,4,\xff,\nB,3,\xff\0tail,\n",
        "C",
    );
    selected_sorted(
        &["-s", "-g", "1", "top:1", "2"],
        b"\xff\t1\n\t2\n\x80\t3\na\0y\t4\na\0x\t5\n",
        b"\t2\na\0x\t5\n\x80\t3\n\xff\t1\n",
        "C",
    );
}

#[test]
fn sorted_headers_follow_original_source_schema_and_name_binding() {
    selected_sorted(
        &[
            "-s",
            "-H",
            "-g",
            r"cat\,name,second\:key",
            "top:1",
            "reading",
        ],
        b"cat,name\tsecond:key\treading\treading\nB\ty\t1\t9\nA\tx\t3\t1\nB\ty\t2\t8\nA\ty\t4\t0\n",
        b"cat,name\tsecond:key\treading\treading\nA\tx\t3\t1\nA\ty\t4\t0\nB\ty\t2\t8\n",
        "C",
    );
    for (first, control) in [("NA", Some("--narm")), ("1", None)] {
        let input = format!("# skip\nB\t{first}\nA\t3\textra\t\nB\t2\n");
        let mut args = vec!["-s", "-C", "--header-out", "-g", "1"];
        if let Some(control) = control {
            args.push(control);
        }
        args.extend(["top:1", "2"]);
        selected_sorted(
            &args,
            input.as_bytes(),
            b"field-1\tfield-2\nA\t3\textra\t\nB\t2\n",
            "C",
        );
    }
    for (input, expected) in [
        (b"".as_slice(), b"".as_slice()),
        (b"key\tvalue\n", b"key\tvalue\n"),
        (b"key\tvalue\nB\tNA\nA\tNaN\n", b"key\tvalue\n"),
    ] {
        selected_sorted(
            &["-s", "-H", "-g", "1", "--narm", "top:2", "2"],
            input,
            expected,
            "C",
        );
    }
    for file in [false, true] {
        let output = invoke_in(
            &["-s", "-H", "-g", "missing", "top:1", "value"],
            b"key\tvalue\nA\t3\n",
            file,
            &[("FASTMASH_SORT_MEMORY_BYTES", "1")],
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            b"fastmash: column name 'missing' not found in input file\n"
        );
    }
}

#[test]
fn sorted_text_keeps_whitespace_and_zero_terminated_sort_key_semantics() {
    selected_sorted(
        &["-W", "-s", "-g", "1,2", "--out", "|", "top:1", "3"],
        b" A x 1\nA x 3\n A y 2\nA x 4\n",
        b"A|x|1\nA|y|2\nA|x|4\n",
        "C",
    );
    for locale in ["C", "en_US.UTF-8"] {
        selected_sorted(
            &["-z", "-W", "-s", "-g", "1", "top:1", "2"],
            b"a\nb 1\0a\nc 2\0a\nb 3\0",
            b"a\nb\t1\0a\nc\t2\0a\nb\t3\0",
            locale,
        );
        selected_sorted(
            &[
                "-z", "-s", "-t", ",", "-H", "-g", "key", "bottom:1", "value",
            ],
            b"key,value,note\0B,3,line\nbreak\0A,1,\xff,\0B,2,last\0",
            b"key,value,note\0A,1,\xff,\0B,2,last\0",
            locale,
        );
    }
}

#[test]
fn sorted_group_equality_remains_distinct_from_case_and_locale_ordering() {
    selected_sorted(
        &["-i", "-s", "-g", "1", "top:1", "2"],
        b"[\t1\na\t2\nA\t2\n_\t4\n",
        b"a\t2\n[\t1\n_\t4\n",
        "C",
    );
    for locale in ["en_US.UTF-8", "de_DE.UTF-8"] {
        selected_sorted(
            &["-i", "-s", "-g", "1", "top:1", "2"],
            "a\t1\nA\t2\nä\t3\nÄ\t4\n".as_bytes(),
            "A\t2\nä\t3\nÄ\t4\n".as_bytes(),
            locale,
        );
        selected_sorted(
            &["-s", "-g", "1", "top:1", "2"],
            "и\u{306}\t1\nй\t2\nи\u{306}\t3\n".as_bytes(),
            "и\u{306}\t1\nй\t2\nи\u{306}\t3\n".as_bytes(),
            locale,
        );
    }
}

#[test]
fn sorted_input_errors_keep_original_locations_across_spill() {
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            for (args, input, expected, diagnostic) in [
                (
                    vec!["-C", "-H", "-s", "-g", "key", "top:1", "value"],
                    b"# skip\nkey\tvalue\nB\tinf\n; skip\nA\t3\nB\tbad\n".as_slice(),
                    b"key\tvalue\nA\t3\n".as_slice(),
                    b"fastmash: invalid numeric value in line 4 field 2: 'bad'\n".as_slice(),
                ),
                (
                    vec!["-H", "-s", "-g", "1,3", "--narm", "top:1", "2"],
                    b"key\tvalue\textra\nZ\t3\tx\nB\tNA\n",
                    b"key\tvalue\textra\n",
                    b"fastmash: invalid input: field 3 requested, line 3 has only 2 fields\n",
                ),
                (
                    vec!["-s", "-t", "e", "-g", "1", "top:1", "2"],
                    b"k e9e0\nk e1e99999\n",
                    b"",
                    b"fastmash: invalid numeric value in line 2 field 2: '1'\n",
                ),
                (
                    vec!["-s", "-g", "1", "top:1", "2"],
                    b"Z\t2\nB\tnan\nA\t3\n",
                    b"A\t3\n",
                    b"fastmash: invalid input: field 2 in line 2 has unordered NaN ranking value\n",
                ),
            ] {
                let output = invoke_in(
                    &args,
                    input,
                    file,
                    &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                );
                assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
                assert_eq!(output.stdout, expected, "{args:?}, {memory}, file={file}");
                assert_eq!(output.stderr, diagnostic, "{args:?}, {memory}, file={file}");
            }
        }
    }
}

#[test]
fn sorted_integer_fixtures_match_independent_key_rank_index_order() {
    let mut state = 0x78ab_9023u64;
    for length in [3, 11, 31] {
        let values: Vec<i32> = (0..length)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((state >> 32) % 7) as i32 - 3
            })
            .collect();
        let records: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, value)| format!("g{}\tr{index}\t{value}\tcontext-{index}\n", index % 3))
            .collect();
        let input = records.concat();
        for direction in ["top", "bottom"] {
            for count in [1, 2, 4] {
                let mut expected = String::new();
                for key in 0..3 {
                    let mut order: Vec<usize> =
                        (0..length).filter(|index| index % 3 == key).collect();
                    order.sort_by_key(|&index| {
                        (
                            if direction == "top" {
                                -values[index]
                            } else {
                                values[index]
                            },
                            index,
                        )
                    });
                    for index in order.into_iter().take(count) {
                        expected.push_str(&records[index]);
                    }
                }
                selected_sorted(
                    &["-s", "-g", "1", &format!("{direction}:{count}"), "3"],
                    input.as_bytes(),
                    expected.as_bytes(),
                    "C",
                );
            }
        }
    }
}

#[test]
fn csv_selection_count_direction_and_format_pairings_preserve_decoded_records() {
    let csv =
        b"\"A\",8,first\r\nB,\"10\",\"a,\"\"quote\"\"\r\nline\0\xff\",\r\nC,10,third,\nD,9,fourth";
    for (request, text, encoded) in [
        ("top:1", b"B\t10\ta,\"quote\"\r\nline\0\xff\t\n".as_slice(), b"B,10,\"a,\"\"quote\"\"\r\nline\0\xff\",\"\"\n".as_slice()),
        ("top:0002", b"B\t10\ta,\"quote\"\r\nline\0\xff\t\nC\t10\tthird\t\n", b"B,10,\"a,\"\"quote\"\"\r\nline\0\xff\",\"\"\nC,10,third,\"\"\n"),
        ("bottom:2", b"A\t8\tfirst\nD\t9\tfourth\n", b"A,8,first\nD,9,fourth\n"),
        ("bottom:18446744073709551615", b"A\t8\tfirst\nD\t9\tfourth\nB\t10\ta,\"quote\"\r\nline\0\xff\t\nC\t10\tthird\t\n", b"A,8,first\nD,9,fourth\nB,10,\"a,\"\"quote\"\"\r\nline\0\xff\",\"\"\nC,10,third,\"\"\n"),
    ] {
        selected(&["--csv-in", request, "2"], csv, text);
        selected(&["--csv", "--csv-in", "--csv-out", "--csv", "--full", request, "2"], csv, encoded);
    }
    selected(
        &["--csv-in", "--out", "|", "top:1", "2"],
        b"A,1,\"a|b\"\nB,2,\"x\ny\"\n",
        b"B|2|x\ny\n",
    );
    selected(
        &["--csv-out", "-W", "bottom:1", "2"],
        b" A  2\t\tx\nB\t1\t\n",
        b"B,1,\"\"\n",
    );
}

#[test]
fn csv_headers_and_generated_schema_work_in_every_input_output_pairing() {
    for (mode, csv_in, csv_out) in [
        ("--csv-in", true, false),
        ("--csv-out", false, true),
        ("--csv", true, true),
    ] {
        let separator = if csv_in { "," } else { "\t" };
        let output_separator = if csv_out { "," } else { "\t" };
        for grouped in [false, true] {
            let mut base = vec![mode];
            if grouped {
                base.extend(["-g", "1"]);
            }
            let mut args = base.clone();
            args.extend(["--header-out", "--narm", "top:1", "2"]);
            for first in ["NA", "1"] {
                let input = format!("A{separator}{first}\nA{separator}3{separator}extra\n");
                let expected = format!(
                    "field-1{output_separator}field-2\nA{output_separator}3{output_separator}extra\n"
                );
                selected(&args, input.as_bytes(), expected.as_bytes());
            }
            selected(&args, b"", b"");
            let all_missing = format!("A{separator}NA\nB{separator}NaN\n");
            let generated = format!("field-1{output_separator}field-2\n");
            selected(&args, all_missing.as_bytes(), generated.as_bytes());
            let mut args = base;
            args.extend(["-H", "--narm", "top:1", "rank"]);
            let header = format!("id{separator}rank\n");
            let expected = format!("id{output_separator}rank\n");
            selected(&args, header.as_bytes(), expected.as_bytes());
            selected(&args, b"", b"");
            selected(
                &args,
                format!("{header}{all_missing}").as_bytes(),
                expected.as_bytes(),
            );
        }
    }
    selected(
        &["--csv", "-H", "-g", r"key\,name,second\:key", "top:1", r"rank\,value"],
        b"\"key,name\",second:key,\"rank,value\",\"rank,value\",\"note\0ignored\"\nA,x,2,9,first\n\"A\",\"x\",3,1,last\nB,x,1,8,other\n",
        b"\"key,name\",second:key,\"rank,value\",\"rank,value\",note\nA,x,3,1,last\nB,x,1,8,other\n",
    );
    selected(
        &["--csv", "-H", "top:1", "2"],
        b"\"a\nlabel\",rank\r\nX,2\n",
        b"\"a\nlabel\",rank\nX,2\n",
    );
    selected(&["--csv", "-H", "top:1", "1"], b"\n", b"\"\"\n");
    selected(
        &["--csv", "--header-in", "top:1", "2"],
        b"id,rank\nA,2\n",
        b"A,2\n",
    );
    for mode in ["--csv-in", "--csv"] {
        for selector in ["missing", "absent"] {
            let args = if selector == "missing" {
                vec![mode, "-H", "top:1", selector]
            } else {
                vec![mode, "-H", "-g", selector, "top:1", "rank"]
            };
            for file in [false, true] {
                let output = invoke(&args, b"id,rank\nA,2\n", file);
                assert_eq!(output.status.code(), Some(1));
                assert!(output.stdout.is_empty());
                assert_eq!(output.stderr, format!("fastmash: column name '{selector}' not found in input file\nCSV record 1 starts at physical line 1\n").as_bytes());
            }
        }
    }
}

#[test]
fn csv_numeric_precision_special_values_and_missing_tokens_match_selection() {
    selected(
        &["--csv", "top:3", "2"],
        b"A,9007199254740992\nB,9007199254740993\nC,9.007199254740993e15\nD,9007199254740992.9999\n",
        b"B,9007199254740993\nC,9.007199254740993e15\nD,9007199254740992.9999\n",
    );
    selected(
        &["--csv", "bottom:3", "2"],
        b"A,1.00000000000000000001\nB,1\nC,1.0000000000000000001\nD,0x1.0000000000000002p0\n",
        b"A,1.00000000000000000001\nB,1\nC,1.0000000000000000001\n",
    );
    let input = b"A,-0\nB,0\nC,-inf\nD,INF\nE,+infinity\nF,-infinity\n";
    selected(
        &["--csv", "top:6", "2"],
        input,
        b"D,INF\nE,+infinity\nA,-0\nB,0\nC,-inf\nF,-infinity\n",
    );
    selected(
        &["--csv-in", "bottom:6", "2"],
        input,
        b"C\t-inf\nF\t-infinity\nA\t-0\nB\t0\nD\tINF\nE\t+infinity\n",
    );
    selected(
        &["--csv", "--narm", "bottom:8", "2"],
        b"A,na\nB,\"N/a\"\nC,NaN\nD,-2.5,NA\nE,0x1p-1,NaN\nF,-1e0\nG,1.25\n",
        b"D,-2.5,NA\nF,-1e0\nE,0x1p-1,NaN\nG,1.25\n",
    );
    for token in [
        "nan",
        "+nan",
        "-nan",
        "nan(42)",
        "-nan(0x12)",
        "",
        " NA",
        "NaN ",
        "bad",
        "1e5000",
        "1e-5000",
    ] {
        for narm in [false, true] {
            if narm && token == "nan" {
                continue;
            }
            let mut args = vec!["--csv"];
            if narm {
                args.push("--narm");
            }
            args.extend(["top:1", "2"]);
            for file in [false, true] {
                let output = invoke(
                    &args,
                    format!("A,inf,\"multi\nline\"\nB,\"{token}\",late\n").as_bytes(),
                    file,
                );
                assert_eq!(
                    output.status.code(),
                    Some(1),
                    "{args:?}, {token}: {output:?}"
                );
                assert!(output.stdout.is_empty());
                let location = b"CSV record 2 starts at physical line 3\n";
                assert!(output.stderr.ends_with(location));
                assert!(output.stderr.windows(7).any(|part| part == b"field 2"));
            }
        }
    }
}

#[test]
fn csv_adjacent_groups_use_decoded_case_keys_and_omit_whole_records() {
    for mode in ["--csv-in", "--csv"] {
        let expected = if mode == "--csv" {
            b"a,x,3,best\nA,y,4,other\na,x,1,later\n".as_slice()
        } else {
            b"a\tx\t3\tbest\nA\ty\t4\tother\na\tx\t1\tlater\n".as_slice()
        };
        selected(
            &[mode, "-i", "-g", "1,2", "--narm", "top:1", "3"],
            b"\"A\",\"x\",2,first\na,x,3,best\nA,y,4,other\nB,x,NA,omitted\na,x,1,later\n",
            expected,
        );
    }
    selected(
        &["--csv", "-g", "1", "top:2", "2"],
        b",1,empty\n\"\",2,second\n,2,second\nA,3,other\n",
        b"\"\",2,second\n\"\",2,second\nA,3,other\n",
    );
    selected(
        &["--csv", "-s", "-i", "top:1", "2"],
        b"A,2\nB,2\n",
        b"A,2\n",
    );
    for file in [false, true] {
        let output = invoke_ctype(
            &["--csv", "-i", "-H", "-g", "key", "top:1", "rank"],
            b"key,rank\nA,2\n",
            file,
            Some("tr_TR.UTF-8"),
        );
        assert_eq!(output.status.code(), Some(77));
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn csv_syntax_and_all_required_fields_are_checked_after_cutoff_and_before_narm() {
    for file in [false, true] {
        for (args, input, expected, diagnostic) in [
            (vec!["--csv", "top:1", "2"], b"A,inf,\"multi\nline\"\nB,1,late\"bad\n".as_slice(), b"".as_slice(), b"fastmash: invalid CSV: quote in unquoted field at physical line 3 byte 9; record 2 starts at physical line 3\n".as_slice()),
            (vec!["--csv", "--narm", "top:1", "2"], b"A,inf,first\nB,NA,\"unused\"x\n", b"", b"fastmash: invalid CSV: byte after closing quote at physical line 2 byte 14; record 2 starts at physical line 2\n"),
            (vec!["--csv", "--narm", "-g", "1,3", "top:1", "2"], b"A,3,x\nB,NA\n", b"", b"fastmash: invalid input: field 3 requested, line 2 has only 2 fields\nCSV record 2 starts at physical line 2\n"),
            (vec!["--csv", "--narm", "top:1", "2"], b"A,inf\nB\n", b"", b"fastmash: invalid input: field 2 requested, line 2 has only 1 fields\nCSV record 2 starts at physical line 2\n"),
            (vec!["--csv", "-g", "1", "top:1", "2"], b"A,3,first\nB,4,\"multi\nline\"\nB,bad,late\n", b"A,3,first\n", b"fastmash: invalid numeric value in line 3 field 2: 'bad'\nCSV record 3 starts at physical line 4\n"),
            (vec!["--csv", "top:1", "1"], b"\n", b"", b"fastmash: invalid numeric value in line 1 field 1: ''\nCSV record 1 starts at physical line 1\n"),
        ] {
            let output = invoke(&args, input, file);
            assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, diagnostic);
        }
    }
}

#[test]
fn csv_switches_keep_exact_matching_conflicts_and_selection_refusals() {
    for mode in ["--csv", "--csv-in", "--csv-out"] {
        for control in [
            vec!["-z"],
            vec!["--vnlog"],
            vec!["--output-delimiter", ","],
            vec!["-t", ","],
            vec!["-W"],
            vec!["-C"],
        ] {
            if (mode == "--csv-in" && control[0] == "--output-delimiter")
                || (mode == "--csv-out" && matches!(control[0], "-t" | "-W" | "-C"))
            {
                continue;
            }
            for reversed in [false, true] {
                let mut args = if reversed {
                    control.clone()
                } else {
                    vec![mode]
                };
                if reversed {
                    args.push(mode);
                } else {
                    args.extend(control.iter().copied());
                }
                args.extend(["top:1", "1"]);
                let output = invoke(&args, b"", false);
                assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
                assert!(output.stdout.is_empty());
            }
        }
    }
    for args in [
        vec!["--csv-i", "top:1", "1"],
        vec!["--csv-o", "top:1", "1"],
        vec!["--csv-in", "top:0", "2"],
        vec!["--csv", "top:1", "1-1"],
        vec!["--csv", "-H", "top:1", "1,1"],
    ] {
        let output = invoke(&args, b"", false);
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn csv_selection_validates_long_unused_multiline_fields_and_keeps_bom_as_data() {
    let wide = "x".repeat(128 * 1024 - 13);
    let valid = format!("A,inf,winner\nB,1,\"{wide}\"\"quote\nline\"\n");
    selected(
        &["--csv", "top:1", "2"],
        valid.as_bytes(),
        b"A,inf,winner\n",
    );
    let invalid = format!("A,inf,winner\nB,NA,\"{wide}\"\"quote\nline\"x\n");
    for file in [false, true] {
        let output = invoke(&["--csv", "--narm", "top:1", "2"], invalid.as_bytes(), file);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"fastmash: invalid CSV: byte after closing quote at physical line 3 byte 6; record 2 starts at physical line 2\n");
    }
    selected(
        &["--csv", "bottom:1", "2"],
        b"\xef\xbb\xbfA,1\r\nB,2",
        b"\xef\xbb\xbfA,1\n",
    );
}

#[test]
fn sorted_text_csv_output_keeps_source_schema_and_ordinary_key_and_numeric_boundaries() {
    selected_sorted(
        &[
            "--csv-out",
            "--header-out",
            "--narm",
            "-s",
            "-g",
            "1",
            "top:1",
            "2",
        ],
        b"B\tNA\nA\t3\textra\t\nB\t2\n",
        b"field-1,field-2\nA,3,extra,\"\"\nB,2\n",
        "C",
    );
    selected_sorted(
        &["--csv-out", "-W", "-s", "-g", "1,2", "top:1", "3"],
        b" A x 1\nA x 3\n A y 2\nA x 4\n",
        b"A,x,1\nA,y,2\nA,x,4\n",
        "C",
    );
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            let output = invoke_in(
                &["--csv-out", "-s", "-t", "e", "-g", "1", "top:1", "2"],
                b"k e9e0\nk e1e99999\n",
                file,
                &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
            );
            assert_eq!(output.status.code(), Some(1));
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr,
                b"fastmash: invalid numeric value in line 2 field 2: '1'\n"
            );
        }
    }
}

#[test]
fn csv_whole_and_adjacent_selection_match_independent_integer_oracle() {
    let mut state = 0x4b12_5678u64;
    for length in [1, 7, 31] {
        let values: Vec<i32> = (0..length)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((state >> 32) % 9) as i32 - 4
            })
            .collect();
        let records: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                format!("g{},r{index},{value},\"context,{index}\"\n", index / 3 % 2)
            })
            .collect();
        let text: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                format!("g{}\tr{index}\t{value}\tcontext,{index}\n", index / 3 % 2)
            })
            .collect();
        let input = records.concat();
        for grouped in [false, true] {
            for direction in ["top", "bottom"] {
                for count in [1, 2, length, length + 3] {
                    let mut order = Vec::new();
                    let mut start = 0;
                    while start < length {
                        let end = if grouped {
                            (start + 3).min(length)
                        } else {
                            length
                        };
                        let mut indices: Vec<usize> = (start..end).collect();
                        indices.sort_by_key(|&index| {
                            (
                                if direction == "top" {
                                    -values[index]
                                } else {
                                    values[index]
                                },
                                index,
                            )
                        });
                        order.extend(indices.into_iter().take(count));
                        start = end;
                    }
                    for (mode, source) in [("--csv", &records), ("--csv-in", &text)] {
                        let expected = order
                            .iter()
                            .map(|&index| source[index].as_str())
                            .collect::<String>();
                        let mut args = vec![mode];
                        if grouped {
                            args.extend(["-g", "1"]);
                        }
                        let request = format!("{direction}:{count}");
                        args.extend([&request, "3"]);
                        selected(&args, input.as_bytes(), expected.as_bytes());
                    }
                }
            }
        }
    }
}

#[test]
fn sorted_selection_temporary_write_failures_reclaim_runs() {
    for csv in [false, true] {
        let directory = temp_dir::TempDir::new("selection-spill-cleanup");
        let sentinel = directory.0.join("sentinel");
        let input = directory.0.join("input");
        fs::write(&sentinel, b"untouched").unwrap();
        fs::write(
            &input,
            if csv {
                b"B,1\nA,2\nB,3\nA,4\n".as_slice()
            } else {
                b"B\t1\nA\t2\nB\t3\nA\t4\n"
            },
        )
        .unwrap();
        let mut args = vec!["-s", "-g", "1", "top:1", "2"];
        if csv {
            args.push("--csv");
        }
        let mut command = Command::new(executable::fastmash());
        command
            .arg0("fastmash")
            .args(&args)
            .env_clear()
            .env("LC_ALL", "C")
            .env("FASTMASH_SORT_MEMORY_BYTES", "1")
            .env("TMPDIR", &directory.0)
            .stdin(fs::File::open(&input).unwrap());
        // SAFETY: these async-signal-safe calls only constrain the child process.
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
        let failed = command.output().unwrap();
        assert_eq!(failed.status.code(), Some(1), "{failed:?}");
        assert!(failed.stdout.is_empty());
        assert!(
            failed
                .stderr
                .starts_with(b"fastmash: sort temporary I/O error:")
        );
        assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
        let mut names: Vec<_> = fs::read_dir(&directory.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        names.sort();
        assert_eq!(names, ["input", "sentinel"]);
        let missing = directory.0.join("missing");
        let failed = invoke_in(
            &args,
            if csv { b"A,1\n".as_slice() } else { b"A\t1\n" },
            false,
            &[
                ("FASTMASH_SORT_MEMORY_BYTES", "1"),
                ("TMPDIR", missing.to_str().unwrap()),
            ],
        );
        assert_eq!(failed.status.code(), Some(1));
        assert!(failed.stdout.is_empty());
        assert!(
            failed
                .stderr
                .starts_with(b"fastmash: sort temporary I/O error:")
        );
        assert!(!missing.exists());
    }
}

#[test]
fn csv_sorted_headers_use_original_source_schema_and_complete_ragged_records() {
    selected_sorted(
        &[
            "--csv",
            "--header-out",
            "--narm",
            "-s",
            "-g",
            "1",
            "top:1",
            "2",
        ],
        b"z,NA,source,,\xff\na,2,\"two\nlines\",\na,3,\0\xff,\nb,1,last\n",
        b"field-1,field-2,field-3,field-4,field-5\na,3,\0\xff,\"\"\nb,1,last\n",
        "C",
    );
    selected_sorted(
        &[
            "--csv-in",
            "--output-delimiter",
            "|",
            "--header-out",
            "-s",
            "-g",
            "1",
            "bottom:1",
            "2",
        ],
        b"z,9,source,wide,\na,2,\"two\nlines\",\na,3,later\n",
        b"field-1|field-2|field-3|field-4|field-5\na|2|two\nlines|\nz|9|source|wide|\n",
        "C",
    );
    selected_sorted(
        &["--csv", "-H", "-s", "-g", r"group\,name", "top:2", "value"],
        b"\"group,name\",value,value,note\nb,1,999,first\na,3,0,\"a\"\"first\"\na,3,0,later\n",
        b"\"group,name\",value,value,note\na,3,0,\"a\"\"first\"\na,3,0,later\nb,1,999,first\n",
        "C",
    );
    for (input, expected) in [
        (b"".as_slice(), b"".as_slice()),
        (b"key,value\n", b"key,value\n"),
        (b"key,value\nz,NA\na,NaN\nz,N/A\n", b"key,value\n"),
    ] {
        selected_sorted(
            &["--csv", "-H", "--narm", "-s", "-g", "key", "top:1", "value"],
            input,
            expected,
            "C",
        );
    }
}

#[test]
fn csv_sorted_case_multiple_keys_and_locale_ties_keep_existing_groups() {
    selected_sorted(
        &["--csv", "-i", "-s", "-g", "1,2", "top:1", "3"],
        b"b,z,1\na,y,3\nA,x,4\nb,Z,2\na,X,4\na,y,2\n",
        b"A,x,4\na,y,3\nb,Z,2\n",
        "C",
    );
    selected_sorted(
        &["--csv", "-s", "-g", "1", "top:1", "2"],
        b"a\0x,1\na\0y,2\na,3\na\0x,4\n",
        b"a,3\na\0x,4\n",
        "C",
    );
    for locale in ["en_US.UTF-8", "de_DE.UTF-8"] {
        selected_sorted(
            &["--csv", "-i", "-s", "-g", "1", "top:1", "2"],
            "a,1\nA,2\nä,3\nÄ,4\n".as_bytes(),
            "A,2\nä,3\nÄ,4\n".as_bytes(),
            locale,
        );
        selected_sorted(
            &["--csv", "-s", "-g", "1", "top:1", "2"],
            "и\u{306},1\nй,2\nи\u{306},3\n".as_bytes(),
            "и\u{306},1\nй,2\nи\u{306},3\n".as_bytes(),
            locale,
        );
    }
}

#[test]
fn csv_sorted_ranking_keeps_numeric_precision_and_field_local_conversion() {
    selected_sorted(
        &["--csv", "--full", "-s", "-g", "1", "top:1", "2"],
        b"b,1,e99999\na,9007199254740992,first\na,9007199254740993,precise\nb,2,ignored\n",
        b"a,9007199254740993,precise\nb,2,ignored\n",
        "C",
    );
    selected_sorted(
        &["--csv", "-s", "-g", "1", "bottom:18446744073709551615", "2"],
        b"b,+inf,first\na,-0,first\na,0,second\na,-inf,low\na,0x1p0,hex\nb,inf,second\n",
        b"a,-inf,low\na,-0,first\na,0,second\na,0x1p0,hex\nb,+inf,first\nb,inf,second\n",
        "C",
    );
    selected_sorted(
        &["--csv", "-s", "-g", "1", "top:2", "2"],
        b"b,\"1,25\",first\na,\"-2,75\",low\nb,\"1,5\",higher\na,\"-1,5\",later\n",
        b"a,\"-1,5\",later\na,\"-2,75\",low\nb,\"1,5\",higher\nb,\"1,25\",first\n",
        "de_DE.UTF-8",
    );
}

#[test]
fn csv_sorted_wide_multiline_ties_and_duplicates_survive_many_runs() {
    let note = format!("{}\r\n{}", "q".repeat(9000), "n".repeat(9000));
    let first = format!("\"a,key\",10,\"{note}\",first,\n");
    let duplicate = "b,7,duplicate,\n";
    let mut input = format!("b,7,first,\n{first}");
    for index in 0..35 {
        input.push_str(&format!("\"a,key\",10,late-{index},\n"));
        input.push_str(duplicate);
    }
    let expected = format!(
        "\"a,key\",10,\"{note}\",first,\"\"\n\"a,key\",10,late-0,\"\"\nb,7,first,\"\"\nb,7,duplicate,\"\"\n"
    );
    for direction in ["top:2", "bottom:2"] {
        selected_sorted(
            &["--csv", "-s", "-g", "1", direction, "2"],
            input.as_bytes(),
            expected.as_bytes(),
            "C",
        );
    }
}

#[test]
fn csv_sorted_selection_matches_independent_key_rank_index_oracle() {
    let mut state = 0x4239_879bu64;
    for length in [3, 11, 31] {
        let values: Vec<i32> = (0..length)
            .map(|_| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                ((state >> 32) % 7) as i32 - 3
            })
            .collect();
        let csv: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                format!(
                    "\"g,{}\",h{},{value},\"note\n{index}\"\n",
                    index % 3,
                    index % 2
                )
            })
            .collect();
        let text: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                format!("g,{}\th{}\t{value}\tnote\n{index}\n", index % 3, index % 2)
            })
            .collect();
        let input = csv.concat();
        for direction in ["top", "bottom"] {
            for count in [1, 2, 4, length + 1] {
                let mut order = Vec::new();
                for first in 0..3 {
                    for second in 0..2 {
                        let mut indices: Vec<usize> = (0..length)
                            .filter(|index| index % 3 == first && index % 2 == second)
                            .collect();
                        indices.sort_by_key(|&index| {
                            (
                                if direction == "top" {
                                    -values[index]
                                } else {
                                    values[index]
                                },
                                index,
                            )
                        });
                        order.extend(indices.into_iter().take(count));
                    }
                }
                for (mode, records) in [("--csv", &csv), ("--csv-in", &text)] {
                    let expected = order
                        .iter()
                        .map(|&index| records[index].as_str())
                        .collect::<String>();
                    selected_sorted(
                        &[
                            mode,
                            "-s",
                            "-g",
                            "1,2",
                            &format!("{direction}:{count}"),
                            "3",
                        ],
                        input.as_bytes(),
                        expected.as_bytes(),
                        "C",
                    );
                }
            }
        }
    }
}

#[test]
fn csv_sorted_validation_keeps_original_locations_and_checks_omitted_records() {
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            for (keys, input, expected, diagnostic) in [
                (
                    "key",
                    b"key,value,note\nz,2,\"two\nlines\"\nb,bad,late\na,3,first\n".as_slice(),
                    b"key,value,note\na,3,first\n".as_slice(),
                    b"fastmash: invalid numeric value in line 3 field 2: 'bad'\nCSV record 3 starts at physical line 4\n".as_slice(),
                ),
                (
                    "1,3",
                    b"key,value,note\nz,2,x\nb,NA\n",
                    b"key,value,note\n",
                    b"fastmash: invalid input: field 3 requested, line 3 has only 2 fields\nCSV record 3 starts at physical line 3\n",
                ),
                (
                    "1",
                    b"key,value,note\nz,NaN,last\na,3,first\nb,nan(payload),late\n",
                    b"key,value,note\na,3,first\n",
                    b"fastmash: invalid input: field 2 in line 4 has unordered NaN ranking value\nCSV record 4 starts at physical line 4\n",
                ),
                (
                    "1",
                    b"key,value,note\na,inf,first\na,1,\"bad\"x\n",
                    b"key,value,note\n",
                    b"fastmash: invalid CSV: byte after closing quote at physical line 3 byte 10; record 3 starts at physical line 3\n",
                ),
                (
                    "1",
                    b"key,value,note\na,3,first\nz,NA,\"unfinished",
                    b"key,value,note\n",
                    b"fastmash: invalid CSV: unexpected end of input in quoted field; record 3 starts at physical line 3\n",
                ),
            ] {
                let output = invoke_in(
                    &["--csv", "-H", "--narm", "-s", "-g", keys, "top:1", "value"],
                    input, file, &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                );
                assert_eq!(output.status.code(), Some(1), "{keys}, {memory}, {output:?}");
                assert_eq!(output.stdout, expected, "{keys}, {memory}, {output:?}");
                assert_eq!(output.stderr, diagnostic, "{keys}, {memory}, {output:?}");
            }
        }
    }
}
