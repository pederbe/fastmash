use super::*;

#[test]
fn mixed_moments_match_individual_commands_across_output_routes() {
    let orders = [
        ["pskew", "sskew", "count", "pkurt", "skurt", "jarque", "dpo"],
        ["dpo", "jarque", "skurt", "count", "pkurt", "sskew", "pskew"],
    ];
    let input =
        b"key\tx\ty\na\t0\t4\na\t0\t3\na\t0\t2\na\t4\t1\nb\tNA\tNA\nb\t1\t2\nb\t2\t4\nb\t8\t8\n";
    for prefix in [
        vec!["-H", "--narm"],
        vec!["-H", "--narm", "-g", "key"],
        vec!["-H", "--narm", "-s", "-g", "key"],
    ] {
        for spill in [false, true] {
            for order in orders {
                for alternating in [false, true] {
                    for paired in [false, true] {
                        let mut arguments = prefix.clone();
                        let mut singles = Vec::new();
                        for (i, op) in order.iter().enumerate() {
                            let field = if alternating && i % 2 != 0 { "y" } else { "x" };
                            arguments.extend([op, field]);
                            let mut single = prefix.clone();
                            single.extend([op, field]);
                            let result = invoke(&candidate(), &args(&single), input, spill, false);
                            assert!(result.status.success(), "{:?}", result.stderr);
                            singles.push(result.stdout);
                        }
                        if paired {
                            arguments.extend(["dotprod", "x:y"]);
                        }
                        let result = invoke(&candidate(), &args(&arguments), input, spill, false);
                        assert!(result.status.success(), "{:?}", result.stderr);
                        // -H includes output headers; compare each result column, including
                        // the second group which must not retain the first group's cache.
                        let grouped = prefix.contains(&"-g");
                        for (row, line) in result
                            .stdout
                            .split(|&c| c == b'\n')
                            .filter(|s| !s.is_empty())
                            .enumerate()
                        {
                            let fields: Vec<_> = line.split(|&c| c == b'\t').collect();
                            for (i, single) in singles.iter().enumerate() {
                                let expected = single
                                    .split(|&c| c == b'\n')
                                    .nth(row)
                                    .unwrap()
                                    .split(|&c| c == b'\t')
                                    .next_back()
                                    .unwrap();
                                assert_eq!(fields[i + usize::from(grouped)], expected);
                            }
                        }
                    }
                }
            }
        }
    }
}

fn cases() -> Vec<(Vec<OsString>, Vec<u8>)> {
    let mut cases = Vec::new();
    for op in ["pskew", "sskew", "pkurt", "skurt"] {
        for input in [
            "",
            "1\n",
            "1\n2\n",
            "1\n2\n3\n",
            "1\n2\n3\n4\n",
            "0\n0\n0\n4\n",
            "0.1\n0.2\n0.4\n1.5\n",
            "1\n1\n1\n1\n",
            "-0\n0\n-0\n0\n",
            "1e3000\n2e3000\n3e3000\n4e3000\n",
            "1e-3000\n2e-3000\n3e-3000\n4e-3000\n",
            "nan\n1\n2\n3\n",
            "-nan(2)\nnan(3)\n2\n3\n",
            "inf\n1\n2\n3\n",
            "-inf\ninf\n2\n3\n",
            "NA\n1\n2\n3\n4\n",
            "NA\nNA\n",
            "bad\n",
            "1e4933\n",
            "9007199254740992\n9007199254740993\n9007199254740994\n9007199254740996\n",
        ] {
            for narm in [false, true] {
                for hex in [false, true] {
                    let mut a = args(&[op, "1"]);
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
        cases.push((
            args(&["-Hsg", "key", op, "value"]),
            b"key\tvalue\nb\t1\na\t1\na\t2\na\t3\na\t4\nb\t2\nb\t3\nb\t4\n".to_vec(),
        ));
        cases.push((args(&["-z", op, "1"]), b"1\x002\x003\x004\x00".to_vec()));
        cases.push((args(&[op, "2"]), b"1\n".to_vec()));
    }
    cases
}

#[test]
#[ignore = "requires named reference and fresh evidence directory"]
fn moments_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let all = cases();
    let policy_inputs: [&[u8]; 6] = [
        b"1\n1\n1\n1\n",
        b"-0\n0\n-0\n0\n",
        b"1e3000\n2e3000\n3e3000\n4e3000\n",
        b"1e-3000\n2e-3000\n3e-3000\n4e-3000\n",
        b"inf\n1\n2\n3\n",
        b"-inf\ninf\n2\n3\n",
    ];
    let exact: Vec<_> = all
        .iter()
        .filter(|(_, input)| !policy_inputs.contains(&input.as_slice()))
        .cloned()
        .collect();
    compare_cases(&exact, &reference, &evidence);
    for (a, input) in all
        .iter()
        .filter(|(_, input)| policy_inputs.contains(&input.as_slice()))
    {
        let gnu = invoke(&reference, a, input, false, false);
        let actual = invoke(&candidate(), a, input, false, false);
        assert!(gnu.status.success() && actual.status.success());
        assert_eq!(gnu.stdout, b"-nan\n");
        assert_eq!(actual.stdout, b"nan\n");
        assert!(gnu.stderr.is_empty() && actual.stderr.is_empty());
    }
}

#[test]
#[ignore = "requires named reference and fresh evidence directory"]
fn moments_large_and_shared_reference() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap())
        .with_extension("large");
    fs::create_dir(&evidence).unwrap();
    let mut cases = Vec::new();
    for n in [65_535, 65_536, 65_537, 100_000] {
        let mut input = Vec::new();
        for i in 0..n {
            input.extend_from_slice(if i % 4 == 0 { b"4\n" } else { b"0\n" });
        }
        cases.push((
            args(&["pskew", "1", "sskew", "1", "pkurt", "1", "skurt", "1"]),
            input,
        ));
    }
    for op in ["pskew", "sskew", "pkurt", "skurt"] {
        cases.push((
            args(&["--format=%a", "median", "1", op, "1", "mad", "1", op, "1"]),
            b"0x1p64\n1\n-0x1p64\n2\n3\n4\n5\n6\n".to_vec(),
        ));
        cases.push((
            args(&["-H", "--narm", op, "x", op, "1"]),
            b"x\nNA\n0\n0\n0\n4\n".to_vec(),
        ));
    }
    compare_cases(&cases, &reference, &evidence);
}

