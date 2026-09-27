//! Representative portable expectations, independent of the host's GNU results.
use super::decimal_fixtures as fixtures;
use super::*;
use std::os::unix::process::ExitStatusExt;

struct Case {
    name: &'static str,
    arguments: Vec<OsString>,
    input: Vec<u8>,
    stdout: Vec<u8>,
}

fn corpus() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut add = |name, arguments: &[&str], input: &[u8], stdout: &[u8]| {
        cases.push(Case {
            name,
            arguments: args(arguments),
            input: input.to_vec(),
            stdout: stdout.to_vec(),
        });
    };
    // Exact small-integer arithmetic and order statistics. Each selected
    // operation is explicit, so additions can be audited against the ledger.
    add(
        "arithmetic",
        &[
            "count", "1", "sum", "1", "mean", "1", "min", "1", "max", "1", "absmin", "1", "absmax",
            "1", "range", "1",
        ],
        b"1\n2\n3\n4\n",
        b"4\t10\t2.5\t1\t4\t1\t4\t3\n",
    );
    add(
        "ordered",
        &[
            "median",
            "1",
            "q1",
            "1",
            "q3",
            "1",
            "iqr",
            "1",
            "perc:50",
            "1",
            "trimmean:0.25",
            "1",
        ],
        b"1\n2\n3\n4\n",
        b"2.5\t1.75\t3.25\t1.5\t2.5\t2.5\n",
    );
    add(
        "dispersion",
        &["pvar", "1", "svar", "1", "pstdev", "1", "sstdev", "1"],
        b"1\n3\n",
        b"1\t2\t1\t1.4142135623731\n",
    );
    // sqrt(2), geometric sqrt(2), harmonic 4/3, and rms sqrt(5/2).
    add(
        "means",
        &["geomean", "1", "harmmean", "1", "ms", "1", "rms", "1"],
        b"1\n2\n",
        b"1.4142135623731\t1.3333333333333\t2.5\t1.5811388300842\n",
    );
    add(
        "paired",
        &[
            "pcov", "1:2", "scov", "1:2", "ppearson", "1:2", "spearson", "1:2", "dotprod", "1:2",
        ],
        b"1\t2\n3\t6\n",
        b"2\t4\t1\t1\t20\n",
    );
    // Independent asymmetric-moment and exp(-13/27),
    // exp(-28/9) oracles support these decimal output expectations.
    add(
        "moments",
        &["pskew", "1", "sskew", "1", "pkurt", "1", "skurt", "1"],
        b"0\n0\n0\n4\n",
        b"1.1547005383793\t2\t-0.66666666666667\t4\n",
    );
    add(
        "normality",
        &["jarque", "1", "dpo", "1"],
        b"-1\n1\n-1\n1\n",
        b"0.71653131057379\t0.072439757034251\n",
    );
    // Independent Taylor expansion for JB=26/27 and GNU DP=56/9.
    let exp = |x: f64| {
        let (mut sum, mut term) = (1.0, 1.0);
        for n in 1..80 {
            term *= x / f64::from(n);
            sum += term;
        }
        sum
    };
    add(
        "asymmetric-normality",
        &["--format=%.10f", "jarque", "1", "dpo", "1"],
        b"0\n0\n0\n4\n",
        format!("{:.10}\t{:.10}\n", exp(-13.0 / 27.0), exp(-28.0 / 9.0)).as_bytes(),
    );
    // The reviewed binary64 MAD scale, not an invented exact decimal scale.
    add(
        "robust",
        &["mode", "1", "antimode", "1", "madraw", "1", "mad", "1"],
        b"0\n1\n2\n",
        b"0\t0\t1\t1.4826\n",
    );
    add(
        "text",
        &[
            "first",
            "1",
            "last",
            "1",
            "unique",
            "1",
            "countunique",
            "1",
            "collapse",
            "1",
        ],
        b"b\na\nb\n",
        b"b\tb\ta,b\t2\tb,a,b\n",
    );
    add(
        "rand-identical-values",
        &["rand", "1"],
        b"same\nsame\n",
        b"same\n",
    );
    // Fixed seed-zero vectors independently recorded in rand-reference:
    // first four draws select row 3; next four select row 4 (no group reseed).
    add(
        "rand-seeded-groups",
        &["-S0", "-g1", "rand", "2"],
        b"a\t1\na\t2\na\t3\na\t4\nb\t1\nb\t2\nb\t3\nb\t4\n",
        b"a\t3\nb\t4\n",
    );
    add(
        "rand-safe-long-seed",
        &["--seed=7", "rand", "1"],
        b"a\nb\nc\nd\n",
        b"a\n",
    );
    add(
        "line",
        &[
            "round", "1", "floor", "1", "ceil", "1", "trunc", "1", "frac", "1",
        ],
        b"-0.5\n2.5\n",
        b"-1\t-1\t0\t0\t-0.5\n3\t2\t3\t2\t0.5\n",
    );
    add(
        "extract",
        &[
            "getnum:n", "1", "getnum:i", "1", "getnum:h", "1", "getnum:o", "1", "getnum:p", "1",
            "getnum:d", "1",
        ],
        b"12\n",
        b"12\t12\t18\t10\t12\t12\n",
    );
    add(
        "bin",
        &["bin:10", "1", "strbin:1", "1"],
        b"-1\n11\n",
        b"-10\t0\n10\t0\n",
    );
    add("cut", &["cut", "2,1"], b"a\tb\nc\td\n", b"b\ta\nd\tc\n");
    add(
        "path-fields",
        &[
            "dirname", "1", "basename", "1", "extname", "1", "barename", "1",
        ],
        b"//a/b.tar.gz\na\\b.txt\n\xff/x.y\n",
        b"//a\tb.tar.gz\ttar.gz\tb\n.\ta\\b.txt\ttxt\ta\\b\n\xff\tx.y\ty\tx\n",
    );
    add(
        "path-empty-nul",
        &["-t|", "dirname", "1", "basename", "1", "barename", "1"],
        b"|x\n\0a|x\n",
        b".||\n.|.|.\n",
    );
    for (op, digest) in ["md5", "sha1", "sha224", "sha256", "sha384", "sha512"]
        .into_iter()
        .zip(super::checksum_jobs::ABC)
    {
        add(op, &[op, "1"], b"abc\n", format!("{digest}\n").as_bytes());
    }
    add(
        "base64-bytes",
        &["base64", "1"],
        b"foo\na\x00b\n\xff\n",
        b"Zm9v\nYQBi\n/w==\n",
    );
    add(
        "debase64-text",
        &["debase64", "1"],
        b"Zm9v\nYQBi\n/w==\n",
        b"foo\na\n\xff\n",
    );
    add("nop", &["-f", "nop"], b"a\x00b\t\xff\n", b"a\x00b\t\xff\n");
    add("noop", &["noop"], b"a\x00b\t\xff\n", b"");
    add(
        "transpose",
        &["transpose"],
        b"a\tb\nc\td\n",
        b"a\tc\nb\td\n",
    );
    add("reverse", &["reverse"], b"a\tb\nc\td\n", b"b\ta\nd\tc\n");
    add(
        "groups",
        &["-sg1", "sum", "2"],
        b"b\t2\na\t1\na\t3\n",
        b"a\t4\nb\t2\n",
    );
    // The short-byte hash is (97*512+98)*512+99, modulo 11 = 10.
    add(
        "strbin-assignment",
        &["strbin:11", "1"],
        b"abc\na\x00b\n",
        b"10\n4\n",
    );
    add(
        "dedup",
        &["dedup", "1"],
        b"a\t1\na\t2\nb\t3\n",
        b"a\t1\nb\t3\n",
    );
    add(
        "rmdup",
        &["rmdup", "1"],
        b"a\t1\na\t2\nb\t3\n",
        b"a\t1\nb\t3\n",
    );
    add(
        "check",
        &["check", "2", "fields"],
        b"a\tb\nc\td\n",
        b"2 lines, 2 fields\n",
    );
    add("crosstab", &["ct", "1,2"], b"a\tx\na\tx\n", b"\tx\na\t2\n");
    add(
        "headers-narm-nul",
        &["-zH", "--narm", "sum", "value"],
        b"value\x00NA\x001\x002\x00",
        b"sum(value)\x003\x00",
    );
    // Exactly representable binary80 integers straddling binary64 precision.
    add(
        "decimal-cancellation",
        &["sum", "1"],
        b"9007199254740993\n-9007199254740992\n",
        b"1\n",
    );
    add(
        "decimal-midpoint",
        &["--format=%.20g", "sum", "1"],
        b"1.0000000000000000001\n-1\n",
        b"1.084202172485504434e-19\n",
    );
    add(
        "range",
        &["--format=%a", "sum", "1"],
        b"0x1p-16445\n",
        b"0x0.000000000000001p-16385\n",
    );
    // Exact decimal certificate, distinct from rejected inexact tiny decimals.
    add(
        "decimal-subnormal",
        &["--format=%a", "sum", "1"],
        format!("{}\n", fixtures::dyadic(1, 16445)).as_bytes(),
        b"0x0.000000000000001p-16385\n",
    );
    add(
        "large-exponent",
        &["--format=%a", "sum", "1"],
        b"0x1p16380\n",
        b"0x8p+16377\n",
    );
    add(
        "nan-policy",
        &["round", "1"],
        b"-nan\nnan\n-0\n",
        b"-nan\nnan\n0\n",
    );
    cases
}

