//! Original-order samples shared only by variance and standard deviation.
use super::{Failure, decimal, integer, numerics, quiet_nan, samples};

#[derive(Default)]
pub(super) struct Dispersion {
    samples: samples::Samples,
    deviation_sum: Option<numerics::Value>,
}
impl Dispersion {
    pub(super) fn push(&mut self, value: numerics::Value) -> Result<(), samples::Error> {
        self.samples.push_growable(value)?;
        self.deviation_sum = None;
        Ok(())
    }
    pub(super) fn clear(&mut self) {
        self.samples.clear();
        self.deviation_sum = None;
    }
    pub(super) fn variance(&mut self, sample: bool) -> Result<numerics::Value, Failure> {
        let values = self.samples.as_slice();
        let n = values.len() as u64;
        let df = u64::from(sample);
        if n <= df {
            return Ok(quiet_nan());
        }
        let sum = if let Some(sum) = self.deviation_sum {
            sum
        } else {
            let mut total = integer(0);
            for &value in values {
                total = decimal::add(total, value)?;
            }
            let mean = decimal::mean(total, n)?;
            let mut sum = integer(0);
            for &value in values {
                let delta = decimal::subtract(value, mean)?;
                sum = decimal::add(sum, decimal::multiply(delta, delta)?)?;
            }
            self.deviation_sum = Some(sum);
            sum
        };
        decimal::mean(sum, n - df)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastmash_numeric_contract::Raw80;
    fn value(exponent: u16, significand: u64) -> numerics::Value {
        numerics::canonical(Raw80::new(exponent, significand))
    }
    fn assert_raw(actual: numerics::Value, expected: Raw80) {
        assert_eq!(numerics::Numerics::value80(actual).raw(), expected);
    }
    #[test]
    fn exact_power_scales_cache_and_reset() {
        let mut data = Dispersion::default();
        // [0, 2^k] has population variance 2^(2k-2), sample variance 2^(2k-1).
        for k in [-100, 0, 100] {
            data.push(integer(0)).unwrap();
            data.push(value((16383 + k) as u16, 1 << 63)).unwrap();
            let p = data.variance(false).ok().unwrap();
            assert_raw(p, Raw80::new((16383 + 2 * k - 2) as u16, 1 << 63));
            assert!(data.deviation_sum.is_some());
            assert_raw(
                data.variance(true).ok().unwrap(),
                Raw80::new((16383 + 2 * k - 1) as u16, 1 << 63),
            );
            let capacity = data.samples.capacity();
            data.clear();
            assert!(data.deviation_sum.is_none());
            assert!(data.samples.as_slice().is_empty());
            assert_eq!(data.samples.capacity(), capacity);
        }
        data.push(integer(4)).unwrap();
        assert!(matches!(
            numerics::Numerics::value80(data.variance(true).ok().unwrap()).classify(),
            fastmash_numeric_contract::ValueClass::Nan { .. }
        ));
        assert_raw(data.variance(false).ok().unwrap(), Raw80::new(0, 0));
        data.push(integer(6)).unwrap();
        assert!(data.deviation_sum.is_none());
        assert_raw(
            data.variance(false).ok().unwrap(),
            Raw80::new(0x3fff, 1 << 63),
        );
    }
}
