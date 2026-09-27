use crate::NumericFailure;
use crate::arena::{Arena, Desc, Slot9, Slot17};
use fastmash_numeric_contract::{Raw80, Value80};
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Round {
    Down,
    Up,
    NearestEven,
}

impl Arena {
    pub(crate) fn import_value(
        &mut self,
        dst: Slot9,
        value: Value80,
    ) -> Result<(), NumericFailure> {
        let raw = value.raw();
        let sign = raw.is_negative();
        let significand = raw.significand();
        if significand == 0 {
            return self.clear9(dst, sign);
        }
        let exponent = if raw.exponent() == 0 {
            -16_445
        } else {
            i32::from(raw.exponent()) - 16_446
        };
        self.assign9(dst, significand, sign, exponent)
    }

    pub(crate) fn export_binary80(&self, src: Slot9) -> Result<Value80, NumericFailure> {
        let desc = self.desc9(src)?;
        if desc.len == 0 {
            return Value80::from_raw(Raw80::new(if desc.sign { 0x8000 } else { 0 }, 0))
                .map_err(|_| NumericFailure::Invariant);
        }
        let bits = self.bit_len9(src)?;
        let top = desc
            .exp
            .checked_add(i32::from(bits) - 1)
            .ok_or(NumericFailure::Invariant)?;
        if top > 16_383 {
            return Self::infinity(desc.sign);
        }
        let (field, significand) = if top < -16_382 {
            if desc.exp != -16_445 || bits > 63 {
                return Err(NumericFailure::Invariant);
            }
            (0, self.limb9(src, 0)?)
        } else {
            if bits > 64 {
                return Err(NumericFailure::Invariant);
            }
            let mut significand = self.limb9(src, 0)?;
            if desc.len > 1 {
                return Err(NumericFailure::Invariant);
            }
            significand = significand
                .checked_shl(u32::from(64 - bits))
                .ok_or(NumericFailure::Invariant)?;
            ((top + 16_383) as u16, significand)
        };
        Value80::from_raw(Raw80::new(
            field | if desc.sign { 0x8000 } else { 0 },
            significand,
        ))
        .map_err(|_| NumericFailure::Invariant)
    }

    pub(crate) fn infinity(sign: bool) -> Result<Value80, NumericFailure> {
        Ok(Value80::infinity(sign))
    }

    pub(crate) fn canonical_nan() -> Result<Value80, NumericFailure> {
        Ok(Value80::canonical_quiet_nan())
    }

    fn copy17_bits_to9(
        &mut self,
        dst: Slot9,
        src: Slot17,
        start: u16,
        count: u16,
        sign: bool,
        exp: i32,
    ) -> Result<(), NumericFailure> {
        if count > 576 {
            return Err(NumericFailure::Invariant);
        }
        self.clear9(dst, sign)?;
        for bit in 0..count {
            if self.bit17(
                src,
                start.checked_add(bit).ok_or(NumericFailure::Invariant)?,
            )? {
                self.set_bit9(dst, bit)?;
            }
        }
        self.normalize9(dst, sign, exp)
    }

    pub(crate) fn should_increment(
        sign: bool,
        mode: Round,
        guard: bool,
        lower: bool,
        odd: bool,
    ) -> bool {
        let discarded = guard || lower;
        match mode {
            Round::Down => sign && discarded,
            Round::Up => !sign && discarded,
            Round::NearestEven => guard && (lower || odd),
        }
    }

