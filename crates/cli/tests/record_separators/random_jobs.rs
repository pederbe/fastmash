//! Seeded selection, invocation ownership and the safe long-option correction.
use super::*;

#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn random_jobs_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for seed in [
        "",
        "0",
        "1",
        "7",
        "2147483647",
        "2147483648",
        "4294967295",
        "4294967296",
        "4294967297",
        "9223372036854775807",
        "+7",
        "-0",
        " \t7",
        "0007",
        "-1",
        "9223372036854775808",
        "18446744073709551615",
        " ",
        "+",
        "1 ",
        "0x7",
        "7x",
    ] {
        for a in [
            vec!["-S", seed, "rand", "1"],
            vec!["-S", "7", "-S", seed, "rand", "1"],
        ] {
            cases.push((args(&a), b"a\nb\nc\nd\ne\nf\ng\nh\n".to_vec()));
        }
    }
    for seed in ["0", "7", "2147483648", "4294967295"] {
        for flags in [
            vec![],
            vec!["--narm"],
            vec!["--header-out"],
            vec!["-H", "--narm"],
        ] {
            for ops in [
                vec!["rand", "2"],
                vec!["rand", "1", "rand", "2"],
                vec!["count", "1", "rand", "2", "rand", "2", "last", "1"],
            ] {
                let mut a = args(&["-S", seed]);
                a.extend(args(&flags));
                a.extend(args(&ops));
                cases.push((
                    a,
                    b"key\tvalue\na\tNA\na\tx\nb\t\nb\ty\nb\tNaN\na\tz\n".to_vec(),
                ));
            }
        }
        for sort in [false, true] {
            for narm in [false, true] {
                let mut a = args(&["-S", seed, "-g1"]);
                if sort {
                    a.push("-s".into());
                }
                if narm {
                    a.push("--narm".into());
                }
                a.extend(args(&["rand", "2", "rand", "2"]));
                cases.push((
                    a,
                    b"b\tNA\na\tx\na\ty\nb\tz\nc\tNA\nc\tNaN\nb\tu\n".to_vec(),
                ));
            }
        }
    }
    for input in [
        b"".as_slice(),
        b"\t\n",
        b"a\x00hidden\nb\x00tail\n",
        b"\xff\n\x80\n",
        b"NA\nNaN\nN/A\n",
        b"#comment\na\nb\n",
        b"a\nb\nc\nd\n",
    ] {
        for flags in [vec![], vec!["--narm"], vec!["-C"]] {
            let mut a = args(&["-S0"]);
            a.extend(args(&flags));
            a.extend(args(&["rand", "1"]));
            cases.push((a, input.to_vec()));
        }
    }
    cases.push((
        args(&["-z", "-S7", "rand", "1"]),
        b"a\nb\x00c\x00d\x00".to_vec(),
    ));
    cases.push((
        args(&["-Hsg1", "-S7", "rand", "value", "rand", "2"]),
        b"key\tvalue\nb\tx\na\ty\na\tz\nb\tu\n".to_vec(),
    ));
    let long_input = (0..128)
        .map(|i| format!("{}\t{}\n", i / 32, i))
        .collect::<String>();
    for seed in ["0", "7", "2147483648", "4294967295"] {
        for operations in [vec!["rand", "1,2"], vec!["-g1", "rand", "2", "rand", "2"]] {
            let mut a = args(&["-S", seed]);
            a.extend(args(&operations));
            cases.push((a, long_input.as_bytes().to_vec()));
        }
    }
    compare_cases(&cases, &reference, &evidence);
    let spill = evidence.with_extension("spill");
    fs::create_dir(&spill).unwrap();
    let sorted: Vec<_> = cases
        .into_iter()
        .filter(|(a, _)| a.iter().any(|s| s == "-s" || s == "-Hsg1"))
        .collect();
    compare_cases_settings(&sorted, &reference, &spill, "C", true);
}

