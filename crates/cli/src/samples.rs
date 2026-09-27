//! Admitted samples with the preserved stable merge traversal.
use super::numerics::{Value, sample_order as compare};
use std::cmp::Ordering;

#[cfg(test)]
pub(super) const LIMIT: usize = 65_536;

#[cfg(test)]
thread_local! {
    pub(super) static FAIL_GROWTH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    #[cfg(test)]
    Capacity,
    Allocation,
    SortingAllocation,
}

#[derive(Default)]
pub(super) struct Samples {
    values: Vec<Value>,
    sorted: bool,
}

#[derive(Clone, Copy)]
pub(super) enum Quartile {
    First,
    Third,
}

pub(super) enum Selection {
    Single(Value),
    Between {
        lower: Value,
        upper: Value,
        quarters: u8,
    },
}

impl Quartile {
    pub(super) fn select(self, sorted: &[Value]) -> Option<Selection> {
        match sorted.len() {
            0 => None,
            1 => Some(Selection::Single(sorted[0])),
            n => {
                let numerator = match self {
                    Self::First => 1,
                    Self::Third => 3,
                };
                let whole = (n - 1) / 4;
                let remainder = ((n - 1) % 4) * numerator;
                let index = whole * numerator + remainder / 4;
                Some(Selection::Between {
                    lower: sorted[index],
                    upper: sorted[index + 1],
                    quarters: (remainder % 4) as u8,
                })
            }
        }
    }
}

/// Select a run using GNU's comparator equality, including unordered NaNs.
pub(super) fn mode(sorted: &[Value], least_frequent: bool) -> Option<Value> {
    let mut last = *sorted.first()?;
    let mut best = last;
    let mut best_len = if least_frequent { usize::MAX } else { 1 };
    let mut run_len = 1;
    let mut consider = |len, value| {
        if (least_frequent && len < best_len) || (!least_frequent && len > best_len) {
            best_len = len;
            best = value;
        }
    };
    for &value in &sorted[1..] {
        if compare(value, last) == Ordering::Equal {
            run_len += 1;
        } else {
            consider(run_len, last);
            run_len = 1;
        }
        last = value;
    }
    consider(run_len, last);
    Some(best)
}

impl Samples {
    #[cfg(test)]
    pub(super) fn push(&mut self, value: Value) -> Result<(), Error> {
        if self.values.len() == LIMIT {
            return Err(Error::Capacity);
        }
        self.push_with_limit(value, LIMIT)
    }

    pub(super) fn push_growable(&mut self, value: Value) -> Result<(), Error> {
        self.push_with_limit(value, usize::MAX)
    }

    fn push_with_limit(&mut self, value: Value, limit: usize) -> Result<(), Error> {
        if self.values.len() == self.values.capacity() {
            #[cfg(test)]
            if FAIL_GROWTH.with(|fail| fail.replace(false)) {
                return Err(Error::Allocation);
            }
            let capacity = self.values.capacity().max(4).saturating_mul(2).min(limit);
            self.values
                .try_reserve_exact(capacity - self.values.len())
                .map_err(|_| Error::Allocation)?;
        }
        self.sorted = false;
        self.values.push(value);
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.sorted = false;
        self.values.clear();
    }

    /// Borrow the stored order. Variance never calls the sorting finalizer.
    pub(super) fn as_slice(&self) -> &[Value] {
        &self.values
    }

    #[cfg(test)]
    pub(super) fn capacity(&self) -> usize {
        self.values.capacity()
    }

    /// Transform in order without reallocating. On failure the caller must stop.
    pub(super) fn try_map<E>(
        &mut self,
        mut transform: impl FnMut(Value) -> Result<Value, E>,
    ) -> Result<(), E> {
        self.sorted = false;
        for value in &mut self.values {
            *value = transform(*value)?;
        }
        Ok(())
    }

