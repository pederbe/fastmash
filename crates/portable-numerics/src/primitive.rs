use crate::NumericFailure;
use crate::arena::{Arena, PrimitiveCell, PrimitiveCells};
use fastmash_numeric_contract::{Raw80, Value80};
use std::cmp::Ordering;

#[derive(Clone, Copy)]
struct Window {
    count: u8,
    latest: i32,
    sticky: bool,
}

impl Arena {
    pub(crate) fn subtract_finite(
        &mut self,
        left: Value80,
        right: Value80,
    ) -> Result<Value80, NumericFailure> {
        self.primitive_value_scope(|scratch| subtract(scratch, left, right))
    }

    pub(crate) fn multiply_finite(
        &mut self,
        left: Value80,
        right: Value80,
    ) -> Result<Value80, NumericFailure> {
        self.primitive_value_scope(|scratch| multiply(scratch, left, right))
    }

    pub(crate) fn sqrt_finite(&mut self, value: Value80) -> Result<Value80, NumericFailure> {
        self.primitive_value_scope(|scratch| sqrt(scratch, value))
    }
}

fn finite_parts(value: Value80) -> (bool, u64, i32) {
    let exponent = value.exponent();
    let quantum = if exponent == 0 {
        -16_445
    } else {
        i32::from(exponent) - 16_446
    };
    (value.is_negative(), value.significand(), quantum)
}

fn scaled_cmp(left: u64, left_q: i32, right: u64, right_q: i32) -> Ordering {
    let left_top = left_q + (63 - left.leading_zeros()) as i32;
    let right_top = right_q + (63 - right.leading_zeros()) as i32;
    match left_top.cmp(&right_top) {
        Ordering::Equal => {
            let low = left_q.min(right_q);
            for position in (low..=left_top).rev() {
                let left_bit = operand_bit(left, left_q, position);
                let right_bit = operand_bit(right, right_q, position);
                match left_bit.cmp(&right_bit) {
                    Ordering::Equal => {}
                    order => return order,
                }
            }
            Ordering::Equal
        }
        order => order,
    }
}

fn operand_bit(value: u64, quantum: i32, position: i32) -> u8 {
    let index = position - quantum;
    if (0..64).contains(&index) {
        ((value >> index) & 1) as u8
    } else {
        0
    }
}

fn subtract(
    scratch: &mut PrimitiveCells<'_>,
    left: Value80,
    right: Value80,
) -> Result<Raw80, NumericFailure> {
    let (left_sign, left_magnitude, left_q) = finite_parts(left);
    let (right_sign, right_magnitude, right_q) = finite_parts(right);
    scratch.set(PrimitiveCell::V0, left_magnitude)?;
    scratch.set(PrimitiveCell::V1, right_magnitude)?;

    let right_effective_sign = !right_sign;
    let (add, first_magnitude, first_q, second_magnitude, second_q, sign) =
        if left_sign == right_effective_sign {
            (
                true,
                left_magnitude,
                left_q,
                right_magnitude,
                right_q,
                left_sign,
            )
        } else {
            match scaled_cmp(left_magnitude, left_q, right_magnitude, right_q) {
                Ordering::Equal => return Ok(Raw80::new(0, 0)),
                Ordering::Greater => (
                    false,
                    left_magnitude,
                    left_q,
                    right_magnitude,
                    right_q,
                    left_sign,
                ),
                Ordering::Less => (
                    false,
                    right_magnitude,
                    right_q,
                    left_magnitude,
                    left_q,
                    right_effective_sign,
                ),
            }
        };

    scratch.set(PrimitiveCell::V2, 0)?;
    scratch.set(PrimitiveCell::V3, 0)?;
    let low = first_q.min(second_q);
    let high = (first_q + 63).max(second_q + 63);
    let mut window = Window {
        count: 0,
        latest: low - 1,
        sticky: false,
    };
    let mut carry_or_borrow = 0_u8;
    let mut position = low;
    while position <= high {
        let first_active = (first_q..=first_q + 63).contains(&position);
        let second_active = (second_q..=second_q + 63).contains(&position);
        if !first_active && !second_active {
            let next = [first_q, second_q]
                .into_iter()
                .filter(|start| *start > position)
                .min()
                .ok_or(NumericFailure::Invariant)?;
            let count = (next - position) as u32;
            if add {
                if carry_or_borrow != 0 {
                    push(scratch, &mut window, 1)?;
                    push_run(scratch, &mut window, 0, count - 1)?;
                    carry_or_borrow = 0;
                } else {
                    push_run(scratch, &mut window, 0, count)?;
                }
            } else if carry_or_borrow != 0 {
                push_run(scratch, &mut window, 1, count)?;
            } else {
                push_run(scratch, &mut window, 0, count)?;
            }
            position = next;
            continue;
        }
        let first_bit = operand_bit(first_magnitude, first_q, position);
        let second_bit = operand_bit(second_magnitude, second_q, position);
        let output = if add {
            let sum = first_bit + second_bit + carry_or_borrow;
            carry_or_borrow = sum >> 1;
            sum & 1
        } else {
            let subtrahend = second_bit + carry_or_borrow;
            if first_bit >= subtrahend {
                carry_or_borrow = 0;
                first_bit - subtrahend
            } else {
                carry_or_borrow = 1;
                first_bit + 2 - subtrahend
            }
        };
        push(scratch, &mut window, output)?;
        position += 1;
    }
    if add && carry_or_borrow != 0 {
        push(scratch, &mut window, 1)?;
    } else if !add && carry_or_borrow != 0 {
        return Err(NumericFailure::Invariant);
    }
    let raw = round_window(scratch, window, sign)?;
    scratch.set(PrimitiveCell::V4, raw.significand())?;
    Ok(raw)
}

