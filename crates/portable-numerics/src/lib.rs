//! Software arithmetic for the `portable-binary80-v3` numerical contract.
//!
//! The production CLI uses this crate for its remaining wider arithmetic. Its
//! public surface is the numerical seam; the fixed-limb substrate and
//! certificate machine are internal.

#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]

mod arena;
mod dyadic;
mod limb;
mod machine;
mod primitive;

use arena::Arena;
use fastmash_numeric_contract::{Value80, ValueClass};
use std::cmp::Ordering;

pub const PROFILE: &str = "portable-binary80-v3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueAdmissionFailure {
    SignalingNan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumericFailure {
    UnsupportedDomain,
    Capacity,
    Allocation,
    Invariant,
    TableIdentity,
}

#[derive(Clone, Copy, Debug)]
pub struct ArithmeticValue(Value80);

impl From<ArithmeticValue> for Value80 {
    fn from(value: ArithmeticValue) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparison {
    Less,
    Equal,
    Greater,
    Unordered,
}

impl TryFrom<Value80> for ArithmeticValue {
    type Error = ValueAdmissionFailure;

    fn try_from(value: Value80) -> Result<Self, Self::Error> {
        // Only the all-ones exponent encodes a NaN; every other value is admitted
        // without classification, the hot path for arithmetic results.
        if value.exponent() != 0x7fff {
            return Ok(Self(value));
        }
        if matches!(value.classify(), ValueClass::Nan { quiet: false, .. }) {
            Err(ValueAdmissionFailure::SignalingNan)
        } else {
            Ok(Self(value))
        }
    }
}

/// Restricted owner for exact square root without transcendental cache setup.
/// Retains the checked arena allocation and primitive cleanup rules.
pub struct SquareRoot {
    arena: Arena,
}

impl SquareRoot {
    pub fn new() -> Result<Self, NumericFailure> {
        Self::new_with(Arena::new)
    }

    fn new_with(allocate: fn() -> Result<Arena, NumericFailure>) -> Result<Self, NumericFailure> {
        Ok(Self { arena: allocate()? })
    }

    pub fn sqrt(&mut self, value: ArithmeticValue) -> Result<Value80, NumericFailure> {
        square_root(&mut self.arena, value)
    }
}

fn square_root(arena: &mut Arena, value: ArithmeticValue) -> Result<Value80, NumericFailure> {
    if let Some(value) = first_nan(value.0, None) {
        return Ok(value);
    }
    match value.0.classify() {
        ValueClass::Zero => Ok(value.0),
        ValueClass::Infinity if value.0.is_negative() => Arena::canonical_nan(),
        ValueClass::Infinity => Ok(value.0),
        ValueClass::Subnormal | ValueClass::Normal if value.0.is_negative() => {
            Arena::canonical_nan()
        }
        ValueClass::Subnormal | ValueClass::Normal => arena.sqrt_finite(value.0),
        ValueClass::Nan { .. } => Ok(value.0.quiet_nan()),
    }
}

pub struct PortableNumerics {
    arena: Arena,
}

impl PortableNumerics {
    pub fn new() -> Result<Self, NumericFailure> {
        let mut arena = Arena::new()?;
        arena.initialize_cache()?;
        arena.reset_disposable()?;
        Ok(Self { arena })
    }

    pub const fn profile() -> &'static str {
        PROFILE
    }

    pub fn add(
        &mut self,
        left: ArithmeticValue,
        right: ArithmeticValue,
    ) -> Result<Value80, NumericFailure> {
        if let Some(value) = first_nan(left.0, Some(right.0)) {
            return Ok(value);
        }
        match (left.0.classify(), right.0.classify()) {
            (ValueClass::Infinity, ValueClass::Infinity)
                if left.0.is_negative() != right.0.is_negative() =>
            {
                Arena::canonical_nan()
            }
            (ValueClass::Infinity, _) => Ok(left.0),
            (_, ValueClass::Infinity) => Ok(right.0),
            (ValueClass::Zero, ValueClass::Zero)
                if left.0.is_negative() && right.0.is_negative() =>
            {
                signed_zero(true)
            }
            (ValueClass::Zero, ValueClass::Zero) => signed_zero(false),
            _ => {
                let result = self.arena.add_finite(left.0, right.0);
                self.finish_call(result)
            }
        }
    }

