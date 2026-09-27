use crate::NumericFailure;
use crate::arena::{Arena, Desc, Slot9, Slot17};
use std::cmp::Ordering;

impl Arena {
    pub(crate) fn assign9(
        &mut self,
        slot: Slot9,
        value: u64,
        sign: bool,
        exp: i32,
    ) -> Result<(), NumericFailure> {
        self.clear9(slot, sign)?;
        if value == 0 {
            return Ok(());
        }
        self.set_limb9(slot, 0, value)?;
        self.set_desc9(slot, Desc { sign, len: 1, exp })
    }

    pub(crate) fn copy9(&mut self, dst: Slot9, src: Slot9) -> Result<(), NumericFailure> {
        if dst == src {
            return Ok(());
        }
        let desc = self.desc9(src)?;
        self.clear9(dst, desc.sign)?;
        for index in 0..usize::from(desc.len) {
            self.set_limb9(dst, index, self.limb9(src, index)?)?;
        }
        self.set_desc9(dst, desc)
    }

    pub(crate) fn bit_len9(&self, slot: Slot9) -> Result<u16, NumericFailure> {
        let desc = self.desc9(slot)?;
        if desc.len == 0 {
            return Ok(0);
        }
        let high = self.limb9(slot, usize::from(desc.len) - 1)?;
        let lower = u16::from(desc.len - 1)
            .checked_mul(64)
            .ok_or(NumericFailure::Invariant)?;
        lower
            .checked_add((64 - high.leading_zeros()) as u16)
            .ok_or(NumericFailure::Invariant)
    }

    pub(crate) fn bit_len17(&self, slot: Slot17) -> Result<u16, NumericFailure> {
        let desc = self.desc17(slot)?;
        if desc.len == 0 {
            return Ok(0);
        }
        let high = self.limb17(slot, usize::from(desc.len) - 1)?;
        let lower = u16::from(desc.len - 1)
            .checked_mul(64)
            .ok_or(NumericFailure::Invariant)?;
        lower
            .checked_add((64 - high.leading_zeros()) as u16)
            .ok_or(NumericFailure::Invariant)
    }

    pub(crate) fn bit9(&self, slot: Slot9, bit: u16) -> Result<bool, NumericFailure> {
        if bit >= 576 {
            return Err(NumericFailure::Invariant);
        }
        Ok((self.limb9(slot, usize::from(bit / 64))? >> (bit % 64)) & 1 != 0)
    }

    pub(crate) fn bit17(&self, slot: Slot17, bit: u16) -> Result<bool, NumericFailure> {
        if bit >= 1088 {
            return Err(NumericFailure::Invariant);
        }
        Ok((self.limb17(slot, usize::from(bit / 64))? >> (bit % 64)) & 1 != 0)
    }

    pub(crate) fn set_bit9(&mut self, slot: Slot9, bit: u16) -> Result<(), NumericFailure> {
        if bit >= 576 {
            return Err(NumericFailure::Invariant);
        }
        let index = usize::from(bit / 64);
        let value = self.limb9(slot, index)? | (1u64 << (bit % 64));
        self.set_limb9(slot, index, value)
    }

    pub(crate) fn set_bit17(&mut self, slot: Slot17, bit: u16) -> Result<(), NumericFailure> {
        if bit >= 1088 {
            return Err(NumericFailure::Invariant);
        }
        let index = usize::from(bit / 64);
        let value = self.limb17(slot, index)? | (1u64 << (bit % 64));
        self.set_limb17(slot, index, value)
    }

    pub(crate) fn normalize9(
        &mut self,
        slot: Slot9,
        sign: bool,
        exp: i32,
    ) -> Result<(), NumericFailure> {
        let mut len = 9usize;
        while len != 0 && self.limb9(slot, len - 1)? == 0 {
            len -= 1;
        }
        self.set_desc9(
            slot,
            if len == 0 {
                Desc::zero(sign)
            } else {
                Desc {
                    sign,
                    len: len as u8,
                    exp,
                }
            },
        )
    }

    pub(crate) fn normalize17(
        &mut self,
        slot: Slot17,
        sign: bool,
        exp: i32,
    ) -> Result<(), NumericFailure> {
        let mut len = 17usize;
        while len != 0 && self.limb17(slot, len - 1)? == 0 {
            len -= 1;
        }
        self.set_desc17(
            slot,
            if len == 0 {
                Desc::zero(sign)
            } else {
                Desc {
                    sign,
                    len: len as u8,
                    exp,
                }
            },
        )
    }

    pub(crate) fn cmp17_9(&self, left: Slot17, right: Slot9) -> Result<Ordering, NumericFailure> {
        let ll = self.desc17(left)?.len;
        let rl = self.desc9(right)?.len;
        if ll != rl {
            return Ok(ll.cmp(&rl));
        }
        for index in (0..usize::from(ll)).rev() {
            let cmp = self.limb17(left, index)?.cmp(&self.limb9(right, index)?);
            if cmp != Ordering::Equal {
                return Ok(cmp);
            }
        }
        Ok(Ordering::Equal)
    }

