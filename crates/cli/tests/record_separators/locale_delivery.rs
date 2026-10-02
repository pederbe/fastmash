//! The documented locale policy and profile-bound GNU comparisons.
use super::*;

pub(super) fn run(
    binary: &OsStr,
    a: &[OsString],
    input: &[u8],
    env: &[(&str, &str)],
    spill: bool,
) -> Output {
    let path = std::env::temp_dir().join(format!(
        "locale-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let mut c = Command::new(binary);
    c.arg0("fastmash")
        .args(a)
        .env_clear()
        .env("LANG", "C")
        .env("PATH", "/usr/bin:/bin")
        // The documented policy keeps English messages even where GNU catalogs
        // supply localized quotation marks. Compare that named profile here.
        .env("LANGUAGE", "C")
        .envs(env.iter().copied())
        .stdin(fs::File::open(&path).unwrap());
    if let Some(locales) = std::env::var_os("FASTMASH_RECORD_LOCALE_PATH") {
        c.env("LOCPATH", locales);
    }
    if spill {
        c.env("FASTMASH_SORT_MEMORY_BYTES", "1");
    }
    let result = c.output().unwrap();
    fs::remove_file(path).unwrap();
    result
}

#[test]
fn portable_language_order_identity_and_spill() {
    let cases = [
        // Punctuation decides after the letters, in glibc's order: hyphen
        // before low line, as GNU sort gives.
        (
            vec!["-sg1", "collapse", "2"],
            "a_1\t1\na-1\t2\nA1\t3\na1\t4\nZ\t5\nz\t6\n",
            "a-1\t2\na_1\t1\na1\t4\nA1\t3\nz\t6\nZ\t5\n",
        ),
        (
            vec!["-sg1", "collapse", "2"],
            "é\t1\ne\u{301}\t2\né\t3\ne\u{301}\t4\n",
            "e\u{301}\t2,4\né\t1,3\n",
        ),
        // Each key's identity tie is broken before the next key, as glibc's
        // strcoll orders the two spellings: GNU sort gives this order.
        (
            vec!["-sg1,2", "collapse", "3"],
            "é\ta\t1\ne\u{301}\tz\t2\ne\u{301}\ta\t3\né\tz\t4\n",
            "e\u{301}\ta\t3\ne\u{301}\tz\t2\né\ta\t1\né\tz\t4\n",
        ),
        // glibc ties a letter spelled in parts with the letter at every
        // level, so the next key decides, as GNU sort gives.
        (
            vec!["-sg1,2", "collapse", "3"],
            "и\u{306}\tb\t1\nй\ta\t2\n",
            "й\ta\t2\nи\u{306}\tb\t1\n",
        ),
        // Tied keys stay in input order, a Group per run of one spelling, as
        // in GNU; hash grouping gives up for them.
        (
            vec!["-sg1", "collapse", "2"],
            "и\u{306}\t1\nй\t2\nи\u{306}\t3\n",
            "и\u{306}\t1\nй\t2\nи\u{306}\t3\n",
        ),
        (
            vec!["-sg1", "collapse", "2"],
            "й\t1\nи\u{306}\t2\nй\t3\n",
            "й\t1\nи\u{306}\t2\nй\t3\n",
        ),
        (
            vec!["-sg1", "collapse", "2"],
            "L\u{b7}\t1\nL\u{387}\t2\nL\u{b7}\t3\n",
            "L\u{b7}\t1\nL\u{387}\t2\nL\u{b7}\t3\n",
        ),
        (
            vec!["-isg1", "collapse", "2"],
            "Ŀ\t1\nl\u{b7}\t2\nL\u{b7}\t3\nĿ\t4\n",
            "Ŀ\t1\nl\u{b7}\t2,3\nĿ\t4\n",
        ),
        (
            vec!["-sg1,2", "collapse", "3"],
            "й\tи\u{306}\t1\nи\u{306}\tй\t2\nй\tи\u{306}\t3\n",
            "й\tи\u{306}\t1\nи\u{306}\tй\t2\nй\tи\u{306}\t3\n",
        ),
        (
            vec!["-s", "rmdup", "1"],
            "и\u{306}\t1\nй\t2\nи\u{306}\t3\nй\t4\n",
            "и\u{306}\t1\nй\t2\n",
        ),
        (
            vec!["-sg1,2", "collapse", "3"],
            "Ŀ\tb\t1\nL\u{b7}\ta\t2\n",
            "L\u{b7}\ta\t2\nĿ\tb\t1\n",
        ),
        // glibc places a letter after its decomposed spelling, here before
        // a space, and so for letters coded below their base.
        (
            vec!["-sg1,2", "collapse", "3"],
            "Phổ Yên\tb\t1\nPho\u{302}\u{309} Yên\ta\t2\n",
            "Pho\u{302}\u{309} Yên\ta\t2\nPhổ Yên\tb\t1\n",
        ),
        (
            vec!["-sg1,2", "collapse", "3"],
            "Ё\tb\t1\nЕ\u{308}\ta\t2\n",
            "Е\u{308}\ta\t2\nЁ\tb\t1\n",
        ),
        // With -i the key is composed as `sort -f` reads it, uppercased.
        (
            vec!["-isg1,2", "collapse", "3"],
            "l\u{b7}\tb\t1\nĿ\ta\t2\n",
            "Ŀ\ta\t2\nl\u{b7}\tb\t1\n",
        ),
        (
            vec!["-isg1", "collapse", "2"],
            "a\t1\nA\t2\nä\t3\nÄ\t4\n",
            "a\t1,2\nä\t3\nÄ\t4\n",
        ),
        (
            vec!["-is", "dedup", "1"],
            "a\t1\nA\t2\na\t3\n",
            "a\t1\nA\t2\n",
        ),
    ];
    for language in ["en_US.UTF-8", "de_DE.UTF-8"] {
        for spill in [false, true] {
            for (a, input, expected) in &cases {
                let out = run(
                    &candidate(),
                    &args(a),
                    input.as_bytes(),
                    &[("LC_ALL", language)],
                    spill,
                );
                assert!(out.status.success(), "{a:?}: {out:?}");
                assert_eq!(
                    out.stdout,
                    expected.as_bytes(),
                    "{language}, spill={spill}, {a:?}"
                );
                assert!(out.stderr.is_empty());
            }
        }
    }
}

#[test]
fn parallel_language_runs_preserve_headers_comments_case_and_representatives() {
    let mut input = String::from("key\tvalue\n");
    for i in 0..100_000 {
        if i % 1000 == 0 {
            input.push_str("# comment\n");
        }
        let key = ["é", "e\u{301}", "A", "a", "Ä", "ä"][i % 6];
        input.push_str(&format!("{key}\t{}\n", i % 7 + 1));
    }
    for language in ["C", "en_US.UTF-8", "de_DE.UTF-8"] {
        for operations in [
            vec![
                "-H", "-C", "-isg1", "first", "2", "last", "2", "collapse", "2",
            ],
            vec!["-H", "-C", "-isg1", "mean", "2", "sum", "2"],
        ] {
            let serial = run(
                &candidate(),
                &args(&operations),
                input.as_bytes(),
                &[
                    ("LC_ALL", language),
                    ("FASTMASH_SORT_MEMORY_BYTES", "1073741824"),
                ],
                false,
            );
            let spilled = run(
                &candidate(),
                &args(&operations),
                input.as_bytes(),
                &[
                    ("LC_ALL", language),
                    ("FASTMASH_SORT_MEMORY_BYTES", "8388608"),
                ],
                false,
            );
            assert!(serial.status.success(), "{language}: {serial:?}");
            assert!(spilled.status.success(), "{language}: {spilled:?}");
            assert_eq!(serial.stdout, spilled.stdout, "{language}: {operations:?}");
            assert_eq!(serial.stderr, spilled.stderr);
        }
    }
    let rows = input
        .lines()
        .skip(1)
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n");
    let operations = args(&["-is", "dedup", "1"]);
    let serial = run(
        &candidate(),
        &operations,
        rows.as_bytes(),
        &[
            ("LC_ALL", "de_DE.UTF-8"),
            ("FASTMASH_SORT_MEMORY_BYTES", "1073741824"),
        ],
        false,
    );
    let spilled = run(
        &candidate(),
        &operations,
        rows.as_bytes(),
        &[
            ("LC_ALL", "de_DE.UTF-8"),
            ("FASTMASH_SORT_MEMORY_BYTES", "8388608"),
        ],
        false,
    );
    assert!(serial.status.success() && spilled.status.success());
    assert_eq!(serial.stdout, spilled.stdout);
    assert_eq!(serial.stderr, spilled.stderr);
}

#[test]
fn locale_refusals_do_not_remove_raw_byte_jobs() {
    for input in [b"a\xff\t1\n".as_slice(), b"a\0b\t1\n"] {
        for language in ["en_US.UTF-8", "de_DE.UTF-8"] {
            let env = [("LC_ALL", language)];
            let sorted = run(
                &candidate(),
                &args(&["-sg1", "count", "2"]),
                input,
                &env,
                false,
            );
            assert_eq!(sorted.status.code(), Some(77));
            assert!(sorted.stdout.is_empty());
            let raw = run(&candidate(), &args(&["count", "2"]), input, &env, false);
            assert!(raw.status.success(), "{raw:?}");
        }
        let raw = run(
            &candidate(),
            &args(&["-sg1", "count", "2"]),
            input,
            &[("LC_ALL", "C.UTF-8")],
            false,
        );
        assert!(raw.status.success(), "{raw:?}");
    }
    // Only categories that can change a result refuse, and only when used.
    for (category, status) in [
        ("LC_CTYPE", 0),
        ("LC_MESSAGES", 0),
        ("LC_NUMERIC", 77),
        ("LC_ALL", 77),
    ] {
        let out = run(
            &candidate(),
            &args(&["sum", "1"]),
            b"1\n",
            &[(category, "ps_AF.UTF-8")],
            false,
        );
        assert_eq!(out.status.code(), Some(status), "{category}: {out:?}");
    }
    let env = [("LC_COLLATE", "cs_CZ.UTF-8")];
    assert!(
        run(&candidate(), &args(&["count", "1"]), b"1\n", &env, false)
            .status
            .success()
    );
    assert_eq!(
        run(
            &candidate(),
            &args(&["-sg1", "count", "1"]),
            b"1\n",
            &env,
            false
        )
        .status
        .code(),
        Some(77)
    );
}

#[test]
#[ignore = "bounded timing comparison requires old installed CLI and fresh evidence directory"]
fn locale_job_costs() {
    let old = std::env::var_os("FASTMASH_RECORD_BASELINE").unwrap();
    let dir = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&dir).unwrap();
    let mut report = String::from("job\trepeat\tbinary\tlocale\telapsed_ns\n");
    for job in ["identifiers", "many-keys", "spill"] {
        let input: String = (0..32768)
            .map(|i| {
                let n = (i * 7919) % 4096;
                format!("sample{:04}\tgene{:05}\t1\n", n % 32, n)
            })
            .collect();
        let a = args(&[
            if job == "many-keys" { "-sg1,2" } else { "-sg2" },
            "count",
            "3",
        ]);
        let expected = run(
            &old,
            &a,
            input.as_bytes(),
            &[("LC_ALL", "C")],
            job == "spill",
        );
        assert!(expected.status.success(), "{expected:?}");
        fs::write(dir.join(format!("{job}.input")), &input).unwrap();
        fs::write(dir.join(format!("{job}.stdout")), &expected.stdout).unwrap();
        for repeat in 0..6 {
            let mut runs = vec![
                ("old", old.clone(), "C"),
                ("new", candidate(), "C"),
                ("new", candidate(), "en_US.UTF-8"),
            ];
            if repeat % 2 == 1 {
                runs.reverse();
            }
            for (label, binary, locale) in runs {
                let start = std::time::Instant::now();
                let out = run(
                    &binary,
                    &a,
                    input.as_bytes(),
                    &[("LC_ALL", locale)],
                    job == "spill",
                );
                let elapsed = start.elapsed().as_nanos();
                assert!(out.status.success(), "{out:?}");
                assert_eq!(out.stdout, expected.stdout);
                assert!(out.stderr.is_empty());
                report.push_str(&format!("{job}\t{repeat}\t{label}\t{locale}\t{elapsed}\n"));
            }
        }
    }
    fs::write(dir.join("timings.tsv"), report).unwrap();
}

#[test]
#[ignore = "requires installed candidate, GNU reference, locales and fresh evidence directory"]
fn locale_gnu_compatible_slices() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let environments = [
        vec![("LC_ALL", "C")],
        vec![("LC_ALL", "POSIX")],
        vec![("LC_ALL", "C.UTF-8")],
        vec![("LC_ALL", "C.utf8")],
        vec![("LC_ALL", "en_US.UTF-8")],
        vec![("LC_ALL", "en_US.utf8")],
        vec![("LC_ALL", "de_DE.UTF-8")],
        vec![("LC_ALL", "de_DE.utf8")],
        vec![
            ("LANG", "de_DE.UTF-8"),
            ("LC_NUMERIC", "en_US.UTF-8"),
            ("LC_CTYPE", "C"),
            ("LC_MESSAGES", "C"),
        ],
        vec![
            ("LANG", "en_US.UTF-8"),
            ("LC_NUMERIC", "de_DE.UTF-8"),
            ("LC_COLLATE", "C"),
        ],
        vec![("LANG", "unknown"), ("LC_ALL", "C.UTF-8")],
        vec![("LANG", "de_DE.UTF-8"), ("LC_ALL", ""), ("LC_NUMERIC", "C")],
    ];
    let mut jobs: Vec<(Vec<OsString>, Vec<u8>)> = [
        (vec!["sum", "1"], "1.25\n2.5\n"),
        (vec!["sum", "1"], "1,25\n2,5\n"),
        (vec!["sum", "1"], "1,234.5\n"),
        (vec!["sum", "1"], "1.234,5\n"),
        (vec!["--format=%'015.2f", "sum", "1"], "1234\n"),
        (vec!["--format=%Q", "sum", "1"], "1\n"),
        (vec!["bogus", "1"], ""),
        (vec!["sum", "bad"], ""),
        (vec!["-H", "count", "missing"], "header\n"),
        (vec!["-isg1", "count", "2"], "b\t1\na\t2\nA\t3\n"),
        (
            vec!["-sHgkey", "pcov", "x:y"],
            "key\tx\ty\nb\t2\t3\na\t1\t2\na\t3\t4\n",
        ),
        (
            vec!["-sfHgkey", "harmmean", "x"],
            "key\tx\nb\t2\na\t2\na\t2\n",
        ),
        (vec!["-sg1", "rand", "2"], "b\t7\na\t7\na\t7\n"),
        (vec!["-sH", "dedup", "key"], "key\tx\nb\t2\na\t1\na\t3\n"),
        (vec!["-is", "dedup", "1"], "b\t3\na\t1\nA\t2\na\t4\n"),
        (
            vec!["--vnlog", "-s", "dedup", "key"],
            "# key x\nb 2\na 1\na 3\n",
        ),
        (vec!["-Wsg1", "count", "2"], "b  1\na 2\na   3\n"),
        (vec!["-sg2", "count", "1"], "z\tz\nbad\na\ta\n"),
        (vec!["-W", "count", "2"], "a\u{a0}2\n"),
        (vec!["-sHgkey", "count", "value"], ""),
        (vec!["-sH", "dedup", "key"], ""),
    ]
    .into_iter()
    .map(|(a, i)| (args(&a), i.as_bytes().to_vec()))
    .collect();
    for name in ["a'\\\"\t\r\n", "ä’é", "x\u{200b}y", "x\u{85}y"] {
        jobs.push((args(&["-H", "count", name]), b"header\n".to_vec()));
    }
    jobs.push((
        vec![
            "-H".into(),
            "count".into(),
            OsString::from_vec(b"bad\xff".to_vec()),
        ],
        b"header\n".to_vec(),
    ));
    let mut failures = Vec::new();
    let mut count = 0;
    for (e, env) in environments.iter().enumerate() {
        for (j, (a, input)) in jobs.iter().enumerate() {
            for spill in [false, true] {
                let prefix = format!("{e:02}-{j:02}-{spill}");
                fs::write(evidence.join(format!("{prefix}.input")), input).unwrap();
                fs::write(
                    evidence.join(format!("{prefix}.invocation")),
                    format!("{env:?}\n{a:?}\n"),
                )
                .unwrap();
                // Empty unresolved named keys use the existing fixed-C child
                // diagnostic. No records are ordered on this path.
                let reference_env = if matches!(j, 19 | 20) {
                    vec![("LC_ALL", "C")]
                } else {
                    env.clone()
                };
                fs::write(
                    evidence.join(format!("{prefix}.reference-environment")),
                    format!("LANGUAGE=C\n{reference_env:?}\n"),
                )
                .unwrap();
                let expected = run(&reference, a, input, &reference_env, false);
                let actual = run(&candidate(), a, input, env, spill);
                for (label, out) in [("gnu", &expected), ("fastmash", &actual)] {
                    fs::write(
                        evidence.join(format!("{prefix}.{label}.stdout")),
                        &out.stdout,
                    )
                    .unwrap();
                    fs::write(
                        evidence.join(format!("{prefix}.{label}.stderr")),
                        &out.stderr,
                    )
                    .unwrap();
                    fs::write(
                        evidence.join(format!("{prefix}.{label}.status")),
                        out.status.to_string(),
                    )
                    .unwrap();
                }
                if (actual.status.code(), &actual.stdout, &actual.stderr)
                    != (expected.status.code(), &expected.stdout, &expected.stderr)
                {
                    failures.push(prefix);
                }
                count += 1;
            }
        }
    }
    fs::write(
        evidence.join("summary.txt"),
        format!("{count} comparisons\nfailures: {failures:?}\n"),
    )
    .unwrap();
    assert!(failures.is_empty(), "{failures:?}");
}
