#![forbid(unsafe_code)]
//! Built-in sorting, replay and private spill storage at the command boundary.
#[path = "support/descriptor_process.rs"]
mod descriptor_process;
#[path = "support/executable.rs"]
#[allow(clippy::duplicate_mod)]
mod executable;
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;
#[path = "support/test_process.rs"]
mod test_process;

use nix::sys::signal::{Signal, kill};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::fd::OwnedFd,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const SPILL: &[u8] = b"sort spill: private run opened\n";
const FULL_WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";

static EXECUTABLE: Mutex<Weak<temp_dir::TempDir>> = Mutex::new(Weak::new());

fn standalone(pool: &Mutex<Weak<temp_dir::TempDir>>) -> Arc<temp_dir::TempDir> {
    let mut owner = pool.lock().unwrap();
    if let Some(root) = owner.upgrade() {
        return root;
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = Arc::new(temp_dir::TempDir::new(&format!(
        "native-sort-executable-{id}"
    )));
    // Publish only after copying closes its writable descriptor. Every live
    // fixture retains this owner, so another copy cannot overlap its children.
    fs::copy(executable::fastmash(), root.0.join("fastmash")).unwrap();
    *owner = Arc::downgrade(&root);
    root
}

#[test]
fn staged_executable_lives_until_its_last_fixture_owner() {
    let pool = Mutex::new(Weak::new());
    let first = standalone(&pool);
    let second = standalone(&pool);
    assert!(Arc::ptr_eq(&first, &second));
    let path = first.0.clone();
    assert!(path.join("fastmash").is_file());
    drop(first);
    assert!(path.join("fastmash").is_file());
    drop(second);
    assert!(!path.try_exists().unwrap());
    let fresh = standalone(&pool);
    assert_ne!(fresh.0, path);
    let path = fresh.0.clone();
    drop(fresh);
    assert!(!path.try_exists().unwrap());
}

struct Fixture {
    root: temp_dir::TempDir,
    binary: PathBuf,
    _executable: Arc<temp_dir::TempDir>,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = temp_dir::TempDir::new(name);
        // A standalone executable also exercises the built-in fallback on
        // Linux for Operations that ordinarily use the Sort supervisor.
        let executable = standalone(&EXECUTABLE);
        let binary = executable.0.join("fastmash");
        fs::create_dir(root.0.join("spill")).unwrap();
        Self {
            root,
            binary,
            _executable: executable,
        }
    }

    fn command(&self, args: &[&str], memory: &str) -> Command {
        let mut command = test_process::bounded(&self.binary, 15);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/nonexistent")
            .env("LC_ALL", "C")
            .env("TMPDIR", self.root.0.join("spill"))
            .env("FASTMASH_GROUPING", "sort")
            .env("FASTMASH_SORT_MEMORY_BYTES", memory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    }

    fn invoke_closed(&self, command: &Command, input: &[u8], file: bool) -> Output {
        let capture = temp_dir::TempDir::create(self.root.0.join("closed-output")).unwrap();
        let (reader, mut writer) = std::io::pipe().unwrap();
        let stdin: OwnedFd = if file {
            let path = self.root.0.join("input");
            fs::write(&path, input).unwrap();
            File::open(path).unwrap().into()
        } else {
            reader.into()
        };
        let files = [
            stdin,
            File::create_new(capture.0.join("stdout")).unwrap().into(),
            File::create_new(capture.0.join("stderr")).unwrap().into(),
        ];
        // Bind directly to the candidate. A Rust launcher before it could
        // repair closed standard descriptors during its own startup.
        let arguments: Vec<_> = command
            .get_args()
            .skip(2)
            .map(|s| s.to_str().unwrap())
            .collect();
        let environment = child_environment(command);
        thread::scope(|scope| {
            let writer = scope.spawn(move || {
                if file {
                    Ok(())
                } else {
                    writer.write_all(input)
                }
            });
            let output = descriptor_process::invoke_bound(
                descriptor_process::BoundCommand {
                    binary: self.binary.as_os_str(),
                    arguments: &arguments,
                    files,
                    closed: 2,
                    environment: &environment,
                },
                &capture.0,
                1 << 20,
                1 << 20,
            )
            .unwrap()
            .into_output();
            if let Err(error) = writer.join().unwrap() {
                assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
            }
            output
        })
    }

    fn invoke(&self, command: &mut Command, input: &[u8], file: bool) -> Output {
        if file {
            let path = self.root.0.join("input");
            fs::write(&path, input).unwrap();
            command.stdin(File::open(path).unwrap());
            command.output().unwrap()
        } else {
            let mut child = command.spawn().unwrap();
            let mut pipe = child.stdin.take().unwrap();
            thread::scope(|scope| {
                let writer = scope.spawn(move || pipe.write_all(input));
                let output = child.wait_with_output().unwrap();
                if let Err(error) = writer.join().unwrap() {
                    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
                }
                output
            })
        }
    }

    fn clean(&self) {
        assert_eq!(fs::read_dir(self.root.0.join("spill")).unwrap().count(), 0);
    }
}

fn exact(output: &Output, stdout: &[u8]) {
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, stdout, "{output:?}");
    let trace = String::from_utf8_lossy(&output.stderr);
    assert!(trace.starts_with("sort route: native sort"), "{output:?}");
    assert!(!trace.contains("system sort"), "{output:?}");
}

