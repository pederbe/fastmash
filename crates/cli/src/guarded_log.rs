//! Table-driven fixed-point logarithm with a rounding certificate and optional
//! fallback. The table comes from log_table.rs; the error bound is below.
use super::log_table::{LOWER, UPPER};

const ONE: u128 = 1 << 112;
/// The binary80 bits of 1.0, whose logarithm is exactly zero.
const ONE_BITS: u128 = (16383 << 64) | (1 << 63);
const LN2: i128 = 0xb17217f7d1cf79abc9e3b39803f2;
/// Terms of log1p(t) = sum (-1)^(n+1) t^n / n kept for |t| <= 2^-8.
const TERMS: usize = 13;
/// Horner terms 1..=WIDE_TERMS run in units of 2^-112; the rest, whose weight t^n is
/// below 2^-56, in units of 2^-64.
const WIDE_TERMS: usize = 6;
/// (-1)^(n+1) round(2^112 / n) for n = 1..=WIDE_TERMS.
const WIDE_COEFFICIENTS: [i128; WIDE_TERMS] = {
    let mut coefficients = [0; WIDE_TERMS];
    let mut n = 1;
    while n <= WIDE_TERMS {
        let magnitude = ((2 * ONE + n as u128) / (2 * n as u128)) as i128;
        coefficients[n - 1] = if n % 2 == 1 { magnitude } else { -magnitude };
        n += 1;
    }
    coefficients
};
/// (-1)^(n+1) round(2^64 / n) for n = WIDE_TERMS+1..=TERMS.
const NARROW_COEFFICIENTS: [i64; TERMS - WIDE_TERMS] = {
    let mut coefficients = [0; TERMS - WIDE_TERMS];
    let mut n = WIDE_TERMS + 1;
    while n <= TERMS {
        let magnitude = (((1u128 << 65) + n as u128) / (2 * n as u128)) as i64;
        coefficients[n - WIDE_TERMS - 1] = if n % 2 == 1 { magnitude } else { -magnitude };
        n += 1;
    }
    coefficients
};

// Exact floor(a*b/2^112) for a, b < 2^113 (a derived bound, checked by the
// tests below): each
// 64-bit partial product and the 114-bit middle sum fit, so wrapping
// arithmetic never wraps and skips the overflow checks of this hot path.
fn multiply(a: u128, b: u128) -> u128 {
    debug_assert!(a >> 113 == 0 && b >> 113 == 0);
    let (al, ah) = (u128::from(a as u64), a >> 64);
    let (bl, bh) = (u128::from(b as u64), b >> 64);
    let middle = al
        .wrapping_mul(bh)
        .wrapping_add(ah.wrapping_mul(bl))
        .wrapping_add(al.wrapping_mul(bl) >> 64);
    (ah.wrapping_mul(bh) << 16).wrapping_add(middle >> 48)
}

/// a*b/2^112 rounded toward zero, for |a|, |b| < 2^113.
fn multiply_signed(a: i128, b: i128) -> i128 {
    let magnitude = multiply(a.unsigned_abs(), b.unsigned_abs()) as i128;
    if (a < 0) != (b < 0) {
        -magnitude
    } else {
        magnitude
    }
}

