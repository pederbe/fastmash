//! Input-order central moments with shared Rust binary80 arithmetic.
use super::{Failure, decimal, integer, numeric_failure, numerics, quiet_nan};

/// A sample owner's finished moments for the current group, cleared with its
/// samples, so every operation on that field reuses them in any request order.
#[derive(Default)]
pub(super) struct Cache {
    mean: Option<numerics::Value>,
    skew: Option<numerics::Value>,
    kurtosis: Option<numerics::Value>,
}
impl Cache {
    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }
}

fn sample_correction_product(n: u64) -> u64 {
    n.wrapping_mul(n - 1)
}

fn sample_mean(values: &[numerics::Value], cache: &mut Cache) -> Result<numerics::Value, Failure> {
    if let Some(mean) = cache.mean {
        return Ok(mean);
    }
    let mut sum = integer(0);
    for &value in values {
        sum = decimal::add(sum, value)?;
    }
    let mean = decimal::divide(sum, integer(values.len() as u64))?;
    cache.mean = Some(mean);
    Ok(mean)
}

pub(super) fn skewness(
    values: &[numerics::Value],
    sample: bool,
    arithmetic: &mut numerics::Numerics,
    cache: &mut Cache,
) -> Result<numerics::Value, Failure> {
    let n = values.len() as u64;
    if n <= 1 {
        return Ok(quiet_nan());
    }
    let result = if let Some(result) = cache.skew {
        result
    } else {
        let mean = sample_mean(values, cache)?;
        let mut moment2 = integer(0);
        let mut moment3 = integer(0);
        for &value in values {
            let delta = decimal::subtract(value, mean)?;
            let square = decimal::multiply(delta, delta)?;
            moment2 = decimal::add(moment2, square)?;
            let cube = decimal::multiply(delta, square)?;
            moment3 = decimal::add(moment3, cube)?;
        }
        moment2 = decimal::divide(moment2, integer(n))?;
        moment3 = decimal::divide(moment3, integer(n))?;
        let squared_moment2 = decimal::multiply(moment2, moment2)?;
        let cubed_moment2 = decimal::multiply(moment2, squared_moment2)?;
        let denominator = arithmetic.sqrt(cubed_moment2).map_err(numeric_failure)?;
        let result = decimal::divide(moment3, denominator)?;
        cache.skew = Some(result);
        result
    };
    if sample {
        if n <= 2 {
            return Ok(quiet_nan());
        }
        // GNU multiplies size_t before conversion; preserve its 64-bit wraparound.
        let correction = arithmetic
            .sqrt(integer(sample_correction_product(n)))
            .map_err(numeric_failure)?;
        let correction = decimal::divide(correction, integer(n - 2))?;
        decimal::multiply(result, correction)
    } else {
        Ok(result)
    }
}

