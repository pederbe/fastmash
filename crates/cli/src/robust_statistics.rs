//! Destructive MAD finalization, separate from immutable ordered samples.
use super::{Failure, Kind, decimal, integer, numerics, sample_failure, samples, unsupported};

fn median(values: &mut samples::Samples, kind: Kind) -> Result<numerics::Value, Failure> {
    let (upper, lower) = values
        .middle()
        .map_err(|error| sample_failure(kind, error))?
        .ok_or_else(|| unsupported("internal MAD sample state missing"))?;
    if let Some(lower) = lower {
        // Both medians within GNU MAD add lower first, unlike standalone median.
        let half = numerics::canonical(fastmash_numeric_contract::Raw80::new(0x3ffe, 1 << 63));
        decimal::multiply(decimal::add(lower, upper)?, half)
    } else {
        Ok(upper)
    }
}

/// Consume this group's original sample state. The owner caches the result and
/// clears the samples before collecting another group.
pub(super) fn deviation(
    values: &mut samples::Samples,
    kind: Kind,
) -> Result<numerics::Value, Failure> {
    let center = median(values, kind)?;
    values.try_map(|value| -> Result<_, Failure> {
        Ok(numerics::clear_sign(decimal::subtract(center, value)?))
    })?;
    median(values, kind)
}

pub(super) fn scale(deviation: numerics::Value, scaled: bool) -> Result<numerics::Value, Failure> {
    let scale = if scaled {
        // GNU's binary64 1.4826 promoted exactly, not binary80 decimal 1.4826.
        numerics::canonical(fastmash_numeric_contract::Raw80::new(
            0x3fff,
            0xbdc5_d638_8659_4800,
        ))
    } else {
        integer(1)
    };
    decimal::multiply(deviation, scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastmash_numeric_contract::Raw80;

    #[test]
    fn subnormal_deviations_and_scale_overflow_follow_binary80_rounding() {
        // [0, k*u, 2*k*u] has raw deviation k*u, where u is the least subnormal.
        // Multiplying k=1,2,3 by the GNU scale rounds to 1,3,4 units respectively.
        for (units, scaled_units) in [(1, 1), (2, 3), (3, 4)] {
            let mut samples = samples::Samples::default();
            for value in [0, units, 2 * units] {
                samples
                    .push_growable(numerics::canonical(Raw80::new(0, value)))
                    .unwrap();
            }
            let raw = deviation(&mut samples, Kind::Madraw).ok().unwrap();
            assert_eq!(numerics::Numerics::value80(raw).raw(), Raw80::new(0, units));
            let scaled = scale(raw, true).ok().unwrap();
            assert_eq!(
                numerics::Numerics::value80(scaled).raw(),
                Raw80::new(0, scaled_units)
            );
        }
        let largest = numerics::canonical(Raw80::new(0x7ffe, u64::MAX));
        let scaled = scale(largest, true).ok().unwrap();
        assert_eq!(
            numerics::Numerics::value80(scaled).raw(),
            Raw80::new(0x7fff, 1 << 63)
        );
    }
}