    pub fn divide(
        &mut self,
        left: ArithmeticValue,
        right: ArithmeticValue,
    ) -> Result<Value80, NumericFailure> {
        if let Some(value) = first_nan(left.0, Some(right.0)) {
            return Ok(value);
        }
        let sign = left.0.is_negative() ^ right.0.is_negative();
        match (left.0.classify(), right.0.classify()) {
            (ValueClass::Zero, ValueClass::Zero) | (ValueClass::Infinity, ValueClass::Infinity) => {
                Arena::canonical_nan()
            }
            (ValueClass::Infinity, _) | (_, ValueClass::Zero) => Arena::infinity(sign),
            (ValueClass::Zero, _) | (_, ValueClass::Infinity) => signed_zero(sign),
            _ => {
                let result = self.arena.divide_finite(left.0, right.0);
                self.finish_call(result)
            }
        }
    }

    pub fn subtract(
        &mut self,
        left: ArithmeticValue,
        right: ArithmeticValue,
    ) -> Result<Value80, NumericFailure> {
        if let Some(value) = first_nan(left.0, Some(right.0)) {
            return Ok(value);
        }
        match (left.0.classify(), right.0.classify()) {
            (ValueClass::Infinity, ValueClass::Infinity)
                if left.0.is_negative() == right.0.is_negative() =>
            {
                Arena::canonical_nan()
            }
            (ValueClass::Infinity, _) => Ok(left.0),
            (_, ValueClass::Infinity) => flip_sign(right.0),
            (ValueClass::Zero, ValueClass::Zero) => {
                signed_zero(left.0.is_negative() && !right.0.is_negative())
            }
            (_, ValueClass::Zero) => Ok(left.0),
            (ValueClass::Zero, _) => flip_sign(right.0),
            _ => self.arena.subtract_finite(left.0, right.0),
        }
    }

    pub fn multiply(
        &mut self,
        left: ArithmeticValue,
        right: ArithmeticValue,
    ) -> Result<Value80, NumericFailure> {
        if let Some(value) = first_nan(left.0, Some(right.0)) {
            return Ok(value);
        }
        let sign = left.0.is_negative() ^ right.0.is_negative();
        match (left.0.classify(), right.0.classify()) {
            (ValueClass::Zero, ValueClass::Infinity) | (ValueClass::Infinity, ValueClass::Zero) => {
                Arena::canonical_nan()
            }
            (ValueClass::Infinity, _) | (_, ValueClass::Infinity) => Arena::infinity(sign),
            (ValueClass::Zero, _) | (_, ValueClass::Zero) => signed_zero(sign),
            _ => self.arena.multiply_finite(left.0, right.0),
        }
    }

    pub fn sqrt(&mut self, value: ArithmeticValue) -> Result<Value80, NumericFailure> {
        square_root(&mut self.arena, value)
    }

    pub fn log(&mut self, value: ArithmeticValue) -> Result<Value80, NumericFailure> {
        if let Some(value) = first_nan(value.0, None) {
            return Ok(value);
        }
        match value.0.classify() {
            ValueClass::Zero => Arena::infinity(true),
            ValueClass::Subnormal => Err(NumericFailure::UnsupportedDomain),
            ValueClass::Infinity if value.0.is_negative() => Arena::canonical_nan(),
            ValueClass::Infinity => Ok(value.0),
            ValueClass::Normal if value.0.is_negative() => Arena::canonical_nan(),
            ValueClass::Normal => {
                let result = self.arena.log_finite(value.0);
                self.finish_call(result)
            }
            ValueClass::Nan { .. } => Ok(value.0.quiet_nan()),
        }
    }

    pub fn exp(&mut self, value: ArithmeticValue) -> Result<Value80, NumericFailure> {
        if let Some(value) = first_nan(value.0, None) {
            return Ok(value);
        }
        match value.0.classify() {
            ValueClass::Infinity if value.0.is_negative() => signed_zero(false),
            ValueClass::Infinity => Ok(value.0),
            ValueClass::Zero | ValueClass::Subnormal | ValueClass::Normal => {
                let result = self.arena.exp_finite(value.0);
                self.finish_call(result)
            }
            ValueClass::Nan { .. } => Ok(value.0.quiet_nan()),
        }
    }

    fn finish_call(
        &mut self,
        result: Result<Value80, NumericFailure>,
    ) -> Result<Value80, NumericFailure> {
        self.arena.release_call_state()?;
        result
    }
}

fn first_nan(left: Value80, right: Option<Value80>) -> Option<Value80> {
    [Some(left), right].into_iter().flatten().find_map(|value| {
        matches!(value.classify(), ValueClass::Nan { .. }).then(|| value.quiet_nan())
    })
}

