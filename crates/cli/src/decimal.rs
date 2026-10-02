//! Borrowed conversion and basic arithmetic, independent of wider-math state.
use super::{
    Failure, conversion_failure, decimal_powers, failure, numeric_failure, numerics, unsupported,
};
use fastmash_conversion::{
    bytes::{self, Lexeme},
    convert, field_policy,
    profile::Profile,
};
use fastmash_numeric_contract::{Raw80, Value80};
use rustc_apfloat::{
    Float, Round, Status,
    ieee::{IeeeFloat, Semantics, X87DoubleExtended as Extended},
};

// The reference classifies tininess after ordinary p64 rounding, before the
// binary80 exponent restriction. This auxiliary type never supplies output bits.
struct Ordinary64;
impl Semantics for Ordinary64 {
    const BITS: usize = 80;
    const EXP_BITS: usize = 16;
}

/// The value of `text` and whether it is a range error. A range error of a
/// zero, subnormal or smallest normal result needs the conversion to be
/// inexact, which `exact` decides, given the value and whether the converter
/// found it exact: the converter's own finding can be wrong for decimals.
fn finite(
    text: &str,
    exact: impl FnOnce(Value80, bool) -> Result<bool, Failure>,
) -> Result<(Value80, bool), Failure> {
    let invalid = || unsupported("internal decimal conversion failure");
    let parsed = Extended::from_str_r(text, Round::NearestTiesToEven).map_err(|_| invalid())?;
    if parsed.status.contains(Status::INVALID_OP) {
        return Err(invalid());
    }
    let tiny = parsed.value.is_zero()
        || parsed.value.is_denormal()
        || parsed.value.is_smallest_normalized();
    let range = if parsed.status.contains(Status::OVERFLOW) {
        true
    } else if !tiny
        || exact(
            value(parsed.value)?,
            !parsed.status.contains(Status::INEXACT),
        )?
    {
        false
    } else if parsed.value.is_smallest_normalized() {
        let ordinary = IeeeFloat::<Ordinary64>::from_str_r(text, Round::NearestTiesToEven)
            .map_err(|_| invalid())?;
        if !ordinary.value.is_finite_non_zero()
            || ordinary.value.is_denormal()
            || ordinary
                .status
                .intersects(Status::INVALID_OP | Status::OVERFLOW | Status::UNDERFLOW)
        {
            return Err(invalid());
        }
        ordinary.value.ilogb() < Extended::MIN_EXP
    } else {
        parsed.value.is_zero() || parsed.value.is_denormal()
    };
    Ok((value(parsed.value)?, range))
}

/// Significant digits a long decimal keeps; the rest fold into one sticky
/// digit. Conversion to binary80 changes its value only at a representable
/// value or a midpoint between two, and its range error only there and at
/// the range boundaries; the tininess check at 64-bit precision runs only
/// near the smallest normal, where its points are of the same kind. Each is
/// an odd multiple below 2^66 of a power of two from 2^-16448 to 2^16384,
/// whose decimal expansion has at most log10(2^66 * 5^16448) + 1 < 11,518
/// significant digits. The first 11,600 digits, followed by a 1 where any
/// later digit is not zero, therefore lie strictly between the same two such
/// points as the whole decimal, or are it exactly, and round the same way;
/// and where a later digit is not zero, the value is inexact, whatever the
/// converter reports. The converter's digit loop, whose time grows with the
/// square of the digits, stays short.
const SIGNIFICANT: usize = 11_600;

/// A decimal `token` of more significant digits than [`SIGNIFICANT`], as the
/// shorter decimal that converts the same way; `None` for any other token.
fn shortened(token: &bytes::FiniteToken<'_>) -> Result<Option<Shortened>, Failure> {
    if token.radix() != 10 || token.digit_count() <= SIGNIFICANT {
        return Ok(None);
    }
    // Beyond the lexer's exponent bound, the digits can still bring the
    // value back into range.
    let exponent = token.exponent_saturating();
    let mut digits = token.digits().skip_while(|&digit| digit == 0);
    let mut text = Vec::new();
    // A sign, the kept digits, a sticky digit and an exponent of at most 21
    // characters.
    super::command_memory::reserve(&mut text, SIGNIFICANT + 24)?;
    if token.negative() {
        text.push(b'-');
    }
    let start = text.len();
    text.extend(digits.by_ref().take(SIGNIFICANT).map(|digit| b'0' + digit));
    if text.len() - start < SIGNIFICANT {
        return Ok(None);
    }
    let (mut dropped, mut sticky) = (0i64, false);
    for digit in digits {
        dropped += 1;
        sticky |= digit != 0;
    }
    let kept = start..text.len();
    if sticky {
        text.push(b'1');
    }
    let fraction = i64::try_from(token.fraction_digits())
        .map_err(|_| unsupported("numeric token exceeds converter index range"))?;
    let scale = exponent - fraction + dropped - i64::from(sticky);
    text.extend_from_slice(format!("e{scale}").as_bytes());
    Ok(Some(Shortened {
        text,
        kept,
        scale: scale + i64::from(sticky),
        sticky,
    }))
}

/// A long decimal as the shorter one that converts the same way ([`shortened`]).
struct Shortened {
    /// The decimal: a sign, the kept digits, any sticky digit and an exponent.
    text: Vec<u8>,
    /// Where the kept digits are in `text`.
    kept: std::ops::Range<usize>,
    /// The power of ten of the kept digits' last.
    scale: i64,
    /// Whether a digit dropped was not zero: then the value is inexact.
    sticky: bool,
}

/// Whether `token` is exactly `value`, which the converter found exact or not
/// (`converter`). Hexadecimal conversion finds it exactly. A decimal with a
/// sticky digit ([`shortened`]) is inexact; otherwise its digits (the kept
/// ones, or its own, at most [`SIGNIFICANT`] significant digits) are
/// compared with the value exactly.
fn exact(
    token: &bytes::FiniteToken<'_>,
    shortened: Option<&Shortened>,
    value: Value80,
    converter: bool,
) -> Result<bool, Failure> {
    if token.radix() != 10 {
        return Ok(converter);
    }
    let (digits, scale) = match shortened {
        Some(shortened) if shortened.sticky => return Ok(false),
        Some(shortened) => {
            let mut digits = Vec::new();
            super::command_memory::reserve(&mut digits, shortened.kept.len())?;
            digits.extend(
                shortened.text[shortened.kept.clone()]
                    .iter()
                    .map(|b| b - b'0'),
            );
            (digits, shortened.scale)
        }
        None => {
            let mut digits = Vec::new();
            super::command_memory::reserve(&mut digits, token.digit_count().min(SIGNIFICANT))?;
            digits.extend(token.digits().skip_while(|&digit| digit == 0));
            let fraction = i64::try_from(token.fraction_digits())
                .map_err(|_| unsupported("numeric token exceeds converter index range"))?;
            (digits, token.exponent_saturating() - fraction)
        }
    };
    let biased = i64::from(value.sign_exponent() & 0x7fff);
    // The value is its significand times 2^(exponent - 16383 - 63), and a
    // subnormal's exponent reads as 1.
    let exponent = biased.max(1) - 16383 - 63;
    convert::decimal_equals(&digits, scale, value.significand(), exponent)
        .map_err(|_| unsupported("internal decimal conversion failure"))
}

fn admit(value: Value80) -> Result<numerics::Value, Failure> {
    #[cfg(test)]
    let value = super::operation_set::tests::parsed_value(value);
    numerics::Numerics::admit(value).map_err(numeric_failure)
}

fn value(x: Extended) -> Result<Value80, Failure> {
    Raw80::try_from_bits(x.to_bits())
        .ok()
        .and_then(|raw| Value80::from_raw(raw).ok())
        .ok_or_else(|| unsupported("internal decimal encoding failure"))
}

/// One parse of a numeric prefix, as `strtold` reports it.
struct Parsed {
    /// `None` only when no number begins the input, so nothing was consumed.
    value: Option<Value80>,
    consumed: usize,
    /// Whether the value is a range error (`ERANGE`).
    range_error: bool,
}

/// GNU extraction permits a numeric prefix and maps known range errors to zero.
pub(super) fn extracted_prefix(input: &[u8], profile: Profile) -> Result<numerics::Value, Failure> {
    let parsed = parse(input, profile)?;
    match parsed.value {
        Some(value) if !parsed.range_error => admit(value),
        _ => Ok(super::integer(0)),
    }
}

/// Convert a small decimal rational with one rounding, reusing the validated lexer.
/// The bounds select an optimization; all other tokens keep the general converter.
fn compact_decimal(token: &bytes::FiniteToken<'_>) -> Option<Extended> {
    if token.radix() != 10 || token.digit_count() > 19 {
        return None;
    }
    let bytes::Exponent::Finite(exponent) = token.exponent() else {
        return None;
    };
    let scale = exponent.checked_sub(i32::try_from(token.fraction_digits()).ok()?)?;
    if !(-19..=19).contains(&scale) {
        return None;
    }
    let coefficient = token
        .digits()
        .fold(0u64, |n, digit| n * 10 + u64::from(digit));
    compact_scaled(token.negative(), coefficient, scale)
}

/// A hexadecimal token with at most 32 coefficient digits, converted with exact
/// integer arithmetic and one nearest-even rounding. Only results that are
/// normal and away from both range boundaries (biased exponent 2..=0x7ffe
/// after rounding) take this path: they are never range errors, and the
/// general converter rounds the same exact value the same way. Everything else,
/// including zero, keeps the general converter and its range flags.
fn compact_hex(token: &bytes::FiniteToken<'_>) -> Option<Value80> {
    if token.radix() != 16 || token.digit_count() > 32 {
        return None;
    }
    let bytes::Exponent::Finite(exponent) = token.exponent() else {
        return None;
    };
    // The value is coefficient * 2^scale; a coefficient of 1..=4*digits bits
    // bounds the biased exponent, so far-out values leave before any work.
    let scale = i64::from(exponent) - 4 * i64::try_from(token.fraction_digits()).ok()?;
    let digits = i64::try_from(token.digit_count()).ok()?;
    if scale + 16383 > 0x7ffe || 4 * digits - 1 + scale + 16383 < 2 {
        return None;
    }
    // At most 32 digits: the shifted coefficient never loses bits.
    let coefficient = token
        .digits()
        .fold(0u128, |n, digit| (n << 4) | u128::from(digit));
    if coefficient == 0 {
        return None;
    }
    let bits = 128 - coefficient.leading_zeros();
    // Leave out-of-range values early; a rounding carry is checked below.
    if !(2..=0x7ffe).contains(&(i64::from(bits) - 1 + scale + 16383)) {
        return None;
    }
    let normalized = coefficient << (128 - bits);
    let mut significand = normalized >> 64;
    let remainder = normalized as u64;
    if remainder > 1 << 63 || (remainder == 1 << 63 && significand & 1 != 0) {
        significand += 1;
    }
    let mut biased = i64::from(bits) - 1 + scale + 16383;
    if significand == 1 << 64 {
        significand >>= 1;
        biased += 1;
    }
    if !(2..=0x7ffe).contains(&biased) {
        return None;
    }
    Some(Value80::canonical_normal(
        token.negative(),
        biased as u16,
        significand as u64,
    ))
}

/// A decimal of at most 19 digits whose power of ten lies outside
/// [`compact_decimal`]'s window, such as the `1.234568e-25` of `%e` output or
/// an e-value like `4e-115`. The coefficient c and 10^scale = 5^scale * 2^scale
/// meet in an interval of width c * 5^r (from the table's 5^(28 j), truncated
/// to 128 bits, and the exact 5^r, 0 <= r < 28) that holds the exact product.
/// Rounding is monotone, so when both ends round to the same normal value
/// below the overflow boundary ([`normal_result`]), that is the correctly
/// rounded value and no range error.
/// Otherwise the general converter decides; that includes an exact tie
/// unless the interval's ends both round to its even neighbour.
fn scaled_decimal(token: &bytes::FiniteToken<'_>) -> Option<Value80> {
    if token.radix() != 10 || token.digit_count() > 19 {
        return None;
    }
    let bytes::Exponent::Finite(exponent) = token.exponent() else {
        return None;
    };
    let scale = exponent.checked_sub(i32::try_from(token.fraction_digits()).ok()?)?;
    let coefficient = token
        .digits()
        .fold(0u64, |n, digit| n * 10 + u64::from(digit));
    scaled_value(token.negative(), coefficient, scale)
}

/// [`scaled_decimal`]'s value of a coefficient below 10^19 times 10^scale.
#[inline(always)]
fn scaled_value(negative: bool, coefficient: u64, scale: i32) -> Option<Value80> {
    if coefficient == 0 {
        return None;
    }
    let block = scale.div_euclid(decimal_powers::STEP);
    let index = usize::try_from(block.checked_sub(decimal_powers::FIRST)?).ok()?;
    let &(power, binary) = decimal_powers::FIVE.get(index)?;
    // c * 5^r < 10^19 * 5^27 < 2^127: exact.
    let exact = u128::from(coefficient)
        * u128::from(5u64.pow(scale.rem_euclid(decimal_powers::STEP).unsigned_abs()));
    let (high, low) = widening_mul(exact, power);
    let (upper, carry) = low.overflowing_add(exact);
    let rounded = round_nearest(high, low);
    if round_nearest(high + u128::from(carry), upper) != rounded {
        return None;
    }
    normal_result(negative, rounded, i64::from(binary) + i64::from(scale))
}

/// The value `rounded` (a significand and the bit position of its leading
/// one) times 2^`exponent`, where both ends of an interval holding the exact
/// value round to it at 64 bits with an unbounded exponent; `None` beyond
/// both range boundaries. At biased exponent 1 that value is also binary80's
/// (the smallest normal's neighbourhood: the denormal grid's midpoint below
/// 2^-16382 lies further down than the 64-bit one) and, being normal after
/// that rounding, not tiny, so no range error.
#[inline(always)]
fn normal_result(negative: bool, (significand, top): (u64, u32), exponent: i64) -> Option<Value80> {
    let biased = i64::from(top) + exponent + 16383;
    (1..=0x7ffe)
        .contains(&biased)
        .then(|| Value80::canonical_normal(negative, biased as u16, significand))
}

