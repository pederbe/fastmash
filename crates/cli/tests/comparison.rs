//! Comparison Commands with real independently opened files and stdin.
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

fn candidate() -> Command {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}
fn success(output: Output, expected: &[u8]) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn comparison_files_and_each_stdin_side_support_pipe_and_redirected_file() {
    let root = temp_dir::TempDir::new("comparison-sources");
    let before = root.0.join("before with spaces");
    let after = root.0.join("after with spaces");
    fs::write(&before, b"2\n").unwrap();
    fs::write(&after, b"5\n").unwrap();
    let expected = b"matched\t\tnot_requested\t2\t5\t3\tavailable\t150\tavailable\n";
    success(
        candidate()
            .args([
                "compare",
                before.to_str().unwrap(),
                after.to_str().unwrap(),
                "sum",
                "1",
            ])
            .output()
            .unwrap(),
        expected,
    );
    for stdin_before in [false, true] {
        for regular in [false, true] {
            let mut command = candidate();
            command.args([
                "compare",
                if stdin_before {
                    "-"
                } else {
                    before.to_str().unwrap()
                },
                if stdin_before {
                    after.to_str().unwrap()
                } else {
                    "-"
                },
                "sum",
                "1",
            ]);
            let path = if stdin_before { &before } else { &after };
            let output = if regular {
                command
                    .stdin(fs::File::open(path).unwrap())
                    .output()
                    .unwrap()
            } else {
                let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(if stdin_before { b"2\n" } else { b"5\n" })
                    .unwrap();
                child.wait_with_output().unwrap()
            };
            success(output, expected);
        }
    }
    success(
        candidate()
            .args([
                "compare",
                before.to_str().unwrap(),
                before.to_str().unwrap(),
                "sum",
                "1",
            ])
            .output()
            .unwrap(),
        b"matched\t\tnot_requested\t2\t2\t0\tavailable\t0\tavailable\n",
    );
}

#[test]
fn comparison_literal_dash_paths_option_scanning_and_nul_records() {
    let root = temp_dir::TempDir::new("comparison-literal-paths");
    fs::write(root.0.join("-before"), b"# comment\0 2  1\0").unwrap();
    fs::write(root.0.join("-after"), b"; comment\0 5  1\0").unwrap();
    success(
        candidate()
            .current_dir(&root.0)
            .args([
                "-z",
                "-W",
                "-C",
                "--output-delimiter=|",
                "--",
                "compare",
                "-before",
                "-after",
                "sum",
                "1",
            ])
            .output()
            .unwrap(),
        b"matched||not_requested|2|5|3|available|150|available\0",
    );
    fs::write(root.0.join("before"), b"2\n").unwrap();
    fs::write(root.0.join("after"), b"5\n").unwrap();
    success(
        candidate()
            .current_dir(&root.0)
            .args([
                "compare",
                "before",
                "after",
                "sum",
                "--form=%.1f",
                "1",
                "-s",
            ])
            .output()
            .unwrap(),
        b"matched\t\tnot_requested\t2.0\t5.0\t3.0\tavailable\t150.0\tavailable\n",
    );
}

#[test]
fn comparison_global_keys_are_canonical_across_pipe_file_sort_and_locale() {
    let root = temp_dir::TempDir::new("comparison-keys");
    let before = root.0.join("before");
    let after = root.0.join("after");
    let before_bytes = b"\xc3\xa4\t1\nz\t1\n\xc3\xa4\t1\n";
    let after_bytes = b"z\t1\n\xc3\xa4\t1\n";
    fs::write(&before, before_bytes).unwrap();
    fs::write(&after, after_bytes).unwrap();
    let expected = b"z\tmatched\t\tnot_requested\t1\t1\t0\tavailable\t0\tavailable\n\xc3\xa4\tmatched\t\tnot_requested\t2\t1\t-1\tavailable\t-50\tavailable\n";
    for locale in ["C", "de_DE.UTF-8"] {
        for sorting in [false, true] {
            for pipe in [false, true] {
                let mut command = candidate();
                command.env("LC_ALL", locale).args([
                    "-g1",
                    "compare",
                    before.to_str().unwrap(),
                    if pipe { "-" } else { after.to_str().unwrap() },
                    "count",
                    "2",
                ]);
                if sorting {
                    command.arg("-s");
                }
                let output = if pipe {
                    let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
                    child.stdin.take().unwrap().write_all(after_bytes).unwrap();
                    child.wait_with_output().unwrap()
                } else {
                    command.output().unwrap()
                };
                success(output, expected);
            }
        }
    }
}