fn push(
    scratch: &mut PrimitiveCells<'_>,
    window: &mut Window,
    bit: u8,
) -> Result<(), NumericFailure> {
    if bit > 1 {
        return Err(NumericFailure::Invariant);
    }
    if window.count < 66 {
        set_window_bit(scratch, window.count, bit != 0)?;
        window.count += 1;
    } else {
        let low = scratch.get(PrimitiveCell::V2)?;
        let high = scratch.get(PrimitiveCell::V3)?;
        window.sticky |= low & 1 != 0;
        scratch.set(PrimitiveCell::V2, (low >> 1) | (high << 63))?;
        scratch.set(PrimitiveCell::V3, (high >> 1) | (u64::from(bit) << 1))?;
    }
    window.latest += 1;
    Ok(())
}

fn push_run(
    scratch: &mut PrimitiveCells<'_>,
    window: &mut Window,
    bit: u8,
    count: u32,
) -> Result<(), NumericFailure> {
    if count > 66 {
        window.sticky |= scratch.get(PrimitiveCell::V2)? != 0
            || scratch.get(PrimitiveCell::V3)? != 0
            || bit != 0;
        scratch.set(PrimitiveCell::V2, if bit == 0 { 0 } else { u64::MAX })?;
        scratch.set(PrimitiveCell::V3, if bit == 0 { 0 } else { 3 })?;
        window.count = 66;
        window.latest = window
            .latest
            .checked_add(count as i32)
            .ok_or(NumericFailure::Invariant)?;
        return Ok(());
    }
    for _ in 0..count {
        push(scratch, window, bit)?;
    }
    Ok(())
}

fn set_window_bit(
    scratch: &mut PrimitiveCells<'_>,
    index: u8,
    value: bool,
) -> Result<(), NumericFailure> {
    let (cell, shift) = if index < 64 {
        (PrimitiveCell::V2, index)
    } else if index < 66 {
        (PrimitiveCell::V3, index - 64)
    } else {
        return Err(NumericFailure::Invariant);
    };
    let current = scratch.get(cell)?;
    let mask = 1_u64 << shift;
    scratch.set(
        cell,
        if value {
            current | mask
        } else {
            current & !mask
        },
    )
}

fn window_bit(
    scratch: &PrimitiveCells<'_>,
    window: Window,
    position: i32,
) -> Result<bool, NumericFailure> {
    if window.count == 0 {
        return Ok(false);
    }
    let first = window.latest - i32::from(window.count) + 1;
    let index = position - first;
    if !(0..i32::from(window.count)).contains(&index) {
        return Ok(false);
    }
    let index = index as u8;
    if index < 64 {
        Ok(scratch.get(PrimitiveCell::V2)? >> index & 1 != 0)
    } else {
        Ok(scratch.get(PrimitiveCell::V3)? >> (index - 64) & 1 != 0)
    }
}