/// A decimal of more than 19 digits, which [`scaled_decimal`] leaves, such as
/// the `0.1234567890123456789` of `%.19f` or other high-precision output. Its
/// first 38 significant digits c (below 10^38), the later ones folded into
/// whether any is not zero, and its power of ten 10^s = 5^r * 5^(STEP*j) *
/// 2^s bound the exact value: c * 5^r (or (c + 1) * 5^r when a later digit
/// is not zero), cut to at most 127 bits, times [m, m + 1) from the table.
/// As in [`scaled_decimal`], when both ends round to the same normal value
/// ([`normal_result`]), that is the correctly rounded value and no range
/// error; otherwise the general converter decides. A zero is zero.
#[inline(never)]
fn long_decimal(token: &bytes::FiniteToken<'_>) -> Option<Value80> {
    if token.radix() != 10 || token.digit_count() <= 19 {
        return None;
    }
    let bytes::Exponent::Finite(exponent) = token.exponent() else {
        return None;
    };
    let (mut coefficient, mut kept, mut dropped, mut sticky) = (0u128, 0u32, 0i64, false);
    for digit in token.digits().skip_while(|&digit| digit == 0) {
        if kept < 38 {
            coefficient = coefficient * 10 + u128::from(digit);
            kept += 1;
        } else {
            dropped += 1;
            sticky |= digit != 0;
        }
    }
    if kept == 0 {
        // Zero, as `%.20f` writes it: never a range error.
        return Some(Value80::signed_zero(token.negative()));
    }
    let scale = i64::from(exponent) - i64::try_from(token.fraction_digits()).ok()? + dropped;
    let scale = i32::try_from(scale).ok()?;
    let block = scale.div_euclid(decimal_powers::STEP);
    let index = usize::try_from(block.checked_sub(decimal_powers::FIRST)?).ok()?;
    let &(power, binary) = decimal_powers::FIVE.get(index)?;
    // 5^r < 2^63 and c + 1 <= 10^38 < 2^127: both products fit in 190 bits.
    let five = u128::from(5u64.pow(scale.rem_euclid(decimal_powers::STEP).unsigned_abs()));
    let (low_high, low_low) = widening_mul(coefficient, five);
    let (high_high, high_low) = widening_mul(coefficient + u128::from(sticky), five);
    // Cut both ends to below 2^127, rounding the lower down and the upper up.
    let bits = 256
        - if high_high == 0 {
            128 + high_low.leading_zeros()
        } else {
            high_high.leading_zeros()
        };
    let cut = bits.saturating_sub(127);
    // At most 190 bits: the cut is below 64.
    let shifted = |high: u128, low: u128| match cut {
        0 => (low, false),
        _ => (
            (high << (128 - cut)) | (low >> cut),
            low << (128 - cut) != 0,
        ),
    };
    let (lower, _) = shifted(low_high, low_low);
    let (upper, rest) = shifted(high_high, high_low);
    let upper = upper + u128::from(rest);
    let rounded = round_nearest_product(lower, power, false);
    if round_nearest_product(upper, power, true) != rounded {
        return None;
    }
    normal_result(
        token.negative(),
        rounded,
        i64::from(binary) + i64::from(scale) + i64::from(cut),
    )
}

/// `coefficient` times `power`, or times `power + 1` when `above`, rounded
/// as [`round_nearest`] rounds it; `coefficient` is at most 2^127.
fn round_nearest_product(coefficient: u128, power: u128, above: bool) -> (u64, u32) {
    let (high, low) = widening_mul(coefficient, power);
    if !above {
        return round_nearest(high, low);
    }
    let (low, carry) = low.overflowing_add(coefficient);
    round_nearest(high + u128::from(carry), low)
}

/// The 256-bit product of `a` and `b`, as its high and low halves.
fn widening_mul(a: u128, b: u128) -> (u128, u128) {
    let (a1, a0) = (a >> 64, a & u128::from(u64::MAX));
    let (b1, b0) = (b >> 64, b & u128::from(u64::MAX));
    let (high, middle_a, middle_b, low) = (a1 * b1, a1 * b0, a0 * b1, a0 * b0);
    // Each partial product is below 2^128; the sum of the middle terms and the
    // carries fits in the high half.
    let (middle, carry) = middle_a.overflowing_add(middle_b);
    let (low, low_carry) = low.overflowing_add(middle << 64);
    let high = high + (middle >> 64) + (u128::from(carry) << 64) + u128::from(low_carry);
    (high, low)
}

/// The nonzero 256-bit integer `high`:`low` rounded to 64 significant bits,
/// nearest even: the significand and the bit position of its leading one.
fn round_nearest(high: u128, low: u128) -> (u64, u32) {
    let zeros = if high == 0 {
        128 + low.leading_zeros()
    } else {
        high.leading_zeros()
    };
    debug_assert!(zeros < 256, "round_nearest of zero");
    let (high, low) = match zeros {
        0 => (high, low),
        1..=127 => ((high << zeros) | (low >> (128 - zeros)), low << zeros),
        _ => (low << (zeros - 128), 0),
    };
    let significand = (high >> 64) as u64;
    let rest = high as u64;
    let half = 1u64 << 63;
    let up = rest > half || (rest == half && (low != 0 || significand & 1 != 0));
    let top = 255 - zeros;
    match significand.checked_add(u64::from(up)) {
        Some(significand) => (significand, top),
        None => (half, top + 1),
    }
}

/// The value of a signed coefficient times 10^scale, |scale| <= 19, with at
/// most one rounding. Shared by the lexer path and `scientific_number`.
fn compact_scaled(negative: bool, coefficient: u64, scale: i32) -> Option<Extended> {
    debug_assert!((-19..=19).contains(&scale), "compact scale {scale}");
    let power = 10u64.pow(scale.unsigned_abs());
    let result = if scale >= 0 {
        // The product is below 10^38: form it exactly before the only rounding.
        let product = u128::from(coefficient) * u128::from(power);
        match u64::try_from(product) {
            Ok(integer) => compact_integer(integer),
            Err(_) => Extended::from_u128_r(product, Round::NearestTiesToEven).value,
        }
    } else {
        compact_ratio(coefficient, std::num::NonZeroU64::new(power)?)
    };
    Some(if negative { -result } else { result })
}

/// A plain decimal field, `-`? digits (point digits)?, with at most 19 digits
/// in total, converted without the lexer through the lexer path's own
/// `compact_scaled`, so the bits are identical. The general parser reads from
/// the field start through the record, so this applies only when the byte
/// after the field (`next`) cannot continue a number. After digits only a
/// digit, the radix point, `e`/`E` or (after `0`) `x` can; the guard rejects
/// every letter and digit and the radix point. With `-t e`, for example, the
/// field `1` of `1e9999` is a range error, not 1.
fn plain_number(part: &[u8], next: Option<u8>, point: u8) -> Option<Value80> {
    if !shortcuts() {
        return None;
    }
    if next.is_some_and(|b| b.is_ascii_alphanumeric() || b == point) {
        return None;
    }
    let (negative, body) = match part {
        [b'-', rest @ ..] => (true, rest),
        _ => (false, part),
    };
    // At least one whole digit, then optionally the point and at least one
    // fraction digit; at most 19 digits, so the coefficient cannot overflow.
    let digits = |bytes: &[u8], coefficient: u64| {
        bytes.iter().try_fold(coefficient, |n, &byte| {
            let digit = byte.wrapping_sub(b'0');
            (digit <= 9).then(|| n.wrapping_mul(10).wrapping_add(u64::from(digit)))
        })
    };
    let (whole, fraction) = match body.iter().position(|&b| b == point) {
        Some(at) => (&body[..at], &body[at + 1..]),
        None => (body, &[][..]),
    };
    if whole.is_empty()
        || (fraction.is_empty() && whole.len() < body.len())
        || whole.len() + fraction.len() > 19
    {
        return None;
    }
    let coefficient = digits(fraction, digits(whole, 0)?)?;
    let fraction = fraction.len() as u32;
    // The same bits compact_scaled produces, without the Extended round trip.
    if coefficient == 0 {
        return Some(Value80::signed_zero(negative));
    }
    let (significand, exponent) = if fraction == 0 {
        let shift = coefficient.leading_zeros();
        (coefficient << shift, 63 - shift as i32)
    } else {
        rounded_ratio(coefficient, 10u64.pow(fraction))
    };
    Some(Value80::canonical_normal(
        negative,
        (exponent + 16383) as u16,
        significand,
    ))
}

/// A decimal field in scientific notation, `-`? digits (point digits)? then
/// `e` or `E`, a sign or none and at most five digits, with at most 19
/// coefficient digits: the `1.234568e-25` of `%e` output, BLAST e-values and
/// GWAS p-values. Like [`plain_number`] it reads the field without the lexer,
/// where the byte after it (`next`) cannot continue a number (no letter,
/// digit or radix point), and converts through the lexer path's own
/// [`compact_scaled`] or [`scaled_value`], so the bits are identical; `None`
/// leaves every other field, and every value those leave, to the general
/// parser. `marker` is the field's last `e` or `E` ([`exponent_marker`]).
#[inline(never)]
fn scientific_number(part: &[u8], marker: usize, next: Option<u8>, point: u8) -> Option<Value80> {
    if !shortcuts() {
        return None;
    }
    if next.is_some_and(|b| b.is_ascii_alphanumeric() || b == point) {
        return None;
    }
    let negative = part[0] == b'-';
    let (mantissa, exponent) = (&part[usize::from(negative)..marker], &part[marker + 1..]);
    let (whole, fraction) = match mantissa.iter().position(|&b| b == point) {
        Some(at) => (&mantissa[..at], &mantissa[at + 1..]),
        None => (mantissa, &[][..]),
    };
    if whole.is_empty()
        || (fraction.is_empty() && whole.len() < mantissa.len())
        || whole.len() + fraction.len() > 19
    {
        return None;
    }
    // A plain loop, not `plain_number`'s `try_fold`: a second user of that
    // iterator instance would move it out of line in `plain_number` too.
    let digits = |bytes: &[u8], start: u64| {
        let mut n = start;
        for &byte in bytes {
            let digit = byte.wrapping_sub(b'0');
            if digit > 9 {
                return None;
            }
            n = n * 10 + u64::from(digit);
        }
        Some(n)
    };
    let coefficient = digits(fraction, digits(whole, 0)?)?;
    let (below, magnitude) = match exponent {
        [b'-', rest @ ..] => (true, rest),
        [b'+', rest @ ..] => (false, rest),
        _ => (false, exponent),
    };
    if magnitude.is_empty() || magnitude.len() > 5 {
        return None;
    }
    let magnitude = digits(magnitude, 0)? as i32;
    let scale = if below { -magnitude } else { magnitude } - fraction.len() as i32;
    if (-19..=19).contains(&scale) {
        // As `compact_decimal`: nonzero values here are normal and far from
        // either range boundary, and zero is zero.
        value(compact_scaled(negative, coefficient, scale)?).ok()
    } else {
        scaled_value(negative, coefficient, scale)
    }
}

/// An integer below 2^64 is exact in binary80's 64-bit significand: normalize
/// it directly, without rounding.
fn compact_integer(integer: u64) -> Extended {
    if integer == 0 {
        return Extended::ZERO;
    }
    let shift = integer.leading_zeros();
    let exponent = 63 - shift as u128;
    Extended::from_bits((exponent + 16383) << 64 | u128::from(integer << shift))
}

/// Round an exact small ratio directly to a normal binary80 value.
fn compact_ratio(numerator: u64, denominator: std::num::NonZeroU64) -> Extended {
    if numerator == 0 {
        return Extended::ZERO;
    }
    let (significand, exponent) = rounded_ratio(numerator, denominator.get());
    Extended::from_bits(((exponent + 16383) as u128) << 64 | u128::from(significand))
}

/// The nearest-even 64-bit significand s and exponent e with s*2^(e-63)
/// nearest to n/d, for nonzero n and d.
fn rounded_ratio(numerator: u64, denominator: u64) -> (u64, i32) {
    let mut exponent = denominator.leading_zeros() as i32 - numerator.leading_zeros() as i32;
    let n = u128::from(numerator);
    let d = u128::from(denominator);
    if if exponent >= 0 {
        n < d << exponent
    } else {
        n << -exponent < d
    } {
        exponent -= 1;
    }
    // e=floor(log2(n/d)), so the shift is 0..127 and n*2^(63-e) < d*2^64 < 2^128.
    let scaled = n << (63 - exponent);
    let (quotient, remainder) = divide_narrow(scaled, denominator);
    let mut significand = u128::from(quotient);
    let remainder = u128::from(remainder);
    if remainder * 2 > d || (remainder * 2 == d && significand & 1 != 0) {
        significand += 1;
    }
    if significand == 1u128 << 64 {
        significand >>= 1;
        exponent += 1;
    }
    (significand as u64, exponent)
}

/// Quotient and remainder of `numerator / divisor` when `numerator < divisor *
/// 2^64`, so the quotient fits in 64 bits: one hardware 128-by-64 division
/// instead of the general 128-bit routine.
fn divide_narrow(numerator: u128, divisor: u64) -> (u64, u64) {
    assert!(
        numerator >> 64 < u128::from(divisor),
        "narrow division precondition"
    );
    let (quotient, remainder): (u64, u64);
    // SAFETY: `div` faults only for a zero divisor or a quotient wider than 64
    // bits; the high half is below the (therefore nonzero) divisor, so neither
    // can happen. It reads and writes only the named registers and flags.
    unsafe {
        core::arch::asm!(
            "div {divisor}",
            divisor = in(reg) divisor,
            inout("rax") numerator as u64 => quotient,
            inout("rdx") (numerator >> 64) as u64 => remainder,
            options(pure, nomem, nostack),
        );
    }
    (quotient, remainder)
}

