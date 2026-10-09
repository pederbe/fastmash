//! Executable weighted Command checks over actual files and pipes.
#[path = "support/output_fault.rs"]
mod output_fault;
use output_fault::OutputFault;
use std::{
    fs,
    io::Write,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

#[path = "support/executable.rs"]
mod executable;

#[path = "support/temp_dir.rs"]
mod temp_dir;

const FULL_WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";

#[cfg(target_os = "linux")]
const PAIRED_SORT_ROUTE: &str = "system sort";
#[cfg(target_os = "macos")]
const PAIRED_SORT_ROUTE: &str = "native sort";

fn candidate() -> Command {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C");
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    command
}

fn invoke(args: &[&str], input: &[u8], regular_file: bool) -> Output {
    invoke_in(args, input, regular_file, &[])
}

fn invoke_in(
    args: &[&str],
    input: &[u8],
    regular_file: bool,
    environment: &[(&str, &str)],
) -> Output {
    let mut command = candidate();
    command.args(args).envs(environment.iter().copied());
    if regular_file {
        let temporary =
            temp_dir::TempDir::new(&format!("weighted-{:?}", std::thread::current().id()));
        let path = temporary.0.join("input");
        fs::write(&path, input).unwrap();
        command
            .stdin(fs::File::open(&path).unwrap())
            .output()
            .unwrap()
    } else {
        let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(input);
        child.wait_with_output().unwrap()
    }
}

#[test]
fn sorted_weighted_text_needs_temporary_storage_only_when_it_spills() {
    let directory = temp_dir::TempDir::new("weighted-sort-temporary");
    let unusable = directory.0.join("not-a-directory");
    fs::write(&unusable, b"untouched").unwrap();
    let input = b"b\t5\t2\na\t10\t1\na\t20\t3\n";
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            let output = invoke_in(
                &["-s", "-g1", "wmean", "2:3"],
                input,
                regular_file,
                &[
                    ("FASTMASH_GROUPING", "sort"),
                    ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ("TMPDIR", unusable.to_str().unwrap()),
                ],
            );
            if memory == "1" {
                assert_eq!(output.status.code(), Some(1), "{output:?}");
                assert!(output.stdout.is_empty());
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("sort temporary I/O error")
                );
            } else {
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, b"a\t17.5\nb\t5\n");
                assert!(output.stderr.is_empty());
            }
        }
    }
    assert_eq!(fs::read(unusable).unwrap(), b"untouched");
}

#[test]
fn weighted_numeric_sort_eligibility_and_system_composition_are_exercised() {
    let input = b"b\t5\t2\na\t10\t1\na\t20\t3\n";
    for regular_file in [false, true] {
        for (extra, expected, route) in [
            (vec![], b"a\t17.5\nb\t5\n".as_slice(), "native sort"),
            (
                vec!["dotprod", "2:3"],
                b"a\t17.5\t70\nb\t5\t10\n",
                PAIRED_SORT_ROUTE,
            ),
        ] {
            let mut args = vec!["-s", "-g1", "wmean", "2:3"];
            args.extend(extra);
            let output = invoke_in(
                &args,
                input,
                regular_file,
                &[("FASTMASH_GROUPING", "sort"), ("FASTMASH_SORT_TRACE", "1")],
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, expected);
            // The maintained trace establishes exercised coverage; correctness
            // is observed only through the weighted report bytes above.
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .starts_with(&format!("sort route: {route}"))
            );
        }
    }
}

