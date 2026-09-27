use super::*;
fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    for op in ["jarque", "dpo"] {
        for input in [
            "",
            "1\n",
            "1\n2\n",
            "1\n2\n3\n",
            "1\n2\n3\n4\n",
            "0\n0\n0\n4\n",
            "0.1\n0.2\n0.4\n1.5\n",
            "1\n1\n1\n1\n",
            "-0\n0\n-0\n0\n",
            "1e3000\n2e3000\n3e3000\n4e3000\n",
            "1e-3000\n2e-3000\n3e-3000\n4e-3000\n",
            "nan\n1\n2\n3\n",
            "-nan(2)\nnan(3)\n2\n3\n",
            "inf\n1\n2\n3\n",
            "-inf\ninf\n2\n3\n",
            "NA\n1\n2\n3\n4\n",
            "NA\nNA\n",
            "bad\n",
            "1e4933\n",
            "9007199254740992\n9007199254740993\n9007199254740994\n9007199254740996\n",
        ] {
            for narm in [false, true] {
                for hex in [false, true] {
                    let mut a = args(&[op, "1"]);
                    if narm {
                        a.insert(0, "--narm".into());
                    }
                    if hex {
                        a.insert(0, "--format=%a".into());
                    }
                    cases.push((a, input.as_bytes().to_vec()));
                }
            }
        }
        for n in [8, 20, 78, 79, 83, 84, 89, 90, 94, 95, 1000] {
            let mut input = b"0\n".repeat(n - 1);
            input.extend_from_slice(b"1\n");
            cases.push((args(&[op, "1"]), input));
        }
    }
    cases
}
#[test]
#[ignore = "requires named reference and fresh evidence"]
fn normality_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let all = cases();
    let known_hex_difference = |(a, input): &&(Vec<OsString>, Vec<u8>)| {
        (input == b"1\n2\n"
            || input == b"1\n2\n3\n"
            || (input == b"nan\n1\n2\n3\n" && a.contains(&OsString::from("--narm"))))
            && a.contains(&OsString::from("jarque"))
            && a.contains(&OsString::from("--format=%a"))
    };
    let exact: Vec<_> = all
        .iter()
        .filter(|c| !known_hex_difference(c))
        .cloned()
        .collect();
    compare_cases(&exact, &reference, &evidence);
    for (a, input) in all.iter().filter(known_hex_difference) {
        let expected = invoke(&reference, a, input, false, false);
        let actual = invoke(&candidate(), a, input, false, false);
        assert!(expected.status.success() && actual.status.success());
        if input == b"1\n2\n" {
            assert_eq!(expected.stdout, b"0xd.8b306bd111d840cp-4\n");
            assert_eq!(actual.stdout, b"0xd.8b306bd111d840bp-4\n");
        } else {
            // WSL and native GNU exp differ in this final bit. Fastmash does not.
            assert!(
                [
                    b"0xd.e6aa9dcebdb100bp-4\n".as_slice(),
                    b"0xd.e6aa9dcebdb100cp-4\n"
                ]
                .contains(&expected.stdout.as_slice())
            );
            assert_eq!(actual.stdout, b"0xd.e6aa9dcebdb100bp-4\n");
        }
        assert!(expected.stderr.is_empty() && actual.stderr.is_empty());
    }
}

#[test]
#[ignore = "requires named reference and fresh evidence"]
fn normality_large_shared_and_locale_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let base = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    let evidence = base.with_extension("large");
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for n in [65535, 65536, 65537, 100000] {
        let input = (0..n)
            .map(|i| format!("{}\n", 1 + i % 7))
            .collect::<String>()
            .into_bytes();
        cases.push((
            args(&["jarque", "1", "dpo", "1", "pskew", "1", "pkurt", "1"]),
            input,
        ));
    }
    for a in [
        args(&["-H", "--narm", "jarque", "x", "dpo", "1", "pskew", "x"]),
        args(&["-H", "--narm", "pskew", "x", "dpo", "1", "jarque", "x"]),
    ] {
        cases.push((a, b"x\nNA\n-1\n1\n-1\n1\n".to_vec()));
    }
    for op in ["jarque", "dpo"] {
        cases.push((args(&["-z", op, "1"]), b"-1\x001\x00-1\x001\x00".to_vec()));
        cases.push((
            args(&["-s", "-g", "1", op, "2", op, "2"]),
            b"b\t1\na\t-1\na\t1\na\t-1\na\t1\nb\t2\nb\t3\nb\t4\n".to_vec(),
        ));
        cases.push((
            args(&["-g", "1", op, "2"]),
            b"a\t-1\na\t1\na\t-1\na\t1\nb\tbad\n".to_vec(),
        ));
    }
    compare_cases(&cases, &reference, &evidence);
    let evidence = base.with_extension("german");
    fs::create_dir(&evidence).unwrap();
    compare_cases_settings(
        &[
            (
                args(&["jarque", "1", "dpo", "1"]),
                b"-1,5\n1,5\n-1,5\n1,5\n".to_vec(),
            ),
            (
                args(&["-s", "-g", "1", "jarque", "2", "dpo", "2"]),
                b"a\t-1,5\na\t1,5\na\t-1,5\na\t1,5\n".to_vec(),
            ),
        ],
        &reference,
        &evidence,
        "de_DE.UTF-8",
        true,
    );
}
#[test]
fn normality_independent_probabilities_growth_reset_and_failures() {
    // On [-1,1,-1,1], JB=2/3 and GNU's DP=21/4 exactly.
    // Independent Taylor series checks the final ordinary probabilities.
    fn exp_series(x: f64) -> f64 {
        let mut sum = 1.0;
        let mut term = 1.0;
        for n in 1..80 {
            term *= x / n as f64;
            sum += term;
        }
        sum
    }
    // Asymmetric [0,0,0,4] has JB=26/27 and GNU DP=56/9.
    for (input, exponents) in [
        (b"-1\n1\n-1\n1\n".as_slice(), [-1.0 / 3.0, -21.0 / 8.0]),
        (b"0\n0\n0\n4\n".as_slice(), [-13.0 / 27.0, -28.0 / 9.0]),
    ] {
        let output = invoke(
            &candidate(),
            &args(&["jarque", "1", "dpo", "1"]),
            input,
            false,
            false,
        );
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let values: Vec<f64> = std::str::from_utf8(&output.stdout)
            .unwrap()
            .split_whitespace()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(values.len(), 2);
        for (actual, exponent) in values.iter().zip(exponents) {
            assert!((actual - exp_series(exponent)).abs() < 1e-13);
        }
    }
    let mut input = b"a\t0\n".repeat(99999);
    input.extend_from_slice(b"a\t1\nb\t1\n");
    let output = invoke(
        &candidate(),
        &args(&["-g", "1", "jarque", "2", "dpo", "2"]),
        &input,
        false,
        false,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a\t0\t0\nb\tnan\tnan\n");
    for op in ["jarque", "dpo"] {
        let output = invoke(
            &candidate(),
            &args(&[op, "1"]),
            b"-1\n1\n-1\n1\n",
            false,
            true,
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("write error"));
    }
    let output = invoke(
        &candidate(),
        &args(&["geomean", "2", "jarque", "1", "dpo", "1"]),
        b"-1\t0x1p-16445\n1\t0x1p-16445\n-1\t0x1p-16445\n1\t0x1p-16445\n",
        false,
        false,
    );
    assert!(output.status.success());
    assert!(
        output
            .stdout
            .ends_with(b"\t0.71653131057379\t0.072439757034251\n")
    );
}
