//! Independent compacted samples for paired aggregate operations.

use super::numerics::Value;
use super::{
    Failure, Kind as OperationKind, decimal, failure, integer, numeric_failure, numerics,
    quiet_nan, samples,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Covariance { sample: bool },
    Pearson { sample: bool },
    DotProduct,
}

pub(super) trait Arithmetic {
    fn add(&mut self, left: Value, right: Value) -> Result<Value, Failure>;
    fn subtract(&mut self, left: Value, right: Value) -> Result<Value, Failure>;
    fn multiply(&mut self, left: Value, right: Value) -> Result<Value, Failure>;
    fn divide(&mut self, left: Value, right: Value) -> Result<Value, Failure>;
    fn sqrt(&mut self, value: Value) -> Result<Value, Failure>;
}

impl Arithmetic for numerics::Numerics {
    fn add(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
        decimal::add(left, right)
    }
    fn subtract(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
        decimal::subtract(left, right)
    }
    fn multiply(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
        decimal::multiply(left, right)
    }
    fn divide(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
        decimal::divide(left, right)
    }
    fn sqrt(&mut self, value: Value) -> Result<Value, Failure> {
        self.sqrt(value).map_err(numeric_failure)
    }
}

#[derive(Default)]
pub(super) struct PairSamples {
    left: samples::Samples,
    right: samples::Samples,
    #[cfg(test)]
    fail_left_allocation: bool,
    #[cfg(test)]
    fail_right_allocation: bool,
}

impl PairSamples {
    /// Callers append the left side before attempting the right side for each record.
    pub(super) fn push_left(&mut self, value: Value) -> Result<(), samples::Error> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_left_allocation) {
            return Err(samples::Error::Allocation);
        }
        self.left.push_growable(value)
    }

    pub(super) fn push_right(&mut self, value: Value) -> Result<(), samples::Error> {
        #[cfg(test)]
        if std::mem::take(&mut self.fail_right_allocation) {
            return Err(samples::Error::Allocation);
        }
        self.right.push_growable(value)
    }

    pub(super) fn reset(&mut self) {
        self.left.clear();
        self.right.clear();
    }

    pub(super) fn summarize(
        &self,
        kind: Kind,
        operation: OperationKind,
        left_field: u64,
        right_field: u64,
        arithmetic: &mut impl Arithmetic,
        utf8: bool,
    ) -> Result<Value, Failure> {
        let left = self.left.as_slice();
        let right = self.right.as_slice();
        if right.is_empty() {
            return Ok(quiet_nan());
        }
        if left.len() != right.len() {
            return Err(failure(
                format!(
                    "input error for operation {}: fields {left_field},{right_field} have different number of items\n",
                    super::grammar::quoted(operation.name(), utf8)
                )
                .into_bytes(),
            ));
        }
        if kind == Kind::DotProduct {
            let mut sum = integer(0);
            for (&right, &left) in right.iter().zip(left) {
                let product = arithmetic.multiply(right, left)?;
                sum = arithmetic.add(sum, product)?;
            }
            return Ok(sum);
        }
        let sample = match kind {
            Kind::Covariance { sample } | Kind::Pearson { sample } => sample,
            Kind::DotProduct => unreachable!("dot product returned above"),
        };
        let degrees = usize::from(sample);
        if right.len() == degrees {
            return Ok(quiet_nan());
        }

        let mean_right = mean(right, arithmetic)?;
        let mean_left = mean(left, arithmetic)?;
        let divisor = integer((right.len() - degrees) as u64);
        match kind {
            Kind::Covariance { .. } => {
                let mut sum = integer(0);
                for (&right, &left) in right.iter().zip(left) {
                    let right = arithmetic.subtract(right, mean_right)?;
                    let left = arithmetic.subtract(left, mean_left)?;
                    let product = arithmetic.multiply(right, left)?;
                    sum = arithmetic.add(sum, product)?;
                }
                arithmetic.divide(sum, divisor)
            }
            Kind::Pearson { .. } => {
                let mut right_squares = integer(0);
                let mut left_squares = integer(0);
                let mut cross = integer(0);
                for (&right, &left) in right.iter().zip(left) {
                    let right = arithmetic.subtract(right, mean_right)?;
                    let left = arithmetic.subtract(left, mean_left)?;
                    let square = arithmetic.multiply(right, right)?;
                    right_squares = arithmetic.add(right_squares, square)?;
                    let square = arithmetic.multiply(left, left)?;
                    left_squares = arithmetic.add(left_squares, square)?;
                    let product = arithmetic.multiply(right, left)?;
                    cross = arithmetic.add(cross, product)?;
                }
                let covariance = arithmetic.divide(cross, divisor)?;
                let right_variance = arithmetic.divide(right_squares, divisor)?;
                let sd_right = arithmetic.sqrt(right_variance)?;
                let left_variance = arithmetic.divide(left_squares, divisor)?;
                let sd_left = arithmetic.sqrt(left_variance)?;
                let product = arithmetic.multiply(sd_right, sd_left)?;
                arithmetic.divide(covariance, product)
            }
            Kind::DotProduct => unreachable!("dot product returned above"),
        }
    }

    #[cfg(test)]
    pub(super) fn fail_next_left_allocation(&mut self) {
        self.fail_left_allocation = true;
    }

    #[cfg(test)]
    pub(super) fn fail_next_right_allocation(&mut self) {
        self.fail_right_allocation = true;
    }

    #[cfg(test)]
    pub(super) fn lengths(&self) -> (usize, usize) {
        (self.left.as_slice().len(), self.right.as_slice().len())
    }

    #[cfg(test)]
    pub(super) fn capacities(&self) -> (usize, usize) {
        (self.left.capacity(), self.right.capacity())
    }
}