#[test]
fn sorted_named_reports_retain_reversed_shared_and_overlapping_pair_fields() {
    let input = b"key\tvalue\tweight\ttag\tother\tunused\nb\t10\t2\tz\t9\t\xff\na\t2\t1\ty\t4\n b\t8\t1\tx\t2\na\t6\t3\ty\t8\nb\t10\t2\tz\t9\n";
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for grouping in ["sort", "hash", "hash:restart=2"] {
                let output = invoke_in(
                    &[
                        "-H",
                        "-s",
                        "-g",
                        "tag,key",
                        "--result-name=2:average",
                        "count",
                        "value",
                        "wmean",
                        "value:weight,other:weight,weight:weight,weight:value",
                        "mean",
                        "value",
                    ],
                    input,
                    regular_file,
                    &[
                        ("FASTMASH_GROUPING", grouping),
                        ("FASTMASH_PIPE_GROUPING", "hash"),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ],
                );
                assert!(output.status.success(), "{grouping} {memory}: {output:?}");
                assert_eq!(output.stdout, b"GroupBy(tag)\tGroupBy(key)\tcount(value)\taverage\twmean(other,weight)\twmean(weight,weight)\twmean(weight,value)\tmean(value)\nx\t b\t1\t8\t2\t1\t1\t8\ny\ta\t2\t5\t7\t2.5\t2.5\t4\nz\tb\t2\t10\t9\t2\t2\t10\n");
                assert!(output.stderr.is_empty(), "{output:?}");
            }
        }
        let output = invoke_in(
            &["-s", "-g3", "wmean", "2:3,3:2,3:3"],
            b"a\t2\t1\nb\t6\t3\nc\t2\t1\n",
            regular_file,
            &[
                ("FASTMASH_GROUPING", "sort"),
                ("FASTMASH_SORT_MEMORY_BYTES", "1"),
            ],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"1\t2\t1\t1\n3\t6\t3\t3\n");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn sorting_spill_and_hash_replay_preserve_the_ordered_binary80_trace() {
    // 2^64+1 ties and rounds to 2^64. Group a then cancels to zero;
    // Group b cancels first and retains 1/3. Group c is exact beyond binary64.
    // Valid zero contributors separate the active contributions across runs.
    let mut input =
        b"a\t18446744073709551616\t1\nb\t18446744073709551616\t1\nc\t9007199254740993\t1\n"
            .to_vec();
    input.extend(b"b\t0\t0\na\t0\t0\nc\t0\t0\n".repeat(40));
    input.extend(b"a\t1\t1\nb\t-18446744073709551616\t1\n");
    input.extend(b"c\t0\t0\na\t0\t0\nb\t0\t0\n".repeat(40));
    input.extend(b"a\t-18446744073709551616\t1\nb\t1\t1\n");
    for regular_file in [false, true] {
        for locale in ["C", "en_US.UTF-8"] {
            for memory in ["67108864", "1"] {
                for grouping in ["sort", "hash", "hash:restart=2"] {
                    let output = invoke_in(
                        &["-s", "-g1", "--format=%.20g", "wmean", "2:3"],
                        &input,
                        regular_file,
                        &[
                            ("LC_ALL", locale),
                            ("FASTMASH_GROUPING", grouping),
                            ("FASTMASH_PIPE_GROUPING", "hash"),
                            ("FASTMASH_SORT_MEMORY_BYTES", memory),
                        ],
                    );
                    assert!(
                        output.status.success(),
                        "{locale} {memory} {grouping}: {output:?}"
                    );
                    assert_eq!(
                        output.stdout,
                        b"a\t0\nb\t0.33333333333333333334\nc\t9007199254740993\n"
                    );
                    assert!(output.stderr.is_empty());
                }
            }
        }
        // Existing paired composition selects the system sort on Linux in the
        // C locale; macOS uses the native sort.
        // Zero products and these integer products are exact for dotprod too.
        let output = invoke_in(
            &[
                "-s",
                "-g1",
                "--format=%.20g",
                "wmean",
                "2:3",
                "dotprod",
                "2:3",
            ],
            &input,
            regular_file,
            &[("FASTMASH_GROUPING", "sort")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"a\t0\t0\nb\t0.33333333333333333334\t1\nc\t9007199254740993\t9007199254740993\n"
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn weighted_hash_success_and_restart_rebind_headers_without_duplicate_contributions() {
    let input = b"key\tvalue\tweight\nb\t5\t2\na\t10\t1\nb\t7\t2\na\t20\t3\n";
    for regular_file in [false, true] {
        for (grouping, evidence) in [
            ("hash", "hash grouping: 2 Groups of 4 records"),
            ("hash:restart=2", "restart requested"),
        ] {
            let output = invoke_in(
                &[
                    "-H",
                    "-s",
                    "-g",
                    "key",
                    "--result-name=1:average",
                    "wmean",
                    "value:weight",
                ],
                input,
                regular_file,
                &[
                    ("FASTMASH_GROUPING", grouping),
                    ("FASTMASH_PIPE_GROUPING", "hash"),
                    ("FASTMASH_SORT_TRACE", "1"),
                ],
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"GroupBy(key)\taverage\na\t17.5\nb\t6\n");
            assert!(
                String::from_utf8_lossy(&output.stderr).contains(evidence),
                "{output:?}"
            );
        }
    }
}

#[test]
fn weighted_sort_preserves_original_numeric_continuations_on_both_pair_roles() {
    let crossing = format!("a+{}1e+5\n", " ".repeat(510));
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for (args, input, expected) in [
                (
                    vec!["-te", "-s", "-g3", "wmean", "1:2"],
                    b"1e2ea\n3e1eb\n".as_slice(),
                    b"ae1\nbe3\n".as_slice(),
                ),
                (
                    vec!["-t+", "-s", "-g1", "wmean", "2:3"],
                    b"b+10+2\na+3+1\n",
                    b"a+3\nb+10\n",
                ),
                (
                    vec!["-t+", "-s", "-g1", "wmean", "3:2"],
                    b"b+10+2\na+3+1\n",
                    b"a+1\nb+2\n",
                ),
            ] {
                let output = invoke_in(
                    &args,
                    input,
                    regular_file,
                    &[
                        ("FASTMASH_GROUPING", "sort"),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ],
                );
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, expected);
                assert!(output.stderr.is_empty());
            }
            // GNU field-ops.c:385-404 reparses a crossed Field alone. "1e"
            // is incomplete, while a crossed 512-byte Field hits its explicit
            // too-long check first. Packed conversion must retain that result
            // for either weighted role (as in the existing sum/mean fixture).
            for pair in ["2:3", "3:2"] {
                for (input, diagnostic) in [
                    (
                        b"a+1e+2\n".as_slice(),
                        b"fastmash: invalid numeric value in line 1 field 2: '1e'\n".as_slice(),
                    ),
                    (
                        crossing.as_bytes(),
                        b"fastmash: internal error: input field too long (512)\n".as_slice(),
                    ),
                ] {
                    let output = invoke_in(
                        &["-t+", "-s", "-g1", "wmean", pair],
                        input,
                        regular_file,
                        &[
                            ("FASTMASH_GROUPING", "sort"),
                            ("FASTMASH_SORT_MEMORY_BYTES", memory),
                        ],
                    );
                    assert_eq!(output.status.code(), Some(1), "{output:?}");
                    assert!(output.stdout.is_empty());
                    assert_eq!(output.stderr, diagnostic);
                }
            }
        }
    }
}

#[test]
fn sorted_weighted_key_equality_and_text_controls_follow_existing_grouping() {
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for grouping in ["sort", "hash", "hash:restart=2"] {
                for (args, input, locale, expected) in [
                    (
                        vec!["-s", "-ig1", "wmean", "2:3"],
                        b"b\t5\t2\nA\t2\t1\na\t6\t3\n".as_slice(),
                        "C",
                        b"A\t5\nb\t5\n".as_slice(),
                    ),
                    (
                        vec!["-s", "-ig1", "wmean", "2:3"],
                        b"b\t5\t2\nA\t2\t1\na\t6\t3\n",
                        "en_US.UTF-8",
                        b"A\t5\nb\t5\n",
                    ),
                    (
                        vec!["-Ws", "-g1", "wmean", "2:3"],
                        b" b 5 2\na 2 1\nb 7 2\n a 6 3\n",
                        "C",
                        b"a\t6\nb\t5\na\t2\nb\t7\n",
                    ),
                    (
                        vec!["-zCs", "-g1", "wmean", "2:3"],
                        b"#skip\0b\t5\t2\0a\t2\t1\0a\t6\t3\0",
                        "C",
                        b"a\t5\0b\t5\0",
                    ),
                ] {
                    let output = invoke_in(
                        &args,
                        input,
                        regular_file,
                        &[
                            ("LC_ALL", locale),
                            ("FASTMASH_GROUPING", grouping),
                            ("FASTMASH_PIPE_GROUPING", "hash"),
                            ("FASTMASH_SORT_MEMORY_BYTES", memory),
                        ],
                    );
                    assert!(output.status.success(), "{args:?} {grouping}: {output:?}");
                    assert_eq!(output.stdout, expected, "{args:?} {grouping}");
                    assert!(output.stderr.is_empty());
                }
            }
        }
        let output = invoke_in(
            &["-s", "wmean", "1:2"],
            b"18446744073709551616\t1\n1\t1\n-18446744073709551616\t1\n",
            regular_file,
            &[
                ("FASTMASH_GROUPING", "invalid"),
                ("FASTMASH_SORT_MEMORY_BYTES", "invalid"),
            ],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"0\n");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn sorted_weighted_text_csv_output_encodes_keys_labels_and_copied_fields() {
    let input = b"key,\"x\tvalue\tweight\tnote,\"x\nb\t10\t2\tother\na,\"x\t2\t1\tfirst,\"x\ra\na,\"x\t6\t3\theavy\n";
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for grouping in ["sort", "hash:restart=2"] {
                for (full, expected) in [
                    (false, b"\"GroupBy(key,\"\"x)\",\"wmean(value,weight)\",\"average,\"\"daily\"\"\nreading\"\n\"a,\"\"x\",5,2.5\nb,10,2\n".as_slice()),
                    (true, b"\"key,\"\"x\",value,weight,\"note,\"\"x\",\"wmean(value,weight)\",\"average,\"\"daily\"\"\nreading\"\n\"a,\"\"x\",2,1,\"first,\"\"x\ra\",5,2.5\nb,10,2,other,10,2\n".as_slice()),
                ] {
                    let mut args = vec!["--csv-out", "-H", "-s", "-g1", "--result-name=2:average,\"daily\"\nreading", "wmean", "2:3,3:3"];
                    if full { args.push("--full"); }
                    let output = invoke_in(&args, input, regular_file, &[("FASTMASH_GROUPING", grouping), ("FASTMASH_PIPE_GROUPING", "hash"), ("FASTMASH_SORT_MEMORY_BYTES", memory)]);
                    assert!(output.status.success(), "{output:?}");
                    assert_eq!(output.stdout, expected);
                    assert_eq!(output.stderr, if full { FULL_WARNING } else { b"" });
                }
                let output = invoke_in(
                    &[
                        "--csv-out",
                        "-H",
                        "-s",
                        "-g1",
                        "--format=%.2f",
                        "wmean",
                        "2:3",
                    ],
                    b"key\tvalue\tweight\nb\t5,5\t2\na\t1,5\t1\na\t3,5\t3\n",
                    regular_file,
                    &[
                        ("LC_ALL", "de_DE.UTF-8"),
                        ("FASTMASH_GROUPING", grouping),
                        ("FASTMASH_PIPE_GROUPING", "hash"),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ],
                );
                assert!(output.status.success(), "{output:?}");
                assert_eq!(
                    output.stdout,
                    b"GroupBy(key),\"wmean(value,weight)\"\na,\"3,00\"\nb,\"5,50\"\n"
                );
                assert!(output.stderr.is_empty());
            }
        }
        for args in [
            vec!["--csv-out", "-z", "-s", "-g1", "wmean", "2:3"],
            vec![
                "--csv-out",
                "--output-delimiter=|",
                "-s",
                "-g1",
                "wmean",
                "2:3",
            ],
        ] {
            let output = invoke(&args, b"bad\n", regular_file);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("line 1"));
        }
    }
}