#[test]
fn comparison_named_headers_and_result_blocks_support_each_stdin_transport() {
    let root = temp_dir::TempDir::new("comparison-headers");
    let before = root.0.join("before");
    let after = root.0.join("after");
    let before_bytes = b"item\tvalue\tweight\nA\t2\t1\nA\t4\t1\n";
    let after_bytes = b"weight\titem\tvalue\n1\tA\t6\n";
    fs::write(&before, before_bytes).unwrap();
    fs::write(&after, after_bytes).unwrap();
    let expected = b"key(item)\tpresence\trank\trank_state\tbefore(reading)\tafter(reading)\tdifference(reading)\tdifference_state(reading)\tpercentage(reading)\tpercentage_state(reading)\nA\tmatched\t\tnot_requested\t3\t6\t3\tavailable\t100\tavailable\n";
    for stdin_before in [false, true] {
        for regular in [false, true] {
            let mut command = candidate();
            command.args([
                "-H",
                "-i",
                "-gitem",
                "--result-name=1:reading",
                "compare",
                if stdin_before {
                    "-"
                } else {
                    before.to_str().unwrap()
                },
                if stdin_before {
                    after.to_str().unwrap()
                } else {
                    "-"
                },
                "wmean",
                "value:weight",
            ]);
            let output = if regular {
                command
                    .stdin(fs::File::open(if stdin_before { &before } else { &after }).unwrap())
                    .output()
                    .unwrap()
            } else {
                let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(if stdin_before {
                        before_bytes.as_slice()
                    } else {
                        after_bytes.as_slice()
                    })
                    .unwrap();
                child.wait_with_output().unwrap()
            };
            success(output, expected);
        }
    }
}

#[test]
fn comparison_header_only_files_validate_both_sources_before_any_output() {
    let root = temp_dir::TempDir::new("comparison-header-only");
    fs::write(root.0.join("before"), b"v\n").unwrap();
    fs::write(root.0.join("after"), b"v\n").unwrap();
    let mut command = candidate();
    command
        .current_dir(&root.0)
        .args(["-H", "compare", "before", "after", "sum", "v"]);
    success(command.output().unwrap(), b"presence\trank\trank_state\tbefore(sum(v))\tafter(sum(v))\tdifference(sum(v))\tdifference_state(sum(v))\tpercentage(sum(v))\tpercentage_state(sum(v))\n");
    for (path, bytes, fragment) in [
        ("after", b"other\n".as_slice(), "column name 'v'"),
        ("after", b"", "requires an input header"),
        ("before", b"", "requires an input header"),
    ] {
        fs::write(root.0.join("before"), b"v\n").unwrap();
        fs::write(root.0.join("after"), b"v\n").unwrap();
        fs::write(root.0.join(path), bytes).unwrap();
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        assert!(diagnostic.contains(fragment), "{diagnostic}");
        assert!(
            diagnostic.contains(&format!("{path} source '{path}'")),
            "{diagnostic}"
        );
    }
}

#[test]
fn comparison_ranked_named_reports_limit_winners_and_keep_exclusions_on_each_transport() {
    let root = temp_dir::TempDir::new("comparison-ranking");
    let before = root.0.join("before");
    let after = root.0.join("after");
    let before_bytes = b"k\tv\nA\t100\nB\t10\nzero\t0\nremoved\t4\n";
    let after_bytes = b"v\tk\n130\tA\n20\tB\n1\tzero\n5\tadded\n";
    fs::write(&before, before_bytes).unwrap();
    fs::write(&after, after_bytes).unwrap();
    let expected = b"key(k)\tpresence\trank\trank_state\tbefore(total)\tafter(total)\tdifference(total)\tdifference_state(total)\tpercentage(total)\tpercentage_state(total)\nB\tmatched\t1\tranked\t10.0\t20.0\t10.0\tavailable\t100.0\tavailable\nadded\tadded\t\tone_sided\t\t5.0\t\tnot_matched\t\tnot_matched\nremoved\tremoved\t\tone_sided\t4.0\t\t\tnot_matched\t\tnot_matched\nzero\tmatched\t\tunavailable\t0.0\t1.0\t1.0\tavailable\t\tzero_baseline\n";
    for stdin_side in [None, Some(0), Some(1)] {
        for regular in [false, true] {
            let mut command = candidate();
            command.args([
                "-H",
                "-gk",
                "--format=%.1f",
                "--result-name=1:total",
                "compare",
                if stdin_side == Some(0) {
                    "-"
                } else {
                    before.to_str().unwrap()
                },
                if stdin_side == Some(1) {
                    "-"
                } else {
                    after.to_str().unwrap()
                },
                "rank",
                "01",
                "percent",
                "limit",
                "0001",
                "sum",
                "v",
            ]);
            let output = if let Some(side) = stdin_side {
                if regular {
                    command
                        .stdin(fs::File::open(if side == 0 { &before } else { &after }).unwrap())
                        .output()
                        .unwrap()
                } else {
                    let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
                    let mut input = child.stdin.take().unwrap();
                    for part in if side == 0 {
                        before_bytes.as_slice()
                    } else {
                        after_bytes.as_slice()
                    }
                    .chunks(3)
                    {
                        input.write_all(part).unwrap();
                    }
                    drop(input);
                    child.wait_with_output().unwrap()
                }
            } else {
                command.output().unwrap()
            };
            success(output, expected);
        }
    }
}

