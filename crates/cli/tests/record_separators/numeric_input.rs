use super::decimal_fixtures as fixtures;
use super::*;

fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    let mut tokens: Vec<String> = [
        "10e-4951",
        "1.0e-4950",
        "10e-4933",
        "0e-999999",
        "1e999999",
        "3.36210314311209350626e-4932",
        "3.36210314311209350626267781732175260e-4932",
        "0x1p-16445",
        "0x1.1p-16445",
        "0x1",
        "-0x1.8",
        "0x1.",
        "0x.8",
        "0x1p+",
        "1e-",
        "1e-4950junk",
        "nan(",
        "nan(a-b)",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    tokens.push(fixtures::dyadic(1, 16445));
    for offset in [0, 1, 2, 3, 4, 5, 8] {
        let exact = fixtures::dyadic((1u128 << 66) - offset, 16448);
        tokens.push(exact.clone());
        tokens.push(format!("-{exact}"));
        if offset == 2 || offset == 4 {
            tokens.push(format!("{exact}{}1", "0".repeat(256)));
            let mut below = exact.into_bytes();
            *below.last_mut().unwrap() -= 1;
            below.extend_from_slice("9".repeat(257).as_bytes());
            tokens.push(String::from_utf8(below).unwrap());
        }
    }
    for token in tokens {
        for a in [
            args(&[
                "--format=%La",
                "sum",
                "1",
                "min",
                "1",
                "max",
                "1",
                "mean",
                "1",
            ]),
            args(&[
                "round", "1", "floor", "1", "ceil", "1", "trunc", "1", "frac", "1",
            ]),
            args(&["getnum:d", "1"]),
            args(&["bin:1", "1"]),
        ] {
            cases.push((a, format!("{token}\n").into_bytes()));
        }
    }
    for token in ["10e-4951", "3.36210314311209350626e-4932", "0x1p-16445"] {
        for spill in [false, true] {
            let mut a = args(&["-g", "1", "sum", "2", "min", "2", "median", "2"]);
            if spill {
                a.insert(0, "-s".into());
            }
            cases.push((a, format!("a\t{token}\na\t{token}\nb\t1\n").into_bytes()));
        }
        // Preserve prior completed output and requested operation error order.
        for a in [
            args(&["round", "1"]),
            args(&["sum", "1", "sum", "2"]),
            args(&["sum", "2", "sum", "1"]),
        ] {
            cases.push((a, format!("1\t2\n{token}\n").into_bytes()));
        }
    }
    cases.push((args(&["-t", "e", "sum", "1"]), b"10e-4951\n".to_vec()));
    cases.push((
        args(&["-t", ".", "sum", "1", "sum", "2"]),
        b"1.25\n".to_vec(),
    ));
    for length in [511, 512, 513] {
        cases.push((
            args(&["-t", ".", "sum", "1"]),
            format!("{}.5\n", "0".repeat(length)).into_bytes(),
        ));
    }
    cases.push((args(&["sum", "1"]), b"1\0e-4950\n".to_vec()));
    cases.push((args(&["--narm", "sum", "1"]), b"NA\n10e-4951\n".to_vec()));
    cases
}