#[test]
fn sorted_weighted_full_rows_keep_the_first_or_other_operations_representative() {
    let input = b"b\t5\t2\tother\ta\xff\0raw\na\t2\t1\tfirst\t\xff\na\t6\t3\theavy\tmore\n";
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for (extra, expected) in [
                (
                    vec![],
                    b"a\t2\t1\tfirst\t\xff\t5\nb\t5\t2\tother\ta\xff\0raw\t5\n".as_slice(),
                ),
                (
                    vec!["last", "4"],
                    b"a\t6\t3\theavy\tmore\t5\theavy\nb\t5\t2\tother\ta\xff\0raw\t5\tother\n"
                        .as_slice(),
                ),
            ] {
                let mut args = vec!["--full", "-s", "-g1", "wmean", "2:3"];
                args.extend(extra);
                let output = invoke_in(
                    &args,
                    input,
                    regular_file,
                    &[
                        ("FASTMASH_GROUPING", "sort"),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ],
                );
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, expected);
                assert_eq!(output.stderr, FULL_WARNING);
            }
        }
    }
}

#[test]
fn sorting_and_replay_retain_weighted_validation_and_range_failures() {
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for grouping in ["sort", "hash", "hash:restart=1"] {
                for (input, status, diagnostic) in [
                    (b"a\t5\t1\nb\tNA\t-1\n".as_slice(), 1, "line 2 field 3"),
                    (b"a\t5\t1\nb\tNA\tinf\n", 1, "line 2 field 3"),
                    (b"a\t5\t1\nb\tNA\toops\n", 1, "line 2 field 3"),
                    (b"a\t5\t1\nb\tNA\n", 1, "field 3 requested"),
                    (b"a\t5\t1\nb\tbad\t0\n", 1, "line 2 field 2"),
                    (b"a\tinf\t1\na\tbad\t1\n", 1, "line 2 field 2"),
                    (b"a\t-nan\t1\na\tbad\t1\n", 1, "line 2 field 2"),
                    (b"a\t5\t1\nb\t2\t0\n", 1, "group keys: field 1='b'"),
                    (b"a\t5\t1\nb\tNA\t1\n", 1, "group keys: field 1='b'"),
                    (b"a\t5\t1\nb\tNA\t0\nb\t2\tNA\n", 1, "group keys: field 1='b'"),
                    (b"a\t2\t1e4932\na\t-2\t1e4932\n", 77, "product overflow"),
                    (b"a\t1e-4000\t1e-4000\n", 77, "nonzero product rounded to zero"),
                    (b"a\t1e4932\t1\na\t1e4932\t1\na\t-1e4932\t2\n", 77, "weighted total overflow"),
                    (b"a\t0\t1e4932\na\t0\t1e4932\n", 77, "weight total overflow"),
                    (b"a\t0x1.fffffffffffffffep+16383\t0x1p-1\na\t0x1.fffffffffffffffep+16383\t0x1p-65\n", 77, "final division overflow"),
                ] {
                    let output = invoke_in(&["--narm", "-s", "-g1", "wmean", "2:3"], input, regular_file, &[("FASTMASH_GROUPING", grouping), ("FASTMASH_PIPE_GROUPING", "hash"), ("FASTMASH_SORT_MEMORY_BYTES", memory)]);
                    assert_eq!(output.status.code(), Some(status), "{grouping} {memory}: {output:?}");
                    assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic), "{diagnostic}: {output:?}");
                    assert!(!output.stdout.ends_with(b"nan\n"));
                }
                let output = invoke_in(
                    &["-s", "-g1", "wmean", "2:3"],
                    b"a\tinf\t0\nb\t-nan\t0\na\t5\t2\nb\t9\t1\n",
                    regular_file,
                    &[
                        ("FASTMASH_GROUPING", grouping),
                        ("FASTMASH_PIPE_GROUPING", "hash"),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ],
                );
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, b"a\t5\nb\t9\n");
                assert!(output.stderr.is_empty());
            }
        }
    }
}

