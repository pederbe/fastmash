use crate::NumericFailure;
use crate::arena::{Arena, Desc, Slot9, Slot17};
use crate::dyadic::Round;
use fastmash_numeric_contract::{Raw80, Value80};
use std::cmp::Ordering;

#[derive(Clone, Copy)]
pub(crate) struct Interval {
    pub(crate) lo: Slot9,
    pub(crate) hi: Slot9,
}

impl Interval {
    pub(crate) const fn cache(index: u8) -> Self {
        Self {
            lo: Slot9::cache(index, 0),
            hi: Slot9::cache(index, 1),
        }
    }

    pub(crate) const fn work(register: u8) -> Self {
        Self {
            lo: Slot9::work(register, 0),
            hi: Slot9::work(register, 1),
        }
    }
}

#[cfg(test)]
pub(crate) const STAGES: [(u16, u16); 5] =
    [(160, 58), (224, 79), (288, 99), (416, 140), (544, 181)];

impl Arena {
    fn stage_limits(&self) -> Result<(u16, u16), NumericFailure> {
        match self.scalar(0)? {
            1 => Ok((160, 58)),
            2 => Ok((224, 79)),
            3 => Ok((288, 99)),
            4 => Ok((416, 140)),
            5 => Ok((544, 181)),
            _ => Err(NumericFailure::Invariant),
        }
    }

    fn copy_interval(&mut self, dst: Interval, src: Interval) -> Result<(), NumericFailure> {
        self.copy9(dst.lo, src.lo)?;
        self.copy9(dst.hi, src.hi)
    }

    fn exact_interval(&mut self, dst: Interval, value: i32) -> Result<(), NumericFailure> {
        let sign = value < 0;
        let magnitude = u64::from(value.unsigned_abs());
        self.assign9(dst.lo, magnitude, sign, 0)?;
        self.assign9(dst.hi, magnitude, sign, 0)
    }

    fn compare9_signed(&self, left: Slot9, right: Slot9) -> Result<Ordering, NumericFailure> {
        if self.desc9(left)?.len == 0 && self.desc9(right)?.len == 0 {
            return Ok(Ordering::Equal);
        }
        if self.desc9(left)?.sign != self.desc9(right)?.sign {
            return Ok(if self.desc9(left)?.sign {
                Ordering::Less
            } else {
                Ordering::Greater
            });
        }
        let mut cmp =
            self.shifted_cmp9(left, self.desc9(left)?.exp, right, self.desc9(right)?.exp)?;
        if self.desc9(left)?.sign {
            cmp = cmp.reverse();
        }
        Ok(cmp)
    }

    fn endpoint_div(
        &mut self,
        left: Slot9,
        right: Slot9,
        precision: u16,
        mode: Round,
        dst: Slot9,
    ) -> Result<(), NumericFailure> {
        if self.desc9(right)?.len == 0 {
            return Err(NumericFailure::Invariant);
        }
        if self.desc9(left)?.len == 0 {
            return self.clear9(dst, self.desc9(left)?.sign ^ self.desc9(right)?.sign);
        }
        self.exact_scope(|arena| {
            let left_bits = arena.bit_len9(left)?;
            let right_bits = arena.bit_len9(right)?;
            let shift = i32::from(precision)
                .checked_add(i32::from(right_bits))
                .and_then(|v| v.checked_sub(i32::from(left_bits)))
                .ok_or(NumericFailure::Invariant)?;
            if !(1..=i32::from(2 * precision - 1)).contains(&shift) {
                return Err(NumericFailure::Invariant);
            }
            arena.shift_copy9_to17(Slot17::X0, left, shift as u16)?;
            arena.div_rem_17_by_9(Slot17::X0, right, Slot17::X1, Slot17::X2)?;
            let exponent = arena
                .desc9(left)?
                .exp
                .checked_sub(arena.desc9(right)?.exp)
                .and_then(|v| v.checked_sub(shift))
                .ok_or(NumericFailure::Invariant)?;
            arena.round_quotient(
                Slot17::X1,
                Slot17::X2,
                right,
                dst,
                precision,
                arena.desc9(left)?.sign ^ arena.desc9(right)?.sign,
                mode,
                exponent,
            )
        })
    }