#[test]
#[cfg(target_os = "linux")]
fn native_merge_pipe_preserves_numerics_seeded_draws_headers_and_full_records() {
    let fixture = Fixture::new("native-merge-pipe-results");
    for (flags, header, end) in [
        (&[][..], true, b'\n'),
        (&["-z", "-i", "--full"][..], true, 0),
        (&["--header-out"][..], false, b'\n'),
    ] {
        let mut input = Vec::new();
        if header {
            input.extend_from_slice(b"key\tx\ty\tunused\tsequence");
            input.push(end);
        }
        for row in 0..800_000 {
            let key = ["B", "a", "b", "A"][row % 4];
            write!(&mut input, "{key}\t3\t4\tretained by full\t{row}").unwrap();
            input.push(end);
        }
        let mut args = flags.to_vec();
        if header {
            args.push("-H");
        }
        args.extend([
            "--seed", "17", "-s", "-g1", "rms", "2", "pcov", "2:3", "last", "5", "rand", "5",
            "rms", "5",
        ]);
        for file in [false, true] {
            let invoke = |limit| {
                let mut command = fixture.command(&args, "33554432");
                command
                    .env("FASTMASH_SORT_TRACE", "1")
                    .env("OMP_NUM_THREADS", "2")
                    .env("OMP_THREAD_LIMIT", limit);
                fixture.invoke(&mut command, &input, file)
            };
            let serial = invoke("1");
            assert!(serial.status.success(), "{serial:?}");
            let result_at = if flags.contains(&"--full") { 5 } else { 1 };
            for row in serial
                .stdout
                .split(|&byte| byte == end)
                .skip(1)
                .filter(|row| !row.is_empty())
            {
                // Constant x=3 and y=4 give RMS 3 and covariance 0 exactly.
                assert_eq!(
                    row.split(|&byte| byte == b'\t').nth(result_at),
                    Some(b"3".as_slice())
                );
                assert_eq!(
                    row.split(|&byte| byte == b'\t').nth(result_at + 1),
                    Some(b"0".as_slice())
                );
            }
            let piped = invoke("2");
            exact(&piped, &serial.stdout);
            assert!(piped.stderr.windows(SPILL.len()).any(|part| part == SPILL));
            assert!(
                String::from_utf8_lossy(&piped.stderr)
                    .contains("sort calculation: native merge pipe\n")
            );
            assert!(!String::from_utf8_lossy(&serial.stderr).contains("native merge pipe"));
            fixture.clean();
        }
    }
}

#[test]
#[cfg(target_os = "linux")]
fn native_merge_pipe_keeps_diagnostics_and_joins_after_calculation_failure() {
    let fixture = Fixture::new("native-merge-pipe-errors");
    for (bad, first) in [
        (b"ignored\t!\tbad\t4\t0\n".as_slice(), "3"),
        (b"!\n".as_slice(), "3"),
        (b"ignored\t!\tbad\tbad\t0\n".as_slice(), "4"),
    ] {
        let mut input = b"id\tkey\tx\ty\tsequence\n".to_vec();
        input.extend_from_slice(bad);
        for _ in 0..800_000 {
            input.extend_from_slice(b"ignored\tz\t3\t4\t0\n");
        }
        let args = ["-H", "-s", "-g2", "rms", first, "pcov", "3:4"];
        for file in [false, true] {
            let invoke = |limit| {
                let mut command = fixture.command(&args, "33554432");
                command
                    .env("FASTMASH_SORT_TRACE", "1")
                    .env("OMP_NUM_THREADS", "2")
                    .env("OMP_THREAD_LIMIT", limit);
                fixture.invoke(&mut command, &input, file)
            };
            let serial = invoke("1");
            let piped = invoke("2");
            assert_eq!(serial.status.code(), Some(1), "{serial:?}");
            assert_eq!(piped.status.code(), Some(1), "{piped:?}");
            assert_eq!(piped.stdout, serial.stdout);
            let diagnostics = |output: &Output| {
                output
                    .stderr
                    .split(|&byte| byte == b'\n')
                    .filter(|line| !line.starts_with(b"sort "))
                    .map(<[u8]>::to_vec)
                    .collect::<Vec<_>>()
            };
            assert_eq!(diagnostics(&piped), diagnostics(&serial));
            assert!(
                String::from_utf8_lossy(&piped.stderr)
                    .contains("sort calculation: native merge pipe\n")
            );
            fixture.clean();
        }
    }
}

