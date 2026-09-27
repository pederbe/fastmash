use super::*;

#[test]
fn line_numeric_approved_nan_and_ties() {
    let a = args(&[
        "round", "1", "floor", "1", "ceil", "1", "trunc", "1", "frac", "1",
    ]);
    let o = invoke(
        &candidate(),
        &a,
        b"-nan\nnan\n-0\n-0.5\n2.5\n-inf\n",
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(o.stdout,b"-nan\t-nan\t-nan\t-nan\t-nan\nnan\tnan\tnan\tnan\tnan\n0\t0\t0\t0\t0\n-1\t-1\t0\t0\t-0.5\n3\t2\t3\t2\t0.5\n-inf\t-inf\t-inf\t-inf\t0\n");
}

#[test]
fn line_numeric_numeric_type_parameter_is_invalid() {
    for op in ["getnum:1", "getnum:1.5"] {
        let o = invoke(&candidate(), &args(&[op, "1"]), b"123\n", false, false);
        assert_eq!(o.status.code(), Some(1));
        assert!(o.stdout.is_empty());
        assert_eq!(
            o.stderr,
            b"fastmash: getnum requires a character type parameter\n"
        );
    }
}

#[test]
#[ignore = "requires named GNU reference, German locale and fresh evidence directory"]
fn line_numeric_german_numeric() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for op in [
        "getnum:p", "getnum:d", "getnum:n", "getnum:i", "getnum:h", "getnum:o",
    ] {
        cases.push((args(&[op, "1"]), b"a-1,5\nx.5\nx1.5\nx+12\n".to_vec()));
    }
    for op in ["round", "floor", "ceil", "trunc", "frac"] {
        cases.push((args(&[op, "1"]), b"-1,5\n0,5\n1,25\nnan\ninf\n".to_vec()));
    }
    compare_cases_locale(&cases, &reference, &evidence, "de_DE.utf8");
}

fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    let extract=b"none\n-0\n+0\n--2x3\n.5x7\n..5x7\n1e3\n-9223372036854775808\n-9223372036854775809\n9223372036854775807\n9223372036854775808\n7fffffffffffffff\n8000000000000000\n777777777777777777777\n1000000000000000000000\n1.2.3\na\0-5\nNA\nN/A\nNaN\n-NaN\nabc-12.5x\nx0xFF\n";
    for op in [
        "getnum",
        "getnum:n",
        "getnum:i",
        "getnum:h",
        "getnum:o",
        "getnum:p",
        "getnum:d",
        "getnum:hex",
    ] {
        for flags in [
            vec![],
            vec!["--header-out"],
            vec!["-H"],
            vec!["--full"],
            vec!["-fH"],
            vec!["--narm"],
            vec!["-s"],
            vec!["--format=%.3f"],
            vec!["--round=3"],
            vec!["--format=%La"],
        ] {
            let mut a = flags;
            a.extend([op, "1"]);
            cases.push((args(&a), extract.to_vec()));
        }
    }
    for op in ["round", "floor", "ceil", "trunc", "frac"] {
        for input in [
            "",
            "-0\n",
            "0.5\n-0.5\n1.5\n2.5\n",
            "nan\ninf\n-inf\n",
            "0x1.fffffffffffffffep-2\n0x1.0000000000000002p-1\n",
            "0x1.fffffffffffffffep62\n0x1p63\n0x1p100\n",
            "1e4932\n",
            "1e-4930\n",
            "1e4933\n",
            "1e-5000\n",
            "1\ninvalid\n3\n",
            "NA\nN/A\n",
            "2.2\t3.7\n",
            "\n",
        ] {
            for flags in [
                vec![],
                vec!["--header-out"],
                vec!["-H"],
                vec!["-f"],
                vec!["--narm"],
                vec!["--round=2"],
                vec!["--format=%La"],
            ] {
                let mut a = flags;
                a.extend([op, "1"]);
                cases.push((args(&a), input.as_bytes().to_vec()));
            }
        }
    }
    for a in [
        vec!["getnum:x", "1"],
        vec!["getnum:P", "1"],
        vec!["getnum:p:i", "1"],
        vec!["getnum:", "1"],
        vec!["getnum::p", "1"],
        vec!["round:2", "1"],
        vec!["round:x", "1"],
        vec!["round:", "1"],
        vec!["round", "1", "sum", "1"],
        vec!["sum", "1", "round", "1"],
        vec!["-g1", "round", "2"],
        vec!["ct", "1,2", "round", "3"],
        vec!["round", "1:2"],
        vec!["getnum:d", "1-2", "cut", "1"],
        vec!["round", "1,2", "floor", "2", "frac", "1"],
        vec!["-H", "getnum:d", "value", "round", "number"],
        vec!["--vnlog", "getnum:d", "value", "round", "number"],
    ] {
        cases.push((args(&a), b"value\tnumber\nx-2.5\t3.5\n".to_vec()));
    }
    cases.push((args(&["-z", "getnum:d", "1"]), b"x1\ny\0x-2\n\0".to_vec()));
    cases.push((
        args(&["-t,", "round", "1", "frac", "2"]),
        b"1.2,3.5\n".to_vec(),
    ));
    cases.push((args(&["-C", "round", "1"]), b"#skip\n1.5\n".to_vec()));
    for op in ["sum", "mean", "min", "median", "round", "frac"] {
        cases.push((args(&[op, "1"]), b"-1e4933\n".to_vec()));
    }
    for op in ["getnum:p", "getnum:d"] {
        cases.push((
            args(&[op, "1"]),
            format!("{}\n0.{}1\n", "9".repeat(5000), "0".repeat(5000)).into_bytes(),
        ));
    }
    cases
}

