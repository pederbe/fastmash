//! Checks of the Operation set's internal sharing, Group state and failure
//! paths, which its interface does not expose.
use super::*;
use std::cell::RefCell;

thread_local! {
    static PARSED: RefCell<std::collections::VecDeque<Option<fastmash_numeric_contract::Value80>>> = RefCell::default();
}
/// Replaces the next parsed field values in this thread, for injected failures.
pub(in super::super) fn parsed_value(
    value: fastmash_numeric_contract::Value80,
) -> fastmash_numeric_contract::Value80 {
    PARSED.with(|values| values.borrow_mut().pop_front().flatten().unwrap_or(value))
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
/// The Operations of `options`, not yet bound, so none share work.
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
fn args(items: &[&str]) -> Vec<std::ffi::OsString> {
    items.iter().map(std::ffi::OsString::from).collect()
}

fn completed_bytes(
    mut completion: GroupCompletion<'_>,
    options: &options::Options,
    arithmetic: &mut numerics::Numerics,
) -> (Vec<u8>, Result<(), (Kind, Failure)>) {
    let mut bytes = Vec::new();
    let buffer =
        super::super::buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false).unwrap();
    let mut output = command_output::Results::new(buffer, options);
    let result = completion.write_results(&mut output, arithmetic, options);
    assert!(output.buffer.finish(|_| Ok(())).first_error.is_none());
    (bytes, result)
}