#[test]
fn every_c_locale_calculation_sorts_natively_with_or_without_a_supervisor() {
    let mut fixture = Fixture::new("native-all-calculations");
    let standalone = fixture.binary.clone();
    let operations = [
        ("geomean", "2"),
        ("harmmean", "2"),
        ("ms", "2"),
        ("rms", "2"),
        ("pskew", "2"),
        ("sskew", "2"),
        ("pkurt", "2"),
        ("skurt", "2"),
        ("jarque", "2"),
        ("dpo", "2"),
        ("pcov", "2:3"),
        ("scov", "2:3"),
        ("ppearson", "2:3"),
        ("spearson", "2:3"),
        ("dotprod", "2:3"),
        ("rand", "2"),
    ];
    let a = b"a\t1\t4\na\t2\t3\na\t4\t2\na\t8\t1\n".repeat(3);
    let b = b"b\t3\t1\nb\t6\t2\nb\t9\t3\nb\t12\t4\n".repeat(3);
    let sorted = [a.as_slice(), b.as_slice()].concat();
    let input = [b.as_slice(), a.as_slice()].concat();
    for available in [true, false] {
        fixture.binary = if available {
            executable::fastmash().into()
        } else {
            standalone.clone()
        };
        for locale in ["C", "POSIX", "C.UTF-8", "C.utf8", "POSIX.UTF8"] {
            for file in [false, true] {
                for memory in ["67108864", "1"] {
                    for (operation, field) in operations {
                        // Mixed text and numerical results exercise stable ties as well
                        // as each formerly external calculation. The reference reads
                        // already grouped input in exactly the contribution order.
                        let args = [
                            "-g1",
                            "--seed=42",
                            operation,
                            field,
                            "sum",
                            "2",
                            "first",
                            "3",
                            "last",
                            "3",
                        ];
                        let mut reference = fixture.command(&args, memory);
                        reference.env("LC_ALL", locale);
                        let reference = fixture.invoke(&mut reference, &sorted, file);
                        assert!(reference.status.success(), "{operation}: {reference:?}");
                        let mut args = args.to_vec();
                        args.insert(0, "-s");
                        let mut command = fixture.command(&args, memory);
                        command
                            .env("LC_ALL", locale)
                            .env("FASTMASH_SORT_TRACE", "1");
                        let output = fixture.invoke(&mut command, &input, file);
                        exact(&output, &reference.stdout);
                        assert_eq!(
                            output.stderr.windows(SPILL.len()).any(|line| line == SPILL),
                            memory == "1",
                            "{operation}: {output:?}"
                        );
                        fixture.clean();
                    }
                }
            }
        }
    }
}

#[test]
fn migrated_calculations_keep_record_formats_and_numeric_crossings() {
    let fixture = Fixture::new("native-calculation-formats");
    let cases: &[(&[&str], &[u8], &[u8])] = &[
        (
            &["-sCH", "-gkey", "ms", "value"],
            b"#skip\nkey\tvalue\nb\t2\na\t3\n",
            b"GroupBy(key)\tms(value)\na\t9\nb\t4\n",
        ),
        (
            &["-sWi", "-g1", "ms", "2", "first", "3", "last", "3"],
            b"B 2 first\na 3 one\nb 4 last\nA 5 two\n",
            b"A\t17\tone\ttwo\nb\t10\tfirst\tlast\n",
        ),
        (&["-sz", "-g1", "ms", "2"], b"b\t2\0a\t3\0", b"a\t9\0b\t4\0"),
        (
            &["-s", "-t.", "-g1", "ms", "2"],
            b"b.2.5\na.3.5\n",
            b"a.9\nb.4\n",
        ),
        (
            &["-s", "-te", "-g1", "ms", "2"],
            b"b e2e2\na e3e2\n",
            b"a e9\nb e4\n",
        ),
        (
            &["-s", "-t1", "-g1", "ms", "2"],
            b"b1215\na1315\n",
            b"a19\nb14\n",
        ),
    ];
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            for &(args, input, expected) in cases {
                let mut command = fixture.command(args, memory);
                command.env("FASTMASH_SORT_TRACE", "1");
                let output = fixture.invoke(&mut command, input, file);
                exact(&output, expected);
                fixture.clean();
            }
        }
    }
}

