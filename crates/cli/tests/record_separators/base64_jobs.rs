use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn base64_jobs_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for op in ["base64", "debase64"] {
        for value in [
            b"".as_slice(),
            b"f",
            b"foo",
            b"foobar",
            b"Zg==",
            b"Zm8=",
            b"Zm9v",
            b"Zg",
            b"Zg=",
            b"Zg===",
            b"Zh==",
            b"Zm9=",
            b"=AAA",
            b"A===",
            b"Zg==AAAA",
            b"Z g==",
            b"Zg==\r",
            b"YQBi",
            b"AA==",
            b"/w==",
            b"a\x00b",
            b"Zg==\x00",
            b"\xff",
            b"NA",
            b"NaN",
            b"N/A",
        ] {
            for flags in [
                vec![],
                vec!["--narm"],
                vec!["--header-out"],
                vec!["-f"],
                vec!["-t,", "--output-delimiter=|"],
            ] {
                let mut a = args(&flags);
                a.extend(args(&[op, "1"]));
                let mut input = value.to_vec();
                input.push(b'\n');
                cases.push((a, input));
            }
        }
        for a in [
            vec!["-H", op, "value"],
            vec!["-H", op, "2,1"],
            vec!["-Hf", op, "1"],
            vec!["-z", op, "1"],
            vec!["-s", op, "1"],
            vec!["-g1", op, "2"],
            vec![op, "1:2"],
            vec![op, "1", "count", "1"],
        ] {
            cases.push((args(&a), b"value\tother\nZg==\tZm8=\n!\tZm9v\n".to_vec()));
        }
        for parameter in ["1", "x", "", "1:2"] {
            cases.push((
                args(&[&format!("{op}:{parameter}"), "1"]),
                b"Zg==\n".to_vec(),
            ));
        }
    }
    for byte in 0..=255u8 {
        let mut a = args(&["-t|"]);
        if byte == b'|' {
            a = args(&["-t;"]);
        }
        if byte == b'\n' {
            a.push("-z".into());
        }
        a.extend(args(&["base64", "1"]));
        cases.push((a, vec![byte, if byte == b'\n' { 0 } else { b'\n' }]));
    }
    for input in [
        b"Zg==\n!\n".as_slice(),
        b"Zg==\t!\n",
        b"YQBi\tYg==\n",
        b"\tZg==\n",
    ] {
        cases.push((args(&["debase64", "1", "debase64", "2"]), input.to_vec()));
    }
    cases.push((args(&["-z", "debase64", "1"]), b"Zg==\n\0".to_vec()));
    for input in [b"f\tZg==\nx\t!\n".as_slice(), b"f\t!\n", b"f\n"] {
        cases.push((args(&["base64", "1", "debase64", "2"]), input.to_vec()));
    }
    for op in ["base64", "debase64"] {
        cases.push((args(&["--narm", op, "1"]), b"NA\nNaN\nN/A\n".to_vec()));
        for full in [false, true] {
            let expected = super::transpose::failing_input(&reference, &[op, "1"], full);
            let actual = super::transpose::failing_input(&candidate(), &[op, "1"], full);
            fs::write(
                evidence.join(format!("{op}-pty-{full}.gnu.txt")),
                format!("{expected:?}"),
            )
            .unwrap();
            fs::write(
                evidence.join(format!("{op}-pty-{full}.candidate.txt")),
                format!("{actual:?}"),
            )
            .unwrap();
            assert_eq!(actual.status.code(), expected.status.code());
            assert_eq!(actual.stdout, expected.stdout);
            assert_eq!(actual.stderr, expected.stderr);
        }
    }
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn base64_jobs_wide_fields_and_many_rows() {
    let plain = b"foo".repeat(350_000);
    let encoded = b"Zm9v".repeat(350_000);
    for (op, input, expected) in [("base64", &plain, &encoded), ("debase64", &encoded, &plain)] {
        let mut input = input.clone();
        input.extend_from_slice(b"\nf\n");
        // The second row also verifies buffer reuse after a wide first row.
        if op == "debase64" {
            input.truncate(input.len() - 2);
            input.extend_from_slice(b"Zg==\n");
        }
        let out = invoke(&candidate(), &args(&[op, "1"]), &input, false, false);
        assert!(out.status.success(), "{out:?}");
        let mut expected = expected.clone();
        expected.extend_from_slice(if op == "base64" {
            b"\nZg==\n"
        } else {
            b"\nf\n"
        });
        assert_eq!(out.stdout, expected);
        assert!(out.stderr.is_empty());
    }
    let out = invoke(
        &candidate(),
        &args(&["base64", "1"]),
        &b"foo\n".repeat(100_000),
        false,
        false,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"Zm9v\n".repeat(100_000));
}

#[test]
fn base64_jobs_independent_vectors_and_failures() {
    // RFC4648 section10, independent of the candidate encoder/decoder.
    for (plain, encoded) in [
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        for (op, input, expected) in [("base64", plain, encoded), ("debase64", encoded, plain)] {
            let out = invoke(
                &candidate(),
                &args(&[op, "1"]),
                format!("{input}\n").as_bytes(),
                false,
                false,
            );
            assert!(out.status.success(), "{out:?}");
            assert_eq!(out.stdout, format!("{expected}\n").as_bytes());
            assert!(out.stderr.is_empty());
        }
    }
    let out = invoke(
        &candidate(),
        &args(&["debase64", "1"]),
        b"YQBi\n",
        false,
        false,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a\n");
    for (op, input) in [
        ("base64", b"f\n".as_slice()),
        ("debase64", b"Zg==\n".as_slice()),
    ] {
        let out = invoke(&candidate(), &args(&[op, "1"]), input, false, true);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("write error"));
    }
}
