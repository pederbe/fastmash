//! Fixed-size weighted totals with the ordered binary80 contribution trace.
use super::{Failure, decimal, failure, integer, numerics, unsupported};
use fastmash_numeric_contract::ValueClass;

pub(super) struct Accumulator {
    weighted_total: numerics::Value,
    weight_total: numerics::Value,
}

fn finite(value: numerics::Value) -> bool {
    matches!(
        numerics::Numerics::value80(value).classify(),
        ValueClass::Zero | ValueClass::Subnormal | ValueClass::Normal
    )
}

fn zero(value: numerics::Value) -> bool {
    numerics::Numerics::value80(value).classify() == ValueClass::Zero
}

fn range(stage: &str) -> Failure {
    unsupported(&format!(
        "wmean {stage} exceeds the supported numerical range"
    ))
}

impl Accumulator {
    pub(super) fn new() -> Self {
        Self {
            weighted_total: integer(0),
            weight_total: integer(0),
        }
    }

    /// Both Fields have already been converted, value first. Validate the
    /// supplied weight even when its partner is Missing, then omit the pair.
    pub(super) fn push(
        &mut self,
        value: Option<numerics::Value>,
        weight: Option<numerics::Value>,
        weight_field: u64,
        line: u64,
    ) -> Result<(), Failure> {
        if let Some(weight) = weight
            && (!finite(weight)
                || numerics::compare(weight, integer(0)) == numerics::Comparison::Less)
        {
            return Err(failure(format!("invalid wmean contribution weight in line {line} field {weight_field}: expected a finite nonnegative weight\n").into_bytes()));
        }
        let (Some(value), Some(weight)) = (value, weight) else {
            return Ok(());
        };
        if zero(weight) {
            return Ok(());
        }

        let product = decimal::multiply(weight, value)?;
        if finite(value) {
            if !finite(product) {
                return Err(range("product overflow"));
            }
            if !zero(value) && zero(product) {
                return Err(range("nonzero product rounded to zero"));
            }
        }
        let total = decimal::add(self.weighted_total, product)?;
        if finite(self.weighted_total) && finite(product) && !finite(total) {
            return Err(range("weighted total overflow"));
        }
        let weights = decimal::add(self.weight_total, weight)?;
        if !finite(weights) {
            return Err(range("weight total overflow"));
        }
        self.weighted_total = total;
        self.weight_total = weights;
        Ok(())
    }

    pub(super) fn result(
        &self,
        value_field: u64,
        weight_field: u64,
    ) -> Result<numerics::Value, Failure> {
        if zero(self.weight_total) {
            return Err(failure(format!("wmean fields {value_field}:{weight_field} have no positive retained contribution weight\n").into_bytes()));
        }
        let result = decimal::divide(self.weighted_total, self.weight_total)?;
        if finite(self.weighted_total) && !finite(result) {
            return Err(range("final division overflow"));
        }
        Ok(result)
    }
}