    pub(crate) fn round17(
        &mut self,
        src: Slot17,
        dst: Slot9,
        precision: u16,
        sign: bool,
        mode: Round,
    ) -> Result<(), NumericFailure> {
        if precision == 0 || precision > 544 {
            return Err(NumericFailure::Invariant);
        }
        let desc = self.desc17(src)?;
        let bits = self.bit_len17(src)?;
        if bits == 0 {
            return self.clear9(dst, sign);
        }
        if bits <= precision {
            return self.copy17_bits_to9(dst, src, 0, bits, sign, desc.exp);
        }
        let cut = bits
            .checked_sub(precision)
            .ok_or(NumericFailure::Invariant)?;
        let guard = self.bit17(src, cut - 1)?;
        let lower = self.any_below17(src, cut - 1)?;
        let exp = desc
            .exp
            .checked_add(i32::from(cut))
            .ok_or(NumericFailure::Invariant)?;
        self.copy17_bits_to9(dst, src, cut, precision, sign, exp)?;
        let odd = self.bit9(dst, 0)?;
        if Self::should_increment(sign, mode, guard, lower, odd) {
            let old_exp = self.desc9(dst)?.exp;
            if self.increment9(dst)? {
                return Err(NumericFailure::Invariant);
            }
            self.normalize9(dst, sign, old_exp)?;
        }
        if self.bit_len9(dst)? > precision {
            let old_exp = self.desc9(dst)?.exp;
            if !self.shift_right9_one(dst)? {
                return Err(NumericFailure::Invariant);
            }
            let next = old_exp.checked_add(1).ok_or(NumericFailure::Invariant)?;
            self.normalize9(dst, sign, next)?;
        } else {
            self.normalize9(dst, sign, self.desc9(dst)?.exp)?;
        }
        Ok(())
    }

    fn round9_in_place(
        &mut self,
        slot: Slot9,
        precision: u16,
        sign: bool,
        mode: Round,
        sticky: bool,
    ) -> Result<(), NumericFailure> {
        let bits = self.bit_len9(slot)?;
        if bits == 0 {
            return self.clear9(slot, sign);
        }
        if bits <= precision {
            if sticky {
                return Err(NumericFailure::Invariant);
            }
            return self.normalize9(slot, sign, self.desc9(slot)?.exp);
        }
        if bits != precision + 1 {
            return Err(NumericFailure::Invariant);
        }
        let guard = self.bit9(slot, 0)?;
        let old_exp = self.desc9(slot)?.exp;
        self.shift_right9_one(slot)?;
        self.normalize9(
            slot,
            sign,
            old_exp.checked_add(1).ok_or(NumericFailure::Invariant)?,
        )?;
        let odd = self.bit9(slot, 0)?;
        if Self::should_increment(sign, mode, guard, sticky, odd) {
            let old_exp = self.desc9(slot)?.exp;
            if self.increment9(slot)? {
                return Err(NumericFailure::Invariant);
            }
            self.normalize9(slot, sign, old_exp)?;
        }
        if self.bit_len9(slot)? > precision {
            let old_exp = self.desc9(slot)?.exp;
            if !self.shift_right9_one(slot)? {
                return Err(NumericFailure::Invariant);
            }
            let exp = old_exp.checked_add(1).ok_or(NumericFailure::Invariant)?;
            self.normalize9(slot, sign, exp)?;
        } else {
            self.normalize9(slot, sign, self.desc9(slot)?.exp)?;
        }
        Ok(())
    }

    fn round9_to_grid(
        &mut self,
        slot: Slot9,
        grid: i32,
        sign: bool,
        mut lower: bool,
    ) -> Result<(), NumericFailure> {
        let desc = self.desc9(slot)?;
        let bits = self.bit_len9(slot)?;
        if bits == 0 {
            return self.clear9(slot, sign);
        }
        let cut = grid
            .checked_sub(desc.exp)
            .ok_or(NumericFailure::Invariant)?;
        if cut <= 0 {
            let shift = u16::try_from(-cut).map_err(|_| NumericFailure::Invariant)?;
            self.shift_left9(slot, shift)?;
            return self.normalize9(slot, sign, grid);
        }
        let cut = u16::try_from(cut).map_err(|_| NumericFailure::Invariant)?;
        let guard = cut <= bits && self.bit9(slot, cut - 1)?;
        if cut > 1 {
            lower |= self.any_below9(slot, (cut - 1).min(bits))?;
        }
        let retained = bits.saturating_sub(cut);
        if retained == 0 {
            self.clear9(slot, sign)?;
        } else {
            for _ in 0..cut {
                let _ = self.shift_right9_one(slot)?;
            }
            self.normalize9(slot, sign, grid)?;
        }
        let odd = retained != 0 && self.bit9(slot, 0)?;
        if Self::should_increment(sign, Round::NearestEven, guard, lower, odd) {
            self.increment9(slot)?;
            self.normalize9(slot, sign, grid)?;
        }
        Ok(())
    }

