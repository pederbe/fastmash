//! Table-driven fixed-point exponential with a rounding certificate: the
//! correctly rounded binary80 result where the certificate settles it, `None`
//! otherwise, so the caller keeps its arbitrary-precision path.
//!
//! x = n ln 2 + r with |r| <= ln 2 / 2, r = j/256 + t with |t| <= 2^-9, and
//! exp(x) = 2^n exp(j/256) exp(t): exp(j/256) from exp_table.rs, exp(t) from its
//! Taylor series, all in units u = 2^-112.
//!
//! Error bound of m, the computed exp(r) in units u (checked by the tests):
//! - LN2 is within 1/2 u of ln 2, so r is within |n|/2 u of x - n ln 2, which
//!   moves exp(r) by at most exp(r) |n|/2 u <= 0.71 |n| u;
//! - the table entry is within 1/2 u, times exp(t) <= 1.002: 0.51 u;
//! - the series: coefficients within 1/2 u, each Horner product truncated by
//!   less than 1 u, later steps scaling earlier errors by |t| <= 2^-9, and the
//!   omitted terms below 2^-130: under 2 u, times the entry (< 1.42): 2.84 u;
//! - the final product truncates by less than 1 u.
//!
//! Total below 0.71 |n| + 4.4 u; the certificate deliberately uses the wider
//! |n| + 64 u.
use super::exp_table::{CENTRE, EXP};

const ONE: i128 = 1 << 112;
/// round(ln 2 * 2^112).
const LN2: i128 = 0xb17217f7d1cf79abc9e3b39803f3;
/// Taylor terms t^k / k! kept for |t| <= 2^-9; t^12/12! is below 2^-136.
const TERMS: usize = 11;
/// round(2^112 / k!) for k = 0..=TERMS.
const COEFFICIENTS: [i128; TERMS + 1] = {
    let mut coefficients = [0; TERMS + 1];
    let mut factorial: i128 = 1;
    let mut k = 0;
    while k <= TERMS {
        if k > 0 {
            factorial *= k as i128;
        }
        coefficients[k] = (2 * ONE + factorial) / (2 * factorial);
        k += 1;
    }
    coefficients
};

/// a*b/2^112 rounded toward zero, for |a|, |b| < 2^113.
fn multiply(a: i128, b: i128) -> i128 {
    let (x, y) = (a.unsigned_abs(), b.unsigned_abs());
    debug_assert!(x >> 113 == 0 && y >> 113 == 0);
    let (xl, xh) = (u128::from(x as u64), x >> 64);
    let (yl, yh) = (u128::from(y as u64), y >> 64);
    let middle = xl * yh + xh * yl + ((xl * yl) >> 64);
    let magnitude = ((xh * yh) << 16) + (middle >> 48);
    if (a < 0) != (b < 0) {
        -(magnitude as i128)
    } else {
        magnitude as i128
    }
}

/// value / divisor rounded to nearest (ties away from zero), divisor > 0.
fn round_div(value: i128, divisor: i128) -> i128 {
    let half = divisor / 2;
    if value >= 0 {
        (value + half) / divisor
    } else {
        -((-value + half) / divisor)
    }
}

/// The correctly rounded binary80 exp(x) bits, when the certificate settles
/// it; `None` outside 2^-40 <= |x| < 2^14, for results that are not normal, and
/// for an ambiguous rounding.
pub fn exp(bits: u128) -> Option<u128> {
    let x = fixed_input(bits)?;
    let n = round_div(x, LN2);
    let r = x - n * LN2;
    let j = round_div(r, 1 << 104);
    let t = r - (j << 104);
    let entry = *EXP.get(usize::try_from(CENTRE as i128 + j).ok()?)?;
    let mut polynomial = COEFFICIENTS[TERMS];
    for coefficient in COEFFICIENTS[..TERMS].iter().rev() {
        polynomial = coefficient + multiply(t, polynomial);
    }
    let m = multiply(entry as i128, polynomial);
    let error = n.abs() + 64;
    let lower = scaled_binary80(m - error, n)?;
    (scaled_binary80(m + error, n)? == lower).then_some(lower)
}

