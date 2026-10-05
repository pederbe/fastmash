//! Weighted mean checks through the existing Command interface.
use super::command_test_support::{Input, Output, command};

fn run(args: &[&str], bytes: &[u8]) -> (i32, Vec<u8>, Vec<Vec<u8>>) {
    let mut input = Input {
        bytes,
        segment: 7,
        error: None,
    };
    let mut output = Output {
        bytes: Vec::new(),
        error: None,
        closed: false,
    };
    let (status, diagnostics) = command(&mut input, &mut output, args);
    (status, output.bytes, diagnostics)
}

#[test]
fn weighted_formula_and_request_expansion() {
    for input in [b"10\t1\n20\t3\n".as_slice(), b"10\t0.25\n20\t0.75"] {
        assert_eq!(
            run(&["wmean", "1:2"], input),
            (0, b"17.5\n".to_vec(), vec![])
        );
    }
    assert_eq!(
        run(
            &["WmEaN", "1:2,2:2", "mean", "1", "wmean", "1:2"],
            b"10\t1\n20\t3\n"
        ),
        (0, b"17.5\t2.5\t15\t17.5\n".to_vec(), vec![])
    );
}

#[test]
fn csv_weighted_pairs_bind_decoded_names_and_keep_expanded_report_order() {
    for csv in ["--csv", "--csv-in"] {
        let expected = if csv == "--csv" {
            b"\"wmean(value:raw,weight,raw)\",second,\"wmean(value:raw,weight,raw)\",\"wmean(weight,raw,weight,raw)\",\"wmean(value:raw,weight,raw)\",sum(value:raw),sum(other)\n17.5,7,17.5,2.5,17.5,30,12\n".as_slice()
        } else {
            b"wmean(value:raw,weight,raw)\tsecond\twmean(value:raw,weight,raw)\twmean(weight,raw,weight,raw)\twmean(value:raw,weight,raw)\tsum(value:raw)\tsum(other)\n17.5\t7\t17.5\t2.5\t17.5\t30\t12\n"
        };
        success(
            &[
                csv,
                "-H",
                "--result-name=2:second",
                "wmean",
                "value\\:raw:weight\\,raw,3:weight\\,raw,value\\:raw:2,2:2,value\\:raw:weight\\,raw",
                "sum",
                "1,3",
            ],
            b"value:raw,\"weight,raw\",other,\"weight,raw\"\r\n\"10\",1,4,99\n20,\"3\",8,99",
            expected,
        );
    }
    // Each conversion must end at its decoded Field, including the last Field.
    success(
        &["--csv-in", "wmean", "1:2"],
        b"\"10\",\"1\",e99999\n\"20\",\"3\",.5",
        b"17.5\n",
    );
    success(
        &["--csv-in", "wmean", "2:3"],
        b"ignored,\"10\",\"1\"\nignored,\"20\",\"3\"",
        b"17.5\n",
    );
    success(
        &[
            "--csv", "--narm", "wmean", "1:2", "mean", "1,2", "dotprod", "1:2",
        ],
        b"1,NA\nNA,2\n3,4\n",
        b"3,2,3,14\n",
    );
}

#[test]
fn csv_weighted_omission_and_zero_contributions_keep_partner_validation() {
    for input in [
        b"NA,2\nn/a,1\n2,NaN\n10,1\n20,3".as_slice(),
        b"NA,NA\nNaN,0\n2,N/A\n10,1\n20,3",
    ] {
        success(&["--csv", "--narm", "wmean", "1:2"], input, b"17.5\n");
    }
    for (input, field) in [
        ("NA,oops\n", 2),
        ("NA,-1\n", 2),
        ("NA,inf\n", 2),
        ("NA,-nan\n", 2),
        ("NA,nan(7)\n", 2),
        ("NA,1e5000\n", 2),
        ("NA,\n", 2),
        ("NA\n", 2),
        ("oops,NA\n", 1),
        ("bad,0\n", 1),
        ("\"\",-0\n", 1),
        ("1e-5000,0\n", 1),
        ("\" NA\",1\n", 1),
        ("\"NaN \",1\n", 1),
    ] {
        error(
            &["--csv", "--narm", "wmean", "1:2"],
            input.as_bytes(),
            1,
            format!("field {field}").as_bytes(),
        );
    }
    for input in ["NA,1\n", "N/A,1\n", "2,NaN\n", "2,-nan\n", "2,nan(7)\n"] {
        error(&["--csv", "wmean", "1:2"], input.as_bytes(), 1, b"field");
    }
    for zero in ["0", "-0"] {
        for value in ["inf", "-inf", "nan", "-nan(7)"] {
            success(
                &["--csv", "wmean", "1:2"],
                format!("{value},{zero}\n5,2\n").as_bytes(),
                b"5\n",
            );
        }
    }
    for (input, expected) in [
        ("inf,1\n2,3\n", "inf\n"),
        ("-inf,1\n-2,3\n", "-inf\n"),
        ("inf,1\n-inf,2\n", "nan\n"),
        ("-nan,1\n2,3\n", "-nan\n"),
    ] {
        success(
            &["--csv", "wmean", "1:2"],
            input.as_bytes(),
            expected.as_bytes(),
        );
    }
    for special in ["inf", "nan", "-nan(7)"] {
        error(
            &["--csv", "wmean", "1:2"],
            format!("{special},1\nbad,1\n").as_bytes(),
            1,
            b"line 2 field 1",
        );
        error(
            &["--csv", "wmean", "1:2"],
            format!("{special},1\n1,-1\n").as_bytes(),
            1,
            b"line 2 field 2",
        );
    }
}

#[test]
fn csv_weighted_numerical_stages_keep_the_approved_range_boundary() {
    for (input, stage) in [
        ("2,1e4932\n", "product overflow"),
        ("1e-4000,1e-4000\n", "nonzero product rounded to zero"),
        ("1e4932,1\n1e4932,1\n-1e4932,2\n", "weighted total overflow"),
        ("0,1e4932\n0,1e4932\n", "weight total overflow"),
        ("inf,1\n2,1e4932\n", "product overflow"),
        (
            "nan,1\n1e-4000,1e-4000\n",
            "nonzero product rounded to zero",
        ),
        // M/2+M*2^-65 rounds to 2^16383 and the weights tie to 1/2.
        (
            "0x1.fffffffffffffffep+16383,0x1p-1\n0x1.fffffffffffffffep+16383,0x1p-65\n",
            "final division overflow",
        ),
    ] {
        error(
            &["--csv", "wmean", "1:2"],
            input.as_bytes(),
            77,
            stage.as_bytes(),
        );
    }
    success(
        &["--csv", "--format=%a", "wmean", "1:2"],
        b"0x1p-16382,0.5\n",
        b"0x8p-16385\n",
    );
    success(&["--csv", "wmean", "1:2"], b"0x1p-16445,1\n0,2\n", b"0\n");
    // Products round before addition: (1+2^-63)^2 loses the 2^-126 term.
    success(
        &["--csv", "wmean", "1:2"],
        b"-0x8.000000000000002p-3,1\n0x8.000000000000001p-3,0x8.000000000000001p-3\n",
        b"0\n",
    );
}

