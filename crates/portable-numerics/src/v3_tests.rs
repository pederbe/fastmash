use super::*;
use std::cmp::Ordering;

#[test]
#[ignore = "bounded release-mode startup diagnostic; run explicitly"]
#[allow(clippy::assertions_on_constants)] // Keep the diagnostic's runtime mode check.
fn startup_cost_breakdown() {
    use std::hint::black_box;
    use std::time::Instant;

    assert!(
        !cfg!(debug_assertions),
        "run this diagnostic with --release"
    );
    for round in 0..4 {
        let whole = || {
            let start = Instant::now();
            let session = black_box(PortableNumerics::new());
            let elapsed = start.elapsed().as_nanos();
            let session = session.expect("session initializes");
            black_box(&session);
            drop(session);
            elapsed
        };
        let before = if round % 2 == 0 { Some(whole()) } else { None };
        let start = Instant::now();
        let arena = black_box(Arena::new());
        let allocation = start.elapsed().as_nanos();
        let mut arena = arena.expect("arena allocates");
        let start = Instant::now();
        let result = black_box(&mut arena).initialize_cache();
        let cache = start.elapsed().as_nanos();
        result.expect("cache initializes");
        let start = Instant::now();
        let result = black_box(&mut arena).reset_disposable();
        let reset = start.elapsed().as_nanos();
        result.expect("reset succeeds");
        black_box(&arena);
        let start = Instant::now();
        drop(arena);
        let release = start.elapsed().as_nanos();
        let total = before.unwrap_or_else(whole);
        println!(
            "startup_cost round={round} allocation_ns={allocation} cache_ns={cache} reset_ns={reset} drop_ns={release} constructor_ns={total}"
        );
    }
}

fn value(hex: &str) -> Value80 {
    Value80::from_hex(hex.as_bytes()).expect("test value is canonical binary80")
}

fn arithmetic(hex: &str) -> ArithmeticValue {
    ArithmeticValue::try_from(value(hex)).expect("test value is admitted")
}

fn bits(value: Value80) -> String {
    std::str::from_utf8(&value.to_hex()).unwrap().to_owned()
}

fn admitted(result: Result<Value80, NumericFailure>) -> Result<Value80, NumericFailure> {
    result.and_then(|value| {
        ArithmeticValue::try_from(value)
            .map(Value80::from)
            .map_err(|_| NumericFailure::Invariant)
    })
}

fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(std::mem::take(&mut field)),
            _ => field.push(ch),
        }
    }
    fields.push(field);
    fields
}

fn hex_values(text: &str) -> Vec<&str> {
    text.split(|ch: char| !ch.is_ascii_hexdigit())
        .filter(|part| part.len() == 20)
        .collect()
}

fn json_usize(text: &str, key: &str) -> usize {
    let marker = format!("\"{key}\":");
    let rest = text.split_once(&marker).unwrap().1;
    rest.bytes()
        .take_while(u8::is_ascii_digit)
        .fold(0usize, |value, digit| {
            value * 10 + usize::from(digit - b'0')
        })
}

fn json_hex<'a>(text: &'a str, key: &str) -> &'a str {
    let marker = format!("\"{key}\":\"");
    text.split_once(&marker)
        .unwrap()
        .1
        .split_once('"')
        .unwrap()
        .0
}

fn merge_sort(values: &mut [ArithmeticValue]) {
    let mut scratch = values.to_vec();
    merge_sort_with(values, &mut scratch);
}

fn merge_sort_with(values: &mut [ArithmeticValue], scratch: &mut [ArithmeticValue]) {
    let n = values.len();
    if n < 2 {
        return;
    }
    let middle = n / 2;
    let (left, right) = values.split_at_mut(middle);
    merge_sort_with(left, scratch);
    merge_sort_with(right, scratch);
    let (mut left_index, mut right_index, mut output) = (0, middle, 0);
    while left_index < middle && right_index < n {
        let next = if sample_order(values[left_index], values[right_index]) != Ordering::Greater {
            let next = left_index;
            left_index += 1;
            next
        } else {
            let next = right_index;
            right_index += 1;
            next
        };
        scratch[output] = values[next];
        output += 1;
    }
    scratch[output..output + middle - left_index].copy_from_slice(&values[left_index..middle]);
    output += middle - left_index;
    scratch[output..n].copy_from_slice(&values[right_index..n]);
    values.copy_from_slice(&scratch[..n]);
}

