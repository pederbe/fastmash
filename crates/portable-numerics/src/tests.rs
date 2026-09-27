use super::*;

fn value(hex: &str) -> Value80 {
    Value80::from_hex(hex.as_bytes()).expect("test value is canonical binary80")
}

fn arithmetic(hex: &str) -> ArithmeticValue {
    ArithmeticValue::try_from(value(hex)).expect("test value is admitted")
}

fn assert_bits(actual: Value80, expected: &str) {
    assert_eq!(std::str::from_utf8(&actual.to_hex()).unwrap(), expected);
}

#[test]
fn allocation_failure_is_closed_before_an_arena_exists() {
    assert!(matches!(
        Arena::allocation_failure(),
        Err(NumericFailure::Allocation)
    ));
}

#[test]
fn profile_and_exact_u64_promotion_are_stable() {
    assert_eq!(PortableNumerics::profile(), "portable-binary80-v3");
    assert_eq!(
        crate::machine::STAGES,
        [(160, 58), (224, 79), (288, 99), (416, 140), (544, 181)]
    );
    assert_eq!(
        crate::machine::STAGES
            .iter()
            .map(|(_, terms)| usize::from(*terms))
            .sum::<usize>(),
        557
    );
    for (integer, expected) in [
        (0, "00000000000000000000"),
        (1, "3fff8000000000000000"),
        (u64::MAX, "403effffffffffffffff"),
    ] {
        assert_bits(promote_u64(integer), expected);
    }
}

#[test]
fn signaling_nan_is_rejected_and_quiet_payload_is_preserved() {
    assert_eq!(
        ArithmeticValue::try_from(value("7fff8000000000000001")).unwrap_err(),
        ValueAdmissionFailure::SignalingNan
    );
    let quiet = ArithmeticValue::try_from(value("ffffc000000000000007")).unwrap();
    assert_bits(quiet.0, "ffffc000000000000007");
}

#[test]
fn fixed_cache_and_representative_operations_match_frozen_rows() {
    let mut numerics = PortableNumerics::new().expect("fixed cache initializes");
    assert_bits(
        numerics
            .add(
                arithmetic("3fff8000000000000000"),
                arithmetic("40008000000000000000"),
            )
            .unwrap(),
        "4000c000000000000000",
    );
    assert_bits(
        numerics
            .divide(
                arithmetic("3fff8000000000000000"),
                arithmetic("4000c000000000000000"),
            )
            .unwrap(),
        "3ffdaaaaaaaaaaaaaaab",
    );
    assert_bits(
        numerics.log(arithmetic("40008000000000000000")).unwrap(),
        "3ffeb17217f7d1cf79ac",
    );
    assert_bits(
        numerics.exp(arithmetic("3fff8000000000000000")).unwrap(),
        "4000adf85458a2bb4a9b",
    );
}

#[test]
fn special_values_keep_the_closed_v1_rules() {
    let mut numerics = PortableNumerics::new().unwrap();
    assert_bits(
        numerics
            .add(
                arithmetic("80000000000000000000"),
                arithmetic("80000000000000000000"),
            )
            .unwrap(),
        "80000000000000000000",
    );
    assert_bits(
        numerics
            .add(
                arithmetic("7fff8000000000000000"),
                arithmetic("ffff8000000000000000"),
            )
            .unwrap(),
        "7fffc000000000000000",
    );
    assert_bits(
        numerics
            .divide(
                arithmetic("00000000000000000000"),
                arithmetic("00000000000000000000"),
            )
            .unwrap(),
        "7fffc000000000000000",
    );
    assert!(matches!(
        numerics.log(arithmetic("00000000000000000001")),
        Err(NumericFailure::UnsupportedDomain)
    ));
}