#[test]
fn csv_weighted_domains_keep_headers_group_boundaries_and_key_precedence() {
    for csv in ["--csv", "--csv-in"] {
        success(&[csv, "-H", "wmean", "value:weight"], b"", b"");
        success(
            &[csv, "-H", "wmean", "value:weight"],
            b"value,weight\n",
            if csv == "--csv" {
                b"\"wmean(value,weight)\"\n"
            } else {
                b"wmean(value,weight)\n"
            },
        );
        error(&[csv, "wmean", "value:2"], b"bad", 1, b"-H or --header-in");
        assert_eq!(run(&[csv, "-H", "wmean", "value:absent"], b"\"value\nraw\",weight\n"), (1, vec![], vec![b"column name 'value' not found in input file\nCSV record 1 starts at physical line 1\n".to_vec()]));
        for input in [b"10,0\n".as_slice(), b"NA,1\n", b"NA,0\n2,NA\n"] {
            error(
                &[csv, "--narm", "wmean", "1:2"],
                input,
                1,
                b"wmean fields 1:2 have no positive retained contribution weight",
            );
        }
    }
    success(
        &["--csv", "-g1,2", "wmean", "3:4"],
        b"a,x,10,1\n\"a\",x,20,3\nb,x,5,2\na,x,30,1\na,y,-2,4",
        b"a,x,17.5\nb,x,5\na,x,30\na,y,-2\n",
    );
    success(
        &["--csv", "-ig1", "wmean", "2:3"],
        b"A,10,1\na,20,3\nB,5,2\n",
        b"A,17.5\nB,5\n",
    );
    success(
        &["--csv", "-g1", "wmean", "2:3"],
        b",10,1\n\"\",20,3\na,5,2\n",
        b"\"\",17.5\na,5\n",
    );
    assert_eq!(run(&["--csv", "--narm", "-g1", "wmean", "2:3"], b"a,10,1\nb,NA,1\na,20,3\n"), (1, b"a,10\nb,".to_vec(), vec![b"wmean fields 2:3 have no positive retained contribution weight\ngroup keys: field 1='b'\n".to_vec()]));
    success(
        &["--csv-in", "--output-delimiter=|", "-g1", "wmean", "2:3"],
        b"\"a|b\nc\",10,1\n\"a|b\nc\",20,3\n",
        b"a|b\nc|17.5\n",
    );
    // The prior Group completes before a later omitted Record's missing key.
    assert_eq!(run(&["--csv", "--narm", "-g1,4", "wmean", "2:3"], b"a,10,1,x\nb,NA,2\n"), (1, b"a,x,10\n".to_vec(), vec![b"invalid input: field 4 requested, line 2 has only 3 fields\nCSV record 2 starts at physical line 2\n".to_vec()]));
}

#[test]
fn csv_unused_syntax_still_fails_after_special_omitted_and_zero_pairs() {
    for first in [
        "inf,1,valid\n",
        "nan,1,valid\n",
        "NA,1,valid\n",
        "5,0,valid\n",
    ] {
        let input = format!("{first}NA,0,\"x\ny\"!\n");
        for sort in [false, true] {
            let mut args = vec!["--csv", "--narm", "wmean", "1:2"];
            if sort {
                args.extend(["-s", "-g3"]);
            }
            assert_eq!(run(&args, input.as_bytes()), (1, vec![], vec![b"invalid CSV: byte after closing quote at physical line 3 byte 3; record 2 starts at physical line 2\n".to_vec()]));
        }
    }
}

#[test]
fn csv_weighted_transports_preserve_short_writes_and_incomplete_scan_failures() {
    use super::command_test_support::command_in;
    let bytes = b"key,value,weight,note\r\nb,5,2,\"two\r\nlines\"\n\"a\",10,1,first\na,20,3,\"last\"\"note\"";
    for memory in ["67108864", "1"] {
        for sort in [false, true] {
            for segment in [1, 2, 7, 128] {
                for read_error in [None, Some(5)] {
                    for write_error in [None, Some(28), Some(5)] {
                        let mut input = Input {
                            bytes,
                            segment,
                            error: read_error,
                        };
                        let mut output = Output {
                            bytes: vec![],
                            error: write_error,
                            closed: false,
                        };
                        let mut args = vec!["--csv", "-H", "-g1", "wmean", "2:3"];
                        if sort {
                            args.push("-s");
                        }
                        let environment = super::Environment {
                            locale: Default::default(),
                            posixly_correct: false,
                            grouping: None,
                            sort_memory: Some(memory.into()),
                            pipe_grouping: None,
                            terminal: Default::default(),
                        };
                        let (status, diagnostics) =
                            command_in(&mut input, &mut output, &args, environment);
                        assert_eq!(
                            status,
                            if write_error == Some(5) {
                                77
                            } else if read_error.is_some() || write_error.is_some() {
                                1
                            } else {
                                0
                            },
                            "{diagnostics:?}"
                        );
                        if let Some(5) = read_error {
                            assert!(
                                diagnostics
                                    .iter()
                                    .any(|message| message == b"read error: Input/output error\n")
                            );
                            if sort && write_error.is_none() {
                                assert_eq!(output.bytes, b"GroupBy(key),\"wmean(value,weight)\"\n");
                            }
                        } else if write_error.is_none() {
                            assert!(diagnostics.is_empty());
                            assert_eq!(
                                output.bytes,
                                if sort {
                                    b"GroupBy(key),\"wmean(value,weight)\"\na,17.5\nb,5\n"
                                        .as_slice()
                                } else {
                                    b"GroupBy(key),\"wmean(value,weight)\"\nb,5\na,17.5\n"
                                }
                            );
                        }
                        assert!(output.closed);
                    }
                }
            }
        }
    }
}

#[test]
fn missing_pair_does_not_hide_invalid_weight() {
    let (status, stdout, diagnostics) = run(&["--narm", "wmean", "1:2"], b"NA\t-1\n");
    assert_eq!(status, 1);
    assert!(stdout.is_empty());
    assert!(
        diagnostics[0]
            .windows(b"weight".len())
            .any(|bytes| bytes == b"weight")
    );
}

fn success(args: &[&str], input: &[u8], expected: &[u8]) {
    let actual = run(args, input);
    assert_eq!(
        actual,
        (0, expected.to_vec(), vec![]),
        "{args:?}, {input:?}"
    );
}

fn error(args: &[&str], input: &[u8], status: i32, contains: &[u8]) {
    let actual = run(args, input);
    assert_eq!(actual.0, status, "{args:?}, {input:?}, {:?}", actual.2);
    assert!(
        actual
            .2
            .iter()
            .any(|message| message.windows(contains.len()).any(|part| part == contains)),
        "{args:?}, {input:?}, {:?}",
        actual.2
    );
}