    fn iadd(
        &mut self,
        left: Interval,
        right: Interval,
        precision: u16,
        dst: Interval,
    ) -> Result<(), NumericFailure> {
        let temp = Interval::work(19);
        if left.lo == temp.lo || left.hi == temp.hi || right.lo == temp.lo || right.hi == temp.hi {
            return Err(NumericFailure::Invariant);
        }
        self.aligned_combine(
            left.lo,
            right.lo,
            false,
            precision,
            Round::Down,
            temp.lo,
            false,
        )?;
        self.aligned_combine(
            left.hi,
            right.hi,
            false,
            precision,
            Round::Up,
            temp.hi,
            false,
        )?;
        self.copy_interval(dst, temp)
    }

    fn isub(
        &mut self,
        left: Interval,
        right: Interval,
        precision: u16,
        dst: Interval,
    ) -> Result<(), NumericFailure> {
        let temp = Interval::work(19);
        if left.lo == temp.lo || left.hi == temp.hi || right.lo == temp.lo || right.hi == temp.hi {
            return Err(NumericFailure::Invariant);
        }
        self.aligned_combine(
            left.lo,
            right.hi,
            true,
            precision,
            Round::Down,
            temp.lo,
            false,
        )?;
        self.aligned_combine(
            left.hi,
            right.lo,
            true,
            precision,
            Round::Up,
            temp.hi,
            false,
        )?;
        self.copy_interval(dst, temp)
    }

    fn imul(
        &mut self,
        left: Interval,
        right: Interval,
        precision: u16,
        dst: Interval,
    ) -> Result<(), NumericFailure> {
        self.exact_scope(|arena| {
            let pairs = [
                (left.lo, right.lo),
                (left.lo, right.hi),
                (left.hi, right.lo),
                (left.hi, right.hi),
            ];
            for (index, (a, b)) in pairs.into_iter().enumerate() {
                arena.mul_9x9(Slot17::X0, a, b, precision)?;
                let mut desc = arena.desc17(Slot17::X0)?;
                desc.sign = arena.desc9(a)?.sign ^ arena.desc9(b)?.sign;
                arena.set_desc17(Slot17::X0, desc)?;
                if index == 0 {
                    arena.copy17(Slot17::X1, Slot17::X0)?;
                    arena.copy17(Slot17::X2, Slot17::X0)?;
                } else {
                    if arena.cmp17_signed(Slot17::X0, Slot17::X1)? == Ordering::Less {
                        arena.copy17(Slot17::X1, Slot17::X0)?;
                    }
                    if arena.cmp17_signed(Slot17::X0, Slot17::X2)? == Ordering::Greater {
                        arena.copy17(Slot17::X2, Slot17::X0)?;
                    }
                }
            }
            arena.round17(
                Slot17::X1,
                dst.lo,
                precision,
                arena.desc17(Slot17::X1)?.sign,
                Round::Down,
            )?;
            arena.round17(
                Slot17::X2,
                dst.hi,
                precision,
                arena.desc17(Slot17::X2)?.sign,
                Round::Up,
            )
        })
    }

    fn idiv(
        &mut self,
        left: Interval,
        right: Interval,
        precision: u16,
        dst: Interval,
    ) -> Result<(), NumericFailure> {
        let zero = Interval::work(16);
        self.exact_interval(zero, 0)?;
        let lower_zero = self.compare9_signed(right.lo, zero.lo)?;
        let upper_zero = self.compare9_signed(right.hi, zero.hi)?;
        if lower_zero != upper_zero
            || lower_zero == Ordering::Equal
            || upper_zero == Ordering::Equal
        {
            return Err(NumericFailure::Invariant);
        }
        let reciprocal = Interval::work(17);
        let one = Interval::work(16);
        self.exact_interval(one, 1)?;
        self.endpoint_div(one.lo, right.hi, precision, Round::Down, reciprocal.lo)?;
        self.endpoint_div(one.hi, right.lo, precision, Round::Up, reciprocal.hi)?;
        self.imul(left, reciprocal, precision, dst)
    }

