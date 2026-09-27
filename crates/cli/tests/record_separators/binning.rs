use super::*;
fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    let numeric=b"-inf\ninf\nnan\n-0\n0\n-1000.25\n-100\n-10.1\n-0.1\n0.1\n9.999\n10\n100\n0x1.fffffffffffffffep-1\n0x1.0000000000000002p0\n-0x1.fffffffffffffffep-1\n-0x1.0000000000000002p0\n1e4932\n-1e4932\n1e-4930\n-1e-4930\n";
    let text = b"abc\nABC\na\0b\n\xff\n\t\nNA\nN/A\nNaN\n";
    for op in [
        "bin",
        "bin:0",
        "bin:1",
        "bin:10",
        "bin:0.1",
        "bin:0.25",
        "bin:9223372036854775807",
        "bin:1.0e4932",
        "bin:1.0e-4930",
        "strbin",
        "strbin:1",
        "strbin:3",
        "strbin:512",
        "strbin:9223372036854775807",
    ] {
        for flags in [
            vec![],
            vec!["-H"],
            vec!["--header-out"],
            vec!["--full"],
            vec!["-fH"],
            vec!["--narm"],
            vec!["--round=3"],
            vec!["--format=%La"],
            vec!["-i"],
            vec!["-s"],
        ] {
            let mut a = flags;
            a.extend([op, "1"]);
            cases.push((
                args(&a),
                if op.starts_with("strbin") {
                    text.to_vec()
                } else {
                    numeric.to_vec()
                },
            ));
        }
    }
    for op in [
        "bin:-1",
        "bin:+1",
        "bin:.5",
        "bin:1e2",
        "bin:0:1",
        "bin:",
        "bin:foo",
        "bin:1:2",
        "bin:18446744073709551615",
        "bin:1.0e4933",
        "strbin:0",
        "strbin:-1",
        "strbin:",
        "strbin:foo",
        "strbin:1:2",
        "strbin:0:0",
        "strbin:18446744073709551615",
    ] {
        cases.push((args(&[op, "1"]), b"3\n".to_vec()));
    }
    for a in [
        vec!["bin", "1:2"],
        vec!["strbin", "1:2"],
        vec!["bin", "1", "sum", "1"],
        vec!["sum", "1", "strbin", "1"],
        vec!["-g1", "bin", "2"],
        vec!["ct", "1,2", "strbin", "1"],
        vec!["bin:3", "2", "strbin:7", "1", "cut", "1"],
        vec!["-H", "strbin", "label", "bin:0.25", "value"],
        vec!["--vnlog", "strbin", "label", "bin:0.25", "value"],
    ] {
        cases.push((args(&a), b"label\tvalue\na\t3.5\nb\t-2.5\n".to_vec()));
    }
    cases.push((args(&["-z", "strbin", "1"]), b"a\nb\0\xff\0".to_vec()));
    for op in ["bin:1.0e4933", "trimmean:1.0e4933", "perc:1.0e4933"] {
        cases.push((args(&[op, "1"]), b"2\n".to_vec()));
    }
    let mut bytes = (0..4096)
        .map(|i| ((i * 37) % 256) as u8)
        .filter(|b| !matches!(b, b'\n' | b'\t'))
        .collect::<Vec<_>>();
    bytes.push(b'\n');
    for buckets in ["3", "10", "512", "9223372036854775807"] {
        let op = format!("strbin:{buckets}");
        cases.push((args(&[&op, "1", &op, "1"]), bytes.repeat(3)));
    }
    cases
}

#[test]
fn binning_signed_nan_input_and_zero_width() {
    let o = invoke(
        &candidate(),
        &args(&["bin", "1", "bin:0", "1"]),
        b"-nan\nnan\n-0\n0\n1\n-inf\n",
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(
        o.stdout,
        b"-nan\t-nan\nnan\tnan\n0\t0\n0\t0\n0\t-nan\n-inf\t-nan\n"
    );
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn binning_read_failures() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    for full in [false, true] {
        let expected = super::transpose::failing_input(&reference, &["strbin", "1"], full);
        let actual = super::transpose::failing_input(&candidate(), &["strbin", "1"], full);
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
fn binning_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    compare_cases(&cases(), &reference, &evidence);
}
#[test]
#[ignore = "requires named GNU reference, locale and fresh evidence directory"]
fn binning_german() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let cases = vec![
        (args(&["bin:10", "1"]), b"-10,5\n0,5\n100,2\n".to_vec()),
        (args(&["strbin:11", "1"]), b"-10,5\n0,5\n100,2\n".to_vec()),
        (args(&["bin:0.5", "1"]), b"1,5\n".to_vec()),
    ];
    compare_cases_locale(&cases, &reference, &evidence, "de_DE.utf8");
}
#[test]
#[ignore = "requires named GNU reference, RefGene and fresh evidence directory"]
fn binning_real_data() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let input = fs::read(std::env::var_os("FASTMASH_REFGENE").unwrap()).unwrap();
    compare_cases(
        &[(
            args(&["strbin:16", "2", "bin:1000", "5", "cut", "2"]),
            input,
        )],
        &reference,
        &evidence,
    );
}
#[test]
fn binning_wide_streaming_and_defined_refusal() {
    let input = format!("{}\n", "a".repeat(1_000_000));
    let o = invoke(
        &candidate(),
        &args(&["strbin:1", "1"]),
        input.as_bytes(),
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(o.stdout, b"0\n");
    let o = invoke(
        &candidate(),
        &args(&["bin:10", "1", "strbin:1", "1"]),
        &b"-1\n".repeat(100_000),
        false,
        false,
    );
    assert!(o.status.success(), "{o:?}");
    assert_eq!(o.stdout, b"-10\t0\n".repeat(100_000));
    let o = invoke(
        &candidate(),
        &args(&["strbin:2.5", "1"]),
        b"a\n",
        false,
        false,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
}