#[test]
fn weighted_values_use_ordinary_conversion() {
    for (input, expected) in [
        ("5\t2\n", "5\n"),
        ("-10\t1\n10\t3\n", "5\n"),
        ("+1.25e2\t+0.5\n-2.5e1\t.5\n", "50\n"),
        ("0x1p2\t0x1p-2\n0x1\t0x3p-2\n", "1.75\n"),
        ("9007199254740993\t1\n", "9007199254740993\n"),
    ] {
        success(
            &["--format=%.20g", "wmean", "1:2"],
            input.as_bytes(),
            expected.as_bytes(),
        );
    }
}

#[test]
fn weighted_pair_validation_is_ordered_and_record_local() {
    for args in [
        vec!["--narm", "wmean", "1:2"],
        vec!["--narm", "mean", "1", "mean", "2", "wmean", "1:2"],
    ] {
        for input in ["NA\t2\n", "2\tn/a\n", "nAn\tNa\n", "NA\t2\n2\tNA\n"] {
            error(
                &args,
                input.as_bytes(),
                1,
                b"no positive retained contribution weight",
            );
        }
        for input in ["NA\toops\n", "oops\tNA\n", "NA\t\n", "NA\n", "NA\t1e5000\n"] {
            error(
                &args,
                input.as_bytes(),
                1,
                if input.starts_with("oops") {
                    b"field 1"
                } else {
                    b"field 2"
                },
            );
        }
    }
    for weight in ["-1", "-0.25", "inf", "-inf", "NaN", "-nan", "nan(7)"] {
        error(
            &["wmean", "1:2"],
            format!("2\t{weight}\n").as_bytes(),
            1,
            b"contribution weight in line 1 field 2",
        );
        if weight != "NaN" {
            error(
                &["--narm", "wmean", "1:2"],
                format!("NA\t{weight}\n").as_bytes(),
                1,
                b"contribution weight",
            );
        }
    }
    for input in [
        "NA\t1\n",
        "N/A\t1\n",
        " NA\t1\n",
        "NaN \t1\n",
        "bad\t0\n",
        "\t0\n",
        "1e-5000\t0\n",
        "bad\tbad\n",
    ] {
        error(&["wmean", "1:2"], input.as_bytes(), 1, b"field 1");
    }
    error(&["--narm", "wmean", "1:2"], b"NaN \t1\n", 1, b"field 1");
    success(
        &["--narm", "wmean", "1:2"],
        b"NA\t2\n2\tNA\n10\t1\n20\t3\n",
        b"17.5\n",
    );
    // Later requests fail in request order; each pair validates value first.
    error(
        &["wmean", "3:4", "wmean", "1:2"],
        b"bad\t-1\t1\t-2\n",
        1,
        b"field 4",
    );
    error(
        &["wmean", "1:2", "wmean", "3:4"],
        b"bad\t-1\t1\t-2\n",
        1,
        b"field 1",
    );
}

#[test]
fn zero_contribution_and_special_values_do_not_end_validation() {
    for zero in ["0", "-0", "0x0p+0", "-0x0p+0"] {
        for special in ["inf", "-inf", "nan", "-nan", "nan(42)", "-nan(42)"] {
            success(
                &["wmean", "1:2"],
                format!("{special}\t{zero}\n5\t2\n").as_bytes(),
                b"5\n",
            );
        }
    }
    for (input, expected) in [
        ("inf\t1\n2\t3\n", "inf\n"),
        ("-inf\t1\n-2\t3\n", "-inf\n"),
        ("inf\t1\n-inf\t2\n", "nan\n"),
        ("-nan\t1\n2\t3\n", "-nan\n"),
        ("nan(2)\t1\n-nan(3)\t1\n", "-nan\n"),
        ("nan(3)\t1\n-nan(3)\t1\n", "nan\n"),
    ] {
        success(&["wmean", "1:2"], input.as_bytes(), expected.as_bytes());
    }
    success(
        &["--narm", "wmean", "1:2"],
        b"NaN\t1\n-nan\t0\n5\t2\n",
        b"5\n",
    );
    for special in ["inf", "nan", "-nan(7)"] {
        error(
            &["wmean", "1:2"],
            format!("{special}\t1\nbad\t1\n").as_bytes(),
            1,
            b"line 2 field 1",
        );
        error(
            &["wmean", "1:2"],
            format!("{special}\t1\n1\t-1\n").as_bytes(),
            1,
            b"line 2 field 2",
        );
    }
}

#[test]
fn nonempty_weighted_domains_need_a_positive_contribution() {
    success(&["wmean", "1:2"], b"", b"");
    for input in ["10\t0\n", "10\t-0\n", "NA\t1\n", "NA\t0\n2\tNA\n"] {
        error(
            &["--narm", "wmean", "1:2"],
            input.as_bytes(),
            1,
            b"wmean fields 1:2 have no positive retained contribution weight",
        );
    }
    let (status, stdout, _) = run(&["sum", "1", "wmean", "1:2"], b"2\t0\n");
    assert_eq!(status, 1);
    assert_eq!(stdout, b"2\t");
}

#[test]
fn finite_weighted_stages_refuse_lost_range_even_after_a_special() {
    for (input, stage) in [
        ("2\t1e4932\n", "product overflow"),
        ("1e-4000\t1e-4000\n", "nonzero product rounded to zero"),
        (
            "1e4932\t1\n1e4932\t1\n-1e4932\t2\n",
            "weighted total overflow",
        ),
        ("0\t1e4932\n0\t1e4932\n", "weight total overflow"),
        ("inf\t1\n2\t1e4932\n", "product overflow"),
        (
            "nan\t1\n1e-4000\t1e-4000\n",
            "nonzero product rounded to zero",
        ),
        ("inf\t1e4932\n0\t1e4932\n", "weight total overflow"),
    ] {
        error(&["wmean", "1:2"], input.as_bytes(), 77, stage.as_bytes());
    }
    // M=(2-2^-63)*2^16383. Products M/2 and M*2^-65 are finite;
    // their sum rounds to 2^16383. The weight sum is the exact midpoint
    // 1/2+2^-65 and ties to 1/2, so the final quotient overflows to 2^16384.
    error(
        &["wmean", "1:2"],
        b"0x1.fffffffffffffffep+16383\t0x1p-1\n0x1.fffffffffffffffep+16383\t0x1p-65\n",
        77,
        b"final division overflow",
    );
    // Exact hexadecimal operands admit subnormals; final underflow is permitted.
    success(&["wmean", "1:2"], b"0x1p-16445\t1\n0\t2\n", b"0\n");
    success(
        &["--format=%a", "wmean", "1:2"],
        b"0x1p-16382\t0.5\n",
        b"0x8p-16385\n",
    );
}