#[test]
fn frozen_v2_special_rows_are_disproof_coverage() {
    let mut numerics = PortableNumerics::new().unwrap();
    let packet = include_str!("../testdata/v2/primitive-special.csv");
    let mut checked = 0usize;
    for line in packet.lines().skip(1) {
        let mut fields = line.split(',');
        let case_id = fields.next().unwrap();
        let operation = fields.next().unwrap();
        let operands = fields.next().unwrap();
        let _integer = fields.next().unwrap();
        let expected = fields.next().unwrap();
        match operation {
            "admit" => match expected {
                "SignalingNan" => assert_eq!(
                    ArithmeticValue::try_from(value(operands)).unwrap_err(),
                    ValueAdmissionFailure::SignalingNan,
                    "{case_id}"
                ),
                bits => assert_bits(ArithmeticValue::try_from(value(operands)).unwrap().0, bits),
            },
            "admit_raw" => assert_eq!(
                format!("{:?}", Value80::from_hex(operands.as_bytes()).unwrap_err()),
                expected,
                "{case_id}"
            ),
            "add" | "divide" => {
                let mut parts = operands.split(';');
                let left = arithmetic(parts.next().unwrap());
                let right = arithmetic(parts.next().unwrap());
                let actual = if operation == "add" {
                    numerics.add(left, right)
                } else {
                    numerics.divide(left, right)
                }
                .unwrap();
                assert_bits(actual, expected);
            }
            "log" => assert!(
                matches!(
                    numerics.log(arithmetic(operands)),
                    Err(NumericFailure::UnsupportedDomain)
                ),
                "{case_id}"
            ),
            _ => panic!("{case_id}: unknown operation {operation}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 19);
}

#[test]
fn fixed_limb_multiply_divide_and_reset_keep_the_pinned_slots() {
    use crate::arena::{Desc, Slot9, Slot17};

    let mut arena = Arena::new().unwrap();
    let left = Slot9::work(0, 0);
    let right = Slot9::work(0, 1);
    arena.assign9(left, u64::MAX, false, 0).unwrap();
    arena.assign9(right, u64::MAX, false, 0).unwrap();
    arena.mul_9x9(Slot17::X0, left, right, 64).unwrap();
    let product = u128::from(u64::MAX) * u128::from(u64::MAX);
    assert_eq!(arena.limb17(Slot17::X0, 0).unwrap(), product as u64);
    assert_eq!(arena.limb17(Slot17::X0, 1).unwrap(), (product >> 64) as u64);
    assert_eq!(arena.bit_len17(Slot17::X0).unwrap(), 128);

    arena.assign9(left, 100, false, 0).unwrap();
    arena.assign9(right, 3, false, 0).unwrap();
    arena.shift_copy9_to17(Slot17::X0, left, 0).unwrap();
    arena
        .div_rem_17_by_9(Slot17::X0, right, Slot17::X1, Slot17::X2)
        .unwrap();
    assert_eq!(arena.limb17(Slot17::X1, 0).unwrap(), 33);
    assert_eq!(arena.limb17(Slot17::X2, 0).unwrap(), 1);

    arena.clear9(left, false).unwrap();
    arena.clear9(right, false).unwrap();
    for bit in 0..544 {
        arena.set_bit9(left, bit).unwrap();
        arena.set_bit9(right, bit).unwrap();
    }
    arena.normalize9(left, false, 0).unwrap();
    arena.normalize9(right, false, 0).unwrap();
    arena.mul_9x9(Slot17::X0, left, right, 544).unwrap();
    assert_eq!(arena.bit_len17(Slot17::X0).unwrap(), 1088);
    assert_eq!(arena.limb17(Slot17::X0, 0).unwrap(), 1);
    assert!(arena.bit17(Slot17::X0, 1087).unwrap());

    let failure: Result<(), NumericFailure> = arena.exact_scope(|scope| {
        scope.set_bit17(Slot17::X0, 0)?;
        Err(NumericFailure::Invariant)
    });
    assert_eq!(failure, Err(NumericFailure::Invariant));
    assert_eq!(arena.bit_len17(Slot17::X0).unwrap(), 0);
    assert_eq!(arena.bit_len17(Slot17::X1).unwrap(), 0);
    assert_eq!(arena.bit_len17(Slot17::X2).unwrap(), 0);

    let preserved = Slot9::work(20, 0);
    arena.assign9(preserved, 7, true, -9).unwrap();
    arena.reset_disposable().unwrap();
    assert_eq!(
        arena.desc9(preserved).unwrap(),
        Desc {
            sign: true,
            len: 1,
            exp: -9
        }
    );
    assert_eq!(arena.limb9(preserved, 0).unwrap(), 7);
    assert_eq!(arena.bit_len17(Slot17::X0).unwrap(), 0);
}

#[test]
fn certificate_owns_zero_midpoints_and_overflow_boundaries() {
    use crate::arena::Slot9;

    let mut arena = Arena::new().unwrap();
    let lo = Slot9::work(0, 0);
    let hi = Slot9::work(0, 1);

    arena.clear9(lo, false).unwrap();
    arena.assign9(hi, 1, false, -16_446).unwrap();
    assert_bits(
        arena.certificate_for_test(lo, hi).unwrap().unwrap(),
        "00000000000000000000",
    );

    arena.assign9(lo, 1, true, -16_446).unwrap();
    arena.clear9(hi, true).unwrap();
    assert_bits(
        arena.certificate_for_test(lo, hi).unwrap().unwrap(),
        "80000000000000000000",
    );

    arena.clear9(lo, false).unwrap();
    arena.clear9(hi, false).unwrap();
    for bit in 0..65 {
        arena.set_bit9(lo, bit).unwrap();
        arena.set_bit9(hi, bit).unwrap();
    }
    arena.normalize9(lo, false, 16_319).unwrap();
    arena.normalize9(hi, false, 16_319).unwrap();
    assert_bits(
        arena.certificate_for_test(lo, hi).unwrap().unwrap(),
        "7fff8000000000000000",
    );
}

#[test]
fn canonical_import_rejects_the_two_raw_encoding_gaps() {
    use fastmash_numeric_contract::EncodingError;
    assert_eq!(
        Value80::from_hex(b"00008000000000000000").unwrap_err(),
        EncodingError::PseudoDenormal
    );
    assert_eq!(
        Value80::from_hex(b"00010000000000000000").unwrap_err(),
        EncodingError::MissingIntegerBit
    );
}

#[test]
fn frozen_v2_primitive_rows_are_disproof_coverage() {
    let mut numerics = PortableNumerics::new().unwrap();
    let packet = include_str!("../testdata/v2/primitive-finite.csv");
    let mut checked = 0usize;
    for line in packet.lines().skip(1) {
        if line.ends_with(",mathematical_reference") {
            continue;
        }
        let mut fields = line.split(',');
        let case_id = fields.next().unwrap();
        let operation = fields.next().unwrap();
        let operands = fields.next().unwrap();
        let integer = fields.next().unwrap();
        let expected = fields.next().unwrap();
        let actual = match operation {
            "promote_u64" => Ok(promote_u64(integer.parse().unwrap())),
            "add" | "divide" => {
                let mut parts = operands.split(';');
                let left = arithmetic(parts.next().unwrap());
                let right = arithmetic(parts.next().unwrap());
                assert!(parts.next().is_none(), "{case_id}: unexpected operand");
                if operation == "add" {
                    numerics.add(left, right)
                } else {
                    numerics.divide(left, right)
                }
            }
            "log" => numerics.log(arithmetic(operands)),
            "exp" => numerics.exp(arithmetic(operands)),
            _ => panic!("{case_id}: unknown operation {operation}"),
        };
        match expected {
            "Capacity" => assert!(
                matches!(actual, Err(NumericFailure::Capacity)),
                "{case_id}: {actual:?}"
            ),
            "UnsupportedDomain" => assert!(
                matches!(actual, Err(NumericFailure::UnsupportedDomain)),
                "{case_id}: {actual:?}"
            ),
            bits => {
                let actual = actual.unwrap_or_else(|error| panic!("{case_id}: {error:?}"));
                assert_eq!(
                    std::str::from_utf8(&actual.to_hex()).unwrap(),
                    bits,
                    "{case_id}"
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 8_624);
}