fn window_any_below(
    scratch: &PrimitiveCells<'_>,
    window: Window,
    cut: i32,
) -> Result<bool, NumericFailure> {
    if window.sticky {
        let first = window.latest - i32::from(window.count) + 1;
        if first <= cut {
            return Ok(true);
        }
        return Err(NumericFailure::Invariant);
    }
    let first = window.latest - i32::from(window.count) + 1;
    let end = cut.min(window.latest + 1);
    for position in first..end {
        if window_bit(scratch, window, position)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn round_window(
    scratch: &PrimitiveCells<'_>,
    window: Window,
    sign: bool,
) -> Result<Raw80, NumericFailure> {
    let mut top = None;
    for index in (0..window.count).rev() {
        let position = window.latest - i32::from(window.count - 1 - index);
        if window_bit(scratch, window, position)? {
            top = Some(position);
            break;
        }
    }
    let top = top.ok_or(NumericFailure::Invariant)?;
    round_bits(
        sign,
        top,
        |position| window_bit(scratch, window, position),
        |cut| window_any_below(scratch, window, cut),
    )
}

fn multiply(
    scratch: &mut PrimitiveCells<'_>,
    left: Value80,
    right: Value80,
) -> Result<Raw80, NumericFailure> {
    let (left_sign, left_magnitude, left_q) = finite_parts(left);
    let (right_sign, right_magnitude, right_q) = finite_parts(right);
    scratch.set(PrimitiveCell::V0, left_magnitude)?;
    scratch.set(PrimitiveCell::V1, right_magnitude)?;
    let product = u128::from(left_magnitude) * u128::from(right_magnitude);
    scratch.set(PrimitiveCell::V2, product as u64)?;
    scratch.set(PrimitiveCell::V3, (product >> 64) as u64)?;
    let quantum = left_q
        .checked_add(right_q)
        .ok_or(NumericFailure::Invariant)?;
    let high = scratch.get(PrimitiveCell::V3)?;
    let top_index = if high == 0 {
        (63 - scratch.get(PrimitiveCell::V2)?.leading_zeros()) as i32
    } else {
        64 + (63 - high.leading_zeros()) as i32
    };
    let raw = round_bits(
        left_sign ^ right_sign,
        quantum + top_index,
        |position| product_bit(scratch, position - quantum),
        |cut| product_any_below(scratch, cut - quantum),
    )?;
    scratch.set(PrimitiveCell::V4, raw.significand())?;
    Ok(raw)
}

fn product_bit(scratch: &PrimitiveCells<'_>, index: i32) -> Result<bool, NumericFailure> {
    if !(0..128).contains(&index) {
        return Ok(false);
    }
    if index < 64 {
        Ok(scratch.get(PrimitiveCell::V2)? >> index & 1 != 0)
    } else {
        Ok(scratch.get(PrimitiveCell::V3)? >> (index - 64) & 1 != 0)
    }
}

fn product_any_below(scratch: &PrimitiveCells<'_>, cut: i32) -> Result<bool, NumericFailure> {
    for index in 0..cut.clamp(0, 128) {
        if product_bit(scratch, index)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn round_bits(
    sign: bool,
    top: i32,
    mut bit: impl FnMut(i32) -> Result<bool, NumericFailure>,
    mut any_below: impl FnMut(i32) -> Result<bool, NumericFailure>,
) -> Result<Raw80, NumericFailure> {
    let mut quantum = (top - 63).max(-16_445);
    let mut retained = 0_u64;
    for index in 0..64 {
        if bit(quantum + index)? {
            retained |= 1_u64 << index;
        }
    }
    let guard = bit(quantum - 1)?;
    let sticky = any_below(quantum - 1)?;
    if guard && (sticky || retained & 1 != 0) {
        if retained == u64::MAX {
            retained = 1_u64 << 63;
            quantum = quantum.checked_add(1).ok_or(NumericFailure::Invariant)?;
        } else {
            retained += 1;
        }
    }
    if retained == 0 {
        return Ok(Raw80::new(if sign { 0x8000 } else { 0 }, 0));
    }
    if quantum == -16_445 && retained < 1_u64 << 63 {
        return Ok(Raw80::new(if sign { 0x8000 } else { 0 }, retained));
    }
    let exponent = quantum
        .checked_add(16_446)
        .ok_or(NumericFailure::Invariant)?;
    if exponent >= 0x7fff {
        return Ok(Raw80::new(if sign { 0xffff } else { 0x7fff }, 1_u64 << 63));
    }
    if !(1..0x7fff).contains(&exponent) || retained >> 63 == 0 {
        return Err(NumericFailure::Invariant);
    }
    Ok(Raw80::new(
        exponent as u16 | if sign { 0x8000 } else { 0 },
        retained,
    ))
}

fn sqrt(scratch: &mut PrimitiveCells<'_>, value: Value80) -> Result<Raw80, NumericFailure> {
    let (_, magnitude, quantum) = finite_parts(value);
    let leading = magnitude.leading_zeros();
    let normalized = magnitude
        .checked_shl(leading)
        .ok_or(NumericFailure::Invariant)?;
    let normalized_q = quantum - leading as i32;
    scratch.set(PrimitiveCell::V0, normalized)?;
    scratch.set(PrimitiveCell::V1, 0)?;
    let scale = if normalized_q & 1 == 0 {
        scratch.set(PrimitiveCell::V2, normalized << 2)?;
        scratch.set(PrimitiveCell::V3, normalized >> 62)?;
        normalized_q / 2 - 33
    } else {
        scratch.set(PrimitiveCell::V2, normalized << 1)?;
        scratch.set(PrimitiveCell::V3, normalized >> 63)?;
        (normalized_q - 1) / 2 - 32
    };
    for cell in [
        PrimitiveCell::V4,
        PrimitiveCell::V5,
        PrimitiveCell::V6,
        PrimitiveCell::V7,
    ] {
        scratch.set(cell, 0)?;
    }
    for pair_index in (0..65).rev() {
        shift_pair_left(scratch, PrimitiveCell::V6, PrimitiveCell::V7, 2)?;
        let pair = (radicand_bit(scratch, pair_index * 2 + 1)? as u64) << 1
            | radicand_bit(scratch, pair_index * 2)? as u64;
        scratch.set(PrimitiveCell::V6, scratch.get(PrimitiveCell::V6)? | pair)?;
        let take = compare_trial(scratch)? != Ordering::Less;
        if take {
            subtract_trial(scratch)?;
        }
        shift_pair_left(scratch, PrimitiveCell::V4, PrimitiveCell::V5, 1)?;
        if take {
            scratch.set(PrimitiveCell::V4, scratch.get(PrimitiveCell::V4)? | 1)?;
        }
    }
    let root_low = scratch.get(PrimitiveCell::V4)?;
    let root_high = scratch.get(PrimitiveCell::V5)?;
    let guard = root_low & 1 != 0;
    let remainder_nonzero =
        scratch.get(PrimitiveCell::V6)? != 0 || scratch.get(PrimitiveCell::V7)? != 0;
    let mut kept = (root_low >> 1) | (root_high << 63);
    let mut result_quantum = scale + 1;
    if guard && (remainder_nonzero || kept & 1 != 0) {
        if kept == u64::MAX {
            kept = 1_u64 << 63;
            result_quantum += 1;
        } else {
            kept += 1;
        }
    }
    let exponent = result_quantum + 16_446;
    if !(1..0x7fff).contains(&exponent) || kept >> 63 == 0 {
        return Err(NumericFailure::Invariant);
    }
    Ok(Raw80::new(exponent as u16, kept))
}

fn trial_limb(scratch: &PrimitiveCells<'_>, index: u8) -> Result<u64, NumericFailure> {
    let root_low = scratch.get(PrimitiveCell::V4)?;
    match index {
        0 => Ok((root_low << 2) | 1),
        1 => Ok((scratch.get(PrimitiveCell::V5)? << 2) | (root_low >> 62)),
        _ => Err(NumericFailure::Invariant),
    }
}

fn compare_trial(scratch: &PrimitiveCells<'_>) -> Result<Ordering, NumericFailure> {
    for (index, remainder_cell) in [(1, PrimitiveCell::V7), (0, PrimitiveCell::V6)] {
        match scratch
            .get(remainder_cell)?
            .cmp(&trial_limb(scratch, index)?)
        {
            Ordering::Equal => {}
            order => return Ok(order),
        }
    }
    Ok(Ordering::Equal)
}

fn subtract_trial(scratch: &mut PrimitiveCells<'_>) -> Result<(), NumericFailure> {
    let mut borrow = false;
    for (index, remainder_cell) in [(0, PrimitiveCell::V6), (1, PrimitiveCell::V7)] {
        let (difference, limb_borrow) = scratch
            .get(remainder_cell)?
            .overflowing_sub(trial_limb(scratch, index)?);
        let (difference, carry_borrow) = difference.overflowing_sub(u64::from(borrow));
        scratch.set(remainder_cell, difference)?;
        borrow = limb_borrow || carry_borrow;
    }
    if borrow {
        return Err(NumericFailure::Invariant);
    }
    Ok(())
}

fn shift_pair_left(
    scratch: &mut PrimitiveCells<'_>,
    low_cell: PrimitiveCell,
    high_cell: PrimitiveCell,
    count: u32,
) -> Result<(), NumericFailure> {
    let low = scratch.get(low_cell)?;
    let high = scratch.get(high_cell)?;
    if count == 0 || count >= 64 || high >> (64 - count) != 0 {
        return Err(NumericFailure::Invariant);
    }
    scratch.set(low_cell, low << count)?;
    scratch.set(high_cell, (high << count) | (low >> (64 - count)))
}

fn radicand_bit(scratch: &PrimitiveCells<'_>, index: i32) -> Result<bool, NumericFailure> {
    if !(0..192).contains(&index) {
        return Ok(false);
    }
    let (cell, shift) = if index < 64 {
        (PrimitiveCell::V1, index)
    } else if index < 128 {
        (PrimitiveCell::V2, index - 64)
    } else {
        (PrimitiveCell::V3, index - 128)
    };
    Ok(scratch.get(cell)? >> shift & 1 != 0)
}
