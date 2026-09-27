//! Checked entry points to the selected integer-only BigUint dependency paths.
//!
//! Internal library allocations can still abort. Preflights bound requested
//! sizes; they do not turn the library into a recoverable-OOM implementation.

use num_bigint::BigUint;
use num_integer::Integer;
use num_traits::{One, Zero};
use std::cmp::Ordering;

use crate::profile::{COEFFICIENT_BITS, Error, INTERMEDIATE_BITS, POWER_CAP, SHIFT_CAP};

pub fn check_bits(bits: u64) -> Result<(), Error> {
    if bits > INTERMEDIATE_BITS {
        Err(Error::IntermediateCapacity)
    } else {
        Ok(())
    }
}

pub fn shl(value: &BigUint, shift: u32) -> Result<BigUint, Error> {
    if shift > SHIFT_CAP {
        return Err(Error::ShiftCapacity);
    }
    check_bits(
        value
            .bits()
            .checked_add(u64::from(shift))
            .ok_or(Error::IntermediateCapacity)?,
    )?;
    Ok(value << shift as usize)
}

pub fn mul(left: &BigUint, right: &BigUint) -> Result<BigUint, Error> {
    check_bits(
        left.bits()
            .checked_add(right.bits())
            .ok_or(Error::IntermediateCapacity)?,
    )?;
    Ok(left * right)
}

pub fn power(base: u8, exponent: u32) -> Result<BigUint, Error> {
    if exponent > POWER_CAP {
        return Err(Error::PowerCapacity);
    }
    let per_bit = match base {
        5 => 3,
        10 => 4,
        _ => return Err(Error::InternalInvariant),
    };
    check_bits(u64::from(exponent) * per_bit + 1)?;
    Ok(BigUint::from(base).pow(exponent))
}

pub fn coefficient(digits: &[u8], radix: u8) -> Result<BigUint, Error> {
    if digits.is_empty() || !matches!(radix, 10 | 16) || digits.iter().any(|&d| d >= radix) {
        return Err(Error::InternalInvariant);
    }
    if digits.len() > 8192 {
        return Err(Error::CoefficientCapacity);
    }
    let value = BigUint::from_radix_be(digits, u32::from(radix)).ok_or(Error::InternalInvariant)?;
    if value.bits() > COEFFICIENT_BITS {
        return Err(Error::CoefficientCapacity);
    }
    Ok(value)
}

#[derive(Clone, Debug)]
pub struct Ratio {
    pub numerator: BigUint,
    pub denominator: BigUint,
}

impl Ratio {
    pub fn new(numerator: BigUint, denominator: BigUint) -> Result<Self, Error> {
        check_bits(numerator.bits())?;
        check_bits(denominator.bits())?;
        if denominator.is_zero() {
            return Err(Error::InternalInvariant);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    pub fn shifted(coefficient: BigUint, exponent: i32) -> Result<Self, Error> {
        if exponent >= 0 {
            Self::new(shl(&coefficient, exponent as u32)?, BigUint::one())
        } else {
            Self::new(coefficient, shl(&BigUint::one(), exponent.unsigned_abs())?)
        }
    }

    pub fn compare_power_two(&self, exponent: i32) -> Result<Ordering, Error> {
        if exponent >= 0 {
            Ok(self
                .numerator
                .cmp(&shl(&self.denominator, exponent as u32)?))
        } else {
            Ok(shl(&self.numerator, exponent.unsigned_abs())?.cmp(&self.denominator))
        }
    }

    pub fn floor_log2(&self) -> Result<i32, Error> {
        if self.numerator.is_zero() {
            return Err(Error::InternalInvariant);
        }
        let k = i32::try_from(self.numerator.bits()).map_err(|_| Error::IntermediateCapacity)?
            - i32::try_from(self.denominator.bits()).map_err(|_| Error::IntermediateCapacity)?;
        Ok(if self.compare_power_two(k)? == Ordering::Less {
            k - 1
        } else {
            k
        })
    }

    pub fn at_quantum(&self, quantum: i32) -> Result<RoundedInteger, Error> {
        if quantum >= 0 {
            round_div(&self.numerator, &shl(&self.denominator, quantum as u32)?)
        } else {
            round_div(
                &shl(&self.numerator, quantum.unsigned_abs())?,
                &self.denominator,
            )
        }
    }

    pub fn scaled_ten(&self, exponent: i32) -> Result<Self, Error> {
        let scale = power(10, exponent.unsigned_abs())?;
        if exponent >= 0 {
            Self::new(mul(&self.numerator, &scale)?, self.denominator.clone())
        } else {
            Self::new(self.numerator.clone(), mul(&self.denominator, &scale)?)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemainderRelation {
    Zero,
    BelowHalf,
    Tie,
    AboveHalf,
}

#[derive(Clone, Debug)]
pub struct RoundedInteger {
    pub value: BigUint,
    pub relation: RemainderRelation,
    pub quotient_odd: bool,
}

pub fn round_div(numerator: &BigUint, denominator: &BigUint) -> Result<RoundedInteger, Error> {
    check_bits(numerator.bits())?;
    check_bits(denominator.bits())?;
    if denominator.is_zero() {
        return Err(Error::InternalInvariant);
    }
    let (mut quotient, remainder) = numerator.div_rem(denominator);
    let quotient_odd = quotient.bit(0);
    let relation = if remainder.is_zero() {
        RemainderRelation::Zero
    } else {
        match shl(&remainder, 1)?.cmp(denominator) {
            Ordering::Less => RemainderRelation::BelowHalf,
            Ordering::Equal => RemainderRelation::Tie,
            Ordering::Greater => RemainderRelation::AboveHalf,
        }
    };
    if relation == RemainderRelation::AboveHalf
        || (relation == RemainderRelation::Tie && quotient_odd)
    {
        check_bits(
            quotient
                .bits()
                .checked_add(1)
                .ok_or(Error::IntermediateCapacity)?,
        )?;
        quotient += BigUint::one();
    }
    Ok(RoundedInteger {
        value: quotient,
        relation,
        quotient_odd,
    })
}

pub fn as_u64(value: &BigUint) -> Result<u64, Error> {
    if value.bits() > 64 {
        return Err(Error::InternalInvariant);
    }
    Ok(value.iter_u64_digits().next().unwrap_or(0))
}

pub fn decimal_digits(value: &BigUint) -> Result<String, Error> {
    check_bits(value.bits())?;
    Ok(value.to_str_radix(10))
}
