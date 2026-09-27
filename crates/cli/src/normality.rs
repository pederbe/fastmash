//! GNU-compatible normality probabilities and count corrections.
use super::moments::{excess_kurtosis, skewness};
use super::{Failure, decimal, integer, numeric_failure, numerics, quiet_nan};
use fastmash_numeric_contract::Raw80;
use rustc_apfloat::{
    Float, FloatConvert,
    ieee::{Double, X87DoubleExtended as Extended},
};

fn ses_numerator(n: u64) -> numerics::Value {
    let a = Double::from_u128(n.into()).value;
    let b = Double::from_u128((n - 1).into()).value;
    let product = ((a * Double::from_u128(6).value).value * b).value;
    let extended: rustc_apfloat::StatusAnd<Extended> = product.convert(&mut false);
    numerics::canonical(
        Raw80::try_from_bits(extended.value.to_bits()).expect("finite binary64 promotion"),
    )
}

fn probability(
    exponent: numerics::Value,
    arithmetic: &mut numerics::Numerics,
) -> Result<numerics::Value, Failure> {
    if matches!(
        numerics::Numerics::value80(exponent).classify(),
        fastmash_numeric_contract::ValueClass::Nan { .. }
    ) {
        return Ok(quiet_nan());
    }
    // exp(-64) < 2^-65, half the binary80 gap below 1. Thus GNU's
    // composed 1-(1-exp(x)) is +0 here, even when exp(x) is nonzero.
    let cutoff = numerics::canonical(Raw80::new(0xc005, 1 << 63));
    if matches!(
        numerics::compare(exponent, cutoff),
        numerics::Comparison::Less | numerics::Comparison::Equal
    ) {
        return Ok(integer(0));
    }
    let tail = arithmetic
        .normality_exp(exponent)
        .map_err(numeric_failure)?;
    decimal::subtract(integer(1), decimal::subtract(integer(1), tail)?)
}

pub(super) fn jarque_bera(
    values: &[numerics::Value],
    arithmetic: &mut numerics::Numerics,
    cache: &mut super::moments::Cache,
) -> Result<numerics::Value, Failure> {
    use fastmash_numeric_contract::{Raw80, ValueClass};

    let n = values.len() as u64;
    if n <= 1 {
        return Ok(quiet_nan());
    }
    let kurtosis = excess_kurtosis(values, false, arithmetic, cache)?;
    let skew = skewness(values, false, arithmetic, cache)?;
    let squared_kurtosis = decimal::multiply(kurtosis, kurtosis)?;
    let quarter = numerics::canonical(Raw80::new(0x3ffd, 0x8000_0000_0000_0000));
    let kurtosis_term = decimal::multiply(squared_kurtosis, quarter)?;
    let squared_skew = decimal::multiply(skew, skew)?;
    let sum = decimal::add(kurtosis_term, squared_skew)?;
    let scaled = decimal::multiply(integer(n), sum)?;
    let statistic = decimal::divide(scaled, integer(6))?;
    let negative_half = numerics::canonical(Raw80::new(0xbffe, 0x8000_0000_0000_0000));
    let exponent = decimal::multiply(statistic, negative_half)?;
    let tail = probability(exponent, arithmetic)?;
    if matches!(
        numerics::Numerics::value80(kurtosis).classify(),
        ValueClass::Nan { .. }
    ) || matches!(
        numerics::Numerics::value80(skew).classify(),
        ValueClass::Nan { .. }
    ) {
        return Ok(quiet_nan());
    }
    Ok(tail)
}

fn skewness_standard_error(
    n: u64,
    arithmetic: &mut numerics::Numerics,
) -> Result<numerics::Value, Failure> {
    if n <= 2 {
        return Ok(quiet_nan());
    }
    // Preserve GNU's binary64 round points before binary80 promotion.
    let numerator = ses_numerator(n);
    let denominator = decimal::multiply(integer(n - 2), integer(n + 1))?;
    let denominator = decimal::multiply(denominator, integer(n + 3))?;
    let ratio = decimal::divide(numerator, denominator)?;
    arithmetic.sqrt(ratio).map_err(numeric_failure)
}

fn kurtosis_standard_error(
    n: u64,
    arithmetic: &mut numerics::Numerics,
) -> Result<numerics::Value, Failure> {
    let ses = skewness_standard_error(n, arithmetic)?;
    if n <= 3 {
        return Ok(quiet_nan());
    }
    let doubled = decimal::add(ses, ses)?;
    let (numerator, denominator) = sek_counts(n);
    let ratio = decimal::divide(integer(numerator), integer(denominator))?;
    let root = arithmetic.sqrt(ratio).map_err(numeric_failure)?;
    decimal::multiply(doubled, root)
}