pub(super) fn excess_kurtosis(
    values: &[numerics::Value],
    sample: bool,
    _arithmetic: &mut numerics::Numerics,
    cache: &mut Cache,
) -> Result<numerics::Value, Failure> {
    let n = values.len() as u64;
    if n <= 1 {
        return Ok(quiet_nan());
    }
    let result = if let Some(result) = cache.kurtosis {
        result
    } else {
        let mean = sample_mean(values, cache)?;
        let mut moment2 = integer(0);
        let mut moment4 = integer(0);
        for &value in values {
            let delta = decimal::subtract(value, mean)?;
            let square = decimal::multiply(delta, delta)?;
            moment2 = decimal::add(moment2, square)?;
            let cube = decimal::multiply(square, delta)?;
            let fourth = decimal::multiply(delta, cube)?;
            moment4 = decimal::add(moment4, fourth)?;
        }
        moment2 = decimal::divide(moment2, integer(n))?;
        moment4 = decimal::divide(moment4, integer(n))?;
        let denominator = decimal::multiply(moment2, moment2)?;
        let result = decimal::divide(moment4, denominator)?;
        let result = decimal::subtract(result, integer(3))?;
        cache.kurtosis = Some(result);
        result
    };
    if sample {
        if n <= 3 {
            return Ok(quiet_nan());
        }
        let adjusted = decimal::multiply(result, integer(n + 1))?;
        let adjusted = decimal::add(adjusted, integer(6))?;
        let n1 = decimal::subtract(integer(n), integer(1))?;
        let n2 = decimal::subtract(integer(n), integer(2))?;
        let n3 = decimal::subtract(integer(n), integer(3))?;
        let denominator = decimal::multiply(n3, n2)?;
        let correction = decimal::divide(n1, denominator)?;
        decimal::multiply(adjusted, correction)
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, operation_set::moment_value};
    use super::*;

    #[test]
    fn reuse_matches_fresh_bits_in_every_order() {
        let kinds = [
            Kind::Pskew,
            Kind::Sskew,
            Kind::Pkurt,
            Kind::Skurt,
            Kind::Jarque,
            Kind::Dpo,
        ];
        let special = |bits| {
            numerics::canonical(fastmash_numeric_contract::Raw80::try_from_bits(bits).unwrap())
        };
        let datasets = [
            vec![],
            vec![integer(1)],
            vec![integer(1), integer(2)],
            vec![integer(0), integer(0), integer(4)],
            vec![integer(0), integer(0), integer(0), integer(4)],
            (0..100).map(|i| integer(i * i % 37)).collect(),
            vec![integer(1); 8],
            vec![special(1), special(2), special(3), special(4)],
            vec![
                special(0x7ffe_8000_0000_0000_0000),
                integer(1),
                integer(2),
                integer(3),
            ],
            vec![special(0x8000_0000_0000_0000_0000), integer(0), integer(0)],
            vec![
                special(0xffff_c000_0000_0000_0002),
                special(0x7fff_c000_0000_0000_0003),
                integer(2),
                integer(3),
            ],
            vec![
                special(0x7fff_8000_0000_0000_0000),
                integer(1),
                integer(2),
                integer(3),
            ],
        ];
        let mut math = numerics::Numerics::with_requirements(false, true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        // All 720 orders exercise normality-first, sample-first and interleaving.
        fn orders(prefix: &mut Vec<Kind>, rest: &[Kind], out: &mut Vec<Vec<Kind>>) {
            if rest.is_empty() {
                out.push(prefix.clone());
            }
            for (i, &kind) in rest.iter().enumerate() {
                prefix.push(kind);
                let mut remaining = rest.to_vec();
                remaining.remove(i);
                orders(prefix, &remaining, out);
                prefix.pop();
            }
        }
        let mut permutations = Vec::new();
        orders(&mut Vec::new(), &kinds, &mut permutations);
        for values in datasets {
            let expected: Vec<_> = kinds
                .iter()
                .map(|&kind| {
                    numerics::Numerics::value80(
                        moment_value(kind, &values, &mut math, &mut Cache::default())
                            .ok()
                            .unwrap(),
                    )
                })
                .collect();
            for order in &permutations {
                let mut cache = Cache::default();
                for &kind in order {
                    let actual = numerics::Numerics::value80(
                        moment_value(kind, &values, &mut math, &mut cache)
                            .ok()
                            .unwrap(),
                    );
                    assert!(
                        actual.same_bits(expected[kinds.iter().position(|&k| k == kind).unwrap()])
                    );
                }
            }
        }
    }

    #[test]
    fn failed_result_is_not_cached_and_clear_forgets_results() {
        let mut cache = Cache::default();
        let values = [integer(0), integer(0), integer(0), integer(4)];
        let mut unavailable = numerics::Numerics::new(false).unwrap();
        assert!(skewness(&values, false, &mut unavailable, &mut cache).is_err());
        assert!(cache.skew.is_none());
        let mut math = numerics::Numerics::with_requirements(false, true).unwrap();
        assert!(skewness(&values, false, &mut math, &mut cache).is_ok());
        assert!(cache.skew.is_some());
        cache.clear();
        assert!(cache.mean.is_none() && cache.skew.is_none());
    }
    #[test]
    fn sample_count_product_matches_gnu_size_t_boundary() {
        assert_eq!(sample_correction_product(65_537), 4_295_032_832);
        assert_eq!(sample_correction_product(1 << 32), u64::MAX - (1 << 32) + 1);
        assert_eq!(sample_correction_product((1 << 32) + 1), 1 << 32);
    }
}