/// x * 2^112 exactly, for canonical finite x with 2^-40 <= |x| < 2^14.
fn fixed_input(bits: u128) -> Option<i128> {
    let significand = bits as u64;
    let exponent = ((bits >> 64) & 0x7fff) as i32;
    if bits >> 80 != 0 || significand >> 63 == 0 {
        return None;
    }
    // |x| = significand 2^(exponent - 16383 - 63); scaled by 2^112.
    let shift = exponent - 16383 - 63 + 112;
    if !(9..=62).contains(&shift) {
        return None;
    }
    let magnitude = i128::from(significand) << shift;
    Some(if bits >> 79 == 1 {
        -magnitude
    } else {
        magnitude
    })
}

/// Binary80 bits nearest (ties to even) to value * 2^(n - 112), for value > 0;
/// `None` unless the result is a normal number.
fn scaled_binary80(value: i128, n: i128) -> Option<u128> {
    let magnitude = u128::try_from(value).ok().filter(|&v| v != 0)?;
    let top = 127 - magnitude.leading_zeros() as i128;
    let shift = top - 63;
    let mut significand = if shift <= 0 {
        magnitude << -shift
    } else {
        let retained = magnitude >> shift;
        let remainder = magnitude & ((1 << shift) - 1);
        let half = 1 << (shift - 1);
        retained + u128::from(remainder > half || (remainder == half && retained & 1 != 0))
    };
    let mut exponent = 16383 + top - 112 + n;
    if significand == 1 << 64 {
        significand >>= 1;
        exponent += 1;
    }
    (1..=32766)
        .contains(&exponent)
        .then_some((exponent as u128) << 64 | significand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use astro_float_num::{BigFloat, Consts, RoundingMode, Sign};

    /// round(value * 2^112) of a positive astro-float value.
    fn q112(value: &BigFloat) -> i128 {
        let (words, _, sign, exponent, _) = value.as_raw_parts().unwrap();
        assert_eq!(sign, Sign::Pos);
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
        ((doubled + 1) >> 1) as i128
    }

    #[test]
    fn table_and_constants_are_nearest_to_independent_values() {
        let mut constants = Consts::new().unwrap();
        for (at, &entry) in EXP.iter().enumerate() {
            let j = at as i64 - CENTRE as i64;
            let exponent = BigFloat::from_i64(j, 64)
                .div(&BigFloat::from_u16(256, 64), 256, RoundingMode::ToEven)
                .exp(256, RoundingMode::ToEven, &mut constants);
            assert_eq!(entry as i128, q112(&exponent), "entry {j}");
        }
        let ln2 = BigFloat::from_u8(2, 64).ln(256, RoundingMode::ToEven, &mut constants);
        assert_eq!(LN2, q112(&ln2));
        let mut factorial = 1u128;
        for (k, &coefficient) in COEFFICIENTS.iter().enumerate() {
            if k > 0 {
                factorial *= k as u128;
            }
            // |coefficient * k! - 2^112| <= k!/2.
            let product = coefficient as u128 * factorial;
            assert!(product.abs_diff(1 << 112) <= factorial / 2, "{k}");
        }
    }

    #[test]
    fn every_reduced_argument_stays_in_the_table_and_series_range() {
        // r is reduced exactly by the integer LN2, so |r| <= LN2 / 2 < 89.5 * 2^104:
        // j is within the table and |t| <= 2^-9.
        for x in [
            ONE * 11356,
            -ONE * 11356,
            LN2 / 2,
            -LN2 / 2,
            LN2 / 2 + 1,
            1 << 104,
        ] {
            let n = round_div(x, LN2);
            let r = x - n * LN2;
            let j = round_div(r, 1 << 104);
            assert!(j.abs() <= CENTRE as i128, "{x}");
            assert!((r - (j << 104)).abs() <= 1 << 103, "{x}");
        }
    }
}
