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
        // These tests are of the sort itself (its memory, spills and
        // routes), which hash grouping would otherwise take over for input
        // from a file.
        .env("FASTMASH_GROUPING", "sort")
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

/// Spilled runs through several merge levels keep every field length (empty,
/// and across the one-, two- and three-byte length boundaries) and the input
/// order of equal keys: the output equals the in-memory sort.
#[test]
fn multi_level_spills_match_the_in_memory_sort() {
    let lengths = [0, 1, 127, 128, 16383, 16384, 20000, 3];
    let mut input = Vec::new();
    let mut x = 12345u64;
    for row in 0..700u64 {
        x = (x * 69069 + 1) % 4294967296;
        let key = (x >> 16) % 23;
        let width = lengths[(x % 8) as usize];
        let text: String = (0..width)
            .map(|at| char::from(b'a' + ((at as u64 + row) % 26) as u8))
            .collect();
        let key = if key == 0 {
            String::new()
        } else {
            format!("k{key}")
        };
        input.extend_from_slice(format!("{key}\t{}\t{text}\t{row}\n", x % 1000).as_bytes());
    }
    let jobs: &[&[&str]] = &[
        // Packed records.
        &["-s", "-g", "1", "count", "2", "collapse", "3", "last", "4"],
        &["-s", "-g", "3,1", "first", "4"],
        // Original records.
        &["-s", "-g", "1", "sum", "2", "first", "3", "last", "4"],
        &["-s", "--full", "-g", "1", "count", "2"],
    ];
    let binary = candidate();
    for args in jobs {
        let memory = invoke(&binary, args, &input, "67108864");
        assert!(memory.status.success(), "{args:?}: {memory:?}");
        // One record per run with "1": about four merge levels.
        for target in ["1", "65536"] {
            let spilled = invoke(&binary, args, &input, target);
            assert!(spilled.status.success(), "{args:?} {target}: {spilled:?}");
            assert_eq!(spilled.stdout, memory.stdout, "{args:?} {target}");
            assert_eq!(spilled.stderr, memory.stderr, "{args:?} {target}");
        }
    }
}

/// The unsorted Group loop and the native sorted one (in memory and spilled,
/// with packed and original records) group already sorted input alike.
#[test]
fn every_sort_route_groups_sorted_input_alike() {
    let mut tab = Vec::new();
    let mut spaced = Vec::new();
    for row in 0..3000u32 {
        let key = row / 7;
        let label = ["x", "y", "z"][(key % 3) as usize];
        tab.extend_from_slice(format!("k{key:04}\t{}\t{label}\n", row % 97).as_bytes());
        spaced.extend_from_slice(format!("k{key:04}  {}   {label}\n", row % 97).as_bytes());
    }
    let jobs: &[(&[&str], &[u8])] = &[
        // Packed records: text operations only.
        (
            &[
                "-g", "1", "count", "2", "first", "3", "last", "3", "unique", "3",
            ],
            &tab,
        ),
        // Original records: numbers and the Full row.
        (&["-g", "1", "sum", "2", "mean", "2", "median", "2"], &tab),
        (&["--full", "-g", "1", "count", "2"], &tab),
        (&["-g", "1,3", "countunique", "2"], &tab),
        (&["-W", "-g", "1", "sum", "2", "collapse", "3"], &spaced),
    ];
    let binary = candidate();
    for (args, input) in jobs {
        let unsorted = invoke(&binary, args, input, "67108864");
        assert!(unsorted.status.success(), "{args:?}: {unsorted:?}");
        let sorted_args: Vec<&str> = ["-s"].into_iter().chain(args.iter().copied()).collect();
        for memory in ["67108864", "4096"] {
            let sorted = invoke(&binary, &sorted_args, input, memory);
            assert_eq!(
                sorted.status.code(),
                unsorted.status.code(),
                "{args:?} {memory}"
            );
            assert_eq!(sorted.stdout, unsorted.stdout, "{args:?} {memory}");
            assert_eq!(sorted.stderr, unsorted.stderr, "{args:?} {memory}");
        }
    }
}