pub(super) fn dagostino_pearson(
    values: &[numerics::Value],
    arithmetic: &mut numerics::Numerics,
    cache: &mut super::moments::Cache,
) -> Result<numerics::Value, Failure> {
    use fastmash_numeric_contract::{Raw80, ValueClass};

    let n = values.len() as u64;
    if n <= 1 {
        return Ok(quiet_nan());
    }
    let is_nan = |value: numerics::Value| {
        matches!(
            numerics::Numerics::value80(value).classify(),
            ValueClass::Nan { .. }
        )
    };
    let skew = skewness(values, true, arithmetic, cache)?;
    let ses = skewness_standard_error(n, arithmetic)?;
    let z_skew = if is_nan(skew) || is_nan(ses) {
        quiet_nan()
    } else {
        decimal::divide(skew, ses)?
    };
    let kurtosis = excess_kurtosis(values, true, arithmetic, cache)?;
    let sek = kurtosis_standard_error(n, arithmetic)?;
    let squared_skew = decimal::multiply(z_skew, z_skew)?;
    let invalid_kurtosis = is_nan(kurtosis) || is_nan(sek);
    let z_kurtosis = if invalid_kurtosis {
        quiet_nan()
    } else {
        decimal::divide(kurtosis, sek)?
    };
    let squared_kurtosis = if invalid_kurtosis {
        quiet_nan()
    } else {
        decimal::multiply(z_kurtosis, z_kurtosis)?
    };
    let statistic = decimal::add(squared_skew, squared_kurtosis)?;
    let negative_half = numerics::canonical(Raw80::new(0xbffe, 0x8000_0000_0000_0000));
    let exponent = decimal::multiply(statistic, negative_half)?;
    let tail = probability(exponent, arithmetic)?;
    if invalid_kurtosis {
        return Ok(quiet_nan());
    }
    let result = tail;
    Ok(if is_nan(z_skew) || is_nan(z_kurtosis) {
        quiet_nan()
    } else {
        result
    })
}

fn sek_counts(n: u64) -> (u64, u64) {
    (
        n.wrapping_mul(n).wrapping_sub(1),
        (n - 3).wrapping_mul(n + 5),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(text: &str) -> numerics::Value {
        numerics::canonical(
            Raw80::try_from_bits(
                Extended::from_str_r(text, rustc_apfloat::Round::NearestTiesToEven)
                    .unwrap()
                    .value
                    .to_bits(),
            )
            .unwrap(),
        )
    }
    #[test]
    fn count_round_points_and_unsigned_wrap() {
        for n in [
            4,
            65536,
            65537,
            100_000_001,
            (1 << 32) - 1,
            1 << 32,
            (1 << 32) + 1,
            (1 << 53) - 1,
            1 << 53,
            (1 << 53) + 1,
            (1 << 60) - 1,
        ] {
            // Independent hardware binary64 arithmetic, outside production.
            let native = (n as f64 * 6.0) * ((n - 1) as f64);
            let expected: rustc_apfloat::StatusAnd<Extended> =
                Double::from_bits(native.to_bits().into()).convert(&mut false);
            assert_eq!(
                numerics::Numerics::value80(ses_numerator(n))
                    .raw()
                    .to_bits(),
                expected.value.to_bits()
            );
        }
        assert_eq!(sek_counts(1 << 32), (u64::MAX, (1 << 33) - 15));
        assert_eq!(sek_counts((1 << 32) + 1), (1 << 33, (1 << 34) - 12));
    }
    #[test]
    fn tails_match_independent_retained_engine_at_cancellation_and_cutoff() {
        let mut actual = numerics::Numerics::with_requirements(false, true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        let mut independent = fastmash_portable_numerics::PortableNumerics::new().unwrap();
        let mut transition = [false; 2];
        for text in [
            "-0.0001",
            "-1",
            "-44",
            "-45",
            "-45.054566736396445112",
            "-46",
            "-63.99999999999999",
            "-64",
            "-65",
        ] {
            let bits = numerics::Numerics::value80(value(text)).raw().to_bits();
            let center = Extended::from_bits(bits);
            for point in [center.next_down().value, center, center.next_up().value] {
                let bits = point.to_bits();
                let input = numerics::canonical(Raw80::try_from_bits(bits).unwrap());
                let tail = independent.exp(input).unwrap();
                let cumulative = independent
                    .subtract(integer(1), numerics::Numerics::admit(tail).unwrap())
                    .unwrap();
                let expected = independent
                    .subtract(integer(1), numerics::Numerics::admit(cumulative).unwrap())
                    .unwrap();
                let result = probability(input, &mut actual).ok().unwrap();
                if text.starts_with("-45.054") {
                    transition[usize::from(matches!(
                        expected.classify(),
                        fastmash_numeric_contract::ValueClass::Zero
                    ))] = true;
                }
                assert_eq!(
                    numerics::Numerics::value80(result).raw(),
                    expected.raw(),
                    "{text} {bits:x}"
                );
            }
        }
        assert_eq!(transition, [true, true]);
        for (input, expected) in [
            ("0", integer(1)),
            ("-0x1p-16445", integer(1)),
            ("-20000", integer(0)),
            ("-inf", integer(0)),
            ("nan", quiet_nan()),
        ] {
            let result = probability(value(input), &mut actual).ok().unwrap();
            assert_eq!(
                numerics::Numerics::value80(result).raw(),
                numerics::Numerics::value80(expected).raw()
            );
        }
        assert!(!actual.has_session());
    }
}