    pub(crate) fn copy17(&mut self, dst: Slot17, src: Slot17) -> Result<(), NumericFailure> {
        if dst == src {
            return Ok(());
        }
        let desc = self.desc17(src)?;
        self.clear17(dst)?;
        for index in 0..usize::from(desc.len) {
            self.set_limb17(dst, index, self.limb17(src, index)?)?;
        }
        self.set_desc17(dst, desc)
    }

    pub(crate) fn cmp17_signed(
        &mut self,
        left: Slot17,
        right: Slot17,
    ) -> Result<Ordering, NumericFailure> {
        if self.desc17(left)?.len == 0 && self.desc17(right)?.len == 0 {
            return Ok(Ordering::Equal);
        }
        if self.desc17(left)?.sign != self.desc17(right)?.sign {
            return Ok(if self.desc17(left)?.sign {
                Ordering::Less
            } else {
                Ordering::Greater
            });
        }
        let lt = self
            .desc17(left)?
            .exp
            .checked_add(i32::from(self.bit_len17(left)?) - 1)
            .ok_or(NumericFailure::Invariant)?;
        let rt = self
            .desc17(right)?
            .exp
            .checked_add(i32::from(self.bit_len17(right)?) - 1)
            .ok_or(NumericFailure::Invariant)?;
        self.set_scalar(
            4,
            match lt.cmp(&rt) {
                Ordering::Less => 0,
                Ordering::Equal => 1,
                Ordering::Greater => 2,
            },
        )?;
        if self.scalar(4)? == 1 {
            self.set_scalar(3, lt as u32 as u64)?;
            loop {
                let position = self.scalar(3)? as u32 as i32;
                let lrel = position
                    .checked_sub(self.desc17(left)?.exp)
                    .ok_or(NumericFailure::Invariant)?;
                let rrel = position
                    .checked_sub(self.desc17(right)?.exp)
                    .ok_or(NumericFailure::Invariant)?;
                let lbit = lrel >= 0
                    && lrel < i32::from(self.bit_len17(left)?)
                    && self.bit17(left, lrel as u16)?;
                let rbit = rrel >= 0
                    && rrel < i32::from(self.bit_len17(right)?)
                    && self.bit17(right, rrel as u16)?;
                if lbit != rbit {
                    self.set_scalar(4, if lbit { 2 } else { 0 })?;
                    break;
                }
                if position == self.desc17(left)?.exp.min(self.desc17(right)?.exp) {
                    break;
                }
                self.set_scalar(
                    3,
                    position.checked_sub(1).ok_or(NumericFailure::Invariant)? as u32 as u64,
                )?;
            }
        }
        let comparison = match self.scalar(4)? {
            0 => Ordering::Less,
            1 => Ordering::Equal,
            2 => Ordering::Greater,
            _ => return Err(NumericFailure::Invariant),
        };
        Ok(if self.desc17(left)?.sign {
            comparison.reverse()
        } else {
            comparison
        })
    }

    pub(crate) fn shifted_cmp9(
        &self,
        left: Slot9,
        left_exp: i32,
        right: Slot9,
        right_exp: i32,
    ) -> Result<Ordering, NumericFailure> {
        let left_bits = self.bit_len9(left)?;
        let right_bits = self.bit_len9(right)?;
        if left_bits == 0 || right_bits == 0 {
            return Ok(left_bits.cmp(&right_bits));
        }
        let left_top = left_exp
            .checked_add(i32::from(left_bits) - 1)
            .ok_or(NumericFailure::Invariant)?;
        let right_top = right_exp
            .checked_add(i32::from(right_bits) - 1)
            .ok_or(NumericFailure::Invariant)?;
        if left_top != right_top {
            return Ok(left_top.cmp(&right_top));
        }
        let bottom = left_exp.min(right_exp);
        let mut position = left_top;
        loop {
            let left_bit = self.virtual_bit9(left, left_exp, position)?;
            let right_bit = self.virtual_bit9(right, right_exp, position)?;
            if left_bit != right_bit {
                return Ok(left_bit.cmp(&right_bit));
            }
            if position == bottom {
                break;
            }
            position = position.checked_sub(1).ok_or(NumericFailure::Invariant)?;
        }
        Ok(Ordering::Equal)
    }

    pub(crate) fn virtual_bit9(
        &self,
        slot: Slot9,
        exp: i32,
        position: i32,
    ) -> Result<bool, NumericFailure> {
        let Some(relative) = position.checked_sub(exp) else {
            return Err(NumericFailure::Invariant);
        };
        if relative < 0 {
            return Ok(false);
        }
        let relative = u16::try_from(relative).map_err(|_| NumericFailure::Invariant)?;
        if relative >= self.bit_len9(slot)? {
            return Ok(false);
        }
        self.bit9(slot, relative)
    }

