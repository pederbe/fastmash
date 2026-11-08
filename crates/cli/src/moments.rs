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

#[cfg_attr(feature = "hotpath", hotpath::measure)]
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

#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
        let divisor = integer(n);
        let moment2 = decimal::divide(moment2, divisor)?;
        let moment3 = decimal::divide(moment3, divisor)?;
        let squared_moment2 = decimal::multiply(moment2, moment2)?;
        let result = finished_skew(moment2, moment3, squared_moment2, arithmetic)?;
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

#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
        let divisor = integer(n);
        let moment2 = decimal::divide(moment2, divisor)?;
        let moment4 = decimal::divide(moment4, divisor)?;
        let squared_moment2 = decimal::multiply(moment2, moment2)?;
        let result = finished_kurtosis(moment4, squared_moment2)?;
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

fn finished_skew(
    moment2: numerics::Value,
    moment3: numerics::Value,
    squared_moment2: numerics::Value,
    arithmetic: &mut numerics::Numerics,
) -> Result<numerics::Value, Failure> {
    let cubed_moment2 = decimal::multiply(moment2, squared_moment2)?;
    let denominator = arithmetic.sqrt(cubed_moment2).map_err(numeric_failure)?;
    decimal::divide(moment3, denominator)
}

fn finished_kurtosis(
    moment4: numerics::Value,
    squared_moment2: numerics::Value,
) -> Result<numerics::Value, Failure> {
    let result = decimal::divide(moment4, squared_moment2)?;
    decimal::subtract(result, integer(3))
}

/// Jarque-Bera needs both central moments. Share only identical scalar
/// subexpressions, retaining each input-order accumulation and its round points.
/// Multiplication is commutative for admitted values, including the CLI's
/// symmetric NaN policy, so the rounded cube serves both accumulations.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn prepare_normality(
    values: &[numerics::Value],
    arithmetic: &mut numerics::Numerics,
    cache: &mut Cache,
) -> Result<(), Failure> {
    let n = values.len() as u64;
    if n <= 1 || cache.skew.is_some() || cache.kurtosis.is_some() {
        return Ok(());
    }
    let mean = sample_mean(values, cache)?;
    let mut moment2 = integer(0);
    let mut moment3 = integer(0);
    let mut moment4 = integer(0);
    for &value in values {
        let delta = decimal::subtract(value, mean)?;
        let square = decimal::multiply(delta, delta)?;
        moment2 = decimal::add(moment2, square)?;
        let cube = decimal::multiply(square, delta)?;
        let fourth = decimal::multiply(delta, cube)?;
        moment4 = decimal::add(moment4, fourth)?;
        moment3 = decimal::add(moment3, cube)?;
    }
    // Jarque-Bera finishes kurtosis before skewness, including on failure.
    let divisor = integer(n);
    let moment2 = decimal::divide(moment2, divisor)?;
    let moment4 = decimal::divide(moment4, divisor)?;
    let squared_moment2 = decimal::multiply(moment2, moment2)?;
    cache.kurtosis = Some(finished_kurtosis(moment4, squared_moment2)?);
    let moment3 = decimal::divide(moment3, divisor)?;
    cache.skew = Some(finished_skew(
        moment2,
        moment3,
        squared_moment2,
        arithmetic,
    )?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::{Kind, operation_set::moment_value};
    use super::*;

    #[test]
    fn combined_normality_matches_separate_kernels_bit_for_bit() {
        let special = |bits| {
            numerics::canonical(fastmash_numeric_contract::Raw80::try_from_bits(bits).unwrap())
        };
        let edges = [
            0,
            1 << 79,
            1,
            (1 << 63) - 1,
            0x0001_8000_0000_0000_0000,
            0x3fff_8000_0000_0000_0000,
            0xbfff_8000_0000_0000_0000,
            0x7ffe_ffff_ffff_ffff_ffff,
            0xfffe_ffff_ffff_ffff_ffff,
            0x7fff_8000_0000_0000_0000,
            0xffff_8000_0000_0000_0000,
            0x7fff_c000_0000_0000_0001,
            0xffff_c000_0000_0000_0002,
        ];
        let mut math = numerics::Numerics::with_requirements(false, true)
            .unwrap()
            .with_mean_math(true)
            .unwrap();
        let mut random = 17u64;
        for case in 0..384 {
            let count = case % 129;
            let values: Vec<_> = (0..count)
                .map(|at| {
                    random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
                    if case < edges.len() * 2 || (case % 3 == 0 && at % 17 == 0) {
                        special(edges[(case + at) % edges.len()])
                    } else {
                        let exponent = 0x3fffu16 - 40 + (random % 81) as u16;
                        numerics::canonical(fastmash_numeric_contract::Raw80::new(
                            exponent | ((random >> 48) as u16 & 0x8000),
                            random | (1 << 63),
                        ))
                    }
                })
                .collect();
            let mut separate = Cache::default();
            excess_kurtosis(&values, false, &mut math, &mut separate)
                .ok()
                .unwrap();
            let expected = super::super::normality::jarque_bera(&values, &mut math, &mut separate)
                .ok()
                .unwrap();
            let mut combined = Cache::default();
            let actual = super::super::normality::jarque_bera(&values, &mut math, &mut combined)
                .ok()
                .unwrap();
            assert!(
                numerics::Numerics::value80(actual)
                    .same_bits(numerics::Numerics::value80(expected)),
                "case {case}"
            );
            for kind in [
                Kind::Pskew,
                Kind::Sskew,
                Kind::Pkurt,
                Kind::Skurt,
                Kind::Dpo,
            ] {
                let expected = moment_value(kind, &values, &mut math, &mut Cache::default())
                    .ok()
                    .unwrap();
                let actual = moment_value(kind, &values, &mut math, &mut combined)
                    .ok()
                    .unwrap();
                assert!(
                    numerics::Numerics::value80(actual)
                        .same_bits(numerics::Numerics::value80(expected)),
                    "case {case}, {kind:?}"
                );
            }
        }
    }

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
                remaining.swap_remove(i);
                orders(prefix, &remaining, out);
                prefix.pop();
            }
        }
        let mut permutations = Vec::new();
        orders(&mut Vec::new(), &kinds, &mut permutations);
        assert_eq!(permutations.len(), 720);
        for (at, order) in permutations.iter().enumerate() {
            assert!(!permutations[..at].contains(order));
        }
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
