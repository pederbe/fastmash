use super::*;

fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    for input in [
        "NA\t1\n2\tNA\n4\t5\n",
        "NA\t1\n2\t3\n",
        "1\tNA\n",
        "bad\t2\n",
        "1\tbad\n",
    ] {
        cases.push((
            args(&[
                "--narm", "pcov", "1:2", "sum", "1", "dotprod", "1:2", "scov", "2:1",
            ]),
            input.as_bytes().to_vec(),
        ));
    }
    cases.push((
        args(&[
            "-H", "--narm", "-g", "key", "pcov", "x:y", "dotprod", "x:y", "scov", "x:y",
        ]),
        b"key\tx\ty\na\tNA\t1\na\t2\tNA\na\t4\t5\nb\t3\t7\n".to_vec(),
    ));
    for op in ["pcov", "scov", "ppearson", "spearson", "dotprod"] {
        for input in [
            "",
            "1\t2\n",
            "0\t0\n2\t4\n",
            "1\t4\n2\t2\n3\t0\n",
            "0.1\t1.25\n0.2\t2.5\n0.3\t4.75\n",
            "9007199254740992\t1\n9007199254740993\t2\n9007199254740994\t3\n",
            "1e1000\t1e-1000\n-1e1000\t-1e-1000\n",
            "0x1p-16445\t1\n0x1p-16445\t2\n",
            "NA\t1\n2\tNA\n3\t4\n",
            "NA\t1\n2\t3\n",
            "1\tNA\n",
            "NA\t1\n",
            "NA\tNA\n",
            "nan(2)\t-nan(3)\n",
            "bad\t2\n",
            "1\tbad\n",
            "1\n",
            "1e4933\t1\n",
        ] {
            // Approved positive generated NaN is checked separately below.
            if (op == "ppearson" && input == "1\t2\n")
                || (["ppearson", "spearson"].contains(&op)
                    && input == "0x1p-16445\t1\n0x1p-16445\t2\n")
            {
                continue;
            }
            for narm in [false, true] {
                for hex in [false, true] {
                    let mut a = args(&[op, "1:2"]);
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
        for selector in ["1", "1:", ":2", "1::2", "0:2", "2:0", "1:2,2:1"] {
            cases.push((args(&[op, selector]), b"1\t3\n2\t4\n".to_vec()));
        }
        for sorted in [false, true] {
            let mut a = args(&["-H", "--narm", "-g", "key", op, "x:y", op, "y:x"]);
            if sorted {
                a.insert(0, "-s".into());
            }
            cases.push((
                a,
                b"key\tx\ty\na\tNA\t1\na\t2\tNA\na\t3\t4\nb\t0\t0\nb\t2\t4\n".to_vec(),
            ));
        }
        cases.push((args(&["-z", op, "1:2"]), b"1\t2\x003\t4\x00".to_vec()));
        cases.push((
            args(&["-g", "1", "count", "2", op, "2:3"]),
            b"a\t1\t2\na\t3\t4\nb\t1\tbad\n".to_vec(),
        ));
    }
    for input in [
        b"0x1p64\t1\n1\t1\n-0x1p64\t1\n".as_slice(),
        b"0x1p64\t1\n-0x1p64\t1\n1\t1\n",
        b"1e3000\t1e3000\n",
        b"1e-3000\t1e-3000\n",
    ] {
        cases.push((args(&["--format=%a", "dotprod", "1:2"]), input.to_vec()));
    }
    for input in [
        b"0x1p8000\t0x1p8000\n-0x1p8000\t-0x1p8000\n".as_slice(),
        b"0x1p-8200\t0x1p-8200\n-0x1p-8200\t-0x1p-8200\n",
    ] {
        cases.push((args(&["--format=%a", "ppearson", "1:2"]), input.to_vec()));
    }
    cases
}

#[test]
#[ignore = "requires named reference and fresh evidence directory"]
fn paired_jobs_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    compare_cases(&cases(), &reference, &evidence);
}

#[test]
fn paired_large_independent_results_and_write_failure() {
    let mut input = Vec::new();
    for _ in 0..50_000 {
        input.extend_from_slice(b"0\t0\n2\t4\n");
    }
    let a = args(&["pcov", "1:2", "dotprod", "1:2"]);
    let result = invoke(&candidate(), &a, &input, false, false);
    assert!(result.status.success(), "{result:?}");
    assert_eq!(result.stdout, b"2\t400000\n");
    for op in ["ppearson", "spearson"] {
        let output = invoke(
            &candidate(),
            &args(&[op, "1:2"]),
            b"1\t2\n1\t3\n",
            false,
            false,
        );
        assert!(output.status.success());
        assert_eq!(output.stdout, b"nan\n");
    }
    for (a, input, expected) in [
        (
            args(&[
                "pcov", "1:2", "scov", "1:2", "ppearson", "1:2", "spearson", "1:2", "dotprod",
                "1:2",
            ]),
            b"0\t0\n2\t4\n".as_slice(),
            b"2\t4\t1\t1\t8\n".as_slice(),
        ),
        (
            args(&["--narm", "dotprod", "1:2"]),
            b"1\tNA\nNA\t10\n2\t20\n",
            b"50\n",
        ),
        (
            args(&["dotprod", "1:2"]),
            b"0x1p64\t1\n1\t1\n-0x1p64\t1\n",
            b"0\n",
        ),
        (
            args(&["dotprod", "1:2"]),
            b"0x1p64\t1\n-0x1p64\t1\n1\t1\n",
            b"1\n",
        ),
        (
            args(&["ppearson", "1:2"]),
            b"0x1p8000\t0x1p8000\n-0x1p8000\t-0x1p8000\n",
            b"1\n",
        ),
        (
            args(&["ppearson", "1:2"]),
            b"0x1p-8200\t0x1p-8200\n-0x1p-8200\t-0x1p-8200\n",
            b"1\n",
        ),
    ] {
        let result = invoke(&candidate(), &a, input, false, false);
        assert!(result.status.success(), "{result:?}");
        assert_eq!(result.stdout, expected);
    }
    for op in ["pcov", "scov", "ppearson", "spearson", "dotprod"] {
        let output = invoke(
            &candidate(),
            &args(&[op, "1:2"]),
            b"0\t0\n2\t4\n",
            false,
            true,
        );
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
#[ignore = "requires named reference and fresh evidence directory"]
fn paired_large_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap())
        .with_extension("large");
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for n in [65_535, 65_536, 65_537, 100_000] {
        let mut input = Vec::new();
        for i in 0..n {
            input.extend_from_slice(if i % 2 == 0 { b"0\t0\n" } else { b"2\t4\n" });
        }
        cases.push((
            args(&[
                "pcov", "1:2", "scov", "1:2", "ppearson", "1:2", "spearson", "1:2", "dotprod",
                "1:2",
            ]),
            input,
        ));
    }
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn paired_large_filter_skew_reset_and_mixed_sqrt_owner() {
    let mut skew = b"1\tNA\n".repeat(70_000);
    skew.extend_from_slice(&b"NA\t2\n".repeat(70_000));
    let output = invoke(
        &candidate(),
        &args(&["--narm", "dotprod", "1:2"]),
        &skew,
        false,
        false,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"140000\n");
    let mut groups = b"a\t0\t0\na\t2\t4\n".repeat(35_000);
    groups.extend_from_slice(b"b\t1\t2\na\t3\t4\n");
    let output = invoke(
        &candidate(),
        &args(&["-g", "1", "pcov", "2:3", "dotprod", "2:3"]),
        &groups,
        false,
        false,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a\t2\t280000\nb\t0\t2\na\t0\t12\n");
    for input in [
        b"1\t2\t0x1p-16445\n3\t4\t0x1p-16445\n".as_slice(),
        b"1\t2\t1\n3\t4\t1\n",
    ] {
        let base = invoke(
            &candidate(),
            &args(&["ppearson", "1:2"]),
            input,
            false,
            false,
        );
        for a in [
            args(&["geomean", "3", "ppearson", "1:2"]),
            args(&["pskew", "1", "ppearson", "1:2"]),
        ] {
            let mixed = invoke(&candidate(), &a, input, false, false);
            assert!(mixed.status.success());
            assert!(mixed.stdout.ends_with(&base.stdout));
        }
    }
}

#[test]
#[ignore = "requires named GNU locale and fresh evidence directory"]
fn paired_german_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap())
        .with_extension("german");
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for op in ["pcov", "scov", "ppearson", "spearson", "dotprod"] {
        cases.push((args(&[op, "1:2"]), b"1,25\t2,5\n2,5\t3,75\n".to_vec()));
        cases.push((
            args(&["--narm", op, "1:2"]),
            b"NA\t1,25\n2,5\tNA\n3,75\t5\n".to_vec(),
        ));
        cases.push((
            args(&["-s", "-g", "1", op, "2:3"]),
            b"b\t1,25\t2,5\nb\t2,5\t5\na\t1,25\t2,5\na\t2,5\t5\n".to_vec(),
        ));
    }
    compare_cases_settings(&cases, &reference, &evidence, "de_DE.UTF-8", true);
}