#[test]
fn ordered_binary80_trace_has_separate_product_and_sum_roundings() {
    // The hexadecimal operands below are exact. (1+2^-63)^2 rounds to
    // 1+2^-62 before subtraction, giving zero rather than a fused 2^-126.
    success(
        &["wmean", "1:2"],
        b"-0x8.000000000000002p-3\t1\n0x8.000000000000001p-3\t0x8.000000000000001p-3\n",
        b"0\n",
    );
    // 2^64+1 ties at the 64-bit significand and rounds to the even 2^64.
    success(
        &["wmean", "1:2"],
        b"18446744073709551616\t1\n1\t1\n-18446744073709551616\t1\n",
        b"0\n",
    );
    success(
        &["wmean", "1:2"],
        b"18446744073709551616\t1\n-18446744073709551616\t1\n1\t1\n",
        b"0.33333333333333\n",
    );
    success(&["--format=%a", "wmean", "1:2"], b"-0\t1\n", b"0x0p+0\n");
}

#[test]
fn weighted_omission_leaves_existing_independent_samples_unchanged() {
    success(
        &[
            "--narm", "wmean", "1:2", "mean", "1", "mean", "2", "dotprod", "1:2", "pcov", "1:2",
            "scov", "1:2", "ppearson", "1:2", "spearson", "1:2",
        ],
        b"1\tNA\nNA\t2\n3\t4\n",
        b"3\t2\t3\t14\t1\t2\t1\t1\n",
    );
    success(
        &[
            "--narm", "mean", "1", "mean", "2", "wmean", "1:2", "wmean", "1:2", "wmean", "1:3",
        ],
        b"NA\t1\t2\n10\tNA\t1\n20\t3\tNA\n30\t1\t1\n",
        b"20\t1.6666666666667\t22.5\t22.5\t20\n",
    );
    error(
        &["--narm", "wmean", "1:2", "dotprod", "1:2"],
        b"1\tNA\n3\t2\n",
        1,
        b"different number of items",
    );
}

#[test]
fn weighted_grammar_preserves_existing_mode_and_option_rules() {
    for args in [
        vec!["wmean", "1"],
        vec!["wmean", "1-2"],
        vec!["wmean", "1:"],
        vec!["wmean", ":2"],
        vec!["wmean", "1:2:3"],
        vec!["wmean", "1:2-3"],
        vec!["wmean:2", "1:2"],
        vec!["wmean:bad", "1:2"],
        vec!["wmean", "1:2", "cut", "1"],
        vec!["top:1", "1", "wmean", "1:2"],
        vec!["crosstab", "1,2", "wmean", "3:4"],
    ] {
        let (status, stdout, diagnostics) = run(&args, b"bad\n");
        assert_eq!(status, 1, "{args:?}: {diagnostics:?}");
        assert!(stdout.is_empty(), "{args:?}");
        assert!(!diagnostics.is_empty());
        assert!(
            !diagnostics
                .iter()
                .any(|message| message.windows(8).any(|part| part == b"in line "))
        );
    }
    success(&["--nar", "wmean", "1:2"], b"NA\t1\n5\t2\n", b"5\n");
    for action in ["--help", "--version"] {
        let (status, stdout, diagnostics) = run(&[action, "wmean", "not-a-pair"], b"bad\n");
        assert_eq!(status, 0);
        assert!(!stdout.is_empty());
        assert!(diagnostics.is_empty());
    }
}

fn exact_decimal(numerator: i64, denominator: i64) -> String {
    let mut result = if numerator < 0 {
        "-".to_owned()
    } else {
        String::new()
    };
    let numerator = numerator.abs();
    result.push_str(&(numerator / denominator).to_string());
    let mut remainder = numerator % denominator;
    if remainder != 0 {
        result.push('.');
    }
    while remainder != 0 {
        remainder *= 10;
        result.push(char::from(b'0' + (remainder / denominator) as u8));
        remainder %= denominator;
    }
    result.push('\n');
    result
}

#[test]
fn deterministic_exact_rationals_verify_contribution_properties() {
    // Values and weights are small integers, every total is exactly representable,
    // and the denominator is a power of two. Integer arithmetic is an independent
    // formula oracle; none of these expectations uses production accumulation.
    for seed in 0..64i64 {
        let values = [
            seed - 32,
            (seed * 13 % 65) - 32,
            (seed * 7 % 65) - 32,
            32 - seed,
        ];
        for weights in [[1, 1, 1, 1], [1, 1, 2, 4]] {
            let total: i64 = weights.iter().sum();
            let numerator: i64 = values
                .iter()
                .zip(weights)
                .map(|(&value, weight)| value * weight)
                .sum();
            let expected = exact_decimal(numerator, total);
            for scale in [1, 2, 8] {
                let input: String = values
                    .iter()
                    .zip(weights)
                    .map(|(&value, weight)| format!("{value}\t{}\n", weight * scale))
                    .collect();
                success(&["wmean", "1:2"], input.as_bytes(), expected.as_bytes());
                success(
                    &["wmean", "1:2"],
                    format!("{input}999\t0\n-999\t-0\n").as_bytes(),
                    expected.as_bytes(),
                );
            }
            let replicated: String = values
                .iter()
                .zip(weights)
                .flat_map(|(&value, weight)| (0..weight).map(move |_| format!("{value}\t1\n")))
                .collect();
            success(
                &["wmean", "1:2"],
                replicated.as_bytes(),
                expected.as_bytes(),
            );
        }
    }
}

