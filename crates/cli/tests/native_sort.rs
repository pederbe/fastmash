//! Built-in sorting, replay and private spill storage at the command boundary.
#[path = "support/temp_dir.rs"]
mod temp_dir;
#[path = "support/terminal.rs"]
mod terminal;

use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    os::unix::process::{CommandExt, ExitStatusExt},
    process::{Child, Command, Output, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const SPILL: &[u8] = b"sort spill: private run opened\n";
const FULL_WARNING: &[u8] = b"fastmash: Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n";

struct Fixture {
    root: temp_dir::TempDir,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let root = temp_dir::TempDir::new(name);
        // A standalone executable also exercises the built-in fallback on
        // Linux for Operations that ordinarily use the companion.
        let binary = std::env::var_os("FASTMASH_TEST_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_fastmash").into());
        fs::copy(binary, root.0.join("fastmash")).unwrap();
        fs::create_dir(root.0.join("spill")).unwrap();
        Self { root }
    }

    fn command(&self, args: &[&str], memory: &str) -> Command {
        let mut command = Command::new(self.root.0.join("fastmash"));
        command
            .arg0("fastmash")
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
        // SAFETY: alarm is async-signal-safe and bounds the executed command
        // if a regression leaves it waiting for input or a child process.
        unsafe {
            command.pre_exec(|| {
                libc::alarm(15);
                Ok(())
            });
        }
        command
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
                    "-s", "-g1", "count", "2", "sum", "2", "first", "3", "last", "3",
                ],
                memory,
            );
            command.env("FASTMASH_SORT_TRACE", "1");
            let output = fixture.invoke(&mut command, &input, file);
            exact(
                &output,
                b"a\t32\t32\trecord01\trecord63\nb\t32\t32\trecord00\trecord62\n",
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
            let mut command = fixture.command(&["-s", "-g1", "sum", "2"], memory);
            command
                .env("FASTMASH_GROUPING", grouping)
                .env("FASTMASH_PIPE_GROUPING", "hash")
                .env("FASTMASH_SORT_TRACE", "1");
            let output = fixture.invoke(&mut command, &input, file);
            assert!(output.status.success(), "{output:?}");
            assert_eq!(output.stdout, b"a\t32000\nb\t48000\n");
            let trace = String::from_utf8_lossy(&output.stderr);
            assert!(trace.contains("hash first"), "{output:?}");
            assert_eq!(trace.contains("holding the input"), !file, "{output:?}");
            assert_eq!(trace.contains("sorting instead"), restart, "{output:?}");
            assert_eq!(trace.contains("private run opened"), restart, "{output:?}");
            fixture.clean();
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
                let mut args = vec!["-s", "-g1", "sum", "2"];
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
                            b"a,3\nb,5\n".as_slice()
                        } else {
                            b"a\t3\nb\t5\n"
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
fn unseeded_random_sort_uses_native_entropy_without_a_companion() {
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

struct Reap(Option<Child>);

impl Drop for Reap {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn interrupted_spill(fixture: &Fixture, csv: bool, signal: i32) {
    let args = if csv {
        ["--csv", "-s", "-g1", "sum", "2"].as_slice()
    } else {
        ["-s", "-g1", "sum", "2"].as_slice()
    };
    let mut command = fixture.command(args, "1");
    command.env("FASTMASH_SORT_TRACE", "1");
    if signal != libc::SIGKILL {
        // SAFETY: sigaction is async-signal-safe. The initialized action
        // installs the default in the child before the executable starts.
        unsafe {
            command.pre_exec(move || {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = libc::SIG_DFL;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    let mut child = Reap(Some(command.spawn().unwrap()));
    let child_ref = child.0.as_mut().unwrap();
    let stderr = child_ref.stderr.take().unwrap();
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
    let input = if csv {
        b"b,5\na,3\n".as_slice()
    } else {
        b"b\t5\na\t3\n"
    };
    // Keep the input open after these records. The child must stay alive
    // waiting for more input after its first successfully anonymized run.
    let mut input_pipe = child_ref.stdin.take().unwrap();
    input_pipe.write_all(input).unwrap();
    assert_eq!(receiver.recv_timeout(Duration::from_secs(5)), Ok(()));
    fixture.clean();
    assert!(child_ref.try_wait().unwrap().is_none());
    let pid = i32::try_from(child_ref.id()).unwrap();
    // SAFETY: pid names the live child held by the reaping guard.
    assert_eq!(unsafe { libc::kill(pid, signal) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child_ref.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "interrupted child did not exit");
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(status.signal(), Some(signal));
    drop(input_pipe);
    let mut stdout = Vec::new();
    child_ref
        .stdout
        .take()
        .unwrap()
        .read_to_end(&mut stdout)
        .unwrap();
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

#[cfg(target_os = "macos")]
#[test]
fn native_missing_header_reports_input_failures_without_external_sort() {
    let fixture = Fixture::new("native-missing-header");
    for file in [false, true] {
        for operation in ["sum", "geomean"] {
            for closed_stdout in [false, true] {
                let mut command =
                    fixture.command(&["-H", "-s", "-g", "key", operation, "value"], "1");
                if closed_stdout {
                    // SAFETY: close is async-signal-safe and changes only the
                    // child's standard output after Command installs its pipe.
                    unsafe {
                        command.pre_exec(|| {
                            if libc::close(1) != 0 {
                                return Err(std::io::Error::last_os_error());
                            }
                            Ok(())
                        });
                    }
                }
                let output = fixture.invoke(&mut command, b"", file);
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
            let mut args = vec!["-H", "-s", "-g", "key"];
            args.extend_from_slice(&requests);
            let mut command = fixture.command(&args, "1");
            let output = fixture.invoke(&mut command, b"", file);
            assert!(output.status.success(), "{output:?}");
            assert!(output.stdout.is_empty());
            assert!(output.stderr.is_empty());
        }
    }
    for operation in ["sum", "geomean"] {
        let mut command = fixture.command(&["-H", "-s", "-g", "key", operation, "value"], "1");
        command.stdin(File::open(&fixture.root.0).unwrap());
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(output.stderr, b"fastmash: read error: Is a directory\n");
    }
    fixture.clean();
}
