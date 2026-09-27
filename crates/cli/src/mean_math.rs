//! Checked Rust log/exp shared by geometric mean and normality probabilities.
use super::numerics::{NumericFailure, Numerics, Value};
use astro_float_num::{BigFloat, Consts, RoundingMode, Sign};
use fastmash_numeric_contract::{Raw80, Value80, ValueClass};

pub(super) struct Transcendentals {
    constants: Consts,
    logarithms: Option<super::log_cache::LogCache>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended as Extended};

    #[test]
    fn adapter_cache_and_fallback_match_independent_mpfr() {
        let mut math = Transcendentals::new().unwrap();
        for line in include_str!("testdata/log-mpfr.tsv")
            .lines()
            .filter(|s| !s.starts_with('#'))
        {
            let (input, expected) = line.split_once('\t').unwrap();
            let bits = u128::from_str_radix(input, 16).unwrap();
            let input =
                Numerics::admit(Value80::from_raw(Raw80::try_from_bits(bits).unwrap()).unwrap())
                    .unwrap();
            let expected = u128::from_str_radix(expected, 16).unwrap();
            for _ in 0..2 {
                assert_eq!(
                    Numerics::value80(math.log(input).unwrap()).raw().to_bits(),
                    expected,
                    "{bits:020x}"
                );
            }
        }
        math.log(Numerics::promote_u64(2)).unwrap();
        FAIL_ALLOCATION.with(|flag| flag.set(true));
        assert!(matches!(
            math.log(Numerics::promote_u64(2)),
            Err(NumericFailure::Allocation)
        ));
    }

    #[test]
    fn exponential_adapter_matches_independent_mpfr() {
        let mut math = Transcendentals::new().unwrap();
        let mut retained = fastmash_portable_numerics::PortableNumerics::new().unwrap();
        for line in include_str!("testdata/exp-mpfr.tsv")
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            let (input, expected) = line.split_once('\t').unwrap();
            let bits = u128::from_str_radix(input, 16).unwrap();
            let input =
                Numerics::admit(Value80::from_raw(Raw80::try_from_bits(bits).unwrap()).unwrap())
                    .unwrap();
            let expected = u128::from_str_radix(expected, 16).unwrap();
            let actual = math.exp(input).unwrap();
            let actual = actual
                .map(Numerics::value80)
                .unwrap_or_else(|| retained.exp(input).unwrap());
            assert_eq!(actual.raw().to_bits(), expected, "{bits:020x}");
        }
    }

    #[test]
    fn exp_only_owner_does_not_initialize_log_work() {
        let mut math = Transcendentals::new().unwrap();
        math.exp(Numerics::promote_u64(1)).unwrap();
        assert!(math.logarithms.is_none());
    }

    #[test]
    fn logarithms_preserve_retained_bits_and_wider_precision_consistency() {
        let mut log = Transcendentals::new().unwrap();
        let mut retained = fastmash_portable_numerics::PortableNumerics::new().unwrap();
        let mut constants = Consts::new().unwrap();
        let mut values = Vec::new();
        for exponent in [1, 2, 8192, 16318, 16382, 16383, 16384, 24576, 32765, 32766] {
            for significand in [1 << 63, (1 << 63) + 1, 0xc000_0000_0000_0000, u64::MAX] {
                values.push(Value80::from_raw(Raw80::new(exponent, significand)).unwrap());
            }
        }
        for width in 1..64 {
            for significand in [1u64 << (width - 1), (1u64 << width) - 1] {
                values.push(Value80::from_raw(Raw80::new(0, significand)).unwrap());
            }
        }
        for input in values {
            let actual = Numerics::value80(log.log(Numerics::admit(input).unwrap()).unwrap());
            if input.exponent() != 0 {
                assert_eq!(
                    actual.raw(),
                    retained.log(Numerics::admit(input).unwrap()).unwrap().raw(),
                    "{input:?}"
                );
            }
            if input.raw() == Raw80::new(0x3fff, 1 << 63) {
                assert_eq!(actual.raw(), Value80::signed_zero(false).raw());
                continue;
            }
            let shift = input.significand().leading_zeros();
            let source = BigFloat::from_raw_parts(
                &[input.significand() << shift],
                64,
                Sign::Pos,
                i32::from(input.exponent().max(1)) - 16382 - shift as i32,
                false,
            );
            // Cross-precision consistency is not an independent oracle. Normal
            // inputs above also compare with the independent retained kernel.
            for precision in [128, 256] {
                let high = source.ln(precision, RoundingMode::ToEven, &mut constants);
                checked(&high).unwrap();
                let (words, _, sign, exponent, _) = high.as_raw_parts().unwrap();
                let mut hex = if sign == Sign::Neg {
                    "-0x".to_owned()
                } else {
                    "0x".to_owned()
                };
                for word in words.iter().rev() {
                    hex.push_str(&format!("{word:016x}"));
                }
                hex.push_str(&format!("p{}", exponent - words.len() as i32 * 64));
                let expected = Extended::from_str_r(&hex, Round::NearestTiesToEven)
                    .unwrap()
                    .value
                    .to_bits();
                assert_eq!(actual.raw().to_bits(), expected, "{input:?} at {precision}");
            }
        }
    }

    #[test]
    fn special_values_and_library_errors_are_distinct() {
        let mut log = Transcendentals::new().unwrap();
        for (input, expected) in [
            (Value80::signed_zero(false), Value80::infinity(true)),
            (Value80::signed_zero(true), Value80::infinity(true)),
            (Value80::infinity(false), Value80::infinity(false)),
            (Value80::infinity(true), Value80::canonical_quiet_nan()),
            (
                Value80::from_raw(Raw80::new(0x8000, 1)).unwrap(),
                Value80::canonical_quiet_nan(),
            ),
            (
                Value80::from_raw(Raw80::new(0xffff, 0xc000_0000_0000_0007)).unwrap(),
                Value80::from_raw(Raw80::new(0xffff, 0xc000_0000_0000_0007)).unwrap(),
            ),
        ] {
            assert_eq!(
                Numerics::value80(log.log(Numerics::admit(input).unwrap()).unwrap()).raw(),
                expected.raw()
            );
        }
        let invalid = BigFloat::from_raw_parts(&[1], 65, Sign::Pos, 0, false);
        assert_eq!(checked(&invalid), Err(NumericFailure::Invariant));
    }

    #[test]
    fn exponential_bits_and_boundary_selection() {
        let mut math = Transcendentals::new().unwrap();
        let mut retained = fastmash_portable_numerics::PortableNumerics::new().unwrap();
        for exponent in [2, 0x7ffe] {
            let boundary =
                Numerics::admit(Value80::from_raw(Raw80::new(exponent, 1 << 63)).unwrap()).unwrap();
            let logarithm = retained.log(boundary).unwrap().raw().to_bits();
            for bits in [logarithm - 1, logarithm, logarithm + 1] {
                let input = Numerics::admit(
                    Value80::from_raw(Raw80::try_from_bits(bits).unwrap()).unwrap(),
                )
                .unwrap();
                let actual = math.exp(input).unwrap();
                let expected = retained.exp(input).unwrap();
                assert_eq!(Numerics::value80(actual.unwrap()).raw(), expected.raw());
            }
        }
        let guard = Value80::exact_u64(16384).raw().to_bits();
        for sign in [0, 1u128 << 79] {
            for bits in [Raw80::new(16396, u64::MAX).to_bits(), guard, guard + 1] {
                let input = Numerics::admit(
                    Value80::from_raw(Raw80::try_from_bits(bits | sign).unwrap()).unwrap(),
                )
                .unwrap();
                let result = math.exp(input).unwrap();
                assert_eq!(result.is_none(), bits >= guard);
                if let Some(result) = result {
                    assert_eq!(
                        Numerics::value80(result).raw(),
                        retained.exp(input).unwrap().raw()
                    );
                }
            }
            let tiny = Numerics::admit(
                Value80::from_raw(Raw80::try_from_bits(1 | sign).unwrap()).unwrap(),
            )
            .unwrap();
            assert_eq!(
                Numerics::value80(math.exp(tiny).unwrap().unwrap()).raw(),
                Value80::exact_u64(1).raw()
            );
        }
        for text in [
            "-11354", "-100", "-1", "-0.00001", "0.00001", "1", "100", "11355",
        ] {
            let raw = Extended::from_str_r(text, Round::NearestTiesToEven)
                .unwrap()
                .value
                .to_bits();
            let input =
                Numerics::admit(Value80::from_raw(Raw80::try_from_bits(raw).unwrap()).unwrap())
                    .unwrap();
            assert_eq!(
                Numerics::value80(math.exp(input).unwrap().unwrap()).raw(),
                retained.exp(input).unwrap().raw()
            );
        }
        for text in [
            "-11400",
            "-11356",
            "-11355.2",
            "-11355.137111933024058",
            "11356",
            "11356.52340629414395",
            "11357",
            "16384",
            "-16384",
        ] {
            let raw = Extended::from_str_r(text, Round::NearestTiesToEven)
                .unwrap()
                .value
                .to_bits();
            let input =
                Numerics::admit(Value80::from_raw(Raw80::try_from_bits(raw).unwrap()).unwrap())
                    .unwrap();
            let actual = math.exp(input).unwrap();
            if text == "16384" || text == "-16384" {
                assert!(actual.is_none(), "{text}");
            } else {
                assert_eq!(
                    Numerics::value80(actual.unwrap()).raw(),
                    retained.exp(input).unwrap().raw()
                );
            }
        }
        for (input, expected) in [
            (Value80::signed_zero(false), Value80::exact_u64(1)),
            (Value80::signed_zero(true), Value80::exact_u64(1)),
            (Value80::infinity(false), Value80::infinity(false)),
            (Value80::infinity(true), Value80::signed_zero(false)),
            (
                Value80::canonical_quiet_nan().with_sign(true),
                Value80::canonical_quiet_nan().with_sign(true),
            ),
        ] {
            assert_eq!(
                Numerics::value80(math.exp(Numerics::admit(input).unwrap()).unwrap().unwrap())
                    .raw(),
                expected.raw()
            );
        }
    }
}
#[cfg(test)]
thread_local! {
    pub(super) static FAIL_ALLOCATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
fn checked(value: &BigFloat) -> Result<(), NumericFailure> {
    match value.err() {
        None => Ok(()),
        Some(astro_float_num::Error::MemoryAllocation) => Err(NumericFailure::Allocation),
        Some(_) => Err(NumericFailure::Invariant),
    }
}
fn exact_input(input: Value80) -> BigFloat {
    let shift = input.significand().leading_zeros();
    BigFloat::from_raw_parts(
        &[input.significand() << shift],
        64,
        if input.is_negative() {
            Sign::Neg
        } else {
            Sign::Pos
        },
        i32::from(input.exponent().max(1)) - 16382 - shift as i32,
        false,
    )
}
impl Transcendentals {
    pub fn new() -> Result<Self, NumericFailure> {
        Ok(Self {
            constants: Consts::new().map_err(|_| NumericFailure::Allocation)?,
            logarithms: None,
        })
    }
    pub fn log(&mut self, input: Value) -> Result<Value, NumericFailure> {
        #[cfg(test)]
        if FAIL_ALLOCATION.with(|flag| flag.replace(false)) {
            return Err(NumericFailure::Allocation);
        }
        let input = Numerics::value80(input);
        let special = match input.classify() {
            ValueClass::Nan { .. } => Some(input.quiet_nan()),
            ValueClass::Zero => Some(Value80::infinity(true)),
            _ if input.is_negative() => Some(Value80::canonical_quiet_nan()),
            ValueClass::Infinity => Some(input),
            _ => None,
        };
        if let Some(value) = special {
            return Numerics::admit(value);
        }

        let bits = input.raw().to_bits();
        let cache = self
            .logarithms
            .get_or_insert_with(super::log_cache::LogCache::new);
        if let Some(value) = cache.get(bits) {
            return Ok(value);
        }
        let value = self.finite_log(input)?;
        self.logarithms
            .as_mut()
            .expect("initialized cache")
            .insert(bits, value);
        Ok(value)
    }

    fn finite_log(&mut self, input: Value80) -> Result<Value, NumericFailure> {
        if let Some(bits) = super::guarded_log::log(input.raw().to_bits()) {
            let raw = Raw80::try_from_bits(bits).map_err(|_| NumericFailure::Invariant)?;
            return Numerics::admit(Value80::from_raw(raw).map_err(|_| NumericFailure::Invariant)?);
        }

        // Exact normalization, including all 63 subnormal significand widths.
        let source = exact_input(input);
        checked(&source)?;
        let result = source.ln(64, RoundingMode::ToEven, &mut self.constants);
        checked(&result)?;
        if result.is_zero() {
            return Numerics::admit(Value80::signed_zero(false));
        }
        let (mantissa, precision, sign, exponent, _) =
            result.as_raw_parts().ok_or(NumericFailure::Invariant)?;
        // For positive binary80 x != 1, ln(x) is a normal binary80 number:
        // even the closest inputs to 1 give magnitudes around 2^-64.
        if precision != 64 || mantissa.len() != 1 || !(-16381..=16384).contains(&exponent) {
            return Err(NumericFailure::Invariant);
        }
        let raw = Raw80::new(
            (exponent + 16382) as u16 | if sign == Sign::Neg { 0x8000 } else { 0 },
            mantissa[0],
        );
        Numerics::admit(Value80::from_raw(raw).map_err(|_| NumericFailure::Invariant)?)
    }

    /// None requests retained exp at range boundaries, not after a library error.
    pub fn exp(&mut self, input: Value) -> Result<Option<Value>, NumericFailure> {
        let input = Numerics::value80(input);
        let special = match input.classify() {
            ValueClass::Nan { .. } => Some(input.quiet_nan()),
            ValueClass::Zero => Some(Value80::exact_u64(1)),
            ValueClass::Infinity => Some(if input.is_negative() {
                Value80::signed_zero(false)
            } else {
                input
            }),
            _ => None,
        };
        if let Some(value) = special {
            return Numerics::admit(value).map(Some);
        }
        // |x| >= 2^14 is beyond binary80 exp's finite output range. Keep
        // those inputs on the retained path without asking Astro to expand them.
        if input.exponent() >= 16397 {
            return Ok(None);
        }
        let source = exact_input(input);
        checked(&source)?;
        let result = source.exp(64, RoundingMode::ToEven, &mut self.constants);
        checked(&result)?;
        let Some((mantissa, precision, sign, exponent, _)) = result.as_raw_parts() else {
            return Ok(None);
        };
        // A direct conversion at range boundaries risks double rounding.
        // Certify those results with directed wider endpoints instead.
        if !(-16380..=16383).contains(&exponent) {
            return super::boundary_exp::exp(&source, &mut self.constants)
                .map_err(|error| match error {
                    astro_float_num::Error::MemoryAllocation => NumericFailure::Allocation,
                    _ => NumericFailure::Invariant,
                })?
                .map(|bits| {
                    let raw = Raw80::try_from_bits(bits).map_err(|_| NumericFailure::Invariant)?;
                    Numerics::admit(Value80::from_raw(raw).map_err(|_| NumericFailure::Invariant)?)
                })
                .transpose();
        }
        if precision != 64 || mantissa.len() != 1 || sign != Sign::Pos {
            return Err(NumericFailure::Invariant);
        }
        let raw = Raw80::new((exponent + 16382) as u16, mantissa[0]);
        Numerics::admit(Value80::from_raw(raw).map_err(|_| NumericFailure::Invariant)?).map(Some)
    }
}