#[test]
fn weighted_commands_complete_short_writes_and_refuse_incomplete_transports() {
    for segment in [1, 2, 7, 128] {
        let mut input = Input {
            bytes: b"10\t1\n20\t3",
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        assert_eq!(
            command(&mut input, &mut output, &["wmean", "1:2"]),
            (0, vec![])
        );
        assert_eq!(output.bytes, b"17.5\n");
        assert!(output.closed);
    }
    for code in [9, 5] {
        let mut input = Input {
            bytes: b"inf\t1\n10\t1\n",
            segment: 2,
            error: Some(code),
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: None,
            closed: false,
        };
        let (status, diagnostics) = command(&mut input, &mut output, &["wmean", "1:2"]);
        assert_eq!(status, 1);
        assert_eq!(
            diagnostics,
            vec![if code == 9 {
                b"read error: Bad file descriptor\n".to_vec()
            } else {
                b"read error: Input/output error\n".to_vec()
            }]
        );
        assert!(output.closed);
    }
    for code in [28, 5] {
        let mut input = Input {
            bytes: b"10\t1\n20\t3\n",
            segment: 2,
            error: None,
        };
        let mut output = Output {
            bytes: Vec::new(),
            error: Some(code),
            closed: false,
        };
        let (status, diagnostics) = command(&mut input, &mut output, &["wmean", "1:2"]);
        assert_eq!(status, if code == 28 { 1 } else { 77 });
        assert!(!diagnostics.is_empty());
        assert!(output.closed);
    }
}

#[test]
fn weighted_header_reads_distinguish_empty_input_from_incomplete_input() {
    for csv in [false, true] {
        let header = if csv {
            b"key,value,weight\n".as_slice()
        } else {
            b"key\tvalue\tweight\n"
        };
        let partial = if csv {
            b"key,value".as_slice()
        } else {
            b"key\tvalue"
        };
        for bytes in [b"".as_slice(), partial, header] {
            for full in [false, true] {
                let mut args = vec!["-H", "-s", "-g1", "wmean", "value:weight"];
                if csv {
                    args.push("--csv");
                }
                if full {
                    args.push("--full");
                }
                let mut input = Input {
                    bytes,
                    segment: 1,
                    error: Some(5),
                };
                let mut output = Output {
                    bytes: vec![],
                    error: None,
                    closed: false,
                };
                let (status, diagnostics) = command(&mut input, &mut output, &args);
                assert_eq!(status, 1, "{diagnostics:?}");
                assert_eq!(
                    diagnostics.last().unwrap(),
                    b"read error: Input/output error\n"
                );
                assert_eq!(diagnostics.len(), 1);
                if bytes == header {
                    assert!(!output.bytes.is_empty());
                } else {
                    assert!(output.bytes.is_empty());
                }
                assert!(output.closed);
            }
        }
    }
}

#[test]
fn weighted_output_close_failure_keeps_ordinary_failure_precedence() {
    use super::{Write, command_output, io};
    struct Closing(Output, i32);
    impl Write for Closing {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl command_output::Transport for Closing {
        fn buffering(&self) -> (usize, bool) {
            self.0.buffering()
        }
        fn close(&mut self) -> io::Result<()> {
            self.0.closed = true;
            Err(io::Error::from_raw_os_error(self.1))
        }
    }
    for code in [28, 5] {
        for (args, bytes, calculation_status, expected) in [
            (
                vec!["-H", "-s", "-g", "key", "wmean", "value:weight"],
                b"".as_slice(),
                0,
                b"".as_slice(),
            ),
            (
                vec!["--csv", "-H", "-s", "-g", "key", "wmean", "value:weight"],
                b"",
                0,
                b"",
            ),
            (
                vec!["wmean", "1:2"],
                b"5\t2\n".as_slice(),
                0,
                b"5\n".as_slice(),
            ),
            (vec!["wmean", "1:2"], b"5\t-1\n", 1, b""),
            (vec!["wmean", "1:2"], b"2\t1e4932\n", 77, b""),
            (
                vec!["-g1", "wmean", "2:3"],
                b"a\t10\t1\nb\t5\t2\n",
                0,
                b"a\t10\nb\t5\n",
            ),
            (
                vec!["-s", "-g1", "wmean", "2:3"],
                b"b\t5\t2\na\t10\t1\n",
                0,
                b"a\t10\nb\t5\n",
            ),
            (
                vec!["-g1", "wmean", "2:3"],
                b"a\t10\t1\nb\t5\t0\n",
                1,
                b"a\t10\nb\t",
            ),
            (
                vec!["-g1", "wmean", "2:3"],
                b"a\t10\t1\nb\t2\t1e4932\n",
                77,
                b"a\t10\n",
            ),
            (vec!["--csv", "wmean", "1:2"], b"5,2\n", 0, b"5\n"),
            (vec!["--csv", "wmean", "1:2"], b"5,-1\n", 1, b""),
            (vec!["--csv", "wmean", "1:2"], b"2,1e4932\n", 77, b""),
            (
                vec!["--csv", "-g1", "wmean", "2:3"],
                b"a,10,1\nb,5,2\n",
                0,
                b"a,10\nb,5\n",
            ),
            (
                vec!["--csv", "-s", "-g1", "wmean", "2:3"],
                b"b,5,2\na,10,1\n",
                0,
                b"a,10\nb,5\n",
            ),
            (
                vec!["--csv", "-g1", "wmean", "2:3"],
                b"a,10,1\nb,5,0\n",
                1,
                b"a,10\nb,",
            ),
            (
                vec!["--csv", "-g1", "wmean", "2:3"],
                b"a,10,1\nb,2,1e4932\n",
                77,
                b"a,10\n",
            ),
        ] {
            let mut input = Input {
                bytes,
                segment: 1,
                error: None,
            };
            let mut output = Closing(
                Output {
                    bytes: Vec::new(),
                    error: None,
                    closed: false,
                },
                code,
            );
            let (status, diagnostics) = command(&mut input, &mut output, &args);
            assert_eq!(
                status,
                if calculation_status == 77 || code == 5 {
                    77
                } else {
                    1
                }
            );
            assert!(!diagnostics.is_empty());
            if bytes.is_empty() {
                assert_eq!(diagnostics.len(), 1);
            }
            assert_eq!(output.0.bytes, expected);
            assert_eq!(
                diagnostics.last().unwrap(),
                if code == 28 {
                    b"write error: No space left on device\n".as_slice()
                } else {
                    b"unsupported output I/O error\n".as_slice()
                }
            );
            if args[0] == "-g1" && calculation_status == 1 {
                assert_eq!(diagnostics[0], b"wmean fields 2:3 have no positive retained contribution weight\ngroup keys: field 1='b'\n");
            }
            assert!(output.0.closed);
        }
    }
}

#[test]
fn sorted_weighted_read_failures_never_complete_a_partial_group() {
    use super::command_test_support::command_in;
    for memory in ["67108864", "1"] {
        for grouping in ["sort", "hash:restart=2"] {
            for csv_out in [false, true] {
                for output_error in [None, Some(5)] {
                    let mut input = Input {
                        bytes: b"key\tvalue\tweight\nb\tinf\t1\na\t5\t2\nb\t7\t1\n",
                        segment: 2,
                        error: Some(5),
                    };
                    let mut output = Output {
                        bytes: vec![],
                        error: output_error,
                        closed: false,
                    };
                    let mut args = vec!["-H", "-s", "-g1", "wmean", "2:3"];
                    if csv_out {
                        args.push("--csv-out");
                    }
                    let environment = super::Environment {
                        locale: Default::default(),
                        posixly_correct: false,
                        grouping: Some(grouping.into()),
                        sort_memory: Some(memory.into()),
                        pipe_grouping: Some("hash".into()),
                        terminal: Default::default(),
                    };
                    let (status, diagnostics) =
                        command_in(&mut input, &mut output, &args, environment);
                    assert_eq!(
                        status,
                        if output_error.is_some() { 77 } else { 1 },
                        "{diagnostics:?}"
                    );
                    assert!(
                        diagnostics
                            .iter()
                            .any(|message| message == b"read error: Input/output error\n")
                    );
                    assert_eq!(
                        output.bytes,
                        if output_error.is_some() {
                            b"".as_slice()
                        } else if csv_out {
                            b"GroupBy(key),\"wmean(value,weight)\"\n".as_slice()
                        } else {
                            b"GroupBy(key)\twmean(value,weight)\n"
                        }
                    );
                    assert!(output.closed);
                }
            }
        }
    }
}

#[test]
fn a_failed_weighted_hash_rewind_cannot_emit_abandoned_contributions() {
    use super::{BufRead, io, replay};
    use std::io::Read;
    struct BrokenRewind<'a>(Input<'a>);
    impl Read for BrokenRewind<'_> {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            self.0.read(bytes)
        }
    }
    impl BufRead for BrokenRewind<'_> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            self.0.fill_buf()
        }
        fn consume(&mut self, count: usize) {
            self.0.consume(count);
        }
    }
    impl replay::Rewind for BrokenRewind<'_> {
        fn mark(&mut self) -> Option<u64> {
            Some(0)
        }
        fn rewind(&mut self, _: u64) -> io::Result<()> {
            Err(io::Error::from_raw_os_error(5))
        }
    }
    let mut input = BrokenRewind(Input {
        bytes: b"key\tvalue\tweight\nb\t5\t2\na\t10\t1\nb\t7\t2\n",
        segment: 2,
        error: None,
    });
    let mut output = Output {
        bytes: vec![],
        error: None,
        closed: false,
    };
    let environment = super::Environment {
        locale: Default::default(),
        posixly_correct: false,
        grouping: Some("hash:restart=2".into()),
        sort_memory: Some("67108864".into()),
        pipe_grouping: None,
        terminal: Default::default(),
    };
    let args = ["-H", "-s", "-g", "key", "wmean", "value:weight"].map(Into::into);
    let mut diagnostics = vec![];
    let mut report = |failure: &super::Failure| {
        diagnostics.push(failure.message.clone());
        true
    };
    let status = super::run_in(
        &mut input,
        &mut output,
        &args,
        b"fastmash",
        &mut report,
        environment,
    )
    .unwrap_or_else(|failure| {
        report(&failure);
        failure.status
    });
    assert_eq!(status, 1);
    assert!(output.bytes.is_empty());
    assert_eq!(
        diagnostics,
        vec![b"read error: Input/output error\n".to_vec()]
    );
}