#[test]
fn retained_groups_collect_interleaved_shared_results_and_complete_independently() {
    let opts = options(&[
        "count", "1", "sum", "1", "mean", "1", "median", "1", "q1", "1", "pvar", "1", "median",
        "1", "first", "2", "last", "2",
    ]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut first = set.retained_group().ok().unwrap();
    let mut second = set.retained_group().ok().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    for (line, group, record) in [
        (1, true, b"1\talpha".as_slice()),
        (2, false, b"10\tone"),
        (3, true, b"3\tbeta"),
        (4, false, b"14\ttwo"),
    ] {
        index.clear();
        let group = if group { &mut first } else { &mut second };
        group
            .collect_fields(
                &mut Whole {
                    record,
                    index: &mut index,
                },
                line,
                &opts,
                &mut arithmetic,
            )
            .ok()
            .unwrap();
    }
    let (bytes, result) = completed_bytes(second.completion(), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"2\t24\t12\t12\t11\t4\t12\tone\ttwo\n");
    let (bytes, result) = completed_bytes(first.completion(), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"2\t4\t2\t2\t1.5\t1\t2\talpha\tbeta\n");
    drop((first, second));
    set.collect_line(b"7\tordinary", 5, &opts, &mut arithmetic, None)
        .ok()
        .unwrap();
    let (bytes, result) = completed_bytes(set.completion(), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"1\t7\t7\t7\t7\t0\t7\tordinary\tordinary\n");
}

#[test]
fn retained_group_construction_and_collection_failures_keep_other_groups_usable() {
    let opts = options(&["count", "1", "median", "1", "wmean", "1:2"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut first = set.retained_group().ok().unwrap();
    for successful_reservations in [0, 2] {
        command_memory::FAIL_RESERVATION.with(|fail| fail.set(Some(successful_reservations)));
        let error = set.retained_group().err().unwrap();
        assert_eq!(error.status, 77);
        assert_eq!(error.message, b"command memory allocation failed\n");
    }
    let mut second = set.retained_group().ok().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    samples::FAIL_GROWTH.with(|fail| fail.set(true));
    let error = first
        .collect_fields(
            &mut Whole {
                record: b"2\t1",
                index: &mut index,
            },
            1,
            &opts,
            &mut arithmetic,
        )
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(error.message, b"median sample memory allocation failed\n");
    index.clear();
    second
        .collect_fields(
            &mut Whole {
                record: b"8\t1",
                index: &mut index,
            },
            2,
            &opts,
            &mut arithmetic,
        )
        .ok()
        .unwrap();
    let (bytes, result) = completed_bytes(second.completion(), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"1\t8\t8\n");
}

#[test]
fn retained_group_typed_completion_failure_leaves_the_next_group_independent() {
    let opts = options(&["--narm", "count", "1", "dotprod", "1:2"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut first = set.retained_group().ok().unwrap();
    let mut second = set.retained_group().ok().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    for (line, failed, record) in [
        (1, true, b"1\tNA".as_slice()),
        (2, false, b"2\t3"),
        (3, true, b"2\t3"),
    ] {
        index.clear();
        let group = if failed { &mut first } else { &mut second };
        group
            .collect_fields(
                &mut Whole {
                    record,
                    index: &mut index,
                },
                line,
                &opts,
                &mut arithmetic,
            )
            .ok()
            .unwrap();
    }
    let mut completion = first.completion();
    assert_eq!(completion.len(), 2);
    assert_eq!(
        opts.presentation
            .render(numerics::Numerics::value80(
                completion
                    .numerical_result(0, &mut arithmetic, &opts)
                    .ok()
                    .unwrap(),
            ))
            .ok()
            .unwrap(),
        b"2"
    );
    let error = completion
        .numerical_result(1, &mut arithmetic, &opts)
        .err()
        .unwrap();
    assert_eq!(error.status, 1);
    assert_eq!(
        error.message,
        b"input error for operation 'dotprod': fields 1,2 have different number of items\n"
    );
    let mut completion = second.completion();
    assert_eq!(
        opts.presentation
            .render(numerics::Numerics::value80(
                completion
                    .numerical_result(0, &mut arithmetic, &opts)
                    .ok()
                    .unwrap(),
            ))
            .ok()
            .unwrap(),
        b"1"
    );
    assert_eq!(
        opts.presentation
            .render(numerics::Numerics::value80(
                completion
                    .numerical_result(1, &mut arithmetic, &opts)
                    .ok()
                    .unwrap(),
            ))
            .ok()
            .unwrap(),
        b"6"
    );
}

#[test]
fn retained_group_numerical_completion_rejects_an_unexpected_text_result() {
    let opts = options(&["first", "1"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut group = set.retained_group().ok().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    group
        .collect_fields(
            &mut Whole {
                record: b"text",
                index: &mut index,
            },
            1,
            &opts,
            &mut arithmetic,
        )
        .ok()
        .unwrap();
    let error = group
        .completion()
        .numerical_result(0, &mut arithmetic, &opts)
        .err()
        .unwrap();
    assert_eq!(error.status, 70);
    assert_eq!(error.message, b"internal numerical invariant failure\n");
}

#[test]
fn dense_groups_collect_interleaved_shared_results_and_complete_independently() {
    let opts = options(&[
        "count", "1", "sum", "1", "mean", "1", "median", "1", "q1", "1", "pvar", "1", "median",
        "1", "first", "2", "last", "2",
    ]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut groups = set.dense_groups();
    let first = groups.add_group().unwrap();
    let second = groups.add_group().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    for (line, group, record) in [
        (1, first, b"1\talpha".as_slice()),
        (2, second, b"10\tone"),
        (3, first, b"3\tbeta"),
        (4, second, b"14\ttwo"),
    ] {
        index.clear();
        groups
            .collect(group, record, &mut index, line, &opts, &mut arithmetic)
            .ok()
            .unwrap();
    }
    let (bytes, result) = completed_bytes(groups.completion(second), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"2\t24\t12\t12\t11\t4\t12\tone\ttwo\n");
    let (bytes, result) = completed_bytes(groups.completion(first), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"2\t4\t2\t2\t1.5\t1\t2\talpha\tbeta\n");
    // Completing retained Groups leaves the ordinary Group ready to collect.
    set.collect_line(b"7\tordinary", 5, &opts, &mut arithmetic, None)
        .ok()
        .unwrap();
    let (bytes, result) = completed_bytes(set.completion(), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"1\t7\t7\t7\t7\t0\t7\tordinary\tordinary\n");
}

#[test]
fn dense_group_construction_failure_keeps_previous_groups_and_next_index() {
    let opts = options(&["count", "1", "median", "1", "wmean", "1:2"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut groups = set.dense_groups();
    assert_eq!(groups.add_group(), Some(0));
    // Fail both vector growth and later initialization using the established
    // allocation fixture. Incomplete Groups cannot be selected afterwards.
    for successful_reservations in [0, 2] {
        command_memory::FAIL_RESERVATION.with(|fail| fail.set(Some(successful_reservations)));
        assert_eq!(groups.add_group(), None);
    }
    assert_eq!(groups.add_group(), Some(1));
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    groups
        .collect(0, b"2\t1", &mut index, 1, &opts, &mut arithmetic)
        .ok()
        .unwrap();
    index.clear();
    groups
        .collect(1, b"8\t1", &mut index, 2, &opts, &mut arithmetic)
        .ok()
        .unwrap();
    for (group, expected) in [(0, b"1\t2\t2\n".as_slice()), (1, b"1\t8\t8\n")] {
        let (bytes, result) = completed_bytes(groups.completion(group), &opts, &mut arithmetic);
        assert!(result.is_ok());
        assert_eq!(bytes, expected);
    }
}

#[test]
fn dense_group_collection_failure_leaves_other_groups_independent() {
    let opts = options(&["count", "1", "median", "1"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut groups = set.dense_groups();
    let first = groups.add_group().unwrap();
    let second = groups.add_group().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    samples::FAIL_GROWTH.with(|fail| fail.set(true));
    let error = groups
        .collect(first, b"2", &mut index, 1, &opts, &mut arithmetic)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(error.message, b"median sample memory allocation failed\n");
    index.clear();
    groups
        .collect(second, b"8", &mut index, 2, &opts, &mut arithmetic)
        .ok()
        .unwrap();
    let (bytes, result) = completed_bytes(groups.completion(second), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"1\t8\n");
}

#[test]
fn retained_completion_failure_keeps_progressive_text_and_operation_context() {
    let opts = options(&["--narm", "first", "3", "count", "1", "dotprod", "1:2"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut groups = set.dense_groups();
    let first = groups.add_group().unwrap();
    let second = groups.add_group().unwrap();
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let mut index = records::FieldIndex::selecting(set.fields());
    for (line, group, record) in [
        (1, first, b"1\tNA\talpha".as_slice()),
        (2, second, b"2\t3\tbeta"),
        (3, first, b"2\t3\tgamma"),
    ] {
        index.clear();
        groups
            .collect(group, record, &mut index, line, &opts, &mut arithmetic)
            .ok()
            .unwrap();
    }
    let (bytes, result) = completed_bytes(groups.completion(first), &opts, &mut arithmetic);
    assert_eq!(bytes, b"alpha\t2\t");
    let (kind, error) = result.err().unwrap();
    assert_eq!(kind, Kind::Dotprod);
    assert_eq!(error.status, 1);
    assert_eq!(
        error.message,
        b"input error for operation 'dotprod': fields 1,2 have different number of items\n"
    );
    let (bytes, result) = completed_bytes(groups.completion(second), &opts, &mut arithmetic);
    assert!(result.is_ok());
    assert_eq!(bytes, b"beta\t1\t6\n");
}
struct NoEntropy;
impl random::SeedSource for NoEntropy {
    fn seed(&mut self) -> Result<u32, ()> {
        panic!("entropy source must not be used")
    }
}

#[test]
fn request_grammar_preserves_order_and_duplicates() {
    let parse = |items: &[&str]| {
        grammar::command(&args(items), None)
            .and_then(|command| OperationSet::new(command.operations))
    };
    let parsed = parse(&["mean", "2", "sum", "1", "mean", "2"]).ok().unwrap();
    let actual: Vec<_> = parsed
        .0
        .plans
        .iter()
        .zip(&parsed.0.states)
        .map(|(op, state)| (op.kind, op.selector, state.count))
        .collect();
    assert_eq!(
        actual,
        [
            (Kind::Mean, Selector::Single(2), 0),
            (Kind::Sum, Selector::Single(1), 0),
            (Kind::Mean, Selector::Single(2), 0)
        ]
    );
    for input in [
        vec![],
        vec!["sum"],
        vec!["sum", "0"],
        vec!["sum", "18446744073709551616"],
        vec!["sum", "+1"],
        vec!["sum", "1", "unknown", "1"],
    ] {
        assert!(parse(&input).is_err());
    }
    assert!(parse(&["sum", "18446744073709551615"]).is_err());
    assert!(parse(&["sum", "01"].repeat(16)).is_ok());
    assert!(parse(&["sum", "1"].repeat(1200)).is_ok());
}

#[test]
fn count_fast_format_matches_general_default14() {
    use fastmash_conversion::{
        format::{self, FormatId},
        profile::Profile,
    };
    let mut counts = vec![0, 12_345_678_901_234, 99_999_999_999_999, u64::MAX];
    for power in 0..=19 {
        let n = 10u64.pow(power);
        counts.extend([n - 1, n, n + 1]);
    }
    for count in counts {
        let expected = format::format(
            numerics::Numerics::value80(integer(count)),
            Profile::C,
            FormatId::Default14,
            16384,
        )
        .unwrap()
        .bytes;
        assert_eq!(
            format_count(count, &Default::default()).ok().unwrap(),
            expected,
            "count {count}"
        );
    }
}

#[test]
fn scalar_replacement_failure_is_transactional_and_status_77() {
    let mut set = requests(&options(&["rand", "1"]));
    let plan = set.plans[0];
    let operation = &mut set.states[0];
    operation.text.replace(b"old").unwrap();
    operation.text.fail_next_growth();
    operation.count = 1;
    let mut random = random::RandomState::initialize(Some(0), &mut NoEntropy).unwrap();
    let error = select_scalar(&plan, operation, b"replacement", Some(&mut random))
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(error.message, b"text value allocation failed\n");
    assert_eq!(operation.text.output(), Some(b"old".as_slice()));
}

#[test]
fn random_selection_count_boundaries_preserve_gnu_modulo_rule() {
    // A fixed independently observed first draw avoids billions of rows.
    // Counts beyond the 31-bit draw range retain the old value for any
    // nonzero draw; this intentionally does not claim unbiased sampling.
    for (count, selected) in [
        (1, true),
        (1_804_289_383, true),
        ((1u64 << 31) - 1, false),
        (1u64 << 31, false),
        ((1u64 << 31) + 1, false),
        (u64::MAX, false),
    ] {
        let mut set = requests(&options(&["rand", "1"]));
        let plan = set.plans[0];
        let operation = &mut set.states[0];
        operation.text.replace(b"old").unwrap();
        operation.count = count;
        let mut random = random::RandomState::initialize(Some(0), &mut NoEntropy).unwrap();
        select_scalar(&plan, operation, b"new", Some(&mut random))
            .ok()
            .unwrap();
        assert_eq!(
            operation.text.output(),
            Some(if selected {
                b"new".as_slice()
            } else {
                b"old".as_slice()
            })
        );
    }
}

#[test]
fn shared_conversion_matches_independent_operations_across_rows_and_resets() {
    let opts = options(&[
        "--narm", "ms", "1", "sum", "2", "sum", "1", "mean", "1", "median", "1", "q1", "1", "min",
        "1", "max", "2", "pvar", "1", "svar", "1", "max", "1",
    ]);
    let mut shared = requests(&opts);
    let mut independent = requests(&opts);
    shared.bind([]).ok().unwrap();
    let mut math = numerics::Numerics::new(false).unwrap();
    for group in [
        [b"2\t7".as_slice(), b"NA\t3", b"5\tNA"],
        [b"NA\tNA".as_slice(), b"-3\t4", b"1\t2"],
    ] {
        for (line, row) in group.into_iter().enumerate() {
            shared
                .collect_line(row, line as u64 + 1, &opts, &mut math, None)
                .ok()
                .unwrap();
            independent
                .collect_line(row, line as u64 + 1, &opts, &mut math, None)
                .ok()
                .unwrap();
            for index in 0..shared.len() {
                assert_eq!(shared.states[index].count, independent.states[index].count);
                // Compare results through the same summarization path, including
                // sample/dispersion followers whose own storage intentionally stays empty.
                let actual = shared.result(index, &mut math, &opts).ok().unwrap();
                let expected = independent.result(index, &mut math, &opts).ok().unwrap();
                assert_eq!(actual, expected);
            }
        }
        shared.reset();
        independent.reset();
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
        let mut set = requests(&opts);
        set.bind([]).ok().unwrap();
        let mut math = numerics::Numerics::new(false).unwrap();
        let error = set
            .collect_line(input, 1, &opts, &mut math, None)
            .err()
            .unwrap();
        assert_eq!(error.message, message);
        assert_eq!(
            set.states.iter().map(|op| op.count).collect::<Vec<_>>(),
            counts
        );
    }
    let opts = options(&["sum", "1", "median", "1", "max", "1"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    let mut math = numerics::Numerics::new(false).unwrap();
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let error = set
        .collect_line(b"2", 1, &opts, &mut math, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(set.states[0].count, 1);
    assert_eq!(set.states[2].count, 0);
}

#[test]
fn path_fields_use_fallible_text_storage_without_a_numerical_session() {
    let opts = options(&[
        "dirname", "1", "basename", "1", "extname", "1", "barename", "1",
    ]);
    let mut set = requests(&opts);
    let mut math = numerics::Numerics::new(false).unwrap();
    set.states[0].text.replace(b"old").unwrap();
    set.states[0].text.fail_next_growth();
    let error = set
        .collect_line(b"long-directory/file.tar.gz", 1, &opts, &mut math, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(set.states[0].text.output(), Some(b"old".as_slice()));
    set.collect_line(b"a/file.tar.gz", 1, &opts, &mut math, None)
        .ok()
        .unwrap();
    assert!(!math.has_session());
    assert_eq!(
        set.states
            .iter()
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
    let mut set = requests(&opts);
    let mut math = numerics::Numerics::new(false).unwrap();
    set.collect_line(b"a\0b", 1, &opts, &mut math, None)
        .ok()
        .unwrap();
    assert!(!math.has_session());
    assert_eq!(
        set.states
            .iter()
            .map(|op| op.text.output().unwrap().len())
            .collect::<Vec<_>>(),
        [32, 40, 56, 64, 96, 128]
    );
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
        let mut set = requests(&options);
        let mut arithmetic = numerics::Numerics::new(false).unwrap();
        PARSED.with(|values| values.borrow_mut().push_back(Some(snan)));
        let error = set
            .collect_line(b"1", 1, &options, &mut arithmetic, None)
            .err()
            .unwrap();
        assert_eq!(
            (error.status, error.message),
            (70, b"internal numerical invariant failure\n".to_vec())
        );
        assert_eq!(set.states[0].count, 0);
        assert!(
            set.states[0]
                .stored
                .as_ref()
                .is_none_or(|stored| match stored {
                    Stored::Kept(kept) => kept[0].samples.as_slice().is_empty(),
                    Stored::Weighted(_) => true,
                })
        );
        assert!(
            numerics::Numerics::value80(set.states[0].value)
                .same_bits(fastmash_numeric_contract::Value80::exact_u64(0))
        );
    }
    let options = options(&["dotprod", "1:2"]);
    let mut set = requests(&options);
    let mut arithmetic = numerics::Numerics::new(true).unwrap();
    PARSED.with(|values| values.borrow_mut().extend([None, Some(snan)]));
    let error = set
        .collect_line(b"1\t2", 1, &options, &mut arithmetic, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 70);
    assert_eq!(set.states[0].kept().pair_samples.lengths(), (1, 0));
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
        let mut set = requests(&options);
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
            super::super::mean_math::FAIL_ALLOCATION.with(|flag| flag.set(true));
            let error = set
                .collect_line(input, 1, &options, &mut arithmetic, None)
                .err()
                .unwrap();
            assert_eq!(error.status, 77);
            assert_eq!(set.states[0].count, 1);
        } else {
            // Pair mismatch happens during finalization after count output is pending.
            PARSED.with(|values| values.borrow_mut().clear());
            let pair = &mut set.states[1].kept_mut().pair_samples;
            pair.push_left(integer(1)).unwrap();
            pair.push_right(integer(2)).unwrap();
            pair.push_right(integer(3)).unwrap();
            set.states[0].count = 1;
            let mut bytes = Vec::new();
            let buffer =
                super::super::buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false)
                    .unwrap();
            let mut output = command_output::Results::new(buffer, &options);
            use command_output::CommandOutput;
            let first = set.result(0, &mut arithmetic, &options).ok().unwrap();
            output.result(&first, b'\t');
            let error = set.result(1, &mut arithmetic, &options).err().unwrap();
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
    let mut set = requests(&options);
    let pair = &mut set.states[1].kept_mut().pair_samples;
    pair.push_left(integer(1)).unwrap();
    pair.reset(); // Left retains capacity; right's next append must allocate.
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let mut arithmetic = numerics::Numerics::new(false).unwrap();
    let error = set
        .collect_line(b"1\t2", 1, &options, &mut arithmetic, None)
        .unwrap_err();
    assert_eq!(error.status, 77);
    assert_eq!(set.states[0].count, 1);
    assert_eq!(set.states[1].kept().pair_samples.lengths(), (1, 0));
}

#[test]
fn dispersion_shares_only_original_order_samples_and_propagates_growth_failure() {
    let opts = options(&[
        "pvar", "1", "median", "1", "sstdev", "1", "svar", "1", "pvar", "2",
    ]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    assert_eq!(
        set.plans
            .iter()
            .map(|op| op.dispersion_source)
            .collect::<Vec<_>>(),
        [None, None, Some(0), Some(0), None]
    );
    let mut arithmetic = numerics::Numerics::with_requirements(false, true).unwrap();
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let error = set
        .collect_line(b"2\t3", 1, &opts, &mut arithmetic, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(error.message, b"pvar sample memory allocation failed\n");
    set.reset();
    for (i, record) in [b"2\t3".as_slice(), b"0\t1", b"4\t5"].iter().enumerate() {
        assert!(
            set.collect_line(record, i as u64 + 1, &opts, &mut arithmetic, None)
                .is_ok()
        );
    }
    let before = set.result(0, &mut arithmetic, &opts).ok().unwrap();
    assert_eq!(set.result(1, &mut arithmetic, &opts).ok().unwrap(), b"2");
    assert_eq!(set.result(0, &mut arithmetic, &opts).ok().unwrap(), before);
    assert_eq!(set.result(2, &mut arithmetic, &opts).ok().unwrap(), b"2");
    assert_eq!(set.result(3, &mut arithmetic, &opts).ok().unwrap(), b"4");
}

#[test]
fn moments_share_only_immutable_samples_and_propagate_growth_failure() {
    let opts = options(&[
        "pskew", "1", "median", "1", "skurt", "1", "mad", "1", "pkurt", "1", "sskew", "2",
        "jarque", "1",
    ]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    assert_eq!(
        set.plans
            .iter()
            .map(|o| o.sample_source)
            .collect::<Vec<_>>(),
        [None, None, Some(0), None, Some(0), None, Some(0)]
    );
    let mut arithmetic = numerics::Numerics::with_requirements(true, true)
        .ok()
        .unwrap();
    for i in 0..4 {
        set.collect_line(
            format!("{}\t{}", i, i + 1).as_bytes(),
            i + 1,
            &opts,
            &mut arithmetic,
            None,
        )
        .ok()
        .unwrap();
    }
    assert_eq!(set.states[0].kept().samples.as_slice().len(), 4);
    assert!(set.states[2].kept().samples.as_slice().is_empty());
    assert_eq!(set.states[2].count, 4);
    set.reset();
    assert!(set.states[0].kept().samples.as_slice().is_empty());
    let opts = options(&["count", "1", "pskew", "1", "skurt", "1"]);
    let mut set = requests(&opts);
    set.bind([]).ok().unwrap();
    samples::FAIL_GROWTH.with(|flag| flag.set(true));
    let error = set
        .collect_line(b"2", 1, &opts, &mut arithmetic, None)
        .err()
        .unwrap();
    assert_eq!(error.status, 77);
    assert_eq!(set.states[0].count, 1);
    assert!(set.states[1].kept().samples.as_slice().is_empty());
    assert_eq!(set.states[2].count, 0);
}

#[test]
fn normality_owners_share_samples_and_report_injected_growth_failure() {
    for op in ["jarque", "dpo"] {
        let opts = options(&["count", "1", op, "1", "pskew", "1"]);
        let mut set = requests(&opts);
        set.bind([]).ok().unwrap();
        assert_eq!(set.plans[2].sample_source, Some(1));
        let mut math = numerics::Numerics::with_requirements(false, true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        samples::FAIL_GROWTH.with(|flag| flag.set(true));
        let error = set
            .collect_line(b"2", 1, &opts, &mut math, None)
            .err()
            .unwrap();
        assert_eq!(error.status, 77);
        assert_eq!(set.states[0].count, 1);
        assert!(set.states[1].kept().samples.as_slice().is_empty());
        assert_eq!(set.states[2].count, 0);
        assert!(!math.has_session());
    }
}

/// The links of the former pairwise search: for each Operation, the first
/// earlier Operation of each family it shares on its selector (sums, samples,
/// dispersion, conversion, then a pair's left and right conversion).
fn pairwise_links(operations: &[Plan]) -> Vec<[Option<usize>; 6]> {
    let samples = |kind: Kind| {
        if kind.uses_shared_sorted_samples() {
            1
        } else if kind.is_mad() {
            2
        } else if kind.is_moment() || kind.is_normality() {
            3
        } else {
            0
        }
    };
    (0..operations.len())
        .map(|index| {
            let op = &operations[index];
            let find =
                |same: &dyn Fn(&Plan) -> bool| (0..index).find(|&prior| same(&operations[prior]));
            let converts = |field| {
                find(&|prior| {
                    prior.kind.converts_numeric_field() && prior.selector == Selector::Single(field)
                })
            };
            let mut links = [None; 6];
            if let Selector::Pair { left, right } = op.selector {
                links[1] = find(&|prior| prior.selector == op.selector);
                links[4] = converts(left);
                links[5] = converts(right);
                return links;
            }
            if op.kind.converts_numeric_field() {
                links[3] = find(&|prior| {
                    prior.kind.converts_numeric_field() && prior.selector == op.selector
                });
            }
            if matches!(op.kind, Kind::Sum | Kind::Mean) {
                links[0] = find(&|prior| {
                    matches!(prior.kind, Kind::Sum | Kind::Mean) && prior.selector == op.selector
                });
            }
            if op.kind.is_dispersion() {
                links[2] =
                    find(&|prior| prior.kind.is_dispersion() && prior.selector == op.selector);
            }
            if samples(op.kind) != 0 {
                links[1] = find(&|prior| {
                    samples(prior.kind) == samples(op.kind) && prior.selector == op.selector
                });
            }
            links
        })
        .collect()
}

#[test]
fn sorted_linking_matches_the_pairwise_search() {
    let kinds = [
        "sum", "mean", "median", "q1", "mode", "perc", "trimmean", "mad", "madraw", "pskew",
        "skurt", "jarque", "dpo", "pvar", "sstdev", "min", "range", "ms", "count", "first",
        "unique", "pcov", "spearson", "dotprod",
    ];
    let mut state = 147u64;
    let mut next = |bound: usize| {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 33) as usize % bound
    };
    for _ in 0..2000 {
        let mut items = Vec::new();
        for _ in 0..1 + next(14) {
            let kind = kinds[next(kinds.len())];
            items.push(kind.to_string());
            items.push(if matches!(kind, "pcov" | "spearson" | "dotprod") {
                format!("{}:{}", 1 + next(3), 1 + next(3))
            } else {
                format!("{}", 1 + next(3))
            });
        }
        let items: Vec<&str> = items.iter().map(String::as_str).collect();
        let opts = options(&items);
        let mut set = requests(&opts);
        set.bind([]).ok().unwrap();
        let actual: Vec<_> = set
            .plans
            .iter()
            .map(|op| {
                [
                    op.sum_source,
                    op.sample_source,
                    op.dispersion_source,
                    op.conversion_source,
                    op.pair_conversion[0],
                    op.pair_conversion[1],
                ]
            })
            .collect();
        assert_eq!(actual, pairwise_links(&set.plans), "{items:?}");
        // Binding again gives the same links.
        set.bind([]).ok().unwrap();
        assert_eq!(
            set.plans
                .iter()
                .map(|op| (op.sum_source, op.sample_source, op.conversion_source))
                .collect::<Vec<_>>(),
            actual
                .iter()
                .map(|l| (l[0], l[1], l[3]))
                .collect::<Vec<_>>()
        );
    }
}