#[test]
fn weighted_sorted_headers_and_key_errors_keep_established_output_timing() {
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for (input, expected) in [
                (b"".as_slice(), b"".as_slice()),
                (
                    b"key\tvalue\tweight\n",
                    b"GroupBy(key),\"wmean(value,weight)\"\n",
                ),
            ] {
                let output = invoke_in(
                    &[
                        "--csv-out",
                        "-H",
                        "-s",
                        "-g",
                        "key",
                        "wmean",
                        "value:weight",
                    ],
                    input,
                    regular_file,
                    &[
                        ("FASTMASH_GROUPING", "sort"),
                        ("FASTMASH_SORT_MEMORY_BYTES", memory),
                    ],
                );
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, expected);
                assert!(output.stderr.is_empty());
            }
            let output = invoke_in(
                &["--narm", "-s", "-g1,4", "wmean", "2:3"],
                b"a\t5\t1\tx\nb\tNA\t2\n",
                regular_file,
                &[
                    ("FASTMASH_GROUPING", "hash"),
                    ("FASTMASH_PIPE_GROUPING", "hash"),
                    ("FASTMASH_SORT_MEMORY_BYTES", memory),
                ],
            );
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(
                String::from_utf8_lossy(&output.stderr)
                    .contains("field 4 requested, line 2 has only 3 fields")
            );
        }
    }
}

