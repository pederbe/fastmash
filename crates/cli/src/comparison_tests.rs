//! Dataset comparison through the complete Command interface.
use super::command_test_support::{Input, Output, comparison_command};

#[test]
fn comparison_missing_header_combines_source_failures_before_any_output() {
    use super::command_test_support::{Sources, command_sources};
    for csv in [false, true] {
        for side in [0, 1] {
            let mut sources = Sources {
                inputs: [
                    Input {
                        bytes: b"v\n1\n",
                        segment: 1,
                        error: None,
                    },
                    Input {
                        bytes: b"v\n2\n",
                        segment: 1,
                        error: None,
                    },
                ],
                closes: [None, None],
                opened: 0,
                completed: 0,
            };
            sources.inputs[side].bytes = b"";
            sources.inputs[side].error = Some(5);
            sources.closes[side] = Some(5);
            let mut output = Output {
                bytes: vec![],
                error: None,
                closed: false,
            };
            let mut args = vec!["-H", "compare", "before", "after", "sum", "v"];
            if csv {
                args.insert(0, "--csv");
            }
            let (status, diagnostics) = command_sources(&mut sources, &mut output, &args);
            assert_eq!(status, 1);
            assert!(output.bytes.is_empty());
            assert!(output.closed);
            assert_eq!((sources.opened, sources.completed), (side + 1, side + 1));
            assert_eq!(diagnostics.len(), 1);
            let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
            assert!(diagnostic.starts_with("compare requires an input header on each source\nread error: Input/output error\ninput close error:"), "{diagnostic}");
            assert!(diagnostic.contains(if side == 0 {
                "before source"
            } else {
                "after source"
            }));
            assert!(!diagnostic.contains("completing result"));
        }
    }
}

#[test]
fn comparison_source_read_failure_precedes_retained_numerical_completion() {
    use super::command_test_support::{Sources, command_sources};
    for csv in [false, true] {
        let mut sources = Sources {
            inputs: [
                Input {
                    bytes: if csv {
                        b"k,v,w\nZ,1,0\n"
                    } else {
                        b"k\tv\tw\nZ\t1\t0\n"
                    },
                    segment: 1,
                    error: Some(5),
                },
                Input {
                    bytes: b"unread",
                    segment: 1,
                    error: None,
                },
            ],
            closes: [None, None],
            opened: 0,
            completed: 0,
        };
        let mut output = Output {
            bytes: vec![],
            error: None,
            closed: false,
        };
        let mut args = vec!["-H", "-gk", "compare", "before", "after", "wmean", "v:w"];
        if csv {
            args.insert(0, "--csv");
        }
        let (status, diagnostics) = command_sources(&mut sources, &mut output, &args);
        assert_eq!(status, 1);
        assert!(output.bytes.is_empty());
        assert!(output.closed);
        assert_eq!((sources.opened, sources.completed), (1, 1));
        assert_eq!(diagnostics.len(), 1);
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        assert!(
            diagnostic.starts_with("read error: Input/output error\n"),
            "{diagnostic}"
        );
        assert!(diagnostic.contains("before source"));
        assert!(!diagnostic.contains("completing result") && !diagnostic.contains("weight"));
    }
}

#[test]
fn comparison_retained_completion_failures_follow_key_encounter_order_before_output() {
    for csv in [false, true] {
        for side in [0, 1] {
            let (good, failed): (&[u8], &[u8]) = if csv {
                (b"k,v,w\nA,1,1\n", b"k,v,w\nZ,3,0\nA,2,0\n")
            } else {
                (b"k\tv\tw\nA\t1\t1\n", b"k\tv\tw\nZ\t3\t0\nA\t2\t0\n")
            };
            let mut args = vec![
                "-H", "-gk", "compare", "before", "after", "rank", "1", "limit", "1", "count", "v",
                "wmean", "v:w",
            ];
            if csv {
                args.insert(0, "--csv");
            }
            let (status, output, diagnostics) = if side == 0 {
                run(&args, failed, good)
            } else {
                run(&args, good, failed)
            };
            assert_eq!(status, 1);
            assert!(output.is_empty());
            assert_eq!(diagnostics.len(), 1);
            let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
            assert!(diagnostic.contains("completing result 2"), "{diagnostic}");
            assert!(diagnostic.contains("key ('Z')"), "{diagnostic}");
            assert!(diagnostic.contains(if side == 0 {
                "before source"
            } else {
                "after source"
            }));
        }
    }
}

#[test]
fn comparison_csv_formats_are_independent_and_header_neutral() {
    for (flags, before, after, expected) in [
        (
            vec!["--csv-in"],
            b"\"a,b\",2\r\n".as_slice(),
            b"\"a,b\",5".as_slice(),
            b"a,b\tmatched\t\tnot_requested\t2\t5\t3\tavailable\t150\tavailable\n".as_slice(),
        ),
        (
            vec!["--csv-out"],
            b"a,b\t2\n",
            b"a,b\t5\n",
            b"\"a,b\",matched,\"\",not_requested,2,5,3,available,150,available\n",
        ),
        (
            vec!["--csv"],
            b"\"a,b\",2\r\n",
            b"\"a,b\",5",
            b"\"a,b\",matched,\"\",not_requested,2,5,3,available,150,available\n",
        ),
    ] {
        let mut args = flags;
        args.extend(["-g1", "compare", "before", "after", "sum", "2"]);
        assert_eq!(run(&args, before, after), (0, expected.to_vec(), vec![]));
    }
}

#[test]
fn comparison_ranking_selects_absolute_or_percentage_magnitude() {
    let before = b"B\t10\nA\t100\n";
    let after = b"A\t130\nB\t20\n";
    for (measure, expected) in [
        (None, b"A\tmatched\t1\tranked\t100\t130\t30\tavailable\t30\tavailable\nB\tmatched\t2\tranked\t10\t20\t10\tavailable\t100\tavailable\n".as_slice()),
        (Some("absolute"), b"A\tmatched\t1\tranked\t100\t130\t30\tavailable\t30\tavailable\nB\tmatched\t2\tranked\t10\t20\t10\tavailable\t100\tavailable\n"),
        (Some("percent"), b"B\tmatched\t1\tranked\t10\t20\t10\tavailable\t100\tavailable\nA\tmatched\t2\tranked\t100\t130\t30\tavailable\t30\tavailable\n"),
    ] {
        let mut args = vec!["-g1", "compare", "before", "after", "rank", "1"];
        args.extend(measure);
        args.extend(["sum", "2"]);
        assert_eq!(run(&args, before, after), (0, expected.to_vec(), vec![]));
    }
}

#[test]
fn comparison_rank_limit_retains_every_exclusion_after_winners() {
    let before = b"zero\t-0\nB\t10\nremoved\t4\nsameinf\tinf\nA\t100\nnan\tnan\nunchanged\t3\n";
    let after = b"added\t7\nA\t130\nB\t20\nunchanged\t3\nzero\t0\nsameinf\tinf\nnan\t8\n";
    assert_eq!(
        run(
            &["-g1", "compare", "before", "after", "rank", "1", "percent", "limit", "1", "min", "2"],
            before, after,
        ),
        (0, b"B\tmatched\t1\tranked\t10\t20\t10\tavailable\t100\tavailable\nadded\tadded\t\tone_sided\t\t7\t\tnot_matched\t\tnot_matched\nnan\tmatched\t\tunavailable\tnan\t8\t\tunordered\t\tnonfinite_baseline\nremoved\tremoved\t\tone_sided\t4\t\t\tnot_matched\t\tnot_matched\nsameinf\tmatched\t\tunavailable\tinf\tinf\t\tunordered\t\tnonfinite_baseline\nzero\tmatched\t\tunavailable\t-0\t0\t0\tavailable\t\tzero_baseline\n".to_vec(), vec![]),
    );
}

#[test]
fn comparison_rank_limits_accept_leading_zeros_maximum_and_fewer_winners() {
    let before = b"zero\t0\nsame\t3\n";
    let after = b"same\t3\nzero\t-0\n";
    let expected = b"same\tmatched\t1\tranked\t3\t3\t0\tavailable\t0\tavailable\nzero\tmatched\t\tunavailable\t0\t-0\t-0\tavailable\t\tzero_baseline\n";
    for limit in ["0001", "10", "18446744073709551615"] {
        assert_eq!(
            run(
                &[
                    "-g1", "compare", "before", "after", "rank", "0001", "percent", "limit", limit,
                    "min", "2"
                ],
                before,
                after
            ),
            (0, expected.to_vec(), vec![]),
        );
    }
    assert_eq!(
        run(
            &[
                "-g1", "compare", "before", "after", "rank", "1", "percent", "limit", "1", "min",
                "2"
            ],
            b"zero\t0\n",
            b"zero\t-0\n"
        ),
        (
            0,
            b"zero\tmatched\t\tunavailable\t0\t-0\t-0\tavailable\t\tzero_baseline\n".to_vec(),
            vec![]
        ),
    );
    assert_eq!(
        run(
            &[
                "compare",
                "before",
                "after",
                "rank",
                "1",
                "limit",
                "18446744073709551615",
                "sum",
                "1"
            ],
            b"",
            b""
        ),
        (0, vec![], vec![])
    );
}

#[test]
fn comparison_ranking_preserves_signed_changes_zero_ties_and_infinite_ties() {
    let before =
        b"z\t0\na\t-0\npositive\t-100\nnegative\t100\ninf-b\tinf\ninf-a\t1\nunordered\t-inf\n";
    let after =
        b"negative\t50\npositive\t-50\ninf-b\t1\ninf-a\tinf\na\t0\nz\t-0\nunordered\t-inf\n";
    let (status, output, diagnostics) = run(
        &["-g1", "compare", "before", "after", "rank", "1", "min", "2"],
        before,
        after,
    );
    assert_eq!((status, diagnostics), (0, vec![]));
    let rows: Vec<Vec<&[u8]>> = output
        .split(|b| *b == b'\n')
        .filter(|row| !row.is_empty())
        .map(|row| row.split(|b| *b == b'\t').collect())
        .collect();
    type ExpectedRow<'a> = (&'a [u8], &'a [u8], &'a [u8], &'a [u8]);
    let expected: &[ExpectedRow<'_>] = &[
        (b"inf-a", b"1", b"inf", b"nonfinite_after"),
        (b"inf-b", b"2", b"-inf", b"nonfinite_baseline"),
        (b"negative", b"3", b"-50", b"available"),
        (b"positive", b"4", b"50", b"available"),
        (b"a", b"5", b"0", b"zero_baseline"),
        (b"z", b"6", b"-0", b"zero_baseline"),
        (b"unordered", b"", b"", b"nonfinite_baseline"),
    ];
    assert_eq!(rows.len(), expected.len());
    for (row, &(key, rank, difference, percentage_state)) in rows.iter().zip(expected) {
        assert_eq!(
            (row[0], row[2], row[6], row[9]),
            (key, rank, difference, percentage_state)
        );
        assert_eq!(
            row[3],
            if rank.is_empty() {
                b"unavailable".as_slice()
            } else {
                b"ranked"
            }
        );
    }
    assert_eq!(rows[2][8], b"-50");
    assert_eq!(rows[3][8], b"-50");
    let (status, output, diagnostics) = run(
        &[
            "-g1", "compare", "before", "after", "rank", "1", "percent", "min", "2",
        ],
        b"cross\t-2\nfall\t-100\n",
        b"fall\t-150\ncross\t3\n",
    );
    assert_eq!((status, diagnostics), (0, vec![]));
    assert_eq!(output, b"cross\tmatched\t1\tranked\t-2\t3\t5\tavailable\t-250\tavailable\nfall\tmatched\t2\tranked\t-100\t-150\t-50\tavailable\t50\tavailable\n");
}