#[test]
fn cross_environment_corpus() {
    let evidence = std::env::var_os("FASTMASH_REPRO_EVIDENCE").map(std::path::PathBuf::from);
    if let Some(path) = &evidence {
        fs::create_dir(path).expect("evidence directory must be fresh");
    }
    let mut manifest = String::new();
    let mut count = 0;
    // Mixed categories exercise precedence without changing numeric punctuation.
    let settings: &[&[(&str, &str)]] = &[
        &[("LC_ALL", "C")],
        &[("LC_ALL", "POSIX")],
        &[("LC_ALL", "C.UTF-8")],
        &[("LC_ALL", "en_US.UTF-8")],
        &[("LANG", "de_DE.UTF-8"), ("LC_NUMERIC", "C")],
        &[
            ("LANG", "de_DE.UTF-8"),
            ("LC_NUMERIC", "de_DE.UTF-8"),
            ("LC_ALL", "C"),
        ],
    ];
    let mut check = |name: &str,
                     a: &[OsString],
                     input: &[u8],
                     expected: &[u8],
                     error: &[u8],
                     code: i32,
                     env: &[(&str, &str)],
                     spill| {
        let output = super::locale_delivery::run(&candidate(), a, input, env, spill);
        let id = format!("{count:04}-{name}");
        let status = format!(
            "code={:?};signal={:?}\n",
            output.status.code(),
            output.status.signal()
        );
        if let Some(path) = &evidence {
            let dir = path.join(&id);
            fs::create_dir(&dir).unwrap();
            for (file, bytes) in [
                ("input", input),
                ("stdout", output.stdout.as_slice()),
                ("stderr", output.stderr.as_slice()),
                ("expected-stdout", expected),
                ("expected-stderr", error),
            ] {
                fs::write(dir.join(file), bytes).unwrap();
            }
            fs::write(dir.join("settings"), format!("argv0=fastmash\nargs={a:?}\nbase=LANG=C;PATH=/usr/bin:/bin;LANGUAGE=C\noverrides={env:?}\nFASTMASH_SORT_MEMORY_BYTES={}\nLOCPATH={:?}\n", if spill { "1" } else { "unset" }, std::env::var_os("FASTMASH_RECORD_LOCALE_PATH"))).unwrap();
            fs::write(dir.join("status"), &status).unwrap();
            fs::write(
                dir.join("expected-status"),
                format!("code=Some({code});signal=None\n"),
            )
            .unwrap();
        }
        assert_eq!(output.status.code(), Some(code), "{id}: {output:?}");
        assert_eq!(output.stdout, expected, "{id}");
        assert_eq!(output.stderr, error, "{id}");
        manifest.push_str(&format!("{id}\n"));
        count += 1;
    };
    let cases = corpus();
    for env in settings {
        for case in &cases {
            check(
                case.name,
                &case.arguments,
                &case.input,
                &case.stdout,
                b"",
                0,
                env,
                false,
            );
            if case.name == "groups" {
                check(
                    case.name,
                    &case.arguments,
                    &case.input,
                    &case.stdout,
                    b"",
                    0,
                    env,
                    true,
                );
            }
        }
        // GNU's ERANGE rejection is intentional, even though rounding to a
        // subnormal would be possible (numeric-input-design, field-ops.c:386).
        check(
            "decimal-underflow",
            &args(&["sum", "1"]),
            b"10e-4951\n",
            b"",
            b"fastmash: invalid numeric value in line 1 field 1: '10e-4951'\n",
            1,
            env,
            false,
        );
    }
    for locale in ["en_US.UTF-8", "de_DE.UTF-8"] {
        for spill in [false, true] {
            // Approved ICU ordering and raw identity ties, from the policy tests.
            check(
                "language-order",
                &args(&["-sg1", "collapse", "2"]),
                "é\t1\ne\u{301}\t2\né\t3\n".as_bytes(),
                "e\u{301}\t2\né\t1,3\n".as_bytes(),
                b"",
                0,
                &[("LC_ALL", locale)],
                spill,
            );
        }
    }
    check(
        "german-numeric",
        &args(&["sum", "1"]),
        b"1,25\n2,5\n",
        b"3,75\n",
        b"",
        0,
        &[("LC_ALL", "de_DE.UTF-8")],
        false,
    );
    check(
        "deterministic-failure",
        &args(&["getnum:1", "1"]),
        b"12\n",
        b"",
        b"fastmash: getnum requires a character type parameter\n",
        1,
        &[("LC_ALL", "C")],
        false,
    );
    if let Some(path) = evidence {
        fs::write(path.join("COMPLETE"), manifest).unwrap();
    }
    eprintln!("{count} reproducibility cases passed");
}