/// The correctly rounded binary80 logarithm's bits, when the certificate
/// settles it; `None` for inputs outside the kernel or an ambiguous rounding.
pub fn log(bits: u128) -> Option<u128> {
    if bits == ONE_BITS {
        return Some(0);
    }
    let (fixed, k) = fixed(bits)?;
    // |fixed - ln(x) 2^112| < |k| + 3 (the table's error bound); one unit spare.
    let error = i128::from(k.unsigned_abs()) + 4;
    let lower = nearest_binary80(fixed - error)?;
    (nearest_binary80(fixed + error)? == lower).then_some(lower)
}
/// ln(x) * 2^112 as an integer, and the k of x = m 2^k with m in
/// [0.75, 1.5); `None` unless x is positive, finite, canonical and not 1.
fn fixed(bits: u128) -> Option<(i128, i32)> {
    let significand = bits as u64;
    let exponent = (bits >> 64) as u16;
    if bits >> 80 != 0
        || exponent >= 0x7fff
        || significand == 0
        || (exponent == 0) != (significand >> 63 == 0)
        || bits == ONE_BITS
    {
        return None;
    }
    let shift = significand.leading_zeros();
    let mut k = i32::from(exponent.max(1)) - 16383 - shift as i32;
    let s = significand << shift;
    // m = s/2^63 in [1, 1.5), or m = s/2^64 in [0.75, 1) with k + 1; t = m*c - 1
    // is exact in units of 2^-126 before flooring to 2^-112.
    let (l, t126) = if s < 0xc000_0000_0000_0000 {
        let (c, l) = UPPER[((s - (1 << 63)) >> 55) as usize];
        (l, (u128::from(s) * u128::from(c)) as i128 - (1 << 126))
    } else {
        k += 1;
        let (c, l) = LOWER[((s - 0xc000_0000_0000_0000) >> 56) as usize];
        let t127 = (u128::from(s) * u128::from(c)).wrapping_sub(1 << 127) as i128;
        (l, t127 >> 1)
    };
    let t = t126 >> 14;
    let narrow = (t >> 48) as i64;
    let mut tail = NARROW_COEFFICIENTS[TERMS - WIDE_TERMS - 1];
    for coefficient in NARROW_COEFFICIENTS[..TERMS - WIDE_TERMS - 1].iter().rev() {
        tail = coefficient + ((i128::from(narrow) * i128::from(tail)) >> 64) as i64;
    }
    let mut polynomial = i128::from(tail) << 48;
    for coefficient in WIDE_COEFFICIENTS.iter().rev() {
        polynomial = coefficient + multiply_signed(t, polynomial);
    }
    Some((multiply_signed(t, polynomial) + l + i128::from(k) * LN2, k))
}