#[test]
fn weighted_command_checked_allocations_fail_safely() {
    for (args, bytes, expected) in [
        (
            vec!["wmean", "1:2,3:2", "sum", "1"],
            b"10\t1\t2\n20\t3\t4\n".as_slice(),
            b"17.5\t3.5\t30\n".as_slice(),
        ),
        (
            vec!["--csv", "wmean", "1:2,3:2", "sum", "1"],
            b"10,1,2\n20,3,4\n",
            b"17.5,3.5,30\n",
        ),
        (
            vec!["--csv", "-s", "-g4", "wmean", "1:2,3:2", "sum", "1"],
            b"10,1,2,a\n20,3,4,a\n",
            b"a,17.5,3.5,30\n",
        ),
    ] {
        let mut completed = false;
        let mut refusals = 0;
        for at in 0..200 {
            super::command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let actual = run(&args, bytes);
            super::command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            if actual.0 == 0 {
                assert_eq!(actual.1, expected);
                assert!(actual.2.is_empty());
                completed = true;
                break;
            }
            assert_eq!(actual.0, 77);
            assert_eq!(
                actual.2,
                vec![b"command memory allocation failed\n".to_vec()]
            );
            refusals += 1;
        }
        assert!(completed && refusals > 0);
    }
}

#[test]
fn a_missing_only_group_remains_a_distinct_failing_domain() {
    assert_eq!(
        run(
            &["--narm", "-g", "1,2", "wmean", "3:4"],
            b"a\tx\t10\t1\nb\ty\tNA\t1\na\tx\t20\t3\n",
        ),
        (
            1,
            b"a\tx\t10\nb\ty\t".to_vec(),
            vec![b"wmean fields 3:4 have no positive retained contribution weight\ngroup keys: field 1='b', field 2='y'\n".to_vec()],
        ),
    );
}

#[test]
fn named_pairs_bind_escape_bytes_and_expand_in_request_order() {
    success(
        &[
            "-H", "--result-name=2:second", "wmean",
            "value\\:raw:weight\\,raw,3:weight\\,raw,value\\:raw:2,2:2,value\\:raw:weight\\,raw",
            "sum", "1,3",
        ],
        b"value:raw\tweight,raw\tother\tweight,raw\n10\t1\t4\t99\n20\t3\t8\t99\n",
        b"wmean(value:raw,weight,raw)\tsecond\twmean(value:raw,weight,raw)\twmean(weight,raw,weight,raw)\twmean(value:raw,weight,raw)\tsum(value:raw)\tsum(other)\n17.5\t7\t17.5\t2.5\t17.5\t30\t12\n",
    );
    success(
        &["--header-in", "wmean", "reading:weight"],
        b"reading\tweight\n10\t1\n20\t3\n",
        b"17.5\n",
    );
    assert_eq!(
        run(&["wmean", "reading:2"], b"10\t1\n"),
        (
            1,
            vec![],
            vec![b"-H or --header-in must be used with named columns\n".to_vec()]
        ),
    );
    assert_eq!(
        run(
            &["-H", "wmean", "reading:absent"],
            b"reading\tweight\n10\t1\n"
        ),
        (
            1,
            vec![],
            vec![b"column name 'absent' not found in input file\n".to_vec()]
        ),
    );
}

#[test]
fn weighted_headers_keep_aggregate_timing_and_required_pair_fields() {
    success(&["--header-out", "wmean", "1:2"], b"", b"");
    success(
        &["-H", "wmean", "value:weight"],
        b"value\tweight\n",
        b"wmean(value,weight)\n",
    );
    success(&["-H", "wmean", "value:weight"], b"", b"");
    success(
        &["-H", "-g", "key", "wmean", "value:weight"],
        b"key\tvalue\tweight\n",
        b"GroupBy(key)\twmean(value,weight)\n",
    );
    success(
        &["--header-out", "wmean", "1:2"],
        b"10\t1\n20\t3\n",
        b"wmean(field-1,field-2)\n17.5\n",
    );
    for args in [
        vec!["--result-name=1:average", "wmean", "1:2"],
        vec![
            "--header-in",
            "--result-name=1:average",
            "wmean",
            "value:weight",
        ],
    ] {
        assert_eq!(
            run(&args, b""),
            (
                1,
                vec![],
                vec![b"--result-name requires --header-out or -H\n".to_vec()]
            )
        );
    }
    assert_eq!(
        run(
            &["--header-out", "--result-name=3:extra", "wmean", "1:2,2:2"],
            b""
        ),
        (
            1,
            vec![],
            vec![b"--result-name target exceeds the number of results\n".to_vec()]
        ),
    );
    assert_eq!(
        run(
            &["-H", "--result-name=1:average", "wmean", "1:2"],
            b"value\n"
        ),
        (
            1,
            vec![],
            vec![b"invalid input: field 2 requested, line 1 has only 1 fields\n".to_vec()]
        ),
    );
    assert_eq!(
        run(
            &["--header-out", "--result-name=1:average", "wmean", "1:2"],
            b"bad\t1\n"
        ),
        (
            1,
            b"average\n".to_vec(),
            vec![b"invalid numeric value in line 1 field 1: 'bad'\n".to_vec()]
        ),
    );
}

