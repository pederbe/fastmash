use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn robust_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for operation in ["mode", "antimode", "madraw", "mad"] {
        for input in [
            b"".as_slice(),
            b"-0\n",
            b"1.25\n",
            b"2\n1\n2\n1\n",
            b"2\n2\n1\n3\n3\n",
            b"0\n-0\n",
            b"-0\n0\n",
            b"1\n2\n3\n",
            b"1.1\n2.2\n4.4\n9.9\n",
            b"NA\nN/A\nNaN\n",
            b"1\nNA\n3\n",
            b"nan\n2\n1\n",
            b"-nan\n1\n",
            b"inf\n",
            b"inf\n-inf\n",
            b"0x1p-16382\n0x1p-16381\n0x1p-16380\n",
            b"1e4932\n1e4932\n",
            b"1.0000000000000000001\n1\n1\n",
            b"1e30\n-1e30\n1\n",
            b"bad\n",
            b"\n",
        ] {
            for narm in [false, true] {
                let mut a = args(&[operation, "1"]);
                if narm {
                    a.insert(0, "--narm".into());
                }
                cases.push((a, input.to_vec()));
            }
        }
        for a in [
            vec!["-H", operation, "value"],
            vec!["-Hsgkey", operation, "value"],
            vec!["-sg1", "--header-out", operation, "2"],
            vec!["-fsg1", "--header-out", operation, "2"],
        ] {
            let input = if a[0].contains('H') {
                b"key\tvalue\nb\t3\na\t1\na\t2\nb\t7\n".as_slice()
            } else {
                b"b\t3\na\t1\na\t2\nb\t7\n"
            };
            cases.push((args(&a), input.to_vec()));
        }
        for input in [b"".as_slice(), b"# comment\n", b"key\tvalue\n"] {
            cases.push((
                args(&["-CHs", "-g", "key", operation, "value"]),
                input.to_vec(),
            ));
        }
        let values = ["nan", "0", "-0", "2"];
        for a in 0..4 {
            for b in 0..4 {
                for c in 0..4 {
                    for d in 0..4 {
                        if a == b || a == c || a == d || b == c || b == d || c == d {
                            continue;
                        }
                        let input = format!(
                            "{}\n{}\n{}\n{}\n",
                            values[a], values[b], values[c], values[d]
                        );
                        cases.push((args(&[operation, "1"]), input.into_bytes()));
                    }
                }
            }
        }
    }
    for ops in [
        vec!["madraw", "1", "mad", "1", "mode", "1", "median", "1"],
        vec![
            "median",
            "1",
            "mode",
            "1",
            "mad",
            "1",
            "madraw",
            "1",
            "trimmean:0.25",
            "1",
        ],
    ] {
        for input in [b"3\nnan\n1\n2\n".as_slice(), b"2\n2\n1\n1\n4\n", b"-0\n0\n"] {
            cases.push((args(&ops), input.to_vec()));
        }
    }
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn robust_independent_large_samples_and_scale() {
    let mut input = b"0\n".repeat(50_000);
    input.extend(b"2\n".repeat(50_000));
    let out = invoke(
        &candidate(),
        &args(&[
            "mode", "1", "antimode", "1", "madraw", "1", "mad", "1", "median", "1",
        ]),
        &input,
        false,
        false,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"0\t0\t1\t1.4826\t1\n");
    let out = invoke(
        &candidate(),
        &args(&["--format=%.20g", "mad", "1"]),
        b"0\n1\n2\n",
        false,
        false,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"1.4825999999999999179\n");
}

#[test]
fn robust_group_reset_named_aliases_spill_and_output_failure() {
    let mut input = b"key\tvalue\tother\nb\tNA\t".to_vec();
    input.extend(vec![b'x'; 100_000]);
    input.extend_from_slice(b"\na\t1\tx\na\t3\tx\nc\t-0\tx\na\t5\tx\n");
    let a = args(&[
        "-Hs", "--narm", "-g", "key", "mad", "value", "mode", "2", "madraw", "2", "median",
        "value", "mad", "2",
    ]);
    let out = invoke(&candidate(), &a, &input, true, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"GroupBy(key)\tmad(value)\tmode(value)\tmadraw(value)\tmedian(value)\tmad(value)\na\t2.9652\t1\t2\t3\t2.9652\nb\tnan\tnan\tnan\tnan\tnan\nc\t0\t-0\t0\t-0\t0\n");
    let out = invoke(&candidate(), &a, &input, true, true);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
}
