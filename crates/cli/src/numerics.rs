//! Invocation-owned portable arithmetic and checked command values.
use fastmash_numeric_contract::{Raw80, Value80};
pub(super) use fastmash_portable_numerics::{
    Comparison, NumericFailure, clear_sign, compare, compare_magnitude, sample_order,
};
pub(super) type Value = fastmash_portable_numerics::ArithmeticValue;

/// Documented CLI rule: larger quiet significand, then positive sign on ties.
/// Keep historical arithmetic kernels and comparison/unary rules separate.
pub(super) fn binary_nan(left: Value, right: Value) -> Option<Value> {
    use fastmash_numeric_contract::ValueClass;
    let a = Numerics::value80(left);
    let b = Numerics::value80(right);
    match (
        matches!(a.classify(), ValueClass::Nan { .. }),
        matches!(b.classify(), ValueClass::Nan { .. }),
    ) {
        (true, true) => Some(
            if (a.significand(), !a.is_negative()) >= (b.significand(), !b.is_negative()) {
                left
            } else {
                right
            },
        ),
        (true, false) => Some(left),
        (false, true) => Some(right),
        (false, false) => None,
    }
}

/// The optional numerics a Command's Operations need before input is read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Requirements {
    pub square_root: bool,
    pub mean_math: bool,
}
impl std::ops::BitOr for Requirements {
    type Output = Self;
    fn bitor(self, other: Self) -> Self {
        Self {
            square_root: self.square_root || other.square_root,
            mean_math: self.mean_math || other.mean_math,
        }
    }
}

pub(super) struct Numerics {
    session: Option<fastmash_portable_numerics::PortableNumerics>,
    square_root: Option<fastmash_portable_numerics::SquareRoot>,
    mean_math: Option<super::mean_math::Transcendentals>,
}
fn admit_result(value: Value80) -> Result<Value, NumericFailure> {
    Value::try_from(value).map_err(|_| NumericFailure::Invariant)
}
/// Only reviewed canonical constants may use this constructor.
pub(super) fn canonical(raw: Raw80) -> Value {
    Numerics::admit(Value80::from_raw(raw).expect("canonical constant"))
        .expect("constant is not a signaling NaN")
}
impl Numerics {
    #[cfg(test)]
    pub(super) fn has_session(&self) -> bool {
        self.session.is_some()
    }
    #[cfg(test)]
    pub(super) fn identity(&self) -> *const fastmash_portable_numerics::PortableNumerics {
        self.session
            .as_ref()
            .map_or(std::ptr::null(), |session| session as *const _)
    }

