use std::{
    fs,
    process::{Command, Output, Stdio},
};

fn invoke(binary: &std::ffi::OsStr, args: &[&str], input: &[u8], memory: &str) -> Output {
    invoke_at(binary, args, input, memory, None, false)
}

fn invoke_at(
    binary: &std::ffi::OsStr,
    args: &[&str],
    input: &[u8],
    memory: &str,
    temporary: Option<&std::path::Path>,
    full: bool,
) -> Output {
    let path = std::env::temp_dir().join(format!(
        "grouping-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let mut command = Command::new("/usr/bin/timeout");
    command
        .args(["--kill-after=2s", "30s"])
        .arg(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .env("FASTMASH_SORT_MEMORY_BYTES", memory)
        .stdin(fs::File::open(&path).unwrap())
        .stdout(if full {
            Stdio::from(fs::File::options().write(true).open("/dev/full").unwrap())
        } else {
            Stdio::piped()
        })
        .stderr(Stdio::piped());
    if let Some(temporary) = temporary {
        command.env("TMPDIR", temporary);
    }
    let result = command.output().unwrap();
    fs::remove_file(path).unwrap();
    result
}
fn candidate() -> std::ffi::OsString {
    std::env::var_os("FASTMASH_GROUP_TEST_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into())
}

#[test]
fn native_numerical_groups_share_state_and_preserve_ties() {
    let args = [
        "--header-in",
        "-s",
        "-g",
        "gene",
        "count",
        "value",
        "sum",
        "2",
        "mean",
        "value",
        "min",
        "2",
        "max",
        "2",
        "median",
        "2",
        "q1",
        "value",
        "q3",
        "2",
        "iqr",
        "2",
        "sum",
        "3",
        "first",
        "4",
        "last",
        "4",
    ];
    let input = b"gene\tvalue\tother\tid\nb\t5\t40\tb1\na\t3\t20\ta1\nb\t1\t10\tb2\na\t1\t30\ta2\n";
    for memory in ["67108864", "1"] {
        let out = invoke(&candidate(), &args, input, memory);
        assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
        assert_eq!(out.stdout, b"a\t2\t4\t2\t1\t3\t2\t1.5\t2.5\t1\t50\ta1\ta2\nb\t2\t6\t3\t1\t5\t3\t2\t4\t2\t50\tb1\tb2\n");
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn numerical_groups_accept_wide_records_and_clear_missing_groups() {
    let input = format!(
        "b\tNA\t{}\na\t3\t{}\na\t1\t{}\n",
        "x".repeat(20000),
        "y".repeat(20000),
        "z".repeat(20000)
    );
    for sort in [false, true] {
        let mut args = vec![
            "--narm", "-g", "1", "count", "2", "min", "2", "max", "2", "mean", "2", "median", "2",
        ];
        if sort {
            args.insert(0, "-s");
        }
        let out = invoke(&candidate(), &args, input.as_bytes(), "1024");
        assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
        let a = "a\t2\t1\t3\t2\t2\n";
        let b = "b\t0\t-inf\tinf\tnan\tnan\n";
        assert_eq!(
            out.stdout,
            if sort {
                format!("{a}{b}")
            } else {
                format!("{b}{a}")
            }
            .as_bytes()
        );
    }
}

#[test]
fn native_numerical_spill_and_write_failures_are_reported() {
    let temporary =
        std::env::temp_dir().join(format!("numerical-unusable-temp-{}", std::process::id()));
    fs::write(&temporary, b"not a directory").unwrap();
    let args = ["-s", "-g", "1", "mean", "2", "median", "2"];
    let failed = invoke_at(&candidate(), &args, b"a\t1\n", "1", Some(&temporary), false);
    assert_eq!(failed.status.code(), Some(1));
    assert!(failed.stdout.is_empty());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("sort temporary I/O error"));
    let memory = invoke_at(
        &candidate(),
        &args,
        b"a\t1\n",
        "67108864",
        Some(&temporary),
        false,
    );
    assert_eq!(memory.status.code(), Some(0));
    assert_eq!(memory.stdout, b"a\t1\t1\n");
    fs::remove_file(temporary).unwrap();
    let output = invoke_at(&candidate(), &args, b"a\t1\na\t3\n", "1", None, true);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("No space left on device"));
}

#[test]
fn numerical_backend_accepts_more_than_32_keys() {
    let row = format!("{}1\n", "k\t".repeat(40));
    let out = invoke(
        &candidate(),
        &["-s", "-g", "1-40", "min", "41"],
        row.as_bytes(),
        "1024",
    );
    assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
    assert_eq!(out.stdout, row.as_bytes());
}

#[test]
fn grouping_accepts_more_than_32_keys() {
    let row = format!("{}value\n", "k\t".repeat(40));
    for args in [
        vec!["-g", "1-40", "count", "41"],
        vec!["-s", "-g", "1-40", "count", "41"],
    ] {
        let out = invoke(&candidate(), &args, row.repeat(2).as_bytes(), "1024");
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(out.stdout, format!("{}2\n", "k\t".repeat(40)).as_bytes());
    }
}

#[test]
fn adjacent_and_stable_sorted_groups_have_distinct_meanings() {
    let input = b"b\tx\na\ty\nb\tz\n";
    for (args, expected) in [
        (
            vec!["-g", "1", "count", "2", "collapse", "2"],
            b"b\t1\tx\na\t1\ty\nb\t1\tz\n".as_slice(),
        ),
        (
            vec![
                "-s", "-g", "1", "count", "2", "first", "2", "last", "2", "collapse", "2",
            ],
            b"a\t1\ty\ty\ty\nb\t2\tx\tz\tx,z\n".as_slice(),
        ),
    ] {
        for memory in ["67108864", "1"] {
            let out = invoke(&candidate(), &args, input, memory);
            assert_eq!(out.status.code(), Some(0), "{:?}", out.stderr);
            assert_eq!(out.stdout, expected);
            assert!(out.stderr.is_empty());
        }
    }
}

#[test]
#[ignore = "requires named GNU 1.9 reference and installed candidate"]
fn projected_boundaries_match_gnu() {
    let gnu = std::env::var_os("FASTMASH_GNU_REFERENCE").expect("named GNU reference");
    let evidence = std::env::var_os("FASTMASH_GROUP_EVIDENCE").map(std::path::PathBuf::from);
    if let Some(path) = &evidence {
        fs::create_dir(path).unwrap();
    }
    let long_retry = format!("{}e0ea\n", "1".repeat(512));
    let cases: Vec<(Vec<&str>, &[u8])> = vec![
        (
            vec![
                "-s", "-g", "1", "sum", "2", "mean", "2", "min", "2", "max", "2", "median", "2",
                "q1", "2", "q3", "2", "iqr", "2", "first", "3", "last", "3",
            ],
            b"b\t4\tu\na\t3\tx\nb\t2\tv\na\t1\ty\n",
        ),
        (
            vec![
                "-H", "-s", "-g", "gene", "mean", "value", "sum", "2", "median", "2", "q1", "value",
            ],
            b"gene\tvalue\nb\t5\na\t3\na\t1\n",
        ),
        (
            vec!["--header-out", "-s", "-g", "1", "median", "2", "mean", "3"],
            b"b\t5\t10\na\t1\t20\n",
        ),
        (
            vec![
                "--narm", "-s", "-g", "1", "min", "2", "max", "2", "mean", "2", "median", "2",
            ],
            b"b\tNA\na\t3\na\t1\nb\tNaN\n",
        ),
        (
            vec!["-s", "-g", "1", "min", "2", "max", "2", "median", "2"],
            b"a\t-0\na\t0\nb\t0\nb\t-0\nc\tnan\nc\t1\nd\t1\nd\tnan\n",
        ),
        (
            vec!["-s", "-g", "1", "sum", "2", "median", "2"],
            b"z\tbad\na\t1\n",
        ),
        (
            vec!["-s", "-g", "1", "mean", "2", "median", "3"],
            b"z\t2\na\t1\t3\n",
        ),
        (vec!["-s", "-g", "1", "median", "2"], b"z\na\t1\n"),
        (
            vec!["-s", "-g", "1", "median", "2"],
            b"a\0y\t3\na\0x\t1\nb\t2\n",
        ),
        (
            vec!["-W", "-s", "-g", "1", "median", "2", "sum", "2"],
            b"b 4\n a 3\na 1\nb 2\n",
        ),
        (
            vec!["-t", "e", "-s", "-g", "3", "median", "1"],
            b"1e5000ea\n",
        ),
        (
            vec!["-t", "e", "-s", "-g", "3", "min", "1", "max", "1"],
            b"1e5000ea\n",
        ),
        (
            vec!["-t", "e", "-s", "-g", "3", "mean", "1", "median", "1"],
            b"1e2eb\n3e0ea\n",
        ),
        (
            vec!["-t", "e", "-s", "-g", "3", "median", "1"],
            long_retry.as_bytes(),
        ),
        (
            vec!["-C", "-s", "-g", "1", "mean", "2", "median", "2"],
            b"#x\nb\t4\na\t1\n",
        ),
        (vec!["-s", "-g", "1", "median", "2"], b""),
        (vec!["-H", "-s", "-g", "1", "median", "2"], b"key\tvalue\n"),
        (
            vec![
                "-s", "-g", "1,2", "first", "3", "last", "3", "collapse", "3",
            ],
            b"b\tx\t1\na\tz\t2\nb\tx\t3\na\ta\t4\n",
        ),
        (vec!["-s", "-g", "1,1", "count", "2"], b"b\tx\na\ty\nb\tz\n"),
        (
            vec!["-s", "-g", "1", "first", "2", "last", "2", "collapse", "2"],
            b"a\0y\ty\na\0x\tx\nb\tz\n",
        ),
        (vec!["-s", "-g", "1", "count", "2"], b"\xff\tx\n\ty\na\tz\n"),
        (
            vec!["-W", "-s", "-g", "1", "collapse", "2"],
            b"b x\n a y\na z\n\ta t\n  b u\n",
        ),
        (
            vec!["-W", "-s", "-g", "2", "collapse", "1"],
            b"x a\ny  a\nz\ta\nt a \nu \n",
        ),
        (vec!["-s", "-g", "1", "collapse", "2"], b"z\na\tx\n"),
        (vec!["-s", "-g", "2", "collapse", "1"], b"z\nx\ta\n"),
        (
            vec!["--header-out", "-s", "-g", "1", "count", "2"],
            b"z\na\tx\n",
        ),
        (
            vec!["--header-out", "-s", "-g", "1", "count", "3"],
            b"z\tx\na\ty\n",
        ),
        (
            vec!["-H", "-s", "-g", "gene", "first", "value", "last", "value"],
            b"gene\tvalue\nb\tx\na\ty\nb\tz\n",
        ),
        (
            vec!["-C", "-s", "-g", "1", "count", "2"],
            b"#x\nb\tx\n ;x\na\ty\n",
        ),
        (vec!["-s", "-g", "1", "count", "2"], b""),
        (vec!["-H", "-s", "-g", "1", "count", "2"], b"key\tvalue\n"),
        (vec!["-s", "-g", "1", "count", "2"], b"\n"),
        (
            vec![
                "--narm", "-s", "-g", "1", "first", "2", "last", "2", "unique", "2",
            ],
            b"a\tNA\na\tN/A\nb\tx\n",
        ),
        (
            vec!["--narm", "-s", "-g", "1", "count", "2", "countunique", "2"],
            b"a\tNA\na\tN/A\nb\tx\nb\tx\nb\ty\n",
        ),
    ];
    for (at, (args, input)) in cases.iter().enumerate() {
        let reference = invoke(&gnu, args, input, "67108864");
        if let Some(path) = &evidence {
            fs::write(path.join(format!("{at}.input")), input).unwrap();
            fs::write(path.join(format!("{at}.argv")), args.join("\n")).unwrap();
            retain(path, &format!("{at}-gnu"), &reference);
        }
        for memory in ["67108864", "1"] {
            let out = invoke(&candidate(), args, input, memory);
            if let Some(path) = &evidence {
                retain(path, &format!("{at}-candidate-{memory}"), &out);
            }
            assert_eq!(
                out.status.code(),
                reference.status.code(),
                "case {at} memory {memory}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, reference.stdout, "case {at} memory {memory}");
            let diagnostic = |bytes: &[u8]| {
                bytes
                    .splitn(2, |&b| b == b':')
                    .nth(1)
                    .unwrap_or(bytes)
                    .to_vec()
            };
            assert_eq!(
                diagnostic(&out.stderr),
                diagnostic(&reference.stderr),
                "case {at} memory {memory}"
            );
        }
    }
}

fn retain(path: &std::path::Path, name: &str, output: &Output) {
    fs::write(path.join(format!("{name}.stdout")), &output.stdout).unwrap();
    fs::write(path.join(format!("{name}.stderr")), &output.stderr).unwrap();
    fs::write(
        path.join(format!("{name}.status")),
        format!("{:?}\n", output.status.code()),
    )
    .unwrap();
}

#[test]
fn sort_memory_setting_is_active_only_for_native_sorting() {
    for memory in ["0", "-1", "invalid", "18446744073709551616"] {
        let out = invoke(
            &candidate(),
            &["-s", "-g", "1", "count", "2"],
            b"a\tx\n",
            memory,
        );
        assert_eq!(out.status.code(), Some(77));
        assert!(String::from_utf8_lossy(&out.stderr).contains("positive byte count"));
        let out = invoke(&candidate(), &["-g", "1", "count", "2"], b"a\tx\n", memory);
        assert_eq!(out.status.code(), Some(0));
    }
}

#[test]
fn native_spill_and_output_failures_are_explicit() {
    let path = std::env::temp_dir().join(format!("grouping-unusable-temp-{}", std::process::id()));
    fs::write(&path, b"not a directory").unwrap();
    let out = invoke_at(
        &candidate(),
        &["-s", "-g", "1", "collapse", "2"],
        b"a\tx\n",
        "1",
        Some(&path),
        false,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("sort temporary I/O error"));
    // An unusable temporary directory is irrelevant when the input fits memory.
    let out = invoke_at(
        &candidate(),
        &["-s", "-g", "1", "collapse", "2"],
        b"a\tx\n",
        "67108864",
        Some(&path),
        false,
    );
    assert_eq!(out.status.code(), Some(0));
    fs::remove_file(path).unwrap();
    let out = invoke_at(
        &candidate(),
        &["-s", "-g", "1", "collapse", "2"],
        &b"a\ttranscript\n".repeat(70_000),
        "1048576",
        None,
        true,
    );
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("No space left on device"));
}

#[test]
fn native_variable_allocation_refusal_is_explicit() {
    let binary = candidate();
    let out = invoke(
        std::ffi::OsStr::new("/usr/bin/prlimit"),
        &[
            "--as=25165824",
            "--",
            binary.to_str().unwrap(),
            "-s",
            "-g",
            "1",
            "count",
            "2",
        ],
        &[b"a\t".as_slice(), &vec![b'x'; 12 * 1024 * 1024], b"\n"].concat(),
        "67108864",
    );
    assert_eq!(out.status.code(), Some(77), "{:?}", out.stderr);
    assert!(String::from_utf8_lossy(&out.stderr).contains("allocation"));
}

#[test]
fn spill_write_failure_does_not_leave_named_files() {
    use std::os::unix::process::ExitStatusExt;
    let path = std::env::temp_dir().join(format!("grouping-file-limit-{}", std::process::id()));
    fs::create_dir(&path).unwrap();
    let binary = candidate();
    let out = invoke_at(
        std::ffi::OsStr::new("/usr/bin/prlimit"),
        &[
            "--fsize=8192",
            "--",
            binary.to_str().unwrap(),
            "-s",
            "-g",
            "1",
            "count",
            "2",
        ],
        &b"a\ttranscript\n".repeat(10_000),
        "1024",
        Some(&path),
        false,
    );
    assert!(
        out.status.signal() == Some(25) || out.status.code() == Some(153),
        "{:?}",
        out
    );
    assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    fs::remove_dir(path).unwrap();
}