#[cfg(test)]
thread_local! {
    static GENERAL_PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static SHORTCUTS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Whether the conversion shortcuts run: always, except where a test turns
/// them all off for the general converter's reference results. Every
/// shortcut gives the general converter's bits, so its results are the same.
#[inline(always)]
fn shortcuts() -> bool {
    #[cfg(test)]
    return SHORTCUTS.with(|on| on.get());
    #[cfg(not(test))]
    true
}

fn parse(input: &[u8], profile: Profile) -> Result<Parsed, Failure> {
    #[cfg(test)]
    GENERAL_PARSES.with(|count| count.set(count.get() + 1));
    let lex = bytes::lex_slice(input, profile).map_err(conversion_failure)?;
    let mut range_error = false;
    let value = match lex.lexeme {
        Lexeme::NoConversion => None,
        Lexeme::Infinity { negative } => {
            Some(convert::infinity(negative).map_err(conversion_failure)?)
        }
        Lexeme::Nan {
            negative,
            complete_payload,
        } => {
            let mut nan = convert::quiet_nan(negative).map_err(conversion_failure)?;
            if complete_payload {
                let start = input[..lex.consumed]
                    .iter()
                    .position(|&b| b == b'(')
                    .expect("complete NaN payload")
                    + 1;
                let payload = nan_payload(&input[start..lex.consumed - 1]);
                nan =
                    Value80::from_raw(Raw80::new(nan.sign_exponent(), nan.significand() | payload))
                        .map_err(|_| unsupported("internal NaN encoding failure"))?;
            }
            Some(nan)
        }
        Lexeme::Finite(token) => {
            let token_bytes = &input[lex.leading_space.end..lex.consumed];
            i32::try_from(token_bytes.len())
                .map_err(|_| unsupported("numeric token exceeds converter index range"))?;
            if shortcuts()
                && let Some(compact) = compact_decimal(&token)
            {
                // Nonzero compact values are normal and far from either range boundary.
                return Ok(Parsed {
                    value: Some(value(compact)?),
                    consumed: lex.consumed,
                    range_error: false,
                });
            }
            if shortcuts()
                && let Some(value) = scaled_decimal(&token).or_else(|| long_decimal(&token))
            {
                return Ok(Parsed {
                    value: Some(value),
                    consumed: lex.consumed,
                    range_error: false,
                });
            }
            if shortcuts()
                && let Some(value) = compact_hex(&token)
            {
                return Ok(Parsed {
                    value: Some(value),
                    consumed: lex.consumed,
                    range_error: false,
                });
            }
            let mut normalized = Vec::new();
            let needs_exponent =
                token.radix() == 16 && !token_bytes.iter().any(|b| matches!(b, b'p' | b'P'));
            let shortened = shortened(&token)?;
            let token_bytes = if let Some(shortened) = &shortened {
                shortened.text.as_slice()
            } else if needs_exponent || (profile.radix() == b',' && token_bytes.contains(&b',')) {
                super::command_memory::reserve(
                    &mut normalized,
                    token_bytes
                        .len()
                        .checked_add(2)
                        .ok_or_else(|| unsupported("numeric token size overflow"))?,
                )?;
                normalized.extend(
                    token_bytes
                        .iter()
                        .map(|b| if *b == b',' { b'.' } else { *b }),
                );
                if needs_exponent {
                    normalized.extend_from_slice(b"p0");
                }
                &normalized
            } else {
                token_bytes
            };
            let text = std::str::from_utf8(token_bytes)
                .map_err(|_| unsupported("internal decimal grammar failure"))?;
            i32::try_from(text.len())
                .map_err(|_| unsupported("numeric token exceeds converter index range"))?;
            let (value, out_of_range) = finite(text, |value, converter| {
                exact(&token, shortened.as_ref(), value, converter)
            })?;
            range_error = out_of_range;
            Some(value)
        }
    };
    Ok(Parsed {
        value,
        consumed: lex.consumed,
        range_error,
    })
}

// The validated payload is either a complete base-0 unsigned integer or a word.
// Overflow saturates, while any nonnumeric suffix makes the payload canonical.
fn nan_payload(mut bytes: &[u8]) -> u64 {
    let radix = if bytes.starts_with(b"0x") || bytes.starts_with(b"0X") {
        bytes = &bytes[2..];
        16
    } else if bytes.first() == Some(&b'0') {
        8
    } else {
        10
    };
    let mut value = 0u64;
    for &byte in bytes {
        let Some(digit) = bytes::digit(byte, radix) else {
            return 0;
        };
        value = value
            .saturating_mul(u64::from(radix))
            .saturating_add(u64::from(digit));
    }
    value
}

/// The number in `record` at `span`, field `field` on line `line`, with GNU
/// datamash's diagnostic when it has none: [`number`] with an inline shortcut
/// for plain numbers, the common case.
pub(super) fn field(
    record: &[u8],
    span: field_policy::FieldRange,
    field: u64,
    line: u64,
    narm: bool,
    profile: Profile,
) -> Result<Option<numerics::Value>, Failure> {
    let part = &record[span.start..span.start + span.length];
    if narm && field_policy::is_na(part) {
        return Ok(None);
    }
    if !part.is_empty()
        && let Some(value) = plain_number(
            part,
            record.get(span.start + span.length).copied(),
            profile.radix(),
        )
    {
        return admit(value).map(Some);
    }
    other_field(record, span, field, line, profile)
}

/// [`field`] for a field that is not a plain number: out of line, so the
/// plain path stays small.
#[inline(never)]
fn other_field(
    record: &[u8],
    span: field_policy::FieldRange,
    field: u64,
    line: u64,
    profile: Profile,
) -> Result<Option<numerics::Value>, Failure> {
    read_number(record, span, profile, |error| {
        error.report(&record[span.start..span.start + span.length], field, line)
    })
}

/// The number in `record` at `span`, with its failure unreported
/// ([`NumberError::report`] gives [`field`]'s message): GNU runs `strtold`
/// from the field's start through the rest of the record, and repeats the
/// parse on the field alone only when it ran on past the field.
pub(super) fn number(
    record: &[u8],
    span: field_policy::FieldRange,
    narm: bool,
    profile: Profile,
) -> Result<Option<numerics::Value>, NumberError> {
    if narm && field_policy::is_na(&record[span.start..span.start + span.length]) {
        return Ok(None);
    }
    read_number(record, span, profile, |error| error)
}

/// [`number`] for a field that is not NA: the steps [`field`] also takes for
/// a field that is not a plain number. `fail` gives each caller its own
/// failure, so that [`field`]'s values return as they are, unconverted.
#[inline(always)]
fn read_number<E>(
    record: &[u8],
    span: field_policy::FieldRange,
    profile: Profile,
    fail: impl Fn(NumberError) -> E,
) -> Result<Option<numerics::Value>, E> {
    let refused = |failure| fail(NumberError::Refused(failure));
    let part = &record[span.start..span.start + span.length];
    if part.is_empty() {
        return Err(fail(NumberError::Invalid));
    }
    if let Some(marker) = exponent_marker(part)
        && let Some(value) = scientific_number(
            part,
            marker,
            record.get(span.start + span.length).copied(),
            profile.radix(),
        )
    {
        return admit(value).map(Some).map_err(refused);
    }
    let mut parsed = parse(&record[span.start..], profile).map_err(refused)?;
    if parsed.range_error || parsed.consumed < span.length {
        return Err(fail(NumberError::Invalid));
    }
    if parsed.consumed > span.length {
        if span.length >= 512 {
            return Err(fail(NumberError::TooLong));
        }
        parsed = parse(part, profile).map_err(refused)?;
        if parsed.range_error || parsed.consumed != span.length {
            return Err(fail(NumberError::Invalid));
        }
    }
    // A parse that consumed the nonempty field has a value.
    let value = parsed.value.ok_or_else(|| fail(NumberError::Invalid))?;
    admit(value).map(Some).map_err(refused)
}

/// The position of the last `e` or `E` among the last seven bytes of `part`,
/// where a sign and at most five exponent digits leave room for it: one word
/// test for a field of eight bytes or more, so other fields that reach the
/// general parser pay little for [`scientific_number`].
#[inline(always)]
fn exponent_marker(part: &[u8]) -> Option<usize> {
    match part.len().checked_sub(8) {
        Some(start) => {
            let word = u64::from_le_bytes(part[start..].try_into().unwrap());
            let y = (word | 0x2020_2020_2020_2020) ^ 0x6565_6565_6565_6565;
            let low = 0x7f7f_7f7f_7f7f_7f7f_u64;
            let zero = !(((y & low) + low) | y | low) & !0x80;
            (zero != 0).then(|| start + 7 - (zero.leading_zeros() / 8) as usize)
        }
        None => part.iter().rposition(|&b| b | 0x20 == b'e'),
    }
}

/// Why a numeric field has no value: [`number`]'s failures before they name
/// their line and field ([`NumberError::report`]).
#[derive(Clone)]
pub(super) enum NumberError {
    Invalid,
    /// GNU's parse ran on past a field of 512 bytes or more.
    TooLong,
    Refused(Failure),
}

impl NumberError {
    /// The failure for field `field` (`part`) on line `line`.
    #[cold]
    #[inline(never)]
    pub(super) fn report(self, part: &[u8], field: u64, line: u64) -> Failure {
        match self {
            Self::Invalid => {
                let mut message =
                    format!("invalid numeric value in line {line} field {field}: '").into_bytes();
                message.extend_from_slice(
                    &part[..part.iter().position(|&b| b == 0).unwrap_or(part.len())],
                );
                message.extend_from_slice(b"'\n");
                failure(message)
            }
            Self::TooLong => failure(
                format!("internal error: input field too long ({})\n", part.len()).into_bytes(),
            ),
            Self::Refused(failure) => failure,
        }
    }

    /// Whether `self` reports as `other` does.
    pub(super) fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Invalid, Self::Invalid) | (Self::TooLong, Self::TooLong) => true,
            (Self::Refused(a), Self::Refused(b)) => a.status == b.status && a.message == b.message,
            _ => false,
        }
    }
}

pub(super) fn add(
    left: numerics::Value,
    right: numerics::Value,
) -> Result<numerics::Value, Failure> {
    if let Some(result) = normal_sum(
        numerics::Numerics::value80(left),
        numerics::Numerics::value80(right),
        false,
    ) {
        return numerics::Numerics::admit(result).map_err(numeric_failure);
    }
    binary(left, right, |a, b| {
        a.add_r(b, Round::NearestTiesToEven).value
    })
}

fn shift_right_jam(value: u128, distance: u32) -> u128 {
    if distance == 0 {
        value
    } else if distance >= 128 {
        u128::from(value != 0)
    } else {
        (value >> distance) | u128::from(value & ((1u128 << distance) - 1) != 0)
    }
}

/// The nearest-even sum, or difference when `subtract`, of two normal values:
/// +0 for exact cancellation and a signed infinity on overflow. `None` for
/// other operands or a subnormal result, which APFloat handles. The result is
/// exact or correctly rounded by construction.
// Inlined into add and subtract: the call itself was a tenth of its cost.
#[inline(always)]
fn normal_sum(left: Value80, right: Value80, subtract: bool) -> Option<Value80> {
    let (a, b) = (left.raw(), right.raw());
    if !(1..0x7fff).contains(&a.exponent()) || !(1..0x7fff).contains(&b.exponent()) {
        return None;
    }
    let (mut large, mut small) = (
        (a.exponent(), a.significand(), a.is_negative()),
        (b.exponent(), b.significand(), b.is_negative() != subtract),
    );
    if (small.0, small.1) > (large.0, large.1) {
        std::mem::swap(&mut large, &mut small);
    }
    let (exponent, significand, negative) = large;
    // 62 low bits keep every alignment below 63 exact; beyond that the jammed
    // bit decides rounding exactly as the discarded bits would.
    let aligned = shift_right_jam(u128::from(small.1) << 62, u32::from(exponent - small.0));
    let sum = if negative == small.2 {
        (u128::from(significand) << 62) + aligned
    } else {
        (u128::from(significand) << 62) - aligned
    };
    if sum == 0 {
        return Some(Value80::signed_zero(false));
    }
    let shift = sum.leading_zeros();
    let mut exponent = i32::from(exponent) + 2 - shift as i32;
    if exponent < 1 {
        return None;
    }
    let normalized = sum << shift;
    let mut significand = normalized >> 64;
    let remainder = normalized as u64;
    if remainder > 1 << 63 || (remainder == 1 << 63 && significand & 1 != 0) {
        significand += 1;
    }
    if significand == 1 << 64 {
        significand >>= 1;
        exponent += 1;
    }
    if exponent >= 0x7fff {
        return Some(Value80::infinity(negative));
    }
    // 1 <= exponent < 0x7fff and the integer bit is set: a canonical normal.
    debug_assert!((1..0x7fff).contains(&exponent) && significand >> 63 == 1);
    Some(Value80::canonical_normal(
        negative,
        exponent as u16,
        significand as u64,
    ))
}

pub(super) fn subtract(
    left: numerics::Value,
    right: numerics::Value,
) -> Result<numerics::Value, Failure> {
    if let Some(result) = normal_sum(
        numerics::Numerics::value80(left),
        numerics::Numerics::value80(right),
        true,
    ) {
        return numerics::Numerics::admit(result).map_err(numeric_failure);
    }
    binary(left, right, |a, b| {
        a.sub_r(b, Round::NearestTiesToEven).value
    })
}

pub(super) fn divide(
    left: numerics::Value,
    right: numerics::Value,
) -> Result<numerics::Value, Failure> {
    if let Some(result) = normal_quotient(
        numerics::Numerics::value80(left),
        numerics::Numerics::value80(right),
    ) {
        return numerics::Numerics::admit(result).map_err(numeric_failure);
    }
    binary(left, right, |a, b| {
        a.div_r(b, Round::NearestTiesToEven).value
    })
}

/// The nearest-even quotient of two normal values when it is normal.
fn normal_quotient(left: Value80, right: Value80) -> Option<Value80> {
    let (a, b) = (left.raw(), right.raw());
    if !(1..0x7fff).contains(&a.exponent()) || !(1..0x7fff).contains(&b.exponent()) {
        return None;
    }
    let (significand, scale) = rounded_ratio(a.significand(), b.significand());
    let exponent = i32::from(a.exponent()) - i32::from(b.exponent()) + 16383 + scale;
    if !(1..0x7fff).contains(&exponent) {
        return None;
    }
    Value80::from_raw(Raw80::new(
        exponent as u16 | ((a.sign_exponent() ^ b.sign_exponent()) & 0x8000),
        significand,
    ))
    .ok()
}

pub(super) fn multiply(
    left: numerics::Value,
    right: numerics::Value,
) -> Result<numerics::Value, Failure> {
    if let Some(result) = normal_product(
        numerics::Numerics::value80(left),
        numerics::Numerics::value80(right),
    ) {
        return numerics::Numerics::admit(result).map_err(numeric_failure);
    }
    binary(left, right, |a, b| {
        a.mul_r(b, Round::NearestTiesToEven).value
    })
}

fn normal_product(left: Value80, right: Value80) -> Option<Value80> {
    let (a, b) = (left.raw(), right.raw());
    if !(1..0x7fff).contains(&a.exponent()) || !(1..0x7fff).contains(&b.exponent()) {
        return None;
    }
    let product = u128::from(a.significand()) * u128::from(b.significand());
    let shift = 63 + u32::from(product >> 127 != 0);
    let mut exponent =
        i32::from(a.exponent()) + i32::from(b.exponent()) - 16383 + (shift - 63) as i32;
    if !(1..0x7fff).contains(&exponent) {
        return None;
    }
    // Keep the exact remainder until the only nearest-even rounding decision.
    let mut significand = product >> shift;
    let remainder = product & ((1u128 << shift) - 1);
    let half = 1u128 << (shift - 1);
    if remainder > half || (remainder == half && significand & 1 != 0) {
        significand += 1;
    }
    if significand == 1u128 << 64 {
        significand >>= 1;
        exponent += 1;
    }
    if exponent >= 0x7fff {
        return None;
    }
    Value80::from_raw(Raw80::new(
        exponent as u16 | ((a.sign_exponent() ^ b.sign_exponent()) & 0x8000),
        significand as u64,
    ))
    .ok()
}

fn binary(
    left: numerics::Value,
    right: numerics::Value,
    operation: impl FnOnce(Extended, Extended) -> Extended,
) -> Result<numerics::Value, Failure> {
    if let Some(nan) = numerics::binary_nan(left, right) {
        return Ok(nan);
    }
    let left = Extended::from_bits(numerics::Numerics::value80(left).raw().to_bits());
    let right = Extended::from_bits(numerics::Numerics::value80(right).raw().to_bits());
    let result = operation(left, right);
    // APFloat's default invalid result is positive quiet NaN, as in the retained policy.
    numerics::Numerics::admit(value(result)?).map_err(numeric_failure)
}

pub(super) fn mean(sum: numerics::Value, count: u64) -> Result<numerics::Value, Failure> {
    if count != 0 {
        let shift = count.leading_zeros();
        let divisor = Value80::from_raw(Raw80::new(16383 + 63 - shift as u16, count << shift));
        if let Some(result) = divisor
            .ok()
            .and_then(|divisor| normal_quotient(numerics::Numerics::value80(sum), divisor))
        {
            return numerics::Numerics::admit(result).map_err(numeric_failure);
        }
    }
    let sum = Extended::from_bits(numerics::Numerics::value80(sum).raw().to_bits());
    let denominator = Extended::from_u128(count as u128).value;
    let result = if sum.is_nan() {
        sum
    } else {
        sum.div_r(denominator, Round::NearestTiesToEven).value
    };
    numerics::Numerics::admit(value(result)?).map_err(numeric_failure)
}