    fn push_window_one(
        &mut self,
        dst: Slot9,
        position: i32,
        precision: u16,
        sign: bool,
    ) -> Result<(), NumericFailure> {
        if self.bit_len9(dst)? == 0 {
            self.set_bit9(dst, 0)?;
            self.normalize9(dst, sign, position)?;
        } else {
            let index = position
                .checked_sub(self.desc9(dst)?.exp)
                .ok_or(NumericFailure::Invariant)?;
            if index <= 0 {
                return Err(NumericFailure::Invariant);
            }
            let index = u16::try_from(index).map_err(|_| NumericFailure::Invariant)?;
            if index <= precision {
                self.set_bit9(dst, index)?;
                self.normalize9(dst, sign, self.desc9(dst)?.exp)?;
            } else {
                let shifts = index
                    .checked_sub(precision)
                    .ok_or(NumericFailure::Invariant)?;
                for _ in 0..shifts {
                    if self.limb9(dst, 0)? & 1 != 0 {
                        self.set_scalar(5, 1)?;
                    }
                    for limb in 0..9 {
                        let value = self.limb9(dst, limb)? >> 1;
                        let incoming = if limb == 8 {
                            0
                        } else {
                            self.limb9(dst, limb + 1)? << 63
                        };
                        self.set_limb9(dst, limb, value | incoming)?;
                    }
                }
                self.set_bit9(dst, precision)?;
                self.normalize9(
                    dst,
                    sign,
                    position
                        .checked_sub(i32::from(precision))
                        .ok_or(NumericFailure::Invariant)?,
                )?;
            }
        }
        Ok(())
    }

