#![forbid(unsafe_code)]
//! Native duplicate removal, including its observable second-header behavior.
#[path = "support/executable.rs"]
mod executable;
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/test_process.rs"]
mod test_process;

use std::{fs, io::Write, process::Stdio, thread};

struct Case {
    args: &'static [&'static str],
    input: &'static [u8],
    expected: &'static [u8],
}

#[test]
fn native_duplicate_removal_preserves_records_keys_headers_and_separators() {
    let root = temp_dir::TempDir::new("native-rmdup");
    let binary = root.0.join("fastmash");
    fs::copy(executable::fastmash(), &binary).unwrap();
    let cases = [
        Case {
            args: &["-s", "rmdup", "1"],
            input: b"b\tfirst-b\na\tfirst-a\nb\tlast-b\na\tlast-a\n",
            expected: b"a\tfirst-a\nb\tfirst-b\n",
        },
        Case {
            args: &["-s", "-i", "rmdup", "1"],
            input: b"b\tfirst-b\na\tfirst-a\nA\tupper\na\tlast-a\nb\tlast-b\n",
            expected: b"a\tfirst-a\nA\tupper\nb\tfirst-b\n",
        },
        Case {
            args: &["-H", "-s", "rmdup", "key"],
            input: b"key\tvalue\nb\t3\na\t1\na\t2\nb\t4\n",
            expected: b"a\t1\na\t2\nb\t3\n",
        },
        Case {
            args: &["--header-in", "-s", "rmdup", "1"],
            input: b"key\tvalue\nb\t3\na\t1\na\t2\nb\t4\n",
            expected: b"a\t2\nb\t3\n",
        },
        Case {
            args: &["-C", "-s", "rmdup", "1"],
            input: b"# skip\nb\t3\na\t1\nb\t4\n",
            expected: b"a\t1\nb\t3\n",
        },
        Case {
            args: &["--vnlog", "-s", "rmdup", "key"],
            input: b"# key value\n# key value\nb 3\na 1\nb 4\n",
            expected: b"# key value\na 1\nb 3\n",
        },
        Case {
            args: &["-W", "-s", "rmdup", "1"],
            input: b"b  first-b\na  first-a\nb last-b\n",
            expected: b"a\tfirst-a\nb\tfirst-b\n",
        },
        Case {
            args: &["-z", "-s", "rmdup", "1"],
            input: b"b\tB\0a\tA\0b\tlater\0",
            expected: b"a\tA\0b\tB\0",
        },
        // The second header takes the first `a`; the second is printed, the
        // third is a duplicate.
        Case {
            args: &["--header-in", "-s", "rmdup", "1"],
            input: b"key\tv\na\t1\na\t2\na\t3\nb\t4\nb\t5\n",
            expected: b"a\t2\nb\t4\n",
        },
        // The leading blank sorts the later Record first, with the same key.
        Case {
            args: &["-W", "-s", "rmdup", "1"],
            input: b"b first\n b second\nb third\n",
            expected: b"b\tsecond\n",
        },
        // Keys compare up to a NUL, and sort by every byte.
        Case {
            args: &["-s", "rmdup", "1"],
            input: b"a\0c\tfirst\na\0b\tsecond\na\0b\tthird\n",
            expected: b"a\0b\tsecond\n",
        },
        // A key that a NUL ends early repeats a whole key sorted before it.
        Case {
            args: &["-s", "rmdup", "1"],
            input: b"b\tfirst\na\0z\tlater\na\tplain\n",
            expected: b"a\tplain\nb\tfirst\n",
        },
        // Sort keys end at the newline; the fields do not.
        Case {
            args: &["-W", "-z", "-s", "rmdup", "1"],
            input: b"a\nb x\0a\nc y\0a\nb z\0",
            expected: b"a\nb\tx\0a\nc\ty\0",
        },
    ];
    for locale in ["C", "POSIX", "C.UTF-8", "C.utf8", "C.utf-8"] {
        for memory in ["1", "67108864"] {
            for file in [false, true] {
                for case in &cases {
                    let mut command = test_process::bounded(&binary, 15);
                    command
                        .args(case.args)
                        .env_clear()
                        .env("LC_ALL", locale)
                        .env("PATH", "/nonexistent")
                        .env("TMPDIR", &root.0)
                        .env("FASTMASH_SORT_MEMORY_BYTES", memory)
                        .env("FASTMASH_SORT_TRACE", "1")
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped());
                    if file {
                        let input = root.0.join("input");
                        fs::write(&input, case.input).unwrap();
                        command.stdin(fs::File::open(input).unwrap());
                    } else {
                        command.stdin(Stdio::piped());
                    }
                    let mut child = command.spawn().unwrap();
                    let output = thread::scope(|scope| {
                        let writer = child.stdin.take().map(|mut stdin| {
                            scope.spawn(move || stdin.write_all(case.input).unwrap())
                        });
                        let output = child.wait_with_output().unwrap();
                        if let Some(writer) = writer {
                            writer.join().unwrap();
                        }
                        output
                    });
                    assert!(
                        output.status.success(),
                        "{locale} {memory} {file} {:?}: {output:?}",
                        case.args
                    );
                    assert_eq!(
                        output.stdout, case.expected,
                        "{locale} {memory} {file} {:?}",
                        case.args
                    );
                    let trace = String::from_utf8_lossy(&output.stderr);
                    assert!(trace.contains("native sort"), "{trace}");
                    assert!(!trace.contains("system sort"), "{trace}");
                    if memory == "1" {
                        assert!(trace.contains("sort spill: private run opened"), "{trace}");
                    }
                }
            }
        }
    }
    assert!(!root.0.join("fastmash-sort-supervisor").exists());
}

