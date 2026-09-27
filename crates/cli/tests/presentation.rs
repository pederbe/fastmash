use std::{
    ffi::{OsStr, OsString},
    fs,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

fn candidate() -> OsString {
    std::env::var_os("FASTMASH_PRESENTATION_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into())
}
fn invoke(binary: &OsStr, args: &[String], input: &[u8], full: bool) -> Output {
    invoke_at(binary, args, input, full, false)
}
fn invoke_at(binary: &OsStr, args: &[String], input: &[u8], full: bool, german: bool) -> Output {
    let path = std::env::temp_dir().join(format!(
        "presentation-{}-{:?}.tsv",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let mut command = Command::new(binary);
    command
        .arg0("fastmash")
        .args(args)
        .env_clear()
        .env("LC_CTYPE", "C")
        .env("LC_MESSAGES", "C")
        .env("LC_NUMERIC", if german { "de_DE.utf8" } else { "C" })
        .env("PATH", "/usr/bin:/bin")
        .stdin(fs::File::open(&path).unwrap())
        .stdout(if full {
            Stdio::from(
                fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .unwrap(),
            )
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::piped());
    let result = command.output().unwrap();
    fs::remove_file(path).unwrap();
    result
}
fn args(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}
#[test]
fn independent_display_rounding_does_not_change_calculation() {
    for (format, input, expected) in [
        ("%.0f", "2.5\n", "2\n"),
        ("%.0f", "3.5\n", "4\n"),
        ("%+.2f", "-0\n", "-0.00\n"),
        ("%#.3g", "1\n", "1.00\n"),
        ("%.2a", "1.25\n", "0xa.00p-3\n"),
    ] {
        let out = invoke(
            &candidate(),
            &args(&["--format", format, "min", "1"]),
            input.as_bytes(),
            false,
        );
        assert!(out.status.success(), "{out:?}");
        assert_eq!(out.stdout, expected.as_bytes());
    }
    let out = invoke(
        &candidate(),
        &args(&["--round=2", "sum", "1", "mean", "1"]),
        b"0.004\n0.004\n",
        false,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"0.01\t0.00\n");
    let out = invoke_at(
        &candidate(),
        &args(&["--format=%'012.2f", "sum", "1"]),
        b"1234,5\n",
        false,
        true,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout, b"00001.234,50\n");
}
#[test]
fn growable_presentation_and_output_failure() {
    let out = invoke(
        &candidate(),
        &args(&["--format=%.17000f", "min", "1"]),
        b"1\n",
        false,
    );
    assert!(out.status.success(), "{out:?}");
    assert_eq!(out.stdout.len(), 17003);
    assert!(out.stdout.starts_with(b"1."));
    let out = invoke(
        &candidate(),
        &args(&["--format=%20000.2f", "sum", "1"]),
        b"1\n",
        true,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
}

#[test]
fn locale_precedence_and_explicit_refusal() {
    let path = std::env::temp_dir().join(format!("presentation-locale-{}.tsv", std::process::id()));
    fs::write(&path, b"1.25\n").unwrap();
    for (variables, status, expected) in [
        (
            vec![("LANG", "de_DE.utf8"), ("LC_NUMERIC", "C")],
            0,
            b"1.25\n".as_slice(),
        ),
        (
            vec![
                ("LANG", "unsupported"),
                ("LC_NUMERIC", "de_DE.utf8"),
                ("LC_ALL", "C"),
            ],
            0,
            b"1.25\n".as_slice(),
        ),
        (vec![("LC_NUMERIC", "C.UTF-8")], 0, b"1.25\n".as_slice()),
        (vec![("LC_NUMERIC", "ps_AF.utf8")], 77, b"".as_slice()),
        // A comma-decimal locale reads dot-decimal input as invalid.
        (vec![("LC_NUMERIC", "fr_FR.utf8")], 1, b"".as_slice()),
        // Recognized German environment, but dot-decimal input is invalid.
        (vec![("LC_ALL", "de_DE.utf8")], 1, b"".as_slice()),
    ] {
        let out = Command::new(candidate())
            .args(["sum", "1"])
            .env_clear()
            .env("LC_CTYPE", "C")
            .env("LC_MESSAGES", "C")
            .envs(variables)
            .stdin(fs::File::open(&path).unwrap())
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(status), "{out:?}");
        assert_eq!(out.stdout, expected);
    }
    fs::remove_file(path).unwrap();
}
#[test]
#[ignore = "requires named GNU reference and fresh evidence directory"]
fn named_reference_presentation() {
    let reference = std::env::var_os("FASTMASH_PRESENTATION_REFERENCE").unwrap();
    let dir = std::path::PathBuf::from(std::env::var_os("FASTMASH_PRESENTATION_EVIDENCE").unwrap());
    fs::create_dir(&dir).unwrap();
    let mut cases = Vec::new();
    for german in [false, true] {
        for fmt in [
            "%f",
            "%e",
            "%g",
            "%a",
            "%F",
            "%E",
            "%G",
            "%A",
            "%.0f",
            "%.0e",
            "%.0g",
            "%.0a",
            "%.1a",
            "%.2a",
            "%.16a",
            "%#.0a",
            "%#g",
            "%#.6g",
            "%+12.2f",
            "% 012.2f",
            "%-+12.2f",
            "%'012.2f",
            "%'.2f",
            "%'g",
            "%#020a",
            "[%10.3g]%%",
            "%.21g",
            "%.50f",
        ] {
            for raw in [
                "0",
                "-0",
                "1.25",
                "2.5",
                "9.999",
                "12345.625",
                "0.000012345",
                "inf",
                "-nan",
            ] {
                let input = if german {
                    raw.replace('.', ",")
                } else {
                    raw.to_string()
                } + "\n";
                cases.push((
                    german,
                    args(&["--format", fmt, "min", "1"]),
                    input.into_bytes(),
                ));
            }
        }
        for (flags, input) in [
            (vec!["--round=2"], "0.004\n0.004\n"),
            (vec!["--format=%.2f", "--round=3"], "1.25\n"),
            (vec!["--round=3", "--format=%.1e"], "12.5\n"),
        ] {
            let mut a = args(&flags);
            a.extend(args(&["sum", "1", "mean", "1", "count", "1", "first", "1"]));
            cases.push((
                german,
                a,
                if german {
                    input.replace('.', ",").into_bytes()
                } else {
                    input.as_bytes().to_vec()
                },
            ));
        }
        for input in ["1,25\n", "1.25\n", "1.234,5\n", "1,234.5\n"] {
            cases.push((german, args(&["sum", "1"]), input.as_bytes().to_vec()));
        }
        cases.push((
            german,
            args(&[
                "--narm",
                "--format=%+10.2f",
                "sum",
                "1",
                "mean",
                "1",
                "min",
                "1",
                "count",
                "1",
            ]),
            b"NA\n".to_vec(),
        ));
        cases.push((
            german,
            args(&["-t,", "--round=2", "sum", "1"]),
            b"1,5\n2,5\n".to_vec(),
        ));
    }
    for fmt in [
        "", "plain", "%%", "%", "%.2", "%d", "%lf", "%Lf", "%*f", "%1$f", "%f %f", "%f%", "%%%d",
    ] {
        cases.push((false, args(&["--format", fmt, "sum", "1"]), b"1\n".to_vec()));
    }
    for n in ["", "0", "-1", "51", "2x", "+2", " 2", "\u{b}2", "2 ", "50"] {
        cases.push((false, args(&["--round", n, "sum", "1"]), b"1.25\n".to_vec()));
    }
    cases.push((
        false,
        args(&["--format=%.17000f", "sum", "1"]),
        b"1\n".to_vec(),
    ));
    let mut failed = Vec::new();
    for german in [false, true] {
        for fmt in [
            "%.0a", "%.1a", "%.14a", "%.15a", "%a", "%.0f", "%.1g", "%.4g", "%.21e", "%.21g",
        ] {
            for raw in [
                "0xf.fffffffffffffffp+0",
                "0x8.8p-3",
                "0x8.800000000000001p-3",
                "0x8.7ffffffffffffffp-3",
                "0xf.fffffffffffffffp+16380",
                "0x1p-16445",
                "0x7.fffffffffffffffp-16385",
                "-0x1p-16445",
                "9.9999",
                "0.000099999",
                "0.0000099999",
            ] {
                let input = if german {
                    raw.replace('.', ",")
                } else {
                    raw.to_string()
                } + "\n";
                cases.push((
                    german,
                    args(&["--format", fmt, "min", "1"]),
                    input.into_bytes(),
                ));
            }
        }
        for op in [
            "sum",
            "mean",
            "min",
            "max",
            "absmin",
            "absmax",
            "range",
            "median",
            "q1",
            "q3",
            "iqr",
            "pvar",
            "svar",
            "pstdev",
            "sstdev",
            "count",
            "countunique",
            "mode",
            "antimode",
            "perc:25",
            "madraw",
            "mad",
        ] {
            for input in ["1\n2\n3\n", "NA\n"] {
                cases.push((
                    german,
                    args(&["--narm", "--format=[%+.3f]", op, "1"]),
                    input.as_bytes().to_vec(),
                ));
            }
        }
        cases.push((
            german,
            args(&["--format=%.3f", "dotprod", "1:2", "pcov", "1:2"]),
            b"1\t2\n3\t4\n".to_vec(),
        ));
        cases.push((
            german,
            args(&["--format=%.2f", "-g1", "sum", "2"]),
            b"a\t1\na\t2\nb\t4\n".to_vec(),
        ));
    }
    for length in [99, 100] {
        cases.push((
            false,
            args(&[
                "--format",
                &format!("{}%g", "x".repeat(length - 2)),
                "sum",
                "1",
            ]),
            b"1\n".to_vec(),
        ));
    }
    for (i, (german, args, input)) in cases.iter().enumerate() {
        fs::write(
            dir.join(format!("{i:04}.args")),
            format!("German={german} {args:?}"),
        )
        .unwrap();
        fs::write(dir.join(format!("{i:04}.input")), input).unwrap();
        let expected = invoke_at(&reference, args, input, false, *german);
        let actual = invoke_at(&candidate(), args, input, false, *german);
        for (name, out) in [("gnu", &expected), ("candidate", &actual)] {
            fs::write(dir.join(format!("{i:04}.{name}.stdout")), &out.stdout).unwrap();
            fs::write(dir.join(format!("{i:04}.{name}.stderr")), &out.stderr).unwrap();
            fs::write(
                dir.join(format!("{i:04}.{name}.status")),
                out.status.to_string(),
            )
            .unwrap();
        }
        if actual.status != expected.status
            || actual.stdout != expected.stdout
            || actual.stderr != expected.stderr
        {
            failed.push(i);
        }
    }
    eprintln!("{} reference cases", cases.len());
    assert!(failed.is_empty(), "mismatches: {failed:?}");
}
