//! Installed-command coverage for GNU option scanning, independent of operation grammar.
use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

fn candidate() -> OsString {
    std::env::var_os("FASTMASH_INVOCATION_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into())
}

fn invoke(binary: &std::ffi::OsStr, args: &[OsString], posix: bool) -> Output {
    invoke_named(binary, args, posix, "fastmash")
}

fn invoke_named(binary: &std::ffi::OsStr, args: &[OsString], posix: bool, name: &str) -> Output {
    let mut command = Command::new(binary);
    command
        .arg0(name)
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", "UTC")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if posix {
        command.env("POSIXLY_CORRECT", "");
    }
    let mut child = command.spawn().unwrap();
    // Small enough to fit in a pipe even for an early action that never reads.
    let _ = child.stdin.take().unwrap().write_all(b"a\t2\nb\t3\n");
    child.wait_with_output().unwrap()
}

fn arguments(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

#[test]
fn help_and_version_are_incremental_and_product_specific() {
    let binary = candidate();
    for (long, short, prefix) in [("--help", "-h", "--hel"), ("--version", "-V", "--vers")] {
        let expected = invoke(&binary, &arguments(&[long]), false);
        assert!(expected.status.success());
        assert!(!expected.stdout.is_empty());
        for args in [
            vec![short],
            vec![prefix],
            vec![short, "--bad"],
            vec!["count", "1", short],
        ] {
            let result = invoke(&binary, &arguments(&args), false);
            assert_eq!(result.stdout, expected.stdout, "{args:?}");
            assert_eq!(result.stderr, expected.stderr, "{args:?}");
            assert_eq!(result.status, expected.status, "{args:?}");
        }
        let result = invoke(&binary, &arguments(&[short]), true);
        assert_eq!(result.stdout, expected.stdout);
        assert!(result.status.success());
    }
    let help = invoke(&binary, &arguments(&["--help"]), false);
    assert_eq!(
        invoke(&binary, &arguments(&["-hV"]), false).stdout,
        help.stdout
    );
    let version = invoke(&binary, &arguments(&["--version"]), false);
    assert_eq!(
        invoke(&binary, &arguments(&["-Vh"]), false).stdout,
        version.stdout
    );
}

#[test]
fn useful_argument_sizes_have_no_private_raw_cap() {
    let mut args = vec![OsString::from("-W"); 10_000];
    args.extend(arguments(&["count", "1"]));
    let output = invoke(&candidate(), &args, false);
    assert!(output.status.success(), "{:?}", output);
    assert_eq!(output.stdout, b"2\n");
    let output = invoke(
        &candidate(),
        &arguments(&["--help", &"x".repeat(20_000)]),
        false,
    );
    assert!(output.status.success());
}

#[test]
fn information_output_failure_is_an_error() {
    for arg in ["-h", "-V"] {
        let output = Command::new(candidate())
            .arg(arg)
            .env_clear()
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(
                fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .unwrap(),
            )
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&output.stderr).contains("No space left on device"));
    }
}

