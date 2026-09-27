use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn percentile_trim_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    // Never run GNU perc:100 or decimal perc: both have established unsafe paths.
    for op in [
        "perc",
        "perc:1",
        "perc:25",
        "perc:50",
        "perc:99",
        "trimmean",
        "trimmean:0",
        "trimmean:0.1",
        "trimmean:0.25",
        "trimmean:0.4999999999999999999",
        "trimmean:0.5",
    ] {
        for input in [
            b"".as_slice(),
            b"-0\n",
            b"0\n-0\n",
            b"0.1\n0.2\n0.3\n0.4\n",
            b"8.75\n1.25\n2.5\n4.5\n100\n",
            b"NaN\n2\n1\n",
            b"1e30\n-1e30\n1\n",
            b"NA\nN/A\nNaN\n",
        ] {
            for narm in [false, true] {
                let mut a = args(&[op, "1"]);
                if narm {
                    a.insert(0, "--narm".into());
                }
                cases.push((a, input.to_vec()));
            }
        }
        cases.push((
            args(&["-H", op, "value"]),
            b"value\n0.1\n0.4\n0.9\n".to_vec(),
        ));
        cases.push((
            args(&["-sg1", op, "2"]),
            b"b\t4\na\t1\nb\t8\na\t3\n".to_vec(),
        ));
    }
    for op in [
        "trimmean:1",
        "trimmean:0.500001",
        "trimmean:0.123456789",
        "trimmean:0.00000123456789",
        "trimmean:0.1e-1",
        "trimmean:1e-1",
        "trimmean:.1",
        "trimmean:-0.1",
        "trimmean:",
        "trimmean:x",
        "trimmean:0:0",
        "trimmean:1:0",
        "trimmean:0.1:0.2",
        "trimmean:0x1",
        "trimmean:0.1x",
        "perc:0",
        "perc:101",
    ] {
        cases.push((args(&["-H", op, "value"]), b"value\n1\n2\n".to_vec()));
    }
    for percent in 1..100 {
        cases.push((
            args(&[&format!("perc:{percent}"), "1"]),
            b"0.1\n0.3\n0.2\n8.9\n-1.7\n".to_vec(),
        ));
    }
    for n in [2, 3, 9, 10, 11, 20, 101] {
        let input = (0..n)
            .map(|i| format!("{i}.25\n"))
            .collect::<String>()
            .into_bytes();
        for trim in [
            "0.099999999999999999994",
            "0.1",
            "0.10000000000000000001",
            "0.49999999999999999997",
            "0.5",
        ] {
            cases.push((args(&[&format!("trimmean:{trim}"), "1"]), input.clone()));
        }
    }
    cases.push((
        args(&[
            "trimmean:0.25",
            "1",
            "perc:29",
            "1",
            "median",
            "1",
            "trimmean:0.5",
            "1",
            "q1",
            "1",
        ]),
        b"3\nNaN\n1\n2\n".to_vec(),
    ));
    for op in [
        "trimmean:1",
        "trimmean:0.5",
        "trimmean:0.1:0.2",
        "perc:0",
        "perc:99",
    ] {
        for field in ["1:2", "1-2", "missing", "0", "1,2"] {
            cases.push((args(&[op, field]), b"1\t2\n".to_vec()));
        }
    }
    for operation in ["perc", "perc:25", "trimmean", "trimmean:0.25", "count"] {
        for input in [b"".as_slice(), b"# ignored\n; ignored\n", b"key\tvalue\n"] {
            cases.push((
                args(&["-CHs", "-g", "key", operation, "value"]),
                input.to_vec(),
            ));
        }
        for flags in [
            vec!["-sg1", "--header-out"],
            vec!["-sg1", "--header-out", "-f"],
            vec!["--header-out"],
        ] {
            let mut a = args(&flags);
            a.extend(args(&[operation, "2"]));
            cases.push((a, b"a\t1\na\t3\n".to_vec()));
        }
    }
    compare_cases(&cases, &reference, &evidence);
    let locale = evidence.join("de-numeric");
    fs::create_dir(&locale).unwrap();
    let mut cases = Vec::new();
    for op in [
        "trimmean",
        "trimmean:0",
        "trimmean:0.25",
        "trimmean:0,25",
        "trimmean:1",
        "trimmean:0.1:0.2",
    ] {
        for field in ["value", "1.1", "1:2"] {
            cases.push((args(&["-H", op, field]), b"value\n1,25\n2,75\n".to_vec()));
        }
    }
    compare_cases_locale(&cases, &reference, &locale, "de_DE.utf8");
}

#[test]
fn shared_samples_scale_and_native_spill() {
    let input = b"0.25\n".repeat(100_000);
    let a = args(&[
        "perc:29",
        "1",
        "perc:100",
        "1",
        "trimmean:0.1",
        "1",
        "median",
        "1",
    ]);
    let out = invoke(&candidate(), &a, &input, false, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"0.25\t0.25\t0.25\t0.25\n");
    let mut input = b"b\t0.25\t".to_vec();
    input.extend(vec![b'x'; 100_000]);
    input.extend_from_slice(b"\na\t0.5\ty\nb\t0.75\tz\n");
    let a = args(&["-sg1", "trimmean:0.25", "2", "perc:100", "2"]);
    let out = invoke(&candidate(), &a, &input, true, false);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"a\t0.5\t0.5\nb\t0.5\t0.75\n");
    let out = invoke(&candidate(), &a, &input, true, true);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
}

#[test]
fn independent_trim_and_safe_endpoint() {
    for (op, input, expected) in [
        (
            "trimmean:0.25",
            b"100\n0.1\n0.2\n0.3\n".as_slice(),
            b"0.25\n".as_slice(),
        ),
        ("trimmean:0.5", b"0.1\n0.2\n0.3\n0.4\n", b"0.25\n"),
        ("perc:100", b"0.1\n0.9\n0.2\n", b"0.9\n"),
        ("perc:100", b"0\n-0\n", b"-0\n"),
        ("perc:100", b"-0\n0\n", b"0\n"),
        ("perc:100", b"3\nnan\n1\n", b"1\n"),
        ("perc:100", b"1\n-nan\n", b"-nan\n"),
        ("perc:100", b"-inf\n1\ninf\n", b"inf\n"),
        ("perc:100", b"", b""),
        ("trimmean:0.5", b"inf\n-inf\n", b"nan\n"),
    ] {
        let out = invoke(&candidate(), &args(&[op, "1"]), input, false, false);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, expected);
        assert!(out.stderr.is_empty());
    }
    let a = args(&[
        "--vnlog",
        "--narm",
        "-s",
        "-g",
        "key",
        "perc:100",
        "value",
        "trimmean:0.5",
        "value",
    ]);
    let out = invoke(
        &candidate(),
        &a,
        b"# key value\nb NA\na 1\na 3\nc -0\n",
        true,
        false,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        out.stdout,
        b"# GroupBy(key) perc:100(value) trimmean:0.5(value)\na 3 2\nb nan nan\nc -0 -0\n"
    );
    assert!(out.stderr.is_empty());
}