#[test]
fn remaining_calculations_project_fields_without_changing_results() {
    let fixture = Fixture::new("native-advanced-fields");
    let input = include_bytes!("fixtures/native-sort-advanced.tsv");
    // Numeric rows: GNU 1.9, Linux x86-64, LC_ALL=C. Dot product is the
    // exact sum of x * (2*x), for x=1..8 and x=2..9. Seeded random choices
    // preserve the preceding implementation's results and contribution order.
    let cases: &[(&str, &str, &[u8])] = &[
        ("geomean", "2", b"a\t3.7643505995031\nb\t4.9541639996482\n"),
        ("harmmean", "2", b"a\t2.9434954007884\nb\t4.3740507702322\n"),
        ("ms", "2", b"a\t25.5\nb\t35.5\n"),
        ("rms", "2", b"a\t5.049752469181\nb\t5.9581876439065\n"),
        ("pskew", "2", b"a\t0\nb\t0\n"),
        ("sskew", "2", b"a\t0\nb\t0\n"),
        ("pkurt", "2", b"a\t-1.2380952380952\nb\t-1.2380952380952\n"),
        ("skurt", "2", b"a\t-1.2\nb\t-1.2\n"),
        ("jarque", "2", b"a\t0.77454465071599\nb\t0.77454465071599\n"),
        ("dpo", "2", b"a\t0.72013518580735\nb\t0.72013518580735\n"),
        ("pcov", "2:3", b"a\t10.5\nb\t10.5\n"),
        ("scov", "2:3", b"a\t12\nb\t12\n"),
        ("ppearson", "2:3", b"a\t1\nb\t1\n"),
        ("spearson", "2:3", b"a\t1\nb\t1\n"),
        ("dotprod", "2:3", b"a\t408\nb\t568\n"),
        ("rand", "2", b"a\t8\nb\t4\n"),
    ];
    for file in [false, true] {
        for memory in ["1048576", "1"] {
            for &(operation, selector, expected) in cases {
                let mut command =
                    fixture.command(&["--seed", "17", "-s", "-g1", operation, selector], memory);
                command.env("FASTMASH_SORT_TRACE", "1");
                let output = fixture.invoke(&mut command, input, file);
                exact(&output, expected);
                assert!(
                    output
                        .stderr
                        .starts_with(b"sort route: native sort of the selected fields\n"),
                    "{operation}: {output:?}"
                );
                assert_eq!(
                    output.stderr.windows(SPILL.len()).any(|line| line == SPILL),
                    memory == "1",
                    "{operation}: {output:?}"
                );
                fixture.clean();
            }
        }
    }
}

#[test]
fn projected_random_operations_share_draws_in_request_order() {
    let fixture = Fixture::new("native-projected-random");
    let input = include_bytes!("fixtures/native-sort-advanced.tsv");
    // The preceding implementation's fixed-seed result, with two random
    // Operations sharing draws and a numeric Operation reading the same field.
    for file in [false, true] {
        for memory in ["1048576", "1"] {
            let mut command = fixture.command(
                &[
                    "--seed", "17", "-s", "-g1", "rand", "2", "rand", "3", "rms", "2",
                ],
                memory,
            );
            command.env("FASTMASH_SORT_TRACE", "1");
            let output = fixture.invoke(&mut command, input, file);
            exact(
                &output,
                b"a\t8\t16\t5.049752469181\nb\t8\t12\t5.9581876439065\n",
            );
            assert!(
                output
                    .stderr
                    .starts_with(b"sort route: native sort of the selected fields\n"),
                "{output:?}"
            );
            assert_eq!(
                output.stderr.windows(SPILL.len()).any(|line| line == SPILL),
                memory == "1",
                "{output:?}"
            );
            fixture.clean();
        }
    }
}