#[test]
#[ignore = "requires named reference, retained RefGene and fresh evidence directory"]
fn line_numeric_real_data() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let input = fs::read(std::env::var_os("FASTMASH_REFGENE").unwrap()).unwrap();
    compare_cases(
        &[(
            args(&[
                "getnum:n", "2", "getnum:n", "3", "round", "11", "frac", "11",
            ]),
            input,
        )],
        &reference,
        &evidence,
    );
}

#[test]
#[ignore = "requires named reference and fresh evidence directory"]
fn line_numeric_read_failures() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    for full in [false, true] {
        let arguments = ["getnum:n", "1"];
        let expected = super::transpose::failing_input(&reference, &arguments, full);
        let actual = super::transpose::failing_input(&candidate(), &arguments, full);
        fs::write(
            evidence.join(format!("gnu-{full}.txt")),
            format!("{expected:?}"),
        )
        .unwrap();
        fs::write(
            evidence.join(format!("candidate-{full}.txt")),
            format!("{actual:?}"),
        )
        .unwrap();
        assert_eq!(actual.stdout, expected.stdout);
        assert_eq!(actual.stderr, expected.stderr);
        assert_eq!(actual.status, expected.status);
    }
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn line_numeric_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    compare_cases(&cases(), &reference, &evidence);
}

#[test]
fn line_numeric_large_fields_and_streams() {
    let input = format!("{}-123.5\n", "x".repeat(1_000_000));
    let o = invoke(
        &candidate(),
        &args(&["getnum:d", "1"]),
        input.as_bytes(),
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(o.stdout, b"-123.5\n");
    let input = b"1.5\n".repeat(100_000);
    let o = invoke(
        &candidate(),
        &args(&["round", "1", "frac", "1"]),
        &input,
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(o.stdout, b"2\t0.5\n".repeat(100_000));
    let o = invoke(&candidate(), &args(&["round", "1"]), b"1.5\n", false, true);
    assert_eq!(o.status.code(), Some(1));
    assert!(
        o.stderr
            .ends_with(b"write error: No space left on device\n")
    );
}
