//! Percentile positions and trimmed means over the shared stable sample order.
use super::{Failure, decimal, numerics, unsupported};
use fastmash_conversion::{presentation::Spec, profile::Profile};
use fastmash_numeric_contract::{Raw80, Value80};
use rustc_apfloat::{
    Float, FloatConvert, Round, Status,
    ieee::{Double, X87DoubleExtended as Extended},
};

/// Validated binary80 fraction, separate from its rounded header spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Trim(Raw80);

impl Trim {
    pub(super) fn zero() -> Self {
        Self(Raw80::new(0, 0))
    }

    pub(super) fn new(value: Value80, profile: Profile) -> Result<Self, Failure> {
        let x = Extended::from_bits(value.raw().to_bits());
        let half = Extended::from_u128(1).value / Extended::from_u128(2).value;
        if x < Extended::ZERO || x > half.value || x.is_nan() {
            let mut message = b"invalid trim mean value ".to_vec();
            message.extend(display(value, profile)?);
            message.extend_from_slice(b" (expected 0 <= X <= 0.5)\n");
            return Err(super::failure(message));
        }
        Ok(Self(value.raw()))
    }

    pub(super) fn display(self, profile: Profile) -> Result<Vec<u8>, Failure> {
        display(self.value(), profile)
    }

    fn value(self) -> Value80 {
        Value80::from_raw(self.0).expect("validated trim")
    }

    pub(super) fn is_half(self) -> bool {
        self.0 == Raw80::new(0x3ffe, 1 << 63)
    }

    fn count(self, n: usize) -> Result<usize, Failure> {
        let x = Extended::from_bits(self.0.to_bits());
        let product = x
            .mul_r(
                Extended::from_u128(n as u128).value,
                Round::NearestTiesToEven,
            )
            .value;
        let count = product.to_u128_r(usize::BITS as usize, Round::TowardZero, &mut false);
        if count.status.intersects(Status::INVALID_OP) {
            return Err(unsupported("internal trim count out of range"));
        }
        usize::try_from(count.value).map_err(|_| unsupported("internal trim count out of range"))
    }
}

fn display(value: Value80, profile: Profile) -> Result<Vec<u8>, Failure> {
    Spec::parse(b"%g")
        .expect("fixed GNU parameter format")
        .render(value, profile)
        .map_err(super::conversion_failure)
}

/// Reproduce each binary64 rounding in GNU's count conversion and position.
/// Kept separate from the frozen, bounded portable-profile helper.
pub(super) fn position(n: usize, percent: u8) -> Result<(usize, numerics::Value), Failure> {
    if n < 2 || !(1..100).contains(&percent) {
        return Err(unsupported("internal percentile position domain"));
    }
    let ratio = Double::from_u128(percent.into())
        .value
        .div_r(Double::from_u128(100).value, Round::NearestTiesToEven)
        .value;
    let h = Double::from_u128((n - 1) as u128)
        .value
        .mul_r(ratio, Round::NearestTiesToEven)
        .value;
    let index = h.to_u128_r(usize::BITS as usize, Round::TowardZero, &mut false);
    let index = usize::try_from(index.value)
        .ok()
        .filter(|&i| i < n - 1)
        .ok_or_else(|| unsupported("internal percentile successor out of range"))?;
    let fraction = h
        .sub_r(
            Double::from_u128(index as u128).value,
            Round::NearestTiesToEven,
        )
        .value;
    let fraction: Extended = fraction
        .convert_r(Round::NearestTiesToEven, &mut false)
        .value;
    let raw = Raw80::try_from_bits(fraction.to_bits()).expect("binary64 promoted to binary80");
    let value = Value80::from_raw(raw).expect("binary64 promotion is canonical");
    Ok((
        index,
        numerics::Numerics::admit(value).map_err(super::numeric_failure)?,
    ))
}

pub(super) fn percentile(
    sorted: &[numerics::Value],
    percent: u8,
) -> Result<numerics::Value, Failure> {
    if percent == 100 || sorted.len() == 1 {
        return sorted
            .last()
            .copied()
            .ok_or_else(|| unsupported("internal percentile sample state missing"));
    }
    let (index, fraction) = position(sorted.len(), percent)?;
    let delta = decimal::subtract(sorted[index + 1], sorted[index])?;
    decimal::add(sorted[index], decimal::multiply(delta, fraction)?)
}

pub(super) fn trimmed(sorted: &[numerics::Value], trim: Trim) -> Result<numerics::Value, Failure> {
    let count = trim.count(sorted.len())?;
    let end = sorted
        .len()
        .checked_sub(count)
        .filter(|&end| count < end)
        .ok_or_else(|| unsupported("internal trimmed sample range"))?;
    let mut sum = numerics::Numerics::promote_u64(0);
    for &value in &sorted[count..end] {
        sum = decimal::add(sum, value)?;
    }
    decimal::mean(sum, (end - count) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numerics::test_support::Inspect;

    #[test]
    fn positions_preserve_frozen_results_and_round_large_counts() {
        for n in [2, 3, 4, 7, 10, 101, 999, 65_536] {
            for percent in 1..100 {
                let old = fastmash_portable_numerics::percentile_position(n, percent);
                let new = position(n, percent).ok().unwrap();
                assert_eq!(old.0, new.0);
                assert!(old.1.same_bits(new.1));
            }
        }
        // Integer 2^53+1 rounds to even 2^53 before multiplication by one half.
        let (index, fraction) = position((1usize << 53) + 2, 50).ok().unwrap();
        assert_eq!(index, 1usize << 52);
        assert!(numerics::Numerics::value80(fraction).same_bits(Value80::signed_zero(false)));
        for n in [65_537, 1_000_000, (1usize << 53) - 1, usize::MAX] {
            for percent in 1..100 {
                let (index, fraction) = position(n, percent).ok().unwrap();
                assert!(index + 1 < n);
                assert!(!numerics::Numerics::value80(fraction).is_negative());
            }
        }
    }

    #[test]
    fn trim_count_rounds_binary80_before_floor() {
        // Neighbors around one tenth; at ten samples the lower neighbor retains
        // both tails while rounded decimal 0.1 removes one value from each end.
        let below = Trim(Raw80::new(0x3ffb, 0xcccc_cccc_cccc_cccc));
        let tenth = Trim(Raw80::new(0x3ffb, 0xcccc_cccc_cccc_cccd));
        assert_eq!(below.count(10).ok(), Some(0));
        assert_eq!(tenth.count(10).ok(), Some(1));
        assert_eq!(tenth.count(9).ok(), Some(0));
        assert_eq!(tenth.count(11).ok(), Some(1));
    }
}