#[test]
fn moments_independent_oracles_reset_and_failures() {
    let a = args(&["pskew", "1", "sskew", "1", "pkurt", "1", "skurt", "1"]);
    let output = invoke(&candidate(), &a, b"-1\n1\n-1\n1\n", false, false);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"0\t0\t-2\t-6\n");
    let output = invoke(
        &candidate(),
        &args(&["sskew", "1", "skurt", "1"]),
        b"0\n0\n0\n4\n",
        false,
        false,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"2\t4\n");
    let mut input = b"a\t-1\na\t1\n".repeat(50_000);
    input.extend_from_slice(b"b\t-1\nb\t1\nb\t-1\nb\t1\na\t1\n");
    let output = invoke(
        &candidate(),
        &args(&["-g", "1", "pskew", "2", "pkurt", "2"]),
        &input,
        false,
        false,
    );
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a\t0\t-2\nb\t0\t-2\na\tnan\tnan\n");
    for op in ["pskew", "sskew", "pkurt", "skurt"] {
        let output = invoke(
            &candidate(),
            &args(&[op, "1"]),
            b"0\n0\n0\n4\n",
            false,
            true,
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("write error"));
    }
}

#[test]
fn moments_keep_normality_and_fallback_ownership() {
    let input = b"0x1p64\n1\n-0x1p64\n2\n3\n4\n5\n6\n";
    let output = invoke(
        &candidate(),
        &args(&[
            "--format=%a",
            "median",
            "1",
            "pskew",
            "1",
            "mad",
            "1",
            "pskew",
            "1",
            "jarque",
            "1",
            "dpo",
            "1",
        ]),
        input,
        false,
        false,
    );
    assert!(output.status.success());
    // Previous installed Fastmash value; GNU exp differs by one final bit in jarque.
    assert_eq!(output.stdout, b"0xep-2\t-0xcp-64\t0xb.dc5d638865948p-2\t-0xcp-64\t0xd.8b306bd111d840bp-4\t0xf.ad5e47e74aa2d9p-8\n");
    let input = b"0\t0x1p-16445\n0\t0x1p-16445\n0\t0x1p-16445\n4\t0x1p-16445\n";
    let output = invoke(
        &candidate(),
        &args(&["geomean", "2", "sskew", "1", "skurt", "1"]),
        input,
        false,
        false,
    );
    assert!(output.status.success());
    assert!(output.stdout.ends_with(b"\t2\t4\n"));
}