#[test]
fn shared_tiny_input_repairs() {
    let binary = candidate();
    let valid = invoke(
        &binary,
        &args(&["sum", "1"]),
        b"3.36210314311209350626e-4932\n",
        false,
        false,
    );
    assert!(valid.status.success(), "{:?}", valid.stderr);
    assert_eq!(valid.stdout, b"3.3621031431121e-4932\n");
    let invalid = invoke(&binary, &args(&["sum", "1"]), b"10e-4951\n", false, false);
    assert_eq!(invalid.status.code(), Some(1));
    assert_eq!(
        invalid.stderr,
        b"fastmash: invalid numeric value in line 1 field 1: '10e-4951'\n"
    );
    let extracted = invoke(
        &binary,
        &args(&["getnum:d", "1"]),
        format!("0.{}1\n", "0".repeat(4950)).as_bytes(),
        false,
        false,
    );
    assert!(extracted.status.success());
    assert_eq!(extracted.stdout, b"0\n");
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn numeric_input_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    compare_cases(&cases(), &reference, &evidence);
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn shared_parser_routes_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for token in [
        "nan(42)".to_owned(),
        "10e-4951".to_owned(),
        "3.36210314311209350626e-4932".to_owned(),
        "0x1p-16445".to_owned(),
        fixtures::dyadic(1, 16445),
        format!("{}1", "0".repeat(10_000)),
        "nan(1)x".to_owned(),
        "1\0x".to_owned(),
    ] {
        for op in ["harmmean", "geomean", "ms", "rms", "pcov", "scov"] {
            let field = if op.ends_with("cov") { "1:2" } else { "1" };
            cases.push((
                args(&[op, field]),
                format!("{token}\t2\n{token}\t3\n").into_bytes(),
            ));
        }
    }
    for op in ["harmmean", "pcov"] {
        let field = if op == "pcov" { "1:2" } else { "1" };
        for input in ["NA\t2\n1\t3\n", "bad\n", "1\tbad\n"] {
            cases.push((args(&["--narm", op, field]), input.as_bytes().to_vec()));
        }
    }
    for length in [511, 512, 513] {
        cases.push((
            args(&["-t", ".", "harmmean", "1"]),
            format!("{}.5\n", "0".repeat(length)).into_bytes(),
        ));
    }
    cases.push((args(&["-t", "e", "harmmean", "1"]), b"10e-4951\n".to_vec()));
    let wide = "x".repeat(10_000);
    cases.push((
        args(&["harmmean", "1"]),
        format!("1\t{wide}\nwrong\t{wide}\n").into_bytes(),
    ));
    for full in [false, true] {
        let mut a = args(&["-Hsg1", "harmmean", "2", "pcov", "2:3"]);
        if full {
            a.insert(0, "--full".into());
        }
        cases.push((
            a,
            format!("key\tvalue\tother\t{wide}\nb\t1\t2\t{wide}\na\t2\t4\t{wide}\n").into_bytes(),
        ));
    }
    cases.push((
        args(&["-S", "42", "rand", "1"]),
        format!("{wide}\n").into_bytes(),
    ));
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn documented_nan_policy_and_unary_exceptions() {
    for (input, expected) in [
        ("nan(1)\n-nan(2)\n", "-nan\t-nan\n"),
        ("-nan(2)\nnan(2)\n", "nan\tnan\n"),
        ("-nan\nnan\n", "nan\tnan\n"),
        // Invalid arithmetic still creates the accepted positive canonical NaN.
        ("inf\n-inf\n-nan\n", "nan\tnan\n"),
        ("inf\n-inf\n-nan(1)\n", "-nan\t-nan\n"),
    ] {
        let o = invoke(
            &candidate(),
            &args(&["sum", "1", "mean", "1"]),
            input.as_bytes(),
            false,
            false,
        );
        assert!(o.status.success(), "{o:?}");
        assert_eq!(o.stdout, expected.as_bytes());
    }
    for op in ["round", "floor", "ceil", "trunc", "frac", "bin:1"] {
        let o = invoke(
            &candidate(),
            &args(&[op, "1"]),
            b"-nan(42)\nnan(42)\n",
            false,
            false,
        );
        assert!(o.status.success(), "{o:?}");
        assert_eq!(o.stdout, b"-nan\nnan\n");
    }
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn nan_input_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for payload in [
        "",
        "word",
        "a_b",
        "_",
        "0",
        "1",
        "2",
        "42",
        "052",
        "09",
        "0X2a",
        "0x",
        "00x1",
        "18446744073709551615",
        "18446744073709551616",
        "18446744073709551616x",
        "4611686018427387904",
        "9223372036854775808",
    ] {
        for sign in ["", "-", "+"] {
            let token = format!("{sign}NaN({payload})");
            cases.push((
                args(&["sum", "1", "mean", "1", "min", "1", "max", "1"]),
                format!("{token}\n").into_bytes(),
            ));
            // Opposite-sign anchors make payload ordering observable, not merely
            // a comparison of two identically printed single NaNs.
            cases.push((
                args(&["sum", "1", "mean", "1"]),
                format!("{token}\n-nan(41)\nnan(42)\n").into_bytes(),
            ));
        }
    }
    for input in [
        "nan(1)\n-nan(2)\n",
        "-nan(2)\nnan(1)\n",
        "nan(2)\n-nan(2)\n",
        "-nan(2)\nnan(2)\n",
        "-nan\nnan\n",
        "nan\n-nan\n",
        "1\n-nan(3)\n2\n",
        "-nan(3)\n1\n2\n",
        "NA\nnan(1)\n-nan(2)\n",
    ] {
        for operations in [
            vec!["sum", "1", "mean", "1"],
            vec!["harmmean", "1", "geomean", "1"],
            vec!["pvar", "1", "svar", "1", "pstdev", "1", "sstdev", "1"],
            vec![
                "min", "1", "max", "1", "absmin", "1", "absmax", "1", "range", "1",
            ],
            vec!["median", "1", "q1", "1", "q3", "1", "iqr", "1"],
        ] {
            let mut a = args(&operations);
            a.insert(0, "--narm".into());
            cases.push((a, input.as_bytes().to_vec()));
        }
    }
    for input in [
        b"nan(1)x\n".as_slice(),
        b"nan(\n",
        b"nan(a-b)\n",
        b"nan(1)\0x\n",
        b"1\nnan(+)\n",
    ] {
        for a in [
            args(&["sum", "1", "sum", "2"]),
            args(&["sum", "2", "sum", "1"]),
            args(&["round", "1"]),
        ] {
            cases.push((a, input.to_vec()));
        }
    }
    for full in [false, true] {
        for sort in [false, true] {
            let mut a = args(&[
                "-H", "-g", "key", "sum", "value", "mean", "value", "min", "value", "max", "value",
            ]);
            if full {
                a.insert(0, "--full".into());
            }
            if sort {
                a.insert(0, "-s".into());
            }
            cases.push((
                a,
                b"key\tvalue\na\tnan(1)\na\t-nan(2)\nb\t-nan(2)\nb\tnan(2)\n".to_vec(),
            ));
        }
    }
    // A delimiter inside a valid token exercises retry rather than prefix acceptance.
    cases.push((args(&["-t", "(", "sum", "1"]), b"nan(1)\n".to_vec()));
    cases.push((args(&["-z", "sum", "1"]), b"nan(1)\0-nan(2)\0".to_vec()));
    for suffix in ["", "x"] {
        cases.push((
            args(&["sum", "1"]),
            format!("nan({}{suffix})\n-nan(42)\n", "9".repeat(10_000)).into_bytes(),
        ));
    }
    compare_cases(&cases, &reference, &evidence);
}