#[test]
fn comparison_ranking_ties_use_normalized_complete_tuples_across_interleavings() {
    let keys: &[(&[u8], &[u8])] = &[
        (b"", b"z"),
        (b"01", b"k"),
        (b"1", b"k"),
        (b"A", b"bc"),
        (b"Ab", b"c"),
        (b"b", b"a"),
        (b"X\0A", b"1"),
        (b"x\0b", b"1"),
        (b"Z", b"k"),
        (b"\xff", b"k"),
    ];
    for reversed in [false, true] {
        let mut before = Vec::new();
        let mut after = Vec::new();
        for at in 0..keys.len() {
            let at = if reversed { keys.len() - 1 - at } else { at };
            let (first, second) = keys[at];
            for (input, value, fold) in [
                (&mut before, if at % 2 == 0 { b"1" } else { b"6" }, false),
                (&mut after, if at % 2 == 0 { b"6" } else { b"1" }, true),
            ] {
                if fold {
                    input.extend(first.iter().map(u8::to_ascii_lowercase));
                } else {
                    input.extend_from_slice(first);
                }
                input.push(b'\t');
                input.extend_from_slice(second);
                input.push(b'\t');
                input.extend_from_slice(value);
                input.push(b'\n');
            }
        }
        let (status, output, diagnostics) = run(
            &[
                "-i", "-g1,2", "compare", "before", "after", "rank", "1", "min", "3",
            ],
            &before,
            &after,
        );
        assert_eq!((status, diagnostics), (0, vec![]));
        let rows: Vec<_> = output
            .split(|b| *b == b'\n')
            .filter(|row| !row.is_empty())
            .collect();
        assert_eq!(rows.len(), keys.len());
        for (at, (row, &(first, second))) in rows.iter().zip(keys).enumerate() {
            let cells: Vec<_> = row.split(|b| *b == b'\t').collect();
            assert_eq!((cells[0], cells[1]), (first, second));
            assert_eq!(cells[3], (at + 1).to_string().as_bytes());
        }
    }
}

#[test]
fn comparison_ranking_uses_typed_scores_and_decimal_ranks_under_numeric_formatting() {
    for measure in ["absolute", "percent"] {
        assert_eq!(
            run(&["--format=%.0f", "-g1", "compare", "before", "after", "rank", "1", measure, "sum", "2"], b"A\t100\nB\t100\n", b"A\t101.4\nB\t101.49\n"),
            (0, b"B\tmatched\t1\tranked\t100\t101\t1\tavailable\t1\tavailable\nA\tmatched\t2\tranked\t100\t101\t1\tavailable\t1\tavailable\n".to_vec(), vec![]),
        );
    }
    assert_eq!(
        run(&["--format=%a", "compare", "before", "after", "rank", "1", "sum", "1"], b"3\n", b"5\n"),
        (0, b"matched\t1\tranked\t0xcp-2\t0xap-1\t0x8p-2\tavailable\t0x8.555555555555556p+3\tavailable\n".to_vec(), vec![]),
    );
}

#[test]
fn comparison_ranking_indexes_all_expanded_list_range_repeated_and_paired_results() {
    for (index, expected_keys) in [
        ("1", [b"A", b"B"]),
        ("2", [b"B", b"A"]),
        ("3", [b"A", b"B"]),
        ("4", [b"A", b"B"]),
        ("5", [b"B", b"A"]),
    ] {
        let (status, output, diagnostics) = run(
            &[
                "-g1", "compare", "before", "after", "rank", index, "sum", "2-3", "mean", "2,2",
                "dotprod", "2:3",
            ],
            b"A\t100\t4\nB\t10\t1\n",
            b"B\t20\t10\nA\t130\t4\n",
        );
        assert_eq!((status, diagnostics), (0, vec![]));
        let rows: Vec<_> = output
            .split(|b| *b == b'\n')
            .filter(|row| !row.is_empty())
            .collect();
        for (at, row) in rows.iter().enumerate() {
            let cells: Vec<_> = row.split(|b| *b == b'\t').collect();
            assert_eq!(cells.len(), 34);
            assert_eq!(cells[0], expected_keys[at]);
            assert_eq!(cells[2], (at + 1).to_string().as_bytes());
            let values: Vec<_> = cells[4..]
                .as_chunks::<6>()
                .0
                .iter()
                .map(|block| (block[0], block[1], block[2]))
                .collect();
            let expected: &[(&[u8], &[u8], &[u8])] = if cells[0] == b"A" {
                &[
                    (b"100", b"130", b"30"),
                    (b"4", b"4", b"0"),
                    (b"100", b"130", b"30"),
                    (b"100", b"130", b"30"),
                    (b"400", b"520", b"120"),
                ]
            } else {
                &[
                    (b"10", b"20", b"10"),
                    (b"1", b"10", b"9"),
                    (b"10", b"20", b"10"),
                    (b"10", b"20", b"10"),
                    (b"10", b"200", b"190"),
                ]
            };
            assert_eq!(values, expected);
        }
    }
}

#[test]
fn comparison_named_ranked_reports_keep_independent_binding_and_result_names() {
    for input_csv in [false, true] {
        let (status, output, diagnostics) = run(
            &[
                "-H",
                "-gk",
                "--result-name=3:score",
                "compare",
                "before",
                "after",
                "rank",
                "03",
                "percent",
                "limit",
                "1",
                "sum",
                "v,w",
                "wmean",
                "v:w",
                "sum",
                "v",
                if input_csv { "--csv-in" } else { "-s" },
            ],
            if input_csv {
                b"k,v,w\nA,100,1\nB,10,100\n"
            } else {
                b"k\tv\tw\nA\t100\t1\nB\t10\t100\n"
            },
            if input_csv {
                b"w,k,v\n1,A,130\n100,B,20\n"
            } else {
                b"w\tk\tv\n1\tA\t130\n100\tB\t20\n"
            },
        );
        assert_eq!((status, diagnostics), (0, vec![]));
        let rows: Vec<_> = output
            .split(|b| *b == b'\n')
            .filter(|row| !row.is_empty())
            .collect();
        assert_eq!(rows.len(), 2);
        let labels: Vec<_> = rows[0].split(|b| *b == b'\t').collect();
        assert_eq!(
            &labels[16..22],
            &[
                b"before(score)".as_slice(),
                b"after(score)",
                b"difference(score)",
                b"difference_state(score)",
                b"percentage(score)",
                b"percentage_state(score)"
            ]
        );
        assert_eq!(rows[1], b"B\tmatched\t1\tranked\t10\t20\t10\tavailable\t100\tavailable\t100\t100\t0\tavailable\t0\tavailable\t10\t20\t10\tavailable\t100\tavailable\t10\t20\t10\tavailable\t100\tavailable");
    }
}

