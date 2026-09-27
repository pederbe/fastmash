//! Five fixed layouts over canonical bits and integer display rounding.

use fastmash_numeric_contract::{Raw80, Value80, ValueClass};
use num_traits::Zero;
use std::cmp::Ordering;

use crate::convert::value_ratio;
use crate::integer::{self, Ratio};
use crate::profile::{Error, OUTPUT_CAP, Profile};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatId {
    Default14,
    HexLower,
    HexUpper,
    Signed21,
    Fixed0,
}

impl FormatId {
    pub fn from_name(name: &str) -> Result<Self, Error> {
        match name {
            "default" => Ok(Self::Default14),
            "a" => Ok(Self::HexLower),
            "upper-a" => Ok(Self::HexUpper),
            "signed21" => Ok(Self::Signed21),
            "fixed0" => Ok(Self::Fixed0),
            _ => Err(Error::UnsupportedFormat),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Formatted {
    pub input: Raw80,
    pub after: Raw80,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct FormatFailure {
    pub error: Error,
    pub input: Raw80,
    pub after: Raw80,
    /// Diagnostic construction prefix; never a successful complete result.
    pub partial_bytes: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct RawFormatFailure {
    pub error: Error,
    pub raw: Raw80,
    pub formatter: Option<FormatFailure>,
}

struct Output {
    bytes: Vec<u8>,
    limit: usize,
}

impl Output {
    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let length = self
            .bytes
            .len()
            .checked_add(bytes.len())
            .ok_or(Error::OutputCapacity)?;
        if length > self.limit {
            return Err(Error::OutputCapacity);
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(|_| Error::Allocation)?;
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn repeat(&mut self, byte: u8, count: usize) -> Result<(), Error> {
        let length = self
            .bytes
            .len()
            .checked_add(count)
            .ok_or(Error::OutputCapacity)?;
        if length > self.limit {
            return Err(Error::OutputCapacity);
        }
        self.bytes
            .try_reserve(count)
            .map_err(|_| Error::Allocation)?;
        self.bytes.resize(length, byte);
        Ok(())
    }

    fn exponent(&mut self, exponent: i32, minimum_digits: usize) -> Result<(), Error> {
        self.append(if exponent < 0 { b"-" } else { b"+" })?;
        let mut magnitude = exponent.unsigned_abs();
        let mut digits = [0_u8; 10];
        let mut start = digits.len();
        loop {
            start -= 1;
            digits[start] = b'0' + (magnitude % 10) as u8;
            magnitude /= 10;
            if magnitude == 0 {
                break;
            }
        }
        self.repeat(b'0', minimum_digits.saturating_sub(digits.len() - start))?;
        self.append(&digits[start..])
    }
}

fn compare_ten(ratio: &Ratio, exponent: i32) -> Result<Ordering, Error> {
    let power = integer::power(10, exponent.unsigned_abs())?;
    if exponent >= 0 {
        Ok(ratio
            .numerator
            .cmp(&integer::mul(&ratio.denominator, &power)?))
    } else {
        Ok(integer::mul(&ratio.numerator, &power)?.cmp(&ratio.denominator))
    }
}

/// floor(binary * log10(2)) for |binary| within binary80 range: 30103/100000
/// approximates log10(2) with error below 4.4e-9, contributing below 0.000073
/// decimal digits across finite binary80 exponents.
fn decimal_estimate(binary: i64) -> i64 {
    (binary * 30103).div_euclid(100000)
}

fn decimal_exponent(ratio: &Ratio) -> Result<i32, Error> {
    if ratio.numerator.is_zero() {
        return Err(Error::InternalInvariant);
    }
    // Bit-length difference is within one of log2(value), so the estimate is
    // within one decimal exponent, including subnormals. It is only a starting
    // point: exact comparisons establish both inequalities.
    let binary = i64::try_from(ratio.numerator.bits()).map_err(|_| Error::InternalInvariant)?
        - i64::try_from(ratio.denominator.bits()).map_err(|_| Error::InternalInvariant)?;
    let mut exponent =
        i32::try_from(decimal_estimate(binary)).map_err(|_| Error::InternalInvariant)?;
    while compare_ten(ratio, exponent)? == Ordering::Less {
        exponent -= 1;
    }
    while compare_ten(ratio, exponent + 1)? != Ordering::Less {
        exponent += 1;
    }
    Ok(exponent)
}

fn general(output: &mut Output, ratio: &Ratio, precision: i32, radix: u8) -> Result<(), Error> {
    let mut exponent = decimal_exponent(ratio)?;
    let scaled = ratio.scaled_ten(precision - 1 - exponent)?;
    let rounded = integer::round_div(&scaled.numerator, &scaled.denominator)?;
    let mut digits = integer::decimal_digits(&rounded.value)?;
    if digits.len() == (precision + 1) as usize {
        // The only p+1-digit rounded value is 10^p. This is a decimal carry,
        // independent of the binary80 conversion that supplied the input.
        if !digits.starts_with('1') || !digits.as_bytes()[1..].iter().all(|&b| b == b'0') {
            return Err(Error::InternalInvariant);
        }
        digits.pop();
        exponent += 1;
    }
    if digits.len() != precision as usize {
        return Err(Error::InternalInvariant);
    }
    layout(output, digits.as_bytes(), exponent, precision, radix)
}

/// Writes `precision` significant decimal digits with decimal exponent
/// `exponent` in `%g` layout: fixed or exponential, trailing zeros removed.
fn layout(
    output: &mut Output,
    digits: &[u8],
    exponent: i32,
    precision: i32,
    radix: u8,
) -> Result<(), Error> {
    if exponent < -4 || exponent >= precision {
        output.append(&digits[..1])?;
        let mut end = digits.len();
        while end > 1 && digits[end - 1] == b'0' {
            end -= 1;
        }
        if end > 1 {
            output.append(&[radix])?;
            output.append(&digits[1..end])?;
        }
        output.append(b"e")?;
        output.exponent(exponent, 2)
    } else {
        let point = exponent + 1;
        if point <= 0 {
            let mut end = digits.len();
            while end > 1 && digits[end - 1] == b'0' {
                end -= 1;
            }
            output.append(b"0")?;
            output.append(&[radix])?;
            output.repeat(b'0', (-point) as usize)?;
            output.append(&digits[..end])
        } else {
            let point = point as usize;
            if point >= digits.len() {
                output.append(digits)?;
                output.repeat(b'0', point - digits.len())
            } else {
                let mut end = digits.len();
                while end > point && digits[end - 1] == b'0' {
                    end -= 1;
                }
                output.append(&digits[..point])?;
                if end > point {
                    output.append(&[radix])?;
                    output.append(&digits[point..end])?;
                }
                Ok(())
            }
        }
    }
}

fn hexadecimal(output: &mut Output, value: Value80, upper: bool, radix: u8) -> Result<(), Error> {
    output.append(if upper { b"0X" } else { b"0x" })?;
    if value.significand() == 0 {
        output.append(if upper { b"0P+0" } else { b"0p+0" })?;
        return Ok(());
    }
    let alphabet = if upper {
        b"0123456789ABCDEF"
    } else {
        b"0123456789abcdef"
    };
    let mut digits = [0_u8; 16];
    for (index, byte) in digits.iter_mut().enumerate() {
        *byte = alphabet[((value.significand() >> (4 * (15 - index))) & 15) as usize];
    }
    output.append(&digits[..1])?;
    let mut end = digits.len();
    while end > 1 && digits[end - 1] == b'0' {
        end -= 1;
    }
    if end > 1 {
        output.append(&[radix])?;
        output.append(&digits[1..end])?;
    }
    output.append(if upper { b"P" } else { b"p" })?;
    output.exponent(
        if value.exponent() == 0 {
            -16385
        } else {
            i32::from(value.exponent()) - 16386
        },
        1,
    )
}

// Default14 prints these exact integers without rounding or an exponent.
fn small_integer(output: &mut Output, value: Value80) -> Result<bool, Error> {
    let raw = value.raw();
    let exponent = i32::from(raw.exponent()) - 16383;
    if !(0..=46).contains(&exponent) {
        return Ok(false);
    }
    let shift = 63 - exponent as u32;
    if raw.significand() & ((1u64 << shift) - 1) != 0 {
        return Ok(false);
    }
    let mut integer = raw.significand() >> shift;
    if integer >= 100_000_000_000_000 {
        return Ok(false);
    }
    let mut digits = [0; 14];
    let mut start = digits.len();
    loop {
        start -= 1;
        digits[start] = b'0' + (integer % 10) as u8;
        integer /= 10;
        if integer == 0 {
            break;
        }
    }
    output.append(&digits[start..])?;
    Ok(true)
}

/// Default14 for an ordinary non-integer whose decimal exponent is -4..=13, where
/// the exact value times 10^(13 - exponent) fits 128 bits: the same exponent,
/// half-even rounding, carry and layout as `general`, computed without BigUint.
/// Returns false, having written nothing, outside that range.
fn small_decimal(output: &mut Output, value: Value80, radix: u8) -> Result<bool, Error> {
    let raw = value.raw();
    let significand = raw.significand();
    // value = significand / 2^shift, with the explicit integer bit set.
    let shift = 16383 + 63 - i32::from(raw.exponent());
    if raw.exponent() == 0 || significand >> 63 == 0 || !(1..=127).contains(&shift) {
        return Ok(false);
    }
    let shift = shift as u32;
    // floor(log2 value) is 63 - shift; this estimate is within one of the
    // decimal exponent, which the exact range check below settles.
    let mut exponent = decimal_estimate(63 - i64::from(shift)) as i32;
    for _ in 0..3 {
        if !(-4..=13).contains(&exponent) {
            return Ok(false);
        }
        let scaled = u128::from(significand) * 10u128.pow((13 - exponent) as u32);
        let quotient = scaled >> shift;
        if quotient >= 100_000_000_000_000 {
            exponent += 1;
            continue;
        }
        if quotient < 10_000_000_000_000 {
            exponent -= 1;
            continue;
        }
        let remainder = scaled & ((1u128 << shift) - 1);
        let half = 1u128 << (shift - 1);
        let mut digits = quotient as u64;
        if remainder > half || (remainder == half && digits & 1 == 1) {
            digits += 1;
        }
        if digits == 100_000_000_000_000 {
            digits = 10_000_000_000_000;
            exponent += 1;
        }
        if exponent > 13 {
            return Ok(false);
        }
        let mut text = [0u8; 14];
        for byte in text.iter_mut().rev() {
            *byte = b'0' + (digits % 10) as u8;
            digits /= 10;
        }
        layout(output, &text, exponent, 14, radix)?;
        return Ok(true);
    }
    Ok(false)
}

pub fn format(
    value: Value80,
    profile: Profile,
    id: FormatId,
    output_cap: usize,
) -> Result<Formatted, FormatFailure> {
    format_impl::<true>(value, profile, id, output_cap)
}

fn format_impl<const FAST_PATHS: bool>(
    value: Value80,
    profile: Profile,
    id: FormatId,
    output_cap: usize,
) -> Result<Formatted, FormatFailure> {
    let mut output = Output {
        bytes: Vec::new(),
        limit: output_cap,
    };
    let result = (|| -> Result<(), Error> {
        if output_cap > OUTPUT_CAP {
            return Err(Error::OutputCapacity);
        }
        if value.is_negative() {
            output.append(b"-")?;
        } else if id == FormatId::Signed21 {
            output.append(b"+")?;
        }
        let upper = id == FormatId::HexUpper;
        match value.classify() {
            ValueClass::Infinity => output.append(if upper { b"INF" } else { b"inf" })?,
            ValueClass::Nan { .. } => output.append(if upper { b"NAN" } else { b"nan" })?,
            ValueClass::Zero if !matches!(id, FormatId::HexLower | FormatId::HexUpper) => {
                output.append(b"0")?
            }
            _ if matches!(id, FormatId::HexLower | FormatId::HexUpper) => {
                hexadecimal(&mut output, value, upper, profile.radix())?
            }
            _ => {
                if FAST_PATHS && id == FormatId::Default14 && small_integer(&mut output, value)? {
                    return Ok(());
                }
                if FAST_PATHS
                    && id == FormatId::Default14
                    && small_decimal(&mut output, value, profile.radix())?
                {
                    return Ok(());
                }
                let ratio = value_ratio(value)?;
                match id {
                    FormatId::Fixed0 => {
                        let rounded = integer::round_div(&ratio.numerator, &ratio.denominator)?;
                        output.append(integer::decimal_digits(&rounded.value)?.as_bytes())?;
                    }
                    FormatId::Default14 => general(&mut output, &ratio, 14, profile.radix())?,
                    FormatId::Signed21 => general(&mut output, &ratio, 21, profile.radix())?,
                    _ => return Err(Error::InternalInvariant),
                }
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Ok(Formatted {
            input: value.raw(),
            after: value.raw(),
            bytes: output.bytes,
        }),
        Err(error) => Err(FormatFailure {
            error,
            input: value.raw(),
            after: value.raw(),
            partial_bytes: output.bytes,
        }),
    }
}

pub fn format_raw(
    raw: Raw80,
    profile: Profile,
    id: FormatId,
    output_cap: usize,
) -> Result<Formatted, RawFormatFailure> {
    let value = Value80::from_raw(raw).map_err(|error| RawFormatFailure {
        error: error.into(),
        raw,
        formatter: None,
    })?;
    format(value, profile, id, output_cap).map_err(|failure| RawFormatFailure {
        error: failure.error,
        raw,
        formatter: Some(failure),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(raw: Raw80) {
        let value = Value80::from_raw(raw).unwrap();
        for profile in [Profile::C, Profile::DE_NUMERIC, Profile::EN_NUMERIC] {
            for id in [
                FormatId::Default14,
                FormatId::Signed21,
                FormatId::Fixed0,
                FormatId::HexLower,
                FormatId::HexUpper,
            ] {
                for cap in (0..=22).chain([OUTPUT_CAP + 1]) {
                    let actual = format(value, profile, id, cap);
                    let expected = format_impl::<false>(value, profile, id, cap);
                    match (actual, expected) {
                        (Ok(a), Ok(b)) => {
                            assert_eq!(a.bytes, b.bytes, "{raw:?} {profile:?} {id:?}");
                            assert_eq!((a.input, a.after), (b.input, b.after));
                        }
                        (Err(a), Err(b)) => {
                            assert_eq!(a.error, b.error);
                            assert_eq!(a.partial_bytes, b.partial_bytes, "{raw:?} cap={cap}");
                            assert_eq!((a.input, a.after), (b.input, b.after));
                        }
                        pair => panic!("different outcome for {raw:?} {id:?} cap={cap}: {pair:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn exact_integer_specialization_preserves_general_results_and_failures() {
        let mut integers = vec![
            1,
            99_999_999_999_999,
            100_000_000_000_000,
            100_000_000_000_001,
            u64::MAX,
        ];
        for power in 1..=14 {
            let n = 10u64.pow(power);
            integers.extend([n - 1, n, n + 1]);
        }
        for power in 1..=47 {
            let n = 1u64 << power;
            integers.extend([n - 1, n, n + 1]);
        }
        integers.sort_unstable();
        integers.dedup();
        for n in integers {
            let top = 63 - n.leading_zeros();
            let exponent = (16383 + top) as u16;
            let significand = n << (63 - top);
            for sign in [0, 0x8000] {
                compare(Raw80::new(exponent | sign, significand));
                if top <= 46 {
                    compare(Raw80::new(exponent | sign, significand + 1));
                    if significand > 1u64 << 63 {
                        compare(Raw80::new(exponent | sign, significand - 1));
                    } else {
                        compare(Raw80::new((exponent - 1) | sign, u64::MAX));
                    }
                }
            }
        }
        for sign in [0, 0x8000] {
            for raw in [
                Raw80::new(sign, 0),
                Raw80::new(sign, 1),
                Raw80::new(sign | 0x7fff, 1 << 63),
                Raw80::new(sign | 0x7fff, (1 << 63) | 1),
                Raw80::new(sign | 0x7fff, (1 << 63) | (1 << 62)),
            ] {
                compare(raw);
            }
        }
    }

    fn default14_matches(raw: Raw80) {
        let value = Value80::from_raw(raw).unwrap();
        for profile in [Profile::C, Profile::DE_NUMERIC, Profile::EN_NUMERIC] {
            let actual = format(value, profile, FormatId::Default14, OUTPUT_CAP);
            let expected = format_impl::<false>(value, profile, FormatId::Default14, OUTPUT_CAP);
            assert_eq!(
                actual.map(|f| f.bytes).map_err(|f| f.error),
                expected.map(|f| f.bytes).map_err(|f| f.error),
                "{raw:?} {profile:?}"
            );
        }
    }

    fn normal(shift: u32, significand: u64) -> Raw80 {
        Raw80::new((16383 + 63 - shift) as u16, significand)
    }

    #[test]
    fn small_decimal_matches_general() {
        // The specialization handles ordinary values (so the comparisons below
        // exercise it) and declines exponential layout.
        for (raw, taken) in [
            (normal(64, 1u64 << 63), true),             // 0.5
            (normal(60, 0xc400_0000_0000_0000), true),  // 12.25
            (normal(76, 0xd1b7_1758_e219_652c), true),  // about 2e-4
            (normal(80, 0xa7c5_ac47_1b47_8423), false), // about 1e-5
            // 99999999999999.5 rounds to 10^14, whose exponential layout is declined.
            (
                normal(63 - 47 + 1, 199_999_999_999_999u64 << (63 - 47)),
                false,
            ),
        ] {
            let mut output = Output {
                bytes: Vec::new(),
                limit: OUTPUT_CAP,
            };
            let value = Value80::from_raw(raw).unwrap();
            assert_eq!(
                small_decimal(&mut output, value, b'.').unwrap(),
                taken,
                "{raw:?}"
            );
            assert_eq!(output.bytes.is_empty(), !taken);
        }
        let mut boundary = Vec::new();
        // Exact halfway cases: at exponent 13 these are n + 1/2 with 14 digits.
        for n in [
            10_000_000_000_000u64,
            12_345_678_901_234,
            12_345_678_901_235,
            99_999_999_999_999,
        ] {
            let twice = 2 * n + 1;
            let top = 63 - twice.leading_zeros();
            boundary.push(normal(63 - top + 1, twice << (63 - top)));
        }
        // Exact ties at every specialized exponent X: 2N + 1 = m 5^(13 - X), so the
        // value m / 2^(14 - X) is representable; both parities of N.
        for exponent in -4i32..=13 {
            let power = 5u128.pow((13 - exponent) as u32);
            let mut m = (20_000_000_000_001u128).div_ceil(power) | 1;
            for _ in 0..6 {
                let n = (m * power - 1) / 2;
                if (10_000_000_000_000..100_000_000_000_000).contains(&n) {
                    let top = 127 - m.leading_zeros();
                    let significand = (m << (63 - top)) as u64;
                    boundary.push(normal(63 - top + (14 - exponent) as u32, significand));
                }
                m += 2;
            }
        }
        // Binary80 neighbours of every power of ten across the specialized range.
        for power in -6i32..=15 {
            for shift in 1u32..=127 {
                let estimate = if power >= 0 {
                    10u128
                        .checked_pow(power as u32)
                        .and_then(|p| p.checked_mul(1u128 << shift))
                } else {
                    Some((1u128 << shift) / 10u128.pow((-power) as u32))
                };
                let Some(estimate) = estimate.filter(|e| *e < 1u128 << 65) else {
                    continue;
                };
                for delta in -3i128..=3 {
                    let significand = estimate as i128 + delta;
                    if (1i128 << 63..1i128 << 64).contains(&significand) {
                        boundary.push(normal(shift, significand as u64));
                    }
                }
            }
        }
        // Every binary shift with edge significands.
        for shift in 1..=127 {
            for significand in [
                1u64 << 63,
                (1u64 << 63) + 1,
                0xcccc_cccc_cccc_cccd,
                u64::MAX - 1,
                u64::MAX,
            ] {
                boundary.push(normal(shift, significand));
            }
        }
        for &raw in &boundary {
            for sign in [0, 0x8000] {
                compare(Raw80::new(raw.exponent() | sign, raw.significand()));
            }
        }
        let mut state = 0x6a09_e667_f3bc_c909u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..200_000 {
            let shift = 1 + (next() % 80) as u32;
            let sign = if next() & 1 == 0 { 0 } else { 0x8000 };
            let raw = normal(shift, next() | 1u64 << 63);
            default14_matches(Raw80::new(raw.exponent() | sign, raw.significand()));
        }
    }
}