    fn negate_interval(&mut self, src: Interval, dst: Interval) -> Result<(), NumericFailure> {
        self.copy9(dst.lo, src.hi)?;
        let lo = self.desc9(dst.lo)?;
        self.set_desc9(
            dst.lo,
            Desc {
                sign: !lo.sign,
                ..lo
            },
        )?;
        self.copy9(dst.hi, src.lo)?;
        let hi = self.desc9(dst.hi)?;
        self.set_desc9(
            dst.hi,
            Desc {
                sign: !hi.sign,
                ..hi
            },
        )
    }

    fn atanh_series(&mut self, input: Interval) -> Result<Option<Interval>, NumericFailure> {
        let (precision, terms) = self.stage_limits()?;
        let z = Interval::work(0);
        self.copy_interval(z, input)?;
        let zero = Interval::work(13);
        let one = Interval::work(12);
        self.exact_interval(zero, 0)?;
        self.exact_interval(one, 1)?;
        if self.compare9_signed(z.lo, zero.lo)? == Ordering::Equal
            && self.compare9_signed(z.hi, zero.hi)? == Ordering::Equal
        {
            return Ok(Some(z));
        }
        if self.compare9_signed(z.lo, zero.lo)? != Ordering::Greater
            || self.compare9_signed(z.hi, one.hi)? != Ordering::Less
        {
            return Ok(None);
        }
        let z2 = Interval::work(1);
        let term = Interval::work(2);
        let sum = Interval::work(3);
        let quotient = Interval::work(4);
        let denominator = Interval::work(5);
        self.imul(z, z, precision, z2)?;
        self.copy_interval(term, z)?;
        self.exact_interval(sum, 0)?;
        self.set_scalar(1, 0)?;
        while self.scalar(1)? < u64::from(terms) {
            let index = u16::try_from(self.scalar(1)?).map_err(|_| NumericFailure::Invariant)?;
            self.exact_interval(denominator, i32::from(2 * index + 1))?;
            self.idiv(term, denominator, precision, quotient)?;
            self.iadd(sum, quotient, precision, sum)?;
            self.imul(term, z2, precision, term)?;
            self.set_scalar(
                1,
                self.scalar(1)?
                    .checked_add(1)
                    .ok_or(NumericFailure::Invariant)?,
            )?;
        }
        let two = Interval::work(11);
        self.exact_interval(two, 2)?;
        let body = Interval::work(6);
        self.imul(two, sum, precision, body)?;
        let one_minus = Interval::work(7);
        self.isub(one, z2, precision, one_minus)?;
        let tail_denominator = Interval::work(8);
        self.exact_interval(denominator, i32::from(2 * terms + 1))?;
        self.imul(denominator, one_minus, precision, tail_denominator)?;
        let numerator = Interval::work(13);
        self.imul(two, term, precision, numerator)?;
        let tail = Interval::work(9);
        self.idiv(numerator, tail_denominator, precision, tail)?;
        let upper = Interval::work(10);
        self.iadd(body, tail, precision, upper)?;
        let result = Interval::work(14);
        self.copy9(result.lo, body.lo)?;
        self.copy9(result.hi, upper.hi)?;
        Ok(Some(result))
    }

    fn initialize_cache_stage(&mut self, stage: u8) -> Result<(), NumericFailure> {
        self.reset_disposable()?;
        self.set_scalar(0, u64::from(stage + 1))?;
        let (precision, _) = self.stage_limits()?;
        let one = Interval::work(0);
        let three = Interval::work(1);
        let third = Interval::work(2);
        self.exact_interval(one, 1)?;
        self.exact_interval(three, 3)?;
        self.idiv(one, three, precision, third)?;
        let ln2 = self.atanh_series(third)?.ok_or(NumericFailure::Invariant)?;
        self.copy_interval(Interval::cache(stage), ln2)
    }