#[test]
fn comparison_rank_modifiers_fail_before_source_opening() {
    use super::command_test_support::{Sources, command_sources};
    let cases: &[(&[&str], &[u8])] = &[
        (&["rank"], b"missing comparison modifier value\n"),
        (
            &["rank", "0", "sum", "1"],
            b"comparison modifier value is zero or outside u64 range\n",
        ),
        (
            &["rank", "+1", "sum", "1"],
            b"comparison modifiers require positive unsigned decimal integers\n",
        ),
        (
            &["rank", "-1", "sum", "1"],
            b"comparison modifiers require positive unsigned decimal integers\n",
        ),
        (
            &["rank", "18446744073709551616", "sum", "1"],
            b"comparison modifier value is zero or outside u64 range\n",
        ),
        (
            &["rank", "1"],
            b"compare requires BEFORE AFTER and at least one operation\n",
        ),
        (
            &["rank", "1", "percent"],
            b"compare requires BEFORE AFTER and at least one operation\n",
        ),
        (
            &["rank", "1", "relative", "sum", "1"],
            b"invalid operation 'relative'\n",
        ),
        (
            &["rank", "2", "sum", "1"],
            b"comparison rank result exceeds the number of results\n",
        ),
        (&["limit", "1", "sum", "1"], b"invalid operation 'limit'\n"),
        (&["sum", "1", "rank", "1"], b"invalid operation 'rank'\n"),
        (&["sum", "1", "limit", "1"], b"invalid operation 'limit'\n"),
        (
            &["rank", "1", "absolute", "rank", "1", "sum", "1"],
            b"invalid operation 'rank'\n",
        ),
        (
            &["rank", "1", "limit", "2", "limit", "3", "sum", "1"],
            b"invalid operation 'limit'\n",
        ),
        (
            &["rank", "1", "percent", "absolute", "sum", "1"],
            b"invalid operation 'absolute'\n",
        ),
        (
            &["rank", "1", "limit"],
            b"missing comparison modifier value\n",
        ),
        (
            &["rank", "1", "limit", "0", "sum", "1"],
            b"comparison modifier value is zero or outside u64 range\n",
        ),
        (
            &["rank", "1", "limit", "+1", "sum", "1"],
            b"comparison modifiers require positive unsigned decimal integers\n",
        ),
        (
            &["rank", "1", "limit", "-1", "sum", "1"],
            b"comparison modifiers require positive unsigned decimal integers\n",
        ),
        (
            &["rank", "1", "limit", "18446744073709551616", "sum", "1"],
            b"comparison modifier value is zero or outside u64 range\n",
        ),
        (
            &["rank", "1", "limit", "1", "rank", "1", "sum", "1"],
            b"invalid operation 'rank'\n",
        ),
    ];
    for &(operands, expected) in cases {
        let mut sources = Sources {
            inputs: [
                Input {
                    bytes: b"unread",
                    segment: 1,
                    error: None,
                },
                Input {
                    bytes: b"unread",
                    segment: 1,
                    error: None,
                },
            ],
            closes: [None, None],
            opened: 0,
            completed: 0,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        let mut args = vec!["--", "compare", "before", "after"];
        args.extend_from_slice(operands);
        let (status, diagnostics) = command_sources(&mut sources, &mut output, &args);
        assert_eq!(
            (status, diagnostics),
            (1, vec![expected.to_vec()]),
            "{args:?}"
        );
        assert!(output.bytes.is_empty());
        assert_eq!(sources.opened, 0);
        assert_eq!(sources.inputs[0].bytes, b"unread");
        assert_eq!(sources.inputs[1].bytes, b"unread");
    }
}

#[test]
fn comparison_rank_limit_cannot_hide_late_input_or_unselected_calculation_failures() {
    for (operations, before, after, status, fragment) in [
        (
            vec!["sum", "v"],
            b"k\tv\nA\t1\n".as_slice(),
            b"k\tv\nA\t100\nZ\tbad\n".as_slice(),
            1,
            "line 3",
        ),
        (
            vec!["sum", "v", "wmean", "v:w"],
            b"k\tv\tw\nA\t1\t1\nZ\t1\t1\n",
            b"k\tv\tw\nA\t100\t1\nZ\t1\t0\n",
            1,
            "completing result 2",
        ),
        (
            vec!["sum", "v,w"],
            b"k\tv\tw\nA\t1\t1\nZ\t1\t-0x1p+16383\n",
            b"k\tv\tw\nA\t100\t1\nZ\t1\t0x1p+16383\n",
            77,
            "result 2, subtraction",
        ),
        (
            vec!["min", "v"],
            b"k\tv\nA\t1\nZ\t0x1p-16445\n",
            b"k\tv\nA\tinf\nZ\t1\n",
            77,
            "result 1, percentage division",
        ),
        (
            vec!["sum", "v,w"],
            b"k\tv\tw\nA\t1\t1\nZ\t1\t1\n",
            b"k\tv\tw\nA\t100\t1\nZ\t1\t0x1p+16378\n",
            77,
            "result 2, percentage multiplication",
        ),
    ] {
        let mut args = vec![
            "-H", "-gk", "compare", "before", "after", "rank", "1", "limit", "1",
        ];
        args.extend(operations);
        let (actual, output, diagnostics) = run(&args, before, after);
        assert_eq!(actual, status, "{args:?}: {diagnostics:?}");
        assert!(output.is_empty());
        assert!(
            String::from_utf8_lossy(&diagnostics[0]).contains(fragment),
            "{args:?}: {diagnostics:?}"
        );
    }
}

#[test]
fn comparison_ranked_source_completion_and_output_errors_prevent_success() {
    use super::command_test_support::{Sources, command_sources};
    let args = [
        "--header-out",
        "-g1",
        "compare",
        "before",
        "after",
        "rank",
        "1",
        "limit",
        "1",
        "sum",
        "2",
    ];
    for side in [0, 1] {
        for close in [false, true] {
            let mut sources = Sources {
                inputs: [
                    Input {
                        bytes: b"A\t1\nZ\t1\n",
                        segment: 1,
                        error: None,
                    },
                    Input {
                        bytes: b"A\t100\nZ\t1\n",
                        segment: 1,
                        error: None,
                    },
                ],
                closes: [None, None],
                opened: 0,
                completed: 0,
            };
            if close {
                sources.closes[side] = Some(5);
            } else {
                sources.inputs[side].error = Some(5);
            }
            let mut output = Output {
                bytes: Vec::new(),
                error: None,
                closed: false,
            };
            let (status, diagnostics) = command_sources(&mut sources, &mut output, &args);
            assert_eq!(status, 1);
            assert!(output.bytes.is_empty());
            assert!(output.closed);
            assert!(
                String::from_utf8_lossy(&diagnostics[0]).contains(if side == 0 {
                    "before source"
                } else {
                    "after source"
                })
            );
        }
    }
    let mut output = Output {
        bytes: Vec::new(),
        error: Some(28),
        closed: false,
    };
    let (status, diagnostics) = comparison_command(&args, b"A\t1\n", b"A\t100\n", &mut output);
    assert_eq!(status, 1);
    assert!(output.closed);
    assert!(!diagnostics.is_empty());
}

#[test]
fn comparison_ranked_allocation_failures_refuse_before_header_or_winners() {
    use super::command_memory::FAIL_RESERVATION;
    let args = [
        "--header-out",
        "-g1",
        "compare",
        "before",
        "after",
        "rank",
        "1",
        "percent",
        "limit",
        "1",
        "sum",
        "2",
    ];
    let before = b"A\t1\nZ\t0\nremoved\t4\n";
    let after = b"A\t10\nZ\t1\nadded\t5\n";
    let success = run(&args, before, after);
    assert_eq!(success.0, 0);
    let mut reached_success = false;
    for fail_after in 0..500 {
        FAIL_RESERVATION.with(|slot| slot.set(Some(fail_after)));
        let (status, output, diagnostics) = run(&args, before, after);
        let pending = FAIL_RESERVATION.with(|slot| slot.replace(None));
        if pending.is_some() {
            assert_eq!((status, output, diagnostics), success);
            reached_success = true;
            break;
        }
        assert_eq!(status, 77, "{fail_after}: {diagnostics:?}");
        assert!(output.is_empty(), "{fail_after}: {output:?}");
    }
    assert!(reached_success);
}

#[test]
fn comparison_ranking_keeps_subnormal_changes_ahead_of_exact_cancellation() {
    let before = b"equal\t0x1p-16445\nsubnormal\t0x1p-16382\n";
    let after = b"subnormal\t0x8.000000000000001p-16385\nequal\t0x1p-16445\n";
    for measure in ["absolute", "percent"] {
        assert_eq!(run(&["--format=%a", "-g1", "compare", "before", "after", "rank", "1", measure, "sum", "2"], before, after),
            (0, b"subnormal\tmatched\t1\tranked\t0x8p-16385\t0x8.000000000000001p-16385\t0x0.000000000000001p-16385\tavailable\t0xc.8p-60\tavailable\nequal\tmatched\t2\tranked\t0x0.000000000000001p-16385\t0x0.000000000000001p-16385\t0x0p+0\tavailable\t0x0p+0\tavailable\n".to_vec(), vec![]));
    }
}

#[test]
fn comparison_rank_cutoffs_match_an_independent_integer_and_rational_oracle() {
    // Integer summaries and dyadic ratios make every percentage exactly
    // representable. The oracle uses tuple keys and rational cross-products,
    // without the production key comparator or change arithmetic.
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut oracle = Vec::new();
    for index in 0..24i64 {
        let key = format!("K{index:02}");
        let tag = format!("{}", index % 3);
        let baseline = 8i64 << (index % 4);
        let difference = [-4, -2, -1, 0, 1, 2, 4, 8][index as usize % 8];
        oracle.push(((key.to_ascii_lowercase(), tag), baseline, difference));
    }
    for pass in 0..2 {
        for position in 0..24usize {
            let index = 23 - position;
            let ((key, tag), baseline, _) = &oracle[index];
            before.extend_from_slice(
                format!("{}\t{tag}\t{}\n", key.to_ascii_uppercase(), baseline / 2).as_bytes(),
            );
            let index = (position * 7 + pass * 11) % 24;
            let ((key, tag), baseline, difference) = &oracle[index];
            after.extend_from_slice(
                format!(
                    "{key}\t{tag}\t{}\n",
                    baseline / 2 + if pass == 1 { *difference } else { 0 }
                )
                .as_bytes(),
            );
        }
    }
    before.extend_from_slice(b"Removed\tkey\t4\n");
    after.extend_from_slice(b"Added\tkey\t5\n");
    for measure in ["absolute", "percent"] {
        let mut expected = oracle.clone();
        expected.sort_by(|left, right| {
            let magnitude = if measure == "percent" {
                (left.2.unsigned_abs() * right.1 as u64)
                    .cmp(&(right.2.unsigned_abs() * left.1 as u64))
            } else {
                left.2.unsigned_abs().cmp(&right.2.unsigned_abs())
            };
            magnitude.reverse().then_with(|| left.0.cmp(&right.0))
        });
        for (input_csv, output_csv) in [(false, false), (true, false), (false, true), (true, true)]
        {
            for limit in [1, 7, u64::MAX] {
                let count = (limit.min(expected.len() as u64)) as usize;
                let limit = limit.to_string();
                let before = if input_csv {
                    quoted_csv_fixture(&before)
                } else {
                    before.clone()
                };
                let after = if input_csv {
                    quoted_csv_fixture(&after)
                } else {
                    after.clone()
                };
                let (status, output, diagnostics) = run(
                    &[
                        "-i",
                        "-g1,2",
                        "compare",
                        "before",
                        "after",
                        "rank",
                        "1",
                        measure,
                        "limit",
                        &limit,
                        "sum",
                        "3",
                        if input_csv { "--csv-in" } else { "-s" },
                        if output_csv { "--csv-out" } else { "-s" },
                    ],
                    &before,
                    &after,
                );
                assert_eq!((status, diagnostics), (0, vec![]));
                let rows: Vec<Vec<&[u8]>> = output
                    .split(|b| *b == b'\n')
                    .filter(|row| !row.is_empty())
                    .map(|row| {
                        row.split(|b| *b == if output_csv { b',' } else { b'\t' })
                            .map(|cell| {
                                if cell == b"\"\"" {
                                    b"".as_slice()
                                } else {
                                    cell
                                }
                            })
                            .collect()
                    })
                    .collect();
                assert_eq!(rows.len(), count + 2);
                for (at, (row, ((key, tag), baseline, difference))) in
                    rows.iter().zip(&expected).take(count).enumerate()
                {
                    assert_eq!(
                        (row[0], row[1]),
                        (key.to_ascii_uppercase().as_bytes(), tag.as_bytes())
                    );
                    assert_eq!(row[3], (at + 1).to_string().as_bytes());
                    assert_eq!(row[4], b"ranked");
                    assert_eq!(row[5], baseline.to_string().as_bytes());
                    assert_eq!(row[6], (baseline + difference).to_string().as_bytes());
                    assert_eq!(row[7], difference.to_string().as_bytes());
                }
                assert_eq!(
                    (rows[count][0], rows[count][3], rows[count][4]),
                    (b"Added".as_slice(), b"".as_slice(), b"one_sided".as_slice())
                );
                assert_eq!(
                    (rows[count + 1][0], rows[count + 1][3], rows[count + 1][4]),
                    (
                        b"Removed".as_slice(),
                        b"".as_slice(),
                        b"one_sided".as_slice()
                    )
                );
            }
        }
    }
}

#[test]
fn comparison_headers_bind_named_keys_and_reordered_operands_independently() {
    let expected = b"key(category)\tpresence\trank\trank_state\tbefore(sum(reading))\tafter(sum(reading))\tdifference(sum(reading))\tdifference_state(sum(reading))\tpercentage(sum(reading))\tpercentage_state(sum(reading))\na\tmatched\t\tnot_requested\t4\t8\t4\tavailable\t100\tavailable\nb\tmatched\t\tnot_requested\t6\t3\t-3\tavailable\t-50\tavailable\n";
    assert_eq!(
        run(
            &[
                "-H",
                "-gcategory",
                "compare",
                "before",
                "after",
                "sum",
                "reading"
            ],
            b"category\treading\tweight\nb\t2\t1\na\t4\t1\nb\t4\t1\n",
            b"weight\treading\tcategory\n1\t3\tb\n1\t8\ta\n",
        ),
        (0, expected.to_vec(), vec![])
    );
}

#[test]
fn comparison_result_names_address_expanded_summaries_and_complete_blocks() {
    let expected = b"key(k)\tpresence\trank\trank_state\tbefore(total)\tafter(total)\tdifference(total)\tdifference_state(total)\tpercentage(total)\tpercentage_state(total)\tbefore(sum(v))\tafter(sum(v))\tdifference(sum(v))\tdifference_state(sum(v))\tpercentage(sum(v))\tpercentage_state(sum(v))\tbefore(total)\tafter(total)\tdifference(total)\tdifference_state(total)\tpercentage(total)\tpercentage_state(total)\tbefore(wmean(w,w))\tafter(wmean(w,w))\tdifference(wmean(w,w))\tdifference_state(wmean(w,w))\tpercentage(wmean(w,w))\tpercentage_state(wmean(w,w))\nz\tmatched\t\tnot_requested\t1\t6\t5\tavailable\t500\tavailable\t2\t2\t0\tavailable\t0\tavailable\t2\t6\t4\tavailable\t200\tavailable\t1\t6\t5\tavailable\t500\tavailable\n";
    assert_eq!(
        run(
            &[
                "-H",
                "-gk",
                "--result-name=3:total",
                "--result-name=1:total",
                "compare",
                "before",
                "after",
                "sum",
                "3,2",
                "wmean",
                "v:3,3:w"
            ],
            b"k\tv\tw\nz\t2\t1\n",
            b"k\tw\tv\nz\t2\t6\n",
        ),
        (0, expected.to_vec(), vec![])
    );
}

#[test]
fn comparison_preferred_before_labels_do_not_validate_unused_after_labels() {
    assert_eq!(
        run(
            &["-H", "compare", "before", "after", "sum", "3"],
            b"a\tb\tv\n0\t0\t2\n",
            b"a\tb\n0\t0\t4\n",
        ),
        (0, b"presence\trank\trank_state\tbefore(sum(v))\tafter(sum(v))\tdifference(sum(v))\tdifference_state(sum(v))\tpercentage(sum(v))\tpercentage_state(sum(v))\nmatched\t\tnot_requested\t2\t4\t2\tavailable\t100\tavailable\n".to_vec(), vec![])
    );
}

#[test]
fn comparison_header_controls_are_explicit_and_empty_reports_keep_the_schema() {
    let named_header = b"presence\trank\trank_state\tbefore(sum(v))\tafter(sum(v))\tdifference(sum(v))\tdifference_state(sum(v))\tpercentage(sum(v))\tpercentage_state(sum(v))\n";
    assert_eq!(
        run(
            &["-H", "compare", "before", "after", "sum", "v"],
            b"v\n",
            b"v\n"
        ),
        (0, named_header.to_vec(), vec![])
    );
    assert_eq!(
        run(&["--header-out", "-g1", "compare", "before", "after", "sum", "2"], b"", b""),
        (0, b"key(field-1)\tpresence\trank\trank_state\tbefore(sum(field-2))\tafter(sum(field-2))\tdifference(sum(field-2))\tdifference_state(sum(field-2))\tpercentage(sum(field-2))\tpercentage_state(sum(field-2))\n".to_vec(), vec![])
    );
    let row = b"matched\t\tnot_requested\t2\t4\t2\tavailable\t100\tavailable\n";
    assert_eq!(
        run(
            &[
                "-C",
                "--header-in",
                "compare",
                "before",
                "after",
                "sum",
                "v"
            ],
            b"# skip\nv\n# skip\n2\n",
            b"; skip\nv\n4\n"
        ),
        (0, row.to_vec(), vec![])
    );
    assert_eq!(
        run(&["compare", "before", "after", "sum", "1"], b"2\n", b"4\n"),
        (0, row.to_vec(), vec![])
    );
    // A header-only side stays absent, while the other side establishes data.
    let (status, output, diagnostics) = run(
        &["-H", "compare", "before", "after", "sum", "v"],
        b"v\n",
        b"v\n4\n",
    );
    assert_eq!((status, diagnostics), (0, vec![]));
    assert_eq!(&output[..named_header.len()], named_header);
    assert_eq!(
        &output[named_header.len()..],
        b"added\t\tnot_requested\t\t4\t\tnot_matched\t\tnot_matched\n"
    );
}

#[test]
fn comparison_required_headers_names_and_preferred_labels_fail_with_source_roles() {
    for (before, after, role, fragment) in [
        (
            b"".as_slice(),
            b"k\tv\n".as_slice(),
            "before",
            "requires an input header",
        ),
        (b"k\tv\n", b"", "after", "requires an input header"),
        (b"k\tx\n", b"k\tv\n", "before", "column name 'v'"),
        (b"k\tv\n", b"k\tx\n", "after", "column name 'v'"),
        (b"x\tv\n", b"k\tv\n", "before", "column name 'k'"),
        (b"k\tv\n", b"x\tv\n", "after", "column name 'k'"),
    ] {
        let (status, output, diagnostics) = run(
            &["-H", "-gk", "compare", "before", "after", "sum", "v"],
            before,
            after,
        );
        assert_eq!(status, 1);
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        assert!(diagnostic.contains(fragment), "{diagnostic}");
        assert!(
            diagnostic.contains(&format!("{role} source '{role}'")),
            "{diagnostic}"
        );
    }
    let (status, output, diagnostics) = run(
        &["-H", "compare", "before", "after", "sum", "3"],
        b"a\tb\n0\t0\t2\n",
        b"a\tb\tc\n0\t0\t4\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    assert_eq!(diagnostics, [b"invalid input: field 3 requested, line 1 has only 2 fields\ncompare: before source 'before'\n".to_vec()]);
    assert_eq!(
        run(
            &["--header-in", "compare", "before", "after", "sum", "3"],
            b"a\tb\n0\t0\t2\n",
            b"a\tb\n0\t0\t4\n"
        ),
        (
            0,
            b"matched\t\tnot_requested\t2\t4\t2\tavailable\t100\tavailable\n".to_vec(),
            vec![]
        )
    );
}

#[test]
fn comparison_escaped_duplicate_empty_and_nul_labels_preserve_complete_keys() {
    let expected = b"key(kind)\tpresence\trank\trank_state\tbefore(sum(metric,one))\tafter(sum(metric,one))\tdifference(sum(metric,one))\tdifference_state(sum(metric,one))\tpercentage(sum(metric,one))\tpercentage_state(sum(metric,one))\tbefore(sum())\tafter(sum())\tdifference(sum())\tdifference_state(sum())\tpercentage(sum())\tpercentage_state(sum())\nx\0A\tmatched\t\tnot_requested\t2\t3\t1\tavailable\t50\tavailable\t1\t999\t998\tavailable\t99800\tavailable\nx\0B\tremoved\t\tnot_requested\t4\t\t\tnot_matched\t\tnot_matched\t1\t\t\tnot_matched\t\tnot_matched\n";
    assert_eq!(
        run(
            &[
                "-H",
                "-i",
                "-gkind",
                "compare",
                "before",
                "after",
                "sum",
                "metric\\,one,4"
            ],
            b"kind\0hidden\tmetric,one\tmetric,one\t\nx\0A\t2\t999\t1\nx\0B\t4\t888\t1\n",
            b"metric,one\t\tkind\0extra\tmetric,one\n3\t1\tx\0a\t999\n",
        ),
        (0, expected.to_vec(), vec![])
    );
}

#[test]
fn comparison_mixed_keys_keep_named_mapping_and_positional_meaning() {
    assert_eq!(
        run(
            &["-H", "-gk,1", "compare", "before", "after", "sum", "v"],
            b"k\tv\tw\na\t2\t1\n",
            b"v\tw\tk\n3\t1\ta\n",
        ),
        (0, b"key(k)\tkey(k)\tpresence\trank\trank_state\tbefore(sum(v))\tafter(sum(v))\tdifference(sum(v))\tdifference_state(sum(v))\tpercentage(sum(v))\tpercentage_state(sum(v))\na\t3\tadded\t\tnot_requested\t\t3\t\tnot_matched\t\tnot_matched\na\ta\tremoved\t\tnot_requested\t2\t\t\tnot_matched\t\tnot_matched\n".to_vec(), vec![])
    );
}

#[test]
fn comparison_generated_headers_keep_range_and_parameter_expansion_order() {
    assert_eq!(
        run(
            &["--header-out", "compare", "before", "after", "sum", "1-2", "perc:50", "2"],
            b"", b"",
        ),
        (0, b"presence\trank\trank_state\tbefore(sum(field-1))\tafter(sum(field-1))\tdifference(sum(field-1))\tdifference_state(sum(field-1))\tpercentage(sum(field-1))\tpercentage_state(sum(field-1))\tbefore(sum(field-2))\tafter(sum(field-2))\tdifference(sum(field-2))\tdifference_state(sum(field-2))\tpercentage(sum(field-2))\tpercentage_state(sum(field-2))\tbefore(perc:50(field-2))\tafter(perc:50(field-2))\tdifference(perc:50(field-2))\tdifference_state(perc:50(field-2))\tpercentage(perc:50(field-2))\tpercentage_state(perc:50(field-2))\n".to_vec(), vec![])
    );
}

#[test]
fn comparison_labeled_reports_keep_formatting_and_original_operand_locations() {
    assert_eq!(
        run(
            &["-H", "--form=%.1f", "--output-delimiter=|", "--result-name=1:total", "compare", "before", "after", "sum", "v"],
            b"v\n2\n", b"v\n4\n",
        ),
        (0, b"presence|rank|rank_state|before(total)|after(total)|difference(total)|difference_state(total)|percentage(total)|percentage_state(total)\nmatched||not_requested|2.0|4.0|2.0|available|100.0|available\n".to_vec(), vec![])
    );
    let (status, output, diagnostics) = run(
        &["-C", "-H", "-gk", "compare", "before", "after", "sum", "v"],
        b"k\tv\nz\t2\n",
        b"# skip\nv\tk\n4\tz\n; skip\nbad\tz\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
    assert!(
        diagnostic.contains("field 1") && diagnostic.contains("line 3"),
        "{diagnostic}"
    );
    assert!(diagnostic.contains("after source 'after'"), "{diagnostic}");
    let (status, output, diagnostics) = run(
        &["-C", "-H", "-gk", "compare", "before", "after", "sum", "v"],
        b"k\tv\nz\t2\n",
        b"# skip\nv\tk\n4\tz\n; skip\n3\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    assert_eq!(diagnostics, [b"invalid input: field 2 requested, line 3 has only 1 fields\ncompare: extracting key at line 3\ncompare: after source 'after'\n".to_vec()]);
}

#[test]
fn comparison_labeled_reports_validate_expanded_result_name_targets() {
    for args in [
        vec!["--result-name=1:total"],
        vec!["--header-out", "--result-name=3:total"],
        vec!["--header-out", "--result-name=1:"],
        vec![
            "--header-out",
            "--result-name=1:total",
            "--result-name=1:again",
        ],
        vec!["--header-out", "--result-name=1:bad\tlabel"],
    ] {
        let mut args = args;
        args.extend(["compare", "before", "after", "sum", "1-2"]);
        let (status, output, diagnostics) = run(&args, b"1\t2\n", b"1\t2\n");
        assert_eq!(status, 1, "{args:?}: {diagnostics:?}");
        assert!(output.is_empty());
        assert!(!diagnostics.is_empty());
    }
}

#[test]
fn comparison_keys_consolidate_interleaved_records_and_align_the_union() {
    let expected = b"a\tmatched\t\tnot_requested\t4\t6\t2\tavailable\t50\tavailable\nb\tremoved\t\tnot_requested\t2\t\t\tnot_matched\t\tnot_matched\nc\tadded\t\tnot_requested\t\t5\t\tnot_matched\t\tnot_matched\n";
    for sorting in [false, true] {
        let mut args = vec!["-g1", "compare", "before", "after", "sum", "2"];
        if sorting {
            args.push("-s");
        }
        assert_eq!(
            run(&args, b"a\t1\nb\t2\na\t3\n", b"c\t5\na\t2\na\t4\n"),
            (0, expected.to_vec(), vec![])
        );
    }
}

#[test]
fn comparison_complete_key_tuples_keep_bytes_boundaries_and_canonical_order() {
    // Independent literal order over complete fields, including both sides of NUL.
    let keys: &[(&[u8], &[u8])] = &[
        (b"", b""),
        (b" a", b"x"),
        (b"01", b"x"),
        (b"1", b"x"),
        (b"a", b""),
        (b"a", b"b"),
        (b"a", b"bc"),
        (b"a ", b"b"),
        (b"ab", b"c"),
        (b"x\0a", b"z"),
        (b"x\0b", b"z"),
        (b"\x80", b"z"),
        (b"\xff", b"z"),
    ];
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut expected = Vec::new();
    for &(left, right) in keys.iter().rev() {
        for _ in 0..2 {
            before.extend_from_slice(left);
            before.push(b'\t');
            before.extend_from_slice(right);
            before.extend_from_slice(b"\t1\n");
        }
    }
    for &(left, right) in keys {
        after.extend_from_slice(left);
        after.push(b'\t');
        after.extend_from_slice(right);
        after.extend_from_slice(b"\t1\n");
        expected.extend_from_slice(left);
        expected.push(b'\t');
        expected.extend_from_slice(right);
        expected.extend_from_slice(
            b"\tmatched\t\tnot_requested\t2\t1\t-1\tavailable\t-50\tavailable\n",
        );
    }
    assert_eq!(
        run(
            &["-g1,2", "compare", "before", "after", "count", "3"],
            &before,
            &after
        ),
        (0, expected, vec![])
    );
}

#[test]
fn comparison_ascii_folding_retains_the_first_before_or_after_spelling() {
    let before = b"Z\t1\nx\0A\t1\n\xc3\x84\t1\nz\t1\n";
    let after = b"q\t1\nQ\t1\nx\0a\t1\nz\t1\n\xc3\xa4\t1\n";
    let expected = b"q\tadded\t\tnot_requested\t\t2\t\tnot_matched\t\tnot_matched\nx\0A\tmatched\t\tnot_requested\t1\t1\t0\tavailable\t0\tavailable\nZ\tmatched\t\tnot_requested\t2\t1\t-1\tavailable\t-50\tavailable\n\xc3\x84\tremoved\t\tnot_requested\t1\t\t\tnot_matched\t\tnot_matched\n\xc3\xa4\tadded\t\tnot_requested\t\t1\t\tnot_matched\t\tnot_matched\n";
    assert_eq!(
        run(
            &["-i", "-g1", "compare", "before", "after", "count", "2"],
            before,
            after
        ),
        (0, expected.to_vec(), vec![])
    );
}

#[test]
fn comparison_per_key_weighted_order_survives_unrelated_key_reordering_and_sharing() {
    let before = b"a\t18446744073709551616\t1\nb\t18446744073709551616\t1\na\t1\t1\nb\t-18446744073709551616\t1\na\t-18446744073709551616\t1\nb\t1\t1\n";
    let after = b"b\t18446744073709551616\t1\nb\t-18446744073709551616\t1\na\t18446744073709551616\t1\nb\t1\t1\na\t1\t1\na\t-18446744073709551616\t1\n";
    let mut expected = Vec::new();
    for (key, weighted) in [
        (b"a".as_slice(), b"0".as_slice()),
        (b"b", b"0.33333333333333"),
    ] {
        expected.extend_from_slice(key);
        expected.extend_from_slice(b"\tmatched\t\tnot_requested");
        for _ in 0..2 {
            expected.push(b'\t');
            expected.extend_from_slice(weighted);
            expected.push(b'\t');
            expected.extend_from_slice(weighted);
            expected.extend_from_slice(if key == b"a" {
                b"\t0\tavailable\t\tzero_baseline".as_slice()
            } else {
                b"\t0\tavailable\t0\tavailable".as_slice()
            });
        }
        let total = if key == b"a" {
            b"0".as_slice()
        } else {
            b"1".as_slice()
        };
        for value in [total, weighted, weighted] {
            expected.push(b'\t');
            expected.extend_from_slice(value);
            expected.push(b'\t');
            expected.extend_from_slice(value);
            expected.extend_from_slice(if key == b"a" {
                b"\t0\tavailable\t\tzero_baseline".as_slice()
            } else {
                b"\t0\tavailable\t0\tavailable".as_slice()
            });
        }
        for _ in 0..2 {
            expected.extend_from_slice(b"\t1\t1\t0\tavailable\t0\tavailable");
        }
        expected.push(b'\n');
    }
    for input_csv in [false, true] {
        let mut args = vec![
            "-s", "-g1", "compare", "before", "after", "wmean", "2:3", "wmean", "2:3", "sum", "2",
            "mean", "2", "mean", "2", "median", "2", "median", "2",
        ];
        if input_csv {
            args.push("--csv-in");
        }
        let before = if input_csv {
            quoted_csv_fixture(before)
        } else {
            before.to_vec()
        };
        let after = if input_csv {
            quoted_csv_fixture(after)
        } else {
            after.to_vec()
        };
        assert_eq!(run(&args, &before, &after), (0, expected.clone(), vec![]));
    }
}

#[test]
fn comparison_keys_exist_before_omission_and_missing_keys_are_errors() {
    assert_eq!(
        run(&["--narm", "-g1", "compare", "before", "after", "sum", "2"], b"\tNA\nb\tN/A\n", b"\tNaN\nc\tNA\n"),
        (0, b"\tmatched\t\tnot_requested\t0\t0\t0\tavailable\t\tzero_baseline\nb\tremoved\t\tnot_requested\t0\t\t\tnot_matched\t\tnot_matched\nc\tadded\t\tnot_requested\t\t0\t\tnot_matched\t\tnot_matched\n".to_vec(), vec![])
    );
    for (args, before, after, context) in [
        (
            vec!["--narm", "-g3", "compare", "before", "after", "sum", "1"],
            b"NA\t1\n".as_slice(),
            b"".as_slice(),
            "field 3 requested",
        ),
        (
            vec![
                "--narm", "-g1", "compare", "before", "after", "sum", "2", "wmean", "2:3",
            ],
            b"b\tNA\t1\n",
            b"",
            "key ('b')",
        ),
        (
            vec!["-g1", "compare", "before", "after", "sum", "2"],
            b"a\t1\n",
            b"a\t2\nz\tbad\n",
            "key ('z')",
        ),
    ] {
        let (status, output, diagnostics) = run(&args, before, after);
        assert_eq!(status, 1, "{diagnostics:?}");
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        assert!(diagnostic.contains(context), "{diagnostic}");
        assert!(diagnostic.contains("source"), "{diagnostic}");
    }
}

#[test]
fn comparison_later_key_arithmetic_failures_prevent_earlier_rows() {
    let (status, output, diagnostics) = run(
        &[
            "--header-out",
            "-g1",
            "compare",
            "before",
            "after",
            "sum",
            "2",
        ],
        b"a\t1\nz\t-0x1p+16383\n",
        b"a\t2\nz\t0x1p+16383\n",
    );
    assert_eq!(status, 77);
    assert!(output.is_empty());
    let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
    for context in [
        "key ('z')",
        "result 1",
        "subtraction",
        "before source",
        "after source",
    ] {
        assert!(diagnostic.contains(context), "{diagnostic}");
    }
}

#[test]
fn comparison_wide_key_selectors_refresh_field_locations_for_every_record() {
    assert_eq!(
        run(
            &["-g1,5,9", "compare", "before", "after", "sum", "10"],
            b"long\tx\tx\tx\tb\tx\tx\tx\tc\t1\na\tx\tx\tx\tdifferent\tx\tx\tx\te\t2\nlong\tx\tx\tx\tb\tx\tx\tx\tc\t3\n",
            b"a\tx\tx\tx\tdifferent\tx\tx\tx\te\t4\nlong\tx\tx\tx\tb\tx\tx\tx\tc\t8\n",
        ),
        (0, b"a\tdifferent\te\tmatched\t\tnot_requested\t2\t4\t2\tavailable\t100\tavailable\nlong\tb\tc\tmatched\t\tnot_requested\t4\t8\t4\tavailable\t100\tavailable\n".to_vec(), vec![])
    );
}

#[test]
fn comparison_small_integer_oracle_covers_repeated_unique_and_one_sided_tuples() {
    // The oracle owns a two-field tuple and exact integer sums, independently
    // of the production key representation, state and comparator.
    use std::collections::BTreeMap;
    let fixture: &[(&[u8], &[u8])] = &[
        (b"", b""),
        (b"a", b"bc"),
        (b"ab", b"c"),
        (b"01", b"x"),
        (b"1", b"x"),
        (b"x\0a", b"y"),
        (b"x\0b", b"y"),
        (b"\xff", b"z"),
    ];
    let mut entries = Vec::new();
    let mut seed = 19usize;
    for index in 0..48usize {
        seed = (seed * 37 + 11) % 101;
        let (left, right) = fixture[seed % fixture.len()];
        entries.push((left.to_vec(), right.to_vec(), 1 + index as u64 % 7));
    }
    for index in 0..12 {
        entries.push((
            format!("unique{index:02}").into_bytes(),
            b"only".to_vec(),
            3,
        ));
    }
    let mut oracle: BTreeMap<(Vec<u8>, Vec<u8>), (u64, u64)> = BTreeMap::new();
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (side, input) in [(0, &mut before), (1, &mut after)] {
        for (left, right, value) in if side == 0 {
            entries.iter().collect::<Vec<_>>()
        } else {
            entries.iter().rev().collect::<Vec<_>>()
        } {
            let value = value * (1 + side as u64);
            input.extend_from_slice(left);
            input.push(b'\t');
            input.extend_from_slice(right);
            input.extend_from_slice(format!("\t{value}\n").as_bytes());
            let sums = oracle.entry((left.clone(), right.clone())).or_default();
            if side == 0 {
                sums.0 += value;
            } else {
                sums.1 += value;
            }
        }
    }
    before.extend_from_slice(b"removed\tkey\t7\n");
    after.extend_from_slice(b"added\tkey\t5\n");
    oracle.insert((b"removed".to_vec(), b"key".to_vec()), (7, 0));
    oracle.insert((b"added".to_vec(), b"key".to_vec()), (0, 5));
    let mut expected = Vec::new();
    for ((left, right), (before, after)) in oracle {
        expected.extend_from_slice(&left);
        expected.push(b'\t');
        expected.extend_from_slice(&right);
        expected.extend_from_slice(match (before, after) {
            (0, after) => format!("\tadded\t\tnot_requested\t\t{after}\t\tnot_matched\t\tnot_matched\n"),
            (before, 0) => format!("\tremoved\t\tnot_requested\t{before}\t\t\tnot_matched\t\tnot_matched\n"),
            (before, after) => format!("\tmatched\t\tnot_requested\t{before}\t{after}\t{}\tavailable\t100\tavailable\n", after - before),
        }.as_bytes());
    }
    for (input_csv, output_csv) in [(false, false), (true, false), (false, true), (true, true)] {
        let mut args = vec!["-g1,2", "compare", "before", "after", "sum", "3"];
        if input_csv {
            args.push("--csv-in");
        }
        if output_csv {
            args.push("--csv-out");
        }
        let before = if input_csv {
            quoted_csv_fixture(&before)
        } else {
            before.clone()
        };
        let after = if input_csv {
            quoted_csv_fixture(&after)
        } else {
            after.clone()
        };
        let expected = if output_csv {
            plain_csv_report(&expected)
        } else {
            expected.clone()
        };
        assert_eq!(run(&args, &before, &after), (0, expected, vec![]));
    }
}

#[test]
fn comparison_keyed_allocation_failures_refuse_before_any_report_bytes() {
    use super::command_memory::FAIL_RESERVATION;
    let args = ["-g1,2", "compare", "before", "after", "sum", "3"];
    let before = b"a\tbc\t1\nab\tc\t2\na\tbc\t3\nx\0a\ty\t4\n";
    let after = b"x\0b\ty\t5\na\tbc\t8\nx\0a\ty\t8\n";
    let success = run(&args, before, after);
    assert_eq!(success.0, 0);
    let mut reached_success = false;
    for fail_after in 0..500 {
        FAIL_RESERVATION.with(|slot| slot.set(Some(fail_after)));
        let (status, output, diagnostics) = run(&args, before, after);
        let pending = FAIL_RESERVATION.with(|slot| slot.replace(None));
        if pending.is_some() {
            assert_eq!((status, output, diagnostics), success);
            reached_success = true;
            break;
        }
        assert_eq!(status, 77, "{fail_after}: {diagnostics:?}");
        assert!(output.is_empty(), "{fail_after}: {output:?}");
    }
    assert!(reached_success);
}

#[test]
fn comparison_binary80_trace_and_subnormal_results_follow_independent_expectations() {
    // Derived with the retained exact-v3 expectation model, whose frozen
    // manifest identity covers primitive-finite.csv. These spellings pin each
    // rounded step rather than a rounded display or a binary64 calculation.
    for (before, after, block) in [
        (
            "3\n",
            "5\n",
            "0xcp-2\t0xap-1\t0x8p-2\tavailable\t0x8.555555555555556p+3\tavailable",
        ),
        (
            "3\n",
            "0x1p+65\n",
            "0xcp-2\t0x8p+62\t0xf.ffffffffffffffep+61\tavailable\t0x8.555555555555554p+67\tavailable",
        ),
        (
            "9007199254740992\n",
            "9007199254740993\n",
            "0x8p+50\t0x8.0000000000004p+50\t0x8p-3\tavailable\t0xc.8p-50\tavailable",
        ),
        (
            "0x1p-16382\n",
            "0x8.000000000000001p-16385\n",
            "0x8p-16385\t0x8.000000000000001p-16385\t0x0.000000000000001p-16385\tavailable\t0xc.8p-60\tavailable",
        ),
        (
            "0x1p-16445\n",
            "0x1p-16445\n",
            "0x0.000000000000001p-16385\t0x0.000000000000001p-16385\t0x0p+0\tavailable\t0x0p+0\tavailable",
        ),
    ] {
        let expected = format!("matched\t\tnot_requested\t{block}\n").into_bytes();
        assert_eq!(
            run(
                &["--format=%a", "compare", "before", "after", "sum", "1"],
                before.as_bytes(),
                after.as_bytes()
            ),
            (0, expected, vec![])
        );
    }
    assert_eq!(
        run(
            &["compare", "before", "after", "count", "1"],
            b"x\n",
            b"x\ny\n"
        ),
        (
            0,
            b"matched\t\tnot_requested\t1\t2\t1\tavailable\t100\tavailable\n".to_vec(),
            vec![]
        )
    );
}

#[test]
fn comparison_finite_overflow_refuses_with_stage_and_result_context() {
    for (before, after, stage) in [
        ("-0x1p+16383\n", "0x1p+16383\n", "subtraction"),
        ("0x1p-16445\n", "1\n", "percentage division"),
        ("1\n", "0x1p+16378\n", "percentage multiplication"),
    ] {
        let (status, output, diagnostics) = run(
            &["compare", "before", "after", "sum", "1"],
            before.as_bytes(),
            after.as_bytes(),
        );
        assert_eq!(status, 77, "{diagnostics:?}");
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        assert!(diagnostic.contains(stage), "{diagnostic}");
        assert!(diagnostic.contains("result 1"), "{diagnostic}");
        assert!(diagnostic.contains("before source") && diagnostic.contains("after source"));
    }
}

#[test]
fn comparison_source_completion_and_output_faults_prevent_success() {
    use super::command_test_support::{Sources, command_sources};
    for side in [0, 1] {
        for close in [false, true] {
            let mut sources = Sources {
                inputs: [
                    Input {
                        bytes: b"1\n",
                        segment: 1,
                        error: None,
                    },
                    Input {
                        bytes: b"2\n",
                        segment: 1,
                        error: None,
                    },
                ],
                closes: [None, None],
                opened: 0,
                completed: 0,
            };
            if close {
                sources.closes[side] = Some(5);
            } else {
                sources.inputs[side].error = Some(5);
            }
            let mut output = Output {
                bytes: Vec::new(),
                error: None,
                closed: false,
            };
            let (status, diagnostics) = command_sources(
                &mut sources,
                &mut output,
                &["compare", "before", "after", "sum", "1"],
            );
            assert_eq!(status, 1);
            assert!(output.bytes.is_empty());
            assert!(output.closed);
            assert_eq!(sources.opened, side + 1);
            assert_eq!(sources.completed, side + 1);
            assert!(
                String::from_utf8_lossy(&diagnostics[0]).contains(if side == 0 {
                    "before source"
                } else {
                    "after source"
                })
            );
            assert!(!String::from_utf8_lossy(&diagnostics[0]).contains("line"));
        }
    }
    let mut output = Output {
        bytes: Vec::new(),
        error: Some(28),
        closed: false,
    };
    let (status, diagnostics) = comparison_command(
        &["compare", "before", "after", "sum", "1"],
        b"1\n",
        b"2\n",
        &mut output,
    );
    assert_eq!(status, 1);
    assert!(output.closed);
    assert!(!diagnostics.is_empty());
}

#[test]
fn comparison_allocation_failures_are_checked_before_output() {
    use super::command_memory::FAIL_RESERVATION;
    let mut reached_success = false;
    for fail_after in 0..150 {
        FAIL_RESERVATION.with(|slot| slot.set(Some(fail_after)));
        let (status, output, diagnostics) = run(
            &[
                "--header-out",
                "--result-name=1:total",
                "compare",
                "before",
                "after",
                "sum",
                "1",
            ],
            b"1\n",
            b"2\n",
        );
        let pending = FAIL_RESERVATION.with(|slot| slot.replace(None));
        if pending.is_some() {
            assert_eq!(status, 0);
            reached_success = true;
            break;
        }
        assert_eq!(status, 77, "{fail_after}: {diagnostics:?}");
        assert!(output.is_empty(), "{fail_after}: {output:?}");
    }
    assert!(reached_success);
}

#[test]
fn comparison_label_refusal_keeps_later_header_field_failure_primary() {
    use super::command_memory::FAIL_RESERVATION;
    for csv in [false, true] {
        let mut args = vec![
            "-H",
            "--result-name=1:total",
            "--result-name=2:paired",
            "compare",
            "before",
            "after",
            "sum",
            "1",
            "dotprod",
            "1:2",
        ];
        if csv {
            args.insert(0, "--csv");
        }
        let missing = b"invalid input: field 2 requested, line 1 has only 1 fields\n";
        let mut combined_failure = false;
        let mut reached_validation = false;
        for fail_after in 0..150 {
            FAIL_RESERVATION.with(|slot| slot.set(Some(fail_after)));
            let (status, output, diagnostics) = run(&args, b"v\n", b"v\n");
            let pending = FAIL_RESERVATION.with(|slot| slot.replace(None));
            assert!(output.is_empty());
            assert_eq!(diagnostics.len(), 1);
            if pending.is_some() {
                assert_eq!(status, 1);
                assert!(diagnostics[0].starts_with(missing));
                reached_validation = true;
                break;
            }
            assert_eq!(status, 77, "{fail_after}: {diagnostics:?}");
            let allocation = b"command memory allocation failed\n";
            if diagnostics[0].starts_with(missing) {
                assert!(diagnostics[0][missing.len()..].starts_with(allocation));
                combined_failure = true;
            }
        }
        assert!(combined_failure && reached_validation);
    }
}

#[test]
fn comparison_output_flush_and_close_failures_are_completed() {
    struct Finalization {
        inner: Output,
        close: Option<i32>,
    }
    impl std::io::Write for Finalization {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.inner.write(bytes)
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl super::command_output::Transport for Finalization {
        fn buffering(&self) -> (usize, bool) {
            // The entire row fits, so the first write is the pending-byte
            // transfer during Command output completion.
            (8192, false)
        }
        fn close(&mut self) -> std::io::Result<()> {
            self.inner.closed = true;
            self.close
                .map_or(Ok(()), |code| Err(std::io::Error::from_raw_os_error(code)))
        }
    }
    for csv in [false, true] {
        for (flush, close) in [(Some(28), None), (None, Some(28)), (Some(5), Some(28))] {
            let mut output = Finalization {
                inner: Output {
                    bytes: Vec::new(),
                    error: flush,
                    closed: false,
                },
                close,
            };
            let (status, diagnostics) = comparison_command(
                &[
                    "--header-out",
                    "--result-name=1:total",
                    "compare",
                    "before",
                    "after",
                    "sum",
                    "1",
                    if csv { "--csv" } else { "-s" },
                ],
                b"1\n",
                b"2\n",
                &mut output,
            );
            assert_ne!(status, 0);
            assert!(output.inner.closed);
            assert!(!diagnostics.is_empty());
        }
    }
}

#[test]
fn comparison_recognized_modes_and_explicit_controls_refuse_before_reading() {
    for mode in [
        "reverse",
        "noop",
        "transpose",
        "check",
        "rmdup",
        "health",
        "crosstab 1,2",
        "top:1 1",
        "bottom:1 1",
        "groupby 1 sum 2",
    ] {
        let (status, output, diagnostics) =
            run(&["compare", "before", "after", mode], b"bad", b"bad");
        assert_eq!(status, 77, "{mode}: {diagnostics:?}");
        assert!(output.is_empty());
    }
    for control in [
        "--vnlog",
        "--no-strict",
        "--collapse-delimiter=,",
        "--seed=1",
    ] {
        let (status, output, diagnostics) = run(
            &[control, "compare", "before", "after", "sum", "1"],
            b"bad",
            b"bad",
        );
        assert_eq!(status, 77, "{control}: {diagnostics:?}");
        assert!(output.is_empty());
    }
    for args in [
        vec![
            "--csv-out",
            "--output-delimiter=|",
            "compare",
            "before",
            "after",
            "sum",
            "1",
        ],
        vec!["--csv", "-z", "compare", "before", "after", "sum", "1"],
        vec!["compare", "before", "after", "top:0", "1"],
        vec!["compare", "before", "after", "limit", "1", "sum", "1"],
    ] {
        let (status, output, diagnostics) = run(&args, b"bad", b"bad");
        assert_eq!(status, 1, "{args:?}: {diagnostics:?}");
        assert!(output.is_empty());
    }
}

#[test]
fn comparison_expansion_parameters_and_paired_omission_keep_result_order() {
    assert_eq!(run(&["compare", "before", "after", "sum", "1-2", "perc:50", "1", "wmean", "1:2", "mean", "1"], b"1\t1\n3\t1\n", b"2\t1\n6\t1\n"),
        (0, b"matched\t\tnot_requested\t4\t8\t4\tavailable\t100\tavailable\t2\t2\t0\tavailable\t0\tavailable\t2\t4\t2\tavailable\t100\tavailable\t2\t4\t2\tavailable\t100\tavailable\t2\t4\t2\tavailable\t100\tavailable\n".to_vec(), vec![]));
    assert_eq!(run(&["--narm", "compare", "before", "after", "dotprod", "1:2", "wmean", "1:2"], b"1\tNA\nNA\t2\n3\t4\n", b"2\tNA\nNA\t2\n6\t4\n"),
        (0, b"matched\t\tnot_requested\t14\t28\t14\tavailable\t100\tavailable\t3\t6\t3\tavailable\t100\tavailable\n".to_vec(), vec![]));
    let (status, output, diagnostics) = run(
        &["--narm", "compare", "before", "after", "wmean", "1:2"],
        b"1\t1\n",
        b"NA\t-1\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    assert!(!diagnostics.is_empty());
}

#[test]
fn comparison_admits_every_numerical_aggregate_and_existing_aliases() {
    let mut args = vec!["compare", "before", "after"];
    for operation in [
        "count",
        "countunique",
        "sum",
        "mean",
        "geomean",
        "harmmean",
        "ms",
        "rms",
        "min",
        "max",
        "absmin",
        "absmax",
        "range",
        "median",
        "mode",
        "antimode",
        "q1",
        "q3",
        "iqr",
        "perc:25",
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
    ] {
        args.extend([operation, "1"]);
    }
    for operation in ["wmean", "pcov", "scov", "ppearson", "spearson", "dotprod"] {
        args.extend([operation, "1:2"]);
    }
    let bytes = b"1\t1\n2\t2\n3\t3\n4\t4\n5\t5\n6\t6\n7\t7\n8\t8\n";
    for input_csv in [false, true] {
        let bytes = if input_csv {
            quoted_csv_fixture(bytes)
        } else {
            bytes.to_vec()
        };
        let mut options = args.clone();
        if input_csv {
            options.push("--csv-in");
        }
        let (status, output, diagnostics) = run(&options, &bytes, &bytes);
        assert_eq!(status, 0, "{diagnostics:?}");
        assert!(diagnostics.is_empty());
        let columns: Vec<_> = output
            .strip_suffix(b"\n")
            .unwrap()
            .split(|byte| *byte == b'\t')
            .collect();
        assert_eq!(columns.len(), 3 + (args.len() - 3) / 2 * 6);
        for block in columns[3..].as_chunks::<6>().0 {
            assert_eq!(block[0], block[1]);
            assert_eq!(block[2], b"0");
            assert_eq!(block[3], b"available");
        }
    }
}

fn run(args: &[&str], before: &[u8], after: &[u8]) -> (i32, Vec<u8>, Vec<Vec<u8>>) {
    let mut output = Output {
        bytes: Vec::new(),
        error: None,
        closed: false,
    };
    let (status, diagnostics) = comparison_command(args, before, after, &mut output);
    (status, output.bytes, diagnostics)
}

#[test]
fn comparison_signed_changes_use_the_signed_before_value() {
    for (before, after, expected) in [
        (
            b"100\n".as_slice(),
            b"130\n".as_slice(),
            b"100\t130\t30\tavailable\t30\tavailable".as_slice(),
        ),
        (
            b"-100\n",
            b"-50\n",
            b"-100\t-50\t50\tavailable\t-50\tavailable",
        ),
        (
            b"-100\n",
            b"-150\n",
            b"-100\t-150\t-50\tavailable\t50\tavailable",
        ),
        (b"-2\n", b"3\n", b"-2\t3\t5\tavailable\t-250\tavailable"),
        (b"8\n", b"7\n", b"8\t7\t-1\tavailable\t-12.5\tavailable"),
    ] {
        let mut row = b"matched\t\tnot_requested\t".to_vec();
        row.extend_from_slice(expected);
        row.push(b'\n');
        assert_eq!(
            run(&["compare", "before", "after", "sum", "1"], before, after),
            (0, row, vec![])
        );
    }
}

#[test]
fn comparison_empty_sources_are_absent_and_omitted_values_are_present() {
    assert_eq!(
        run(&["compare", "before", "after", "sum", "1"], b"", b""),
        (0, vec![], vec![])
    );
    assert_eq!(
        run(&["compare", "before", "after", "sum", "1"], b"", b"4\n"),
        (
            0,
            b"added\t\tnot_requested\t\t4\t\tnot_matched\t\tnot_matched\n".to_vec(),
            vec![]
        )
    );
    assert_eq!(
        run(&["compare", "before", "after", "sum", "1"], b"4\n", b""),
        (
            0,
            b"removed\t\tnot_requested\t4\t\t\tnot_matched\t\tnot_matched\n".to_vec(),
            vec![]
        )
    );
    assert_eq!(run(&["--narm", "compare", "before", "after", "sum", "1", "mean", "1"], b"NA\n", b"N/A\n"),
        (0, b"matched\t\tnot_requested\t0\t0\t0\tavailable\t\tzero_baseline\tnan\tnan\t\tunordered\t\tnonfinite_baseline\n".to_vec(), vec![]));
}

#[test]
fn comparison_special_values_keep_difference_and_availability_precedence() {
    for (before, after, block) in [
        (
            b"0\n".as_slice(),
            b"inf\n".as_slice(),
            b"0\tinf\tinf\tavailable\t\tzero_baseline".as_slice(),
        ),
        (b"-0\n", b"nan\n", b"-0\tnan\t\tunordered\t\tzero_baseline"),
        (
            b"inf\n",
            b"inf\n",
            b"inf\tinf\t\tunordered\t\tnonfinite_baseline",
        ),
        (
            b"-inf\n",
            b"inf\n",
            b"-inf\tinf\tinf\tavailable\t\tnonfinite_baseline",
        ),
        (b"2\n", b"nan\n", b"2\tnan\t\tunordered\t\tnonfinite_after"),
    ] {
        let mut expected = b"matched\t\tnot_requested\t".to_vec();
        expected.extend_from_slice(block);
        expected.push(b'\n');
        assert_eq!(
            run(&["compare", "before", "after", "min", "1"], before, after),
            (0, expected, vec![])
        );
    }
}

#[test]
fn comparison_late_input_and_completion_failures_write_no_stdout() {
    let (status, output, diagnostics) = run(
        &["compare", "before", "after", "sum", "1"],
        b"1\n",
        b"2\nbad\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    assert!(
        diagnostics[0]
            .windows(b"after source 'after'".len())
            .any(|w| w == b"after source 'after'")
    );
    assert!(diagnostics[0].windows(6).any(|w| w == b"line 2"));
    let (status, output, diagnostics) = run(
        &["compare", "before", "after", "sum", "1", "wmean", "1:2"],
        b"1\t1\n",
        b"2\t0\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    assert!(!diagnostics.is_empty());
}

#[test]
fn comparison_mixed_operation_families_validate_grammar_before_refusal() {
    let cases: &[(&[&str], i32)] = &[
        (&["sum", "1", "cut"], 1),
        (&["sum", "1", "cut", "0"], 1),
        (&["cut", "1", "sum"], 1),
        (&["cut", "1", "sum", "0"], 1),
        (&["cut", "1", "wmean", "1"], 1),
        (&["sum", "1", "cut", "1"], 77),
        (&["cut", "1", "sum", "1"], 77),
    ];
    for &(operations, expected_status) in cases {
        let mut args = vec!["compare", "before", "after"];
        args.extend_from_slice(operations);
        let mut sources = super::command_test_support::Sources {
            inputs: [
                Input {
                    bytes: b"1\n",
                    segment: 1,
                    error: None,
                },
                Input {
                    bytes: b"2\n",
                    segment: 1,
                    error: None,
                },
            ],
            closes: [None, None],
            opened: 0,
            completed: 0,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        let (status, diagnostics) =
            super::command_test_support::command_sources(&mut sources, &mut output, &args);
        assert_eq!(status, expected_status, "{args:?}: {diagnostics:?}");
        assert_eq!(sources.opened, 0, "{args:?}");
        assert!(output.bytes.is_empty(), "{args:?}");
        assert!(!diagnostics.is_empty(), "{args:?}");
    }
}

#[test]
fn comparison_malformed_and_unimplemented_commands_do_not_open_sources() {
    for (args, status) in [
        (vec!["compare"], 1),
        (vec!["compare", "before"], 1),
        (vec!["compare", "-", "-", "sum", "1"], 1),
        (vec!["compare", "before", "after"], 1),
        (vec!["compare", "before", "after", "sum", "0"], 1),
        (vec!["compare", "before", "after", "first", "1"], 77),
        (vec!["compare", "before", "after", "round", "1"], 77),
        (vec!["compare", "before", "after", "sum", "v"], 1),
        (vec!["--full", "compare", "before", "after", "sum", "1"], 77),
        (
            vec!["--filler=N/A", "compare", "before", "after", "sum", "1"],
            77,
        ),
        (
            vec!["--sort-cmd=sort", "compare", "before", "after", "sum", "1"],
            77,
        ),
        (
            vec!["compare", "before", "after", "rank", "0", "sum", "1"],
            1,
        ),
        (
            vec!["compare", "before", "after", "rank", "2", "sum", "1"],
            1,
        ),
    ] {
        let mut input = Input {
            bytes: b"unread",
            segment: 2,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        let (actual, diagnostics) =
            super::command_test_support::command(&mut input, &mut output, &args);
        assert_eq!(actual, status, "{args:?}: {diagnostics:?}");
        assert!(output.bytes.is_empty());
        assert_eq!(input.bytes, b"unread");
        assert!(!diagnostics.is_empty());
    }
}

// These fixture adapters change framing only. Arithmetic expectations remain
// independent literals or integer/rational oracles in the Command tests.
fn quoted_csv_fixture(text: &[u8]) -> Vec<u8> {
    let mut csv = Vec::new();
    for row in text.split_inclusive(|b| *b == b'\n') {
        let row = row.strip_suffix(b"\n").unwrap_or(row);
        for (at, field) in row.split(|b| *b == b'\t').enumerate() {
            if at != 0 {
                csv.push(b',');
            }
            csv.push(b'"');
            for &byte in field {
                csv.push(byte);
                if byte == b'"' {
                    csv.push(byte);
                }
            }
            csv.push(b'"');
        }
        csv.push(b'\n');
    }
    csv
}

fn plain_csv_report(text: &[u8]) -> Vec<u8> {
    // Only fixtures without CSV metacharacters use this adapter. Literal
    // expectations below independently cover canonical quoting of such bytes.
    assert!(!text.iter().any(|b| matches!(b, b',' | b'"' | b'\r')));
    let mut csv = Vec::new();
    for row in text.split_inclusive(|b| *b == b'\n') {
        for (at, field) in row
            .strip_suffix(b"\n")
            .unwrap()
            .split(|b| *b == b'\t')
            .enumerate()
        {
            if at != 0 {
                csv.push(b',');
            }
            csv.extend_from_slice(if field.is_empty() { b"\"\"" } else { field });
        }
        csv.push(b'\n');
    }
    csv
}

#[test]
fn comparison_csv_decodes_complete_keys_preserving_quote_equivalence_and_spelling() {
    let before = b"\"a,b\",\"q\"\"uote\",1,\r\n\"multi\r\nline\",\"\",2,\n\"X\0A\",\xff,3,\n\"a,b\",\"q\"\"uote\",4,\n\"\",\"\",5,";
    let after = b",,10,\n\"x\0a\",\xff,6,\r\n\"multi\r\nline\",,4,\n\"a,b\",\"q\"\"uote\",10,";
    assert_eq!(run(&["--csv", "-i", "-s", "-g1,2", "compare", "before", "after", "sum", "3"], before, after),
        (0, b"\"\",\"\",matched,\"\",not_requested,5,10,5,available,100,available\n\"a,b\",\"q\"\"uote\",matched,\"\",not_requested,5,10,5,available,100,available\n\"multi\r\nline\",\"\",matched,\"\",not_requested,2,4,2,available,100,available\nX\0A,\xff,matched,\"\",not_requested,3,6,3,available,100,available\n".to_vec(), vec![]));
    // A BOM and blanks are bytes in the key, and quoted/unquoted spellings
    // of a plain key consolidate without losing the first before spelling.
    assert_eq!(run(&["--csv", "-g1", "compare", "before", "after", "sum", "2"], b"A,1\n\"A\",2\n\xef\xbb\xbfA,3\n A ,4\n", b"\"A\",6\n A ,8\n\xef\xbb\xbfA,6\n"),
        (0, b" A ,matched,\"\",not_requested,4,8,4,available,100,available\nA,matched,\"\",not_requested,3,6,3,available,100,available\n\xef\xbb\xbfA,matched,\"\",not_requested,3,6,3,available,100,available\n".to_vec(), vec![]));
    assert_eq!(
        run(
            &["--csv", "-g1", "compare", "before", "after", "count", "1"],
            b"\n\"\"",
            b"\r\n"
        ),
        (
            0,
            b"\"\",matched,\"\",not_requested,2,1,-1,available,-50,available\n".to_vec(),
            vec![]
        )
    );
}

#[test]
fn comparison_csv_named_ranked_headers_and_labels_are_individually_encoded() {
    let args = [
        "--csv",
        "-H",
        "-gkey\\,name",
        "--result-name=1:total,\"quoted\"\r\n",
        "compare",
        "before",
        "after",
        "rank",
        "1",
        "percent",
        "limit",
        "1",
        "sum",
        "metric\\,one",
    ];
    let before = b"\"key,name\",\"metric,one\",\"metric,one\"\r\nA,100,999\nB,10,999\nzero,0,999\nremoved,4,999\n";
    let after = b"\"metric,one\",\"key,name\",\"metric,one\"\n130,A,999\n20,B,999\n1,zero,999\n5,added,999\n";
    assert_eq!(run(&args, before, after), (0, b"\"key(key,name)\",presence,rank,rank_state,\"before(total,\"\"quoted\"\"\r\n)\",\"after(total,\"\"quoted\"\"\r\n)\",\"difference(total,\"\"quoted\"\"\r\n)\",\"difference_state(total,\"\"quoted\"\"\r\n)\",\"percentage(total,\"\"quoted\"\"\r\n)\",\"percentage_state(total,\"\"quoted\"\"\r\n)\"\nB,matched,1,ranked,10,20,10,available,100,available\nadded,added,\"\",one_sided,\"\",5,\"\",not_matched,\"\",not_matched\nremoved,removed,\"\",one_sided,4,\"\",\"\",not_matched,\"\",not_matched\nzero,matched,\"\",unavailable,0,1,1,available,\"\",zero_baseline\n".to_vec(), vec![]));
    assert_eq!(run(&["--csv", "-H", "-g1", "compare", "before", "after", "sum", "metric\\,one"], b"\"key\0hidden\",\"metric,one\"\n\xff,2\n", b"\"metric,one\",k\n4,\xff\n"),
        (0, b"key(key),presence,rank,rank_state,\"before(sum(metric,one))\",\"after(sum(metric,one))\",\"difference(sum(metric,one))\",\"difference_state(sum(metric,one))\",\"percentage(sum(metric,one))\",\"percentage_state(sum(metric,one))\"\n4,added,\"\",not_requested,\"\",4,\"\",not_matched,\"\",not_matched\n\xff,removed,\"\",not_requested,2,\"\",\"\",not_matched,\"\",not_matched\n".to_vec(), vec![]));
}

#[test]
fn comparison_csv_to_text_retains_complete_unescaped_fields_and_custom_delimiter() {
    assert_eq!(
        run(
            &[
                "--csv-in",
                "--output-delimiter=|",
                "-g1",
                "compare",
                "before",
                "after",
                "sum",
                "2"
            ],
            b"\"a|b\n\0\xff\",2\n",
            b"\"a|b\n\0\xff\",4\n"
        ),
        (
            0,
            b"a|b\n\0\xff|matched||not_requested|2|4|2|available|100|available\n".to_vec(),
            vec![]
        )
    );
}

#[test]
fn comparison_csv_validates_unused_omitted_nonwinning_and_nonfinite_records() {
    for tail in [
        b"bad\"quote".as_slice(),
        b"\"closed\"x",
        b"\"unterminated",
        b"bad\r",
    ] {
        for side in [0, 1] {
            let mut malformed = b"k,v,w,unused\nA,1,1,ok\nZ,NA,1,".to_vec();
            malformed.extend_from_slice(tail);
            let valid = b"k,v,w,unused\nA,2,1,ok\nZ,1,1,ok\n";
            let (status, output, diagnostics) = run(
                &[
                    "--csv", "-H", "--narm", "-gk", "compare", "before", "after", "rank", "1",
                    "limit", "1", "sum", "v", "wmean", "v:w",
                ],
                if side == 0 { &malformed } else { valid },
                if side == 0 { valid } else { &malformed },
            );
            assert_eq!(status, 1, "{diagnostics:?}");
            assert!(output.is_empty());
            let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
            assert!(diagnostic.contains("invalid CSV"), "{diagnostic}");
            assert!(
                diagnostic.contains("record 3 starts at physical line 3"),
                "{diagnostic}"
            );
            assert!(diagnostic.contains(if side == 0 {
                "before source"
            } else {
                "after source"
            }));
        }
    }
    for exceptional in ["nan", "inf"] {
        let input = format!("A,{exceptional},ok\nZ,1,bad\"quote\n");
        let (status, output, diagnostics) = run(
            &[
                "--csv",
                "--header-out",
                "-g1",
                "compare",
                "before",
                "after",
                "rank",
                "1",
                "limit",
                "1",
                "min",
                "2",
            ],
            b"A,1,ok\n",
            input.as_bytes(),
        );
        assert_eq!(status, 1);
        assert!(output.is_empty());
        assert!(String::from_utf8_lossy(&diagnostics[0]).contains("invalid CSV"));
    }
}

#[test]
fn comparison_csv_errors_keep_original_logical_and_physical_locations() {
    for (after, fragments) in [
        (
            b"\"k\nname\",v,w\n\"a\nb\",2,1\n\"z\nz\",bad,1\n".as_slice(),
            vec![
                "invalid numeric value",
                "line 3",
                "CSV record 3 starts at physical line 5",
                "after source 'after'",
            ],
        ),
        (
            b"\"k\nname\",v,w\n\"a\nb\",2,1\n\"z\nz\",3\n",
            vec![
                "field 3 requested",
                "CSV record 3 starts at physical line 5",
                "after source 'after'",
            ],
        ),
        (
            b"\"k\nname\",v,w\n\"a\nb\",2,1\n\"z\nz\",3,1,bad\"quote\n",
            vec![
                "invalid CSV",
                "physical line 6 byte 11",
                "record 3 starts at physical line 5",
                "after source 'after'",
            ],
        ),
    ] {
        let (status, output, diagnostics) = run(
            &[
                "--csv", "-H", "-g1", "compare", "before", "after", "wmean", "v:w",
            ],
            b"\"k\nname\",v,w\n\"a\nb\",1,1\n",
            after,
        );
        assert_eq!(status, 1, "{diagnostics:?}");
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        for fragment in fragments {
            assert!(diagnostic.contains(fragment), "{fragment}: {diagnostic}");
        }
    }
    for side in [0, 1] {
        let invalid = b"\"k\nname\",other\n";
        let valid = b"\"k\nname\",v\n";
        let (status, output, diagnostics) = run(
            &["--csv", "-H", "compare", "before", "after", "sum", "v"],
            if side == 0 { invalid } else { valid },
            if side == 0 { valid } else { invalid },
        );
        assert_eq!(status, 1);
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        assert!(diagnostic.contains("CSV record 1 starts at physical line 1"));
        assert!(diagnostic.contains(if side == 0 {
            "before source"
        } else {
            "after source"
        }));
    }
}

#[test]
fn comparison_csv_conflicts_keep_precedence_in_either_option_order() {
    use super::command_test_support::{Sources, command_sources};
    for (csv, control, expected) in [
        (
            "--csv-in",
            "-t,",
            "CSV input conflicts with --field-separator\n",
        ),
        ("--csv-in", "-W", "CSV input conflicts with --whitespace\n"),
        (
            "--csv-in",
            "-C",
            "CSV input conflicts with --skip-comments\n",
        ),
        ("--csv", "-z", "CSV conflicts with --zero-terminated\n"),
        ("--csv-out", "--vnlog", "CSV conflicts with --vnlog\n"),
        (
            "--csv-out",
            "--output-delimiter=|",
            "CSV output conflicts with --output-delimiter\n",
        ),
    ] {
        for reversed in [false, true] {
            let mut args = if reversed {
                vec![control, csv]
            } else {
                vec![csv, control]
            };
            args.extend(["--full", "compare", "before", "after", "first", "1"]);
            let mut sources = Sources {
                inputs: [
                    Input {
                        bytes: b"bad",
                        segment: 1,
                        error: None,
                    },
                    Input {
                        bytes: b"bad",
                        segment: 1,
                        error: None,
                    },
                ],
                closes: [None, None],
                opened: 0,
                completed: 0,
            };
            let mut output = Output {
                bytes: vec![],
                error: None,
                closed: false,
            };
            assert_eq!(
                command_sources(&mut sources, &mut output, &args),
                (1, vec![expected.as_bytes().to_vec()])
            );
            assert_eq!(sources.opened, 0);
            assert!(output.bytes.is_empty());
        }
    }
    for spelling in [
        "--csv-i",
        "--csv-o",
        "--csv=yes",
        "--csv-in=yes",
        "--csv-out=yes",
    ] {
        let (status, output, _) = run(
            &[spelling, "compare", "before", "after", "sum", "1"],
            b"bad",
            b"bad",
        );
        assert_eq!(status, 1);
        assert!(output.is_empty());
    }
}

#[test]
fn comparison_csv_source_and_output_faults_are_completed_before_success() {
    use super::command_test_support::{Sources, command_sources};
    for side in [0, 1] {
        for close in [false, true] {
            let mut sources = Sources {
                inputs: [
                    Input {
                        bytes: b"k,v\nA,1\n",
                        segment: 1,
                        error: None,
                    },
                    Input {
                        bytes: b"v,k\n2,A\n",
                        segment: 1,
                        error: None,
                    },
                ],
                closes: [None, None],
                opened: 0,
                completed: 0,
            };
            if close {
                sources.closes[side] = Some(5);
            } else {
                sources.inputs[side].error = Some(5);
            }
            let mut output = Output {
                bytes: vec![],
                error: None,
                closed: false,
            };
            let (status, diagnostics) = command_sources(
                &mut sources,
                &mut output,
                &[
                    "--csv", "-H", "-gk", "compare", "before", "after", "rank", "1", "limit", "1",
                    "sum", "v",
                ],
            );
            assert_eq!(status, 1);
            assert!(output.bytes.is_empty());
            assert!(output.closed);
            assert_eq!((sources.opened, sources.completed), (side + 1, side + 1));
            assert!(
                String::from_utf8_lossy(&diagnostics[0]).contains(if side == 0 {
                    "before source"
                } else {
                    "after source"
                })
            );
        }
    }
    let mut output = Output {
        bytes: vec![],
        error: Some(28),
        closed: false,
    };
    let (status, diagnostics) = comparison_command(
        &[
            "--csv",
            "--header-out",
            "compare",
            "before",
            "after",
            "sum",
            "1",
        ],
        b"1\n",
        b"2\n",
        &mut output,
    );
    assert_eq!(status, 1);
    assert!(output.closed);
    assert!(!diagnostics.is_empty());
}

#[test]
fn comparison_csv_allocation_failures_on_either_source_precede_headers_and_winners() {
    use super::{
        command_memory::FAIL_RESERVATION,
        command_test_support::{Sources, command_sources},
    };
    let args = [
        "--csv",
        "-H",
        "-gk",
        "--result-name=1:total",
        "compare",
        "before",
        "after",
        "rank",
        "1",
        "percent",
        "limit",
        "1",
        "sum",
        "v",
    ];
    let mut refused_sources = [false; 2];
    let mut succeeded = false;
    for fail_after in 0..1000 {
        let mut sources = Sources {
            inputs: [
                Input {
                    bytes: b"k,v\nA,1\nZ,0\nremoved,4\n",
                    segment: 1,
                    error: None,
                },
                Input {
                    bytes: b"v,k\n10,A\n1,Z\n5,added\n",
                    segment: 1,
                    error: None,
                },
            ],
            closes: [None, None],
            opened: 0,
            completed: 0,
        };
        let mut output = Output {
            bytes: vec![],
            error: None,
            closed: false,
        };
        FAIL_RESERVATION.with(|slot| slot.set(Some(fail_after)));
        let (status, diagnostics) = command_sources(&mut sources, &mut output, &args);
        let pending = FAIL_RESERVATION.with(|slot| slot.replace(None));
        if pending.is_some() {
            assert_eq!(status, 0);
            succeeded = true;
            break;
        }
        assert_eq!(status, 77, "{fail_after}: {diagnostics:?}");
        assert!(output.bytes.is_empty());
        if (1..=2).contains(&sources.opened) {
            refused_sources[sources.opened - 1] = true;
        }
    }
    assert!(succeeded && refused_sources.iter().all(|refused| *refused));
}

#[test]
fn comparison_csv_header_only_and_positional_header_rules_remain_independent() {
    let header = b"presence,rank,rank_state,before(sum(v)),after(sum(v)),difference(sum(v)),difference_state(sum(v)),percentage(sum(v)),percentage_state(sum(v))\n";
    assert_eq!(
        run(
            &["--csv", "-H", "compare", "before", "after", "sum", "v"],
            b"v\r\n",
            b"\"v\""
        ),
        (0, header.to_vec(), vec![])
    );
    let mut expected = header.to_vec();
    expected
        .extend_from_slice(b"added,\"\",not_requested,\"\",4,\"\",not_matched,\"\",not_matched\n");
    assert_eq!(
        run(
            &["--csv", "-H", "compare", "before", "after", "sum", "v"],
            b"v\n",
            b"v\n4\n"
        ),
        (0, expected, vec![])
    );
    for (before, after, role) in [
        (b"".as_slice(), b"v\n".as_slice(), "before"),
        (b"v\n", b"", "after"),
    ] {
        let (status, output, diagnostics) = run(
            &["--csv", "-H", "compare", "before", "after", "sum", "v"],
            before,
            after,
        );
        assert_eq!(status, 1);
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        assert!(diagnostic.contains("requires an input header") && diagnostic.contains(role));
    }
    let mut expected = header.to_vec();
    expected.extend_from_slice(b"matched,\"\",not_requested,2,4,2,available,100,available\n");
    assert_eq!(
        run(
            &["--csv", "-H", "compare", "before", "after", "sum", "3"],
            b"a,b,v\n0,0,2\n",
            b"a,b\n0,0,4\n"
        ),
        (0, expected, vec![])
    );
    assert_eq!(run(&["--csv", "--header-out", "compare", "before", "after", "sum", "1"], b"", b""), (0, b"presence,rank,rank_state,before(sum(field-1)),after(sum(field-1)),difference(sum(field-1)),difference_state(sum(field-1)),percentage(sum(field-1)),percentage_state(sum(field-1))\n".to_vec(), vec![]));
}

#[test]
fn comparison_csv_paired_omission_and_expansion_keep_original_calculations() {
    assert_eq!(run(&["--csv", "--narm", "compare", "before", "after", "dotprod", "1:2", "wmean", "1:2"], b"1,NA\nNA,2\n3,4\n", b"2,NA\nNA,2\n6,4\n"),
        (0, b"matched,\"\",not_requested,14,28,14,available,100,available,3,6,3,available,100,available\n".to_vec(), vec![]));
    let (status, output, diagnostics) = run(
        &[
            "--csv", "--narm", "compare", "before", "after", "wmean", "1:2",
        ],
        b"1,1\n",
        b"NA,-1\n",
    );
    assert_eq!(status, 1);
    assert!(output.is_empty());
    let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
    assert!(
        diagnostic.contains("CSV record 1 starts at physical line 1")
            && diagnostic.contains("after source")
    );
}

#[test]
fn comparison_csv_arithmetic_trace_and_failures_use_completed_values_before_output() {
    assert_eq!(
        run(
            &[
                "--csv",
                "--format=%a",
                "compare",
                "before",
                "after",
                "rank",
                "1",
                "sum",
                "1"
            ],
            b"\"3\"\n",
            b"\"5\"\n"
        ),
        (
            0,
            b"matched,1,ranked,0xcp-2,0xap-1,0x8p-2,available,0x8.555555555555556p+3,available\n"
                .to_vec(),
            vec![]
        )
    );
    for (before, after, stage) in [
        (
            b"a,1\nz,-0x1p+16383\n".as_slice(),
            b"a,2\nz,0x1p+16383\n".as_slice(),
            "subtraction",
        ),
        (b"a,1\nz,0x1p-16445\n", b"a,2\nz,1\n", "percentage division"),
        (
            b"a,1\nz,1\n",
            b"a,2\nz,0x1p+16378\n",
            "percentage multiplication",
        ),
    ] {
        let (status, output, diagnostics) = run(
            &[
                "--csv",
                "--header-out",
                "-g1",
                "compare",
                "before",
                "after",
                "rank",
                "1",
                "limit",
                "1",
                "count",
                "2",
                "sum",
                "2",
            ],
            before,
            after,
        );
        assert_eq!(status, 77);
        assert!(output.is_empty());
        let diagnostic = String::from_utf8_lossy(&diagnostics[0]);
        for fragment in [
            stage,
            "result 2",
            "key ('z')",
            "before source",
            "after source",
        ] {
            assert!(diagnostic.contains(fragment), "{diagnostic}");
        }
        assert!(!diagnostic.contains("CSV record"));
    }
}