#[test]
fn native_sort_keeps_stable_contributions_through_multiple_spill_levels() {
    let fixture = Fixture::new("native-stable-spill");
    let mut input = Vec::new();
    for row in 0..64 {
        let key = if row % 2 == 0 { "b" } else { "a" };
        input.extend_from_slice(format!("{key}\t1\trecord{row:02}\n").as_bytes());
    }
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            let mut command = fixture.command(
                &[
                    "-s", "-g1", "count", "2", "sum", "2", "rms", "2", "first", "3", "last", "3",
                ],
                memory,
            );
            command.env("FASTMASH_SORT_TRACE", "1");
            let output = fixture.invoke(&mut command, &input, file);
            exact(
                &output,
                b"a\t32\t32\t1\trecord01\trecord63\nb\t32\t32\t1\trecord00\trecord62\n",
            );
            assert_eq!(
                output.stderr.windows(SPILL.len()).any(|line| line == SPILL),
                memory == "1",
                "{output:?}"
            );
            let mut command = fixture.command(&["-s", "-g1", "sum", "2"], memory);
            command.env("FASTMASH_SORT_TRACE", "1");
            let output = fixture.invoke(&mut command, b"b\t4\na\t1e20\na\t1\na\t-1e20\n", file);
            // At this binary80 magnitude the intervening one rounds away.
            // Reordering the cancellation before it would instead yield one.
            exact(&output, b"a\t0\nb\t4\n");
            fixture.clean();
        }
    }
}

#[test]
fn complete_records_and_quoted_csv_selection_keep_original_ties() {
    let fixture = Fixture::new("native-record-selection");
    let cases: &[(&[&str], &[u8], &[u8])] = &[
        (
            &["--header-in", "-s", "-g", "key", "top:2", "value"],
            b"key\tvalue\tnote\ttail\nb\t1\tfirst\t\na\t10\ttwo\t\nb\t3\tbest\t\na\t10\tsecond\t\na\t10\tlate\t\n",
            b"a\t10\ttwo\t\na\t10\tsecond\t\nb\t3\tbest\t\nb\t1\tfirst\t\n",
        ),
        (
            &["--csv", "--header-in", "-s", "-g", "key", "top:2", "value"],
            b"key,value,note,tail\r\nb,1,first,\na,10,\"two\nlines\",\nb,3,best,\na,10,\"a\"\"second\",\na,10,late,\n",
            b"a,10,\"two\nlines\",\"\"\na,10,\"a\"\"second\",\"\"\nb,3,best,\"\"\nb,1,first,\"\"\n",
        ),
    ];
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            for &(args, input, expected) in cases {
                let mut command = fixture.command(args, memory);
                let output = fixture.invoke(&mut command, input, file);
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, expected);
                assert!(output.stderr.is_empty(), "{output:?}");
                fixture.clean();
            }
            let mut command = fixture.command(
                &["--full", "-s", "-g1", "wmean", "2:3", "dotprod", "2:3"],
                memory,
            );
            let output = fixture.invoke(
                &mut command,
                b"b\t5\t2\tb-first\na\t10\t1\ta-first\na\t20\t3\ta-last\n",
                file,
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                output.stdout,
                b"a\t10\t1\ta-first\t17.5\t70\nb\t5\t2\tb-first\t5\t10\n"
            );
            assert_eq!(output.stderr, FULL_WARNING);
            fixture.clean();
        }
    }
}

#[test]
fn native_weighted_pairs_preserve_csv_fields_and_builtin_locale_rules() {
    let fixture = Fixture::new("native-weighted-locale");
    for file in [false, true] {
        for memory in ["67108864", "1"] {
            let mut command = fixture.command(
                &[
                    "--csv",
                    "--header-in",
                    "-s",
                    "-g",
                    "key",
                    "wmean",
                    "value:weight",
                    "dotprod",
                    "value:weight",
                ],
                memory,
            );
            let output = fixture.invoke(
                &mut command,
                b"key,value,weight,note\nb,5,2,other\na,10,1,\"first\nrecord\"\na,20,3,\"last,record\"\n",
                file,
            );
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"a,17.5,70\nb,5,10\n");
            assert!(output.stderr.is_empty(), "{output:?}");

            let mut command = fixture.command(&["-s", "-g1", "wmean", "2:3"], memory);
            command.env("LC_ALL", "de_DE.UTF-8");
            let output = fixture.invoke(&mut command, b"b\t5,5\t2\na\t1,5\t1\na\t3,5\t3\n", file);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"a\t3\nb\t5,5\n");
            assert!(output.stderr.is_empty(), "{output:?}");
            fixture.clean();
        }
    }
}