    pub(crate) fn any_below9(&self, slot: Slot9, cut: u16) -> Result<bool, NumericFailure> {
        if cut > 576 {
            return Err(NumericFailure::Invariant);
        }
        let full = usize::from(cut / 64);
        for index in 0..full {
            if self.limb9(slot, index)? != 0 {
                return Ok(true);
            }
        }
        let rem = cut % 64;
        if rem != 0 {
            let mask = (1u64 << rem) - 1;
            if self.limb9(slot, full)? & mask != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn any_below17(&self, slot: Slot17, cut: u16) -> Result<bool, NumericFailure> {
        if cut > 1088 {
            return Err(NumericFailure::Invariant);
        }
        let full = usize::from(cut / 64);
        for index in 0..full {
            if self.limb17(slot, index)? != 0 {
                return Ok(true);
            }
        }
        let rem = cut % 64;
        if rem != 0 {
            let mask = (1u64 << rem) - 1;
            if self.limb17(slot, full)? & mask != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(crate) fn shift_right9_one(&mut self, slot: Slot9) -> Result<bool, NumericFailure> {
        let discarded = self.limb9(slot, 0)? & 1 != 0;
        self.set_scalar(5, u64::from(discarded))?;
        self.set_scalar(4, 0)?;
        for index in (0..9).rev() {
            let old = self.limb9(slot, index)?;
            self.set_limb9(slot, index, (old >> 1) | self.scalar(4)?)?;
            self.set_scalar(4, old << 63)?;
        }
        Ok(self.scalar(5)? != 0)
    }

    pub(crate) fn shift_left9_one(&mut self, slot: Slot9) -> Result<(), NumericFailure> {
        self.set_scalar(4, 0)?;
        for index in 0..9 {
            let old = self.limb9(slot, index)?;
            self.set_limb9(slot, index, (old << 1) | self.scalar(4)?)?;
            self.set_scalar(4, old >> 63)?;
        }
        if self.scalar(4)? != 0 {
            return Err(NumericFailure::Invariant);
        }
        Ok(())
    }

    pub(crate) fn increment9(&mut self, slot: Slot9) -> Result<bool, NumericFailure> {
        self.set_scalar(4, 1)?;
        for index in 0..9 {
            let (next, carry) = self.limb9(slot, index)?.overflowing_add(1);
            self.set_limb9(slot, index, next)?;
            self.set_scalar(4, u64::from(carry))?;
            if !carry {
                return Ok(false);
            }
        }
        Ok(true)
    }

    pub(crate) fn decrement9(&mut self, slot: Slot9) -> Result<(), NumericFailure> {
        if self.bit_len9(slot)? == 0 {
            return Err(NumericFailure::Invariant);
        }
        self.set_scalar(4, 1)?;
        for index in 0..9 {
            let old = self.limb9(slot, index)?;
            self.set_limb9(slot, index, old.wrapping_sub(1))?;
            self.set_scalar(4, u64::from(old == 0))?;
            if old != 0 {
                return Ok(());
            }
        }
        Err(NumericFailure::Invariant)
    }

    pub(crate) fn shift_left9(&mut self, slot: Slot9, count: u16) -> Result<(), NumericFailure> {
        for _ in 0..count {
            self.shift_left9_one(slot)?;
        }
        Ok(())
    }

    pub(crate) fn shift_copy9_to17(
        &mut self,
        dst: Slot17,
        src: Slot9,
        shift: u16,
    ) -> Result<(), NumericFailure> {
        let bits = self.bit_len9(src)?;
        if u32::from(bits) + u32::from(shift) > 1088 {
            return Err(NumericFailure::Invariant);
        }
        self.clear17(dst)?;
        let limb_shift = usize::from(shift / 64);
        let sub = shift % 64;
        let len = usize::from(self.desc9(src)?.len);
        for index in 0..len {
            let value = self.limb9(src, index)?;
            let target = index
                .checked_add(limb_shift)
                .ok_or(NumericFailure::Invariant)?;
            self.set_limb17(dst, target, self.limb17(dst, target)? | (value << sub))?;
            if sub != 0 && value >> (64 - sub) != 0 {
                let high = target.checked_add(1).ok_or(NumericFailure::Invariant)?;
                self.set_limb17(dst, high, self.limb17(dst, high)? | (value >> (64 - sub)))?;
            }
        }
        self.normalize17(dst, false, 0)
    }

    pub(crate) fn left_shift17_one(&mut self, slot: Slot17) -> Result<(), NumericFailure> {
        self.set_scalar(4, 0)?;
        for index in 0..17 {
            let old = self.limb17(slot, index)?;
            self.set_limb17(slot, index, (old << 1) | self.scalar(4)?)?;
            self.set_scalar(4, old >> 63)?;
        }
        if self.scalar(4)? != 0 {
            return Err(NumericFailure::Invariant);
        }
        self.normalize17(slot, false, 0)
    }

    pub(crate) fn sub17_9_in_place(
        &mut self,
        dst: Slot17,
        right: Slot9,
    ) -> Result<(), NumericFailure> {
        if self.cmp17_9(dst, right)? == Ordering::Less {
            return Err(NumericFailure::Invariant);
        }
        self.set_scalar(4, 0)?;
        for index in 0..17 {
            let left = self.limb17(dst, index)?;
            let rhs = if index < 9 {
                self.limb9(right, index)?
            } else {
                0
            };
            let (first, b1) = left.overflowing_sub(rhs);
            let (next, b2) = first.overflowing_sub(self.scalar(4)?);
            self.set_limb17(dst, index, next)?;
            self.set_scalar(4, u64::from(b1 || b2))?;
        }
        if self.scalar(4)? != 0 {
            return Err(NumericFailure::Invariant);
        }
        self.normalize17(dst, false, 0)
    }

    pub(crate) fn mul_9x9(
        &mut self,
        dst: Slot17,
        left: Slot9,
        right: Slot9,
        precision: u16,
    ) -> Result<(), NumericFailure> {
        if self.bit_len9(left)? > precision || self.bit_len9(right)? > precision || precision > 544
        {
            return Err(NumericFailure::Invariant);
        }
        self.clear17(dst)?;
        let ll = usize::from(self.desc9(left)?.len);
        let rl = usize::from(self.desc9(right)?.len);
        if ll == 0 || rl == 0 {
            return Ok(());
        }
        let pairs = ll.checked_mul(rl).ok_or(NumericFailure::Invariant)?;
        self.set_scalar(3, 0)?;
        while usize::try_from(self.scalar(3)?).map_err(|_| NumericFailure::Invariant)? < pairs {
            let cursor = usize::try_from(self.scalar(3)?).map_err(|_| NumericFailure::Invariant)?;
            let i = cursor / rl;
            let j = cursor % rl;
            if j == 0 {
                self.set_scalar(5, 0)?;
            }
            let k = i.checked_add(j).ok_or(NumericFailure::Invariant)?;
            self.set_scalar(4, self.limb17(dst, k)?)?;
            let total = u128::from(self.limb9(left, i)?) * u128::from(self.limb9(right, j)?)
                + u128::from(self.scalar(4)?)
                + u128::from(self.scalar(5)?);
            self.set_limb17(dst, k, total as u64)?;
            self.set_scalar(5, (total >> 64) as u64)?;
            if j + 1 == rl {
                let carry_index = i.checked_add(rl).ok_or(NumericFailure::Invariant)?;
                if carry_index < 17 {
                    self.set_limb17(dst, carry_index, self.scalar(5)?)?;
                } else if self.scalar(5)? != 0 {
                    return Err(NumericFailure::Invariant);
                }
            }
            self.set_scalar(
                3,
                self.scalar(3)?
                    .checked_add(1)
                    .ok_or(NumericFailure::Invariant)?,
            )?;
        }
        self.normalize17(
            dst,
            false,
            self.desc9(left)?
                .exp
                .checked_add(self.desc9(right)?.exp)
                .ok_or(NumericFailure::Invariant)?,
        )
    }

    pub(crate) fn div_rem_17_by_9(
        &mut self,
        numerator: Slot17,
        divisor: Slot9,
        quotient: Slot17,
        remainder: Slot17,
    ) -> Result<(), NumericFailure> {
        if numerator == quotient
            || numerator == remainder
            || quotient == remainder
            || self.bit_len9(divisor)? == 0
        {
            return Err(NumericFailure::Invariant);
        }
        self.clear17(quotient)?;
        self.clear17(remainder)?;
        let bits = self.bit_len17(numerator)?;
        self.set_scalar(3, u64::from(bits))?;
        while self.scalar(3)? != 0 {
            self.set_scalar(
                3,
                self.scalar(3)?
                    .checked_sub(1)
                    .ok_or(NumericFailure::Invariant)?,
            )?;
            let position = u16::try_from(self.scalar(3)?).map_err(|_| NumericFailure::Invariant)?;
            self.left_shift17_one(remainder)?;
            if self.bit17(numerator, position)? {
                self.set_bit17(remainder, 0)?;
                self.normalize17(remainder, false, 0)?;
            }
            if self.cmp17_9(remainder, divisor)? != Ordering::Less {
                self.sub17_9_in_place(remainder, divisor)?;
                self.set_bit17(quotient, position)?;
                self.normalize17(quotient, false, 0)?;
            }
        }
        Ok(())
    }
}