#[test]
fn comparison_rank_errors_precede_missing_files_and_late_data_errors_precede_headers() {
    let root = temp_dir::TempDir::new("comparison-rank-errors");
    let output = candidate()
        .current_dir(&root.0)
        .args(["compare", "absent", "also-absent", "rank", "2", "sum", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"fastmash: comparison rank result exceeds the number of results\n"
    );
    fs::write(root.0.join("before"), b"k\tv\nA\t1\n").unwrap();
    fs::write(root.0.join("after"), b"k\tv\nA\t100\nZ\tbad\n").unwrap();
    let output = candidate()
        .current_dir(&root.0)
        .args([
            "-H", "-gk", "compare", "before", "after", "rank", "1", "limit", "1", "sum", "v",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(
        diagnostic.contains("line 3") && diagnostic.contains("after source 'after'"),
        "{diagnostic}"
    );
}

#[test]
fn comparison_open_read_and_grammar_failures_are_role_aware_and_write_no_stdout() {
    let root = temp_dir::TempDir::new("comparison-input-errors");
    fs::write(root.0.join("before"), b"2\n").unwrap();
    for (args, fragment) in [
        (
            vec!["compare", "absent", "after", "sum", "0"],
            "invalid field",
        ),
        (
            vec!["compare", "absent", "after", "sum", "1"],
            "before source 'absent'",
        ),
        (
            vec!["compare", "before", "absent", "sum", "1"],
            "after source 'absent'",
        ),
        (
            vec!["compare", "before", ".", "sum", "1"],
            "after source '.'",
        ),
        (
            vec![
                "compare", "before", "absent", "rank", "1", "absolute", "rank", "1", "sum", "1",
            ],
            "invalid operation",
        ),
    ] {
        let output = candidate()
            .current_dir(&root.0)
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(fragment),
            "{args:?}: {output:?}"
        );
    }
}

#[test]
fn comparison_all_formats_keep_named_ranking_and_exclusions_on_each_transport() {
    let root = temp_dir::TempDir::new("comparison-csv-transports");
    let before = root.0.join("before.csv");
    let after = root.0.join("after.csv");
    // Negative remains a winner at limit 2; all exclusions follow winners,
    // and A's smaller percentage is omitted. The literal schema is stable.
    let text = b"key(k)\tpresence\trank\trank_state\tbefore(total)\tafter(total)\tdifference(total)\tdifference_state(total)\tpercentage(total)\tpercentage_state(total)\nB\tmatched\t1\tranked\t10\t20\t10\tavailable\t100\tavailable\nnegative\tmatched\t2\tranked\t-100\t-50\t50\tavailable\t-50\tavailable\nadded\tadded\t\tone_sided\t\t5\t\tnot_matched\t\tnot_matched\ninf\tmatched\t\tunavailable\tinf\tinf\t\tunordered\t\tnonfinite_baseline\nremoved\tremoved\t\tone_sided\t4\t\t\tnot_matched\t\tnot_matched\nzero\tmatched\t\tunavailable\t0\t1\t1\tavailable\t\tzero_baseline\n";
    let csv = b"key(k),presence,rank,rank_state,before(total),after(total),difference(total),difference_state(total),percentage(total),percentage_state(total)\nB,matched,1,ranked,10,20,10,available,100,available\nnegative,matched,2,ranked,-100,-50,50,available,-50,available\nadded,added,\"\",one_sided,\"\",5,\"\",not_matched,\"\",not_matched\ninf,matched,\"\",unavailable,inf,inf,\"\",unordered,\"\",nonfinite_baseline\nremoved,removed,\"\",one_sided,4,\"\",\"\",not_matched,\"\",not_matched\nzero,matched,\"\",unavailable,0,1,1,available,\"\",zero_baseline\n";
    for input_csv in [false, true] {
        let before_bytes: &[u8] = if input_csv {
            b"k,v\r\nA,100\nB,10\nnegative,-100\nzero,0\nremoved,4\ninf,inf\n"
        } else {
            b"k\tv\nA\t100\nB\t10\nnegative\t-100\nzero\t0\nremoved\t4\ninf\tinf\n"
        };
        let after_bytes: &[u8] = if input_csv {
            b"v,k\n130,A\n20,B\n-50,negative\n1,zero\n5,added\ninf,inf"
        } else {
            b"v\tk\n130\tA\n20\tB\n-50\tnegative\n1\tzero\n5\tadded\ninf\tinf"
        };
        fs::write(&before, before_bytes).unwrap();
        fs::write(&after, after_bytes).unwrap();
        for output_csv in [false, true] {
            for stdin_side in [None, Some(0), Some(1)] {
                for regular in [false, true] {
                    if stdin_side.is_none() && regular {
                        continue;
                    }
                    let mut command = candidate();
                    command.args(["-H", "-gk", "--result-name=1:total"]);
                    if input_csv {
                        command.arg("--csv-in");
                    }
                    if output_csv {
                        command.arg("--csv-out");
                    }
                    command.args([
                        "compare",
                        if stdin_side == Some(0) {
                            "-"
                        } else {
                            before.to_str().unwrap()
                        },
                        if stdin_side == Some(1) {
                            "-"
                        } else {
                            after.to_str().unwrap()
                        },
                        "rank",
                        "1",
                        "percent",
                        "limit",
                        "2",
                        "min",
                        "v",
                    ]);
                    let output = if let Some(side) = stdin_side {
                        if regular {
                            command
                                .stdin(
                                    fs::File::open(if side == 0 { &before } else { &after })
                                        .unwrap(),
                                )
                                .output()
                                .unwrap()
                        } else {
                            let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
                            let mut input = child.stdin.take().unwrap();
                            for part in if side == 0 { before_bytes } else { after_bytes }.chunks(3)
                            {
                                input.write_all(part).unwrap();
                            }
                            drop(input);
                            child.wait_with_output().unwrap()
                        }
                    } else {
                        command.output().unwrap()
                    };
                    success(output, if output_csv { csv } else { text });
                }
            }
        }
    }
}

#[test]
fn comparison_csv_numeric_locale_commas_stay_in_one_field() {
    let root = temp_dir::TempDir::new("comparison-csv-locale");
    fs::write(root.0.join("before"), b"k,v\n\"a,b\",\"1,5\"\n").unwrap();
    fs::write(root.0.join("after"), b"v,k\n\"3,0\",\"a,b\"\n").unwrap();
    success(candidate().current_dir(&root.0).env("LC_ALL", "de_DE.UTF-8").args(["--csv", "-H", "-gk", "compare", "before", "after", "rank", "1", "sum", "v"]).output().unwrap(),
        b"key(k),presence,rank,rank_state,before(sum(v)),after(sum(v)),difference(sum(v)),difference_state(sum(v)),percentage(sum(v)),percentage_state(sum(v))\n\"a,b\",matched,1,ranked,\"1,5\",3,\"1,5\",available,100,available\n");
}

#[test]
fn comparison_csv_late_multiline_syntax_errors_precede_any_header() {
    let root = temp_dir::TempDir::new("comparison-csv-location");
    let before = root.0.join("before");
    let after = root.0.join("after");
    let valid = b"k,v\n\"a\nb\",1\n";
    let invalid = b"k,v\n\"a\nb\",1\n\"z\nz\",2,bad\"quote\n";
    for side in [0, 1] {
        fs::write(
            &before,
            if side == 0 {
                invalid.as_slice()
            } else {
                valid.as_slice()
            },
        )
        .unwrap();
        fs::write(
            &after,
            if side == 1 {
                invalid.as_slice()
            } else {
                valid.as_slice()
            },
        )
        .unwrap();
        let output = candidate()
            .args([
                "--csv",
                "-H",
                "-gk",
                "compare",
                before.to_str().unwrap(),
                after.to_str().unwrap(),
                "rank",
                "1",
                "limit",
                "1",
                "sum",
                "v",
            ])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        for fragment in [
            "invalid CSV",
            "physical line 5 byte 9",
            "record 3 starts at physical line 4",
            if side == 0 {
                "before source"
            } else {
                "after source"
            },
        ] {
            assert!(diagnostic.contains(fragment), "{fragment}: {diagnostic}");
        }
    }
}

#[test]
fn comparison_csv_preserves_non_utf8_supplied_result_names_and_keys() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let root = temp_dir::TempDir::new("comparison-csv-byte-labels");
    fs::write(root.0.join("before"), b"k,v\n\xff\0,2\n").unwrap();
    fs::write(root.0.join("after"), b"k,v\n\xff\0,4\n").unwrap();
    success(candidate().current_dir(&root.0).args(["--csv", "-H", "-gk"]).arg(OsString::from_vec(b"--result-name=1:\xff,\n".to_vec())).args(["compare", "before", "after", "sum", "v"]).output().unwrap(),
        b"key(k),presence,rank,rank_state,\"before(\xff,\n)\",\"after(\xff,\n)\",\"difference(\xff,\n)\",\"difference_state(\xff,\n)\",\"percentage(\xff,\n)\",\"percentage_state(\xff,\n)\"\n\xff\0,matched,\"\",not_requested,2,4,2,available,100,available\n");
}