fn signed_zero(sign: bool) -> Result<Value80, NumericFailure> {
    Ok(Value80::signed_zero(sign))
}

fn flip_sign(value: Value80) -> Result<Value80, NumericFailure> {
    Ok(value.with_sign(!value.is_negative()))
}

pub fn promote_u64(value: u64) -> Value80 {
    Value80::exact_u64(value)
}

pub fn clear_sign(value: ArithmeticValue) -> ArithmeticValue {
    ArithmeticValue(value.0.with_sign(false))
}

pub fn compare(left: ArithmeticValue, right: ArithmeticValue) -> Comparison {
    if matches!(left.0.classify(), ValueClass::Nan { .. })
        || matches!(right.0.classify(), ValueClass::Nan { .. })
    {
        return Comparison::Unordered;
    }
    if matches!(left.0.classify(), ValueClass::Zero)
        && matches!(right.0.classify(), ValueClass::Zero)
    {
        return Comparison::Equal;
    }
    if left.0.same_bits(right.0) {
        return Comparison::Equal;
    }
    if left.0.is_negative() != right.0.is_negative() {
        return if left.0.is_negative() {
            Comparison::Less
        } else {
            Comparison::Greater
        };
    }
    let magnitude =
        (left.0.exponent(), left.0.significand()).cmp(&(right.0.exponent(), right.0.significand()));
    let magnitude = if left.0.is_negative() {
        magnitude.reverse()
    } else {
        magnitude
    };
    match magnitude {
        Ordering::Less => Comparison::Less,
        Ordering::Equal => Comparison::Equal,
        Ordering::Greater => Comparison::Greater,
    }
}

pub fn compare_magnitude(left: ArithmeticValue, right: ArithmeticValue) -> Comparison {
    compare(clear_sign(left), clear_sign(right))
}

#[inline]
pub fn sample_order(left: ArithmeticValue, right: ArithmeticValue) -> Ordering {
    match compare(left, right) {
        Comparison::Less => Ordering::Less,
        Comparison::Greater => Ordering::Greater,
        Comparison::Equal | Comparison::Unordered => Ordering::Equal,
    }
}

pub fn percentile_position(n: usize, percent: u8) -> (usize, ArithmeticValue) {
    debug_assert!((2..=65_536).contains(&n));
    debug_assert!((1..=99).contains(&percent));

    let (fraction_significand, fraction_power) = round_positive_binary64(u128::from(percent), 100);
    let product = fraction_significand * (n - 1) as u128;
    let bits = 128 - product.leading_zeros();
    let mut shift = bits.saturating_sub(53);
    let mut rounded = product >> shift;
    if shift != 0 {
        let remainder = product & ((1_u128 << shift) - 1);
        let halfway = 1_u128 << (shift - 1);
        if remainder > halfway || (remainder == halfway && rounded & 1 != 0) {
            rounded += 1;
        }
    }
    if rounded == 1_u128 << 53 {
        rounded >>= 1;
        shift += 1;
    }
    let power = fraction_power + shift as i32;
    let (index, fraction) = if power >= 0 {
        ((rounded << power) as usize, 0_u128)
    } else {
        let fractional_bits = (-power) as u32;
        let index = (rounded >> fractional_bits) as usize;
        let mask = (1_u128 << fractional_bits) - 1;
        (index, rounded & mask)
    };
    let value = if fraction == 0 {
        Value80::signed_zero(false)
    } else {
        let top = 127 - fraction.leading_zeros();
        let significand = (fraction << (63 - top)) as u64;
        let quantum = power - (63 - top) as i32;
        let exponent = (quantum + 16_446) as u16;
        // The admitted percentile bounds prove these fields are already in
        // the normal range; the representation owner enforces canonicality.
        Value80::canonical_normal(false, exponent, significand)
    };
    (index, ArithmeticValue(value))
}

fn round_positive_binary64(numerator: u128, denominator: u128) -> (u128, i32) {
    let mut exponent = -1_i32;
    while exponent > -7 && (numerator << (-exponent)) < denominator {
        exponent -= 1;
    }
    let scaled = numerator << (52 - exponent);
    let mut quotient = scaled / denominator;
    let remainder = scaled % denominator;
    if remainder * 2 > denominator || (remainder * 2 == denominator && quotient & 1 != 0) {
        quotient += 1;
    }
    if quotient == 1_u128 << 53 {
        quotient >>= 1;
        exponent += 1;
    }
    (quotient, exponent - 52)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod v3_tests;