#[test]
fn hash_and_replayed_input_keep_exact_results_before_forced_spill() {
    let fixture = Fixture::new("native-replay-spill");
    // Held pipe blocks grant the sorter their existing memory as they are
    // released. Project enough short Records to exceed that block as well.
    let input = b"b\t5\na\t3\nb\t1\na\t1\n".repeat(8_000);
    for file in [false, true] {
        for (grouping, memory, restart) in [
            ("hash", "67108864", false),
            ("hash:restart=1", "65536", true),
        ] {
            for (requests, expected) in [
                (vec!["sum", "2"], b"a\t32000\nb\t48000\n".as_slice()),
                (vec!["sum", "2", "ms", "2"], b"a\t32000\t5\nb\t48000\t13\n"),
                (
                    vec!["sum", "2", "dotprod", "2:2"],
                    b"a\t32000\t80000\nb\t48000\t208000\n",
                ),
            ] {
                let mut args = vec!["-s", "-g1"];
                args.extend(requests);
                let mut command = fixture.command(&args, memory);
                command
                    .env("FASTMASH_GROUPING", grouping)
                    .env("FASTMASH_PIPE_GROUPING", "hash")
                    .env("FASTMASH_SORT_TRACE", "1");
                let output = fixture.invoke(&mut command, &input, file);
                assert!(output.status.success(), "{output:?}");
                assert_eq!(output.stdout, expected);
                let trace = String::from_utf8_lossy(&output.stderr);
                assert!(trace.contains("hash first"), "{output:?}");
                assert_eq!(trace.contains("holding the input"), !file, "{output:?}");
                assert_eq!(trace.contains("sorting instead"), restart, "{output:?}");
                assert_eq!(trace.contains("private run opened"), restart, "{output:?}");
                fixture.clean();
            }
        }
    }
}

#[test]
fn spill_storage_errors_are_reported_only_when_storage_is_needed() {
    let fixture = Fixture::new("native-temporary-error");
    let absent = fixture.root.0.join("absent");
    let not_directory = fixture.root.0.join("ordinary-file");
    fs::write(&not_directory, b"sentinel").unwrap();
    for temporary in [&absent, &not_directory] {
        for csv in [false, true] {
            for memory in ["67108864", "1"] {
                let mut args = vec!["-s", "-g1", "ms", "2"];
                if csv {
                    args.insert(0, "--csv");
                }
                let mut command = fixture.command(&args, memory);
                command.env("TMPDIR", temporary);
                let input = if csv {
                    b"b,5\na,3\n".as_slice()
                } else {
                    b"b\t5\na\t3\n"
                };
                let output = fixture.invoke(&mut command, input, false);
                if memory == "1" {
                    assert_eq!(output.status.code(), Some(1), "{output:?}");
                    assert!(output.stdout.is_empty());
                    assert!(
                        String::from_utf8_lossy(&output.stderr)
                            .starts_with("fastmash: sort temporary I/O error: "),
                        "{output:?}"
                    );
                } else {
                    assert!(output.status.success(), "{output:?}");
                    assert_eq!(
                        output.stdout,
                        if csv {
                            b"a,9\nb,25\n".as_slice()
                        } else {
                            b"a\t9\nb\t25\n"
                        }
                    );
                    assert!(output.stderr.is_empty());
                }
            }
        }
    }
    assert!(!absent.exists());
    assert_eq!(fs::read(not_directory).unwrap(), b"sentinel");
    fixture.clean();
}

#[test]
fn unseeded_random_sort_uses_native_entropy_without_a_sort_supervisor() {
    let fixture = Fixture::new("native-unseeded-sort");
    let mut command = fixture.command(&["-s", "-g1", "rand", "2"], "1");
    command.env("FASTMASH_SORT_TRACE", "1");
    let output = fixture.invoke(&mut command, b"b\t5\na\t10\nb\t5\na\t10\n", false);
    // Each Group contains only one value, so its random choice has an exact
    // expectation while still requiring the unseeded generator's entropy.
    exact(&output, b"a\t10\nb\t5\n");
    assert!(output.stderr.windows(SPILL.len()).any(|line| line == SPILL));
    fixture.clean();
}