#[test]
fn sorted_named_weighted_empty_domains_complete_native_and_external_reports() {
    for regular_file in [false, true] {
        for grouping in ["sort", "hash:restart=1"] {
            for csv in [false, true] {
                for csv_out in [false, true] {
                    for full in [false, true] {
                        for (requests, labels) in [
                            (vec!["wmean", "value:weight"], "wmean(value,weight)"),
                            (
                                vec!["sum", "value", "wmean", "value:weight"],
                                "sum(value)\twmean(value,weight)",
                            ),
                            (
                                vec!["wmean", "value:weight", "sum", "value"],
                                "wmean(value,weight)\tsum(value)",
                            ),
                            (
                                vec!["dotprod", "value:weight", "wmean", "value:weight"],
                                "dotprod(value,weight)\twmean(value,weight)",
                            ),
                            (
                                vec!["wmean", "value:weight", "dotprod", "value:weight"],
                                "wmean(value,weight)\tdotprod(value,weight)",
                            ),
                        ] {
                            let mut args = vec!["-H", "-s", "-g", "key"];
                            if csv {
                                args.push("--csv-in");
                            } else {
                                args.push("-C");
                            }
                            if csv_out {
                                args.push("--csv-out");
                            }
                            if full {
                                args.push("--full");
                            }
                            args.extend(requests);
                            let header = if csv {
                                b"key,value,weight\n".as_slice()
                            } else {
                                b"key\tvalue\tweight\n"
                            };
                            let mut inputs = vec![b"".as_slice(), header];
                            if !csv {
                                inputs.push(b"# no measurements\n# still empty");
                            }
                            for input in inputs {
                                let output = invoke_in(
                                    &args,
                                    input,
                                    regular_file,
                                    &[
                                        ("FASTMASH_GROUPING", grouping),
                                        ("FASTMASH_SORT_MEMORY_BYTES", "1"),
                                    ],
                                );
                                assert!(
                                    output.status.success(),
                                    "{args:?} {grouping} {input:?}: {output:?}"
                                );
                                let expected = if input == header {
                                    let leading = if full {
                                        "key\tvalue\tweight"
                                    } else {
                                        "GroupBy(key)"
                                    };
                                    let fields = format!("{leading}\t{labels}");
                                    if csv_out {
                                        fields
                                            .split('\t')
                                            .map(|field| {
                                                if field.contains(',') {
                                                    format!("\"{field}\"")
                                                } else {
                                                    field.to_owned()
                                                }
                                            })
                                            .collect::<Vec<_>>()
                                            .join(",")
                                            + "\n"
                                    } else {
                                        fields + "\n"
                                    }
                                } else {
                                    String::new()
                                };
                                assert_eq!(output.stdout, expected.as_bytes(), "{args:?}");
                                assert_eq!(output.stderr, if full { FULL_WARNING } else { b"" });
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn sorted_named_empty_completion_keeps_header_errors_and_legacy_commands() {
    for regular_file in [false, true] {
        for requests in [
            vec!["wmean", "value:weight"],
            vec!["dotprod", "value:weight", "wmean", "value:weight"],
            vec!["wmean", "value:weight", "dotprod", "value:weight"],
        ] {
            for input in [
                b"key\twrong\tweight\n".as_slice(),
                b"\n",
                b"key\t\tweight\n",
            ] {
                let mut args = vec!["-H", "-s", "-g", "key"];
                args.extend_from_slice(&requests);
                let output =
                    invoke_in(&args, input, regular_file, &[("FASTMASH_GROUPING", "sort")]);
                assert_eq!(output.status.code(), Some(1), "{output:?}");
                assert!(output.stdout.is_empty());
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("not found"),
                    "{output:?}"
                );
            }
            let mut args = vec!["-s", "-g", "key"];
            args.extend(requests);
            let output = invoke(&args, b"", regular_file);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(String::from_utf8_lossy(&output.stderr).contains("header"));
        }
        // These Operations use native sorting on both platforms, including
        // its self-contained missing Input header diagnostic.
        for operation in ["sum", "mean"] {
            let output = invoke_in(
                &["-H", "-s", "-g", "key", operation, "value"],
                b"",
                regular_file,
                &[("FASTMASH_GROUPING", "sort")],
            );
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr, b"fastmash: missing input header for named grouping key\n",
                "{output:?}"
            );
        }
    }
    let directory = temp_dir::TempDir::new("weighted-empty-read-error");
    for requests in [
        vec!["wmean", "value:weight"],
        vec!["wmean", "value:weight", "dotprod", "value:weight"],
    ] {
        let output = candidate()
            .args(["-H", "-s", "-g", "key"])
            .args(requests)
            .env("FASTMASH_GROUPING", "sort")
            .stdin(fs::File::open(&directory.0).unwrap())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("read"),
            "{output:?}"
        );
    }
    // A present positional key and data retain the mixed route on each platform.
    for requests in [
        vec!["wmean", "2:3", "dotprod", "2:3"],
        vec!["dotprod", "2:3", "wmean", "2:3"],
    ] {
        let mut args = vec!["-s", "-g1"];
        args.extend_from_slice(&requests);
        let output = invoke_in(
            &args,
            b"b\t5\t2\na\t10\t1\n",
            false,
            &[("FASTMASH_GROUPING", "sort"), ("FASTMASH_SORT_TRACE", "1")],
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            if requests[0] == "wmean" {
                b"a\t10\t10\nb\t5\t10\n".as_slice()
            } else {
                b"a\t10\t10\nb\t10\t5\n"
            }
        );
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .starts_with(&format!("sort route: {PAIRED_SORT_ROUTE}")),
            "{output:?}"
        );
    }
}

#[test]
fn weighted_sort_write_failures_reclaim_temporary_storage_and_prevent_success() {
    for csv in [false, true] {
        weighted_sort_cleanup(csv);
    }
}

fn weighted_sort_cleanup(csv: bool) {
    let directory = temp_dir::TempDir::new("weighted-spill-cleanup");
    let sentinel = directory.0.join("sentinel");
    let input = directory.0.join("input");
    fs::write(&sentinel, b"untouched").unwrap();
    let bytes = if csv {
        b"b,5,2\na,2,1\na,6,3\n".as_slice()
    } else {
        b"b\t5\t2\na\t2\t1\na\t6\t3\n"
    };
    fs::write(&input, bytes.repeat(40)).unwrap();
    let mut args = vec!["-s", "-g1", "wmean", "2:3"];
    if csv {
        args.push("--csv");
    }
    let mut command = candidate();
    command
        .args(&args)
        .env("FASTMASH_GROUPING", "sort")
        .env("FASTMASH_SORT_MEMORY_BYTES", "1")
        .env("TMPDIR", &directory.0)
        .stdin(fs::File::open(&input).unwrap());
    // SAFETY: these async-signal-safe operations affect only the child. A zero
    // file-size allowance exercises the maintained temporary-write failure path.
    unsafe {
        command.pre_exec(|| {
            let limit = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::signal(libc::SIGXFSZ, libc::SIG_IGN) == libc::SIG_ERR {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("sort temporary I/O error"));
    assert_eq!(fs::read(&sentinel).unwrap(), b"untouched");
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 2);
    for memory in ["67108864", "1"] {
        let output = candidate()
            .args(&args)
            .env("FASTMASH_GROUPING", "sort")
            .env("FASTMASH_SORT_MEMORY_BYTES", memory)
            .stdin(fs::File::open(&input).unwrap())
            .full_stdout(true)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("write error"));
    }
}

#[test]
fn files_and_pipes_keep_the_same_weighted_trace_and_complete_validation() {
    for regular_file in [false, true] {
        for (args, input, status, expected, diagnostic) in [
            (vec!["wmean", "1:2"], "10\t1\n20\t3", 0, "17.5\n", ""),
            (
                vec!["--narm", "wmean", "1:2"],
                "NA\t1\n10\t1\n20\t3",
                0,
                "17.5\n",
                "",
            ),
            (
                vec!["wmean", "1:2"],
                "18446744073709551616\t1\n1\t1\n-18446744073709551616\t1",
                0,
                "0\n",
                "",
            ),
            (
                vec!["wmean", "1:2"],
                "inf\t1\nbad\t1",
                1,
                "",
                "line 2 field 1",
            ),
            (
                vec!["--narm", "wmean", "1:2"],
                "5\t1\nNA\t-1",
                1,
                "",
                "line 2 field 2",
            ),
            (
                vec!["wmean", "1:2"],
                "2\t1e4932",
                77,
                "",
                "product overflow",
            ),
        ] {
            let output = invoke(&args, input.as_bytes(), regular_file);
            assert_eq!(
                output.status.code(),
                Some(status),
                "{args:?}: {:?}",
                output.stderr
            );
            assert_eq!(output.stdout, expected.as_bytes());
            if diagnostic.is_empty() {
                assert!(output.stderr.is_empty());
            } else {
                assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
            }
        }
    }
}

#[test]
fn a_large_weighted_pipe_is_collected_without_retained_observations() {
    let mut child = candidate()
        .args(["wmean", "1:2", "wmean", "1:2"])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();
    // The input producer reuses this block; the Command must consume a million
    // Records and report both requests, independent of their encounter count.
    let block = b"3\t2\n".repeat(4096);
    let mut input = child.stdin.take().unwrap();
    for _ in 0..256 {
        input.write_all(&block).unwrap();
    }
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{:?}", output.stderr);
    assert_eq!(output.stdout, b"3\t3\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn real_output_failure_prevents_weighted_success() {
    let mut child = candidate()
        .args(["wmean", "1:2"])
        .stdin(Stdio::piped())
        .full_stdout(true)
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"10\t1\n20\t3\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("write error"));
}

#[test]
fn named_adjacent_reports_agree_over_real_files_and_pipes() {
    for regular_file in [false, true] {
        let output = invoke(
            &[
                "-H",
                "-g",
                "key",
                "--result-name=2:weighted",
                "count",
                "value",
                "wmean",
                "value:weight,other:weight",
                "wmean",
                "weight:weight",
                "mean",
                "value",
            ],
            b"key\tvalue\tweight\tother\na\t10\t1\t4\na\t20\t3\t8\nb\t5\t2\t9\na\t30\t1\t2",
            regular_file,
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(output.stdout, b"GroupBy(key)\tcount(value)\tweighted\twmean(other,weight)\twmean(weight,weight)\tmean(value)\na\t2\t17.5\t7\t2.5\t15\nb\t1\t5\t9\t2\t5\na\t1\t30\t2\t1\t30\n");
        assert!(output.stderr.is_empty(), "{output:?}");
        let output = invoke(
            &["--narm", "-g1", "wmean", "2:3"],
            b"a\t10\t1\nb'\xff\tNA\t1\na\t20\t3\n",
            regular_file,
        );
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"a\t10\nb'\xff\t");
        assert_eq!(output.stderr, b"fastmash: wmean fields 2:3 have no positive retained contribution weight\ngroup keys: field 1='b\\'\\377'\n");
    }
}

#[test]
fn weighted_full_reports_keep_the_existing_representative_and_copy_labels() {
    let input = b"key\tvalue\tweight\tnote\na\t10\t1\tfirst\na\t20\t3\theavy\nb\t5\t2\tother\n";
    for regular_file in [false, true] {
        for (extra, expected) in [
            (vec![], b"key\tvalue\tweight\tnote\taverage\na\t10\t1\tfirst\t17.5\nb\t5\t2\tother\t5\n".as_slice()),
            (vec!["last", "note"], b"key\tvalue\tweight\tnote\taverage\tlast(note)\na\t20\t3\theavy\t17.5\theavy\nb\t5\t2\tother\t5\tother\n"),
            (vec!["max", "value"], b"key\tvalue\tweight\tnote\taverage\tmax(value)\na\t20\t3\theavy\t17.5\t20\nb\t5\t2\tother\t5\t5\n"),
        ] {
            let mut args = vec!["-H", "--full", "-g", "key", "--result-name=1:average", "wmean", "value:weight"];
            args.extend(extra);
            let output = invoke(&args, input, regular_file);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, expected);
            assert_eq!(output.stderr, FULL_WARNING);
        }
        let output = invoke(
            &[
                "--full",
                "--header-out",
                "--result-name=1:average",
                "wmean",
                "2:3",
            ],
            b"a\xff\t10\t1\t\0raw\t\nb\t20\t3\tshort",
            regular_file,
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"field-1\tfield-2\tfield-3\tfield-4\tfield-5\taverage\na\xff\t10\t1\t\0raw\t\t17.5\n"
        );
        assert_eq!(output.stderr, FULL_WARNING);
    }
}

#[test]
fn weighted_full_reports_reject_absent_keys_without_hiding_prior_results() {
    for regular_file in [false, true] {
        for (args, input, stdout, diagnostic) in [
            (
                vec!["--full", "-g3", "wmean", "1:2"],
                b"5\t2\n".as_slice(),
                b"".as_slice(),
                b"fastmash: invalid input: field 3 requested, line 1 has only 2 fields\n"
                    .as_slice(),
            ),
            (
                vec!["--full", "--narm", "-g1,4", "wmean", "2:3"],
                b"a\t10\t1\tx\nb\tNA\t3\n",
                b"a\t10\t1\tx\t10\n",
                b"fastmash: invalid input: field 4 requested, line 2 has only 3 fields\n",
            ),
        ] {
            let output = invoke(&args, input, regular_file);
            assert_eq!(output.status.code(), Some(1));
            assert_eq!(output.stdout, stdout);
            assert_eq!(output.stderr, [FULL_WARNING, diagnostic].concat());
        }
    }
}

#[test]
fn csv_weighted_reports_cover_whole_adjacent_and_sorted_format_combinations() {
    let input =
        b"\"site,name\",value:raw,\"weight,raw\",other\r\nb,5,2,9\na,10,1,4\n\"a\",20,3,8\nb,7,2,5";
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for csv_out in [false, true] {
                for (group, sort) in [(false, false), (true, false), (true, true)] {
                    let mut args = vec![
                        if csv_out { "--csv" } else { "--csv-in" },
                        "-H",
                        "--result-name=2:other",
                        "wmean",
                        "value\\:raw:weight\\,raw,4:weight\\,raw,3:3,2:3",
                        "count",
                        "2",
                        "mean",
                        "2",
                    ];
                    if group {
                        args.extend(["-g", "site\\,name"]);
                    }
                    if sort {
                        args.push("-s");
                    }
                    let output = invoke_in(
                        &args,
                        input,
                        regular_file,
                        &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                    );
                    assert!(output.status.success(), "{args:?}: {output:?}");
                    let header: &[u8] = if csv_out {
                        if group {
                            b"\"GroupBy(site,name)\",\"wmean(value:raw,weight,raw)\",other,\"wmean(weight,raw,weight,raw)\",\"wmean(value:raw,weight,raw)\",count(value:raw),mean(value:raw)\n".as_slice()
                        } else {
                            b"\"wmean(value:raw,weight,raw)\",other,\"wmean(weight,raw,weight,raw)\",\"wmean(value:raw,weight,raw)\",count(value:raw),mean(value:raw)\n"
                        }
                    } else if group {
                        b"GroupBy(site,name)\twmean(value:raw,weight,raw)\tother\twmean(weight,raw,weight,raw)\twmean(value:raw,weight,raw)\tcount(value:raw)\tmean(value:raw)\n"
                    } else {
                        b"wmean(value:raw,weight,raw)\tother\twmean(weight,raw,weight,raw)\twmean(value:raw,weight,raw)\tcount(value:raw)\tmean(value:raw)\n"
                    };
                    let data = match (csv_out, group, sort) {
                        (true, false, _) => b"11.75,7,2.25,11.75,4,10.5\n".as_slice(),
                        (false, false, _) => b"11.75\t7\t2.25\t11.75\t4\t10.5\n",
                        (true, true, false) => b"b,5,9,2,5,1,5\na,17.5,7,2.5,17.5,2,15\nb,7,5,2,7,1,7\n",
                        (false, true, false) => b"b\t5\t9\t2\t5\t1\t5\na\t17.5\t7\t2.5\t17.5\t2\t15\nb\t7\t5\t2\t7\t1\t7\n",
                        (true, true, true) => b"a,17.5,7,2.5,17.5,2,15\nb,6,7,2,6,2,6\n",
                        (false, true, true) => b"a\t17.5\t7\t2.5\t17.5\t2\t15\nb\t6\t7\t2\t6\t2\t6\n",
                    };
                    assert_eq!(output.stdout, [header, data].concat());
                    assert!(output.stderr.is_empty());
                }
            }
        }
    }
}

#[test]
fn csv_sort_spill_retains_original_order_for_binary80_sensitive_pairs() {
    // 2^64+1 ties to 2^64. a cancels to zero; b cancels first, retaining
    // nearest-even 1/3. c is exact beyond binary64. Alternate quote spellings
    // decode to the same key; zero contributors separate the active Spill runs.
    let mut input = b"key,kind,value,weight\na,x,18446744073709551616,1\nb,x,18446744073709551616,1\nc,y,9007199254740993,1\n".to_vec();
    input.extend(b"\"b\",x,0,0\na,x,0,0\nc,y,0,0\n".repeat(40));
    input.extend(b"\"a\",x,1,1\nb,x,-18446744073709551616,1\n");
    input.extend(b"c,y,0,0\n\"a\",x,0,0\nb,x,0,0\n".repeat(40));
    input.extend(b"a,x,-18446744073709551616,1\n\"b\",x,1,1");
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for locale in ["C", "en_US.UTF-8"] {
                for csv in ["--csv-in", "--csv"] {
                    let output = invoke_in(
                        &[
                            csv,
                            "--header-in",
                            "-s",
                            "-g",
                            "kind,key",
                            "--format=%.20g",
                            "wmean",
                            "value:weight",
                        ],
                        &input,
                        regular_file,
                        &[("LC_ALL", locale), ("FASTMASH_SORT_MEMORY_BYTES", memory)],
                    );
                    assert!(output.status.success(), "{output:?}");
                    assert_eq!(
                        output.stdout,
                        if csv == "--csv" {
                            b"x,a,0\nx,b,0.33333333333333333334\ny,c,9007199254740993\n".as_slice()
                        } else {
                            b"x\ta\t0\nx\tb\t0.33333333333333333334\ny\tc\t9007199254740993\n"
                        }
                    );
                    assert!(output.stderr.is_empty());
                }
            }
        }
    }
}

#[test]
fn weighted_csv_encoding_preserves_full_context_and_unsorted_text_reports() {
    let csv = b"\"key,\"\"x\",\"value\nraw\",weight,\"note,\"\"x\",tail\r\n\"a,\"\"x\",10,1,\"first\r\n\xff\0raw\",\r\n\"a,\"\"x\",20,3,heavy,last\nb,5,2,other,";
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for sort in [false, true] {
                for (extra, expected) in [
                    (vec![], b"\"key,\"\"x\",\"value\nraw\",weight,\"note,\"\"x\",tail,\"wmean(value\nraw,weight)\",\"average,\"\"daily\"\"\nreading\"\n\"a,\"\"x\",10,1,\"first\r\n\xff\0raw\",\"\",17.5,2.5\nb,5,2,other,\"\",5,2\n".as_slice()),
                    (vec!["last", "4"], b"\"key,\"\"x\",\"value\nraw\",weight,\"note,\"\"x\",tail,\"wmean(value\nraw,weight)\",\"average,\"\"daily\"\"\nreading\",\"last(note,\"\"x)\"\n\"a,\"\"x\",20,3,heavy,last,17.5,2.5,heavy\nb,5,2,other,\"\",5,2,other\n".as_slice()),
                ] {
                    let mut args = vec!["--csv", "-H", "--full", "-g1", "--result-name=2:average,\"daily\"\nreading", "wmean", "2:3,3:3"];
                    if sort { args.push("-s"); }
                    args.extend(extra);
                    let output = invoke_in(&args, csv, regular_file, &[("FASTMASH_SORT_MEMORY_BYTES", memory)]);
                    assert!(output.status.success(), "{output:?}");
                    assert_eq!(output.stdout, expected);
                    assert_eq!(output.stderr, FULL_WARNING);
                }
            }
        }
        for group in [false, true] {
            let mut args = vec![
                "--csv-out",
                "-H",
                "--full",
                "--result-name=1:average,\"daily\"\nreading",
                "wmean",
                "2:3",
            ];
            if group {
                args.push("-g1");
            }
            let output = invoke(&args, b"key\tvalue\tweight\tnote\ttail\na,\"x\t10\t1\tfirst\r\xff\0raw\t\na,\"x\t20\t3\theavy\tlast\n", regular_file);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"key,value,weight,note,tail,\"average,\"\"daily\"\"\nreading\"\n\"a,\"\"x\",10,1,\"first\r\xff\0raw\",\"\",17.5\n");
            assert_eq!(output.stderr, FULL_WARNING);
        }
        for csv_in in [false, true] {
            let mut args = vec!["--csv-out", "-H", "-g1", "--format=%.2f", "wmean", "2:3"];
            if csv_in {
                args.push("--csv-in");
            }
            let input = if csv_in {
                b"key,value,weight\na,\"10,5\",1\na,\"20,5\",3\n".as_slice()
            } else {
                b"key\tvalue\tweight\na\t10,5\t1\na\t20,5\t3\n"
            };
            let output = invoke_in(&args, input, regular_file, &[("LC_ALL", "de_DE.UTF-8")]);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                output.stdout,
                b"GroupBy(key),\"wmean(value,weight)\"\na,\"18,00\"\n"
            );
            assert!(output.stderr.is_empty());
        }
    }
}

