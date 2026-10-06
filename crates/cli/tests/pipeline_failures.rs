//! Installed descriptor regressions. The same matrix can retain reference observations.
use std::{
    ffi::OsStr,
    fs,
    os::unix::{
        net::UnixStream,
        process::{CommandExt, ExitStatusExt},
    },
    process::{Command, Output, Stdio},
};

const MODES: &[(&str, &[&str])] = &[
    ("top", &["top:1", "2"]),
    ("bottom", &["bottom:1", "2"]),
    ("plain", &["count", "1"]),
    ("cut", &["cut", "2,1"]),
    ("round", &["round", "2"]),
    ("getnum", &["getnum:n", "1"]),
    ("bin", &["bin:1", "2"]),
    ("strbin", &["strbin:1", "1"]),
    ("reverse", &["reverse"]),
    ("transpose", &["transpose"]),
    ("crosstab", &["ct", "1,2"]),
    ("check", &["check"]),
    ("dedup", &["dedup", "1"]),
    ("noop", &["-f", "noop"]),
    ("header", &["--header-out", "count", "1"]),
    (
        "csv",
        &[
            "--csv-out",
            "--header-out",
            "--result-name=1:records,\"daily\"",
            "count",
            "1",
        ],
    ),
    (
        "result-name",
        &["--header-out", "--result-name=1:records", "count", "1"],
    ),
    ("group", &["-g", "1", "count", "1"]),
    ("native", &["-s", "-g", "1", "count", "1"]),
    ("external", &["-s", "-g", "1", "geomean", "2"]),
    ("named", &["-H", "-s", "-g", "key", "geomean", "value"]),
    ("full", &["--full", "count", "1"]),
    ("help", &["--help"]),
];
const FAULTS: &[&str] = &[
    "closed-input",
    "write-only-input",
    "directory-input",
    "closed-output",
    "read-only-output",
    "full-output",
    "broken-output",
    "closed-error",
    "read-only-error",
    "closed-output-empty",
    "full-output-closed-error",
];

