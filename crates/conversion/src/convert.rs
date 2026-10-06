//! Exact finite conversion rounded directly onto the binary80 target grid.

use fastmash_numeric_contract::{Raw80, Value80};
use num_bigint::BigUint;
use num_traits::Zero;
use std::cmp::Ordering;

use crate::bytes::{Exponent, FiniteToken};
use crate::integer::{self, Ratio, RemainderRelation};
use crate::profile::{EXPONENT_CAP, Error};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Overflow,
    UnderflowToZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MagnitudeBound {
    ExponentDominates {
        absolute_exponent_greater_than: u32,
    },
    DecimalOrder {
        significant_digits_plus_exponent: i32,
    },
    HexLeadingExponent {
        exponent: i32,
    },
}

#[derive(Clone, Debug)]
pub enum RoundEvidence {
    Zero,
    Ratio {
        quantum: i32,
        remainder: RemainderRelation,
        quotient_odd: bool,
        carry: bool,
    },
    MagnitudeShortcut {
        direction: Direction,
        exponent: Exponent,
        significant_digits: usize,
        fraction_digits: usize,
        bound: MagnitudeBound,
    },
}

#[derive(Clone, Debug)]
pub struct Conversion {
    pub value: Value80,
    pub rounding: RoundEvidence,
    pub exact_zero: bool,
    pub overflow: bool,
    pub target_inexact: bool,
    pub ordinary_p64_tiny: bool,
    pub exact_at_least_min_normal: bool,
    pub shortcut: Option<Direction>,
}

#[derive(Clone, Debug, Default)]
pub struct ConversionPartial {
    pub value: Option<Value80>,
    pub rounding: Option<RoundEvidence>,
}

#[derive(Clone, Debug)]
pub struct ConversionFailure {
    pub error: Error,
    pub partial: ConversionPartial,
}

impl From<Error> for ConversionFailure {
    fn from(error: Error) -> Self {
        Self {
            error,
            partial: ConversionPartial::default(),
        }
    }
}

pub fn zero(negative: bool) -> Result<Value80, Error> {
    Ok(Value80::from_raw(Raw80::new(
        if negative { 0x8000 } else { 0 },
        0,
    ))?)
}

pub fn infinity(negative: bool) -> Result<Value80, Error> {
    Ok(Value80::from_raw(Raw80::new(
        if negative { 0xffff } else { 0x7fff },
        1 << 63,
    ))?)
}

pub fn quiet_nan(negative: bool) -> Result<Value80, Error> {
    Ok(Value80::from_raw(Raw80::new(
        if negative { 0xffff } else { 0x7fff },
        0xc000_0000_0000_0000,
    ))?)
}

fn shortcut(
    token: &FiniteToken<'_>,
    direction: Direction,
    significant_digits: usize,
    bound: MagnitudeBound,
) -> Result<Conversion, ConversionFailure> {
    let overflow = direction == Direction::Overflow;
    Ok(Conversion {
        value: if overflow {
            infinity(token.negative())?
        } else {
            zero(token.negative())?
        },
        rounding: RoundEvidence::MagnitudeShortcut {
            direction,
            exponent: token.exponent(),
            significant_digits,
            fraction_digits: token.fraction_digits(),
            bound,
        },
        exact_zero: false,
        overflow,
        target_inexact: true,
        ordinary_p64_tiny: !overflow,
        exact_at_least_min_normal: overflow,
        shortcut: Some(direction),
    })
}