    /// Sort the current values; MAD transforms them before its second sort.
    /// The non-total NaN relation means sorting twice need not be idempotent.
    pub(super) fn sorted(&mut self) -> Result<&[Value], Error> {
        self.sorted = false;
        let n = self.values.len();
        if n > 1 {
            let mut scratch = Vec::new();
            scratch
                .try_reserve_exact(n)
                .map_err(|_| Error::SortingAllocation)?;
            scratch.extend_from_slice(&self.values);
            merge_initialized(&mut self.values, &mut scratch);
        }
        self.sorted = true;
        Ok(&self.values)
    }

    /// Quantile owners finalize once; repeated NaN sorts can change results.
    pub(super) fn sorted_once(&mut self) -> Result<&[Value], Error> {
        if self.sorted {
            Ok(&self.values)
        } else {
            self.sorted()
        }
    }

    pub(super) fn middle(&mut self) -> Result<Option<(Value, Option<Value>)>, Error> {
        let sorted = self.sorted()?;
        let n = sorted.len();
        if n == 0 {
            return Ok(None);
        }
        let upper = sorted[n / 2];
        Ok(Some((upper, (n % 2 == 0).then(|| sorted[n / 2 - 1]))))
    }
}

/// Both buffers initially contain the same values and have equal lengths.
/// Alternating their roles avoids copying each merged range back to its input.
fn merge_initialized(values: &mut [Value], scratch: &mut [Value]) {
    let n = values.len();
    if n < 2 {
        return;
    }
    if n == 2 {
        // The same single comparison as merging two leaves, without recursion.
        if compare(values[0], values[1]) == Ordering::Greater {
            values.swap(0, 1);
        }
        return;
    }
    let middle = n / 2;
    let (left, right) = values.split_at_mut(middle);
    let (scratch_left, scratch_right) = scratch.split_at_mut(middle);
    merge_initialized(scratch_left, left);
    merge_initialized(scratch_right, right);
    let (mut a, mut b, mut out) = (0, middle, 0);
    while a < middle && b < n {
        let next = if compare(scratch[a], scratch[b]) != Ordering::Greater {
            let next = a;
            a += 1;
            next
        } else {
            let next = b;
            b += 1;
            next
        };
        values[out] = scratch[next];
        out += 1;
    }
    values[out..out + middle - a].copy_from_slice(&scratch[a..middle]);
    out += middle - a;
    values[out..n].copy_from_slice(&scratch[b..n]);
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::numerics::test_support::Inspect;
    use fastmash_numeric_contract::Raw80;
    use fastmash_portable_numerics::percentile_position;

    fn original_merge(values: &mut [Value], scratch: &mut [Value]) {
        let n = values.len();
        if n < 2 {
            return;
        }
        let middle = n / 2;
        let (left, right) = values.split_at_mut(middle);
        original_merge(left, scratch);
        original_merge(right, scratch);
        let (mut a, mut b, mut out) = (0, middle, 0);
        while a < middle && b < n {
            let next = if compare(values[a], values[b]) != Ordering::Greater {
                let next = a;
                a += 1;
                next
            } else {
                let next = b;
                b += 1;
                next
            };
            scratch[out] = values[next];
            out += 1;
        }
        scratch[out..out + middle - a].copy_from_slice(&values[a..middle]);
        out += middle - a;
        scratch[out..n].copy_from_slice(&values[b..n]);
        values.copy_from_slice(&scratch[..n]);
    }

    #[test]
    fn alternating_buffers_preserve_original_raw_order() {
        fn check(mut actual: Vec<Value>) {
            let mut expected = actual.clone();
            let mut scratch = actual.clone();
            original_merge(&mut expected, &mut scratch);
            scratch.copy_from_slice(&actual);
            merge_initialized(&mut actual, &mut scratch);
            assert!(actual.iter().zip(&expected).all(|(a, b)| a.same_bits(*b)));
        }
        let values = [
            value(0, 0),
            value(0x8000, 0),
            one(),
            value(0xbfff, 1 << 63),
            nan(),
            value(0xffff, 0xc000_0000_0000_0042),
            value(0x7fff, 1 << 63),
            value(0, 1),
        ];
        for n in 0..=5u32 {
            for mut pattern in 0..values.len().pow(n) {
                let input = (0..n)
                    .map(|_| {
                        let v = values[pattern % values.len()];
                        pattern /= values.len();
                        v
                    })
                    .collect();
                check(input);
            }
        }
        // Unequal recursive depths and larger merges, with reproducible inputs.
        let mut state = 7u64;
        for n in [7, 8, 9, 31, 32, 33, 255, 256, 257, 1023, 1024, 1025] {
            let input = (0..n)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    values[(state >> 32) as usize % values.len()]
                })
                .collect();
            check(input);
        }
    }

    fn value(exponent: u16, significand: u64) -> Value {
        super::super::numerics::canonical(Raw80::new(exponent, significand))
    }
    fn one() -> Value {
        value(0x3fff, 1 << 63)
    }
    fn nan() -> Value {
        value(0x7fff, 0xc000_0000_0000_0000)
    }

    #[test]
    fn mode_runs_include_final_run_and_keep_first_tie() {
        let two = value(0x4000, 1 << 63);
        for least in [false, true] {
            assert!(mode(&[], least).is_none());
            assert!(mode(&[one()], least).unwrap().same_bits(one()));
            assert!(mode(&[one(), two], least).unwrap().same_bits(one()));
            assert!(
                mode(&[one(), one(), two, two], least)
                    .unwrap()
                    .same_bits(one())
            );
        }
        assert!(mode(&[one(), two, two], false).unwrap().same_bits(two));
        assert!(mode(&[one(), one(), two], true).unwrap().same_bits(two));
    }

    #[test]
    fn mode_uses_last_encoding_and_comparator_equal_nan_chain() {
        let positive = value(0, 0);
        let negative = value(0x8000, 0);
        let two = value(0x4000, 1 << 63);
        for least in [false, true] {
            assert!(
                mode(&[positive, negative], least)
                    .unwrap()
                    .same_bits(negative)
            );
            assert!(
                mode(&[negative, positive], least)
                    .unwrap()
                    .same_bits(positive)
            );
            assert!(mode(&[one(), nan(), two], least).unwrap().same_bits(two));
            assert!(mode(&[one(), nan()], least).unwrap().same_bits(nan()));
        }
    }

    #[test]
    fn transformation_reuses_storage_and_second_sort_sees_new_values() {
        let two = value(0x4000, 1 << 63);
        let mut samples = Samples::default();
        samples.push(two).unwrap();
        samples.push(one()).unwrap();
        samples.middle().unwrap();
        let capacity = samples.values.capacity();
        let mut visited = Vec::new();
        samples
            .try_map(|item| {
                visited.push(item);
                Ok::<_, ()>(if item.same_bits(one()) { two } else { one() })
            })
            .unwrap();
        assert!(visited[0].same_bits(one()) && visited[1].same_bits(two));
        assert_eq!(samples.values.capacity(), capacity);
        assert!(samples.as_slice()[0].same_bits(two));
        let (upper, lower) = samples.middle().unwrap().unwrap();
        assert!(upper.same_bits(two) && lower.unwrap().same_bits(one()));
        samples.clear();
        samples.push(nan()).unwrap();
        assert_eq!(samples.as_slice().len(), 1);
        assert!(samples.as_slice()[0].same_bits(nan()));
        assert_eq!(samples.values.capacity(), capacity);
    }

    #[test]
    fn failed_transformation_stops_before_later_values() {
        let mut samples = Samples::default();
        for _ in 0..3 {
            samples.push(one()).unwrap();
        }
        let mut calls = 0;
        let result = samples.try_map(|_| {
            calls += 1;
            if calls == 2 { Err("stop") } else { Ok(nan()) }
        });
        assert_eq!(result, Err("stop"));
        assert_eq!(calls, 2);
        assert!(samples.as_slice()[0].same_bits(nan()));
        assert!(
            samples.as_slice()[1..]
                .iter()
                .all(|item| item.same_bits(one()))
        );
    }

    #[test]
    fn borrowed_samples_keep_input_order_and_reset() {
        let input = [value(0x4000, 1 << 63), nan(), value(0x8000, 0), one()];
        let mut samples = Samples::default();
        for item in input {
            samples.push(item).unwrap();
        }
        for _ in 0..2 {
            assert!(
                samples
                    .as_slice()
                    .iter()
                    .zip(input)
                    .all(|(a, b)| a.same_bits(b))
            );
        }
        samples.clear();
        assert!(samples.as_slice().is_empty());
        samples.push(one()).unwrap();
        assert_eq!(samples.as_slice().len(), 1);
        assert!(samples.as_slice()[0].same_bits(one()));
    }

    #[test]
    fn percentile_positions_match_independent_binary64_rounding() {
        // Golden digests use rational bisection over binary64 encodings, then
        // nearest/even selection, independently of the production bit shifts.
        for (n, expected) in [
            (2, 1343692906647796045u64),
            (3, 10441548401316612419),
            (4, 2378141079538417712),
            (5, 7604715108629413194),
            (8, 8804390430377181553),
            (51, 14207652410686715444),
            (101, 10615079459943864414),
            (1001, 2831036291611717742),
            (LIMIT, 6083088224829868481),
        ] {
            let mut digest = 0xcbf29ce484222325u64;
            for percent in 1..100 {
                let (index, fraction) = percentile_position(n, percent);
                for byte in (index as u64)
                    .to_le_bytes()
                    .into_iter()
                    .chain(fraction.exponent().to_le_bytes())
                    .chain(fraction.significand().to_le_bytes())
                {
                    digest = (digest ^ u64::from(byte)).wrapping_mul(0x100000001b3);
                }
            }
            assert_eq!(digest, expected, "length {n}");
        }
        // Exact rational arithmetic would incorrectly choose index 29.
        assert_eq!(percentile_position(101, 29).0, 28);
    }

    #[test]
    fn every_admitted_percentile_position_has_two_samples() {
        for n in 2..=LIMIT {
            for percent in 1..100 {
                let (index, fraction) = percentile_position(n, percent);
                assert!(index + 1 < n);
                assert!(!fraction.is_negative());
                assert_eq!(compare(fraction, one()), Ordering::Less);
                let rational_index = (n - 1) * usize::from(percent) / 100;
                assert!(index.abs_diff(rational_index) <= 1);
            }
        }
    }

    #[test]
    fn canonical_order_covers_signs_exponents_and_significands() {
        let ordered = [
            value(0xffff, 1 << 63),
            value(0xbfff, 1 << 63),
            value(0x8001, 1 << 63),
            value(0x8000, (1 << 63) - 1),
            value(0x8000, 1),
            value(0, 0),
            value(0, 1),
            value(0, (1 << 63) - 1),
            value(1, 1 << 63),
            one(),
            value(0x3fff, (1 << 63) + 1),
            value(0x7ffe, u64::MAX),
            value(0x7fff, 1 << 63),
        ];
        for (i, &a) in ordered.iter().enumerate() {
            for (j, &b) in ordered.iter().enumerate() {
                assert_eq!(compare(a, b), i.cmp(&j));
            }
        }
        assert_eq!(compare(value(0x8000, 0), value(0, 0)), Ordering::Equal);
        for a in ordered {
            assert_eq!(compare(a, nan()), Ordering::Equal);
            assert_eq!(compare(nan(), a), Ordering::Equal);
        }
    }

    #[test]
    fn reference_merge_schedule_preserves_non_total_order_and_zero_ties() {
        let two = value(0x4000, 1 << 63);
        let mut values = [two, nan(), one()];
        let mut scratch = values;
        merge_initialized(&mut values, &mut scratch);
        assert!(values[1].same_bits(nan()));
        let mut values = [nan(), two, one()];
        let mut scratch = values;
        merge_initialized(&mut values, &mut scratch);
        assert!(values[1].same_bits(one()));
        let mut values = [value(0, 0), value(0x8000, 0), value(0, 0)];
        let mut scratch = values;
        merge_initialized(&mut values, &mut scratch);
        assert!(values[1].is_negative());
        let negative_nan = value(0xffff, 0xc000_0000_0000_0042);
        let mut values = [negative_nan, nan(), one()];
        let mut scratch = values;
        merge_initialized(&mut values, &mut scratch);
        assert!(values[0].same_bits(negative_nan));
        assert!(values[1].same_bits(nan()));
        assert!(values[2].same_bits(one()));
    }

    #[test]
    fn middle_selects_upper_then_optional_lower_and_reset_reuses_capacity() {
        let mut samples = Samples::default();
        assert!(samples.middle().unwrap().is_none());
        samples.push(one()).unwrap();
        let (upper, lower) = samples.middle().unwrap().unwrap();
        assert!(upper.same_bits(one()) && lower.is_none());
        samples.clear();
        samples.push(one()).unwrap();
        samples.push(value(0, 0)).unwrap();
        let (upper, lower) = samples.middle().unwrap().unwrap();
        assert!(upper.same_bits(one()));
        assert!(lower.unwrap().same_bits(value(0, 0)));
        let capacity = samples.values.capacity();
        samples.clear();
        assert_eq!(samples.values.capacity(), capacity);
        assert!(samples.middle().unwrap().is_none());
    }

    #[test]
    fn exact_limit_declines_without_mutation() {
        assert_eq!(std::mem::size_of::<Value>(), 16);
        let mut samples = Samples::default();
        for _ in 0..LIMIT {
            samples.push(one()).unwrap();
        }
        assert_eq!(samples.values.len(), LIMIT);
        assert_eq!(samples.push(one()), Err(Error::Capacity));
        assert_eq!(samples.values.len(), LIMIT);
        assert!(samples.values.iter().all(|value| value.same_bits(one())));
        samples.clear();
        samples.push(one()).unwrap();
        assert_eq!(samples.values.len(), 1);
    }

    #[test]
    fn quartile_selection_covers_residues_and_singletons() {
        assert!(Quartile::First.select(&[]).is_none());
        let negative_zero = value(0x8000, 0);
        for which in [Quartile::First, Quartile::Third] {
            let Some(Selection::Single(selected)) = which.select(&[negative_zero]) else {
                panic!("singleton must bypass interpolation");
            };
            assert!(selected.same_bits(negative_zero));
        }
        // Independent type-7 examples: (length, Q1 lower index/remainder, Q3).
        for (n, first, third) in [
            (2, (0, 1), (0, 3)),
            (3, (0, 2), (1, 2)),
            (4, (0, 3), (2, 1)),
            (5, (1, 0), (3, 0)),
            (6, (1, 1), (3, 3)),
            (7, (1, 2), (4, 2)),
            (8, (1, 3), (5, 1)),
            (9, (2, 0), (6, 0)),
            (LIMIT, (16383, 3), (49151, 1)),
        ] {
            let values: Vec<_> = (0..n)
                .map(|i| value(0x3fff, (1 << 63) + i as u64))
                .collect();
            for (which, (index, remainder)) in [(Quartile::First, first), (Quartile::Third, third)]
            {
                let Some(Selection::Between {
                    lower,
                    upper,
                    quarters,
                }) = which.select(&values)
                else {
                    panic!("multiple samples must interpolate even at an exact index");
                };
                assert!(lower.same_bits(values[index]));
                assert!(upper.same_bits(values[index + 1]));
                assert_eq!(quarters, remainder);
            }
        }
    }

    #[test]
    fn both_quartiles_select_from_one_sort_without_mutating_nan_order() {
        let two = value(0x4000, 1 << 63);
        let mut samples = Samples::default();
        for item in [nan(), two, one()] {
            samples.push(item).unwrap();
        }
        let sorted = samples.sorted().unwrap();
        let Some(Selection::Between {
            lower,
            upper,
            quarters,
        }) = Quartile::Third.select(sorted)
        else {
            panic!("Q3 pair missing");
        };
        assert!(lower.same_bits(one()) && upper.same_bits(two));
        assert_eq!(quarters, 2);
        let Some(Selection::Between {
            lower,
            upper,
            quarters,
        }) = Quartile::First.select(sorted)
        else {
            panic!("Q1 pair missing");
        };
        assert!(lower.same_bits(nan()) && upper.same_bits(one()));
        assert_eq!(quarters, 2);
        assert!(
            sorted[0].same_bits(nan()) && sorted[1].same_bits(one()) && sorted[2].same_bits(two)
        );
    }
}