fn mean(values: &[Value], arithmetic: &mut impl Arithmetic) -> Result<Value, Failure> {
    let mut sum = integer(0);
    for &value in values {
        sum = arithmetic.add(sum, value)?;
    }
    arithmetic.divide(sum, integer(values.len() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::numerics::test_support::Inspect;

    fn values(rows: &[(u64, u64)]) -> PairSamples {
        let mut samples = PairSamples::default();
        for &(left, right) in rows {
            samples.push_left(integer(left)).unwrap();
            samples.push_right(integer(right)).unwrap();
        }
        samples
    }

    #[derive(Default)]
    struct Recording {
        calls: Vec<&'static str>,
        operands: Vec<(Value, Value)>,
    }
    impl Recording {
        fn result(&self) -> Value {
            integer(100 + self.calls.len() as u64)
        }
        fn binary(
            &mut self,
            name: &'static str,
            left: Value,
            right: Value,
        ) -> Result<Value, Failure> {
            self.calls.push(name);
            self.operands.push((left, right));
            Ok(self.result())
        }
    }
    impl Arithmetic for Recording {
        fn add(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
            self.binary("add", left, right)
        }
        fn subtract(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
            self.binary("subtract", left, right)
        }
        fn multiply(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
            self.binary("multiply", left, right)
        }
        fn divide(&mut self, left: Value, right: Value) -> Result<Value, Failure> {
            self.binary("divide", left, right)
        }
        fn sqrt(&mut self, _: Value) -> Result<Value, Failure> {
            self.calls.push("sqrt");
            Ok(self.result())
        }
    }

    #[test]
    fn covariance_and_pearson_preserve_every_primitive_step() {
        let samples = values(&[(1, 2), (3, 4)]);
        let means = ["add", "add", "divide", "add", "add", "divide"];
        let covariance_row = ["subtract", "subtract", "multiply", "add"];
        for sample in [false, true] {
            let mut arithmetic = Recording::default();
            samples
                .summarize(
                    Kind::Covariance { sample },
                    if sample {
                        OperationKind::Scov
                    } else {
                        OperationKind::Pcov
                    },
                    1,
                    2,
                    &mut arithmetic,
                    false,
                )
                .ok()
                .unwrap();
            let expected: Vec<_> = means
                .into_iter()
                .chain(covariance_row)
                .chain(covariance_row)
                .chain(["divide"])
                .collect();
            assert_eq!(arithmetic.calls, expected);
        }
        let pearson_row = [
            "subtract", "subtract", "multiply", "add", "multiply", "add", "multiply", "add",
        ];
        for sample in [false, true] {
            let mut arithmetic = Recording::default();
            samples
                .summarize(
                    Kind::Pearson { sample },
                    if sample {
                        OperationKind::Spearson
                    } else {
                        OperationKind::Ppearson
                    },
                    1,
                    2,
                    &mut arithmetic,
                    false,
                )
                .ok()
                .unwrap();
            let expected: Vec<_> = means
                .into_iter()
                .chain(pearson_row)
                .chain(pearson_row)
                .chain([
                    "divide", "divide", "sqrt", "divide", "sqrt", "multiply", "divide",
                ])
                .collect();
            assert_eq!(arithmetic.calls, expected);
        }
    }

    #[test]
    fn dot_product_multiplies_then_adds_each_pair_in_order() {
        let samples = values(&[(1, 2), (3, 4), (5, 6)]);
        let mut arithmetic = Recording::default();
        samples
            .summarize(
                Kind::DotProduct,
                OperationKind::Dotprod,
                1,
                2,
                &mut arithmetic,
                false,
            )
            .ok()
            .unwrap();
        assert_eq!(
            arithmetic.calls,
            ["multiply", "add", "multiply", "add", "multiply", "add"]
        );
        let operands: Vec<_> = arithmetic
            .operands
            .iter()
            .map(|(left, right)| (left.raw(), right.raw()))
            .collect();
        assert_eq!(operands[0], (integer(2).raw(), integer(1).raw()));
        assert_eq!(operands[1], (integer(0).raw(), integer(101).raw()));
        assert_eq!(operands[2], (integer(4).raw(), integer(3).raw()));
        assert_eq!(operands[3], (integer(102).raw(), integer(103).raw()));
        assert_eq!(operands[4], (integer(6).raw(), integer(5).raw()));
        assert_eq!(operands[5], (integer(104).raw(), integer(105).raw()));
    }

    #[test]
    fn empty_right_precedes_length_check_and_reverse_mismatch_fails() {
        let mut arithmetic = Recording::default();
        let mut samples = PairSamples::default();
        samples.push_left(integer(1)).unwrap();
        let value = samples
            .summarize(
                Kind::Covariance { sample: false },
                OperationKind::Pcov,
                1,
                2,
                &mut arithmetic,
                false,
            )
            .ok()
            .unwrap();
        assert_eq!(value.raw(), quiet_nan().raw());
        assert!(arithmetic.calls.is_empty());
        samples.reset();
        samples.push_right(integer(1)).unwrap();
        let error = samples
            .summarize(
                Kind::Covariance { sample: false },
                OperationKind::Pcov,
                1,
                2,
                &mut arithmetic,
                false,
            )
            .unwrap_err();
        assert_eq!(error.status, 1);
        assert_eq!(
            error.message,
            b"input error for operation 'pcov': fields 1,2 have different number of items\n"
        );
    }

    #[test]
    fn sample_singleton_returns_nan_without_arithmetic() {
        let samples = values(&[(1, 2)]);
        for (kind, operation) in [
            (Kind::Covariance { sample: true }, OperationKind::Scov),
            (Kind::Pearson { sample: true }, OperationKind::Spearson),
        ] {
            let mut arithmetic = Recording::default();
            let value = samples
                .summarize(kind, operation, 1, 2, &mut arithmetic, false)
                .ok()
                .unwrap();
            assert_eq!(value.raw(), quiet_nan().raw());
            assert!(arithmetic.calls.is_empty());
        }
    }

    #[test]
    fn reset_reuses_both_allocations_and_capacity_is_side_specific() {
        let mut samples = values(&[(1, 2), (3, 4)]);
        let capacities = samples.capacities();
        samples.reset();
        assert_eq!(samples.lengths(), (0, 0));
        assert_eq!(samples.capacities(), capacities);
        for _ in 0..samples::LIMIT {
            samples.push_left(integer(1)).unwrap();
        }
        assert!(samples.push_left(integer(1)).is_ok());
        assert_eq!(samples.lengths(), (samples::LIMIT + 1, 0));
        for _ in 0..samples::LIMIT {
            samples.push_right(integer(2)).unwrap();
        }
        assert!(samples.push_right(integer(2)).is_ok());
        assert_eq!(samples.lengths(), (samples::LIMIT + 1, samples::LIMIT + 1));
    }

    #[test]
    fn injected_allocation_failures_do_not_mutate_the_failing_side() {
        let mut samples = PairSamples::default();
        samples.fail_next_left_allocation();
        assert_eq!(
            samples.push_left(integer(1)),
            Err(samples::Error::Allocation)
        );
        assert_eq!(samples.lengths(), (0, 0));
        samples.push_left(integer(1)).unwrap();
        samples.fail_next_right_allocation();
        assert_eq!(
            samples.push_right(integer(2)),
            Err(samples::Error::Allocation)
        );
        assert_eq!(samples.lengths(), (1, 0));
    }

    #[test]
    fn real_growth_failure_preserves_independently_compacted_sides() {
        let mut samples = PairSamples::default();
        samples::FAIL_GROWTH.with(|flag| flag.set(true));
        assert_eq!(
            samples.push_left(integer(1)),
            Err(samples::Error::Allocation)
        );
        assert_eq!(samples.lengths(), (0, 0));
        samples.push_left(integer(1)).unwrap();
        samples::FAIL_GROWTH.with(|flag| flag.set(true));
        assert_eq!(
            samples.push_right(integer(2)),
            Err(samples::Error::Allocation)
        );
        assert_eq!(samples.lengths(), (1, 0));
        samples.push_right(integer(2)).unwrap();
        assert_eq!(samples.lengths(), (1, 1));
    }
}