#[test]
fn csv_weighted_failures_keep_original_logical_and_physical_locations_through_spill() {
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for sort in [false, true] {
                for (last, diagnostic) in [
                    (
                        "a,NA,-1,unused\n",
                        "invalid wmean contribution weight in line 3 field 3: expected a finite nonnegative weight\nCSV record 3 starts at physical line 5\n",
                    ),
                    (
                        "a,bad,0,unused\n",
                        "invalid numeric value in line 3 field 2: 'bad'\nCSV record 3 starts at physical line 5\n",
                    ),
                    (
                        "a,NA\n",
                        "invalid input: field 3 requested, line 3 has only 2 fields\nCSV record 3 starts at physical line 5\n",
                    ),
                ] {
                    let input =
                        format!("key,\"value\nraw\",weight,note\r\nz,5,2,\"two\nlines\"\n{last}");
                    let mut args = vec!["--csv", "-H", "--narm", "-g1", "wmean", "2:3"];
                    if sort {
                        args.push("-s");
                    }
                    let output = invoke_in(
                        &args,
                        input.as_bytes(),
                        regular_file,
                        &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                    );
                    assert_eq!(output.status.code(), Some(1), "{output:?}");
                    assert_eq!(
                        output.stderr,
                        [b"fastmash: ", diagnostic.as_bytes()].concat()
                    );
                }
                for (pairs, status, diagnostic) in [
                    ("2,1e4932\n-2,1e4932\n", 77, "product overflow"),
                    ("1e-4000,1e-4000\n", 77, "nonzero product rounded to zero"),
                    (
                        "1e4932,1\n1e4932,1\n-1e4932,2\n",
                        77,
                        "weighted total overflow",
                    ),
                    ("0,1e4932\n0,1e4932\n", 77, "weight total overflow"),
                    (
                        "0x1.fffffffffffffffep+16383,0x1p-1\n0x1.fffffffffffffffep+16383,0x1p-65\n",
                        77,
                        "final division overflow",
                    ),
                    ("NA,1\n2,0\n", 1, "group keys: field 1='a'"),
                ] {
                    let input: String = pairs.lines().map(|pair| format!("a,{pair}\n")).collect();
                    let mut args = vec!["--csv", "--narm", "-g1", "wmean", "2:3"];
                    if sort {
                        args.push("-s");
                    }
                    let output = invoke_in(
                        &args,
                        input.as_bytes(),
                        regular_file,
                        &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                    );
                    assert_eq!(output.status.code(), Some(status), "{output:?}");
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains(diagnostic),
                        "{output:?}"
                    );
                    if status == 1 {
                        assert!(!String::from_utf8_lossy(&output.stderr).contains("CSV record"));
                    }
                }
                let mut args = vec!["--csv", "-H", "--narm", "-g1,4", "wmean", "2:3"];
                if sort {
                    args.push("-s");
                }
                let output = invoke_in(
                    &args,
                    b"key,\"value\nraw\",weight,note\nz,5,2,\"two\nlines\"\na,NA,2\n",
                    regular_file,
                    &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                );
                assert_eq!(output.status.code(), Some(1), "{output:?}");
                assert_eq!(output.stderr, b"fastmash: invalid input: field 4 requested, line 3 has only 3 fields\nCSV record 3 starts at physical line 5\n");
            }
        }
    }
}