#[test]
fn native_duplicate_removal_reads_long_headers_without_losing_following_records() {
    let root = temp_dir::TempDir::new("native-rmdup-long-header");
    let mut input = vec![b'h'; 131_073];
    input.extend_from_slice(b"\tvalue\nb\t3\na\t1\na\t2\nb\t4\n");
    let path = root.0.join("input");
    fs::write(&path, input).unwrap();
    let output = test_process::bounded(executable::fastmash(), 15)
        .args(["-H", "-s", "rmdup", "1"])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(fs::File::open(path).unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"a\t1\na\t2\nb\t3\n");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn native_duplicate_removal_accepts_non_ascii_separator_bytes() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let mut command = test_process::bounded(executable::fastmash(), 15);
    command
        .args(["-s", "-t"])
        .arg(OsStr::from_bytes(&[0x80]))
        .args(["rmdup", "1"])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"b\x80first\na\x80kept\nb\x80last\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"a\x80kept\nb\x80first\n");
    assert!(output.stderr.is_empty(), "{output:?}");
}

#[test]
fn duplicate_removal_preserves_the_quoted_csv_refusal() {
    let output = test_process::bounded(executable::fastmash(), 15)
        .args(["--csv-in", "-s", "rmdup", "1"])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(77));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"fastmash: CSV is supported only for aggregate and per-row operations\n"
    );
}

fn run(args: &[&str], input: &[u8], memory: &str) -> std::process::Output {
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = temp_dir::TempDir::new(&format!("native-rmdup-run-{run}"));
    let path = root.0.join("input");
    fs::write(&path, input).unwrap();
    test_process::bounded(executable::fastmash(), 30)
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("TMPDIR", &root.0)
        .env("FASTMASH_SORT_MEMORY_BYTES", memory)
        .stdin(fs::File::open(path).unwrap())
        .output()
        .unwrap()
}

#[test]
fn native_duplicate_removal_reports_missing_fields_after_empty_duplicates() {
    for memory in ["1", "67108864"] {
        let output = run(&["-s", "rmdup", "2"], b"b\t\nb\t\nc\nb\t\n", memory);
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(output.stdout, b"b\t\n");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .ends_with(": invalid input: field 2 requested, line 3 has only 1 fields\n"),
            "{output:?}"
        );
    }
}

/// Distinct keys beyond what the sort tracks, and later repeats of keys whose
/// tracking stopped, still leave only each key's first Record.
#[test]
fn native_duplicate_removal_keeps_first_records_beyond_tracked_keys() {
    let mut input = Vec::new();
    let mut first = std::collections::BTreeMap::new();
    for row in 0..70_000u32 {
        let key = if row < 66_000 {
            row * 7_919 % 66_000
        } else {
            row % 4_000
        };
        let key = format!("k{key:05}");
        input.extend_from_slice(format!("{key}\t{row}\n").as_bytes());
        first.entry(key).or_insert(row);
    }
    let expected: Vec<u8> = first
        .iter()
        .flat_map(|(key, row)| format!("{key}\t{row}\n").into_bytes())
        .collect();
    for memory in ["262144", "67108864"] {
        let output = run(&["-s", "rmdup", "1"], &input, memory);
        assert!(output.status.success(), "{memory}: {output:?}");
        assert!(output.stdout == expected, "{memory}");
    }
}

#[test]
fn native_duplicate_removal_drops_tracked_keys_once_tracking_is_full() {
    // Frequent repeats keep the filter on while distinct keys fill its table.
    let mut rows = Vec::new();
    rows.extend((0..140_000u32).map(|row| format!("hot{}", row % 3)));
    rows.extend((0..140_000u32).map(|row| format!("cold{:06}", row * 7_919 % 140_000)));
    rows.extend((0..3_000u32).map(|row| format!("hot{}", row % 3)));
    rows.extend((0..3_000u32).map(|row| format!("cold{:06}", row * 31 % 140_000)));
    let mut input = Vec::new();
    let mut first = std::collections::BTreeMap::new();
    for (row, key) in rows.into_iter().enumerate() {
        input.extend_from_slice(format!("{key}\t{row}\n").as_bytes());
        first.entry(key).or_insert(row);
    }
    let expected: Vec<u8> = first
        .iter()
        .flat_map(|(key, row)| format!("{key}\t{row}\n").into_bytes())
        .collect();
    let output = run(&["-s", "rmdup", "1"], &input, "262144");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout == expected);
}