fn invoke(binary: &OsStr, args: &[&str], fault: &str) -> Output {
    let path = std::env::temp_dir().join(format!(
        "pipeline-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::write(
        &path,
        if args.contains(&"-H") {
            b"key\tvalue\na\t1\nb\t2\n".as_slice()
        } else {
            b"a\t1\nb\t2\n"
        },
    )
    .unwrap();
    let mut command = Command::new(binary);
    command
        .arg0("fastmash")
        .args(args)
        .env_clear()
        .env("LC_ALL", "C")
        .env("PATH", "/usr/bin:/bin")
        .stdin(fs::File::open(&path).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match fault {
        "closed-input"
        | "closed-output"
        | "closed-error"
        | "closed-output-empty"
        | "full-output-closed-error" => {
            if fault == "closed-output-empty" {
                command.stdin(Stdio::null());
            }
            if fault == "full-output-closed-error" {
                command.stdout(fs::File::options().write(true).open("/dev/full").unwrap());
            }
            let fd = match fault {
                "closed-input" => 0,
                "closed-output" | "closed-output-empty" => 1,
                _ => 2,
            };
            // SAFETY: only async-signal-safe close runs between fork and exec.
            unsafe {
                command.pre_exec(move || {
                    if libc::close(fd) < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        "write-only-input" => {
            command.stdin(fs::File::options().write(true).open("/dev/null").unwrap());
        }
        "directory-input" => {
            command.stdin(fs::File::open("/tmp").unwrap());
        }
        "read-only-output" => {
            command.stdout(fs::File::open("/dev/null").unwrap());
        }
        "read-only-error" => {
            command.stderr(fs::File::open("/dev/null").unwrap());
        }
        "full-output" => {
            command.stdout(fs::File::options().write(true).open("/dev/full").unwrap());
        }
        "broken-output" => {
            let (writer, reader) = UnixStream::pair().unwrap();
            drop(reader);
            command.stdout(Stdio::from(std::os::fd::OwnedFd::from(writer)));
        }
        _ => panic!("unknown fault"),
    }
    let mut output = command.output().unwrap();
    fs::remove_file(path).unwrap();
    output.stderr = normalize_sort_name(&output.stderr);
    output
}

/// The system sort's diagnostics start with its program name, which older GNU
/// sort takes from argv[0] (`/usr/bin/sort`, as Fastmash and GNU datamash run
/// it) and newer GNU sort and uutils give as `sort`: compare them as `sort`.
fn normalize_sort_name(stderr: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(stderr);
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    lines
        .iter()
        .map(|line| {
            line.strip_prefix("/usr/bin/sort: ")
                .map_or((*line).to_owned(), |rest| format!("sort: {rest}"))
        })
        .collect::<String>()
        .into_bytes()
}

fn retain(suite: &str, name: &str, output: &Output) {
    if let Some(root) = std::env::var_os("FASTMASH_FAILURE_EVIDENCE") {
        let root = std::path::PathBuf::from(root).join(suite);
        fs::create_dir_all(&root).unwrap();
        for (extension, bytes) in [
            ("stdout", output.stdout.as_slice()),
            ("stderr", output.stderr.as_slice()),
            ("status", output.status.to_string().as_bytes()),
        ] {
            use std::io::Write;
            fs::File::options()
                .write(true)
                .create_new(true)
                .open(root.join(format!("{name}.{extension}")))
                .unwrap()
                .write_all(bytes)
                .unwrap();
        }
    }
}

#[test]
fn installed_descriptor_failures_are_not_successful_empty_results() {
    let binary = std::env::var_os("FASTMASH_FAILURE_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
    for &(mode, args) in MODES {
        for &fault in FAULTS {
            let output = invoke(&binary, args, fault);
            let name = format!("{mode}-{fault}");
            retain("candidate", &name, &output);
            if matches!(mode, "crosstab" | "round" | "getnum" | "bin" | "strbin")
                && let Some(reference) = std::env::var_os("FASTMASH_FAILURE_REFERENCE")
            {
                let expected = invoke(&reference, args, fault);
                retain("reference", &name, &expected);
                assert_eq!(output.stdout, expected.stdout, "{name}");
                assert_eq!(output.stderr, expected.stderr, "{name}");
                assert_eq!(output.status, expected.status, "{name}");
            }
            check(mode, fault, &name, &output);
        }
    }
}

fn check(mode: &str, fault: &str, name: &str, output: &Output) {
    if mode == "named" && fault == "closed-output-empty" {
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        assert_eq!(output.stderr, b"sort: field number is zero: invalid field specification '0,0'\nfastmash: read error (on close)\n", "{name}");
        return;
    }
    const WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";
    let error_closed = matches!(
        fault,
        "closed-error" | "read-only-error" | "full-output-closed-error"
    );
    let unused_input = mode == "help" && fault.ends_with("input");
    let unused_output =
        fault == "closed-output-empty" && !matches!(mode, "help" | "check" | "crosstab");
    let ordinary_output = unused_input || matches!(fault, "closed-error" | "read-only-error");
    let expected_output: &[u8] = if mode == "crosstab" && fault.ends_with("input") {
        b"\n"
    } else if mode == "check" && fault.ends_with("input") {
        b"0 lines, 0 fields\n"
    } else if ordinary_output {
        match mode {
            "top" => b"b\t2\n",
            "bottom" => b"a\t1\n",
            "plain" => b"2\n",
            "round" | "bin" => b"1\n2\n",
            "getnum" | "strbin" => b"0\n0\n",
            "cut" | "reverse" => b"1\ta\n2\tb\n",
            "transpose" => b"a\tb\n1\t2\n",
            "crosstab" => b"\t1\t2\na\t1\tN/A\nb\tN/A\t1\n",
            "noop" | "dedup" => b"a\t1\nb\t2\n",
            "check" => b"2 lines, 2 fields\n",
            "header" => b"count(field-1)\n2\n",
            "result-name" => b"records\n2\n",
            "csv" => b"\"records,\"\"daily\"\"\"\n2\n",
            "group" | "native" => b"a\t1\nb\t1\n",
            "external" => b"a\t1\nb\t2\n",
            "named" => b"GroupBy(key)\tgeomean(value)\na\t1\nb\t2\n",
            "full" => b"a\t1\t2\n",
            "help" => include_bytes!("../src/help.txt"),
            _ => unreachable!(),
        }
    } else {
        b""
    };
    assert_eq!(output.stdout, expected_output, "{name}");
    let signal = (fault == "broken-output" && mode != "help").then_some(13);
    let code = if signal.is_some() {
        None
    } else if unused_input || unused_output || (ordinary_output && mode != "full") {
        Some(0)
    } else if mode == "help" && fault == "broken-output" {
        Some(77)
    } else {
        Some(1)
    };
    assert_eq!(output.status.code(), code, "{name}: {output:?}");
    assert_eq!(output.status.signal(), signal, "{name}: {output:?}");
    let mut stderr = Vec::new();
    if !error_closed {
        if mode == "full" {
            stderr.extend_from_slice(WARNING);
        }
        if !unused_input && !unused_output && !ordinary_output {
            let message: &[u8] = match fault {
                "closed-input" | "write-only-input" => b"fastmash: read error: Bad file descriptor\n",
                "directory-input" if mode == "named" => b"sort: field number is zero: invalid field specification '0,0'\nfastmash: read error (on close): Is a directory\n",
                "directory-input" if mode == "external" => {
                    // The child owns its diagnostic; these are the two named test profiles.
                    let gnu = b"sort: read failed: -: Is a directory\nfastmash: read error (on close)\n";
                    let uutils = b"sort: Is a directory (os error 21)\nfastmash: read error (on close)\n";
                    assert!(output.stderr == gnu || output.stderr == uutils, "{name}: {output:?}");
                    return;
                }
                "directory-input" => b"fastmash: read error: Is a directory\n",
                "closed-output" | "read-only-output" | "closed-output-empty" => b"fastmash: write error: Bad file descriptor\n",
                "full-output" => b"fastmash: write error: No space left on device\n",
                "broken-output" if mode == "help" => b"fastmash: unsupported output I/O error\n",
                "broken-output" => b"",
                _ => unreachable!("{fault}"),
            };
            stderr.extend_from_slice(message);
        }
    }
    assert_eq!(output.stderr, stderr, "{name}");
}

#[test]
#[ignore = "named reference executable and fresh evidence directory required"]
fn retain_reference_descriptor_matrix() {
    let binary = std::env::var_os("FASTMASH_FAILURE_REFERENCE").expect("reference path");
    assert!(std::env::var_os("FASTMASH_FAILURE_EVIDENCE").is_some());
    for &(mode, args) in MODES {
        for &fault in FAULTS {
            let sorter = std::env::var("FASTMASH_FAILURE_SORT").ok();
            let option = sorter.map(|path| format!("--sort-cmd={path}"));
            let mut arguments = args.to_vec();
            if let Some(option) = &option {
                arguments.insert(0, option);
            }
            let output = invoke(&binary, &arguments, fault);
            retain("reference", &format!("{mode}-{fault}"), &output);
        }
    }
}

#[test]
fn calculation_failure_and_failed_diagnostic_still_finalize_output() {
    let binary = std::env::var_os("FASTMASH_FAILURE_BINARY")
        .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
    for fault in ["full-output", "full-output-closed-error"] {
        let output = invoke(
            &binary,
            &["--header-out", "count", "1", "count", "3"],
            fault,
        );
        retain("candidate", &format!("primary-{fault}"), &output);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let expected: &[u8] = if fault == "full-output" {
            b"fastmash: invalid input: field 3 requested, line 1 has only 2 fields\nfastmash: write error\n"
        } else {
            b""
        };
        assert_eq!(output.stderr, expected);
        if let Some(reference) = std::env::var_os("FASTMASH_FAILURE_REFERENCE") {
            let reference = invoke(
                &reference,
                &["--header-out", "count", "1", "count", "3"],
                fault,
            );
            retain("reference", &format!("primary-{fault}"), &reference);
            assert_eq!(output.status, reference.status);
            assert_eq!(output.stdout, reference.stdout);
            assert_eq!(output.stderr, reference.stderr);
        }
    }
}