#[test]
fn text_and_csv_spill_keep_exact_reports_with_terminal_output() {
    let fixture = Fixture::new("native-spill-terminal");
    for csv in [false, true] {
        let mut args = vec!["-s", "-g1", "wmean", "2:3", "dotprod", "2:3"];
        if csv {
            args.insert(0, "--csv");
        }
        for stderr_terminal in [false, true] {
            let mut command = fixture.command(&args, "1");
            command.env("FASTMASH_SORT_TRACE", "1");
            let input = if csv {
                b"b,5,2\na,10,1\na,20,3\n".as_slice()
            } else {
                b"b\t5\t2\na\t10\t1\na\t20\t3\n"
            };
            let output = terminal::output(&mut command, input, true, stderr_terminal);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(
                output.stdout,
                if csv {
                    b"a,17.5,70\nb,5,10\n".as_slice()
                } else {
                    b"a\t17.5\t70\nb\t5\t10\n"
                }
            );
            assert!(output.stderr.windows(SPILL.len()).any(|line| line == SPILL));
            fixture.clean();
        }
    }
}

fn child_environment(command: &Command) -> Vec<(&str, &str)> {
    command
        .get_envs()
        .map(|(key, value)| (key.to_str().unwrap(), value.unwrap().to_str().unwrap()))
        .collect()
}

struct Reap(Option<Pid>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(pid) = self.0.take() {
            loop {
                match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
                    Ok(WaitStatus::StillAlive) => {
                        let _ = kill(pid, Signal::SIGKILL);
                        while let Err(nix::errno::Errno::EINTR) = waitpid(pid, None) {}
                        break;
                    }
                    Err(nix::errno::Errno::EINTR) => continue,
                    _ => break,
                }
            }
        }
    }
}

fn interrupted_spill(fixture: &Fixture, csv: bool, signal: i32) {
    let args = if csv {
        ["--csv", "-s", "-g1", "sum", "2"].as_slice()
    } else {
        ["-s", "-g1", "rms", "2"].as_slice()
    };
    let mut command = fixture.command(args, "1");
    command.env("FASTMASH_SORT_TRACE", "1");
    let (stdin, mut input_pipe) = std::io::pipe().unwrap();
    let (mut stdout, child_stdout) = std::io::pipe().unwrap();
    let (stderr, child_stderr) = std::io::pipe().unwrap();
    let arguments: Vec<_> = command.get_args().map(|s| s.to_str().unwrap()).collect();
    let environment = child_environment(&command);
    let bindings = descriptor_process::BoundCommand {
        binary: command.get_program(),
        arguments: &arguments,
        files: [stdin.into(), child_stdout.into(), child_stderr.into()],
        closed: 0,
        environment: &environment,
    };
    let mut child = Reap(Some(
        descriptor_process::spawn_bound(&bindings, &[Signal::SIGINT, Signal::SIGTERM]).unwrap(),
    ));
    drop(bindings);
    let pid = child.0.unwrap();
    let (sender, receiver) = mpsc::channel();
    let diagnostic = thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut output = Vec::new();
        let mut observed = false;
        loop {
            let begin = output.len();
            if reader.read_until(b'\n', &mut output).unwrap() == 0 {
                break;
            }
            if !observed && &output[begin..] == SPILL {
                observed = true;
                let _ = sender.send(());
            }
        }
        output
    });
    let input = if csv { b"b,5\n".as_slice() } else { b"b\t5\n" };
    // This oversized record spills immediately. Keep input open so the child
    // waits for the next record without starting another create/unlink window.
    input_pipe.write_all(input).unwrap();
    assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(()));
    fixture.clean();
    assert_eq!(
        waitpid(pid, Some(WaitPidFlag::WNOHANG)),
        Ok(WaitStatus::StillAlive)
    );
    // The reaping guard still holds this live child.
    assert_eq!(kill(pid, Signal::try_from(signal).unwrap()), Ok(()));
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        match waitpid(pid, Some(WaitPidFlag::WNOHANG)) {
            Ok(status @ (WaitStatus::Exited(..) | WaitStatus::Signaled(..))) => {
                child.0.take();
                break status;
            }
            Ok(WaitStatus::StillAlive) | Err(nix::errno::Errno::EINTR) => {}
            status => panic!("unexpected interrupted child status: {status:?}"),
        }
        assert!(Instant::now() < deadline, "interrupted child did not exit");
        thread::sleep(Duration::from_millis(5));
    };
    let WaitStatus::Signaled(_, actual_signal, _) = status else {
        panic!("{status:?}");
    };
    assert_eq!(actual_signal as i32, signal);
    drop(input_pipe);
    let mut output = Vec::new();
    stdout.read_to_end(&mut output).unwrap();
    let stdout = output;
    assert!(stdout.is_empty());
    let trace = diagnostic.join().unwrap();
    assert!(trace.windows(SPILL.len()).any(|line| line == SPILL));
    fixture.clean();
}

