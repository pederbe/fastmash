//! Per-record numeric transformations, without a wider arithmetic session.
use super::{Failure, decimal, numeric_failure, numerics, unsupported};
use fastmash_conversion::profile::Profile;
use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended as Extended};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Rounding {
    Round,
    Floor,
    Ceil,
    Trunc,
    Frac,
}
impl Rounding {
    pub fn name(self) -> &'static str {
        match self {
            Self::Round => "round",
            Self::Floor => "floor",
            Self::Ceil => "ceil",
            Self::Trunc => "trunc",
            Self::Frac => "frac",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Extraction {
    Natural,
    Integer,
    Hex,
    Octal,
    Positive,
    Decimal,
}
impl Extraction {
    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            b'n' => Self::Natural,
            b'i' => Self::Integer,
            b'h' => Self::Hex,
            b'o' => Self::Octal,
            b'p' => Self::Positive,
            b'd' => Self::Decimal,
            _ => return None,
        })
    }
    fn accepts(self, byte: u8) -> bool {
        match self {
            Self::Hex => byte.is_ascii_hexdigit(),
            Self::Octal => (b'0'..=b'7').contains(&byte),
            Self::Natural => byte.is_ascii_digit(),
            Self::Integer => byte.is_ascii_digit() || matches!(byte, b'+' | b'-'),
            Self::Positive => byte.is_ascii_digit() || byte == b'.',
            Self::Decimal => byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.'),
        }
    }
}
pub(super) fn extract(
    bytes: &[u8],
    kind: Extraction,
    profile: Profile,
) -> Result<numerics::Value, Failure> {
    let bytes = &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())];
    let start = bytes
        .iter()
        .position(|b| kind.accepts(*b))
        .unwrap_or(bytes.len());
    let span = &bytes[start..];
    let end = span
        .iter()
        .position(|b| !kind.accepts(*b))
        .unwrap_or(span.len());
    let mut token = &span[..end];
    if matches!(kind, Extraction::Positive | Extraction::Decimal) {
        return decimal::extracted_prefix(token, profile);
    }
    let negative = token.first() == Some(&b'-');
    if token.first().is_some_and(|b| matches!(b, b'+' | b'-')) {
        token = &token[1..];
    }
    let base = match kind {
        Extraction::Hex => 16,
        Extraction::Octal => 8,
        _ => 10,
    };
    let limit = (i64::MAX as u64) + u64::from(negative);
    let mut value = 0u64;
    for &byte in token {
        let Some(digit) = (byte as char).to_digit(base) else {
            break;
        };
        let Some(next) = value
            .checked_mul(base.into())
            .and_then(|v| v.checked_add(digit.into()))
            .filter(|v| *v <= limit)
        else {
            return Ok(super::integer(0));
        };
        value = next;
    }
    let x = Extended::from_u128(value.into()).value;
    admitted(if negative && value != 0 { -x } else { x })
}
fn admitted(x: Extended) -> Result<numerics::Value, Failure> {
    let raw = fastmash_numeric_contract::Raw80::try_from_bits(x.to_bits())
        .map_err(|_| unsupported("internal line numeric encoding failure"))?;
    let value = fastmash_numeric_contract::Value80::from_raw(raw)
        .map_err(|_| unsupported("internal line numeric encoding failure"))?;
    numerics::Numerics::admit(value).map_err(numeric_failure)
}
pub(super) fn rounding(value: numerics::Value, kind: Rounding) -> Result<numerics::Value, Failure> {
    let x = Extended::from_bits(numerics::Numerics::value80(value).raw().to_bits());
    let result = if x.is_nan() {
        x
    } else if kind == Rounding::Frac {
        if x.is_infinite() {
            Extended::ZERO
        } else {
            x.sub_r(
                x.round_to_integral(Round::TowardZero).value,
                Round::NearestTiesToEven,
            )
            .value
        }
    } else {
        x.round_to_integral(match kind {
            Rounding::Round => Round::NearestTiesToAway,
            Rounding::Floor => Round::TowardNegative,
            Rounding::Ceil => Round::TowardPositive,
            Rounding::Trunc => Round::TowardZero,
            Rounding::Frac => unreachable!(),
        })
        .value
    };
    // Approved difference: only actual zero is normalized; negative NaN stays NaN.
    admitted(if result.is_zero() {
        Extended::ZERO
    } else {
        result
    })
}

/// Stable x86-64 GNU assignments over the complete field, including NUL bytes.
pub(super) fn strbin(bytes: &[u8], buckets: std::num::NonZeroU64) -> u64 {
    bytes.iter().fold(0u64, |hash, byte| {
        hash.rotate_left(9).wrapping_add(u64::from(*byte))
    }) % buckets.get()
}

pub(super) fn bin(
    value: numerics::Value,
    width: fastmash_numeric_contract::Raw80,
) -> Result<numerics::Value, Failure> {
    let x = Extended::from_bits(numerics::Numerics::value80(value).raw().to_bits());
    let width = Extended::from_bits(width.to_bits());
    // Input NaN propagation is separate from zero-width compatibility below.
    if x.is_nan() {
        return admitted(x);
    }
    if width.is_zero() {
        // Preserve GNU's defined zero-width results for non-NaN input.
        return admitted(if x.is_zero() {
            Extended::ZERO
        } else {
            -Extended::NAN
        });
    }
    let quotient = x.div_r(width, Round::NearestTiesToEven).value;
    let integral = quotient.round_to_integral(Round::TowardNegative).value;
    let integral = if integral.is_zero() {
        Extended::ZERO
    } else {
        integral
    };
    admitted(integral.mul_r(width, Round::NearestTiesToEven).value)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smallest_subnormal_rounds_without_decimal_or_host_arithmetic() {
        for negative in [false, true] {
            let raw = fastmash_numeric_contract::Raw80::new(if negative { 0x8000 } else { 0 }, 1);
            let value = fastmash_numeric_contract::Value80::from_raw(raw).unwrap();
            let value = numerics::Numerics::admit(value).unwrap();
            for kind in [
                Rounding::Round,
                Rounding::Floor,
                Rounding::Ceil,
                Rounding::Trunc,
                Rounding::Frac,
            ] {
                let result = rounding(value, kind).ok().unwrap();
                let result = numerics::Numerics::value80(result);
                let expected = if kind == Rounding::Frac {
                    raw
                } else if negative && kind == Rounding::Floor {
                    fastmash_numeric_contract::Raw80::new(0xbfff, 1 << 63)
                } else if !negative && kind == Rounding::Ceil {
                    fastmash_numeric_contract::Raw80::new(0x3fff, 1 << 63)
                } else {
                    fastmash_numeric_contract::Raw80::new(0, 0)
                };
                assert_eq!(result.raw(), expected, "{negative} {kind:?}");
            }
        }
    }
}