#[cfg(test)]
mod tests {
    mod fixtures {
        include!("decimal_fixtures.rs");
    }
    use super::*;

    /// Deterministic xorshift64 test values.
    struct XorShift(u64);

    impl XorShift {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn sign(&mut self) -> u16 {
            self.next() as u16 & 0x8000
        }
        fn normal(&mut self) -> Raw80 {
            Raw80::new(
                (self.next() % 32766 + 1) as u16 | self.sign(),
                self.next() | 1 << 63,
            )
        }
        /// Signed normal operands biased towards cancellation, the jamming
        /// boundary, powers of two and nearly equal significands.
        fn biased_pair(&mut self) -> (Raw80, Raw80) {
            let gap = match self.next() % 4 {
                0 => self.next() % 2,
                1 => 60 + self.next() % 8,
                2 => self.next() % 200,
                _ => self.next() % 32766,
            } as u16;
            let high = (gap + 1 + (self.next() % u64::from(32766 - gap)) as u16).min(32766);
            let x = if self.next().is_multiple_of(8) {
                1 << 63
            } else {
                self.next() | 1 << 63
            };
            let y = if self.next().is_multiple_of(4) {
                x ^ (self.next() & 0xff) | 1 << 63
            } else {
                self.next() | 1 << 63
            };
            (
                Raw80::new(high | self.sign(), x),
                Raw80::new((high - gap) | self.sign(), y),
            )
        }
    }

    /// Nearest-even binary80 sum of two normal values from exact 256-bit
    /// integers, independent of APFloat and of `normal_sum`. `None` means a
    /// subnormal result, outside this oracle.
    fn exact_sum(a: Raw80, b: Raw80, subtract: bool) -> Option<Raw80> {
        let (mut x, mut y) = (
            a,
            Raw80::new(
                b.sign_exponent() ^ if subtract { 0x8000 } else { 0 },
                b.significand(),
            ),
        );
        if (y.exponent(), y.significand()) > (x.exponent(), x.significand()) {
            std::mem::swap(&mut x, &mut y);
        }
        let gap = u32::from(x.exponent() - y.exponent());
        if gap > 190 {
            // |y| < 2^-126 |x|: well inside half of the smaller neighbouring spacing.
            return Some(x);
        }
        // (high, low) little-endian 256-bit integers in units of y's last place.
        let shifted = |value: u64, by: u32| -> (u128, u128) {
            let value = u128::from(value);
            match by {
                0 => (0, value),
                1..128 => (value >> (128 - by), value << by),
                _ => (value << (by - 128), 0),
            }
        };
        let (xh, xl) = shifted(x.significand(), gap);
        let yl = u128::from(y.significand());
        let (high, low) = if x.is_negative() == y.is_negative() {
            let (low, carry) = xl.overflowing_add(yl);
            (xh + u128::from(carry), low)
        } else {
            let (low, borrow) = xl.overflowing_sub(yl);
            (xh - u128::from(borrow), low)
        };
        if high == 0 && low == 0 {
            return Some(Raw80::new(0, 0));
        }
        let top = if high != 0 {
            255 - high.leading_zeros()
        } else {
            127 - low.leading_zeros()
        };
        let bit = |n: u32| -> bool {
            if n >= 128 {
                high >> (n - 128) & 1 != 0
            } else {
                low >> n & 1 != 0
            }
        };
        let mut significand = 0u128;
        for n in (top.saturating_sub(63)..=top).rev() {
            significand = significand << 1 | u128::from(bit(n));
        }
        let mut exponent = i32::from(y.exponent()) + top as i32 - 63;
        if top > 63 {
            let half = bit(top - 64);
            let rest = (0..top - 64).any(bit);
            if half && (rest || significand & 1 != 0) {
                significand += 1;
            }
        } else {
            significand <<= 63 - top;
        }
        if significand == 1 << 64 {
            significand >>= 1;
            exponent += 1;
        }
        let sign = if x.is_negative() { 0x8000 } else { 0 };
        if exponent < 1 {
            None
        } else if exponent >= 0x7fff {
            Some(Raw80::new(0x7fff | sign, 1 << 63))
        } else {
            Some(Raw80::new(exponent as u16 | sign, significand as u64))
        }
    }

    #[derive(Clone, Copy, Debug)]
    enum SumBranch {
        SameSign,
        FarDifference,
        NearDifference,
        ExactZero,
        Overflow,
        Fallback,
    }

    /// Checks `add` and `subtract` against APFloat, the exact oracle for
    /// normal operands, and the retained engine, counting the `normal_sum`
    /// branches reached and the retained verdicts.
    struct SumCheck {
        reference: numerics::Numerics,
        branches: [usize; 6],
        retained: usize,
        retained_failures: usize,
    }

    impl SumCheck {
        fn new() -> Self {
            Self {
                reference: numerics::Numerics::new(true).unwrap(),
                branches: [0; 6],
                retained: 0,
                retained_failures: 0,
            }
        }

        fn check(&mut self, a: Raw80, b: Raw80) {
            let left = numerics::canonical(a);
            let right = numerics::canonical(b);
            let normal = |raw: Raw80| (1..0x7fff).contains(&raw.exponent());
            for subtract_right in [false, true] {
                let (actual, expected, retained) = if subtract_right {
                    (
                        subtract(left, right),
                        binary(left, right, |a, b| {
                            a.sub_r(b, Round::NearestTiesToEven).value
                        }),
                        self.reference.subtract(left, right),
                    )
                } else {
                    (
                        add(left, right),
                        binary(left, right, |a, b| {
                            a.add_r(b, Round::NearestTiesToEven).value
                        }),
                        self.reference.add(left, right),
                    )
                };
                let operation = if subtract_right { "-" } else { "+" };
                let actual = numerics::Numerics::value80(
                    actual.unwrap_or_else(|_| panic!("{a:?} {operation} {b:?}")),
                )
                .raw();
                let expected = numerics::Numerics::value80(
                    expected.unwrap_or_else(|_| panic!("APFloat {a:?} {operation} {b:?}")),
                )
                .raw();
                assert_eq!(actual, expected, "{a:?} {operation} {b:?}");
                if normal(a) && normal(b) {
                    if let Some(exact) = exact_sum(a, b, subtract_right) {
                        assert_eq!(actual, exact, "exact {a:?} {operation} {b:?}");
                    }
                    self.branches[self.branch(a, b, subtract_right) as usize] += 1;
                }
                // The retained engine fails with Invariant on some halfway
                // cases; the exact oracle above decides those.
                match retained {
                    Ok(retained) => {
                        assert_eq!(
                            actual,
                            numerics::Numerics::value80(retained).raw(),
                            "retained {a:?} {operation} {b:?}"
                        );
                        self.retained += 1;
                    }
                    Err(error) => {
                        assert!(
                            matches!(error, numerics::NumericFailure::Invariant),
                            "retained {a:?} {operation} {b:?}: {error:?}"
                        );
                        self.retained_failures += 1;
                    }
                }
            }
        }

        fn branch(&self, a: Raw80, b: Raw80, subtract: bool) -> SumBranch {
            let value = |raw| Value80::from_raw(raw).unwrap();
            match normal_sum(value(a), value(b), subtract) {
                None => SumBranch::Fallback,
                Some(result) if result.exponent() == 0 => SumBranch::ExactZero,
                Some(result) if result.exponent() == 0x7fff => SumBranch::Overflow,
                Some(_) if a.is_negative() == (b.is_negative() != subtract) => SumBranch::SameSign,
                Some(_) if a.exponent().abs_diff(b.exponent()) >= 2 => SumBranch::FarDifference,
                Some(_) => SumBranch::NearDifference,
            }
        }
    }

    #[test]
    fn normal_products_match_both_arithmetic_implementations() {
        let mut reference = numerics::Numerics::new(true).unwrap();
        let mut checked = [0usize; 3];
        let mut check = |a: Raw80, b: Raw80| {
            let left = numerics::canonical(a);
            let right = numerics::canonical(b);
            let actual = multiply(left, right).unwrap_or_else(|_| panic!("multiply {a:?} {b:?}"));
            let apfloat = binary(left, right, |a, b| {
                a.mul_r(b, Round::NearestTiesToEven).value
            })
            .unwrap_or_else(|_| panic!("APFloat {a:?} {b:?}"));
            let retained = reference.multiply(left, right).unwrap();
            let actual = numerics::Numerics::value80(actual).raw();
            assert_eq!(
                actual,
                numerics::Numerics::value80(apfloat).raw(),
                "{a:?} {b:?}"
            );
            assert_eq!(
                actual,
                numerics::Numerics::value80(retained).raw(),
                "{a:?} {b:?}"
            );
            let branch = if normal_product(
                numerics::Numerics::value80(left),
                numerics::Numerics::value80(right),
            )
            .is_some()
            {
                usize::from((u128::from(a.significand()) * u128::from(b.significand())) >> 127 != 0)
            } else {
                2
            };
            checked[branch] += 1;
        };
        let sigs = [
            1 << 63,
            (1 << 63) + 1,
            (1 << 63) + 3,
            0xc000_0000_0000_0000,
            u64::MAX,
        ];
        for ea in [0, 1, 2, 16382, 16383, 16384, 32765, 32766] {
            for eb in [0, 1, 2, 16382, 16383, 16384, 32765, 32766] {
                for a in sigs {
                    for b in sigs {
                        for sa in [0, 0x8000] {
                            for sb in [0, 0x8000] {
                                check(
                                    Raw80::new(ea | sa, if ea == 0 { a >> 1 } else { a }),
                                    Raw80::new(eb | sb, if eb == 0 { b >> 1 } else { b }),
                                );
                            }
                        }
                    }
                }
            }
        }
        // Rounding carry crosses a binade, including the overflow boundary.
        for ea in [1, 16383, 32766] {
            check(
                Raw80::new(ea, 0x8000_0b3a_73ce_5b59),
                Raw80::new(16383, 0xffff_e98b_1a5b_9653),
            );
        }
        let mut random = XorShift(0x510e_527f_ade6_82d1);
        for _ in 0..50_000 {
            let a = random.normal();
            let b = random.normal();
            check(a, b);
        }
        assert!(checked.into_iter().all(|count| count > 1000));
    }

    #[test]
    fn signed_sums_and_differences_match_exact_integers() {
        let mut sums = SumCheck::new();
        // Exponent arithmetic below the normal range must not wrap (to infinity).
        sums.check(Raw80::new(2, 1 << 63), Raw80::new(0x8001, u64::MAX));
        let sigs = [
            1u64 << 63,
            (1u64 << 63) + 1,
            (1u64 << 63) + 2,
            0xc000_0000_0000_0000,
            0xaaaa_aaaa_aaaa_aaab,
            0xffff_ffff_ffff_fffe,
            u64::MAX,
        ];
        for gap in (0..=70).chain(120..=132).chain([189, 190, 191, 250, 32765]) {
            for high in [gap + 1, gap + 2, (gap + 16383).min(32766), 32766] {
                if high > 32766 {
                    continue;
                }
                for x in sigs {
                    for y in sigs {
                        for (sa, sb) in [(0, 0), (0, 0x8000), (0x8000, 0), (0x8000, 0x8000)] {
                            let a = Raw80::new(high as u16 | sa, x);
                            let b = Raw80::new((high - gap) as u16 | sb, y);
                            sums.check(a, b);
                            sums.check(b, a);
                        }
                    }
                }
            }
        }
        // Cancellation by every normalization shift, from exact zero upward.
        for exponent in [1u16, 2, 64, 65, 16383, 32766] {
            for difference in (0..64)
                .map(|shift| 1u64 << shift)
                .chain([0, 3, u64::MAX >> 1])
            {
                for x in [
                    u64::MAX,
                    1 << 63 | 1 << 62,
                    (1 << 63) + difference.min(1 << 62),
                ] {
                    let y = x.saturating_sub(difference) | 1 << 63;
                    sums.check(Raw80::new(exponent, x), Raw80::new(0x8000 | exponent, y));
                    sums.check(
                        Raw80::new(exponent + 1, 1 << 63),
                        Raw80::new(0x8000 | exponent, x),
                    );
                }
            }
        }
        let mut random = XorShift(0xbb67_ae85_84ca_a73b);
        for _ in 0..200_000 {
            let (a, b) = random.biased_pair();
            sums.check(a, b);
        }
        let [same, far, near, zero, overflow, fallback] = sums.branches;
        assert!(
            same > 1000 && far > 1000 && near > 1000,
            "{:?}",
            sums.branches
        );
        assert!(
            zero > 10 && overflow > 10 && fallback > 10,
            "{:?}",
            sums.branches
        );
        assert!(sums.retained > 500_000, "{}", sums.retained);
        assert!(sums.retained_failures < 1000, "{}", sums.retained_failures);
    }

    /// Checks from the result alone that a normal quotient is the nearest
    /// value: |a*2^k - q*b| < b/2 for the scale k its exponents imply.
    fn assert_nearest_quotient(a: Raw80, b: Raw80, result: Raw80) {
        let k = i32::from(a.exponent()) - i32::from(b.exponent()) - i32::from(result.exponent())
            + 16383
            + 63;
        assert!(k == 63 || k == 64, "{a:?} / {b:?} = {result:?}");
        let scaled = u128::from(a.significand()) << k;
        let product = u128::from(result.significand()) * u128::from(b.significand());
        assert!(
            2 * scaled.abs_diff(product) < u128::from(b.significand()),
            "{a:?} / {b:?}"
        );
        assert_eq!(result.is_negative(), a.is_negative() != b.is_negative());
    }

    /// A normal significand a whose quotient by odd b lies 1/(2b) units below
    /// (or above) a rounding midpoint, as close as a quotient of 64-bit
    /// integers can: with T = 2q + 1 in [2^64, 2^65) and T*b = a*2^w + s,
    /// s = +/-1. w = 65 gives a < b, w = 64 gives a >= b.
    fn midpoint_numerator(b: u64, w: u32, below: bool) -> Option<u64> {
        let b = u128::from(b);
        // Newton's iteration for 1/b modulo 2^128 (b*b = 1 mod 8 for odd b).
        let mut inverse = b;
        for _ in 0..6 {
            inverse = inverse.wrapping_mul(2u128.wrapping_sub(b.wrapping_mul(inverse)));
        }
        let residue = if below {
            inverse
        } else {
            inverse.wrapping_neg()
        } & ((1 << w) - 1);
        let odd = if w == 65 {
            residue
        } else {
            residue + (1 << 64)
        };
        if !(1u128 << 64..1 << 65).contains(&odd) {
            return None;
        }
        let product = odd.checked_mul(b)?;
        let numerator = if below {
            product - 1
        } else {
            product.checked_add(1)?
        } >> w;
        let numerator = u64::try_from(numerator).ok().filter(|a| a >> 63 != 0)?;
        ((w == 65) == (u128::from(numerator) < b)).then_some(numerator)
    }