pub fn finite(token: &FiniteToken<'_>) -> Result<Conversion, ConversionFailure> {
    let first_nonzero = token.digits().position(|d| d != 0);
    let Some(first_nonzero) = first_nonzero else {
        return Ok(Conversion {
            value: zero(token.negative())?,
            rounding: RoundEvidence::Zero,
            exact_zero: true,
            overflow: false,
            target_inexact: false,
            ordinary_p64_tiny: false,
            exact_at_least_min_normal: false,
            shortcut: None,
        });
    };
    let significant_digits = token.digit_count() - first_nonzero;
    let exponent = match token.exponent() {
        Exponent::Finite(exponent) => exponent,
        Exponent::Greater { negative } => {
            // The old evidence-view bound implied exponent dominance. Borrowed
            // command tokens can be longer, so establish that bound explicitly.
            if token.digit_count().saturating_mul(4).saturating_add(20000) >= EXPONENT_CAP as usize
            {
                return Err(Error::IntermediateCapacity.into());
            }
            return shortcut(
                token,
                if negative {
                    Direction::UnderflowToZero
                } else {
                    Direction::Overflow
                },
                significant_digits,
                MagnitudeBound::ExponentDominates {
                    absolute_exponent_greater_than: EXPONENT_CAP,
                },
            );
        }
    };
    let fractional = i32::try_from(token.fraction_digits()).map_err(|_| Error::InputCapacity)?;
    let fractional = if token.radix() == 16 {
        fractional
            .checked_mul(4)
            .ok_or(Error::IntermediateCapacity)?
    } else {
        fractional
    };
    let adjusted = exponent
        .checked_sub(fractional)
        .ok_or(Error::IntermediateCapacity)?;
    if token.radix() == 10 {
        let order = i32::try_from(significant_digits)
            .map_err(|_| Error::InputCapacity)?
            .checked_add(adjusted)
            .ok_or(Error::IntermediateCapacity)?;
        if order >= 5001 {
            return shortcut(
                token,
                Direction::Overflow,
                significant_digits,
                MagnitudeBound::DecimalOrder {
                    significant_digits_plus_exponent: order,
                },
            );
        }
        if order <= -5000 {
            return shortcut(
                token,
                Direction::UnderflowToZero,
                significant_digits,
                MagnitudeBound::DecimalOrder {
                    significant_digits_plus_exponent: order,
                },
            );
        }
    }
    let mut digits = Vec::new();
    digits
        .try_reserve_exact(significant_digits)
        .map_err(|_| Error::Allocation)?;
    digits.extend(token.digits().skip(first_nonzero));
    let coefficient = integer::coefficient(&digits, token.radix())?;
    let ratio = if token.radix() == 16 {
        let k = i32::try_from(coefficient.bits())
            .map_err(|_| Error::CoefficientCapacity)?
            .checked_sub(1)
            .and_then(|k| k.checked_add(adjusted))
            .ok_or(Error::IntermediateCapacity)?;
        if k >= 16384 {
            return shortcut(
                token,
                Direction::Overflow,
                significant_digits,
                MagnitudeBound::HexLeadingExponent { exponent: k },
            );
        }
        if k < -16446 {
            return shortcut(
                token,
                Direction::UnderflowToZero,
                significant_digits,
                MagnitudeBound::HexLeadingExponent { exponent: k },
            );
        }
        Ratio::shifted(coefficient, adjusted)?
    } else {
        let five = integer::power(5, adjusted.unsigned_abs())?;
        if adjusted >= 0 {
            Ratio::shifted(integer::mul(&coefficient, &five)?, adjusted)?
        } else {
            Ratio::new(coefficient, integer::shl(&five, adjusted.unsigned_abs())?)?
        }
    };
    round_ratio(&ratio, token.negative())
}

