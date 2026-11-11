use super::*;

#[test]
fn parser_errors_preserve_scope_and_precedence() {
    let cases: &[(&[&str], i32, &str)] = &[
        (
            &[""],
            1,
            "missing operation specifiers\nhint: choose an operation and field, for example fastmash sum 1",
        ),
        (
            &["strbin:2.5", "1"],
            1,
            "strbin requires an integer bucket count\nhint: use a positive integer bucket count, for example strbin:10 1",
        ),
        (
            &["strbin:2.0", "1"],
            1,
            "strbin requires an integer bucket count\nhint: use a positive integer bucket count, for example strbin:10 1",
        ),
        (
            &["getnum:1", "1"],
            1,
            "getnum requires a character type parameter\nhint: getnum types are n, i, h, o, p and d. For example, getnum:i 1 extracts a signed integer",
        ),
        (
            &["getnum:1.5", "1"],
            1,
            "getnum requires a character type parameter\nhint: getnum types are n, i, h, o, p and d. For example, getnum:i 1 extracts a signed integer",
        ),
        (
            &["pcov", "1:2-3"],
            1,
            "paired field ranges are invalid\nhint: write pairs as LEFT:RIGHT, for example 1:2 or 1:2,3:4. Pairs cannot contain ranges",
        ),
        (
            &["-H", "pcov", "left:right-3"],
            1,
            "paired field ranges are invalid\nhint: write pairs as LEFT:RIGHT, for example 1:2 or 1:2,3:4. Pairs cannot contain ranges",
        ),
        (
            &["dotprod", "1:2-3"],
            1,
            "paired field ranges are invalid\nhint: write pairs as LEFT:RIGHT, for example 1:2 or 1:2,3:4. Pairs cannot contain ranges",
        ),
        (
            &["mean", "1:2-3"],
            1,
            "paired field ranges are invalid\nhint: write pairs as LEFT:RIGHT, for example 1:2 or 1:2,3:4. Pairs cannot contain ranges",
        ),
        (
            &["strbin:2.5:3", "1"],
            1,
            "too many parameters for operation 'strbin'",
        ),
        (
            &["getnum:1:2", "1"],
            1,
            "too many parameters for operation 'getnum'",
        ),
        (
            &["strbin:2.5", "0"],
            1,
            "invalid field '0' for operation 'strbin'\nhint: field numbers start at 1. Named fields need an input header and --header-in or -H",
        ),
        (
            &["getnum:1", "0"],
            1,
            "invalid field '0' for operation 'getnum'\nhint: field numbers start at 1. Named fields need an input header and --header-in or -H",
        ),
        (
            &["pcov", "0:2-3"],
            1,
            "invalid field '0' for operation 'pcov'\nhint: field numbers start at 1. Named fields need an input header and --header-in or -H",
        ),
        (
            &["pcov", "1:"],
            1,
            "invalid field pair for operation 'pcov'\nhint: write pairs as LEFT:RIGHT, for example 1:2 or 1:2,3:4. Pairs cannot contain ranges",
        ),
        (
            &["strbin:0", "1"],
            1,
            "strbin bucket size must not be zero\nhint: use a positive integer bucket count, for example strbin:10 1",
        ),
        (
            &["getnum:x", "1"],
            1,
            "invalid getnum type 'x'\nhint: getnum types are n, i, h, o, p and d. For example, getnum:i 1 extracts a signed integer",
        ),
        (
            &["--", "--help"],
            1,
            "invalid operation '-'\nhint: use fastmash --help to list operations, for example sum 1 or mean 2",
        ),
        (
            &["", "--bad"],
            1,
            "invalid operation '-'\nhint: use fastmash --help to list operations, for example sum 1 or mean 2",
        ),
        (
            &["perc:2.5", "1"],
            77,
            "non-integer percentile parameters are outside the supported range\nhint: use an integer percentile from 1 through 100, for example perc:95 2",
        ),
        (&["rmdup", "1,2"], 77, "rmdup supports exactly one key"),
    ];
    let evidence = std::env::var_os("FASTMASH_MALFORMED_EVIDENCE").map(std::path::PathBuf::from);
    if let Some(path) = &evidence {
        fs::create_dir(path).unwrap();
    }
    for (index, &(arguments, status, message)) in cases.iter().enumerate() {
        for posix in [false, true] {
            // An attempted input read would fail: syntax must win before execution.
            let mut command = Command::new(candidate());
            command
                .arg0("fastmash")
                .args(arguments)
                .env_clear()
                .env("LC_ALL", "C")
                .env("PATH", "/usr/bin:/bin")
                .stdin(fs::File::open("/").unwrap());
            if posix {
                command.env("POSIXLY_CORRECT", "1");
            }
            let output = command.output().unwrap();
            if let Some(path) = &evidence {
                let stem = path.join(format!("{index:02}-{posix}"));
                fs::write(
                    stem.with_extension("args"),
                    format!("{arguments:?}\nPOSIXLY_CORRECT={posix}\nLC_ALL=C\nstdin=directory\n"),
                )
                .unwrap();
                fs::write(stem.with_extension("stdout"), &output.stdout).unwrap();
                fs::write(stem.with_extension("stderr"), &output.stderr).unwrap();
                fs::write(stem.with_extension("status"), output.status.to_string()).unwrap();
            }
            // Without POSIXLY_CORRECT, --bad is still handled as an option.
            if arguments == ["", "--bad"] && !posix {
                assert_eq!(output.status.code(), Some(1));
                assert!(output.stdout.is_empty());
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("unrecognized option '--bad'")
                );
            } else {
                assert_eq!(
                    output.status.code(),
                    Some(status),
                    "{arguments:?}: {output:?}"
                );
                assert!(output.stdout.is_empty(), "{arguments:?}");
                assert_eq!(
                    output.stderr,
                    format!("fastmash: {message}\n").as_bytes(),
                    "{arguments:?}"
                );
            }
        }
    }
    for arguments in [&[][..], &[" "][..], &["", ""][..]] {
        let output = invoke(&candidate(), &args(arguments), b"", false, false);
        assert_eq!(output.status.code(), Some(1), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty());
    }
    for arguments in [&["sum", "1"][..], &["", "sum", "1"][..]] {
        let output = invoke(&candidate(), &args(arguments), b"1\n2\n", false, false);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"3\n");
        assert!(output.stderr.is_empty());
    }
}