    #[cfg(test)]
    pub(super) fn new(needs_session: bool) -> Result<Self, NumericFailure> {
        Self::with_requirements(needs_session, false)
    }
    /// Numerics providing exactly `needs`, without the wider session.
    pub(super) fn with(needs: Requirements) -> Result<Self, NumericFailure> {
        Self::with_requirements(false, needs.square_root)?.with_mean_math(needs.mean_math)
    }
    pub(super) fn with_requirements(
        needs_session: bool,
        needs_sqrt: bool,
    ) -> Result<Self, NumericFailure> {
        Ok(Self {
            mean_math: None,
            session: needs_session
                .then(fastmash_portable_numerics::PortableNumerics::new)
                .transpose()?,
            square_root: (needs_sqrt && !needs_session)
                .then(fastmash_portable_numerics::SquareRoot::new)
                .transpose()?,
        })
    }
    pub(super) fn admit(value: Value80) -> Result<Value, NumericFailure> {
        admit_result(value)
    }
    pub(super) fn with_mean_math(mut self, needed: bool) -> Result<Self, NumericFailure> {
        self.mean_math = needed
            .then(super::mean_math::Transcendentals::new)
            .transpose()?;
        Ok(self)
    }
    pub(super) fn mean_log(&mut self, value: Value) -> Result<Value, NumericFailure> {
        self.mean_math
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .log(value)
    }
    /// Normality supplies exponents above -64 and at most zero, so exp's
    /// result stays normal. A library failure never activates the old engine.
    pub(super) fn normality_exp(&mut self, value: Value) -> Result<Value, NumericFailure> {
        self.mean_math
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .exp(value)?
            .ok_or(NumericFailure::Invariant)
    }
    pub(super) fn mean_exp(&mut self, value: Value) -> Result<Value, NumericFailure> {
        if let Some(result) = self
            .mean_math
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .exp(value)?
        {
            return Ok(result);
        }
        if self.session.is_none() {
            #[cfg(test)]
            if FAIL_MEAN_FALLBACK.with(|flag| flag.replace(false)) {
                return Err(NumericFailure::Allocation);
            }
            self.session = Some(fastmash_portable_numerics::PortableNumerics::new()?);
            // The full owner now provides sqrt too; do not retain two arenas.
            self.square_root = None;
        }
        self.exp(value)
    }
    pub(super) fn promote_u64(value: u64) -> Value {
        Self::admit(fastmash_portable_numerics::promote_u64(value))
            .expect("u64 promotion is canonical")
    }
    pub(super) fn value80(value: Value) -> Value80 {
        value.into()
    }
    #[cfg(test)]
    pub(super) fn add(&mut self, left: Value, right: Value) -> Result<Value, NumericFailure> {
        if let Some(nan) = binary_nan(left, right) {
            return Ok(nan);
        }
        let result = self
            .session
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .add(left, right)?;
        admit_result(result)
    }
    #[cfg(test)]
    pub(super) fn subtract(&mut self, left: Value, right: Value) -> Result<Value, NumericFailure> {
        if let Some(nan) = binary_nan(left, right) {
            return Ok(nan);
        }
        let result = self
            .session
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .subtract(left, right)?;
        admit_result(result)
    }
    #[cfg(test)]
    pub(super) fn multiply(&mut self, left: Value, right: Value) -> Result<Value, NumericFailure> {
        if let Some(nan) = binary_nan(left, right) {
            return Ok(nan);
        }
        let result = self
            .session
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .multiply(left, right)?;
        admit_result(result)
    }
    #[cfg(test)]
    pub(super) fn divide(&mut self, left: Value, right: Value) -> Result<Value, NumericFailure> {
        if let Some(nan) = binary_nan(left, right) {
            return Ok(nan);
        }
        let result = self
            .session
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .divide(left, right)?;
        admit_result(result)
    }
    pub(super) fn sqrt(&mut self, value: Value) -> Result<Value, NumericFailure> {
        let result = if let Some(session) = &mut self.session {
            session.sqrt(value)?
        } else {
            self.square_root
                .as_mut()
                .ok_or(NumericFailure::Invariant)?
                .sqrt(value)?
        };
        admit_result(result)
    }
    pub(super) fn exp(&mut self, value: Value) -> Result<Value, NumericFailure> {
        let result = self
            .session
            .as_mut()
            .ok_or(NumericFailure::Invariant)?
            .exp(value)?;
        admit_result(result)
    }
}
#[cfg(test)]
thread_local! {
    static FAIL_MEAN_FALLBACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
#[cfg(test)]
mod tests {
    use super::*;
    use fastmash_numeric_contract::Raw80;

    #[test]
    fn nan_selection_matches_both_arithmetic_routes() {
        let mut session = Numerics::new(true).unwrap();
        for (a, b, expected) in [
            ((false, 1), (true, 2), (true, 2)),
            ((true, 2), (false, 1), (true, 2)),
            ((true, 2), (false, 2), (false, 2)),
            ((false, 2), (true, 2), (false, 2)),
            ((true, 0), (false, 0), (false, 0)),
            ((false, 0), (true, 0), (false, 0)),
            ((true, 3), (false, 0), (true, 3)),
        ] {
            let nan = |(negative, payload): (bool, u64)| {
                canonical(Raw80::new(
                    if negative { 0xffff } else { 0x7fff },
                    0xc000_0000_0000_0000 | payload,
                ))
            };
            let (a, b, expected) = (nan(a), nan(b), Numerics::value80(nan(expected)).raw());
            for result in [
                session.add(a, b).unwrap(),
                session.subtract(a, b).unwrap(),
                session.multiply(a, b).unwrap(),
                session.divide(a, b).unwrap(),
                super::super::decimal::add(a, b).unwrap_or_else(|_| panic!("add")),
                super::super::decimal::subtract(a, b).unwrap_or_else(|_| panic!("subtract")),
                super::super::decimal::multiply(a, b).unwrap_or_else(|_| panic!("multiply")),
            ] {
                assert_eq!(Numerics::value80(result).raw(), expected);
            }
        }
        // Invalid arithmetic still creates positive canonical NaN; subsequent
        // selection follows the documented rule without changing unary propagation.
        let infinity = canonical(Raw80::new(0x7fff, 1 << 63));
        let invalid = session.subtract(infinity, infinity).unwrap();
        let negative = canonical(Raw80::new(0xffff, 0xc000_0000_0000_0000));
        assert_eq!(
            Numerics::value80(session.add(negative, invalid).unwrap()).raw(),
            Raw80::new(0x7fff, 0xc000_0000_0000_0000)
        );
        assert_eq!(
            Numerics::value80(session.sqrt(negative).unwrap()).raw(),
            Numerics::value80(negative).raw()
        );
    }

    #[test]
    fn admission_preserves_bits_and_rejects_signaling_nan() {
        for (exponent, significand) in [
            (0, 0),
            (0x8000, 0),
            (0, 1),
            (0x7fff, 1 << 63),
            (0xffff, 0xc000_0000_0000_0042),
        ] {
            let raw = Value80::from_raw(Raw80::new(exponent, significand)).unwrap();
            assert!(Numerics::value80(Numerics::admit(raw).unwrap()).same_bits(raw));
        }
        let snan = Value80::from_raw(Raw80::new(0x7fff, 0x8000_0000_0000_0001)).unwrap();
        assert_eq!(
            Numerics::admit(snan).unwrap_err(),
            NumericFailure::Invariant
        );
        let mut later = false;
        let result = admit_result(snan).map(|_| {
            later = true;
        });
        assert_eq!(result, Err(NumericFailure::Invariant));
        assert!(!later);
    }

    #[test]
    fn absent_session_refuses_every_primitive_without_late_construction() {
        let mut arithmetic = Numerics::new(false).unwrap();
        assert!(arithmetic.session.is_none());
        let one = Numerics::promote_u64(1);
        for result in [
            arithmetic.add(one, one),
            arithmetic.subtract(one, one),
            arithmetic.multiply(one, one),
            arithmetic.divide(one, one),
            arithmetic.sqrt(one),
            arithmetic.mean_log(one),
            arithmetic.exp(one),
        ] {
            assert_eq!(result.unwrap_err(), NumericFailure::Invariant);
        }
        assert!(arithmetic.session.is_none());
    }

    #[test]
    fn all_wrappers_return_admitted_values_from_one_session() {
        let mut arithmetic = Numerics::new(true).unwrap().with_mean_math(true).unwrap();
        let identity = arithmetic.session.as_ref().unwrap() as *const _;
        let one = Numerics::promote_u64(1);
        let zero = Numerics::promote_u64(0);
        for result in [
            arithmetic.add(one, zero),
            arithmetic.subtract(one, zero),
            arithmetic.multiply(one, one),
            arithmetic.divide(one, one),
            arithmetic.sqrt(one),
            arithmetic.mean_log(one),
            arithmetic.exp(zero),
        ] {
            let value = result.unwrap();
            assert!(Numerics::admit(Numerics::value80(value)).is_ok());
            assert_eq!(arithmetic.session.as_ref().unwrap() as *const _, identity);
        }
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use super::*;
    pub(crate) trait Inspect: Copy {
        fn raw(self) -> Raw80;
        fn same_bits(self, other: Self) -> bool {
            self.raw() == other.raw()
        }
        fn exponent(self) -> u16 {
            self.raw().exponent()
        }
        fn significand(self) -> u64 {
            self.raw().significand()
        }
        fn is_negative(self) -> bool;
    }
    impl Inspect for Value {
        fn raw(self) -> Raw80 {
            Numerics::value80(self).raw()
        }
        fn is_negative(self) -> bool {
            Numerics::value80(self).is_negative()
        }
    }
}

#[cfg(test)]
mod root_routing_tests {
    use super::*;
    #[test]
    fn geometric_fallback_is_lazy_reused_and_failure_preserves_root_owner() {
        let mut n = Numerics::with_requirements(false, true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        n.mean_exp(Numerics::promote_u64(1)).unwrap();
        assert!(n.session.is_none());
        let boundary = Numerics::admit(Value80::exact_u64(11356).with_sign(true)).unwrap();
        n.mean_exp(boundary).unwrap();
        assert!(n.session.is_none() && n.square_root.is_some());
        let tiny = Numerics::admit(Value80::exact_u64(16384).with_sign(true)).unwrap();
        FAIL_MEAN_FALLBACK.with(|flag| flag.set(true));
        assert_eq!(n.mean_exp(tiny).unwrap_err(), NumericFailure::Allocation);
        assert!(n.session.is_none() && n.square_root.is_some());
        assert!(n.sqrt(Numerics::promote_u64(4)).is_ok());
        let first = n.mean_exp(tiny).unwrap();
        let identity = n.identity();
        assert!(n.square_root.is_none());
        assert_eq!(
            Numerics::value80(first).raw(),
            Numerics::value80(n.mean_exp(tiny).unwrap()).raw()
        );
        assert_eq!(identity, n.identity());
        assert!(n.sqrt(Numerics::promote_u64(4)).is_ok());
        let mut mixed = Numerics::new(true).unwrap().with_mean_math(true).unwrap();
        let identity = mixed.identity();
        mixed.mean_exp(boundary).unwrap();
        mixed.mean_exp(tiny).unwrap();
        assert_eq!(identity, mixed.identity());
    }
    #[test]
    fn square_root_uses_one_restricted_or_full_owner() {
        for (full, root) in [(false, false), (false, true), (true, false), (true, true)] {
            let mut n = Numerics::with_requirements(full, root).unwrap();
            assert_eq!(n.session.is_some(), full);
            assert_eq!(n.square_root.is_some(), root && !full);
            assert_eq!(n.sqrt(Numerics::promote_u64(4)).is_ok(), full || root);
        }
    }
}