/// Runs `binary` on `input` from a regular file, in the locale and with the
/// sort memory and grouping settings of `environment`.
fn invoke_in(
    binary: &std::ffi::OsStr,
    args: &[&str],
    input: &[u8],
    environment: &[(&str, &str)],
) -> Output {
    let path = std::env::temp_dir().join(format!(
        "grouping-in-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let result = Command::new("/usr/bin/timeout")
        .args(["--kill-after=2s", "30s"])
        .arg(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("TZ", "UTC")
        .envs(environment.iter().copied())
        .stdin(fs::File::open(&path).unwrap())
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    result
}

/// `FASTMASH_GROUPING` is read only by sorted jobs with grouping keys, the
/// only jobs that take a Sort route; the others ignore any value.
#[test]
fn grouping_setting_is_active_only_for_sorted_grouping() {
    let environment = [("LC_ALL", "C"), ("FASTMASH_GROUPING", "invalid")];
    for args in [
        &["-g", "1", "count", "2"][..],
        &["-s", "count", "2"],
        &["sum", "2"],
        &["-s", "transpose"],
    ] {
        let out = invoke_in(&candidate(), args, b"a\t1\n", &environment);
        assert_eq!(out.status.code(), Some(0), "{args:?}");
    }
    let out = invoke_in(
        &candidate(),
        &["-s", "-g", "1", "count", "2"],
        b"a\t1\n",
        &environment,
    );
    assert_eq!(out.status.code(), Some(77));
    assert!(String::from_utf8_lossy(&out.stderr).contains("FASTMASH_GROUPING must be"));
}

/// Hash grouping of sorted Groups, and the sort it restarts into, write what
/// the sort writes, in byte order and in a language locale, whether the
/// classes fit their memory or not.
#[test]
fn hash_grouping_writes_what_the_sort_writes() {
    let mut numbered = Vec::new();
    for row in 0..3000u32 {
        let key = (row * 7919) % 429;
        numbered.extend_from_slice(format!("k{key:04}\t{}\tx{}\n", row % 97, row % 5).as_bytes());
    }
    let mut words = Vec::new();
    let names = [
        "apple",
        "Apple",
        "APPLE",
        "b-1",
        "b_1",
        "éclair",
        "Zebra",
        "zebra",
        "",
        "e\u{301}clair",
        "й",
        "и\u{306}",
    ];
    for row in 0..500usize {
        let name = names[(row * 5) % names.len()];
        let other = names[(row * 3) % names.len()];
        words.extend_from_slice(format!("{name}\t{other}\t{}\n", row % 11).as_bytes());
    }
    let jobs: &[(&[&str], &[u8])] = &[
        (&["-s", "-g", "1", "count", "2", "sum", "2"], &numbered),
        (
            &["-s", "-g", "1", "first", "3", "last", "3", "unique", "3"],
            &numbered,
        ),
        (&["-s", "--full", "-g", "1", "median", "2"], &numbered),
        (&["-s", "--header-out", "-g", "3,1", "mean", "2"], &numbered),
        (
            &["-s", "-i", "-g", "1", "count", "1", "collapse", "2"],
            &words,
        ),
        (&["-s", "-g", "2,1", "sum", "3"], &words),
        (&["-s", "-i", "--full", "-g", "2", "last", "3"], &words),
        (&["-s", "crosstab", "1,2", "sum", "3"], &words),
        // Operations that the system sort serves in the C locales: a restart
        // leaves standard input where it began, for the sort to read.
        (&["-s", "-g", "1", "rms", "2", "geomean", "2"], &numbered),
        (&["-s", "-H", "-g", "1", "rms", "2"], &numbered),
        (
            &["-s", "--header-out", "-g", "3", "harmmean", "2", "ms", "2"],
            &numbered,
        ),
        (
            &["-s", "--full", "-g", "1", "pskew", "2", "jarque", "2"],
            &numbered,
        ),
        (
            &["-s", "-g", "3,1", "pcov", "2:2", "spearson", "2:2"],
            &numbered,
        ),
    ];
    let binary = candidate();
    for locale in ["C", "en_US.UTF-8"] {
        for memory in ["67108864", "4096"] {
            for (args, input) in jobs {
                let environment = |grouping| {
                    [
                        ("LC_ALL", locale),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                        ("FASTMASH_GROUPING", grouping),
                    ]
                };
                let sorted = invoke_in(&binary, args, input, &environment("sort"));
                assert!(sorted.status.success(), "{args:?}: {sorted:?}");
                for grouping in ["hash", "hash:restart=0", "hash:restart=100"] {
                    let hashed = invoke_in(&binary, args, input, &environment(grouping));
                    let case = format!("{args:?} {locale} {memory} {grouping}");
                    assert_eq!(hashed.status.code(), sorted.status.code(), "{case}");
                    assert_eq!(hashed.stdout, sorted.stdout, "{case}");
                    assert_eq!(hashed.stderr, sorted.stderr, "{case}");
                }
            }
        }
    }
    // A key the language sort refuses sends hash grouping back to the sort,
    // which refuses it.
    let invalid = b"a\t1\n\xff\t2\nb\t3\n";
    let args = ["-s", "-g", "1", "sum", "2"];
    let refused = |grouping| {
        invoke_in(
            &binary,
            &args,
            invalid,
            &[("LC_ALL", "en_US.UTF-8"), ("FASTMASH_GROUPING", grouping)],
        )
    };
    let (sorted, hashed) = (refused("sort"), refused("hash"));
    assert_eq!(sorted.status.code(), Some(77), "{sorted:?}");
    assert_eq!(
        (hashed.status.code(), &hashed.stdout, &hashed.stderr),
        (sorted.status.code(), &sorted.stdout, &sorted.stderr)
    );
    // Hash grouping ran, on standard input from a regular file only.
    let args = ["-s", "-g", "1", "count", "2"];
    let traced = [
        ("LC_ALL", "C"),
        ("FASTMASH_GROUPING", "hash"),
        ("FASTMASH_SORT_TRACE", "1"),
    ];
    let hashed = invoke_in(&binary, &args, &numbered, &traced);
    assert_eq!(
        String::from_utf8_lossy(&hashed.stderr),
        "sort route: hash first, then native sort of the selected fields\n\
         hash grouping: 429 Groups of 3000 records\n"
    );
    let mut piped = Command::new(&binary)
        .args(args)
        .env_clear()
        .envs(traced)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(piped.stdin.as_mut().unwrap(), &numbered).unwrap();
    drop(piped.stdin.take());
    let piped = piped.wait_with_output().unwrap();
    assert_eq!(piped.stdout, hashed.stdout);
    assert!(!String::from_utf8_lossy(&piped.stderr).contains("hash grouping"));
}

/// With `-z -W`, GNU datamash sorts with `sort -z` and no `-t`, which splits
/// keys at a newline as at any blank, while datamash's own fields keep it:
/// records whose sort keys tie stay in input order, and Groups follow the
/// fields. The outputs are GNU datamash 1.9's, with GNU sort 9.7.
#[test]
fn zero_terminated_whitespace_keys_split_at_newlines_as_the_sort_does() {
    let jobs: [(&[u8], &[&str], &[u8]); 6] = [
        (
            b"a\nb 1\0a\nc 2\0a\nb 3\0",
            &["-z", "-W", "-s", "-g", "1", "count", "1"],
            b"a\nb\t1\0a\nc\t1\0a\nb\t1\0",
        ),
        (
            b"q a\nb 1\0q a\nc 2\0q a\nb 3\0",
            &["-z", "-W", "-s", "-g", "2", "count", "2"],
            b"a\nb\t1\0a\nc\t1\0a\nb\t1\0",
        ),
        // The second sort key of the first two records is "\na", before " b".
        (
            b"q\na b 1\0q\na c 2\0q b 3\0",
            &["-z", "-W", "-s", "-g", "1,2", "count", "2"],
            b"q\na\tb\t1\0q\na\tc\t1\0q\tb\t1\0",
        ),
        (
            b"b 1\0a\nc 2\0 a\tb 3\0x\n\na 4\0",
            &["-z", "-W", "-s", "-g", "1", "count", "1"],
            b"a\t1\0a\nc\t1\0b\t1\0x\n\na\t1\0",
        ),
        // A field of newlines alone joins the next sort field, so the third
        // sort key of the first and last records is empty.
        (
            b"q \n x\0q a b\0q \n y\0",
            &["-z", "-W", "-s", "-g", "3", "count", "3"],
            b"x\t1\0y\t1\0b\t1\0",
        ),
        (
            b"q \n y\0q a b\0q \n x\0",
            &["-z", "-W", "-s", "-g", "3", "count", "3"],
            b"y\t1\0x\t1\0b\t1\0",
        ),
    ];
    for (input, args, expected) in jobs {
        for locale in ["C", "en_US.UTF-8"] {
            for grouping in ["sort", "hash"] {
                // One byte makes the in-process sort spill.
                for memory in ["67108864", "1"] {
                    let out = invoke_in(
                        &candidate(),
                        args,
                        input,
                        &[
                            ("LC_ALL", locale),
                            ("FASTMASH_GROUPING", grouping),
                            ("FASTMASH_SORT_MEMORY_BYTES", memory),
                        ],
                    );
                    assert_eq!(
                        (out.status.code(), out.stdout.as_slice()),
                        (Some(0), expected),
                        "{args:?} {locale} {grouping} {memory}: {out:?}"
                    );
                }
            }
        }
    }
}

/// Hash grouping that gives up sets standard input back to where the Command
/// found it, which need not be the file's start: here a shell has read the
/// first line, and the sort route, in process or through the system sort,
/// reads the Input header and the records after it.
#[test]
fn a_restart_reads_standard_input_from_where_the_command_began() {
    let mut input = b"read by the shell\nkey\tvalue\n".to_vec();
    for row in 0..300u32 {
        input.extend_from_slice(format!("k{}\t{}\n", row * 7 % 11, row % 13 + 1).as_bytes());
    }
    let path = std::env::temp_dir().join(format!("grouping-offset-{}", std::process::id()));
    fs::write(&path, &input).unwrap();
    let binary = candidate();
    let run = |grouping: &str, args: &[&str]| {
        Command::new("/usr/bin/timeout")
            .args(["--kill-after=2s", "30s", "/bin/sh", "-c"])
            .arg("IFS= read -r _ && exec \"$0\" \"$@\"")
            .arg(&binary)
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "C")
            .env("FASTMASH_GROUPING", grouping)
            .stdin(fs::File::open(&path).unwrap())
            .output()
            .unwrap()
    };
    for args in [
        &["-s", "-H", "-g", "1", "geomean", "2"][..],
        &["-s", "-H", "-g", "key", "sum", "value"],
    ] {
        let sorted = run("sort", args);
        assert!(sorted.status.success(), "{args:?}: {sorted:?}");
        assert!(sorted.stdout.starts_with(b"GroupBy(key)\t"), "{sorted:?}");
        for grouping in ["hash", "hash:restart=0", "hash:restart=100"] {
            let hashed = run(grouping, args);
            assert_eq!(
                (hashed.status.code(), &hashed.stdout, &hashed.stderr),
                (sorted.status.code(), &sorted.stdout, &sorted.stderr),
                "{args:?} {grouping}"
            );
        }
    }
    fs::remove_file(path).unwrap();
}

/// The address space, in bytes, of an eight-thread language sort of 300,000
/// rows under an address-space limit of `limit` bytes, paused after two
/// thirds of its input, with the `environment` added; the sort is checked to
/// complete with the right output.
fn paused_language_sort(limit: u64, environment: &[(&str, &str)]) -> u64 {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    let rows: Vec<u8> = (0..300_000u32)
        .flat_map(|n| format!("k{:05}\t{}\n", n * 7919 % 20_000, n % 1000).into_bytes())
        .collect();
    let mut command = Command::new(candidate());
    command
        .args(["-s", "-g", "1", "count", "2"])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "en_US.UTF-8")
        .env("OMP_NUM_THREADS", "8")
        .env("TMPDIR", std::env::temp_dir())
        .envs(environment.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: only async-signal-safe setrlimit and alarm run between fork
    // and exec. The alarm, which exec keeps, ends a sort that hangs.
    unsafe {
        command.pre_exec(move || {
            let limit = libc::rlimit {
                rlim_cur: limit,
                rlim_max: limit,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            libc::alarm(60);
            Ok(())
        });
    }
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    // Once the sort has read most of two thirds of the rows, it has
    // projected several batches of them on eight threads.
    let (first, rest) = rows.split_at(rows.len() / 3 * 2);
    let _ = input.write_all(first);
    let status = fs::read_to_string(format!("/proc/{}/status", child.id())).unwrap_or_default();
    let _ = input.write_all(rest);
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{environment:?}: {output:?}");
    let expected: String = (0..20_000).map(|key| format!("k{key:05}\t15\n")).collect();
    assert!(output.stdout == expected.as_bytes());
    let size: u64 = status
        .lines()
        .find_map(|line| line.strip_prefix("VmSize:"))
        .and_then(|size| size.trim().strip_suffix(" kB"))
        .and_then(|size| size.trim().parse().ok())
        .unwrap_or_else(|| panic!("no address-space size in {status:?}"));
    size << 10
}

/// Under an address-space limit, a language sort that projects on eight
/// threads completes within a fraction of the limit: the malloc arenas are
/// capped (one below 1 GiB). With the arena count that the environment sets,
/// or without the cap, each thread's arena reserves 64 MiB, and the paused
/// sort takes most of the limit (about 480 MiB of 512 where measured).
#[test]
fn eight_thread_language_sorts_use_a_fraction_of_an_address_space_limit() {
    const LIMIT: u64 = 512 << 20;
    let capped = paused_language_sort(LIMIT, &[]);
    assert!(capped < LIMIT / 2, "{capped} bytes of address space");
    let eight = paused_language_sort(LIMIT, &[("MALLOC_ARENA_MAX", "8")]);
    assert!(eight > LIMIT / 2, "{eight} bytes with eight arenas");
}