#[test]
fn interrupting_established_text_and_csv_spill_leaves_no_named_files() {
    let fixture = Fixture::new("native-interrupted-spill");
    for csv in [false, true] {
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGKILL] {
            interrupted_spill(&fixture, csv, signal);
        }
    }
}

#[test]
fn interrupted_spill_restores_defaults_from_an_ignoring_parent() {
    let launcher = test_process::bounded(std::env::current_exe().unwrap(), 30);
    let output = Command::new("/bin/sh")
        .args([
            "-c",
            r#"trap '' INT TERM && exec "$@""#,
            "native-sort-ignored-signals",
        ])
        .arg(launcher.get_program())
        .args(launcher.get_args())
        .args([
            "--exact",
            "interrupting_established_text_and_csv_spill_leaves_no_named_files",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed; 0 failed;"),
        "{output:?}"
    );
}

#[test]
fn native_missing_header_reports_input_failures_without_external_sort() {
    let fixture = Fixture::new("native-missing-header");
    for locale in ["C", "C.UTF-8", "en_US.UTF-8", "de_DE.UTF-8"] {
        for file in [false, true] {
            for input in [b"".as_slice(), b"#skip\n ;skip\n"] {
                for operation in ["sum", "geomean"] {
                    for closed_stdout in [false, true] {
                        let mut command =
                            fixture.command(&["-CH", "-s", "-g", "key", operation, "value"], "1");
                        command.env("LC_ALL", locale);
                        let output = if closed_stdout {
                            fixture.invoke_closed(&command, input, file)
                        } else {
                            fixture.invoke(&mut command, input, file)
                        };
                        assert_eq!(output.status.code(), Some(1), "{output:?}");
                        assert!(output.stdout.is_empty());
                        assert_eq!(
                            output.stderr,
                            b"fastmash: missing input header for named grouping key\n"
                        );
                    }
                }
                for requests in [
                    vec!["wmean", "value:weight"],
                    vec!["wmean", "value:weight", "dotprod", "value:weight"],
                ] {
                    let mut args = vec!["-CH", "-s", "-g", "key"];
                    args.extend_from_slice(&requests);
                    let mut command = fixture.command(&args, "1");
                    command.env("LC_ALL", locale);
                    let output = fixture.invoke(&mut command, input, file);
                    assert!(output.status.success(), "{output:?}");
                    assert!(output.stdout.is_empty());
                    assert!(output.stderr.is_empty());
                }
            }
        }
        for requests in [
            vec!["sum", "value"],
            vec!["geomean", "value"],
            vec!["wmean", "value:weight"],
            vec!["wmean", "value:weight", "dotprod", "value:weight"],
        ] {
            let mut args = vec!["-H", "-s", "-g", "key"];
            args.extend_from_slice(&requests);
            let mut command = fixture.command(&args, "1");
            command.env("LC_ALL", locale);
            command.stdin(File::open(&fixture.root.0).unwrap());
            let output = command.output().unwrap();
            assert_eq!(output.status.code(), Some(1), "{output:?}");
            assert!(output.stdout.is_empty());
            assert_eq!(output.stderr, b"fastmash: read error: Is a directory\n");
        }
    }
    fixture.clean();
}

#[test]
fn native_sorted_rmdup_missing_header_reports_input_failures_without_external_sort() {
    let fixture = Fixture::new("native-rmdup-missing-header");
    for locale in ["C", "C.UTF-8", "en_US.UTF-8", "de_DE.UTF-8"] {
        for file in [false, true] {
            for input in [b"".as_slice(), b"#skip\n ;skip\n"] {
                for closed_stdout in [false, true] {
                    let mut command = fixture.command(&["-sCH", "rmdup", "key"], "1");
                    command.env("LC_ALL", locale);
                    let output = if closed_stdout {
                        fixture.invoke_closed(&command, input, file)
                    } else {
                        fixture.invoke(&mut command, input, file)
                    };
                    assert_eq!(output.status.code(), Some(1), "{output:?}");
                    assert!(output.stdout.is_empty());
                    assert_eq!(
                        output.stderr,
                        b"fastmash: missing input header for named grouping key\n"
                    );
                }
            }
        }
        let mut command = fixture.command(&["-sH", "rmdup", "key"], "1");
        command.env("LC_ALL", locale);
        command.stdin(File::open(&fixture.root.0).unwrap());
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"fastmash: read error: Is a directory\n");
    }
    fixture.clean();
}