#[test]
fn v3_profile_and_public_bit_helpers_match_the_contract() {
    assert_eq!(PortableNumerics::profile(), "portable-binary80-v3");
    let original = arithmetic("ffffc000000000000042");
    assert_eq!(bits(Value80::from(original)), "ffffc000000000000042");
    assert_eq!(
        bits(Value80::from(clear_sign(original))),
        "7fffc000000000000042"
    );
    assert_eq!(
        compare(
            arithmetic("80000000000000000000"),
            arithmetic("00000000000000000000")
        ),
        Comparison::Equal
    );
    assert_eq!(
        compare_magnitude(
            arithmetic("bfff8000000000000000"),
            arithmetic("3fff8000000000000000")
        ),
        Comparison::Equal
    );
    assert_eq!(
        sample_order(original, arithmetic("3fff8000000000000000")),
        Ordering::Equal
    );
}

#[test]
fn corrected_v3_primitive_rows_are_callable_disproof_coverage() {
    let mut numerics = PortableNumerics::new().unwrap();
    let mut checked = 0usize;
    for packet in [
        include_str!("../testdata/v3/primitive-finite.csv"),
        include_str!("../testdata/v3/primitive-special.csv"),
    ] {
        for line in packet.lines().skip(1) {
            let fields = csv_fields(line);
            let case_id = &fields[0];
            let operation = fields[1].as_str();
            if !matches!(operation, "subtract" | "multiply" | "sqrt") {
                continue;
            }
            let operands = &fields[2];
            let expected = &fields[4];
            let actual = match operation {
                "subtract" | "multiply" => {
                    let mut parts = operands.split(';');
                    let left = arithmetic(parts.next().unwrap());
                    let right = arithmetic(parts.next().unwrap());
                    assert!(parts.next().is_none(), "{case_id}: unexpected operand");
                    if operation == "subtract" {
                        admitted(numerics.subtract(left, right))
                    } else {
                        admitted(numerics.multiply(left, right))
                    }
                }
                "sqrt" => admitted(numerics.sqrt(arithmetic(operands))),
                _ => unreachable!(),
            };
            let actual = actual.unwrap_or_else(|error| panic!("{case_id}: {error:?}"));
            assert_eq!(bits(actual), *expected, "{case_id}");
            checked += 1;
        }
    }
    assert_eq!(checked, 214);
}

#[test]
fn subtraction_keeps_sticky_below_the_first_window_bit() {
    let mut numerics = PortableNumerics::new().unwrap();
    for (left, right, expected) in [
        (
            "3fff8000000000000000",
            "3ffc8000000000000005",
            "3ffedfffffffffffffff",
        ),
        (
            "3ffc8000000000000005",
            "3fff8000000000000000",
            "bffedfffffffffffffff",
        ),
        (
            "bfff8000000000000000",
            "bffc8000000000000005",
            "bffedfffffffffffffff",
        ),
        (
            "bffc8000000000000005",
            "bfff8000000000000000",
            "3ffedfffffffffffffff",
        ),
    ] {
        let actual = admitted(numerics.subtract(arithmetic(left), arithmetic(right))).unwrap();
        assert_eq!(bits(actual), expected, "{left} - {right}");
    }
}

