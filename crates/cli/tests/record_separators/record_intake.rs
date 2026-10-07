//! How Records are taken in: comments, vnlog annotations, the Input header and
//! read errors. Expected bytes and statuses are GNU datamash 1.9's, observed
//! with the reference executable (SHA-256 e18d9451...) on its WSL profile,
//! whose system `sort` is uutils; `datamash:` is written here as `fastmash:`.
use super::{candidate, transpose::failing_input_with};
use std::{
    fs,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

const WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";

fn run(locale: &str, arguments: &[&str], input: &[u8]) -> Output {
    let path = std::env::temp_dir().join(format!(
        "record-intake-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(&path, input).unwrap();
    let output = Command::new(candidate())
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", locale)
        .env("PATH", "/usr/bin:/bin")
        .stdin(fs::File::open(&path).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    fs::remove_file(path).unwrap();
    output
}

fn assert_result(output: &Output, status: i32, stdout: &[u8], stderr: &[u8]) {
    assert_eq!(output.status.code(), Some(status), "{output:?}");
    assert_eq!(output.stdout, stdout, "{output:?}");
    assert_eq!(output.stderr, stderr, "{output:?}");
}

#[test]
fn sorted_vnlog_rmdup_reads_its_second_header_from_the_sorted_records() {
    // GNU reads the Input header before sorting, then again from the sort
    // pipe (datamash.c open_input, remove_dups_in_file), where it skips
    // comments in the vnlog prologue (text-lines.c line_record_fread). The
    // reference host has no en_US locale, so GNU sorted in byte order; `#`
    // records sort before letters under en_US collation too. On Linux the C
    // locale uses the system sort; macOS and the language locale sort natively.
    for locale in ["C", "en_US.UTF-8"] {
        for (arguments, input, stdout) in [
            (
                ["--vnlog", "-s", "rmdup", "x"],
                b"# x y\nb 2\n#late\n# p q\na 1\n".as_slice(),
                b"# p q\na 1\nb 2\n".as_slice(),
            ),
            (
                ["--vnlog", "-s", "rmdup", "x"],
                b"# x y\nb 2\n## c\n\n#!x\n#\n# p q\na 1\nb 3\n",
                b"# p q\na 1\nb 2\n",
            ),
            // Sorted by the second field, `#late` comes first and is the header.
            (
                ["--vnlog", "-s", "rmdup", "y"],
                b"# x y\nb 2\n#late\n# p q\na 1\n",
                b"# late\na 1\nb 2\n",
            ),
            (["--vnlog", "-s", "rmdup", "x"], b"# x y\n", b""),
        ] {
            let output = run(locale, &arguments, input);
            assert_result(&output, 0, stdout, b"");
        }
        // Without a header among the sorted records, the first is data.
        let output = run(
            locale,
            &["--vnlog", "-s", "rmdup", "x"],
            b"# x y\nb 2\na 1\n",
        );
        assert_result(
            &output,
            1,
            b"",
            b"fastmash: invalid vnlog data: received record before header: 'a 1'\n",
        );
    }
}

#[test]
fn missing_headers_keep_native_and_external_full_warning_order() {
    // Native sorting diagnoses the missing Input header before entering the
    // data phase, so it emits no full warning. Linux's external route still
    // reads the header from the sort pipe and warns after sort's error.
    for operation in ["count", "rms"] {
        for input in [b"".as_slice(), b"#c\n"] {
            let output = run(
                "C",
                &["-C", "--full", "-s", "-H", "-g", "x", operation, "1"],
                input,
            );
            #[cfg(target_os = "linux")]
            if operation == "rms" {
                // Older GNU sort includes its absolute program name, as in the corpus.
                let mut stderr = if output.stderr.starts_with(b"/usr/bin/sort: ") {
                    b"/usr/bin/".to_vec()
                } else {
                    Vec::new()
                };
                stderr.extend_from_slice(
                    b"sort: field number is zero: invalid field specification '0,0'\n",
                );
                stderr.extend_from_slice(WARNING);
                stderr.extend_from_slice(b"fastmash: read error (on close)\n");
                assert_result(&output, 1, b"", &stderr);
                continue;
            }
            assert_result(
                &output,
                1,
                b"",
                b"fastmash: missing input header for named grouping key\n",
            );
        }
    }
}

#[test]
fn skipped_comments_never_become_language_sort_keys() {
    // GNU hands comments to sort and skips them when reading its output
    // (text-lines.c line_record_fread), so their bytes cannot matter.
    for (arguments, input) in [
        (
            ["-C", "-s", "-W", "-g1", "count", "1"].as_slice(),
            b"#\xff\na 1\n".as_slice(),
        ),
        (&["-C", "-s", "-W", "rmdup", "1"], b";\xff\na 1\n"),
        (
            &["-C", "-s", "-W", "-g1", "first", "2"],
            b"a 1\n  ;\xff 2\n",
        ),
    ] {
        let output = run("en_US.UTF-8", arguments, input);
        assert_result(&output, 0, b"a\t1\n", b"");
    }
}

#[test]
fn a_read_error_ends_the_input_and_is_reported_after_the_mode_finishes() {
    // GNU's reader treats a read error as the end of input; close_input then
    // dies with "read error" (datamash.c close_input).
    const EIO: &[u8] = b"fastmash: read error: Input/output error\n";
    let table = b"a\tb\nc\td\npartial".as_slice();
    let vnlog = b"# a b\n1 2\n3 4\npartial".as_slice();
    for (arguments, input, stdout) in [
        (&["reverse"][..], table, b"b\ta\nd\tc\n".as_slice()),
        (&["noop"], table, b""),
        (&["--full", "noop"], table, b"a\tb\nc\td\n"),
        (&["check"], table, b"2 lines, 2 fields\n"),
        (&["rmdup", "1"], table, b"a\tb\nc\td\n"),
        (&["-H", "rmdup", "1"], table, b"a\tb\nc\td\n"),
        (&["count", "1"], table, b"2\n"),
        (&["-C", "count", "1"], table, b"2\n"),
        (&["-H", "count", "1"], table, b"count(a)\n1\n"),
        (&["-H", "count", "1"], b"partial", b""),
        (&["-g1", "count", "1"], table, b"a\t1\nc\t1\n"),
        (
            &["md5", "1"],
            table,
            b"0cc175b9c0f1b6a831c399e269772661\n4a8a08f09d37b73795649038408b5f33\n",
        ),
        (&["ct", "1,2"], table, b"\tb\td\na\t1\tN/A\nc\tN/A\t1\n"),
        (&["transpose"], table, b"a\tc\nb\td\n"),
        (&["--vnlog", "count", "a"], vnlog, b"# count(a)\n2\n"),
        (&["--vnlog", "reverse"], vnlog, b"# b a\n2 1\n4 3\n"),
        (&["--vnlog", "reverse"], b"partial", b""),
    ] {
        let output = failing_input_with(&candidate(), arguments, input, false, "C");
        assert_result(&output, 1, stdout, EIO);
    }
    // The read error is reported before the full device's write error, also
    // when a single result row is written at the end.
    for (arguments, input) in [
        (&["reverse"][..], table),
        (&["count", "1"], b"1\n2\npartial"),
        (&["--header-in", "sum", "1"], b"1\n2\npartial"),
    ] {
        let output = failing_input_with(&candidate(), arguments, input, true, "C");
        assert_result(
            &output,
            1,
            b"",
            b"fastmash: read error: Input/output error\nfastmash: write error\n",
        );
    }
}

#[test]
fn a_read_error_fails_the_sort_like_the_system_sort() {
    // GNU's sort fails on the read error and yields no records; datamash
    // finishes on an empty sorted stream, with the header it read before
    // sorting, and reports "read error (on close)" after sort's own message.
    // The native sort reports the read error itself.
    const EIO: &[u8] = b"fastmash: read error: Input/output error\n";
    let table = b"a\tb\nc\td\npartial".as_slice();
    for (locale, arguments, stdout) in [
        ("C", &["-s", "-g1", "count", "1"][..], b"".as_slice()),
        (
            "C",
            &["-s", "-H", "-g1", "first", "2"],
            b"GroupBy(a)\tfirst(b)\n",
        ),
        ("C", &["-s", "--header-out", "-g1", "count", "1"], b""),
        ("C", &["-s", "ct", "1,2"], b"\n"),
        ("en_US.UTF-8", &["-s", "rmdup", "1"], b""),
        ("en_US.UTF-8", &["-s", "-H", "rmdup", "1"], b""),
    ] {
        let output = failing_input_with(&candidate(), arguments, table, false, locale);
        assert_result(&output, 1, stdout, EIO);
    }
    let output = failing_input_with(
        &candidate(),
        &["-s", "-g1", "-H", "--full", "count", "1"],
        table,
        false,
        "C",
    );
    assert_result(&output, 1, b"a\tb\tcount(a)\n", &[WARNING, EIO].concat());
    // The system sort reads the failing input itself after the header read
    // failed; GNU's "read error (on close)" then carries the header's errno.
    let output = failing_input_with(
        &candidate(),
        &["-s", "-H", "-g1", "rms", "2"],
        b"partial",
        false,
        "C",
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    #[cfg(target_os = "linux")]
    assert!(
        output
            .stderr
            .ends_with(b"fastmash: read error (on close): Input/output error\n"),
        "{output:?}"
    );
    #[cfg(target_os = "macos")]
    assert_eq!(output.stderr, EIO);
}