    pub(crate) fn initialize_cache(&mut self) -> Result<(), NumericFailure> {
        self.initialize_cache_stage(0)?;
        self.initialize_cache_stage(1)?;
        self.initialize_cache_stage(2)?;
        self.initialize_cache_stage(3)?;
        self.initialize_cache_stage(4)?;
        self.reset_disposable()
    }

    fn adjacent(value: Value80, upward: bool) -> Result<Value80, NumericFailure> {
        let sign = value.is_negative();
        let exponent = value.exponent();
        let significand = value.significand();
        if exponent == 0x7fff {
            return Err(NumericFailure::Invariant);
        }
        let magnitude_up = upward != sign;
        let (next_exponent, next_significand) = if magnitude_up {
            if exponent == 0 {
                (
                    0,
                    significand
                        .checked_add(1)
                        .ok_or(NumericFailure::Invariant)?,
                )
            } else if significand != u64::MAX {
                (exponent, significand + 1)
            } else {
                (
                    exponent.checked_add(1).ok_or(NumericFailure::Invariant)?,
                    1 << 63,
                )
            }
        } else if exponent == 0 {
            (
                0,
                significand
                    .checked_sub(1)
                    .ok_or(NumericFailure::Invariant)?,
            )
        } else if significand != 1 << 63 {
            (exponent, significand - 1)
        } else if exponent == 1 {
            (0, (1 << 63) - 1)
        } else {
            (exponent - 1, u64::MAX)
        };
        Value80::from_raw(Raw80::new(
            next_exponent | if sign { 0x8000 } else { 0 },
            next_significand,
        ))
        .map_err(|_| NumericFailure::Invariant)
    }

    fn midpoint_boundary(
        &mut self,
        left: Value80,
        right: Value80,
        dst: Slot9,
    ) -> Result<(), NumericFailure> {
        let a = Slot9::work(18, 0);
        let b = Slot9::work(18, 1);
        self.import_value(a, left)?;
        self.import_value(b, right)?;
        self.aligned_combine(a, b, false, 65, Round::NearestEven, dst, false)?;
        let desc = self.desc9(dst)?;
        self.set_desc9(
            dst,
            Desc {
                exp: desc.exp.checked_sub(1).ok_or(NumericFailure::Invariant)?,
                ..desc
            },
        )
    }

    fn overflow_boundary(&mut self, dst: Slot9, sign: bool) -> Result<(), NumericFailure> {
        self.clear9(dst, sign)?;
        for bit in 0..65 {
            self.set_bit9(dst, bit)?;
        }
        self.normalize9(dst, sign, 16_319)
    }

