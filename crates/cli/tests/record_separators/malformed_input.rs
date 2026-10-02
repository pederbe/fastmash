use super::*;

#[test]
fn parser_errors_preserve_scope_and_precedence() {
    let cases: &[(&[&str], i32, &str)] = &[
        (&[""], 1, "missing operation specifiers"),
        (
            &["strbin:2.5", "1"],
            1,
            "strbin requires an integer bucket count",
        ),
        (
            &["strbin:2.0", "1"],
            1,
            "strbin requires an integer bucket count",
        ),
        (
            &["getnum:1", "1"],
            1,
            "getnum requires a character type parameter",
        ),
        (
            &["getnum:1.5", "1"],
            1,
            "getnum requires a character type parameter",
        ),
        (&["pcov", "1:2-3"], 1, "paired field ranges are invalid"),
        (
            &["-H", "pcov", "left:right-3"],
            1,
            "paired field ranges are invalid",
        ),
        (&["dotprod", "1:2-3"], 1, "paired field ranges are invalid"),
        (&["mean", "1:2-3"], 1, "paired field ranges are invalid"),
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
            "invalid field '0' for operation 'strbin'",
        ),
        (
            &["getnum:1", "0"],
            1,
            "invalid field '0' for operation 'getnum'",
        ),
        (
            &["pcov", "0:2-3"],
            1,
            "invalid field '0' for operation 'pcov'",
        ),
        (
            &["pcov", "1:"],
            1,
            "invalid field pair for operation 'pcov'",
        ),
        (&["strbin:0", "1"], 1, "strbin bucket size must not be zero"),
        (&["getnum:x", "1"], 1, "invalid getnum type 'x'"),
        (&["--", "--help"], 1, "invalid operation '-'"),
        (&["", "--bad"], 1, "invalid operation '-'"),
        (
            &["perc:2.5", "1"],
            77,
            "non-integer percentile parameters are outside the supported range",
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