#[test]
#[ignore = "requires the named GNU reference and explicit evidence directory"]
fn named_gnu_option_observations() {
    use std::os::unix::ffi::OsStringExt;
    let reference = std::env::var_os("FASTMASH_INVOCATION_REFERENCE").unwrap();
    let evidence =
        std::path::PathBuf::from(std::env::var_os("FASTMASH_INVOCATION_EVIDENCE").unwrap());
    fs::create_dir(&evidence).unwrap();
    let cases: &[&[&str]] = &[
        &[],
        &["--"],
        &["count", "1"],
        &["--n", "count", "1"],
        &["--hea"],
        &["--he=1"],
        &["--h"],
        &["--s"],
        &["--so"],
        &["--f"],
        &["--fi"],
        &["--xyz"],
        &["--xyz=abc"],
        &["-x"],
        &["-Wx"],
        &["-xh"],
        &["--help=yes"],
        &["--vers=yes"],
        &["--header-in=yes"],
        &["--sort=yes"],
        &["--full=yes"],
        &["--format"],
        &["--filler"],
        &["--round"],
        &["--sort-cmd"],
        &["--field-separator"],
        &["--fie"],
        &["--out"],
        &["--col"],
        &["--g"],
        &["-t"],
        &["-Wt"],
        &["-g"],
        &["-c"],
        &["-S"],
        &["-F"],
        &["-R"],
        &["-t", "--help"],
        &["--fie", "--"],
        &["--fie="],
        &["--g="],
        &["count", "1", "-W"],
        &["count", "-W", "1"],
        &["-W", "count", "1"],
        &["count", "1", "--bad"],
        &["count", "1", "--", "-W"],
        &["--", "count", "1"],
        &["--", "--help"],
        &["-", "--bad"],
        &["", "--bad"],
        &["--narm", "count", "1"],
        &["--nar", "count", "1"],
        &["-WC", "count", "1"],
        &["-CWt,", "count", "1"],
        &["--fie=,", "--out=;", "-W", "count", "1"],
        &["--head", "count", "1"],
        &["--header-i", "count", "1"],
        &["--headers", "count", "1"],
        &["--header-o", "count", "1"],
        &["--sort", "count", "1"],
        &["-S7", "count", "1"],
        &["--bad", "--help"],
        &["-txy", "-h"],
        &["-t=", "count", "1"],
    ];
    let mut failures = Vec::new();
    for (index, args) in cases.iter().enumerate() {
        for posix in [false, true] {
            observe(
                &reference,
                &evidence,
                &mut failures,
                index,
                &arguments(args),
                posix,
            );
        }
    }
    for (offset, bytes) in [b"--\xff".as_slice(), b"-\xff", b"--hea=\xff"]
        .iter()
        .enumerate()
    {
        observe(
            &reference,
            &evidence,
            &mut failures,
            cases.len() + offset,
            &[OsString::from_vec(bytes.to_vec())],
            false,
        );
    }
    for name in ["dir/tool", "/opt/fastmash/bin/tool", ""] {
        let args = arguments(&["--bad"]);
        let expected = invoke_named(&reference, &args, false, name);
        let actual = invoke_named(&candidate(), &args, false, name);
        assert_eq!(actual.status, expected.status, "argv0={name:?}");
        assert_eq!(actual.stderr, expected.stderr, "argv0={name:?}");
        let label = if name.is_empty() {
            "empty"
        } else if name.starts_with('/') {
            "absolute"
        } else {
            "relative"
        };
        fs::write(evidence.join(format!("argv0-{label}.name")), name).unwrap();
        for (product, output) in [("gnu", &expected), ("candidate", &actual)] {
            fs::write(
                evidence.join(format!("argv0-{label}.{product}.stderr")),
                &output.stderr,
            )
            .unwrap();
            fs::write(
                evidence.join(format!("argv0-{label}.{product}.status")),
                output.status.to_string(),
            )
            .unwrap();
        }
    }
    assert!(
        failures.is_empty(),
        "{} mismatches: {}",
        failures.len(),
        failures.join(", ")
    );
}

fn observe(
    reference: &std::ffi::OsStr,
    evidence: &std::path::Path,
    failures: &mut Vec<String>,
    index: usize,
    args: &[OsString],
    posix: bool,
) {
    let id = format!("{index:03}-{}", if posix { "posix" } else { "default" });
    fs::write(
        evidence.join(format!("{id}.args")),
        format!("{args:?}\nPOSIXLY_CORRECT={posix}\n"),
    )
    .unwrap();
    let expected = invoke(reference, args, posix);
    let actual = invoke(&candidate(), args, posix);
    for (label, output) in [("gnu", &expected), ("candidate", &actual)] {
        fs::write(
            evidence.join(format!("{id}.{label}.stdout")),
            &output.stdout,
        )
        .unwrap();
        fs::write(
            evidence.join(format!("{id}.{label}.stderr")),
            &output.stderr,
        )
        .unwrap();
        fs::write(
            evidence.join(format!("{id}.{label}.status")),
            output.status.to_string(),
        )
        .unwrap();
    }
    if expected.status != actual.status
        || expected.stdout != actual.stdout
        || expected.stderr != actual.stderr
    {
        failures.push(id);
    }
}
