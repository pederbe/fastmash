use super::*;

fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    for op in ["geomean", "harmmean", "ms", "rms"] {
        for input in [
            "",
            "1\n",
            "-1\n",
            "0\n",
            "-0\n",
            "0\n-0\n",
            "1\n2\n4\n",
            "0.1\n0.2\n0.3\n",
            "-1\n2\n",
            "0\n2\n",
            "inf\n",
            "-inf\n",
            "nan\n",
            "-nan\n",
            "0x1p-16445\n",
            "-0x1p-16445\n",
            "0x1p-16382\n",
            "0xf.fffffffffffffffp+16380\n",
            "1e3000\n1e-3000\n",
            "NA\n",
            "NA\n2\nN/A\n",
            "bad\n",
            "1e4933\n",
        ] {
            // Invalid reciprocal cancellation uses the documented positive NaN;
            // its explicit policy test below is separate from GNU equality.
            if op == "harmmean" && input == "0\n-0\n" {
                continue;
            }
            for narm in [false, true] {
                let mut a = args(&[op, "1"]);
                if narm {
                    a.insert(0, "--narm".into());
                }
                if op != "geomean" {
                    let mut exact = a.clone();
                    exact.insert(0, "--format=%a".into());
                    cases.push((exact, input.as_bytes().to_vec()));
                }
                cases.push((a, input.as_bytes().to_vec()));
            }
        }
        for sorted in [false, true] {
            let mut a = args(&["-H", "--narm", "-g", "key", op, "value"]);
            if sorted {
                a.insert(0, "-s".into());
            }
            cases.push((
                a,
                b"key\tvalue\na\t1\na\t4\nb\tNA\nc\t0x1p-16445\n".to_vec(),
            ));
        }
        cases.push((args(&["-z", op, "1"]), b"1\x002\x004\x00".to_vec()));
        cases.push((args(&["-g", "1", op, "2"]), b"a\t2\nb\tbad\n".to_vec()));
        cases.push((args(&[op, "1", op, "2"]), b"1\t2\n3\n".to_vec()));
    }
    for a in [
        args(&[
            "geomean", "1", "harmmean", "1", "ms", "1", "rms", "1", "pstdev", "1",
        ]),
        args(&["rms", "1", "pstdev", "1"]),
    ] {
        cases.push((a, b"1\n2\n4\n".to_vec()));
    }
    for operations in [
        vec!["geomean", "2"],
        vec!["rms", "2", "geomean", "2", "harmmean", "2"],
        vec!["pstdev", "2", "geomean", "2"],
    ] {
        let mut a = args(&["--narm", "-g", "1"]);
        a.extend(args(&operations));
        let mut input = b"a\t0x1p-16445\na\t0x2p-16445\nb\t0xf.fffffffffffffffp16380\nc\t1\nc\t4\nd\tNA\ne\t-0\n".to_vec();
        // pstdev(inf) creates a documented positive NaN, unlike GNU's negative
        // NaN. Keep this equality check on the shared finite-value contract.
        if operations[0] != "pstdev" {
            input.extend_from_slice(b"f\tinf\n");
        }
        cases.push((a, input));
    }
    cases
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn alternative_means_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    compare_cases(&cases(), &reference, &evidence);
}

#[test]
fn documented_invalid_nan_and_output_failure() {
    let binary = candidate();
    // Fastmash deliberately creates positive NaN for invalid arithmetic.
    let output = invoke(&binary, &args(&["geomean", "1"]), b"0\ninf\n", false, false);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"nan\n");
    let output = invoke(&binary, &args(&["harmmean", "1"]), b"0\n-0\n", false, false);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"nan\n");
    for op in ["geomean", "harmmean", "ms", "rms"] {
        let output = invoke(&binary, &args(&[op, "1"]), b"1\n4\n", false, true);
        assert!(!output.status.success());
        assert!(
            output
                .stderr
                .windows(b"write error".len())
                .any(|w| w == b"write error")
        );
    }
}

#[test]
#[ignore = "requires named GNU locale and fresh evidence directory"]
fn alternative_means_german_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap())
        .with_extension("german");
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for op in ["geomean", "harmmean", "ms", "rms"] {
        for input in ["1,25\n2,5\n", "-0\n", "0x1p-16445\n", "bad\n"] {
            cases.push((args(&[op, "1"]), input.as_bytes().to_vec()));
        }
        cases.push((
            args(&["-s", "-g", "1", op, "2"]),
            b"b\t1,25\na\t2,5\na\t5\n".to_vec(),
        ));
    }
    compare_cases_settings(&cases, &reference, &evidence, "de_DE.UTF-8", true);
}