    fn magnitude_step(
        &mut self,
        slot: Slot9,
        precision: u16,
        upward: bool,
    ) -> Result<(), NumericFailure> {
        let bits = self.bit_len9(slot)?;
        if bits == 0 || bits > precision {
            return Err(NumericFailure::Invariant);
        }
        if bits < precision {
            self.shift_left9(slot, precision - bits)?;
            let exp = self
                .desc9(slot)?
                .exp
                .checked_sub(i32::from(precision - bits))
                .ok_or(NumericFailure::Invariant)?;
            self.normalize9(slot, self.desc9(slot)?.sign, exp)?;
        }
        if upward {
            let old_desc = self.desc9(slot)?;
            self.increment9(slot)?;
            self.normalize9(slot, old_desc.sign, old_desc.exp)?;
            if self.bit_len9(slot)? > precision {
                let old_exp = self.desc9(slot)?.exp;
                self.shift_right9_one(slot)?;
                let exp = old_exp.checked_add(1).ok_or(NumericFailure::Invariant)?;
                self.normalize9(slot, self.desc9(slot)?.sign, exp)?;
            }
        } else {
            let power = self.bit_len9(slot)? == precision
                && self.bit9(slot, precision - 1)?
                && !self.any_below9(slot, precision - 1)?;
            if power {
                let desc = self.desc9(slot)?;
                self.clear9(slot, desc.sign)?;
                for bit in 0..precision {
                    self.set_bit9(slot, bit)?;
                }
                let exp = desc.exp.checked_sub(1).ok_or(NumericFailure::Invariant)?;
                self.normalize9(slot, desc.sign, exp)?;
            } else {
                self.decrement9(slot)?;
                self.normalize9(slot, self.desc9(slot)?.sign, self.desc9(slot)?.exp)?;
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn aligned_combine(
        &mut self,
        left: Slot9,
        right: Slot9,
        subtract_right: bool,
        precision: u16,
        mode: Round,
        dst: Slot9,
        binary80: bool,
    ) -> Result<(), NumericFailure> {
        if precision == 0 || precision > 544 || dst == left || dst == right {
            return Err(NumericFailure::Invariant);
        }
        if self.desc9(left)?.len == 0 {
            self.copy9(dst, right)?;
            let desc = self.desc9(dst)?;
            self.set_desc9(
                dst,
                Desc {
                    sign: desc.sign ^ subtract_right,
                    ..desc
                },
            )?;
            return Ok(());
        }
        if self.desc9(right)?.len == 0 {
            return self.copy9(dst, left);
        }
        let left_top = self
            .desc9(left)?
            .exp
            .checked_add(i32::from(self.bit_len9(left)?) - 1)
            .ok_or(NumericFailure::Invariant)?;
        let right_top = self
            .desc9(right)?
            .exp
            .checked_add(i32::from(self.bit_len9(right)?) - 1)
            .ok_or(NumericFailure::Invariant)?;
        let gap = left_top.abs_diff(right_top);
        if gap > u32::from(precision + 1) {
            let right_sign = self.desc9(right)?.sign ^ subtract_right;
            let (dominant, dominant_sign, exact_above) = if left_top > right_top {
                let above = if self.desc9(left)?.sign == right_sign {
                    !self.desc9(left)?.sign
                } else {
                    self.desc9(left)?.sign
                };
                (left, self.desc9(left)?.sign, above)
            } else {
                let above = if self.desc9(left)?.sign == right_sign {
                    !right_sign
                } else {
                    right_sign
                };
                (right, right_sign, above)
            };
            self.copy9(dst, dominant)?;
            let desc = self.desc9(dst)?;
            self.set_desc9(
                dst,
                Desc {
                    sign: dominant_sign,
                    ..desc
                },
            )?;
            let step = match mode {
                Round::NearestEven => None,
                Round::Down if !exact_above => Some(false),
                Round::Up if exact_above => Some(true),
                _ => None,
            };
            if let Some(numeric_up) = step {
                let magnitude_up = if dominant_sign {
                    !numeric_up
                } else {
                    numeric_up
                };
                self.magnitude_step(dst, precision, magnitude_up)?;
            }
            return Ok(());
        }

        let right_sign = self.desc9(right)?.sign ^ subtract_right;
        let same_sign = self.desc9(left)?.sign == right_sign;
        let magnitude_order =
            self.shifted_cmp9(left, self.desc9(left)?.exp, right, self.desc9(right)?.exp)?;
        if !same_sign && magnitude_order == Ordering::Equal {
            return self.clear9(dst, false);
        }
        let (larger, smaller, result_sign) = if same_sign || magnitude_order != Ordering::Less {
            (left, right, self.desc9(left)?.sign)
        } else {
            (right, left, right_sign)
        };
        let bottom = self.desc9(left)?.exp.min(self.desc9(right)?.exp);
        let top = left_top.max(right_top);
        if top.checked_sub(bottom).ok_or(NumericFailure::Invariant)? > i32::from(2 * precision) {
            return Err(NumericFailure::Invariant);
        }
        self.clear9(dst, result_sign)?;
        self.set_scalar(4, 0)?;
        self.set_scalar(5, 0)?;
        self.set_scalar(3, bottom as u32 as u64)?;
        loop {
            let position = self.scalar(3)? as u32 as i32;
            let (out, next_state) = if same_sign {
                let sum = u8::from(self.virtual_bit9(left, self.desc9(left)?.exp, position)?)
                    + u8::from(self.virtual_bit9(right, self.desc9(right)?.exp, position)?)
                    + self.scalar(4)? as u8;
                (sum & 1 != 0, sum > 1)
            } else {
                let l = i8::from(self.virtual_bit9(larger, self.desc9(larger)?.exp, position)?);
                let s = i8::from(self.virtual_bit9(smaller, self.desc9(smaller)?.exp, position)?);
                let value = l - s - self.scalar(4)? as i8;
                if value < 0 {
                    (value + 2 != 0, true)
                } else {
                    (value != 0, false)
                }
            };
            self.set_scalar(4, u64::from(next_state))?;
            if out {
                self.push_window_one(dst, position, precision, result_sign)?;
            }
            if position == top {
                break;
            }
            self.set_scalar(
                3,
                position.checked_add(1).ok_or(NumericFailure::Invariant)? as u32 as u64,
            )?;
        }
        if same_sign && self.scalar(4)? != 0 {
            self.push_window_one(
                dst,
                top.checked_add(1).ok_or(NumericFailure::Invariant)?,
                precision,
                result_sign,
            )?;
        } else if !same_sign && self.scalar(4)? != 0 {
            return Err(NumericFailure::Invariant);
        }
        if self.bit_len9(dst)? == 0 {
            return self.clear9(dst, false);
        }
        if binary80 {
            if mode != Round::NearestEven || precision != 64 {
                return Err(NumericFailure::Invariant);
            }
            let top = self
                .desc9(dst)?
                .exp
                .checked_add(i32::from(self.bit_len9(dst)?) - 1)
                .ok_or(NumericFailure::Invariant)?;
            if top < -16_382 {
                self.round9_to_grid(dst, -16_445, result_sign, self.scalar(5)? != 0)
            } else {
                self.round9_in_place(dst, precision, result_sign, mode, self.scalar(5)? != 0)
            }
        } else {
            self.round9_in_place(dst, precision, result_sign, mode, self.scalar(5)? != 0)
        }
    }

    fn compare17_to_overflow(
        &self,
        src: Slot17,
        sign_exp: i32,
    ) -> Result<Ordering, NumericFailure> {
        let bits = self.bit_len17(src)?;
        if bits == 0 {
            return Ok(Ordering::Less);
        }
        let top = sign_exp
            .checked_add(i32::from(bits) - 1)
            .ok_or(NumericFailure::Invariant)?;
        if top != 16_383 {
            return Ok(top.cmp(&16_383));
        }
        // O = (2^65 - 1) * 2^16319.
        for position in (16_319i32..=16_383i32).rev() {
            let rel = position
                .checked_sub(sign_exp)
                .ok_or(NumericFailure::Invariant)?;
            let bit = rel >= 0 && rel < i32::from(bits) && self.bit17(src, rel as u16)?;
            if !bit {
                return Ok(Ordering::Less);
            }
        }
        let mut position = 16_318;
        while position >= sign_exp {
            let rel = position
                .checked_sub(sign_exp)
                .ok_or(NumericFailure::Invariant)?;
            if rel < i32::from(bits) && self.bit17(src, rel as u16)? {
                return Ok(Ordering::Greater);
            }
            if position == i32::MIN {
                break;
            }
            position -= 1;
        }
        Ok(Ordering::Equal)
    }

    pub(crate) fn binary80_round17(
        &mut self,
        src: Slot17,
        dst: Slot9,
        sign: bool,
        exact_exp: i32,
    ) -> Result<Value80, NumericFailure> {
        let bits = self.bit_len17(src)?;
        if bits == 0 {
            self.clear9(dst, sign)?;
            return self.export_binary80(dst);
        }
        if self.compare17_to_overflow(src, exact_exp)? != Ordering::Less {
            return Self::infinity(sign);
        }
        let top = exact_exp
            .checked_add(i32::from(bits) - 1)
            .ok_or(NumericFailure::Invariant)?;
        let grid = if top >= -16_382 { top - 63 } else { -16_445 };
        let cut = grid
            .checked_sub(exact_exp)
            .ok_or(NumericFailure::Invariant)?;
        if cut > i32::from(bits) {
            self.clear9(dst, sign)?;
            return self.export_binary80(dst);
        }
        if cut >= 0 {
            let cut = cut as u16;
            let retained = bits.saturating_sub(cut);
            self.copy17_bits_to9(dst, src, cut, retained, sign, grid)?;
            let guard = cut != 0 && self.bit17(src, cut - 1)?;
            let lower = cut > 1 && self.any_below17(src, cut - 1)?;
            let odd = retained != 0 && self.bit9(dst, 0)?;
            if Self::should_increment(sign, Round::NearestEven, guard, lower, odd) {
                self.increment9(dst)?;
                self.normalize9(dst, sign, grid)?;
            }
        } else {
            let shift = u16::try_from(-cut).map_err(|_| NumericFailure::Invariant)?;
            if u32::from(bits) + u32::from(shift) > 64 {
                return Err(NumericFailure::Invariant);
            }
            self.clear9(dst, sign)?;
            for bit in 0..bits {
                if self.bit17(src, bit)? {
                    self.set_bit9(dst, bit + shift)?;
                }
            }
            self.normalize9(dst, sign, grid)?;
        }
        if self.bit_len9(dst)? == 0 {
            return self.export_binary80(dst);
        }
        let result_top = self
            .desc9(dst)?
            .exp
            .checked_add(i32::from(self.bit_len9(dst)?) - 1)
            .ok_or(NumericFailure::Invariant)?;
        if result_top > 16_383 {
            return Self::infinity(sign);
        }
        self.export_binary80(dst)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn round_quotient(
        &mut self,
        quotient: Slot17,
        remainder: Slot17,
        divisor: Slot9,
        dst: Slot9,
        precision: u16,
        sign: bool,
        mode: Round,
        exponent: i32,
    ) -> Result<(), NumericFailure> {
        let qbits = self.bit_len17(quotient)?;
        if qbits == 0 || qbits > precision + 1 {
            return Err(NumericFailure::Invariant);
        }
        let remainder_nonzero = self.bit_len17(remainder)? != 0;
        let (start, count, guard, lower, result_exp) = if qbits == precision + 1 {
            (
                1,
                precision,
                self.bit17(quotient, 0)?,
                remainder_nonzero,
                exponent.checked_add(1).ok_or(NumericFailure::Invariant)?,
            )
        } else {
            let comparison = if remainder_nonzero {
                self.left_shift17_one(remainder)?;
                self.cmp17_9(remainder, divisor)?
            } else {
                Ordering::Less
            };
            (
                0,
                qbits,
                comparison != Ordering::Less,
                comparison == Ordering::Greater
                    || (comparison == Ordering::Less && remainder_nonzero),
                exponent,
            )
        };
        self.copy17_bits_to9(dst, quotient, start, count, sign, result_exp)?;
        let odd = self.bit9(dst, 0)?;
        if Self::should_increment(sign, mode, guard, lower, odd) {
            let old_exp = self.desc9(dst)?.exp;
            self.increment9(dst)?;
            self.normalize9(dst, sign, old_exp)?;
            if self.bit_len9(dst)? > precision {
                let old_exp = self.desc9(dst)?.exp;
                self.shift_right9_one(dst)?;
                let next = old_exp.checked_add(1).ok_or(NumericFailure::Invariant)?;
                self.normalize9(dst, sign, next)?;
            } else {
                self.normalize9(dst, sign, self.desc9(dst)?.exp)?;
            }
        }
        Ok(())
    }

    pub(crate) fn binary80_round_quotient(
        &mut self,
        quotient: Slot17,
        remainder: Slot17,
        divisor: Slot9,
        dst: Slot9,
        sign: bool,
        exponent: i32,
    ) -> Result<Value80, NumericFailure> {
        let qbits = self.bit_len17(quotient)?;
        if qbits == 0 || qbits > 65 || self.bit_len9(divisor)? == 0 {
            return Err(NumericFailure::Invariant);
        }
        let top = exponent
            .checked_add(i32::from(qbits) - 1)
            .ok_or(NumericFailure::Invariant)?;
        if top > 16_383 {
            return Self::infinity(sign);
        }
        let grid = if top >= -16_382 { top - 63 } else { -16_445 };
        let cut = grid
            .checked_sub(exponent)
            .ok_or(NumericFailure::Invariant)?;
        if cut < 0 {
            return Err(NumericFailure::Invariant);
        }
        let cut = u16::try_from(cut).map_err(|_| NumericFailure::Invariant)?;
        let retained = qbits.saturating_sub(cut);
        self.copy17_bits_to9(dst, quotient, cut, retained, sign, grid)?;

        let remainder_nonzero = self.bit_len17(remainder)? != 0;
        let (guard, lower) = if cut == 0 {
            let comparison = if remainder_nonzero {
                self.left_shift17_one(remainder)?;
                self.cmp17_9(remainder, divisor)?
            } else {
                Ordering::Less
            };
            (
                comparison != Ordering::Less,
                comparison == Ordering::Greater,
            )
        } else {
            let guard = cut <= qbits && self.bit17(quotient, cut - 1)?;
            let lower =
                (cut > 1 && self.any_below17(quotient, (cut - 1).min(qbits))?) || remainder_nonzero;
            (guard, lower)
        };
        let odd = retained != 0 && self.bit9(dst, 0)?;
        if Self::should_increment(sign, Round::NearestEven, guard, lower, odd) {
            if self.increment9(dst)? {
                return Err(NumericFailure::Invariant);
            }
            self.normalize9(dst, sign, grid)?;
        }
        if self.bit_len9(dst)? > 64 {
            let old_exp = self.desc9(dst)?.exp;
            if !self.shift_right9_one(dst)? {
                return Err(NumericFailure::Invariant);
            }
            self.normalize9(
                dst,
                sign,
                old_exp.checked_add(1).ok_or(NumericFailure::Invariant)?,
            )?;
        }
        self.export_binary80(dst)
    }
}
