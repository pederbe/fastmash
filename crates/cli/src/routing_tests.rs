//! Source-routing checks. These run in the unit-test executable, not the CLI.
use super::*;
use std::cell::RefCell;

#[test]
fn groups_and_prepared_sorted_stream_share_the_same_session() {
    // A sorted stream whose Input header was not read before sorting reaches
    // `calculate` as unsorted input does.
    {
        let options = options(&["-g", "1", "sum", "2", "sum", "2", "median", "2"]);
        let mut arithmetic = numerics::Numerics::new(true).unwrap();
        let identity = arithmetic.identity();
        let mut bytes = Vec::new();
        let buffer = buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false).unwrap();
        let mut output = command_output::Results::new(buffer, &options);
        let prepared = None;
        assert!(
            calculate(
                &mut &b"a\t1\na\t3\nb\t2\nb\t4\n"[..],
                &mut output,
                &options,
                binding(&options),
                &mut arithmetic,
                None,
                prepared
            )
            .is_ok()
        );
        assert_eq!(arithmetic.identity(), identity);
        assert_eq!(
            command_output::complete(output.buffer, Ok(()), |_| Ok(()), &mut |_| true),
            0
        );
        assert_eq!(bytes, b"a\t4\t4\t2\nb\t6\t6\t3\n");
    }
}

#[test]
fn mixed_group_state_resets_after_missing_values_and_header_input() {
    let options = options(&[
        "--header-in",
        "--narm",
        "-g",
        "1",
        "sum",
        "2",
        "median",
        "2",
        "dotprod",
        "2:3",
        "collapse",
        "4",
        "first",
        "4",
        "last",
        "4",
        "count",
        "2",
        "range",
        "2",
    ]);
    let mut arithmetic = numerics::Numerics::new(true).unwrap();
    let identity = arithmetic.identity();
    let mut bytes = Vec::new();
    let buffer = buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Results::new(buffer, &options);
    // Independent compaction gives [1, 5] dot [3, 7] = 38; rowwise removal gives 35.
    let input = b"group\tx\ty\tlabel\na\t1\tNA\tred\na\tNA\t3\tNA\na\t5\t7\tblue\nb\tNA\tNA\tNA\nb\t2\t4\tgreen\nc\tNA\tNA\tNA\n";
    assert!(
        calculate(
            &mut &input[..],
            &mut output,
            &options,
            binding(&options),
            &mut arithmetic,
            None,
            None,
        )
        .is_ok()
    );
    assert_eq!(arithmetic.identity(), identity);
    assert_eq!(
        command_output::complete(output.buffer, Ok(()), |_| Ok(()), &mut |_| true),
        0
    );
    assert_eq!(
        bytes,
        b"a\t6\t3\t38\tred,blue\tred\tblue\t2\t4\nb\t2\t2\t8\tgreen\tgreen\tgreen\t1\t0\nc\t0\tnan\tnan\t\tN/A\tN/A\t0\tnan\n"
    );
}

#[test]
fn percentile_endpoint_selects_without_arithmetic() {
    let options = options(&["perc:100", "1"]);
    for (input, expected) in [
        ("3\n1\n2\n", "3"),
        ("-0\n", "-0"),
        ("-inf\n1\ninf\n", "inf"),
        ("-inf\n-inf\n", "-inf"),
        ("0\n-0\n", "-0"),
        ("-0\n0\n", "0"),
        ("1\n-nan\n", "-nan"),
        ("-nan\n1\n", "1"),
        // The existing non-total NaN order differs from a streaming maximum.
        ("3\nnan\n1\n", "1"),
        ("", "nan"),
    ] {
        let mut operations = requests(&options);
        // Both collection and summary now use the shared session-free path.
        let mut arithmetic = numerics::Numerics::new(false).unwrap();
        for (row, line) in input.lines().enumerate() {
            assert!(
                operations
                    .collect_line(
                        line.as_bytes(),
                        row as u64 + 1,
                        &options,
                        &mut arithmetic,
                        None
                    )
                    .is_ok()
            );
        }
        assert_eq!(
            operations
                .result(0, &mut arithmetic, &options)
                .ok()
                .unwrap(),
            expected.as_bytes()
        );
    }
}