/// Binary80 bits nearest (ties to even) to fixed*2^-112, which is normal for
/// every nonzero i128 (|fixed| < 2^127); `None` for zero.
fn nearest_binary80(fixed: i128) -> Option<u128> {
    let magnitude = fixed.unsigned_abs();
    if magnitude == 0 {
        return None;
    }
    let top = 127 - magnitude.leading_zeros();
    let mut exponent = 16383 + top - 112;
    let mut significand = if top <= 63 {
        magnitude << (63 - top)
    } else {
        let shift = top - 63;
        let retained = magnitude >> shift;
        let remainder = magnitude & ((1 << shift) - 1);
        let half = 1 << (shift - 1);
        retained + u128::from(remainder > half || (remainder == half && retained & 1 != 0))
    };
    if significand == 1 << 64 {
        significand >>= 1;
        exponent += 1;
    }
    let sign = u128::from(fixed < 0) << 79;
    Some(sign | (u128::from(exponent) << 64) | significand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_apfloat::{Float, Round, ieee::X87DoubleExtended as Extended};
    #[test]
    fn fixed_product_carries_match_bitwise_wide_integer() {
        fn reference(a: u128, b: u128) -> u128 {
            let mut words = [0u64; 4];
            for bit in 0..113 {
                if b & (1 << bit) == 0 {
                    continue;
                }
                let low = a << bit;
                let high = if bit == 0 { 0 } else { a >> (128 - bit) };
                let add = [
                    low as u64,
                    (low >> 64) as u64,
                    high as u64,
                    (high >> 64) as u64,
                ];
                let mut carry = 0u128;
                for (word, add) in words.iter_mut().zip(add) {
                    let sum = u128::from(*word) + u128::from(add) + carry;
                    *word = sum as u64;
                    carry = sum >> 64;
                }
                assert_eq!(carry, 0);
            }
            u128::from(words[1] >> 48) | (u128::from(words[2]) << 16) | (u128::from(words[3]) << 80)
        }
        let boundaries = [
            0,
            1,
            (1 << 64) - 1,
            1 << 64,
            ONE / 25,
            ONE - 1,
            ONE,
            2 * ONE - 1,
        ];
        for a in boundaries {
            for b in boundaries {
                assert_eq!(multiply(a, b), reference(a, b));
            }
        }
        let mut state = 0x88d1c0ffeeu128;
        for _ in 0..1024 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let a = state & (2 * ONE - 1);
            let b = state.rotate_left(59) & (2 * ONE - 1);
            assert_eq!(multiply(a, b), reference(a, b));
        }
    }
    /// round(value * 2^112), ties away from zero, from an astro-float value's raw parts.
    fn q112(value: &astro_float_num::BigFloat) -> i128 {
        if value.is_zero() {
            return 0;
        }
        let (words, _, sign, exponent, _) = value.as_raw_parts().unwrap();
        // value = M 2^(exponent - n); keep floor(value 2^113) and round the last bit.
        let shift = words.len() as i32 * 64 - exponent - 113;
        assert!(shift > 0, "{shift}");
        let word = |i: usize| u128::from(words.get(i).copied().unwrap_or(0));
        let (at, bits) = (shift as usize / 64, shift as u32 % 64);
        let low = word(at) | (word(at + 1) << 64);
        let doubled = if bits == 0 {
            low
        } else {
            (low >> bits) | (word(at + 2) << (128 - bits))
        };
        let rounded = ((doubled + 1) >> 1) as i128;
        if sign == astro_float_num::Sign::Neg {
            -rounded
        } else {
            rounded
        }
    }

    #[test]
    fn table_entries_are_nearest_to_independent_logarithms() {
        use astro_float_num::{BigFloat, Consts, RoundingMode, Sign};
        let mut constants = Consts::new().unwrap();
        for (at, &(c, l)) in UPPER.iter().chain(&LOWER).enumerate() {
            let shift = c.leading_zeros();
            let reciprocal =
                BigFloat::from_raw_parts(&[c << shift], 64, Sign::Pos, 1 - shift as i32, false);
            let ln = reciprocal.ln(256, RoundingMode::ToEven, &mut constants);
            assert_eq!(l, -q112(&ln), "entry {at}");
        }
        // Reciprocal formulas: centred 1/256 steps, exactly one next to m = 1.
        let nearest = |n: u128, d: u128| ((2 * n + d) / (2 * d)) as u64;
        for (j, &(c, _)) in UPPER.iter().enumerate() {
            let expected = if j == 0 {
                1 << 63
            } else {
                nearest(1 << 72, 513 + 2 * j as u128)
            };
            assert_eq!(c, expected, "upper {j}");
        }
        for (j, &(c, _)) in LOWER.iter().enumerate() {
            let expected = if j == 63 {
                1 << 63
            } else {
                nearest(1 << 72, 385 + 2 * j as u128)
            };
            assert_eq!(c, expected, "lower {j}");
        }
        let ln2 = BigFloat::from_u8(2, 64).ln(256, RoundingMode::ToEven, &mut constants);
        // floor(ln 2 2^112): its fraction is 0.9636, so the nearest value is one more.
        assert_eq!(LN2 + 1, q112(&ln2), "LN2 is floor(ln 2 2^112)");
    }

    #[test]
    fn every_table_interval_reduces_to_at_most_two_to_the_minus_eight() {
        // t = m c - 1 at both ends of each interval, exactly in units of 2^-126.
        let limit = 1i128 << 118;
        for (j, &(c, _)) in UPPER.iter().enumerate() {
            for s in [
                (1u64 << 63) + ((j as u64) << 55),
                (1u64 << 63) + ((j as u64 + 1) << 55) - 1,
            ] {
                let t = (u128::from(s) * u128::from(c)) as i128 - (1 << 126);
                assert!(t.abs() <= limit, "upper {j} {s:#x}");
            }
        }
        for (j, &(c, _)) in LOWER.iter().enumerate() {
            let base = 0xc000_0000_0000_0000u64;
            for s in [
                base + ((j as u64) << 56),
                base.wrapping_add((j as u64 + 1) << 56).wrapping_sub(1),
            ] {
                let t = ((u128::from(s) * u128::from(c)).wrapping_sub(1 << 127) as i128) >> 1;
                assert!(t.abs() <= limit, "lower {j} {s:#x}");
            }
        }
    }

    #[test]
    fn fixed_logarithms_stay_within_their_error_bound() {
        use astro_float_num::{BigFloat, Consts, RoundingMode, Sign};
        let mut constants = Consts::new().unwrap();
        let ln2 = BigFloat::from_u8(2, 64).ln(256, RoundingMode::ToEven, &mut constants);
        let mut worst = 0i128;
        let mut check = |bits: u128| {
            let Some((fixed, k)) = fixed(bits) else {
                return;
            };
            let significand = bits as u64;
            let exponent = (bits >> 64) as u16;
            let shift = significand.leading_zeros();
            let source = BigFloat::from_raw_parts(
                &[significand << shift],
                64,
                Sign::Pos,
                i32::from(exponent.max(1)) - 16382 - shift as i32,
                false,
            );
            let ln = source.ln(256, RoundingMode::ToEven, &mut constants);
            // The k-free part ln(m) = ln(x) - k ln 2 carries the kernel's own
            // error (< 2.6 units); k * LN2 adds under |k| more.
            let k_ln2 = ln2.mul(&BigFloat::from_i32(k, 64), 256, RoundingMode::ToEven);
            let reduced = q112(&ln.sub(&k_ln2, 256, RoundingMode::ToEven));
            let distance = (fixed - i128::from(k) * LN2 - reduced).abs();
            assert!(distance <= 3, "{bits:#x}: k-free {distance}");
            worst = worst.max(distance);
            let whole = (fixed - q112(&ln)).abs();
            assert!(
                whole <= i128::from(k.unsigned_abs()) + 3,
                "{bits:#x}: {whole}"
            );
        };
        let mut state = 0x9e37_79b9_7f4a_7c15u128;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..20_000 {
            let bits = next();
            let significand = (bits as u64) | 1 << 63;
            let exponent = match (bits >> 64) % 4 {
                0 => 16383,
                1 => 16382,
                2 => (bits >> 70) as u16 % 0x7ffe + 1,
                _ => [1, 2, 32766, 32765][(bits >> 70) as usize % 4],
            };
            check((u128::from(exponent) << 64) | u128::from(significand));
        }
        // Every interval: both ends, their neighbours, and random interior
        // points, which reach the largest t floor fractions with each entry's
        // own L rounding.
        let intervals = (0..128u64)
            .map(|j| ((1u64 << 63) + (j << 55), 1u64 << 55))
            .chain((0..64).map(|j| (0xc000_0000_0000_0000u64 + (j << 56), 1u64 << 56)));
        for (start, width) in intervals {
            let mut points = vec![
                start,
                start + 1,
                start + (width - 1),
                start.wrapping_add(width),
            ];
            points.extend((0..16).map(|_| start + (next() as u64) % width));
            for significand in points.into_iter().filter(|s| s >> 63 == 1) {
                check((16383u128 << 64) | u128::from(significand));
                check((16382u128 << 64) | u128::from(significand));
            }
        }
        // The |t| = 2^-8 endpoints and subnormal inputs of every width.
        check((16383u128 << 64) | 0xff00_0000_0000_0000);
        check((16383u128 << 64) | 0x807f_ffff_ffff_ffff);
        for width in 1..64 {
            check(u128::from((1u64 << (width - 1)) | 1));
        }
        assert!(worst <= 3, "{worst}");
    }

    #[test]
    fn independent_mpfr_challenges_and_cancellation_fallback() {
        let mut certified = 0;
        let mut fallback = 0;
        for line in include_str!("testdata/log-mpfr.tsv")
            .lines()
            .filter(|s| !s.starts_with('#'))
        {
            let (input, expected) = line.split_once('\t').unwrap();
            let input = u128::from_str_radix(input, 16).unwrap();
            let expected = u128::from_str_radix(expected, 16).unwrap();
            if let Some(actual) = log(input) {
                assert_eq!(actual, expected, "input {input:020x}");
                certified += 1;
            } else {
                fallback += 1;
            }
        }
        assert_eq!(certified, 2253);
        assert_eq!(certified + fallback, 2509);
        assert_eq!(log(ONE_BITS), Some(0));
    }
    #[test]
    fn fixed_rounding_matches_apfloat_at_every_width_tie_and_carry() {
        fn reference(fixed: i128) -> u128 {
            Extended::from_i128_r(fixed, Round::NearestTiesToEven)
                .value
                .scalbn(-112)
                .to_bits()
        }
        let mut checked = 0;
        let mut check = |magnitude: u128| {
            for fixed in [magnitude as i128, -(magnitude as i128)] {
                assert_eq!(
                    nearest_binary80(fixed),
                    Some(reference(fixed)),
                    "{fixed:#x}"
                );
                checked += 1;
            }
        };
        // Every top-bit position a logarithm can reach, around each rounding boundary.
        for top in 47..=126 {
            let base = 1u128 << top;
            check(base);
            check((base << 1) - 1);
            if top > 63 {
                let half = 1u128 << (top - 64);
                for retained in [
                    base,
                    base | (1 << (top - 63)),
                    (base << 1) - (1 << (top - 63)),
                ] {
                    for offset in [-1i128, 0, 1] {
                        check((retained as i128 + half as i128 + offset) as u128);
                    }
                }
            }
        }
        let mut state = 0x9e3779b97f4a7c15u128;
        for _ in 0..100_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            check(state >> (2 + state % 79) | 1 << 47);
        }
        assert!(checked > 200_000);
        assert_eq!(nearest_binary80(0), None);
    }
    #[test]
    fn accepted_random_logarithms_match_retained_engine() {
        use fastmash_numeric_contract::{Raw80, Value80};
        let mut retained = fastmash_portable_numerics::PortableNumerics::new().unwrap();
        let (mut accepted, mut total) = (0, 0);
        let mut state = 0x2545f4914f6cdd1du128;
        for _ in 0..20_000 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let exponent = (state >> 64) as u16 % 0x7ffe + 1;
            let bits = (u128::from(exponent) << 64) | u128::from(state as u64 | 1 << 63);
            total += 1;
            if let Some(actual) = log(bits) {
                accepted += 1;
                // The retained engine is slow in debug builds; sample it.
                if accepted % 256 != 1 {
                    continue;
                }
                let input = Value80::from_raw(Raw80::try_from_bits(bits).unwrap()).unwrap();
                let input = fastmash_portable_numerics::ArithmeticValue::try_from(input).unwrap();
                assert_eq!(
                    actual,
                    retained.log(input).unwrap().raw().to_bits(),
                    "{bits:020x}"
                );
            }
        }
        assert!(accepted * 1000 > total * 999, "{accepted}/{total}");
    }
    #[test]
    fn rejects_noncanonical_and_nonpositive_inputs() {
        for bits in [
            0,
            1u128 << 79,
            1u128 << 80,
            1u128 << 63,
            (1u128 << 64) | 1,
            (0x7fffu128 << 64) | (1 << 63),
            (0x7fffu128 << 64) | u128::from(u64::MAX),
        ] {
            assert!(log(bits).is_none(), "{bits:020x}");
        }
    }
}