/// Whether the decimal of `digits` (most significant first, each below ten)
/// times 10^`scale` is exactly `significand` times 2^`exponent`, in
/// magnitude. For deciding whether a conversion was exact where a range
/// error depends on it: `significand` times 2^`exponent` is a binary80 value
/// (below 2^16384, and a multiple of 2^-16445), and `digits` are at most a
/// long decimal's kept digits. Outside those bounds it is an error, not an
/// answer.
pub fn decimal_equals(
    digits: &[u8],
    scale: i64,
    significand: u64,
    exponent: i64,
) -> Result<bool, Error> {
    let Some(start) = digits.iter().position(|&d| d != 0) else {
        return Ok(significand == 0);
    };
    if significand == 0 {
        return Ok(false);
    }
    // Trailing zeros move into the scale.
    let end = digits
        .iter()
        .rposition(|&d| d != 0)
        .map_or(start, |at| at + 1);
    let scale = scale
        .checked_add(i64::try_from(digits.len() - end).map_err(|_| Error::InputCapacity)?)
        .ok_or(Error::IntermediateCapacity)?;
    let digits = &digits[start..end];
    let twos = significand.trailing_zeros();
    let odd = BigUint::from(significand >> twos);
    let exponent = exponent
        .checked_add(i64::from(twos))
        .filter(|exponent| (-16445..16384).contains(exponent))
        .ok_or(Error::IntermediateCapacity)?;
    let coefficient = || BigUint::from_radix_be(digits, 10).ok_or(Error::InternalInvariant);
    if exponent < 0 {
        // An odd multiple of 2^-j is an odd multiple of 5^j over 10^j, whose
        // last digit is 5: exactly j fraction digits.
        if scale != exponent {
            return Ok(false);
        }
        let power = u32::try_from(-exponent).map_err(|_| Error::PowerCapacity)?;
        Ok(coefficient()? == integer::mul(&odd, &integer::power(5, power)?)?)
    } else {
        // An integer below 2^16384, so of at most 4,933 digits.
        let length = i64::try_from(digits.len()).map_err(|_| Error::InputCapacity)?;
        if scale < 0 || scale.saturating_add(length) > 4933 {
            return Ok(false);
        }
        let power = u32::try_from(scale).map_err(|_| Error::PowerCapacity)?;
        let decimal = integer::mul(&coefficient()?, &integer::power(10, power)?)?;
        let shift = u32::try_from(exponent).map_err(|_| Error::ShiftCapacity)?;
        Ok(decimal == integer::shl(&odd, shift)?)
    }
}

pub fn round_ratio(ratio: &Ratio, negative: bool) -> Result<Conversion, ConversionFailure> {
    let mut partial = ConversionPartial::default();
    let result = (|| -> Result<Conversion, Error> {
        let k = ratio.floor_log2()?;
        let quantum = (k - 63).max(-16445);
        let target = ratio.at_quantum(quantum)?;
        let inexact = target.relation != RemainderRelation::Zero;
        let bits = target.value.bits();
        let carry = bits > 64 || (k < -16382 && bits == 64);
        let rounding = RoundEvidence::Ratio {
            quantum,
            remainder: target.relation,
            quotient_odd: target.quotient_odd,
            carry,
        };
        partial.rounding = Some(rounding.clone());
        let (value, overflow) = if target.value.is_zero() {
            (zero(negative)?, false)
        } else {
            let result_k =
                i32::try_from(bits).map_err(|_| Error::IntermediateCapacity)? - 1 + quantum;
            if result_k > 16383 {
                (infinity(negative)?, true)
            } else if result_k < -16382 {
                if quantum != -16445 {
                    return Err(Error::InternalInvariant);
                }
                let sig = integer::as_u64(&target.value)?;
                (
                    Value80::from_raw(Raw80::new(if negative { 0x8000 } else { 0 }, sig))?,
                    false,
                )
            } else {
                let normalized = if bits > 64 {
                    &target.value >> (bits - 64) as usize
                } else {
                    integer::shl(&target.value, (64 - bits) as u32)?
                };
                let sig = integer::as_u64(&normalized)?;
                let exponent =
                    u16::try_from(result_k + 16383).map_err(|_| Error::InternalInvariant)?;
                (
                    Value80::from_raw(Raw80::new(
                        exponent | if negative { 0x8000 } else { 0 },
                        sig,
                    ))?,
                    false,
                )
            }
        };
        partial.value = Some(value);
        let exact_at_least_min_normal = ratio.compare_power_two(-16382)? != Ordering::Less;
        // This separate rounding determines source tininess only. It never becomes
        // the input to target rounding, including at the normal/subnormal boundary.
        // The completed target remains available if this later source work fails.
        let ordinary = ratio.at_quantum(k - 63)?;
        let ordinary_k = k + i32::from(ordinary.value.bits() > 64);
        let ordinary_p64_tiny = ordinary_k < -16382;
        Ok(Conversion {
            value,
            rounding,
            exact_zero: false,
            overflow,
            target_inexact: inexact || overflow,
            ordinary_p64_tiny,
            exact_at_least_min_normal,
            shortcut: None,
        })
    })();
    result.map_err(|error| ConversionFailure { error, partial })
}

