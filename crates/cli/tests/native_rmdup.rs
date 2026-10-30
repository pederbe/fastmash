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
        Case {
            args: &["--csv-in", "-s", "rmdup", "1"],
            input: b"b,\"first,b\"\na,\"first\na\"\nb,last\n",
            expected: b"a\tfirst\na\nb\tfirst,b\n",
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