#[test]
fn adjacent_groups_reset_each_request_and_preserve_separate_runs() {
    success(
        &["-H", "-g", "key,kind", "--result-name=2:other", "wmean", "value:weight,other:weight", "wmean", "weight:weight", "count", "value"],
        b"key\tkind\tvalue\tweight\tother\na\tx\t10\t1\t4\na\tx\t20\t3\t8\nb\tx\t5\t2\t9\na\tx\t30\t1\t2\na\ty\t-2\t4\t3\n",
        b"GroupBy(key)\tGroupBy(kind)\twmean(value,weight)\tother\twmean(weight,weight)\tcount(value)\na\tx\t17.5\t7\t2.5\t2\nb\tx\t5\t9\t2\t1\na\tx\t30\t2\t1\t1\na\ty\t-2\t3\t4\t1\n",
    );
    success(
        &["-ig1", "wmean", "2:3"],
        b"A\t10\t1\na\t20\t3\nB\t5\t2\n",
        b"A\t17.5\nB\t5\n",
    );
    success(
        &["-g1", "wmean", "2:3"],
        b"A\t10\t1\na\t20\t3\n",
        b"A\t10\na\t20\n",
    );
    success(
        &["-g1", "wmean", "2:3"],
        b"\t10\t1\n\t20\t3\na\t5\t2\n",
        b"\t17.5\na\t5\n",
    );
    // The existing equality compares equal-length key bytes through NUL.
    success(
        &["-g1", "wmean", "2:3"],
        b"a\0x\t10\t1\na\0y\t20\t3\na\0yy\t5\t2\n",
        b"a\0x\t17.5\na\0yy\t5\n",
    );
}

#[test]
fn grouped_omission_is_local_and_keys_are_required_before_omission() {
    success(
        &[
            "--narm",
            "-g1",
            "wmean",
            "2:3,2:4,3:2",
            "mean",
            "2,3",
            "count",
            "2",
        ],
        b"a\tNA\t2\t1\na\t10\tNA\t1\na\t20\t3\tNA\na\t30\t1\t1\nb\t5\t2\t4\n",
        b"a\t22.5\t20\t1.8\t20\t2\t3\nb\t5\t5\t2\t5\t2\t1\n",
    );
    assert_eq!(
        run(&["--narm", "-g3", "wmean", "1:2"], b"10\t1\ta\nNA\t1\n"),
        (
            1,
            vec![],
            vec![b"invalid input: field 3 requested, line 2 has only 2 fields\n".to_vec()]
        ),
    );
    for input in [
        b"a\t10\t1\nb\t2\t0\n".as_slice(),
        b"a\t10\t1\nb\tNA\t1\nb\t2\t0\n",
    ] {
        assert_eq!(
            run(&["--narm", "-g1", "count", "2", "wmean", "2:3"], input),
            (1, b"a\t1\t10\nb\t1\t".to_vec(), vec![b"wmean fields 2:3 have no positive retained contribution weight\ngroup keys: field 1='b'\n".to_vec()]),
        );
    }
    assert_eq!(
        run(
            &["--narm", "-g1", "wmean", "2:3", "dotprod", "2:3"],
            b"a\t1\tNA\na\t3\t2\n"
        ),
        (
            1,
            b"a\t3\t".to_vec(),
            vec![
                b"input error for operation 'dotprod': fields 2,3 have different number of items\n"
                    .to_vec()
            ]
        ),
    );
    // Unweighted completion errors keep their original bytes and ordering.
    assert_eq!(
        run(
            &["--narm", "-g1", "dotprod", "2:3", "wmean", "2:3"],
            b"a\t1\tNA\na\t3\t2\n"
        ),
        (
            1,
            b"a\t".to_vec(),
            vec![
                b"input error for operation 'dotprod': fields 2,3 have different number of items\n"
                    .to_vec()
            ]
        ),
    );
    error(
        &["-g1", "wmean", "4:5,2:3"],
        b"a\tbad\t-1\t1\t-2\n",
        1,
        b"field 5",
    );
    error(
        &["-g1", "wmean", "2:3,4:5"],
        b"a\tbad\t-1\t1\t-2\n",
        1,
        b"field 2",
    );
}

#[test]
fn text_controls_preserve_weighted_intake_and_presentation() {
    success(
        &[
            "-t|",
            "--output-delimiter=;",
            "-g1",
            "--round=2",
            "wmean",
            "2:3",
        ],
        b"a|10|1\na|20|3",
        b"a;17.50\n",
    );
    success(
        &[
            "-WC",
            "-H",
            "-g",
            "site",
            "--format=%.1f",
            "wmean",
            "value:weight",
        ],
        b" # comment\nsite value weight\n a 10 1 \n ; ignored\n a\t20\t3\n",
        b"GroupBy(site)\twmean(value,weight)\na\t17.5\n",
    );
    success(
        &["-zH", "-g1", "wmean", "2:3"],
        b"key\tvalue\tweight\0a\nmore\t10\t1\0a\nmore\t20\t3",
        b"GroupBy(key)\twmean(value,weight)\0a\nmore\t17.5\0",
    );
    assert_eq!(
        run(
            &["-C", "-H", "wmean", "value:weight"],
            b"# ignored\nvalue\tweight\n; ignored\n1\tbad\n"
        ),
        (
            1,
            b"wmean(value,weight)\n".to_vec(),
            vec![b"invalid numeric value in line 2 field 2: 'bad'\n".to_vec()]
        ),
    );
    error(&["-g1", "wmean", "2:3"], b"a\t2\t\n", 1, b"field 3");
    error(
        &["-g1", "wmean", "2:3"],
        b"a\t2\n",
        1,
        b"line 1 has only 2 fields",
    );
}

#[test]
fn byte_oriented_unused_content_does_not_add_a_health_scan() {
    let mut input = b"a\xff\t10\t1\t".to_vec();
    input.extend(vec![0xff; 100_000]);
    input.extend_from_slice(b"\t\n a\t5\t2\na\xff\t20\t3\t\0raw\t");
    success(
        &["-g1", "wmean", "2:3"],
        &input,
        b"a\xff\t10\n a\t5\na\xff\t20\n",
    );
    let mut input = vec![b'x'; 100_000];
    input.extend_from_slice(b"\t10\t1\t\nshort\t20\t3");
    success(&["wmean", "2:3"], &input, b"17.5\n");
}

#[test]
fn numeric_and_case_locales_apply_to_adjacent_weighted_reports() {
    use super::command_test_support::command_in;
    for (locale, args, bytes, expected) in [
        (
            "de_DE.UTF-8",
            vec!["-H", "-g1", "--format=%.2f", "wmean", "2:3"],
            b"key\tvalue\tweight\na\t10,5\t1\na\t20,5\t3\n".as_slice(),
            b"GroupBy(key)\twmean(value,weight)\na\t18,00\n".as_slice(),
        ),
        (
            "tr_TR.UTF-8",
            vec!["-g1", "wmean", "2:3"],
            b"I\t10\t1\ni\t20\t3\n",
            b"I\t10\ni\t20\n",
        ),
        (
            "en_US.UTF-8",
            vec!["-ig1", "wmean", "2:3"],
            b"I\t10\t1\ni\t20\t3\n",
            b"I\t17.5\n",
        ),
    ] {
        let mut input = Input {
            bytes,
            segment: 1,
            error: None,
        };
        let mut output = Output {
            bytes: vec![],
            error: None,
            closed: false,
        };
        let environment = super::Environment {
            locale: super::locale::Policy::resolve(|name| {
                (name == "LC_ALL").then(|| locale.into())
            }),
            posixly_correct: false,
            grouping: None,
            sort_memory: None,
            pipe_grouping: None,
            terminal: Default::default(),
        };
        assert_eq!(
            command_in(&mut input, &mut output, &args, environment),
            (0, vec![])
        );
        assert_eq!(output.bytes, expected);
        assert!(output.closed);
    }
}