#[test]
fn random_jobs_independent_draw_order_and_long_seed() {
    // Retained independently probed glibc seed-zero stream (rand-reference).
    // Reduce explicit draws here without reimplementing the production PRNG.
    let draws = [
        1_804_289_383u64,
        846_930_886,
        1_681_692_777,
        1_714_636_915,
        1_957_747_793,
        424_238_335,
        719_885_386,
        1_649_760_492,
    ];
    let mut selected = [1usize; 2];
    for row in 0..4 {
        for op in 0..2 {
            if draws[row * 2 + op].is_multiple_of(row as u64 + 1) {
                selected[op] = row + 1;
            }
        }
    }
    let expected = format!("{}\t{}\n", selected[0], selected[1]);
    for seed in [
        vec!["-S0"],
        vec!["--seed=0"],
        vec!["--seed", "0"],
        vec!["--seed", "4294967296"],
        vec!["--seed=7", "-S0"],
        vec!["-S7", "--seed=0"],
    ] {
        let mut a = args(&seed);
        a.extend(args(&["rand", "1", "rand", "1"]));
        for _ in 0..2 {
            let out = invoke(&candidate(), &a, b"1\n2\n3\n4\n", false, false);
            assert!(out.status.success(), "{out:?}");
            assert_eq!(out.stdout, expected.as_bytes());
            assert!(out.stderr.is_empty());
        }
    }
    for seed in [
        "0",
        "1",
        "7",
        "2147483648",
        "4294967295",
        "9223372036854775807",
        "",
        "-0",
        "+7",
        " \t7",
        "-1",
        " ",
        "1 ",
        "9223372036854775808",
    ] {
        let short = invoke(
            &candidate(),
            &args(&["-S", seed, "rand", "1"]),
            b"1\n2\n3\n4\n",
            false,
            false,
        );
        for a in [
            args(&["--seed", seed, "rand", "1"]),
            args(&[&format!("--seed={seed}"), "rand", "1"]),
        ] {
            let long = invoke(&candidate(), &a, b"1\n2\n3\n4\n", false, false);
            assert_eq!(
                (long.status, long.stdout, long.stderr),
                (short.status, short.stdout.clone(), short.stderr.clone())
            );
        }
    }
    let missing = invoke(&candidate(), &args(&["--seed"]), b"", false, false);
    assert_eq!(missing.status.code(), Some(1));
    assert!(missing.stdout.is_empty());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("requires an argument"));
}

#[test]
fn random_jobs_unseeded_membership_wide_fields_and_output_failure() {
    // Membership only: do not assert different outcomes from random trials.
    for _ in 0..16 {
        let out = invoke(
            &candidate(),
            &args(&["--narm", "-g1", "rand", "2"]),
            b"a\tNA\na\tx\na\ty\nb\tu\nb\tv\n",
            false,
            false,
        );
        assert!(out.status.success(), "{out:?}");
        assert!(out.stderr.is_empty());
        assert!(
            [
                b"a\tx\nb\tu\n".as_slice(),
                b"a\tx\nb\tv\n",
                b"a\ty\nb\tu\n",
                b"a\ty\nb\tv\n"
            ]
            .contains(&out.stdout.as_slice())
        );
    }
    let mut input = b"a\t".to_vec();
    input.extend(vec![b'x'; 100_000]);
    input.extend_from_slice(b"\nb\ty\n");
    for spill in [false, true] {
        let a = args(&["-S7", "-sg1", "rand", "2"]);
        let out = invoke(&candidate(), &a, &input, spill, false);
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, input);
        let failed = invoke(&candidate(), &a, &input, spill, true);
        assert_eq!(failed.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&failed.stderr).contains("write error"));
        // C-locale rand sorting uses the external supervisor; its memory
        // policy is not controlled by FASTMASH_SORT_MEMORY_BYTES. Language
        // sorting selects the Rust path, where this setting forces spill.
        for locale in ["en_US.UTF-8", "de_DE.UTF-8"] {
            let native =
                super::locale_delivery::run(&candidate(), &a, &input, &[("LC_ALL", locale)], spill);
            assert!(native.status.success(), "{native:?}");
            assert_eq!(native.stdout, input);
            assert!(native.stderr.is_empty());
        }
    }
}