pub fn value_ratio(value: Value80) -> Result<Ratio, Error> {
    if value.exponent() == 0x7fff {
        return Err(Error::InternalInvariant);
    }
    let exponent = if value.exponent() == 0 {
        -16445
    } else {
        i32::from(value.exponent()) - 16383 - 63
    };
    Ratio::shifted(BigUint::from(value.significand()), exponent)
}

pub fn integer_value(value: i64) -> Result<Value80, Error> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return zero(false);
    }
    let k = 63 - magnitude.leading_zeros();
    let sig = magnitude << (63 - k);
    let exponent = (k as u16) + 16383;
    Ok(Value80::from_raw(Raw80::new(
        exponent | if value < 0 { 0x8000 } else { 0 },
        sig,
    ))?)
}

#[cfg(test)]
mod tests {
    use super::decimal_equals;

    /// `n` as a significand and a power of two, as binary80 holds it.
    fn binary(n: u64) -> (u64, i64) {
        let shift = n.leading_zeros();
        (n << shift, -i64::from(shift))
    }

    #[test]
    fn decimals_equal_binary_values_exactly_or_not_at_all() {
        let half = (1u64 << 63, -64);
        assert_eq!(decimal_equals(&[5], -1, half.0, half.1), Ok(true));
        assert_eq!(decimal_equals(&[0, 5, 0, 0], -3, half.0, half.1), Ok(true));
        assert_eq!(decimal_equals(&[2, 5], -2, 1 << 63, -65), Ok(true));
        assert_eq!(decimal_equals(&[3], -1, half.0, half.1), Ok(false));
        assert_eq!(decimal_equals(&[5, 1], -2, half.0, half.1), Ok(false));
        let four_hundred = binary(400);
        assert_eq!(
            decimal_equals(&[4, 0, 0], 0, four_hundred.0, four_hundred.1),
            Ok(true)
        );
        assert_eq!(
            decimal_equals(&[4], 2, four_hundred.0, four_hundred.1),
            Ok(true)
        );
        assert_eq!(
            decimal_equals(&[4, 0, 1], 0, four_hundred.0, four_hundred.1),
            Ok(false)
        );
        assert_eq!(
            decimal_equals(&[4], -2, four_hundred.0, four_hundred.1),
            Ok(false)
        );
        assert_eq!(decimal_equals(&[0, 0], -99, 0, -16445), Ok(true));
        assert_eq!(decimal_equals(&[1], -99, 0, -16445), Ok(false));
        assert_eq!(decimal_equals(&[0], 0, 1, -16445), Ok(false));
        // The smallest subnormal, 2^-16445, has 16,445 fraction digits.
        let mut digits = vec![1u8];
        for _ in 0..16445 {
            let mut carry = 0;
            for digit in digits.iter_mut().rev() {
                let next = *digit * 5 + carry;
                *digit = next % 10;
                carry = next / 10;
            }
            while carry != 0 {
                digits.insert(0, carry % 10);
                carry /= 10;
            }
        }
        assert_eq!(decimal_equals(&digits, -16445, 1, -16445), Ok(true));
        assert_eq!(decimal_equals(&digits, -16444, 1, -16445), Ok(false));
        let last = digits.len() - 1;
        digits[last] -= 1;
        assert_eq!(decimal_equals(&digits, -16445, 1, -16445), Ok(false));
    }
}