    #[test]
    fn normal_quotients_and_means_are_nearest_and_match_both_implementations() {
        let mut reference = numerics::Numerics::new(true).unwrap();
        let mut checked = [0usize; 2];
        let mut check = |a: Raw80, b: Raw80| {
            let left = numerics::canonical(a);
            let right = numerics::canonical(b);
            let actual = numerics::Numerics::value80(
                divide(left, right).unwrap_or_else(|_| panic!("{a:?} / {b:?}")),
            )
            .raw();
            let apfloat = binary(left, right, |a, b| {
                a.div_r(b, Round::NearestTiesToEven).value
            })
            .unwrap_or_else(|_| panic!("APFloat {a:?} / {b:?}"));
            assert_eq!(
                actual,
                numerics::Numerics::value80(apfloat).raw(),
                "{a:?} / {b:?}"
            );
            let retained = reference.divide(left, right).unwrap();
            assert_eq!(
                actual,
                numerics::Numerics::value80(retained).raw(),
                "retained {a:?} / {b:?}"
            );
            let fast =
                normal_quotient(Value80::from_raw(a).unwrap(), Value80::from_raw(b).unwrap());
            if let Some(fast) = fast {
                assert_nearest_quotient(a, b, fast.raw());
            }
            checked[usize::from(fast.is_none())] += 1;
        };
        let sigs = [
            1 << 63,
            (1 << 63) + 1,
            0xaaaa_aaaa_aaaa_aaab,
            0xc000_0000_0000_0000,
            u64::MAX - 1,
            u64::MAX,
        ];
        for ea in [0, 1, 2, 16382, 16383, 16384, 32765, 32766] {
            for eb in [0, 1, 2, 16382, 16383, 16384, 32765, 32766] {
                for a in sigs {
                    for b in sigs {
                        for (sa, sb) in [(0, 0), (0x8000, 0), (0, 0x8000), (0x8000, 0x8000)] {
                            check(
                                Raw80::new(ea | sa, if ea == 0 { a >> 1 } else { a }),
                                Raw80::new(eb | sb, if eb == 0 { b >> 1 } else { b }),
                            );
                        }
                    }
                }
            }
        }
        // The hardest quotients, next to rounding midpoints, with result
        // exponents across and at both ends of the normal range.
        let mut random = XorShift(0x3c6e_f372_fe94_f82b);
        let mut midpoints = 0;
        for _ in 0..50_000 {
            let b = random.next() | 1 << 63 | 1;
            let w = 64 + (random.next() % 2) as u32;
            let Some(a) = midpoint_numerator(b, w, random.next().is_multiple_of(2)) else {
                continue;
            };
            let remainder = (u128::from(a) << (w - 1)) % u128::from(b);
            assert!(remainder.abs_diff(u128::from(b / 2)) <= 1, "{a:x} / {b:x}");
            midpoints += 1;
            let (ea, eb) = match random.next() % 3 {
                0 => (16383, 16383),
                1 => (
                    1 + (random.next() % 3) as u16,
                    16383 + (random.next() % 3) as u16,
                ),
                _ => (
                    (random.next() % 32766 + 1) as u16,
                    (random.next() % 32766 + 1) as u16,
                ),
            };
            check(
                Raw80::new(ea | random.sign(), a),
                Raw80::new(eb | random.sign(), b),
            );
        }
        for _ in 0..50_000 {
            let a = random.normal();
            let b = random.normal();
            check(a, b);
        }
        assert!(midpoints > 5_000, "{midpoints}");
        assert!(checked.into_iter().all(|count| count > 1000), "{checked:?}");

        let mut fast_means = 0;
        for count in [1, 2, 3, 7, 10, (1 << 53) + 1, 1 << 63, u64::MAX] {
            for _ in 0..200 {
                let sum = numerics::canonical(random.normal());
                let divisor = numerics::Numerics::promote_u64(count);
                let actual = numerics::Numerics::value80(
                    mean(sum, count).unwrap_or_else(|_| panic!("mean {sum:?} {count}")),
                )
                .raw();
                let apfloat = Extended::from_bits(numerics::Numerics::value80(sum).raw().to_bits())
                    .div_r(
                        Extended::from_u128(u128::from(count)).value,
                        Round::NearestTiesToEven,
                    )
                    .value;
                assert_eq!(actual.to_bits(), apfloat.to_bits(), "{sum:?} / {count}");
                let retained = reference.divide(sum, divisor).unwrap();
                assert_eq!(actual, numerics::Numerics::value80(retained).raw());
                fast_means += usize::from(
                    normal_quotient(
                        numerics::Numerics::value80(sum),
                        numerics::Numerics::value80(divisor),
                    )
                    .is_some(),
                );
            }
        }
        assert!(fast_means > 1000, "{fast_means}");
    }

    /// Long qualification run: `FASTMASH_LONG_PAIRS=100000000 cargo test
    /// --release -- --ignored` (default 10^7 pairs).
    #[test]
    #[ignore]
    fn biased_sums_and_quotients_match_apfloat_and_exact_results() {
        let pairs = std::env::var("FASTMASH_LONG_PAIRS").map_or(10_000_000, |pairs| {
            pairs.parse().expect("FASTMASH_LONG_PAIRS is a count")
        });
        let apfloat = |a: Raw80, b: Raw80, operation: fn(Extended, Extended) -> Extended| {
            operation(
                Extended::from_bits(a.to_bits()),
                Extended::from_bits(b.to_bits()),
            )
            .to_bits()
        };
        let mut random = XorShift(0xa54f_f53a_5f1d_36f1);
        for _ in 0..pairs {
            let (a, b) = random.biased_pair();
            let (va, vb) = (Value80::from_raw(a).unwrap(), Value80::from_raw(b).unwrap());
            for subtract in [false, true] {
                let operation = if subtract { "-" } else { "+" };
                let expected = if subtract {
                    apfloat(a, b, |a, b| a.sub_r(b, Round::NearestTiesToEven).value)
                } else {
                    apfloat(a, b, |a, b| a.add_r(b, Round::NearestTiesToEven).value)
                };
                if let Some(actual) = normal_sum(va, vb, subtract) {
                    assert_eq!(actual.raw().to_bits(), expected, "{a:?} {operation} {b:?}");
                    if let Some(exact) = exact_sum(a, b, subtract) {
                        assert_eq!(actual.raw(), exact, "exact {a:?} {operation} {b:?}");
                    }
                }
            }
            for (a, b) in [(a, b), (b, a)] {
                if let Some(actual) =
                    normal_quotient(Value80::from_raw(a).unwrap(), Value80::from_raw(b).unwrap())
                {
                    assert_eq!(
                        actual.raw().to_bits(),
                        apfloat(a, b, |a, b| a.div_r(b, Round::NearestTiesToEven).value),
                        "{a:?} / {b:?}"
                    );
                    assert_nearest_quotient(a, b, actual.raw());
                }
            }
        }
    }

    #[test]
    fn normal_sum_keeps_fallback_policy_and_sequential_rounding() {
        let values = [
            Raw80::new(0, 0),
            Raw80::new(0x8000, 0),
            Raw80::new(0, 1),
            Raw80::new(0x8000, (1 << 63) - 1),
            Raw80::new(1, 1 << 63),
            Raw80::new(0x3fff, 1 << 63),
            Raw80::new(0xbfff, 1 << 63),
            Raw80::new(0x7fff, 1 << 63),
            Raw80::new(0xffff, 1 << 63),
            Raw80::new(0x7fff, (1 << 63) | (1 << 62) | 17),
            Raw80::new(0xffff, (1 << 63) | (1 << 62) | 19),
        ];
        let mut sums = SumCheck::new();
        for a in values {
            for b in values {
                sums.check(a, b);
            }
        }
        let large =
            numerics::Numerics::admit(Value80::from_raw(Raw80::new(16447, 1 << 63)).unwrap())
                .unwrap();
        let minus = numerics::Numerics::admit(
            Value80::from_raw(Raw80::new(16447 | 0x8000, 1 << 63)).unwrap(),
        )
        .unwrap();
        let sum = add(add(large, super::super::integer(1)).ok().unwrap(), minus)
            .ok()
            .unwrap();
        assert_eq!(numerics::Numerics::value80(sum).raw(), Raw80::new(0, 0));
    }

    #[test]
    fn nan_payload_bits_and_lexical_consumption() {
        for (payload, bits) in [
            ("", 0),
            ("word", 0),
            ("a_b", 0),
            ("09", 0),
            ("0x", 0),
            ("42", 42),
            ("052", 42),
            ("0X2a", 42),
            ("0", 0),
            ("18446744073709551615", u64::MAX),
            ("18446744073709551616", u64::MAX),
            ("18446744073709551616x", 0),
            ("9223372036854775808", 1 << 63),
            ("4611686018427387904", 1 << 62),
        ] {
            for (sign, exponent) in [("", 0x7fff), ("+", 0x7fff), ("-", 0xffff)] {
                let text = format!(" {sign}NaN({payload})tail");
                let parsed = parse(text.as_bytes(), Profile::C).unwrap_or_else(|_| panic!("parse"));
                assert_eq!(
                    parsed.value.expect("NaN").raw(),
                    Raw80::new(exponent, 0xc000_0000_0000_0000 | bits)
                );
                assert!(!parsed.range_error);
                assert_eq!(parsed.consumed, text.len() - 4);
            }
        }
    }

    #[test]
    fn finite_boundaries_match_independent_conversion_and_profiles() {
        for text in [
            "10e-4951",
            "1.0e-4950",
            "10e-4933",
            "3.36210314311209350626e-4932",
            "0x1p-16445",
            "0x1.1p-16445",
            "0x1",
            "-0x1.8",
            "1e4932",
            "1e4933",
            "1e-5000",
            "1e-20",
            "-0e-99999",
        ] {
            let lex = bytes::lex_slice(text.as_bytes(), Profile::C).unwrap();
            let Lexeme::Finite(token) = lex.lexeme else {
                panic!("finite")
            };
            let expected = convert::finite(&token).unwrap().value;
            for profile in [Profile::C, Profile::DE_NUMERIC] {
                let input = if profile == Profile::DE_NUMERIC {
                    text.replace('.', ",")
                } else {
                    text.into()
                };
                let result =
                    parse(input.as_bytes(), profile).unwrap_or_else(|e| panic!("{:?}", e.message));
                assert_eq!(result.value.unwrap().raw(), expected.raw(), "{text}");
                assert_eq!(result.consumed, input.len());
            }
        }
    }

    #[test]
    fn tiny_decimal_certificates_preserve_bits_and_range() {
        for (numerator, power, exponent, significand, range) in [
            (1, 16445, 0, 1, false),
            (1u128 << 66, 16448, 1, 1u64 << 63, false),
            ((1u128 << 66) - 1, 16448, 1, 1u64 << 63, false),
            ((1u128 << 66) - 2, 16448, 1, 1u64 << 63, false),
            ((1u128 << 66) - 3, 16448, 1, 1u64 << 63, true),
            ((1u128 << 66) - 4, 16448, 1, 1u64 << 63, true),
            ((1u128 << 66) - 5, 16448, 0, (1u64 << 63) - 1, true),
            ((1u128 << 66) - 8, 16448, 0, (1u64 << 63) - 1, false),
        ] {
            let text = fixtures::dyadic(numerator, power);
            for (sign, sign_bit) in [("", 0), ("-", 0x8000)] {
                let text = format!("{sign}{text}");
                let parsed = parse(text.as_bytes(), Profile::C)
                    .unwrap_or_else(|e| panic!("certificate: {:?}", e.message));
                assert_eq!(parsed.consumed, text.len());
                assert_eq!(parsed.range_error, range);
                assert_eq!(
                    parsed.value.unwrap().raw(),
                    Raw80::new(exponent | sign_bit, significand)
                );
            }
        }
    }

    /// `exact` padded past the kept digits with zeros, then nudged by a
    /// last 1 just above it and, as its last nonzero digit less one and
    /// nines, just below it.
    fn padded(exact: &str) -> [String; 3] {
        let exact = format!("{exact}{}", "0".repeat(SIGNIFICANT));
        let at = exact.rfind(|c: char| c != '0' && c != '.').unwrap();
        let nines: String = exact[at + 1..]
            .chars()
            .map(|c| if c == '.' { '.' } else { '9' })
            .collect();
        let below = format!(
            "{}{}{nines}9",
            &exact[..at],
            char::from(exact.as_bytes()[at] - 1)
        );
        [format!("{exact}1"), below, exact]
    }

    fn parsed(text: &str, profile: Profile) -> (Raw80, bool) {
        let parsed = parse(text.as_bytes(), profile).unwrap_or_else(|e| panic!("{:?}", e.message));
        assert_eq!(parsed.consumed, text.len());
        (parsed.value.unwrap().raw(), parsed.range_error)
    }