#[test]
fn percentile_endpoint_resets_groups_and_renders_headers() {
    let options = options(&[
        "--header-in",
        "--header-out",
        "--narm",
        "-g",
        "1",
        "perc:100",
        "2",
    ]);
    let mut arithmetic = numerics::Numerics::new(true).unwrap();
    let identity = arithmetic.identity();
    let mut bytes = Vec::new();
    let buffer = buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Results::new(buffer, &options);
    let input = b"group\tvalue\na\t3\na\t1\nb\tNA\nb\tNA\nc\t-0\n";
    assert!(
        calculate(
            &mut &input[..],
            &mut output,
            &options,
            binding(&options),
            &mut arithmetic,
            None,
            None,
        )
        .is_ok()
    );
    assert_eq!(arithmetic.identity(), identity);
    assert_eq!(
        command_output::complete(output.buffer, Ok(()), |_| Ok(()), &mut |_| true),
        0
    );
    assert_eq!(
        bytes,
        b"GroupBy(group)\tperc:100(value)\na\t3\nb\tnan\nc\t-0\n"
    );
}

fn options(args: &[&str]) -> options::Options {
    let args: Vec<_> = args.iter().map(std::ffi::OsString::from).collect();
    let options::Action::Calculate(options) =
        options::parse(&args, b"fastmash", false).ok().unwrap()
    else {
        panic!("calculation")
    };
    *options
}
fn requests(options: &options::Options) -> OperationSet {
    OperationSet::new(
        grammar::command(&options.operands, options.group.as_ref())
            .ok()
            .unwrap()
            .operations,
    )
    .ok()
    .unwrap()
    .0
}
struct NoEntropy;
impl random::SeedSource for NoEntropy {
    fn seed(&mut self) -> Result<u32, ()> {
        panic!("unexpected entropy")
    }
}

#[test]
fn sigpipe_policy_in_isolated_test_processes() {
    use std::os::unix::process::ExitStatusExt;
    for case in [
        "blocked",
        "blocked-sorted",
        "help",
        "version",
        "grammar",
        "default",
        "ignored",
        "stderr",
        "sort-mask",
    ] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "routing_tests::sigpipe_child", "--nocapture"])
            .env("FASTMASH_UNIT_SIGNAL_CASE", case)
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        if ["default", "ignored", "stderr", "sort-mask"].contains(&case) {
            assert_eq!(output.status.signal(), Some(13), "{case}: {:?}", output);
        } else {
            assert!(output.status.success(), "{case}: {:?}", output);
        }
    }
}

#[test]
fn sigpipe_child() {
    let Ok(case) = std::env::var("FASTMASH_UNIT_SIGNAL_CASE") else {
        return;
    };
    let old = fastmash_sort_process::linux::mask(None).unwrap();
    if ["blocked", "blocked-sorted", "help", "version", "grammar"].contains(&case.as_str()) {
        fastmash_sort_process::linux::mask(Some(old | (1 << 12))).unwrap();
        let args: Vec<std::ffi::OsString> = match case.as_str() {
            "help" => vec!["--help".into()],
            "version" => vec!["--version".into()],
            "grammar" => vec!["not-an-operation".into(), "1".into()],
            "blocked-sorted" => ["-s", "-g", "1", "count", "2"].map(Into::into).to_vec(),
            _ => vec!["count".into(), "1".into()],
        };
        let mut bytes = Vec::new();
        let result = run(
            &mut &b"1\n"[..],
            &mut bytes,
            &args,
            b"fastmash",
            &mut |_| true,
        );
        match case.as_str() {
            "help" => {
                assert!(result.is_ok());
                assert!(!bytes.is_empty());
            }
            "version" => {
                assert!(result.is_ok());
                assert_eq!(
                    bytes,
                    concat!("fastmash ", env!("CARGO_PKG_VERSION"), "\n").as_bytes()
                );
            }
            "grammar" => {
                let error = result.err().unwrap();
                assert_ne!(error.message, b"blocked SIGPIPE is unsupported\n");
            }
            _ => {
                let error = result.err().unwrap();
                assert_eq!(error.status, 77);
                assert_eq!(error.message, b"blocked SIGPIPE is unsupported\n");
                assert!(bytes.is_empty());
            }
        }
        std::process::exit(0);
    }
    fastmash_sort_process::linux::mask(Some(old & !(1 << 12))).unwrap();
    let action = [u64::from(case == "ignored"), 0, 0, 0];
    // This test owns an isolated subprocess, so no concurrent test sees signal changes.
    assert_eq!(
        // SAFETY: rt_sigaction reads the 32-byte `action` and writes nothing.
        unsafe { linux::syscall(13, 13, action.as_ptr() as usize, 0, 8) },
        0
    );
    assert!(setup_sigpipe().is_ok());
    if case == "sort-mask" {
        // The same mask transition used by Session::start, with no candidate launch.
        let admitted = fastmash_sort_process::linux::mask(None).unwrap();
        fastmash_sort_process::linux::mask(Some(admitted | fastmash_sort_process::linux::CANCEL))
            .unwrap();
        fastmash_sort_process::linux::restore_mask(admitted);
        assert_eq!(fastmash_sort_process::linux::mask(None).unwrap(), admitted);
    }
    let mut fds = [0i32; 2];
    assert_eq!(
        // SAFETY: pipe2 writes two descriptors into `fds`.
        unsafe { linux::syscall(293, fds.as_mut_ptr() as usize, 0, 0, 0) },
        0
    );
    // SAFETY: closes this test's own new read end, which nothing owns.
    assert_eq!(unsafe { linux::syscall(3, fds[0] as usize, 0, 0, 0) }, 0);
    let target = if case == "stderr" { 2 } else { 1 };
    assert_eq!(
        // SAFETY: dup2 onto a standard stream of this isolated subprocess.
        unsafe { linux::syscall(33, fds[1] as usize, target, 0, 0) },
        target as isize
    );
    // SAFETY: write reads one byte from a static string.
    unsafe {
        linux::syscall(1, target, b"x".as_ptr() as usize, 1, 0);
    }
    panic!("default SIGPIPE must terminate before returning from write");
}