    fn interval_within(
        &self,
        interval: Interval,
        lower: Option<(Slot9, bool)>,
        upper: Option<(Slot9, bool)>,
    ) -> Result<bool, NumericFailure> {
        if let Some((bound, closed)) = lower {
            let order = self.compare9_signed(interval.lo, bound)?;
            if order == Ordering::Less || (order == Ordering::Equal && !closed) {
                return Ok(false);
            }
        }
        if let Some((bound, closed)) = upper {
            let order = self.compare9_signed(interval.hi, bound)?;
            if order == Ordering::Greater || (order == Ordering::Equal && !closed) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn certificate_candidate(&self) -> Result<Value80, NumericFailure> {
        Value80::from_raw(Raw80::new(
            u16::try_from(self.scalar(1)?).map_err(|_| NumericFailure::Invariant)?,
            self.scalar(2)?,
        ))
        .map_err(|_| NumericFailure::Invariant)
    }

    fn certificate(&mut self, interval: Interval) -> Result<Option<Value80>, NumericFailure> {
        if self.desc9(interval.lo)?.sign && !self.desc9(interval.hi)?.sign {
            return Ok(None);
        }
        let same_candidate = self.exact_scope(|arena| {
            arena.shift_copy9_to17(Slot17::X0, interval.lo, 0)?;
            let desc = arena.desc9(interval.lo)?;
            let candidate =
                arena.binary80_round17(Slot17::X0, Interval::work(15).lo, desc.sign, desc.exp)?;
            arena.set_scalar(1, u64::from(candidate.sign_exponent()))?;
            arena.set_scalar(2, candidate.significand())?;
            arena.shift_copy9_to17(Slot17::X0, interval.hi, 0)?;
            let desc = arena.desc9(interval.hi)?;
            let upper_candidate =
                arena.binary80_round17(Slot17::X0, Interval::work(15).hi, desc.sign, desc.exp)?;
            Ok(
                u64::from(upper_candidate.sign_exponent()) == arena.scalar(1)?
                    && upper_candidate.significand() == arena.scalar(2)?,
            )
        })?;
        if !same_candidate {
            self.set_scalar(1, 0)?;
            self.set_scalar(2, 0)?;
            return Ok(None);
        }

        let lower = Interval::work(15).lo;
        let upper = Interval::work(15).hi;
        let contained = if self.scalar(2)? == 0 {
            self.assign9(
                lower,
                1,
                self.certificate_candidate()?.is_negative(),
                -16_446,
            )?;
            self.clear9(upper, self.certificate_candidate()?.is_negative())?;
            if self.certificate_candidate()?.is_negative() {
                self.interval_within(interval, Some((lower, true)), Some((upper, true)))?
            } else {
                self.interval_within(interval, Some((upper, true)), Some((lower, true)))?
            }
        } else if self.certificate_candidate()?.exponent() == 0x7fff {
            self.overflow_boundary(lower, self.certificate_candidate()?.is_negative())?;
            if self.certificate_candidate()?.is_negative() {
                self.interval_within(interval, None, Some((lower, true)))?
            } else {
                self.interval_within(interval, Some((lower, true)), None)?
            }
        } else {
            let even = self.scalar(2)? & 1 == 0;
            let maximum =
                self.certificate_candidate()?.exponent() == 0x7ffe && self.scalar(2)? == u64::MAX;
            if maximum && !self.certificate_candidate()?.is_negative() {
                self.midpoint_boundary(
                    Self::adjacent(self.certificate_candidate()?, false)?,
                    self.certificate_candidate()?,
                    lower,
                )?;
                self.overflow_boundary(upper, false)?;
                self.interval_within(interval, Some((lower, even)), Some((upper, false)))?
            } else if maximum {
                self.overflow_boundary(lower, true)?;
                self.midpoint_boundary(
                    self.certificate_candidate()?,
                    Self::adjacent(self.certificate_candidate()?, true)?,
                    upper,
                )?;
                self.interval_within(interval, Some((lower, false)), Some((upper, even)))?
            } else {
                self.midpoint_boundary(
                    Self::adjacent(self.certificate_candidate()?, false)?,
                    self.certificate_candidate()?,
                    lower,
                )?;
                self.midpoint_boundary(
                    self.certificate_candidate()?,
                    Self::adjacent(self.certificate_candidate()?, true)?,
                    upper,
                )?;
                self.interval_within(interval, Some((lower, even)), Some((upper, even)))?
            }
        };
        let result = if contained {
            Some(self.certificate_candidate()?)
        } else {
            None
        };
        self.set_scalar(1, 0)?;
        self.set_scalar(2, 0)?;
        Ok(result)
    }

    #[cfg(test)]
    pub(crate) fn certificate_for_test(
        &mut self,
        lo: Slot9,
        hi: Slot9,
    ) -> Result<Option<Value80>, NumericFailure> {
        self.certificate(Interval { lo, hi })
    }

    fn decompose_log_input(&mut self, input: Value80) -> Result<(), NumericFailure> {
        let ingress = Interval::work(20);
        self.import_value(ingress.lo, input)?;
        self.copy9(ingress.hi, ingress.lo)?;
        let exponent = self
            .desc9(ingress.lo)?
            .exp
            .checked_add(i32::from(self.bit_len9(ingress.lo)?) - 1)
            .ok_or(NumericFailure::Invariant)?;
        self.set_scalar(2, exponent as u32 as u64)?;
        Ok(())
    }

    fn log_stage(&mut self, input: Value80, stage: u8) -> Result<Option<Value80>, NumericFailure> {
        self.reset_disposable()?;
        self.set_scalar(0, u64::from(stage + 1))?;
        self.decompose_log_input(input)?;
        let (precision, _) = self.stage_limits()?;
        let ingress = Interval::work(20);
        self.import_value(ingress.lo, input)?;
        self.copy9(ingress.hi, ingress.lo)?;
        let m = Interval::work(0);
        self.copy_interval(m, ingress)?;
        for endpoint in [m.lo, m.hi] {
            let sign = self.desc9(endpoint)?.sign;
            let len = self.desc9(endpoint)?.len;
            self.set_desc9(
                endpoint,
                Desc {
                    exp: self
                        .desc9(endpoint)?
                        .exp
                        .checked_sub(self.scalar(2)? as u32 as i32)
                        .ok_or(NumericFailure::Invariant)?,
                    sign,
                    len,
                },
            )?;
        }
        let one = Interval::work(1);
        let numerator = Interval::work(2);
        let denominator = Interval::work(3);
        let z = Interval::work(4);
        self.exact_interval(one, 1)?;
        self.isub(m, one, precision, numerator)?;
        self.iadd(m, one, precision, denominator)?;
        self.idiv(numerator, denominator, precision, z)?;
        let Some(ln_m) = self.atanh_series(z)? else {
            return Ok(None);
        };
        let e = Interval::work(7);
        let scaled = Interval::work(8);
        let result = Interval::work(9);
        self.exact_interval(e, self.scalar(2)? as u32 as i32)?;
        self.imul(e, Interval::cache(stage), precision, scaled)?;
        self.iadd(ln_m, scaled, precision, result)?;
        self.certificate(result)
    }

    pub(crate) fn log_finite(&mut self, input: Value80) -> Result<Value80, NumericFailure> {
        if input.exponent() == 0 && input.significand() != 0 {
            return Err(NumericFailure::UnsupportedDomain);
        }
        self.decompose_log_input(input)?;
        if self.scalar(2)? == 0 && input.significand() == 1 << 63 {
            return Ok(Value80::signed_zero(false));
        }
        if let Some(value) = self.log_stage(input, 0)? {
            return Ok(value);
        }
        if let Some(value) = self.log_stage(input, 1)? {
            return Ok(value);
        }
        if let Some(value) = self.log_stage(input, 2)? {
            return Ok(value);
        }
        if let Some(value) = self.log_stage(input, 3)? {
            return Ok(value);
        }
        if let Some(value) = self.log_stage(input, 4)? {
            return Ok(value);
        }
        Err(NumericFailure::Capacity)
    }

    fn round_integer(&mut self, endpoint: Slot9) -> Result<Option<i32>, NumericFailure> {
        if self.desc9(endpoint)?.len == 0 {
            return Ok(Some(0));
        }
        let cut = self
            .desc9(endpoint)?
            .exp
            .checked_neg()
            .ok_or(NumericFailure::Invariant)?;
        let bits = self.bit_len9(endpoint)?;
        self.set_scalar(4, 0)?;
        if cut <= 0 {
            let shift = u32::try_from(-cut).map_err(|_| NumericFailure::Invariant)?;
            if u32::from(bits) + shift > 16 {
                return Ok(None);
            }
            self.set_scalar(
                4,
                u64::from(
                    (self.limb9(endpoint, 0)? as u32)
                        .checked_shl(shift)
                        .ok_or(NumericFailure::Invariant)?,
                ),
            )?;
        } else {
            let cut_u16 = u16::try_from(cut).map_err(|_| NumericFailure::Invariant)?;
            if cut_u16 < bits {
                let integer_bits = bits - cut_u16;
                if integer_bits > 16 {
                    return Ok(None);
                }
                for bit in 0..integer_bits {
                    if self.bit9(endpoint, cut_u16 + bit)? {
                        self.set_scalar(4, self.scalar(4)? | (1u64 << bit))?;
                    }
                }
            }
            let guard = cut_u16 != 0 && cut_u16 <= bits && self.bit9(endpoint, cut_u16 - 1)?;
            let lower = cut_u16 > 1 && self.any_below9(endpoint, (cut_u16 - 1).min(bits))?;
            if guard && (lower || self.scalar(4)? & 1 != 0) {
                self.set_scalar(
                    4,
                    self.scalar(4)?
                        .checked_add(1)
                        .ok_or(NumericFailure::Invariant)?,
                )?;
            }
        }
        if self.scalar(4)? > 23_640 {
            return Ok(None);
        }
        Ok(Some(if self.desc9(endpoint)?.sign {
            -(self.scalar(4)? as i32)
        } else {
            self.scalar(4)? as i32
        }))
    }

    fn exp_positive(&mut self, input: Interval) -> Result<Interval, NumericFailure> {
        let (precision, terms) = self.stage_limits()?;
        let t = Interval::work(0);
        self.copy_interval(t, input)?;
        let term = Interval::work(1);
        let sum = Interval::work(2);
        let integer = Interval::work(3);
        self.exact_interval(term, 1)?;
        self.exact_interval(sum, 0)?;
        self.set_scalar(1, 0)?;
        while self.scalar(1)? < u64::from(terms) {
            let index = u16::try_from(self.scalar(1)?).map_err(|_| NumericFailure::Invariant)?;
            self.iadd(sum, term, precision, sum)?;
            let product = Interval::work(12);
            self.imul(term, t, precision, product)?;
            self.exact_interval(integer, i32::from(index + 1))?;
            self.idiv(product, integer, precision, term)?;
            self.set_scalar(
                1,
                self.scalar(1)?
                    .checked_add(1)
                    .ok_or(NumericFailure::Invariant)?,
            )?;
        }
        let ratio = Interval::work(4);
        self.exact_interval(integer, i32::from(terms + 1))?;
        self.idiv(t, integer, precision, ratio)?;
        let one = Interval::work(13);
        self.exact_interval(one, 1)?;
        let tail_denominator = Interval::work(5);
        self.isub(one, ratio, precision, tail_denominator)?;
        let tail = Interval::work(6);
        self.idiv(term, tail_denominator, precision, tail)?;
        let upper = Interval::work(7);
        self.iadd(sum, tail, precision, upper)?;
        let result = Interval::work(8);
        self.copy9(result.lo, sum.lo)?;
        self.copy9(result.hi, upper.hi)?;
        Ok(result)
    }

    fn exp_stage(&mut self, input: Value80, stage: u8) -> Result<Option<Value80>, NumericFailure> {
        self.reset_disposable()?;
        self.set_scalar(0, u64::from(stage + 1))?;
        let (precision, _) = self.stage_limits()?;
        let x = Interval::work(0);
        self.import_value(x.lo, input)?;
        self.copy9(x.hi, x.lo)?;
        let q = Interval::work(1);
        self.idiv(x, Interval::cache(stage), precision, q)?;
        let Some(k_lo) = self.round_integer(q.lo)? else {
            return Ok(None);
        };
        self.set_scalar(2, k_lo as u32 as u64)?;
        let Some(k_hi) = self.round_integer(q.hi)? else {
            return Ok(None);
        };
        if self.scalar(2)? as u32 as i32 != k_hi {
            return Ok(None);
        }
        let k_interval = Interval::work(2);
        self.exact_interval(k_interval, self.scalar(2)? as u32 as i32)?;
        let scaled = Interval::work(3);
        self.imul(k_interval, Interval::cache(stage), precision, scaled)?;
        let reduced = Interval::work(4);
        self.isub(x, scaled, precision, reduced)?;
        let two = Interval::work(14);
        self.exact_interval(two, 2)?;
        let half = Interval::work(5);
        self.idiv(Interval::cache(stage), two, precision, half)?;
        let zero = Interval::work(15);
        self.exact_interval(zero, 0)?;
        let positive = self.compare9_signed(reduced.lo, zero.lo)? == Ordering::Greater
            && self.compare9_signed(reduced.hi, half.lo)? != Ordering::Greater;
        let negative = self.compare9_signed(reduced.hi, zero.hi)? == Ordering::Less;
        let t = Interval::work(6);
        if positive {
            self.copy_interval(t, reduced)?;
        } else if negative {
            self.negate_interval(reduced, t)?;
            if self.compare9_signed(t.hi, half.lo)? == Ordering::Greater {
                return Ok(None);
            }
        } else {
            return Ok(None);
        }
        let expanded = self.exp_positive(t)?;
        let reduced_value = Interval::work(9);
        if positive {
            self.copy_interval(reduced_value, expanded)?;
        } else {
            let one = Interval::work(13);
            self.exact_interval(one, 1)?;
            self.idiv(one, expanded, precision, reduced_value)?;
        }
        let power = Interval::work(10);
        self.assign9(power.lo, 1, false, self.scalar(2)? as u32 as i32)?;
        self.assign9(power.hi, 1, false, self.scalar(2)? as u32 as i32)?;
        let result = Interval::work(11);
        self.imul(reduced_value, power, precision, result)?;
        self.certificate(result)
    }

    pub(crate) fn exp_finite(&mut self, input: Value80) -> Result<Value80, NumericFailure> {
        if input.significand() == 0 {
            return Ok(Value80::exact_u64(1));
        }
        if let Some(value) = self.exp_stage(input, 0)? {
            return Ok(value);
        }
        if let Some(value) = self.exp_stage(input, 1)? {
            return Ok(value);
        }
        if let Some(value) = self.exp_stage(input, 2)? {
            return Ok(value);
        }
        if let Some(value) = self.exp_stage(input, 3)? {
            return Ok(value);
        }
        if let Some(value) = self.exp_stage(input, 4)? {
            return Ok(value);
        }
        Err(NumericFailure::Capacity)
    }

    pub(crate) fn add_finite(
        &mut self,
        left: Value80,
        right: Value80,
    ) -> Result<Value80, NumericFailure> {
        self.reset_disposable()?;
        let l = Slot9::work(20, 0);
        let r = Slot9::work(21, 0);
        let out = Slot9::work(23, 0);
        self.import_value(l, left)?;
        self.import_value(r, right)?;
        self.aligned_combine(l, r, false, 64, Round::NearestEven, out, true)?;
        self.export_binary80(out)
    }

    pub(crate) fn divide_finite(
        &mut self,
        left: Value80,
        right: Value80,
    ) -> Result<Value80, NumericFailure> {
        self.reset_disposable()?;
        let l = Slot9::work(20, 0);
        let r = Slot9::work(21, 0);
        let out = Slot9::work(23, 0);
        self.import_value(l, left)?;
        self.import_value(r, right)?;
        if self.desc9(r)?.len == 0 {
            return Err(NumericFailure::Invariant);
        }
        if self.desc9(l)?.len == 0 {
            self.clear9(out, self.desc9(l)?.sign ^ self.desc9(r)?.sign)?;
            return self.export_binary80(out);
        }
        let left_bits = self.bit_len9(l)?;
        let right_bits = self.bit_len9(r)?;
        let shift = 64i32
            .checked_add(i32::from(right_bits))
            .and_then(|value| value.checked_sub(i32::from(left_bits)))
            .ok_or(NumericFailure::Invariant)?;
        if !(1..=127).contains(&shift) {
            return Err(NumericFailure::Invariant);
        }
        self.shift_copy9_to17(Slot17::X0, l, shift as u16)?;
        self.div_rem_17_by_9(Slot17::X0, r, Slot17::X1, Slot17::X2)?;
        let exponent = self
            .desc9(l)?
            .exp
            .checked_sub(self.desc9(r)?.exp)
            .and_then(|value| value.checked_sub(shift))
            .ok_or(NumericFailure::Invariant)?;
        self.binary80_round_quotient(
            Slot17::X1,
            Slot17::X2,
            r,
            out,
            self.desc9(l)?.sign ^ self.desc9(r)?.sign,
            exponent,
        )
    }
}