#[test]
fn every_direct_v3_exact_helper_row_matches() {
    let packet = include_str!("../testdata/v3/exact-helpers.csv");
    let mut checked = 0usize;
    for line in packet.lines().skip(1) {
        let fields = csv_fields(line);
        let case_id = &fields[0];
        let helper = fields[1].as_str();
        let input = &fields[2];
        let expected = &fields[3];
        match helper {
            "compare" | "compare_magnitude" | "sample_order" => {
                let operands = hex_values(input);
                if helper == "sample_order" && expected.starts_with('[') {
                    let mut actual: Vec<_> = operands.into_iter().map(arithmetic).collect();
                    merge_sort(&mut actual);
                    let actual: Vec<_> = actual
                        .into_iter()
                        .map(|value| bits(Value80::from(value)))
                        .collect();
                    assert_eq!(actual, hex_values(expected), "{case_id}");
                    checked += 1;
                    continue;
                }
                assert_eq!(operands.len(), 2, "{case_id}");
                let left = arithmetic(operands[0]);
                let right = arithmetic(operands[1]);
                if helper == "sample_order" {
                    let expected_order = match expected.as_str() {
                        "-1" => Ordering::Less,
                        "0" => Ordering::Equal,
                        "1" => Ordering::Greater,
                        _ => panic!("{case_id}: bad ordering"),
                    };
                    assert_eq!(sample_order(left, right), expected_order, "{case_id}");
                } else {
                    let expected_comparison = match expected.trim_matches('"') {
                        "Less" => Comparison::Less,
                        "Equal" => Comparison::Equal,
                        "Greater" => Comparison::Greater,
                        "Unordered" => Comparison::Unordered,
                        _ => panic!("{case_id}: bad comparison"),
                    };
                    let actual = if helper == "compare" {
                        compare(left, right)
                    } else {
                        compare_magnitude(left, right)
                    };
                    assert_eq!(actual, expected_comparison, "{case_id}");
                }
            }
            "clear_sign" => {
                let operand = hex_values(input);
                assert_eq!(operand.len(), 1, "{case_id}");
                assert_eq!(
                    bits(Value80::from(clear_sign(arithmetic(operand[0])))),
                    expected.trim_matches('"'),
                    "{case_id}"
                );
            }
            "promote_u64" => {
                let integer = input.trim_matches(['[', ']']).parse::<u64>().unwrap();
                assert_eq!(
                    bits(promote_u64(integer)),
                    json_hex(expected, "bits"),
                    "{case_id}"
                );
            }
            "percentile_position" => {
                let (n, percent) = if input.starts_with('{') {
                    (json_usize(input, "n"), json_usize(input, "percent") as u8)
                } else {
                    let mut inputs = input.trim_matches(['[', ']']).split(',');
                    (
                        inputs.next().unwrap().parse::<usize>().unwrap(),
                        inputs.next().unwrap().parse::<u8>().unwrap(),
                    )
                };
                let (index, fraction) = percentile_position(n, percent);
                assert_eq!(index, json_usize(expected, "index"), "{case_id}");
                assert_eq!(
                    bits(Value80::from(fraction)),
                    json_hex(expected, "fraction_bits"),
                    "{case_id}"
                );
            }
            "sample_sequence" => continue,
            _ => panic!("{case_id}: unknown helper {helper}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 9_828);
}

#[test]
fn sample_sequence_rows_preserve_stable_helper_order() {
    let packet = include_str!("../testdata/v3/exact-helpers.csv");
    let mut checked = 0usize;
    for line in packet.lines().skip(1) {
        let fields = csv_fields(line);
        if fields[1] != "sample_sequence" {
            continue;
        }
        let case_id = &fields[0];
        let mut actual: Vec<_> = hex_values(&fields[2]).into_iter().map(arithmetic).collect();
        merge_sort(&mut actual);
        let sorted = fields[3]
            .split_once("\"sorted\":[")
            .unwrap()
            .1
            .split_once(']')
            .unwrap()
            .0;
        let expected = hex_values(sorted);
        let actual: Vec<_> = actual
            .into_iter()
            .map(|value| bits(Value80::from(value)))
            .collect();
        assert_eq!(actual, expected, "{case_id}");
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn sample_sequence_uses_the_recursive_merge_traversal() {
    let mut values = [
        arithmetic("40008000000000000000"),
        arithmetic("7fffc000000000000000"),
        arithmetic("3fff8000000000000000"),
        arithmetic("00000000000000000000"),
    ];
    merge_sort(&mut values);
    let actual: Vec<_> = values
        .into_iter()
        .map(|value| bits(Value80::from(value)))
        .collect();
    assert_eq!(
        actual,
        [
            "00000000000000000000",
            "3fff8000000000000000",
            "40008000000000000000",
            "7fffc000000000000000",
        ]
    );
}

#[test]
fn primitive_scope_cleanup_and_mixed_session_reuse_are_closed() {
    use crate::arena::Slot9;

    let mut numerics = PortableNumerics::new().unwrap();
    let cache_before = numerics.arena.cache_snapshot_for_test();
    let preserved = [
        Slot9::work(20, 0),
        Slot9::work(21, 0),
        Slot9::work(22, 0),
        Slot9::work(23, 0),
    ];
    for (index, slot) in preserved.into_iter().enumerate() {
        numerics
            .arena
            .assign9(slot, index as u64 + 11, index & 1 != 0, -9)
            .unwrap();
    }

    admitted(numerics.subtract(
        arithmetic("40008000000000000000"),
        arithmetic("3fff8000000000000000"),
    ))
    .unwrap();
    assert!(numerics.arena.primitive_cells_clear_for_test());
    assert!(matches!(
        numerics.arena.inject_primitive_invariant_for_test(),
        Err(NumericFailure::Invariant)
    ));
    assert!(numerics.arena.primitive_cells_clear_for_test());
    assert_eq!(numerics.arena.cache_snapshot_for_test(), cache_before);
    for (index, slot) in preserved.into_iter().enumerate() {
        assert_eq!(numerics.arena.limb9(slot, 0).unwrap(), index as u64 + 11);
    }

    assert_eq!(
        bits(
            admitted(numerics.add(
                arithmetic("3fff8000000000000000"),
                arithmetic("3fff8000000000000000")
            ))
            .unwrap()
        ),
        "40008000000000000000"
    );
    assert_eq!(
        bits(
            admitted(numerics.multiply(
                arithmetic("40008000000000000000"),
                arithmetic("40008000000000000000")
            ))
            .unwrap()
        ),
        "40018000000000000000"
    );
    assert_eq!(
        bits(
            admitted(numerics.divide(
                arithmetic("40008000000000000000"),
                arithmetic("40008000000000000000")
            ))
            .unwrap()
        ),
        "3fff8000000000000000"
    );
    assert_eq!(
        bits(admitted(numerics.sqrt(arithmetic("40018000000000000000"))).unwrap()),
        "40008000000000000000"
    );
    assert_eq!(
        bits(admitted(numerics.exp(arithmetic("00000000000000000000"))).unwrap()),
        "3fff8000000000000000"
    );
}

#[test]
fn percentile_position_admitted_bounds_keep_a_successor() {
    for &(n, percent) in &[(2, 1), (2, 99), (65_536, 1), (65_536, 99)] {
        let (index, fraction) = percentile_position(n, percent);
        assert!(index + 1 < n);
        assert!(ArithmeticValue::try_from(Value80::from(fraction)).is_ok());
    }
}

#[test]
fn restricted_square_root_matches_full_owner_without_cache_setup() {
    use fastmash_numeric_contract::Raw80;
    assert!(matches!(
        SquareRoot::new_with(Arena::allocation_failure),
        Err(NumericFailure::Allocation)
    ));
    let mut restricted = SquareRoot::new().unwrap();
    let mut full = PortableNumerics::new().unwrap();
    let untouched = restricted.arena.cache_snapshot_for_test();
    assert!(untouched.iter().all(|&cell| cell == 0));
    for exponent in [
        0, 1, 2, 0x1fff, 0x3ffe, 0x3fff, 0x4000, 0x4001, 0x7ffe, 0x7fff,
    ] {
        for significand in [
            0,
            1,
            (1u64 << 63) - 1,
            1u64 << 63,
            (1u64 << 63) + 1,
            0xc000000000000000,
            u64::MAX,
        ] {
            for sign in [0, 0x8000] {
                let Ok(value) = Value80::from_raw(Raw80::new(exponent | sign, significand)) else {
                    continue;
                };
                let Ok(value) = ArithmeticValue::try_from(value) else {
                    continue;
                };
                for _ in 0..2 {
                    let actual = restricted.sqrt(value).unwrap();
                    assert!(actual.same_bits(full.sqrt(value).unwrap()));
                    assert_eq!(restricted.arena.cache_snapshot_for_test(), untouched);
                    assert!(restricted.arena.primitive_cells_clear_for_test());
                }
            }
        }
    }
    let one =
        ArithmeticValue::try_from(Value80::from_raw(Raw80::new(0x3fff, 1 << 63)).unwrap()).unwrap();
    assert!(restricted.sqrt(one).unwrap().same_bits(one.into()));
}
