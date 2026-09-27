//! Source-routing checks. These run in the unit-test executable, not the CLI.
use super::*;
use std::cell::RefCell;

#[test]
fn shared_conversion_matches_independent_operations_across_rows_and_resets() {
    let opts = options(&[
        "--narm", "ms", "1", "sum", "2", "sum", "1", "mean", "1", "median", "1", "q1", "1", "min",
        "1", "max", "2", "pvar", "1", "svar", "1", "max", "1",
    ]);
    let mut shared = requests(&opts);
    let mut independent = requests(&opts);
    link_shared_fields(&mut shared);
    let mut math = numerics::Numerics::new(false).unwrap();
    for group in [
        [b"2\t7".as_slice(), b"NA\t3", b"5\tNA"],
        [b"NA\tNA".as_slice(), b"-3\t4", b"1\t2"],
    ] {
        for (line, row) in group.into_iter().enumerate() {
            collect(row, &mut shared, line as u64 + 1, &opts, &mut math, None)
                .ok()
                .unwrap();
            collect(
                row,
                &mut independent,
                line as u64 + 1,
                &opts,
                &mut math,
                None,
            )
            .ok()
            .unwrap();
            for index in 0..shared.len() {
                assert_eq!(shared[index].count, independent[index].count);
                // Compare results through the same summarization path, including
                // sample/dispersion followers whose own storage intentionally stays empty.
                let actual = summarize_at(
                    &mut shared,
                    index,
                    &mut math,
                    b',',
                    &opts.presentation,
                    false,
                )
                .ok()
                .unwrap();
                let expected = summarize_at(
                    &mut independent,
                    index,
                    &mut math,
                    b',',
                    &opts.presentation,
                    false,
                )
                .ok()
                .unwrap();
                assert_eq!(actual, expected);
            }
        }
        for op in shared.iter_mut().chain(&mut independent) {
            op.reset();
        }
    }
}