#[test]
fn grouped_partial_row_and_completed_rows_survive_later_failure() {
    let options = options(&["--narm", "-g", "1", "count", "2", "dotprod", "2:3"]);
    let mut arithmetic = numerics::Numerics::new(true).unwrap();
    let identity = arithmetic.identity();
    let mut bytes = Vec::new();
    let buffer = buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Results::new(buffer, &options);
    let result = calculate(
        &mut &b"a\t1\t2\na\t3\t4\nb\t5\t6\nb\t7\tNA\n"[..],
        &mut output,
        &options,
        binding(&options),
        &mut arithmetic,
        None,
        None,
    );
    let error = result.err().unwrap();
    assert_eq!(error.status, 1);
    assert_eq!(arithmetic.identity(), identity);
    let mut reports = Vec::new();
    let status = command_output::complete(output.buffer, Err(error), |_| Ok(()), &mut |error| {
        reports.push(error.message.clone());
        true
    });
    assert_eq!(status, 1);
    assert_eq!(reports.len(), 1);
    assert_eq!(bytes, b"a\t2\t14\nb\t2\t");
}

#[test]
fn signal_failures_preserve_diagnostics_and_stop_before_restoration() {
    for (mask, message) in [
        (Err(()), "unable to inspect runtime signal mask\n"),
        (Ok(1 << 12), "blocked SIGPIPE is unsupported\n"),
    ] {
        let error = setup_sigpipe_with(|| mask, || panic!("must not restore"))
            .err()
            .unwrap();
        assert_eq!(
            (error.status, error.message),
            (77, message.as_bytes().to_vec())
        );
    }
    let error = setup_sigpipe_with(|| Ok(0), || Err(())).err().unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(
        error.message,
        b"unable to restore runtime SIGPIPE disposition\n"
    );
}

#[test]
fn command_setup_precedes_numerics_entropy_and_sort_admission() {
    let operations = requests(&options(&["sum", "1", "rand", "1"]));
    for cause in [
        numerics::NumericFailure::Allocation,
        numerics::NumericFailure::Invariant,
        numerics::NumericFailure::TableIdentity,
    ] {
        let events = RefCell::new(Vec::new());
        let result = initialize_before_input(
            true,
            operations.needs(false).random,
            None,
            &mut NoEntropy,
            || {
                events.borrow_mut().push("signals");
                Ok(())
            },
            || {
                events.borrow_mut().push("session");
                Err::<(), _>(numeric_failure(cause))
            },
            || panic!("sort after failed session"),
        );
        assert_eq!(
            result.err().unwrap().message,
            numeric_failure(cause).message
        );
        assert_eq!(*events.borrow(), ["signals", "session"]);
    }
    let result = initialize_before_input(
        true,
        operations.needs(false).random,
        None,
        &mut NoEntropy,
        || Err(unsupported("signal first")),
        || panic!("numerics after signals"),
        || panic!("sort after signals"),
    );
    let _: Result<((), Option<random::RandomState>), _> = result;
}

