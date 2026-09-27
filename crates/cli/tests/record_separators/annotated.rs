use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn annotated_reference_comparison() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for options in [vec![], vec!["--narm"], vec!["-C"], vec!["-C", "--narm"]] {
        for operation in ["count", "collapse", "sum", "mean"] {
            for input in [
                b"".as_slice(),
                b"NA\nN/A\nnAn\n",
                b"1\nNA\n2\n",
                b"-\n",
                b"\n",
                b" NA\n",
                b"# note\n1\n; note\n2\n",
                b"1#note\n",
                b"NA\0tail\n",
                b"nanosecond\n",
            ] {
                let mut a = args(&options);
                a.extend(args(&[operation, "1"]));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for flags in [
        vec![],
        vec!["-g", "key"],
        vec!["-s", "-g", "key"],
        vec!["-f"],
        vec!["-fs", "-g", "key"],
    ] {
        for operation in ["count", "collapse", "sum", "mean", "median", "geomean"] {
            for input in [
                b"# key value\nb 2\na 1\na 3\n".as_slice(),
                b"## note\n#! interpreter\n \n# \n# key value  \nb 2 #note\na 1#suffix\na 3\n#end\n",
                b"# key value\na NA\na N/A\nb NaN\n",
                b"# key value\na -\n",
            ] {
                let mut a = args(&["--vnlog", "--narm"]);
                a.extend(args(&flags));
                a.extend(args(&[operation, "value"]));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for input in [
        b"".as_slice(),
        b"\n \t\n##only\n#!foo\n# \n",
        b"1 2\n",
        b"; note\n# value\n1\n",
        b"# value\n\n  \n#comment\n1\n",
        b"# value\n;note\n",
        b"# value\n\0ignored\n1\n",
        b"\0ignored\n# value\n1\n",
        b"# value\n1\0tail #note\n",
        b"# value #annotation\n1 2\n",
        b"#value\n1\n",
    ] {
        cases.push((args(&["--vnlog", "count", "value"]), input.to_vec()));
    }
    for field in [
        "1", "01", "0", "-1", "+1", "1.0", "1-2", "missing", "value", "\\1",
    ] {
        for grouped in [false, true] {
            let mut a = args(&["--vnlog"]);
            if grouped {
                a.extend(args(&["-g", field]));
            }
            a.extend(args(&["count", field]));
            cases.push((a, b"# 1 01 0 value\na b c d\n".to_vec()));
        }
    }
    for option in [
        vec!["-W"],
        vec!["-t", ","],
        vec!["--output-delimiter", " "],
        vec!["--output-delimiter", ","],
        vec!["-F", "X"],
        vec!["-F", "-"],
        vec!["-z"],
        vec!["-H"],
        vec!["--no-strict"],
    ] {
        for before in [false, true] {
            let mut a = if before {
                args(&option)
            } else {
                args(&["--vnlog"])
            };
            a.extend(if before {
                args(&["--vnlog"])
            } else {
                args(&option)
            });
            a.extend(args(&["count", "value"]));
            cases.push((a, b"# value\n1\n".to_vec()));
        }
    }
    for input in [
        b"# key value\na#z 1\na 2\na#b 3\n".as_slice(),
        b"# key value\na 2\na 1#tail\na 3\n",
        b"# key value\na 1\n# comment\n\nb 2\n",
        b"# key value\na 1\nb\n",
    ] {
        for operation in ["count", "geomean"] {
            cases.push((
                args(&["--vnlog", "-s", "-g", "key", operation, "value"]),
                input.to_vec(),
            ));
        }
    }
    for input in [
        b"# \0tail\n# value\n1\n".as_slice(),
        b"# value\nleft\0right\n",
        b"# value\nNA\nNaN\n-\n",
        b"# value\n\t;literal\n",
    ] {
        cases.push((args(&["--vnlog", "collapse", "value"]), input.to_vec()));
    }
    for input in [
        b"# key value\n a#z 1\n a 2\n".as_slice(),
        b"# key value\n a#z 1\n  a 2\n",
        b"# key value\n a 2\n a#b 3\n a#z 1\n",
    ] {
        for op in ["count", "geomean"] {
            cases.push((
                args(&["--vnlog", "-s", "-g", "key", op, "value"]),
                input.to_vec(),
            ));
        }
    }
    for a in [
        vec!["--vnlog", "sum", "value"],
        vec!["--vnlog", "-W", "absent", "value"],
        vec!["--vnlog", "-W", "count", "missing"],
        vec!["--vnlog", "--format=%.2f", "sum", "value"],
        vec!["--vnlog", "--vnlog", "count", "value"],
        vec!["--vnl", "count", "value"],
    ] {
        cases.push((args(&a), b"# value\n1\nNA\n".to_vec()));
    }
    for operation in ["count", "sum", "geomean"] {
        for input in [
            b"".as_slice(),
            b"## metadata\n#! interpreter\n# \n\n",
            b"# key value\n",
        ] {
            cases.push((
                args(&["--vnlog", "-s", "-g", "key", operation, "value"]),
                input.to_vec(),
            ));
        }
    }
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn annotated_headers_and_missing_markers_preserve_meaning() {
    for (a, input, expected) in [
        (
            args(&["--vnlog", "--narm", "sum", "value", "count", "value"]),
            b"## metadata\n# value\n1# note\nNA\n2\n".as_slice(),
            b"# sum(value) count(value)\n3 2\n".as_slice(),
        ),
        (
            args(&["--vnlog", "--narm", "collapse", "value"]),
            b"# value\n-\nNA\nliteral\n".as_slice(),
            b"# collapse(value)\n-,literal\n".as_slice(),
        ),
        (
            args(&["--vnlog", "count", "01"]),
            b"# x 01\na b\n".as_slice(),
            b"# count(01)\n1\n".as_slice(),
        ),
        (
            args(&["--vnlog", "count", "value"]),
            b"## nothing\n# \0tail\n\n".as_slice(),
            b"".as_slice(),
        ),
    ] {
        let output = invoke(&candidate(), &a, input, false, false);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, expected);
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn annotated_spill_long_records_and_output_failure() {
    let mut input = b"# key value\nb ".to_vec();
    input.extend(vec![b'x'; 100_000]);
    input.extend_from_slice(b" # annotation\na y\n");
    let mut expected = b"# GroupBy(key) first(value)\na y\nb ".to_vec();
    expected.extend(vec![b'x'; 100_000]);
    expected.push(b'\n');
    let a = args(&["--vnlog", "-s", "-g", "key", "first", "value"]);
    let output = invoke(&candidate(), &a, &input, true, false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty());
    let failed = invoke(&candidate(), &a, &input, true, true);
    assert_eq!(failed.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("No space left on device"));
}