#[test]
fn shared_conversion_preserves_interleaved_errors_and_partial_updates() {
    for (args, input, message, counts) in [
        (
            vec!["sum", "1", "min", "3", "max", "2", "max", "1"],
            b"2\tbad".as_slice(),
            b"invalid input: field 3 requested, line 1 has only 2 fields\n".as_slice(),
            vec![1, 0, 0, 0],
        ),
        (
            vec!["sum", "1", "max", "2", "min", "3", "max", "1"],
            b"2\tbad".as_slice(),
            b"invalid numeric value in line 1 field 2: 'bad'\n".as_slice(),
            vec![1, 0, 0, 0],
        ),
    ] {
        let opts = options(&args);
        let mut ops = requests(&opts);
        link_shared_fields(&mut ops);
        let mut math = numerics::Numerics::new(false).unwrap();
        let error = collect(input, &mut ops, 1, &opts, &mut math, None)
            .err()
            .unwrap();
        assert_eq!(error.message, message);
        assert_eq!(ops.iter().map(|op| op.count).collect::<Vec<_>>(), counts);
    }
    let opts = options(&["sum", "1", "median", "1", "max", "1"]);
    let mut ops = requests(&opts);
    link_shared_fields(&mut ops);
    let mut math = numerics::Numerics::new(false).unwrap();
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let error = collect(b"2", &mut ops, 1, &opts, &mut math, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(ops[0].count, 1);
    assert_eq!(ops[2].count, 0);
}

#[test]
fn path_fields_use_fallible_text_storage_without_a_numerical_session() {
    let opts = options(&[
        "dirname", "1", "basename", "1", "extname", "1", "barename", "1",
    ]);
    let mut ops = requests(&opts);
    let mut math = numerics::Numerics::new(false).unwrap();
    ops[0].text.replace(b"old").unwrap();
    ops[0].text.fail_next_growth();
    let error = collect(
        b"long-directory/file.tar.gz",
        &mut ops,
        1,
        &opts,
        &mut math,
        None,
    )
    .err()
    .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(ops[0].text.output(), Some(b"old".as_slice()));
    collect(b"a/file.tar.gz", &mut ops, 1, &opts, &mut math, None)
        .ok()
        .unwrap();
    assert!(!math.has_session());
    assert_eq!(
        ops.iter()
            .map(|op| op.text.output().unwrap())
            .collect::<Vec<_>>(),
        [b"a".as_slice(), b"file.tar.gz", b"tar.gz", b"file"]
    );
}

#[test]
fn checksums_collect_without_a_numerical_session() {
    let opts = options(&[
        "md5", "1", "sha1", "1", "sha224", "1", "sha256", "1", "sha384", "1", "sha512", "1",
    ]);
    let mut ops = requests(&opts);
    let mut math = numerics::Numerics::new(false).unwrap();
    collect(b"a\0b", &mut ops, 1, &opts, &mut math, None)
        .ok()
        .unwrap();
    assert!(!math.has_session());
    assert_eq!(
        ops.iter()
            .map(|op| op.text.output().unwrap().len())
            .collect::<Vec<_>>(),
        [32, 40, 56, 64, 96, 128]
    );
}

thread_local! {
    static PARSED: RefCell<std::collections::VecDeque<Option<fastmash_numeric_contract::Value80>>> = RefCell::default();
}
pub(super) fn parsed_value(
    value: fastmash_numeric_contract::Value80,
) -> fastmash_numeric_contract::Value80 {
    PARSED.with(|values| values.borrow_mut().pop_front().flatten().unwrap_or(value))
}

#[test]
fn parsed_signaling_nan_stops_before_mutation_but_preserves_paired_left() {
    let snan = fastmash_numeric_contract::Value80::from_raw(fastmash_numeric_contract::Raw80::new(
        0x7fff,
        0x8000_0000_0000_0001,
    ))
    .unwrap();
    for kind in [
        "sum", "median", "min", "absmin", "absmax", "range", "mode", "madraw", "mad",
    ] {
        let options = options(&[kind, "1"]);
        let mut operations = requests(&options);
        let mut arithmetic = numerics::Numerics::new(false).unwrap();
        PARSED.with(|values| values.borrow_mut().push_back(Some(snan)));
        let error = collect(b"1", &mut operations, 1, &options, &mut arithmetic, None)
            .err()
            .unwrap();
        assert_eq!(
            (error.status, error.message),
            (70, b"internal numerical invariant failure\n".to_vec())
        );
        assert_eq!(operations[0].count, 0);
        assert!(operations[0].samples.as_slice().is_empty());
        assert!(
            numerics::Numerics::value80(operations[0].value)
                .same_bits(fastmash_numeric_contract::Value80::exact_u64(0))
        );
    }
    let options = options(&["dotprod", "1:2"]);
    let mut operations = requests(&options);
    let mut arithmetic = numerics::Numerics::new(true).unwrap();
    PARSED.with(|values| values.borrow_mut().extend([None, Some(snan)]));
    let error = collect(b"1\t2", &mut operations, 1, &options, &mut arithmetic, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 70);
    assert_eq!(
        operations[0].pair_samples.as_ref().unwrap().lengths(),
        (1, 0)
    );
}

#[test]
fn groups_and_prepared_sorted_stream_share_the_same_session() {
    for sorted in [false, true] {
        let options = options(&["-g", "1", "sum", "2", "sum", "2", "median", "2"]);
        let operations = requests(&options);
        let mut arithmetic = numerics::Numerics::new(true).unwrap();
        let identity = arithmetic.identity();
        let mut bytes = Vec::new();
        let buffer = headers::output::Buffered::new(&mut bytes, 4096, false).unwrap();
        let mut output = command_output::Header::new(buffer, &options);
        let prepared = if sorted {
            sorted_input::Header::Empty
        } else {
            sorted_input::Header::Unprepared
        };
        assert!(
            calculate(
                &mut &b"a\t1\na\t3\nb\t2\nb\t4\n"[..],
                &mut output,
                &options,
                CalculationFields {
                    program: b"fastmash",
                    operations,
                    names: &[],
                    keys: vec![1],
                    key_names: &[]
                },
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
    let buffer = headers::output::Buffered::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Header::new(buffer, &options);
    // Independent compaction gives [1, 5] dot [3, 7] = 38; rowwise removal gives 35.
    let input = b"group\tx\ty\tlabel\na\t1\tNA\tred\na\tNA\t3\tNA\na\t5\t7\tblue\nb\tNA\tNA\tNA\nb\t2\t4\tgreen\nc\tNA\tNA\tNA\n";
    assert!(
        calculate(
            &mut &input[..],
            &mut output,
            &options,
            CalculationFields {
                program: b"fastmash",
                operations: requests(&options),
                names: &[],
                keys: vec![1],
                key_names: &[],
            },
            &mut arithmetic,
            None,
            sorted_input::Header::Unprepared,
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
                collect(
                    line.as_bytes(),
                    &mut operations,
                    row as u64 + 1,
                    &options,
                    &mut arithmetic,
                    None
                )
                .is_ok()
            );
        }
        assert_eq!(
            summarize_at(
                &mut operations,
                0,
                &mut arithmetic,
                b',',
                &Default::default(),
                false
            )
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
    let buffer = headers::output::Buffered::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Header::new(buffer, &options);
    let input = b"group\tvalue\na\t3\na\t1\nb\tNA\nb\tNA\nc\t-0\n";
    assert!(
        calculate(
            &mut &input[..],
            &mut output,
            &options,
            CalculationFields {
                program: b"fastmash",
                operations: requests(&options),
                names: &[],
                keys: vec![1],
                key_names: &[]
            },
            &mut arithmetic,
            None,
            sorted_input::Header::Unprepared,
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

#[test]
fn arithmetic_failure_preserves_partial_rows_and_stdout_completion() {
    // Log allocation failure must stop collection after earlier operations.
    for grouped in [false, true] {
        let options = if grouped {
            options(&["-g", "1", "count", "2", "geomean", "2"])
        } else {
            options(&["count", "1", "dotprod", "1:2", "geomean", "1"])
        };
        let mut operations = requests(&options);
        let mut arithmetic = numerics::Numerics::new(true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        let tiny = fastmash_numeric_contract::Value80::from_raw(
            fastmash_numeric_contract::Raw80::new(0, 1),
        )
        .unwrap();
        PARSED.with(|values| values.borrow_mut().push_back(Some(tiny)));
        let input = if grouped {
            b"a\t1".as_slice()
        } else {
            b"1\t2".as_slice()
        };
        // Direct collection proves the first failed arithmetic operation stops later mutation.
        if grouped {
            mean_math::FAIL_ALLOCATION.with(|flag| flag.set(true));
            let error = collect(input, &mut operations, 1, &options, &mut arithmetic, None)
                .err()
                .unwrap();
            assert_eq!(error.status, 77);
            assert_eq!(operations[0].count, 1);
        } else {
            // Pair mismatch happens during finalization after count output is pending.
            PARSED.with(|values| values.borrow_mut().clear());
            operations[1]
                .pair_samples
                .as_mut()
                .unwrap()
                .push_left(integer(1))
                .unwrap();
            operations[1]
                .pair_samples
                .as_mut()
                .unwrap()
                .push_right(integer(2))
                .unwrap();
            operations[1]
                .pair_samples
                .as_mut()
                .unwrap()
                .push_right(integer(3))
                .unwrap();
            operations[0].count = 1;
            let mut bytes = Vec::new();
            let buffer = headers::output::Buffered::new(&mut bytes, 4096, false).unwrap();
            let mut output = command_output::Header::new(buffer, &options);
            use command_output::CommandOutput;
            let first = summarize_at(
                &mut operations,
                0,
                &mut arithmetic,
                b',',
                &Default::default(),
                false,
            )
            .ok()
            .unwrap();
            output.result(&first, b'\t');
            let error = summarize_at(
                &mut operations,
                1,
                &mut arithmetic,
                b',',
                &Default::default(),
                false,
            )
            .err()
            .unwrap();
            let mut closed = false;
            let status = command_output::complete(
                output.buffer,
                Err(error),
                |_| {
                    closed = true;
                    Ok(())
                },
                &mut |_| true,
            );
            assert_eq!(status, 1);
            assert!(closed);
            assert_eq!(bytes, b"1\t");
        }
    }
}

#[test]
fn paired_real_growth_failure_keeps_count_and_completed_left_side() {
    let options = options(&["count", "1", "dotprod", "1:2"]);
    let mut operations = requests(&options);
    let pair = operations[1].pair_samples.as_mut().unwrap();
    pair.push_left(integer(1)).unwrap();
    pair.reset(); // Left retains capacity; right's next append must allocate.
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let error = collect(b"1\t2", &mut operations, 1, &options, &mut arithmetic, None).unwrap_err();
    assert_eq!(error.status, 77);
    assert_eq!(operations[0].count, 1);
    assert_eq!(
        operations[1].pair_samples.as_ref().unwrap().lengths(),
        (1, 0)
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
fn requests(options: &options::Options) -> Vec<Operation> {
    operations(
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
    let operations = requests(&options);
    let mut arithmetic = numerics::Numerics::new(true).unwrap();
    let identity = arithmetic.identity();
    let mut bytes = Vec::new();
    let buffer = headers::output::Buffered::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Header::new(buffer, &options);
    let result = calculate(
        &mut &b"a\t1\t2\na\t3\t4\nb\t5\t6\nb\t7\tNA\n"[..],
        &mut output,
        &options,
        CalculationFields {
            program: b"fastmash",
            operations,
            names: &[],
            keys: vec![1],
            key_names: &[],
        },
        &mut arithmetic,
        None,
        sorted_input::Header::Unprepared,
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
            &operations,
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
        &operations,
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
                &operations,
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
        &[],
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
        assert!(projected_sort::supports(&ops));
        let mut arithmetic = numerics::Numerics::new(false).unwrap();
        for (i, record) in [b"-3.25".as_slice(), b"2.5".as_slice()].iter().enumerate() {
            assert!(collect(record, &mut ops, i as u64 + 1, &opts, &mut arithmetic, None).is_ok());
        }
        // A missing wider session would refuse if summarization still used it.
        assert!(
            summarize(
                &mut ops[0],
                &mut arithmetic,
                b',',
                &Default::default(),
                false
            )
            .is_ok()
        );
    }
}

#[test]
fn dispersion_shares_only_original_order_samples_and_propagates_growth_failure() {
    let opts = options(&[
        "pvar", "1", "median", "1", "sstdev", "1", "svar", "1", "pvar", "2",
    ]);
    let mut ops = requests(&opts);
    link_shared_fields(&mut ops);
    assert_eq!(
        ops.iter()
            .map(|op| op.dispersion_source)
            .collect::<Vec<_>>(),
        [None, None, Some(0), Some(0), None]
    );
    let mut arithmetic = numerics::Numerics::with_requirements(false, true).unwrap();
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let error = collect(b"2\t3", &mut ops, 1, &opts, &mut arithmetic, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(error.message, b"pvar sample memory allocation failed\n");
    for op in &mut ops {
        op.reset();
    }
    for (i, record) in [b"2\t3".as_slice(), b"0\t1", b"4\t5"].iter().enumerate() {
        assert!(collect(record, &mut ops, i as u64 + 1, &opts, &mut arithmetic, None).is_ok());
    }
    let before = summarize_at(
        &mut ops,
        0,
        &mut arithmetic,
        b',',
        &Default::default(),
        false,
    )
    .ok()
    .unwrap();
    assert_eq!(
        summarize_at(
            &mut ops,
            1,
            &mut arithmetic,
            b',',
            &Default::default(),
            false
        )
        .ok()
        .unwrap(),
        b"2"
    );
    assert_eq!(
        summarize_at(
            &mut ops,
            0,
            &mut arithmetic,
            b',',
            &Default::default(),
            false
        )
        .ok()
        .unwrap(),
        before
    );
    assert_eq!(
        summarize_at(
            &mut ops,
            2,
            &mut arithmetic,
            b',',
            &Default::default(),
            false
        )
        .ok()
        .unwrap(),
        b"2"
    );
    assert_eq!(
        summarize_at(
            &mut ops,
            3,
            &mut arithmetic,
            b',',
            &Default::default(),
            false
        )
        .ok()
        .unwrap(),
        b"4"
    );
}

#[test]
fn moments_share_only_immutable_samples_and_propagate_growth_failure() {
    let opts = options(&[
        "pskew", "1", "median", "1", "skurt", "1", "mad", "1", "pkurt", "1", "sskew", "2",
        "jarque", "1",
    ]);
    let mut ops = requests(&opts);
    link_shared_fields(&mut ops);
    assert_eq!(
        ops.iter().map(|o| o.sample_source).collect::<Vec<_>>(),
        [None, None, Some(0), None, Some(0), None, Some(0)]
    );
    let mut arithmetic = numerics::Numerics::with_requirements(true, true)
        .ok()
        .unwrap();
    for i in 0..4 {
        collect(
            format!("{}\t{}", i, i + 1).as_bytes(),
            &mut ops,
            i + 1,
            &opts,
            &mut arithmetic,
            None,
        )
        .ok()
        .unwrap();
    }
    assert_eq!(ops[0].samples.as_slice().len(), 4);
    assert!(ops[2].samples.as_slice().is_empty());
    assert_eq!(ops[2].count, 4);
    for op in &mut ops {
        op.reset();
    }
    assert!(ops[0].samples.as_slice().is_empty());
    let opts = options(&["count", "1", "pskew", "1", "skurt", "1"]);
    let mut ops = requests(&opts);
    link_shared_fields(&mut ops);
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let error = collect(b"2", &mut ops, 1, &opts, &mut arithmetic, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(ops[0].count, 1);
    assert!(ops[1].samples.as_slice().is_empty());
    assert_eq!(ops[2].count, 0);
}

#[test]
fn normality_owners_share_samples_and_report_injected_growth_failure() {
    for op in ["jarque", "dpo"] {
        let opts = options(&["count", "1", op, "1", "pskew", "1"]);
        let mut ops = requests(&opts);
        link_shared_fields(&mut ops);
        assert_eq!(ops[2].sample_source, Some(1));
        let mut math = numerics::Numerics::with_requirements(false, true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        samples::FAIL_GROWTH.with(|flag| flag.set(true));
        let error = collect(b"2", &mut ops, 1, &opts, &mut math, None)
            .err()
            .unwrap();
        assert_eq!(error.status, 77);
        assert_eq!(ops[0].count, 1);
        assert!(ops[1].samples.as_slice().is_empty());
        assert_eq!(ops[2].count, 0);
        assert!(!math.has_session());
    }
}