#[test]
fn csv_weighted_syntax_and_option_errors_validate_the_complete_input() {
    for regular_file in [false, true] {
        for memory in ["67108864", "1"] {
            for sort in [false, true] {
                for first in [
                    "inf,1,valid\n",
                    "nan,1,valid\n",
                    "NA,1,valid\n",
                    "5,0,valid\n",
                ] {
                    let input = format!("{first}NA,0,\"x\ny\"!\n");
                    let mut args = vec!["--csv", "--narm", "wmean", "1:2"];
                    if sort {
                        args.extend(["-s", "-g3"]);
                    }
                    let output = invoke_in(
                        &args,
                        input.as_bytes(),
                        regular_file,
                        &[("FASTMASH_SORT_MEMORY_BYTES", memory)],
                    );
                    assert_eq!(output.status.code(), Some(1), "{output:?}");
                    assert_eq!(output.stderr, b"fastmash: invalid CSV: byte after closing quote at physical line 3 byte 3; record 2 starts at physical line 2\n");
                }
            }
        }
        for extra in ["-z", "-W", "-t,", "-C", "--output-delimiter=|"] {
            let output = invoke(&["--csv", extra, "wmean", "1:2"], b"bad\n", regular_file);
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(!String::from_utf8_lossy(&output.stderr).contains("CSV record"));
        }
    }
}
