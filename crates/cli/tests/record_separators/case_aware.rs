use super::*;

#[test]
fn case_locale_selection_is_explicit() {
    for (vars, sorted, status) in [
        (vec![], true, 0),
        (vec![("LANG", "POSIX")], true, 0),
        (vec![("LANG", "bad"), ("LC_ALL", "C")], true, 0),
        (vec![("LC_ALL", ""), ("LANG", "C")], true, 0),
        // Character type and messages never refuse (they only select quotation).
        (vec![("LC_CTYPE", "bad")], false, 0),
        (vec![("LANG", "en_US.UTF-8")], true, 0),
        (vec![("LC_CTYPE", "C.UTF-8")], false, 0),
        // A name glibc has no locale for is C; a glibc locale whose order is
        // not verified refuses sorting.
        (vec![("LC_COLLATE", "bad")], true, 0),
        (vec![("LC_COLLATE", "cs_CZ.UTF-8")], true, 77),
        (vec![("LC_COLLATE", "cs_CZ.UTF-8")], false, 0),
        (vec![("LC_COLLATE", "de_DE.utf8")], true, 0),
        (vec![("LC_NUMERIC", "de_DE.utf8")], true, 0),
        (
            vec![("LC_ALL", "cs_CZ.UTF-8"), ("LC_COLLATE", "C")],
            true,
            77,
        ),
    ] {
        let o = Command::new(candidate())
            .env_clear()
            .envs(vars.clone())
            .args(if sorted {
                vec!["-isg1", "count", "1"]
            } else {
                vec!["-i", "count", "1"]
            })
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(o.status.code(), Some(status), "{vars:?}: {o:?}");
        if status == 77 {
            assert!(o.stdout.is_empty());
            assert!(String::from_utf8_lossy(&o.stderr).contains("locale"));
        }
    }
}

#[test]
fn case_spill_large_keys_and_output_failures() {
    let mut input = Vec::new();
    for row in 0..10_000 {
        input.extend_from_slice(if row % 2 == 0 { b"key" } else { b"KEY" });
        input.extend_from_slice(b"\t1\n");
    }
    let a = args(&["-isg1", "count", "2"]);
    let output = invoke(&candidate(), &a, &input, true, false);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"key\t10000\n");
    for a in [
        a,
        args(&["-i", "unique", "1"]),
        args(&["-is", "rmdup", "1"]),
    ] {
        let output = invoke(&candidate(), &a, &input[..12], true, true);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("No space left on device"));
    }
    let mut wide = vec![b'a'; 100_000];
    wide.extend_from_slice(b"\t1\n");
    wide.extend(std::iter::repeat_n(b'A', 100_000));
    wide.extend_from_slice(b"\t2\n");
    let output = invoke(
        &candidate(),
        &args(&["-isg1", "sum", "2"]),
        &wide,
        true,
        false,
    );
    let mut expected = vec![b'a'; 100_000];
    expected.extend_from_slice(b"\t3\n");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, expected);
}

#[test]
#[ignore = "requires RefGene data, named reference and fresh evidence directory"]
fn case_aware_real_data() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let input = fs::read(std::env::var_os("FASTMASH_REFGENE").unwrap()).unwrap();
    let cases = vec![
        (args(&["-isg3", "count", "2", "unique", "4"]), input.clone()),
        (args(&["-isg3", "sum", "9"]), input.clone()),
    ];
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn case_aware_observed_contract() {
    for (a, input, expected) in [
        (
            vec!["-i", "unique", "1", "countunique", "1"],
            b"b\nB\na\nA\nZ\n[\n_\n".as_slice(),
            b"[,_,a,b,Z\t5\n".as_slice(),
        ),
        (
            vec!["-ig1", "collapse", "2"],
            b"a\t1\nA\t2\na\0x\t3\nA\0y\t4\n",
            b"a\t1,2\na\0x\t3,4\n",
        ),
        (
            vec!["-isg1", "collapse", "2"],
            b"a\t1\n[\t2\n_\t3\nZ\t4\nA\t5\n",
            b"a\t1,5\nZ\t4\n[\t2\n_\t3\n",
        ),
        (
            vec!["-i", "rmdup", "1"],
            b"a\t1\nA\t2\na\t3\n",
            b"a\t1\nA\t2\n",
        ),
        (
            vec!["-is", "rmdup", "1"],
            b"a\t1\nA\t2\na\t3\n",
            b"a\t1\nA\t2\n",
        ),
    ] {
        for spill in [false, true] {
            let output = invoke(&candidate(), &args(&a), input, spill, false);
            assert!(output.status.success(), "{a:?}: {output:?}");
            assert_eq!(output.stdout, expected, "{a:?}");
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn case_aware_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for flags in [
        vec!["-i"],
        vec!["--ignore-case"],
        vec!["-is"],
        vec!["-iH"],
        vec!["-isH"],
        vec!["-iW"],
        vec!["-isW"],
        vec!["-if"],
        vec!["-isf"],
        vec!["-iC"],
        vec!["-i", "--header-out"],
    ] {
        for op in [
            vec!["-g1", "count", "2", "first", "2", "last", "2"],
            vec!["-g1", "sum", "2"],
            vec!["-g1", "pcov", "2:3"],
            vec!["-g1,2", "collapse", "3"],
            vec!["unique", "1", "countunique", "1", "collapse", "1"],
            vec!["rmdup", "1"],
            vec!["cut", "1"],
            vec!["check"],
        ] {
            for input in [
                b"".as_slice(),
                b"a\t1\t2\nA\t3\t4\nb\t5\t6\nB\t7\t8\na\t9\t10\n",
                b"a\0x\t1\t2\nA\0y\t3\t4\na\0long\t5\t6\n",
                b"[\t1\t2\nZ\t3\t4\nz\t5\t6\n_\t7\t8\n",
                b"\xff\t1\t2\n\xc4\t3\t4\n\xe4\t5\t6\n",
                b" a  1  2\n A  3  4\n",
                b"a\t1\t2\nA\n",
                b"#comment\na\t1\t2\nA\t3\t4\n",
            ] {
                let mut a = args(&flags);
                a.extend(args(&op));
                cases.push((a, input.to_vec()));
            }
        }
    }
    for a in [
        vec!["-iH", "count", "KEY"],
        vec!["-isHgKEY", "sum", "value"],
        vec!["-isHgkey", "sum", "value"],
        vec!["-izsg1", "collapse", "2"],
        vec!["--vnlog", "-isgkey", "sum", "value"],
        vec!["--ignore-case=yes", "count", "1"],
    ] {
        for input in [
            b"key\tvalue\nb\t1\nB\t2\na\t3\n".as_slice(),
            b"# key value\nb 1 #x\nB 2\na 3\n",
            b"b\tx\0B\ty\0a\tz\0",
        ] {
            cases.push((args(&a), input.to_vec()));
        }
    }
    let mut bytes = Vec::new();
    for byte in (0..=255).filter(|b| !matches!(b, 9 | 10)) {
        bytes.extend_from_slice(&[byte, 9, b'1', 10]);
    }
    for a in [
        vec!["-ig1", "count", "2"],
        vec!["-isg1", "count", "2"],
        vec!["-i", "unique", "1", "countunique", "1"],
    ] {
        cases.push((args(&a), bytes.clone()));
    }
    compare_cases(&cases, &reference, &evidence);
    let spill = evidence.join("spill");
    fs::create_dir(&spill).unwrap();
    compare_cases_settings(&cases, &reference, &spill, "C", true);
}