#[test]
fn startup_keeps_one_value_for_empty_session_free_mixed_and_duplicate_requests() {
    for args in [
        vec!["count", "1"],
        vec!["rand", "1"],
        vec!["sum", "1", "sum", "1", "rand", "1"],
    ] {
        let operations = requests(&options(&args));
        for sorted in [false, true] {
            let events = RefCell::new(Vec::new());
            let needs = false;
            let (value, _) = initialize_before_input(
                sorted,
                operations.needs(false).random,
                Some(7),
                &mut NoEntropy,
                || {
                    events.borrow_mut().push("signals");
                    Ok(())
                },
                || {
                    events.borrow_mut().push("constructor");
                    Ok((42, needs))
                },
                || {
                    events.borrow_mut().push("sort");
                    Ok(())
                },
            )
            .ok()
            .unwrap();
            assert_eq!(value, (42, needs));
            assert_eq!(
                *events.borrow(),
                if sorted {
                    vec!["signals", "constructor", "sort"]
                } else {
                    vec!["signals", "constructor"]
                }
            );
        }
    }
    let (value, _) = initialize_before_input(
        false,
        false,
        None,
        &mut NoEntropy,
        || Ok(()),
        || numerics::Numerics::new(false).map_err(numeric_failure),
        || panic!("not sorted"),
    )
    .ok()
    .unwrap();
    assert!(!value.has_session());
}

#[test]
fn extrema_are_session_free_with_checked_values_and_mixed_routing() {
    for kind in ["min", "max", "absmin", "absmax", "range"] {
        let opts = options(&[kind, "1"]);
        let mut ops = requests(&opts);
        assert_eq!(ops.native_sort(), operation_set::NativeSort::Original);
        let mut arithmetic = numerics::Numerics::new(false).unwrap();
        for (i, record) in [b"-3.25".as_slice(), b"2.5".as_slice()].iter().enumerate() {
            assert!(
                ops.collect_line(record, i as u64 + 1, &opts, &mut arithmetic, None)
                    .is_ok()
            );
        }
        // A missing wider session would refuse if summarization still used it.
        assert!(ops.result(0, &mut arithmetic, &opts).is_ok());
    }
}

/// Each Operation runs on exactly the numerics its Kind declares: with them
/// every summary succeeds, and without any one of them it fails.
#[test]
fn every_operation_runs_on_exactly_its_declared_numeric_requirements() {
    let records: [&[u8]; 4] = [b"3\t5", b"4\t1", b"2.5\t7", b"9\t2"];
    let run = |args: &[&str], needs: numerics::Requirements| -> Result<(), Failure> {
        let opts = options(args);
        let mut ops = requests(&opts);
        let mut arithmetic = numerics::Numerics::with(needs).map_err(numeric_failure)?;
        for (i, record) in records.iter().enumerate() {
            ops.collect_line(record, i as u64 + 1, &opts, &mut arithmetic, None)?;
        }
        ops.result(0, &mut arithmetic, &opts).map(drop)
    };
    let mut names: Vec<String> = operation::PLAIN_OPERATIONS
        .iter()
        .map(|(name, _)| String::from_utf8(name.to_vec()).unwrap())
        .collect();
    names.extend(["percentile:90", "trimmean:0.2"].map(String::from));
    let mut checked = 0;
    for name in &names {
        let field = if name.ends_with("cov") || name.ends_with("pearson") || name == "dotprod" {
            "1:2"
        } else {
            "1"
        };
        let args = [name.as_str(), field];
        // Line operations and rand are outside summaries.
        let opts = options(&args);
        let Some(kind) = grammar::command(&opts.operands, opts.group.as_ref())
            .ok()
            .and_then(|command| command.operations.first().map(|request| request.kind))
        else {
            continue;
        };
        if kind.is_line() || kind == Kind::Rand {
            continue;
        }
        let needs = requests(&opts).needs(false).numerics;
        assert!(run(&args, needs).is_ok(), "{name} with {needs:?}");
        if needs.square_root {
            let fewer = numerics::Requirements {
                square_root: false,
                ..needs
            };
            assert!(run(&args, fewer).is_err(), "{name} without square root");
        }
        if needs.mean_math {
            let fewer = numerics::Requirements {
                mean_math: false,
                ..needs
            };
            assert!(run(&args, fewer).is_err(), "{name} without mean math");
        }
        checked += 1;
    }
    assert!(checked >= 40, "{checked}");
}
fn binding(options: &options::Options) -> binding::Binding<'static> {
    let command = grammar::command(&options.operands, options.group.as_ref())
        .ok()
        .unwrap();
    binding::Binding::new(b"fastmash", command.operations, command.keys)
        .ok()
        .unwrap()
}