#[test]
fn grouped_headers_copies_and_results_survive_transport_faults() {
    use super::{Write, command_output, io};
    struct Limited {
        output: Output,
        remaining: usize,
        code: i32,
    }
    impl Write for Limited {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.remaining == 0 {
                return Err(io::Error::from_raw_os_error(self.code));
            }
            let count = self
                .output
                .write(&bytes[..bytes.len().min(self.remaining)])?;
            self.remaining -= count;
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl command_output::Transport for Limited {
        fn buffering(&self) -> (usize, bool) {
            self.output.buffering()
        }
        fn close(&mut self) -> io::Result<()> {
            self.output.close()
        }
    }
    let bytes = b"key\tvalue\tweight\tnote\na\t10\t1\tfirst\na\t20\t3\theavy\nb\t5\t2\tlast\n";
    let args = [
        "-H",
        "--full",
        "-g1",
        "--result-name=1:average",
        "wmean",
        "2:3",
    ];
    let expected = b"key\tvalue\tweight\tnote\taverage\na\t10\t1\tfirst\t17.5\nb\t5\t2\tlast\t5\n";
    for segment in [1, 2, 7, 128] {
        let mut input = Input {
            bytes,
            segment,
            error: None,
        };
        let mut output = Output {
            bytes: vec![],
            error: None,
            closed: false,
        };
        assert_eq!(command(&mut input, &mut output, &args), (0, vec![]));
        assert_eq!(output.bytes, expected);
        assert!(output.closed);
    }
    for (code, status, diagnostic) in [
        (28, 1, b"write error: No space left on device\n".as_slice()),
        (5, 77, b"unsupported output I/O error\n"),
    ] {
        for remaining in [0, 4, 29, 34, 43, 47, expected.len() - 1] {
            let mut input = Input {
                bytes,
                segment: 2,
                error: None,
            };
            let mut output = Limited {
                output: Output {
                    bytes: vec![],
                    error: None,
                    closed: false,
                },
                remaining,
                code,
            };
            assert_eq!(
                command(&mut input, &mut output, &args),
                (status, vec![diagnostic.to_vec()])
            );
            assert_eq!(output.output.bytes, expected[..remaining]);
            assert!(output.output.closed);
        }
    }
}

#[test]
fn grouped_late_read_failure_keeps_completed_output_and_failure_precedence() {
    for (code, status, diagnostic) in [
        (None, 1, None),
        (Some(28), 1, Some(b"write error\n".as_slice())),
        (
            Some(5),
            77,
            Some(b"unsupported output I/O error\n".as_slice()),
        ),
    ] {
        let mut input = Input {
            bytes: b"key\tvalue\tweight\na\t10\t1\nb\t5\t2\n",
            segment: 1,
            error: Some(5),
        };
        let mut output = Output {
            bytes: vec![],
            error: code,
            closed: false,
        };
        let mut diagnostics = vec![b"read error: Input/output error\n".to_vec()];
        if let Some(diagnostic) = diagnostic {
            diagnostics.push(diagnostic.to_vec());
        }
        assert_eq!(
            command(&mut input, &mut output, &["-H", "-g1", "wmean", "2:3"]),
            (status, diagnostics)
        );
        assert_eq!(
            output.bytes,
            if code.is_some() {
                b"".as_slice()
            } else {
                b"GroupBy(key)\twmean(value,weight)\na\t10\nb\t5\n"
            }
        );
        assert!(output.closed);
    }
}

#[test]
fn weighted_full_groups_require_every_key_before_pair_omission() {
    for (args, input, stdout, diagnostic) in [
        (
            vec!["--full", "-g3", "wmean", "1:2"],
            b"5\t2\n".as_slice(),
            b"".as_slice(),
            b"invalid input: field 3 requested, line 1 has only 2 fields\n".as_slice(),
        ),
        (
            vec!["--full", "--narm", "-g3", "wmean", "1:2"],
            b"NA\t2\n",
            b"",
            b"invalid input: field 3 requested, line 1 has only 2 fields\n",
        ),
        (
            vec!["--full", "--narm", "-g3", "wmean", "1:2"],
            b"NA\n",
            b"",
            b"invalid input: field 3 requested, line 1 has only 1 fields\n",
        ),
        (
            vec!["--full", "--narm", "-g4", "wmean", "1:2"],
            b"NA\tbad\n",
            b"",
            b"invalid input: field 4 requested, line 1 has only 2 fields\n",
        ),
        (
            vec!["--full", "-g1,4", "wmean", "2:3"],
            b"a\t10\t1\tx\nb\t20\t3\n",
            b"a\t10\t1\tx\t10\n",
            b"invalid input: field 4 requested, line 2 has only 3 fields\n",
        ),
        (
            vec!["--full", "--narm", "-g1,4", "wmean", "2:3"],
            b"a\t10\t1\tx\nb\tNA\t3\n",
            b"a\t10\t1\tx\t10\n",
            b"invalid input: field 4 requested, line 2 has only 3 fields\n",
        ),
        (
            vec!["-g1,4", "--narm", "wmean", "2:3"],
            b"a\t10\t1\tx\nb\tNA\t3\n",
            b"a\tx\t10\n",
            b"invalid input: field 4 requested, line 2 has only 3 fields\n",
        ),
    ] {
        assert_eq!(
            run(&args, input),
            (1, stdout.to_vec(), vec![diagnostic.to_vec()])
        );
    }
    success(
        &["--full", "-g1,4", "wmean", "2:3"],
        b"a\t10\t1\t\na\t20\t3\t\n",
        b"a\t10\t1\t\t17.5\n",
    );
    // Ordinary Commands retain their established short-circuit key checks.
    success(&["--full", "-g3", "mean", "1"], b"5\t2\n", b"5\t2\t5\n");
    success(
        &["--full", "-g3", "dotprod", "1:2"],
        b"5\t2\n",
        b"5\t2\t10\n",
    );
}

#[test]
fn preceding_group_completion_keeps_priority_over_a_new_absent_key() {
    assert_eq!(
        run(
            &["--full", "-g1,4", "wmean", "2:3"],
            b"a\t0x1.fffffffffffffffep+16383\t0x1p-1\tx\na\t0x1.fffffffffffffffep+16383\t0x1p-65\tx\nb\t5\t2\n",
        ),
        (77, b"a\t0x1.fffffffffffffffep+16383\t0x1p-1\tx\t".to_vec(), vec![b"wmean final division overflow exceeds the supported numerical range\ngroup keys: field 1='a', field 4='x'\n".to_vec()]),
    );
}
