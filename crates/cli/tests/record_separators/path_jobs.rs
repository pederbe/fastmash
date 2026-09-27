use super::*;

const OPS: [&str; 4] = ["dirname", "basename", "extname", "barename"];

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn path_jobs_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for path in [
        b"".as_slice(),
        b"/",
        b"//",
        b"///",
        b"////",
        b"a",
        b"a/",
        b"a//",
        b"a/b",
        b"a//b//",
        b"/a",
        b"//a",
        b"///a",
        b"//a/b",
        b".",
        b"..",
        b"../",
        b"a/./b",
        b"a/../b",
        b".bashrc",
        b".foo.gz",
        b"a.",
        b"..a",
        b"...",
        b"a..gz",
        b"a.tar.gz",
        b"a.tar.GZ",
        b"a.tar.gz.gpg",
        b"a.tar.std",
        b"a.bz2",
        b"a.tar.zst",
        b"a.x-y",
        b"a.x_y",
        b"a.gz!",
        b"a.\xff",
        b"a.\xff.gz",
        b"a.x-y.gz",
        b"a.123.gz",
        b"a.gz.gz",
        b"C:\\foo\\bar.txt",
        b"a\0/b.txt",
        b"\0",
        b"\0a",
        b"\xff/\xfe.txt",
        b"NA",
        b"NaN",
        b"N/A",
        b" a / b ",
    ] {
        for flags in [
            vec!["-t|"],
            vec!["-t|", "--narm"],
            vec!["-t|", "--header-out"],
            vec!["-t|", "-f", "--output-delimiter=;"],
        ] {
            let mut a = args(&flags);
            for op in OPS {
                a.extend(args(&[op, "1"]));
            }
            let mut input = path.to_vec();
            input.extend_from_slice(b"|other\n");
            cases.push((a, input));
        }
    }
    // Composition exercises slash runs and every advertised add-on suffix.
    for root in ["", "/", "//", "///", "a/", "a//"] {
        for name in ["x", ".x", "x.", "x.y", "x-y", "x_y", ".."] {
            for suffix in [
                "", ".gz", ".xz", ".lz", ".gpg", ".bz2", ".std", ".GZ", ".gz.xz",
            ] {
                let mut a = args(&["-t|"]);
                for op in OPS {
                    a.extend(args(&[op, "1"]));
                }
                cases.push((a, format!("{root}{name}{suffix}/|x\n").into_bytes()));
            }
        }
    }
    for op in OPS {
        for a in [
            vec!["-Hf", op, "value,other"],
            vec!["--vnlog", op, "value"],
            vec!["-s", op, "1"],
            vec!["-g1", op, "2"],
            vec![op, "1:2"],
            vec![op, "1", "count", "1"],
            vec![op, "1", "debase64", "2"],
            vec![op, "2,1"],
        ] {
            cases.push((args(&a), b"value\tother\na/b.tar.gz\t!\n".to_vec()));
        }
        for parameter in ["1", "1.5", "x", "", "1:2"] {
            cases.push((
                args(&[&format!("{op}:{parameter}"), "1"]),
                b"a/b\n".to_vec(),
            ));
        }
        cases.push((args(&["-z", op, "1"]), b"a\nb.txt\0a|b\0".to_vec()));
        cases.push((args(&[op, "1", op, "3"]), b"a\tb\tc\nx\ty\n".to_vec()));
        let mut input = vec![b'a'; 1_000_000];
        input.extend_from_slice(b"/b.tar.gz\nx.txt\n");
        cases.push((args(&[op, "1"]), input));
    }
    let mut chain = b"x.tar".to_vec();
    chain.extend_from_slice(&b".gz".repeat(100_000));
    chain.push(b'\n');
    cases.push((args(&["extname", "1", "barename", "1"]), chain));
    compare_cases(&cases, &reference, &evidence);
    for op in OPS {
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
            assert_eq!(actual.stdout, expected.stdout);
            assert_eq!(actual.stderr, expected.stderr);
            assert_eq!(actual.status, expected.status);
        }
    }
}

#[test]
fn path_jobs_independent_examples_and_output_failures() {
    for (op, expected) in [
        ("dirname", "a"),
        ("basename", "b.tar.gz"),
        ("extname", "tar.gz"),
        ("barename", "b"),
    ] {
        let out = invoke(
            &candidate(),
            &args(&[op, "1"]),
            b"a/b.tar.gz\n",
            false,
            false,
        );
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, format!("{expected}\n").as_bytes());
        let out = invoke(
            &candidate(),
            &args(&[op, "1"]),
            b"a/b.tar.gz\n",
            false,
            true,
        );
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("write error"));
    }
    let request = args(&[
        "dirname", "1", "basename", "1", "extname", "1", "barename", "1",
    ]);
    let out = invoke(
        &candidate(),
        &request,
        &b"a/b.tar.gz\n".repeat(100_000),
        false,
        false,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"a\tb.tar.gz\ttar.gz\tb\n".repeat(100_000));
    let out = invoke(
        &candidate(),
        &args(&["--narm", "basename", "1"]),
        b"a/b\nNA\nx/y\n",
        false,
        false,
    );
    assert!(out.status.success());
    assert_eq!(out.stdout, b"b\n\ny\n");
}
