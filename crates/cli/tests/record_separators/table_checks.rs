use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn table_checks_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for command in [
        vec!["check"],
        vec!["check", "2", "fields"],
        vec!["check", "lines", "2"],
        vec!["rmdup", "1"],
        vec!["dedup", "2"],
    ] {
        for flags in [
            vec![],
            vec!["-H"],
            vec!["--header-in"],
            vec!["--header-out"],
            vec!["-f"],
            vec!["--no-strict"],
            vec!["-W"],
            vec!["-t,"],
            vec!["--output-delimiter=|"],
            vec!["-C"],
            vec!["--narm"],
            vec!["--filler=x"],
            vec!["-s"],
            vec!["-sH"],
        ] {
            for input in [
                b"".as_slice(),
                b"",
                b"a\tb\nc\td\na\tx\n",
                b"a\tb\nc\n",
                b"\n\n",
                b" a  b \n c d  ",
                b"a,b\na,c\n",
                b"a\0x\tb\na\0y\tc\n",
                b"# hi\na\tb\n",
                b"a\tb",
            ] {
                let mut a = args(&flags);
                a.extend(args(&command));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for word in [
        "lines", "line", "rows", "row", "fields", "field", "columns", "column", "col", "LINES",
        "bad",
    ] {
        for a in [
            vec!["check", word, "2"],
            vec!["check", "2", word],
            vec!["check", word],
            vec!["check", word, "0"],
            vec!["check", word, "2.5"],
        ] {
            cases.push((args(&a), b"a\tb\nc\td\n".to_vec()));
        }
    }
    for a in [
        vec!["check", "2"],
        vec!["check", "0", "lines"],
        vec!["check", "lines", "1", "rows", "2"],
        vec!["check", "1", "col", "2", "fields"],
        vec!["check", "2", "rows", "2", "cols"],
        vec!["check", "2", "rows", "2", "fields"],
        vec!["rmdup"],
        vec!["rmdup", "0"],
        vec!["rmdup", "1:2"],
        vec!["rmdup", "2-1"],
        vec!["dedup", "1", "cut", "2"],
        vec!["-H", "rmdup", "key"],
        vec!["-sH", "rmdup", "key"],
        vec!["-sH", "rmdup", "1"],
        vec!["--vnlog", "rmdup", "key"],
        vec!["--vnlog", "-s", "rmdup", "key"],
        vec!["--vnlog", "check"],
        vec!["-z", "check"],
        vec!["-z", "rmdup", "1"],
        vec!["-g1", "check"],
        vec!["-g1", "rmdup", "1"],
    ] {
        for input in [
            b"key\tvalue\nb\tx\na\ty\nb\tz\n".as_slice(),
            b"# key value\nb x #tail\na y\nb z\n",
            b"a\tb\0a\tc\0",
            b"",
            b"key\tvalue\n",
            b";literal\n\n\0hidden\nx #tail\n",
        ] {
            cases.push((args(&a), input.to_vec()));
        }
    }
    for a in [
        vec!["-sH", "rmdup", "key"],
        vec!["-s", "--header-in", "dedup", "1"],
        vec!["-H", "dedup", "2"],
        vec!["--vnlog", "check", "2", "fields"],
        vec!["-z", "check", "3", "fields"],
    ] {
        for input in [
            b"key\tvalue\na\tfirst\na\tsecond\nb\tthird\n".as_slice(),
            b"# x y\na b #tail\nonly #comment\n",
            b"a\tb\0one\0",
            b"key\tvalue\nonly\n",
        ] {
            cases.push((args(&a), input.to_vec()));
        }
    }
    compare_cases(&cases, &reference, &evidence);
}
#[test]
fn table_checks_large_streams_wide_keys_and_failures() {
    let mut input = Vec::new();
    for n in 0..100_000 {
        input.extend_from_slice(format!("{}\t{n}\n", n % 1000).as_bytes());
    }
    let checked = invoke(
        &candidate(),
        &args(&["check", "100000", "lines", "2", "fields"]),
        &input,
        false,
        false,
    );
    assert!(checked.status.success());
    assert_eq!(checked.stdout, b"100000 lines, 2 fields\n");
    let dedup = invoke(&candidate(), &args(&["rmdup", "1"]), &input, false, false);
    assert!(dedup.status.success());
    let expected: String = (0..1000).map(|n| format!("{n}\t{n}\n")).collect();
    assert_eq!(dedup.stdout, expected.as_bytes());
    // GNU's fixed key arena is unsafe at this size. Independent expected bytes only.
    let mut row = vec![b'x'; 1024 * 1024 + 1];
    row.extend_from_slice(b"\tfirst\n");
    let mut wide = row.clone();
    wide.extend_from_slice(&row);
    let output = invoke(&candidate(), &args(&["dedup", "1"]), &wide, false, false);
    assert!(output.status.success());
    assert_eq!(output.stdout, row);
    for a in [vec!["check"], vec!["dedup", "1"]] {
        let out = invoke(&candidate(), &args(&a), b"a\tb\n", false, true);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
    }
}
#[test]
fn multiple_dedup_keys_have_an_explicit_proposed_refusal() {
    // A deliberate refusal: GNU datamash aborts here, so there is no GNU result.
    for fields in ["1,2", "1,1", "1-2", "1-1000000000"] {
        let output = invoke(
            &candidate(),
            &args(&["rmdup", fields]),
            b"a\tb\n",
            false,
            false,
        );
        assert_eq!(output.status.code(), Some(77));
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"fastmash: rmdup supports exactly one key\n");
    }
}