    /// Decimals of more significant digits than `SIGNIFICANT` convert as
    /// their exact values round: subnormal values, exact (no range error) or
    /// nudged (inexact, so a range error), midpoints near one and the
    /// smallest normal; values across the range as the whole digits give
    /// them; exponents beyond the lexer's bound; the comma radix.
    #[test]
    fn long_decimals_convert_as_their_exact_values_round() {
        for significand in [1u64, 5, 7, 41, (1 << 63) - 1] {
            let [above, below, exact] = padded(&fixtures::dyadic(significand.into(), 16445));
            let value = Raw80::new(0, significand);
            assert_eq!(parsed(&exact, Profile::C), (value, false), "{significand}");
            assert_eq!(parsed(&above, Profile::C), (value, true), "{significand}");
            assert_eq!(parsed(&below, Profile::C), (value, true), "{significand}");
        }
        let one = 1u64 << 63;
        for (numerator, power, [above, below, exact]) in [
            ((1u128 << 64) + 1, 64, [one | 1, one, one]),
            ((1u128 << 64) + 3, 64, [one | 2, one | 1, one | 2]),
        ] {
            let [up, down, tie] = padded(&fixtures::dyadic(numerator, power));
            assert_eq!(parsed(&up, Profile::C), (Raw80::new(0x3fff, above), false));
            assert_eq!(
                parsed(&down, Profile::C),
                (Raw80::new(0x3fff, below), false)
            );
            assert_eq!(parsed(&tie, Profile::C), (Raw80::new(0x3fff, exact), false));
        }
        // The smallest normal, exact and nudged: tininess after rounding at
        // 64-bit precision, so no range error.
        for text in padded(&fixtures::dyadic(1 << 66, 16448)) {
            assert_eq!(parsed(&text, Profile::C), (Raw80::new(1, one), false));
        }
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for exponent in ["e-4950", "e-4931", "e0", "e4931", "e4933"] {
            let digits: String = (0..SIGNIFICANT + 200)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    char::from(b'0' + (state >> 60) as u8 % 10)
                })
                .collect();
            for text in [
                format!("0.{digits}{exponent}"),
                format!("-{digits}{exponent}"),
            ] {
                let (whole, _) = finite(&text, |_, converter| Ok(converter))
                    .unwrap_or_else(|e| panic!("{:?}", e.message));
                assert_eq!(parsed(&text, Profile::C).0, whole.raw(), "{exponent}");
            }
        }
        // Beyond the lexer's exponent bound, in range and out of it.
        let long = format!("{}e-1000010", "7".repeat(1_000_100));
        let short = format!("{}e-{}", "7".repeat(SIGNIFICANT + 100), SIGNIFICANT + 10);
        assert_eq!(parsed(&long, Profile::C), parsed(&short, Profile::C));
        let digits = "7".repeat(SIGNIFICANT + 100);
        assert!(parsed(&format!("{digits}e-1000001"), Profile::C).1);
        assert!(parsed(&format!("-0.{digits}e+99999999999999999999"), Profile::C).1);
        assert_eq!(
            parsed(&format!("0,{digits}"), Profile::DE_NUMERIC),
            parsed(&format!("0.{digits}"), Profile::C)
        );
    }

    /// Whether a zero, subnormal or smallest normal result is a range error
    /// follows from the decimal's exact value, as with `strtold`: decimals of
    /// a few digits never equal a subnormal, so they are range errors, and
    /// a subnormal's whole expansion is not. The converter misjudges some of
    /// both (such as 2^-16445's 40-digit prefix, which it calls exact).
    #[test]
    fn tiny_decimals_are_range_errors_unless_exact() {
        let prefix = "3.645199531882474602528405933619419816399e-4951";
        assert!(parsed(prefix, Profile::C).1);
        assert!(parsed("1e-4945", Profile::C).1);
        assert!(parsed("-1e-99999", Profile::C).1);
        assert!(!parsed("0e-99999", Profile::C).1);
        assert!(!parsed("0.000", Profile::C).1);
        for significand in [1u64, 5, 7] {
            let exact = fixtures::dyadic(significand.into(), 16445);
            assert_eq!(
                parsed(&exact, Profile::C),
                (Raw80::new(0, significand), false)
            );
            assert!(!parsed(&format!("-{exact}"), Profile::C).1);
            let de = exact.replace('.', ",");
            assert!(!parsed(&de, Profile::DE_NUMERIC).1);
        }
        // 2^-16445 as 5^16445 times 10^-16445, and past the lexer's exponent
        // bound after a million leading zeros; and just above it there.
        let digits = fixtures::dyadic(1, 16445)
            .trim_start_matches("0.")
            .trim_start_matches('0')
            .to_string();
        let smallest = (Raw80::new(0, 1), false);
        assert_eq!(parsed(&format!("{digits}e-16445"), Profile::C), smallest);
        let zeros = "0".repeat(1_000_000);
        let exponent = 1_000_000 + digits.len() - 16445;
        let far = format!("0.{zeros}{digits}e+{exponent}");
        assert_eq!(parsed(&far, Profile::C), smallest);
        let above = format!("0.{zeros}{digits}1e+{exponent}");
        assert_eq!(parsed(&above, Profile::C), (Raw80::new(0, 1), true));
    }

    #[test]
    fn compact_ratio_matches_independent_float_division() {
        let check = |n: u64, d: u64| {
            let actual = compact_ratio(n, std::num::NonZeroU64::new(d).unwrap());
            let expected = Extended::from_u128(n.into())
                .value
                .div_r(
                    Extended::from_u128(d.into()).value,
                    Round::NearestTiesToEven,
                )
                .value;
            assert_eq!(actual.to_bits(), expected.to_bits(), "{n}/{d}");
        };
        // Power-of-two neighborhoods exercise both exponent estimates and
        // representable values immediately around normalization boundaries.
        for n_bit in 0..64 {
            for d_bit in 0..64 {
                for n in [(1u64 << n_bit) - 1, 1u64 << n_bit, (1u64 << n_bit) + 1] {
                    for d in [(1u64 << d_bit) - 1, 1u64 << d_bit, (1u64 << d_bit) + 1] {
                        if d != 0 {
                            check(n, d);
                        }
                    }
                }
            }
        }
        for n in [0, 1, 2, 3, u64::MAX - 2, u64::MAX - 1, u64::MAX] {
            for d in [1, 2, 3, u64::MAX - 2, u64::MAX - 1, u64::MAX] {
                check(n, d);
            }
        }
        let mut state = 0x834a_3015_1d78_05acu64;
        for _ in 0..16_384 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let n = state;
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            check(n, state.max(1));
        }
    }

    #[test]
    fn plain_number_fields_skip_the_lexer_with_identical_values() {
        let mut state = 0x243f_6a88_85a3_08d3u64;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for profile in [Profile::C, Profile::EN_NUMERIC, Profile::DE_NUMERIC] {
            let point = profile.radix();
            let mut texts: Vec<Vec<u8>> = [
                &b"0"[..],
                b"-0",
                b"7",
                b"-7",
                b"0007",
                b"-0000",
                b"9999999999999999999",
                b"-9999999999999999999",
                b"18446744073709551615",
                b"1000000000000000000",
            ]
            .iter()
            .map(|t| t.to_vec())
            .collect();
            for fixed in [
                &b"0#5"[..],
                b"-0#0",
                b"1#25",
                b"0#1",
                b"-2#675",
                b"123456789#0123456789",
                b"9#999999999999999999",
            ] {
                texts.push(
                    fixed
                        .iter()
                        .map(|&b| if b == b'#' { point } else { b })
                        .collect(),
                );
            }
            for _ in 0..20_000 {
                let bits = next();
                let digits = 1 + (bits % 20) as usize;
                let at = if bits & (1 << 50) != 0 {
                    (bits >> 20) as usize % digits
                } else {
                    digits
                };
                let mut text = if bits & (1 << 40) != 0 {
                    b"-".to_vec()
                } else {
                    Vec::new()
                };
                for index in 0..digits {
                    if index == at && index != 0 {
                        text.push(point);
                    }
                    text.push(b'0' + (next() % 10) as u8);
                }
                texts.push(text);
            }
            for text in &texts {
                let body = text.strip_prefix(b"-").unwrap_or(text);
                let plain = body.iter().filter(|b| b.is_ascii_digit()).count() <= 19
                    && body.first().is_some_and(u8::is_ascii_digit)
                    && body.last().is_some_and(u8::is_ascii_digit);
                let expected = parse(text, profile)
                    .unwrap_or_else(|_| panic!("parse"))
                    .value
                    .unwrap()
                    .raw();
                let span = field_policy::FieldRange {
                    start: 0,
                    length: text.len(),
                };
                for rest in [&b""[..], b"\t9", b";x", b" 1", b"\n"] {
                    let mut record = text.clone();
                    record.extend_from_slice(rest);
                    let before = GENERAL_PARSES.with(|count| count.get());
                    let value = field(&record, span, 1, 1, false, profile)
                        .unwrap_or_else(|_| panic!("field {text:?}"))
                        .unwrap();
                    let fast = GENERAL_PARSES.with(|count| count.get()) == before;
                    let actual = numerics::Numerics::value80(value).raw();
                    assert_eq!(actual, expected, "{profile:?} {text:?}");
                    assert_eq!(fast, plain, "{profile:?} {text:?} {rest:?}");
                }
                // Digits, the point and letters keep the general parser (`p` and a
                // non-zero `x` only exercise the conservative letter guard).
                let own_point: &[u8] = if point == b'.' { b".5" } else { b",5" };
                for rest in [&b"51"[..], b"e9999", b"E5", own_point, b"x1", b"p1"] {
                    let mut record = text.clone();
                    record.extend_from_slice(rest);
                    let before = GENERAL_PARSES.with(|count| count.get());
                    let _ = field(&record, span, 1, 1, false, profile);
                    let general_used = GENERAL_PARSES.with(|count| count.get()) > before;
                    assert!(general_used, "{text:?} {rest:?}");
                }
            }
        }
        // Everything else keeps the general parser.
        for text in [
            &b"+7"[..],
            b" 7",
            b"7.",
            b".5",
            b"0x10",
            b"--7",
            b"-",
            b"1,5",
            b"1.2.3",
        ] {
            let span = field_policy::FieldRange {
                start: 0,
                length: text.len(),
            };
            let before = GENERAL_PARSES.with(|count| count.get());
            let _ = field(text, span, 1, 1, false, Profile::C);
            assert!(
                GENERAL_PARSES.with(|count| count.get()) > before,
                "{text:?}"
            );
        }
    }

    /// Turns every conversion shortcut on, or off for the reference results.
    fn set_shortcuts(on: bool) {
        SHORTCUTS.with(|enabled| enabled.set(on));
    }

    /// A field's value bits, or its failure's status and message.
    type Outcome = Result<Option<u128>, (i32, Vec<u8>)>;

    /// `field`'s and `number`'s results for the field at `span`, both reported
    /// as `field` reports them, with every shortcut on or off.
    fn reader_outcomes(
        record: &[u8],
        span: field_policy::FieldRange,
        narm: bool,
        profile: Profile,
        shortcuts: bool,
    ) -> [Outcome; 2] {
        let bits = |value: Option<numerics::Value>| {
            value.map(|value| numerics::Numerics::value80(value).raw().to_bits())
        };
        let part = &record[span.start..span.start + span.length];
        set_shortcuts(shortcuts);
        let outcomes = [
            field(record, span, 3, 7, narm, profile)
                .map(bits)
                .map_err(|failure| (failure.status, failure.message)),
            number(record, span, narm, profile)
                .map(bits)
                .map_err(|error| {
                    let failure = error.report(part, 3, 7);
                    (failure.status, failure.message)
                }),
        ];
        set_shortcuts(true);
        outcomes
    }

    #[test]
    fn field_and_number_read_alike_with_or_without_shortcuts() {
        let long_run_on = format!("{}#5", "1".repeat(600));
        let long_decimal = format!("0#{}1", "0".repeat(40));
        let fields: Vec<&[u8]> = vec![
            b"",
            b"0",
            b"-0",
            b"7",
            b"-12",
            b"9999999999999999999",
            b"12345678901234567890",
            b"1#25",
            b"1#",
            b"#5",
            b"+7",
            b" 7",
            b"1#2#3",
            b"1a2",
            b"1e5",
            b"-1#5e-16",
            b"9#999999999999999999e4931",
            b"1e4933",
            b"1e-4951",
            b"1e-99999",
            b"1e99999",
            b"1e",
            b"1e+",
            b"1#234567890123456789012345",
            b"0x1#8p1",
            b"0x",
            b"0x1p99999",
            b"inf",
            b"-Infinity",
            b"nan",
            b"nan(12)",
            b"nan(",
            b"NA",
            b"N/A",
            b"x",
            b"-",
            long_run_on.as_bytes(),
            long_decimal.as_bytes(),
        ];
        let mut compared = 0;
        for profile in [Profile::C, Profile::EN_NUMERIC, Profile::DE_NUMERIC] {
            let point = profile.radix();
            let other_point = if point == b'.' { b',' } else { b'.' };
            let nexts = [
                None,
                Some(b'\t'),
                Some(b' '),
                Some(b'5'),
                Some(b'e'),
                Some(b'E'),
                Some(b'p'),
                Some(b'x'),
                Some(b'('),
                Some(b')'),
                Some(b'_'),
                Some(b'-'),
                Some(b'+'),
                Some(point),
                Some(other_point),
                Some(b'a'),
                Some(0),
                Some(0x80),
            ];
            for text in &fields {
                let text: Vec<u8> = text
                    .iter()
                    .map(|&b| if b == b'#' { point } else { b })
                    .collect();
                for next in nexts {
                    for tail in [&b""[..], b"9", b"e-5", b"5 x", b"ab)"] {
                        let mut record = b"k\t".to_vec();
                        record.extend_from_slice(&text);
                        if let Some(next) = next {
                            record.push(next);
                            record.extend_from_slice(tail);
                        } else if !tail.is_empty() {
                            continue;
                        }
                        let span = field_policy::FieldRange {
                            start: 2,
                            length: text.len(),
                        };
                        for narm in [false, true] {
                            let reference = reader_outcomes(&record, span, narm, profile, false);
                            assert_eq!(reference[0], reference[1], "{profile:?} {record:?}");
                            assert_eq!(
                                reader_outcomes(&record, span, narm, profile, true),
                                reference,
                                "{profile:?} {record:?}"
                            );
                            compared += 1;
                        }
                    }
                }
            }
        }
        // 3 profiles x 38 fields x 86 continuations x NA on and off.
        assert_eq!(compared, 19_608);
    }

    /// `field`'s whole result, with every shortcut on or off.
    fn field_outcome(
        record: &[u8],
        span: field_policy::FieldRange,
        narm: bool,
        profile: Profile,
        shortcuts: bool,
    ) -> Outcome {
        let [field, _] = reader_outcomes(record, span, narm, profile, shortcuts);
        field
    }

    #[test]
    fn plain_number_shortcut_matches_general_results_for_every_following_byte() {
        let fields: [&[u8]; 23] = [
            b"0",
            b"-0",
            b"7",
            b"0007",
            b"-12",
            b"9999999999999999999",
            b"1#25",
            b"-0#5",
            b"12#0",
            b"1#",
            b"#5",
            b"1e3",
            b"NA",
            b"nan",
            // Malformed shapes the one-pass reader must refuse like the lexer.
            b"1#2#3",
            b"1#2.3",
            b"-",
            b"-#5",
            b"+1",
            b"1a2",
            b"12345678901234567890",
            b"1234567890#1234567890",
            // The smallest plain fraction: 18 fraction digits.
            b"0#000000000000000001",
        ];
        let mut compared = 0;
        for profile in [Profile::C, Profile::EN_NUMERIC, Profile::DE_NUMERIC] {
            let point = profile.radix();
            for text in fields {
                let text: Vec<u8> = text
                    .iter()
                    .map(|&b| if b == b'#' { point } else { b })
                    .collect();
                for next in (0..=255u8).map(Some).chain([None]) {
                    for tail in [&b""[..], b"9", b"e9999", b"5 x"] {
                        // The field sits in the middle of the record.
                        let mut record = b"k\t".to_vec();
                        record.extend_from_slice(&text);
                        if let Some(next) = next {
                            record.push(next);
                            record.extend_from_slice(tail);
                        } else if !tail.is_empty() {
                            continue;
                        }
                        let span = field_policy::FieldRange {
                            start: 2,
                            length: text.len(),
                        };
                        for narm in [false, true] {
                            assert_eq!(
                                field_outcome(&record, span, narm, profile, true),
                                field_outcome(&record, span, narm, profile, false),
                                "{profile:?} {record:?}"
                            );
                            compared += 1;
                        }
                    }
                }
            }
        }
        // 3 profiles x 23 fields x 1,025 continuations x NA on and off.
        assert_eq!(compared, 141_450);
    }

    /// The field shortcuts' outcomes against the general parser's for each
    /// field of `fields` (`#` standing for the radix point) followed by any
    /// byte and then by some tails; returns how many were compared.
    fn compare_following_bytes(fields: &[&[u8]]) -> usize {
        let mut compared = 0;
        for profile in [Profile::C, Profile::EN_NUMERIC, Profile::DE_NUMERIC] {
            let point = profile.radix();
            for text in fields {
                let text: Vec<u8> = text
                    .iter()
                    .map(|&b| if b == b'#' { point } else { b })
                    .collect();
                for next in (0..=255u8).map(Some).chain([None]) {
                    for tail in [&b""[..], b"9", b"e9999", b"5 x"] {
                        let mut record = b"k\t".to_vec();
                        record.extend_from_slice(&text);
                        if let Some(next) = next {
                            record.push(next);
                            record.extend_from_slice(tail);
                        } else if !tail.is_empty() {
                            continue;
                        }
                        let span = field_policy::FieldRange {
                            start: 2,
                            length: text.len(),
                        };
                        assert_eq!(
                            field_outcome(&record, span, false, profile, true),
                            field_outcome(&record, span, false, profile, false),
                            "{profile:?} {record:?}"
                        );
                        compared += 1;
                    }
                }
            }
        }
        compared
    }

    #[test]
    fn scientific_number_shortcut_matches_general_results_for_every_following_byte() {
        let fields: &[&[u8]] = &[
            b"1e5",
            b"1E5",
            b"-1#5e-16",
            b"3#334833e-16",
            b"5#74e-159",
            b"1#234568e+25",
            b"0e0",
            b"-0#0e-400",
            b"9#999999999999999999e4931",
            b"1#18973149535723176502e4932",
            b"1e4933",
            b"3#362103143112093506e-4932",
            b"1e-4951",
            b"1e-99999",
            b"1e123456",
            b"1e",
            b"1e+",
            b"1e-",
            b"e5",
            b"#5e3",
            b"1#e3",
            b"1#5e3#5",
            b"1e3e3",
            b"--1e3",
            b"+1e3",
            b"1 e3",
            b"12345678901234567890e3",
            b"0#000000000000000001e-300",
        ];
        // 3 profiles x 28 fields x 1,025 continuations.
        assert_eq!(compare_following_bytes(fields), 86_100);
    }

    #[test]
    fn exponent_markers_are_the_last_e_in_the_last_seven_bytes() {
        let mut random = XorShift(0x1f83_d9ab_fb41_bd6b);
        let alphabet = b"eE5-+.x\x00\x80\xe5\xc5Ff";
        for _ in 0..200_000 {
            let length = (random.next() % 14) as usize;
            let part: Vec<u8> = (0..length)
                .map(|_| alphabet[(random.next() % alphabet.len() as u64) as usize])
                .collect();
            let tail = if length >= 8 { length - 7 } else { 0 };
            let expected = part[tail..]
                .iter()
                .rposition(|&b| b | 0x20 == b'e')
                .map(|at| tail + at);
            assert_eq!(exponent_marker(&part), expected, "{part:?}");
        }
    }

    #[test]
    fn scientific_fields_match_the_general_parser() {
        let mut random = XorShift(0x5d58_8b65_6c07_8965);
        let mut taken = 0;
        for round in 0..100_000 {
            let digits = 1 + random.next() % 19;
            let coefficient = random_digits(&mut random, digits);
            let exponent = match round % 4 {
                0 => -4951 + (random.next() % 40) as i64,
                1 => 4932 - (random.next() % 40) as i64,
                _ => (random.next() % 700) as i64 - 350,
            };
            let sign = ["", "-"][(random.next() % 2) as usize];
            let marker = ["e", "E", "e+", "e0"][(random.next() % 4) as usize];
            let (whole, fraction) = coefficient.split_at(1);
            for profile in [Profile::C, Profile::DE_NUMERIC] {
                let point = char::from(profile.radix());
                let text = if fraction.is_empty() {
                    format!("{sign}{whole}{marker}{exponent}")
                } else {
                    format!("{sign}{whole}{point}{fraction}{marker}{exponent}")
                };
                let record = format!("k\t{text}\tz");
                let span = field_policy::FieldRange {
                    start: 2,
                    length: text.len(),
                };
                let fast = field_outcome(record.as_bytes(), span, false, profile, true);
                assert_eq!(
                    fast,
                    field_outcome(record.as_bytes(), span, false, profile, false),
                    "{profile:?} {text}"
                );
                taken += usize::from(
                    exponent_marker(text.as_bytes())
                        .and_then(|marker| {
                            scientific_number(text.as_bytes(), marker, Some(b'\t'), profile.radix())
                        })
                        .is_some(),
                );
            }
        }
        // Texts such as `1e0-4951` are malformed, and a few values lie at the
        // range ends: those go to the general parser.
        assert!(taken > 130_000, "shortcut taken {taken} times");
    }

    /// The source of `decimal_powers.rs`, from exact integer arithmetic: for each
    /// block j, 5^(28 j) truncated to 128 significant bits and its power of two.
    fn five_powers_source() -> String {
        use num_bigint::BigUint;
        const STEP: i32 = 28;
        // A normal c * 10^scale with 1 <= c < 10^19 has scale in -4951..=4932.
        let (first, last) = ((-4951i32).div_euclid(STEP), 4932i32.div_euclid(STEP));
        let mut source = format!(
            "//! @generated by `decimal::tests::five_powers_are_current`; do not edit.\n\
             //! Regenerate with FASTMASH_REGENERATE=1 set for that test.\n\n\
             /// The blocks are powers of 5^STEP.\n\
             pub(super) const STEP: i32 = {STEP};\n\
             /// The block of the first entry.\n\
             pub(super) const FIRST: i32 = {first};\n\
             /// For block j from FIRST, (m, e) with m in [2^127, 2^128) and\n\
             /// 5^(STEP * j) in [m, m + 1) * 2^e: m is truncated, never rounded up.\n\
             pub(super) const FIVE: &[(u128, i32)] = &[\n"
        );
        for block in first..=last {
            let power = BigUint::from(5u32).pow(STEP.unsigned_abs() * block.unsigned_abs());
            let bits = i64::try_from(power.bits()).unwrap();
            let (mantissa, exponent) = if block >= 0 {
                let shift = bits - 128;
                let mantissa = if shift >= 0 {
                    power >> shift.unsigned_abs()
                } else {
                    power << shift.unsigned_abs()
                };
                (mantissa, shift)
            } else {
                // 2^(127 + bits) / 5^k lies in (2^127, 2^128): 5^k is not a
                // power of two.
                (
                    (BigUint::from(1u32) << (127 + bits).unsigned_abs()) / power,
                    -(127 + bits),
                )
            };
            let digits = mantissa.to_u64_digits();
            assert_eq!(digits.len(), 2);
            assert!(digits[1] >> 63 == 1);
            source.push_str(&format!(
                "    (0x{:016x}_{:016x}, {exponent}),\n",
                digits[1], digits[0]
            ));
        }
        source.push_str("];\n");
        source
    }

    #[test]
    fn five_powers_are_current() {
        let expected = five_powers_source();
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/decimal_powers.rs");
        if std::env::var_os("FASTMASH_REGENERATE").is_some() {
            std::fs::write(path, &expected).unwrap();
        }
        assert!(
            std::fs::read_to_string(path).unwrap() == expected,
            "decimal_powers.rs is stale: set FASTMASH_REGENERATE=1 for this test"
        );
        // Independently of the generator: every entry is truncated, so the
        // exact power lies in [m, m + 1) * 2^e, which `scaled_decimal` needs.
        use num_bigint::BigUint;
        for (index, &(m, e)) in decimal_powers::FIVE.iter().enumerate() {
            let block = decimal_powers::FIRST + index as i32;
            let k = decimal_powers::STEP.unsigned_abs() * block.unsigned_abs();
            let five = BigUint::from(5u32).pow(k);
            let (low, high) = (BigUint::from(m), BigUint::from(m) + 1u32);
            assert!(m >> 127 == 1, "block {block}");
            let within = if block < 0 {
                // 5^-k = 2^e * [m, m + 1) with e < 0: m 5^k <= 2^-e < (m + 1) 5^k.
                let two = BigUint::from(1u32) << e.unsigned_abs();
                &low * &five <= two && two < &high * &five
            } else if e >= 0 {
                (&low << e.unsigned_abs()) <= five && five < (&high << e.unsigned_abs())
            } else {
                let scaled = &five << e.unsigned_abs();
                low <= scaled && scaled < high
            };
            assert!(within, "block {block}");
        }
    }

    #[test]
    fn wide_products_and_rounding_are_exact() {
        use num_bigint::BigUint;
        let mut random = XorShift(0x2545_f491_4f6c_dd1d);
        let wide = |a: u128| BigUint::from(a);
        for _ in 0..20_000 {
            let a = u128::from(random.next()) << 64 | u128::from(random.next());
            let b = u128::from(random.next()) << 64 | u128::from(random.next());
            let (a, b) = (a >> (random.next() % 128), b >> (random.next() % 128));
            let (high, low) = widening_mul(a, b);
            assert_eq!((wide(high) << 128u32) | wide(low), wide(a) * wide(b));
            if high == 0 && low == 0 {
                continue;
            }
            // Independent rounding: the significand from the exact quotient.
            let value = (wide(high) << 128u32) | wide(low);
            let top = u32::try_from(value.bits()).unwrap() - 1;
            let (significand, top) = if top < 64 {
                (u64::try_from(value << (63 - top)).unwrap(), top)
            } else {
                let shift = top - 63;
                let floor = &value >> shift;
                let rest = &value - (&floor << shift);
                let half = BigUint::from(1u32) << (shift - 1);
                let odd = floor.bit(0);
                let floor = if rest > half || (rest == half && odd) {
                    floor + 1u32
                } else {
                    floor
                };
                match u64::try_from(&floor) {
                    Ok(significand) => (significand, top),
                    Err(_) => (1 << 63, top + 1),
                }
            };
            assert_eq!(round_nearest(high, low), (significand, top), "{a} * {b}");
        }
        // Ties to even, and a carry out of the significand.
        assert_eq!(round_nearest(0, (1 << 65) | 1), (1 << 63, 65));
        assert_eq!(round_nearest(0, (1 << 65) | 2), (1 << 63, 65));
        assert_eq!(round_nearest(0, (1 << 65) | 3), ((1 << 63) + 1, 65));
        assert_eq!(round_nearest(0, (1 << 65) | 6), ((1 << 63) + 2, 65));
        assert_eq!(round_nearest(0, u128::MAX >> 63), (1 << 63, 65));
        assert_eq!(round_nearest(1, 0), (1 << 63, 128));
    }

    /// `text`'s parse with every shortcut on or off, and whether it took the scaled path.
    fn scaled_parse(
        text: &str,
        profile: Profile,
        shortcuts: bool,
    ) -> (Option<(u128, usize, bool)>, bool) {
        set_shortcuts(shortcuts);
        let parsed = parse(text.as_bytes(), profile);
        set_shortcuts(true);
        let lex = bytes::lex_slice(text.as_bytes(), profile).unwrap();
        let taken = match lex.lexeme {
            Lexeme::Finite(token) => {
                compact_decimal(&token).is_none() && scaled_decimal(&token).is_some()
            }
            _ => false,
        };
        (
            parsed.ok().map(|p| {
                (
                    p.value.map_or(0, |v| v.raw().to_bits()),
                    p.consumed,
                    p.range_error,
                )
            }),
            taken,
        )
    }

    /// The scaled path agrees with the general converter, and where it gives
    /// a value, with the exact rational converter.
    fn check_scaled(text: &str, profile: Profile) -> bool {
        let (fast, taken) = scaled_parse(text, profile, true);
        let (general, _) = scaled_parse(text, profile, false);
        assert_eq!(fast, general, "{profile:?} {text}");
        if taken {
            let lex = bytes::lex_slice(text.as_bytes(), profile).unwrap();
            let Lexeme::Finite(token) = lex.lexeme else {
                unreachable!()
            };
            let exact = convert::finite(&token).unwrap();
            assert!(!exact.overflow && !exact.ordinary_p64_tiny, "{text}");
            assert_eq!(
                scaled_decimal(&token).unwrap().raw().to_bits(),
                exact.value.raw().to_bits(),
                "exact rational: {text}"
            );
        }
        taken
    }

    #[test]
    fn scaled_decimals_match_both_converters() {
        let mut random = XorShift(0x9e6c_63d0_676a_9a99);
        let (mut typical, mut typical_taken) = (0, 0);
        for round in 0..60_000 {
            let digits = 1 + random.next() % 19;
            let coefficient: String = (0..digits)
                .map(|i| {
                    let digit = random.next() % 10;
                    char::from_digit(if i == 0 { digit.max(1) } else { digit } as u32, 10).unwrap()
                })
                .collect();
            let exponent = match round % 4 {
                // Near the smallest normal and the largest finite value.
                0 => -4951 + (random.next() % 40) as i64,
                1 => 4932 - (random.next() % 40) as i64,
                // Scientific notation as programs write it.
                _ => (random.next() % 700) as i64 - 350,
            };
            let sign = if random.next().is_multiple_of(2) {
                ""
            } else {
                "-"
            };
            let (whole, fraction) = coefficient.split_at(1);
            let text = format!("{sign}{whole}.{fraction}e{exponent}");
            let taken = check_scaled(&text, Profile::C);
            check_scaled(&text.replace('.', ","), Profile::DE_NUMERIC);
            check_scaled(&format!("{coefficient}E{exponent}"), Profile::C);
            if round % 4 >= 2 && !(-19..=19).contains(&(exponent - digits as i64 + 1)) {
                typical += 1;
                typical_taken += usize::from(taken);
            }
        }
        // The general converter is left only for the rare ambiguous product.
        assert!(
            typical_taken * 1000 > typical * 999,
            "{typical_taken} of {typical}"
        );
        // Exact ties (an odd 65-bit c * 5^20 times 2^20) and exact values go
        // to nearest even either way; the boundaries and zero stay general.
        for text in [
            "193429e20",
            "193431e20",
            "386855e20",
            "1024e20",
            "4096e-20",
            "9999999999999999999e-4951",
            "1e-4931",
            // 19-digit neighbours of the smallest normal, 2^-16381, the
            // largest finite value, and powers of two.
            "3.362103143112093505e-4932",
            "3.362103143112093506e-4932",
            "3.362103143112093507e-4932",
            "6.724206286224187012e-4932",
            "6.724206286224187013e-4932",
            "1.189731495357231764e4932",
            "1.189731495357231765e4932",
            "1.189731495357231766e4932",
            "7.888609052210118054e-31",
            "1.267650600228229401e30",
            "3.3621031431120935063e-4932",
            "1.18973149535723176502e4932",
            "1.1897314953572317649e4932",
            "0e-400",
            "-0e400",
            "4e-115",
            "1.234568e-25",
            "6.02214076e23",
        ] {
            check_scaled(text, Profile::C);
        }
    }

    /// Like [`check_scaled`], for the path of more than 19 digits: whether
    /// [`long_decimal`] gave the value.
    fn check_long(text: &str, profile: Profile) -> bool {
        check_scaled(text, profile);
        let lex = bytes::lex_slice(text.as_bytes(), profile).unwrap();
        let Lexeme::Finite(token) = lex.lexeme else {
            return false;
        };
        let Some(fast) = long_decimal(&token) else {
            return false;
        };
        if fast.raw().to_bits() & !(1 << 79) == 0 {
            return true;
        }
        let exact = convert::finite(&token).unwrap();
        assert!(!exact.overflow && !exact.ordinary_p64_tiny, "{text}");
        assert_eq!(
            fast.raw().to_bits(),
            exact.value.raw().to_bits(),
            "exact rational: {text}"
        );
        true
    }

    /// Random decimal digits, the first not zero.
    fn random_digits(random: &mut XorShift, count: u64) -> String {
        (0..count)
            .map(|i| {
                let digit = random.next() % 10;
                char::from_digit(if i == 0 { digit.max(1) } else { digit } as u32, 10).unwrap()
            })
            .collect()
    }

    #[test]
    fn long_decimals_match_both_converters() {
        let mut random = XorShift(0x2545_f491_4f6c_dd1d);
        let (mut typical, mut typical_taken) = (0, 0);
        for round in 0..40_000 {
            let digits = 20 + random.next() % 41;
            let coefficient = random_digits(&mut random, digits);
            let exponent = match round % 5 {
                0 => -4951 + (random.next() % 40) as i64,
                1 => 4932 - (random.next() % 40) as i64,
                _ => (random.next() % 700) as i64 - 350,
            };
            let sign = if random.next().is_multiple_of(2) {
                ""
            } else {
                "-"
            };
            let (whole, fraction) = coefficient.split_at(1);
            let text = format!("{sign}{whole}.{fraction}e{exponent}");
            let taken = check_long(&text, Profile::C);
            check_long(&text.replace('.', ","), Profile::DE_NUMERIC);
            check_long(&format!("{coefficient}E{exponent}"), Profile::C);
            // Fixed-point text with leading zeros, as `%.25f` writes it.
            let zeros = "0".repeat((random.next() % 12) as usize);
            check_long(&format!("{sign}0.{zeros}{coefficient}"), Profile::C);
            // Trailing zeros: the value is the kept digits exactly.
            check_long(
                &format!("{sign}{}{}e-30", &coefficient[..19], "0".repeat(25)),
                Profile::C,
            );
            if round % 5 >= 2 {
                typical += 1;
                typical_taken += usize::from(taken);
            }
        }
        assert!(
            typical_taken * 1000 > typical * 999,
            "{typical_taken} of {typical}"
        );
        // Midpoints between neighbouring binary80 values, (2s + 1) * 2^q, as
        // exact decimals and nudged by one unit in a far digit either way.
        for round in 0..4_000u64 {
            let significand = (1u64 << 63) | random.next();
            let q = (random.next() % 600) as i64 - 400;
            let odd = num_bigint::BigUint::from(significand) * 2u32 + 1u32;
            let (digits, scale) = if q < 0 {
                (
                    odd * num_bigint::BigUint::from(5u32).pow(q.unsigned_abs() as u32),
                    q,
                )
            } else {
                (odd << q as usize, 0)
            };
            let exact = digits.to_string();
            let pad = "0".repeat((round % 7) as usize);
            let below = format!("{}{}", (&digits - 1u32), "9".repeat(pad.len() + 1));
            for text in [
                format!("{exact}e{scale}"),
                format!("{exact}{pad}1e{}", scale - pad.len() as i64 - 1),
                format!("{below}e{}", scale - pad.len() as i64 - 1),
                format!(
                    "0.{pad}{exact}e{}",
                    scale + exact.len() as i64 + pad.len() as i64
                ),
            ] {
                check_long(&text, Profile::C);
            }
        }
        for text in [
            "0.1234567890123456789",
            "0.12345678901234567891",
            "0.0000000000000000000000000001",
            "00000000000000000000000000000000000000001.5",
            "1.00000000000000000000000000000000000000000000000000001",
            "0.99999999999999999999999999999999999999999999999",
            "99999999999999999999999999999999999999999999999999",
            "3.3621031431120935062626778173217526e-4932",
            "3.3621031431120935062626778173217527e-4932",
            "6.72420628622418701252535563464350521e-4932",
            "1.18973149535723176502126385303097021e4932",
            "1.18973149535723176508575932662800702e4932",
            "1.18973149535723176508575932662800701e4932",
            "0.00000000000000000000000000000000000000000",
            "-0.0000000000000000000000",
            "1.2345678901234567890123456789e400000000000",
            "1.2345678901234567890123456789e-400000000000",
        ] {
            check_long(text, Profile::C);
        }
    }

    #[test]
    fn compact_hex_matches_the_general_converter() {
        let convert = |text: &str, fast: bool| {
            set_shortcuts(fast);
            let parsed = parse(text.as_bytes(), Profile::C);
            set_shortcuts(true);
            parsed.ok().map(|p| {
                (
                    p.value.map(|v| v.raw().to_bits()),
                    p.consumed,
                    p.range_error,
                )
            })
        };
        let mut texts: Vec<String> = [
            "0x1p0",
            "0x1.8p1",
            "-0x1.0000000000000002p16383",
            "0x1.fffffffffffffffffp16383",
            "0x1.ffffffffffffffffp16383",
            "0x1p-16382",
            "0x1p-16381",
            "0x1.fffffffffffffffffp-16383",
            "0x0.0000000000000000000000000000001p0",
            "0xffffffffffffffffffffffffffffffff",
            "0x8000000000000000800000000000000",
            "0x8000000000000001800000000000000",
            "0x0p0",
            "-0x0.0p5",
            "0x1P+4",
            "0xA.bp-3",
        ]
        .map(String::from)
        .to_vec();
        // Random coefficients and exponents, biased toward rounding ties and
        // both range boundaries.
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let digits = 1 + next() % 32;
            let mut coefficient: String = (0..digits)
                .map(|_| char::from_digit((next() % 16) as u32, 16).unwrap())
                .collect();
            if next() % 4 == 0 {
                // A tie after the 64th significant bit.
                coefficient = format!("1{}8", "0".repeat((next() % 16) as usize));
            }
            let point = (next() % (digits + 1)) as usize;
            let point = point.min(coefficient.len());
            let exponent = match next() % 3 {
                0 => (next() % 400) as i64 - 200,
                1 => 16_383 - (next() % 140) as i64,
                _ => -16_382 - (next() % 140) as i64 + 130,
            };
            let sign = if next() % 2 == 0 { "" } else { "-" };
            texts.push(format!(
                "{sign}0x{}.{}p{exponent}",
                &coefficient[..point],
                &coefficient[point..]
            ));
        }
        let mut fast_hits = 0;
        for text in &texts {
            let fast = convert(text, true);
            let general = convert(text, false);
            assert_eq!(fast, general, "{text}");
            if matches!(bytes::lex_slice(text.as_bytes(), Profile::C).map(|lex| lex.lexeme),
                Ok(Lexeme::Finite(token)) if compact_hex(&token).is_some())
            {
                fast_hits += 1;
            }
        }
        assert!(fast_hits > 10_000, "fast path taken {fast_hits} times");
        // The largest finite exponent is inside the fast path's range.
        for text in ["0x1.0000000000000002p16383", "0x1p-16381"] {
            let Ok(Lexeme::Finite(token)) =
                bytes::lex_slice(text.as_bytes(), Profile::C).map(|lex| lex.lexeme)
            else {
                panic!("{text}")
            };
            assert!(compact_hex(&token).is_some(), "{text}");
        }
    }

    #[test]
    fn narrow_division_matches_wide_division_at_its_bounds() {
        for (numerator, divisor) in [
            (0u128, 1u64),
            (u128::from(u64::MAX), 1),
            (
                (u128::from(u64::MAX - 1) << 64) | u128::from(u64::MAX),
                u64::MAX,
            ),
            (u128::from(u64::MAX - 1) << 64, u64::MAX),
            ((1u128 << 127) - 1, 1 << 63),
            (10u128.pow(19) << 63, 10u64.pow(19)),
        ] {
            let (quotient, remainder) = divide_narrow(numerator, divisor);
            let divisor = u128::from(divisor);
            assert_eq!(
                (u128::from(quotient), u128::from(remainder)),
                (numerator / divisor, numerator % divisor),
                "{numerator} / {divisor}"
            );
        }
    }

    #[test]
    fn compact_integer_matches_general_conversion() {
        let check = |n: u64| {
            let expected = Extended::from_u128_r(n.into(), Round::NearestTiesToEven).value;
            assert_eq!(compact_integer(n).to_bits(), expected.to_bits(), "{n}");
        };
        for bit in 0..64 {
            for n in [(1u64 << bit) - 1, 1u64 << bit, (1u64 << bit) + 1] {
                check(n);
            }
        }
        for n in [0, 1, 7, 10, 999_999, u64::MAX - 1, u64::MAX] {
            check(n);
        }
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..16_384 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            check(state >> (state % 64));
        }
    }

    fn check_compact(text: &str, profile: Profile) {
        let lex = bytes::lex_slice(text.as_bytes(), profile).unwrap();
        let Lexeme::Finite(token) = lex.lexeme else {
            panic!("finite token expected")
        };
        let compact = compact_decimal(&token).expect("compact domain");
        let independent = convert::finite(&token).unwrap().value;
        assert_eq!(
            compact.to_bits(),
            independent.raw().to_bits(),
            "exact rational: {text}"
        );
        let normalized = text.trim_start().replace(',', ".");
        let general = Extended::from_str_r(&normalized, Round::NearestTiesToEven)
            .unwrap()
            .value;
        assert_eq!(
            compact.to_bits(),
            general.to_bits(),
            "general converter: {text}"
        );
    }

    #[test]
    fn compact_rationals_match_independent_rounding() {
        for coefficient in [
            0,
            1,
            5,
            9,
            10,
            999,
            (1u64 << 53) - 1,
            (1u64 << 53) + 1,
            9_999_999_999_999_999_999,
        ] {
            for scale in -19..=19 {
                for sign in ["", "-"] {
                    check_compact(&format!("{sign}{coefficient}e{scale}"), Profile::C);
                }
            }
        }
        let mut state = 0x719a_fbed_584c_a761u64;
        for _ in 0..4096 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
            let coefficient = state % 10_000_000_000_000_000_000;
            let scale = (state.rotate_left(19) % 39) as i32 - 19;
            check_compact(&format!("{coefficient}e{scale}"), Profile::C);
        }
        // At exponent 66 the representable integer spacing is eight; these
        // decimal products are exactly halfway and exercise both tie directions.
        for text in [
            "7378697629483820650e1",
            "7378697629483820654e1",
            "-7378697629483820650e1",
        ] {
            check_compact(text, Profile::C);
        }
        for text in ["  +0,125", "-0,00", "1,25e-2", "0000000000000000001"] {
            check_compact(text, Profile::DE_NUMERIC);
        }
    }

    #[test]
    fn compact_bounds_preserve_fallback_and_field_seams() {
        for text in [
            "1e20",
            "1e-20",
            "00000000000000000001",
            "10000000000000000000",
            "1e99999999999999",
            "0x1p1",
        ] {
            let lex = bytes::lex_slice(text.as_bytes(), Profile::C).unwrap();
            let Lexeme::Finite(token) = lex.lexeme else {
                panic!("finite token expected")
            };
            assert!(compact_decimal(&token).is_none(), "{text}");
        }
        for (input, consumed, expected) in [
            (b"1e+".as_slice(), 1, "1"),
            (b"1.25tail".as_slice(), 4, "1.25"),
        ] {
            let result =
                parse(input, Profile::C).unwrap_or_else(|_| panic!("expected parsed prefix"));
            assert_eq!(result.consumed, consumed);
            assert_eq!(
                result.value.unwrap().raw().to_bits(),
                Extended::from_str_r(expected, Round::NearestTiesToEven)
                    .unwrap()
                    .value
                    .to_bits()
            );
        }
        let span = field_policy::FieldRange {
            start: 0,
            length: 1,
        };
        let retried = field(b"1.25", span, 1, 1, false, Profile::C)
            .unwrap_or_else(|_| panic!("expected field retry"))
            .unwrap();
        assert_eq!(
            numerics::Numerics::value80(retried).raw().to_bits(),
            Extended::from_u128(1).value.to_bits()
        );
        assert!(field(b"1e9999", span, 1, 1, false, Profile::C).is_err());
    }

    #[test]
    fn basic_primitives_match_retained_binary80_bits() {
        let mut reference = numerics::Numerics::new(true).unwrap();
        let mut values = Vec::new();
        for (exponent, significand) in [
            (0, 0),
            (0, 1),
            (0, (1 << 63) - 1),
            (1, 1 << 63),
            (0x3fbe, 1 << 63),
            (0x3fbf, 1 << 63),
            (0x3fff, 1 << 63),
            (0x3fff, (1 << 63) + 1),
            (0x4000, 0xc000_0000_0000_0000),
            (0x7ffe, u64::MAX),
            (0x7fff, 1 << 63),
            (0x7fff, 0xc000_0000_0000_0001),
            (0x7fff, 0xc000_0000_0000_0042),
        ] {
            for sign in [0, 0x8000] {
                values.push(numerics::canonical(Raw80::new(
                    exponent | sign,
                    significand,
                )));
            }
        }
        let mut reference_failures = 0;
        for &left in &values {
            for &right in &values {
                for (name, actual, expected) in [
                    ("add", add(left, right), reference.add(left, right)),
                    ("divide", divide(left, right), reference.divide(left, right)),
                    (
                        "subtract",
                        subtract(left, right),
                        reference.subtract(left, right),
                    ),
                    (
                        "multiply",
                        multiply(left, right),
                        reference.multiply(left, right),
                    ),
                ] {
                    let actual =
                        actual.unwrap_or_else(|error| panic!("{name}: {:?}", error.message));
                    let a = numerics::Numerics::value80(left).raw();
                    let b = numerics::Numerics::value80(right).raw();
                    let known_halfway_failure = name == "add"
                        && [
                            (Raw80::new(0x3fbe, 1 << 63), Raw80::new(0xbfff, 1 << 63)),
                            (Raw80::new(0xbfbe, 1 << 63), Raw80::new(0x3fff, 1 << 63)),
                            (Raw80::new(0x3fff, 1 << 63), Raw80::new(0xbfbe, 1 << 63)),
                            (Raw80::new(0xbfff, 1 << 63), Raw80::new(0x3fbe, 1 << 63)),
                        ]
                        .contains(&(a, b));
                    if known_halfway_failure {
                        assert!(matches!(expected, Err(numerics::NumericFailure::Invariant)));
                        // +/-1 minus the opposite signed half-ulp rounds to +/-1 (even).
                        let one = if a == Raw80::new(0x3fff, 1 << 63)
                            || a == Raw80::new(0xbfff, 1 << 63)
                        {
                            a
                        } else {
                            b
                        };
                        assert_eq!(numerics::Numerics::value80(actual).raw(), one);
                        reference_failures += 1;
                        continue;
                    }
                    let expected = expected.unwrap_or_else(|error| {
                        panic!("retained {name} {left:?} {right:?}: {error:?}")
                    });
                    if matches!(
                        numerics::Numerics::value80(left).classify(),
                        fastmash_numeric_contract::ValueClass::Nan { .. }
                    ) && matches!(
                        numerics::Numerics::value80(right).classify(),
                        fastmash_numeric_contract::ValueClass::Nan { .. }
                    ) {
                        // The CLI's NaN selection differs from the retained core
                        // here; independent policy cases cover both routes.
                        continue;
                    }
                    assert!(
                        numerics::Numerics::value80(actual)
                            .same_bits(numerics::Numerics::value80(expected)),
                        "{name} {left:?} {right:?}"
                    );
                }
            }
        }
        assert_eq!(reference_failures, 4);
    }
}
