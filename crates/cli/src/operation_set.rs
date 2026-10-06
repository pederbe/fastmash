//! The Operations of one Command, in request order. The set owns the work its
//! Operations share (field conversion, sums, samples, dispersion and pairs),
//! each Operation family's Group state, and what the Command needs before
//! input is read. Callers add records, reset between Groups and write results.
use super::normality::{dagostino_pearson, jarque_bera};
use super::{
    Failure, Kind, Selector, base64_fields, command_memory, command_output, conversion_failure,
    decimal, dispersion, field_policy, field_value, grammar, indexed_field, integer, line_numeric,
    moments, named_fields, numeric_failure, numerics, options, ordered_statistics, paired,
    presentation, random, records, robust_statistics, sample_failure, samples, scalar_text,
    scalar_text_failure, text_samples, unsupported, weighted,
};
use moments::{excess_kurtosis, skewness};

#[cfg(test)]
#[path = "operation_completion_tests.rs"]
mod completion_tests;
#[cfg(test)]
#[path = "paired_sharing_tests.rs"]
mod paired_sharing_tests;
#[cfg(test)]
#[path = "operation_set_tests.rs"]
pub(super) mod tests;

/// The Operations of one Command, in request order: what each computes, and
/// its state in the current Group.
pub(super) struct OperationSet {
    plans: Vec<Plan>,
    states: Vec<State>,
    weighted_mean: bool,
}

/// Dense retained Groups tied to the Binding that created them. One state
/// vector holds every Group in insertion order; scalar state stays inline.
pub(super) struct DenseGroups<'a> {
    operations: &'a OperationSet,
    states: Vec<State>,
}

/// One retained Group tied to its Binding, keeping its own state vector.
pub(super) struct RetainedGroup<'a> {
    operations: &'a OperationSet,
    states: Vec<State>,
}

/// Short-lived completion of one Group, directly on its collected state.
/// Callers cannot separate that state from the plans of its Binding.
pub(super) struct GroupCompletion<'a> {
    plans: &'a [Plan],
    states: &'a mut [State],
}

/// What a Command's Operations need before input is read.
#[derive(Clone, Copy, Debug)]
pub(super) struct Needs {
    /// The optional numerics the summaries use.
    pub numerics: numerics::Requirements,
    /// Whether an Operation reads or prints numbers, so the numeric locale applies.
    pub numeric_locale: bool,
    /// Whether `rand` needs random state.
    pub random: bool,
}

/// How the native Sort route can sort records for these Operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeSort {
    /// An Operation needs more than the native Sort route keeps.
    Unsupported,
    /// An Operation reads numbers, which GNU reads on from the field into the
    /// rest of the record: packed fields serve only where the separator
    /// cannot continue a number, otherwise sort the original records.
    Original,
    /// Operations read only selected fields' text: sort packed fields.
    Packed,
}

/// A record's fields as the Operations read them.
pub(super) trait Fields<'r> {
    /// Field `field`'s text, or the failure naming it missing.
    fn text(
        &mut self,
        field: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<&'r [u8], Failure>;
    /// Field `field` as a number, `None` for an NA value that --narm skips.
    fn number(
        &mut self,
        field: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<Option<numerics::Value>, Failure>;
}

/// A whole record, its fields located through `index`, the record's index.
pub(super) struct Whole<'r, 'i> {
    pub(super) record: &'r [u8],
    pub(super) index: &'i mut records::FieldIndex,
}

impl<'r> Fields<'r> for Whole<'r, '_> {
    #[inline]
    fn text(
        &mut self,
        field: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<&'r [u8], Failure> {
        let span = indexed_field(self.record, self.index, field, line, options.input)?;
        Ok(&self.record[span.start..span.start + span.length])
    }
    #[inline]
    fn number(
        &mut self,
        field: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<Option<numerics::Value>, Failure> {
        field_value(
            self.record,
            self.index,
            field,
            line,
            options.input,
            options.narm,
            options.presentation.profile,
        )
    }
}

impl OperationSet {
    /// Builds the Operations from the parsed requests. Named fields are
    /// returned for `bind` once the Input header is read.
    pub(super) fn new(
        requests: Vec<grammar::Request>,
    ) -> Result<(Self, Vec<named_fields::Named>), Failure> {
        let mut names = Vec::new();
        command_memory::reserve(
            &mut names,
            requests
                .len()
                .checked_mul(2)
                .ok_or_else(|| unsupported("command memory allocation failed"))?,
        )?;
        let mut plans = Vec::new();
        command_memory::reserve(&mut plans, requests.len())?;
        let mut states = Vec::new();
        command_memory::reserve(&mut states, requests.len())?;
        let mut weighted_mean = false;
        for (index, request) in requests.into_iter().enumerate() {
            weighted_mean |= request.kind == Kind::Wmean;
            let mut resolve = |field, target| match field {
                grammar::Field::Number(field) => field,
                grammar::Field::Name(name) => {
                    names.push(named_fields::Named {
                        operation: index,
                        target,
                        name,
                    });
                    0
                }
            };
            let selector = match request.selector {
                grammar::Selector::Single(field) => {
                    Selector::Single(resolve(field, named_fields::Target::Single))
                }
                grammar::Selector::Pair { left, right } => Selector::Pair {
                    left: resolve(left, named_fields::Target::Left),
                    right: resolve(right, named_fields::Target::Right),
                },
            };
            let plan = Plan {
                kind: request.kind,
                selector,
                sum_source: None,
                sample_source: None,
                dispersion_source: None,
                conversion_source: None,
                pair_conversion: [None, None],
            };
            states.push(
                State::new(plan.kind, plan.keeps_values())
                    .ok_or_else(|| unsupported("command memory allocation failed"))?,
            );
            plans.push(plan);
        }
        Ok((
            Self {
                plans,
                states,
                weighted_mean,
            },
            names,
        ))
    }

    /// Applies resolved field names, then links the Operations that share
    /// work, which depends on the resolved selectors: before the first record,
    /// with or without an Input header, and again when hash grouping restarts
    /// and the sort route binds anew.
    pub(super) fn bind(
        &mut self,
        named: impl IntoIterator<Item = (usize, named_fields::Target, u64)>,
    ) -> Result<(), Failure> {
        for (index, target, field) in named {
            apply_named(&mut self.plans[index], target, field);
        }
        link_shared_fields(&mut self.plans)
    }

    /// What the Operations need before input is read; `explicit_format` is
    /// whether the output format is set explicitly.
    pub(super) fn needs(&self, explicit_format: bool) -> Needs {
        let mut needs = Needs {
            numerics: numerics::Requirements::default(),
            numeric_locale: false,
            random: false,
        };
        for operation in &self.plans {
            needs.numerics = needs.numerics | operation.kind.numeric_requirements();
            needs.numeric_locale |= operation.kind.reads_or_prints_numbers(explicit_format);
            needs.random |= operation.kind == Kind::Rand;
        }
        needs
    }

    /// Whether and how the native Sort route can sort for these Operations.
    pub(super) fn native_sort(&self) -> NativeSort {
        let supported = self.plans.iter().all(|op| {
            op.kind.uses_borrowed_number()
                || matches!(
                    op.kind,
                    Kind::Count
                        | Kind::First
                        | Kind::Last
                        | Kind::Unique
                        | Kind::Countunique
                        | Kind::Collapse
                )
        });
        if !supported {
            NativeSort::Unsupported
        } else if self.plans.iter().any(|op| op.kind.uses_borrowed_number()) {
            NativeSort::Original
        } else {
            NativeSort::Packed
        }
    }

    /// Each Operation's kind and selector, in request order.
    pub(super) fn requests(&self) -> impl ExactSizeIterator<Item = (Kind, Selector)> + '_ {
        self.plans.iter().map(|op| (op.kind, op.selector))
    }

    pub(super) fn len(&self) -> usize {
        self.plans.len()
    }

    /// Immutable Operation presence, retained through Binding and Group resets.
    pub(super) fn has_weighted_mean(&self) -> bool {
        self.weighted_mean
    }

    /// Every field the Operations select, in request order.
    pub(super) fn fields(&self) -> impl Iterator<Item = u64> + Clone + '_ {
        self.plans
            .iter()
            .flat_map(|op| match op.selector {
                Selector::Single(field) => [Some(field), None],
                Selector::Pair { left, right } => [Some(left), Some(right)],
            })
            .flatten()
    }

    /// Adds one record (line `line`) to the Operations in request order,
    /// locating its fields through `fields`, the record's index; the first
    /// failure stops the later ones. True when the record must become the
    /// Group's representative (for example for `last`). One concrete function,
    /// with the generic collector inlined into it.
    pub(super) fn collect(
        &mut self,
        record: &[u8],
        fields: &mut records::FieldIndex,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        collect(
            &mut Whole {
                record,
                index: fields,
            },
            &self.plans,
            &mut self.states,
            line,
            options,
            arithmetic,
            random,
        )
    }

    /// Collects already decoded fields with the same Operation and random state.
    pub(super) fn collect_decoded<'r>(
        &mut self,
        record: &mut impl Fields<'r>,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        collect(
            record,
            &self.plans,
            &mut self.states,
            line,
            options,
            arithmetic,
            random,
        )
    }

    /// [`OperationSet::collect`] with a fresh index for the selected fields.
    #[cfg(test)]
    pub(super) fn collect_line(
        &mut self,
        record: &[u8],
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        let mut fields = records::FieldIndex::selecting(self.fields());
        self.collect(record, &mut fields, line, options, arithmetic, random)
    }

    /// [`OperationSet::collect`] for a record whose fields `record` supplies,
    /// as the Packed native Sort route keeps only the selected fields. With
    /// `texts`, every Operation reads one field's text (`NativeSort::Packed`),
    /// which goes straight to the text collector.
    #[inline]
    pub(super) fn collect_fields<'r>(
        &mut self,
        record: &mut impl Fields<'r>,
        texts: bool,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
    ) -> Result<bool, Failure> {
        if texts {
            let mut keep = false;
            for (plan, state) in self.plans.iter().zip(&mut self.states) {
                let Selector::Single(field) = plan.selector else {
                    unreachable!("text Operations select single fields")
                };
                let bytes = record.text(field, line, options)?;
                keep |= collect_text(plan, state, bytes, options.narm, None)?;
            }
            return Ok(keep);
        }
        self.collect_selected(record, line, options, arithmetic)
    }

    /// [`OperationSet::collect_fields`] for Operations that read numbers: the
    /// generic collector, inlined here and kept out of the text path.
    #[inline(never)]
    fn collect_selected<'r>(
        &mut self,
        record: &mut impl Fields<'r>,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
    ) -> Result<bool, Failure> {
        collect(
            record,
            &self.plans,
            &mut self.states,
            line,
            options,
            arithmetic,
            None,
        )
    }

    /// The fields that numeric Operations read, each once.
    pub(super) fn numeric_fields(&self) -> Vec<u64> {
        let mut fields: Vec<u64> = self
            .plans
            .iter()
            .filter(|op| {
                op.kind.converts_numeric_field() || matches!(op.selector, Selector::Pair { .. })
            })
            .flat_map(|op| match op.selector {
                Selector::Single(field) => [Some(field), None],
                Selector::Pair { left, right } => [Some(left), Some(right)],
            })
            .flatten()
            .collect();
        fields.sort_unstable();
        fields.dedup();
        fields
    }

    /// Retained Groups that share this Binding and one dense state vector.
    pub(super) fn dense_groups(&self) -> DenseGroups<'_> {
        DenseGroups {
            operations: self,
            states: Vec::new(),
        }
    }

    /// Completion of the ordinary Group without changing its storage.
    pub(super) fn completion(&mut self) -> GroupCompletion<'_> {
        GroupCompletion {
            plans: &self.plans,
            states: &mut self.states,
        }
    }

    /// Adds the fresh Operation state after its caller has reserved storage.
    fn initialize_states(&self, states: &mut Vec<State>) -> bool {
        for plan in &self.plans {
            let Some(state) = State::new(plan.kind, plan.keeps_values()) else {
                return false;
            };
            states.push(state);
        }
        true
    }

    /// Constructs one complete retained Group using Command allocation checks.
    /// Failed construction exposes no owner with incomplete state.
    pub(super) fn retained_group(&self) -> Result<RetainedGroup<'_>, Failure> {
        let mut states = Vec::new();
        command_memory::reserve(&mut states, self.plans.len())?;
        if !self.initialize_states(&mut states) {
            return Err(unsupported("command memory allocation failed"));
        }
        Ok(RetainedGroup {
            operations: self,
            states,
        })
    }

    /// The bytes one Group's state takes before it keeps any values.
    fn state_bytes(&self) -> usize {
        self.plans
            .iter()
            .map(|plan| {
                std::mem::size_of::<State>()
                    + if plan.keeps_values() || plan.kind == Kind::Wmean {
                        if plan.kind == Kind::Wmean {
                            std::mem::size_of::<weighted::Accumulator>()
                        } else {
                            std::mem::size_of::<Kept>()
                        }
                    } else {
                        0
                    }
            })
            .sum()
    }

    /// At most how many bytes of numbers the Operations keep from each record
    /// of a Group, beyond their fixed-size state. Operations that share
    /// another's samples keep none of their own.
    pub(super) fn kept_numbers(&self) -> usize {
        self.keeping()
            .filter(|plan| !plan.keeps_text())
            .map(|plan| match plan.selector {
                Selector::Single(_) => std::mem::size_of::<numerics::Value>(),
                Selector::Pair { .. } => 2 * std::mem::size_of::<numerics::Value>(),
            })
            .sum()
    }

    /// The fields whose text the Operations keep from each record of a Group:
    /// each field's bytes and one.
    pub(super) fn kept_text(&self) -> impl Iterator<Item = u64> + '_ {
        self.keeping()
            .filter(|plan| plan.keeps_text())
            .filter_map(|plan| match plan.selector {
                Selector::Single(field) => Some(field),
                Selector::Pair { .. } => None,
            })
    }

    /// The Operations that keep values of their own from every record.
    fn keeping(&self) -> impl Iterator<Item = &Plan> {
        self.plans.iter().filter(|plan| {
            plan.keeps_values() && plan.sample_source.is_none() && plan.dispersion_source.is_none()
        })
    }

    /// Clears every Operation's Group state for the next Group.
    #[inline]
    pub(super) fn reset(&mut self) {
        for state in &mut self.states {
            state.reset();
        }
    }

    /// The result of the Operation at `at`.
    pub(super) fn result(
        &mut self,
        at: usize,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<Vec<u8>, Failure> {
        self.completion().result(at, arithmetic, options)
    }

    /// Writes one group's results in request order. Text results go straight
    /// to the output, without copying each result. A completion failure carries
    /// its Operation kind so grouped callers can add applicable context.
    pub(super) fn write_results(
        &mut self,
        output: &mut impl command_output::CommandOutput,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<(), (Kind, Failure)> {
        self.completion().write_results(output, arithmetic, options)
    }

    /// Writes the one result row of an ungrouped calculation. With a paired
    /// Operation each result is written as it is computed, as for a Group, so
    /// a failing paired result follows the results before it; otherwise the
    /// row is written only once every result is known.
    pub(super) fn write_ungrouped_results(
        &mut self,
        output: &mut impl command_output::CommandOutput,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<(), Failure> {
        if self.plans.iter().any(|plan| plan.kind.is_paired()) {
            return self
                .write_results(output, arithmetic, options)
                .map_err(|(_, error)| error);
        }
        let mut fields = Vec::new();
        command_memory::reserve(&mut fields, self.plans.len())?;
        for at in 0..self.plans.len() {
            fields.push(self.result(at, arithmetic, options)?);
        }
        output.row(&fields, options.output)
    }
}

impl DenseGroups<'_> {
    /// Appends one complete Group and returns its insertion index. On failure,
    /// previous Groups stay usable and no partial Group is exposed.
    pub(super) fn add_group(&mut self) -> Option<usize> {
        let width = self.operations.plans.len();
        assert_ne!(width, 0, "retained Groups require Operations");
        command_memory::reserve(&mut self.states, width).ok()?;
        let start = self.states.len();
        if !self.operations.initialize_states(&mut self.states) {
            self.states.truncate(start);
            return None;
        }
        Some(start / width)
    }

    /// The fixed bytes of one Group, including its separately kept stores.
    pub(super) fn group_bytes(&self) -> usize {
        self.operations.state_bytes()
    }

    /// Adds an ordinary-text Record to a Group, preserving request order and
    /// whether this Record must replace the Group's representative.
    pub(super) fn collect(
        &mut self,
        group: usize,
        record: &[u8],
        fields: &mut records::FieldIndex,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
    ) -> Result<bool, Failure> {
        let range = self.range(group);
        collect(
            &mut Whole {
                record,
                index: fields,
            },
            &self.operations.plans,
            &mut self.states[range],
            line,
            options,
            arithmetic,
            None,
        )
    }

    /// Completes the selected retained Group without installing its state in
    /// the ordinary Group or requiring a restoration step.
    pub(super) fn completion(&mut self, group: usize) -> GroupCompletion<'_> {
        let range = self.range(group);
        GroupCompletion {
            plans: &self.operations.plans,
            states: &mut self.states[range],
        }
    }

    fn range(&self, group: usize) -> std::ops::Range<usize> {
        let width = self.operations.plans.len();
        let start = group.checked_mul(width).expect("retained Group index fits");
        let end = start
            .checked_add(width)
            .expect("retained Group extent fits");
        assert!(end <= self.states.len(), "retained Group exists");
        start..end
    }
}

impl RetainedGroup<'_> {
    /// Adds ordinary-text or decoded-CSV Fields in request order, using this
    /// Group's Binding and shared calculations.
    pub(super) fn collect_fields<'r>(
        &mut self,
        record: &mut impl Fields<'r>,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
    ) -> Result<bool, Failure> {
        collect(
            record,
            &self.operations.plans,
            &mut self.states,
            line,
            options,
            arithmetic,
            None,
        )
    }

    /// Completes this Group directly, without installing or restoring state.
    pub(super) fn completion(&mut self) -> GroupCompletion<'_> {
        GroupCompletion {
            plans: &self.operations.plans,
            states: &mut self.states,
        }
    }
}

impl GroupCompletion<'_> {
    /// The number of results, in request order, for checked retained output.
    pub(super) fn len(&self) -> usize {
        self.plans.len()
    }

    fn complete(
        &mut self,
        at: usize,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<Completed<'_>, Failure> {
        complete(
            self.plans,
            self.states,
            at,
            arithmetic,
            options.collapse,
            options.presentation.utf8,
            options.ignore_case,
        )
    }

    /// One rendered result, for output that retains a crosstab cell.
    pub(super) fn result(
        &mut self,
        at: usize,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<Vec<u8>, Failure> {
        self.complete(at, arithmetic, options)
            .and_then(|result| result.render(&options.presentation))
    }

    /// One typed numerical result with exact Count promotion. Text here is
    /// an internal failure rather than an implicitly converted number.
    pub(super) fn numerical_result(
        &mut self,
        at: usize,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<numerics::Value, Failure> {
        match self.complete(at, arithmetic, options)? {
            Completed::Count(count) => Ok(integer(count)),
            Completed::Number(value) => Ok(value),
            Completed::Borrowed(_) | Completed::Text(_) => {
                Err(numeric_failure(numerics::NumericFailure::Invariant))
            }
        }
    }

    /// Writes results progressively in request order, borrowing retained text
    /// for each output call and preserving the failed Operation's kind.
    pub(super) fn write_results(
        &mut self,
        output: &mut impl command_output::CommandOutput,
        arithmetic: &mut numerics::Numerics,
        options: &options::Options,
    ) -> Result<(), (Kind, Failure)> {
        let operation_count = self.plans.len();
        for at in 0..operation_count {
            let kind = self.plans[at].kind;
            let separator = if at + 1 == operation_count {
                options.record_end
            } else {
                options.output
            };
            self.complete(at, arithmetic, options)
                .map_err(|error| (kind, error))?
                .write(output, separator, &options.presentation)
                .map_err(|error| (kind, error))?;
        }
        Ok(())
    }
}

/// What an Operation computes, from which fields, and which earlier
/// Operation's work it shares ([`link_shared_fields`]).
#[derive(Clone, Copy)]
struct Plan {
    kind: Kind,
    selector: Selector,
    sum_source: Option<usize>,
    sample_source: Option<usize>,
    dispersion_source: Option<usize>,
    conversion_source: Option<usize>,
    /// Earlier single-field operations that already parse a pair's left and
    /// right fields in each record, whose values the pair reuses.
    pair_conversion: [Option<usize>; 2],
}

impl Plan {
    /// Whether the Operation keeps values from every record (samples, text
    /// lists, pairs or dispersion) rather than a fixed-size state.
    fn keeps_values(&self) -> bool {
        let kind = self.kind;
        kind.uses_shared_sorted_samples()
            || kind.is_mad()
            || kind.is_moment()
            || kind.is_normality()
            || kind.is_dispersion()
            || kind.keeps_pair_samples()
            || self.keeps_text()
    }

    /// Whether the Operation keeps its field's text from every record.
    fn keeps_text(&self) -> bool {
        matches!(self.kind, Kind::Countunique | Kind::Unique | Kind::Collapse)
    }
}

/// An Operation's state in one Group. The running values come first, so
/// that a record's update of a scalar Operation stays within a cache line,
/// and the values kept from every record live apart, for the Operations
/// that keep them.
#[repr(C)]
struct State {
    value: numerics::Value,
    count: u64,
    parsed: Option<numerics::Value>,
    maximum: numerics::Value,
    text: scalar_text::ScalarText,
    stored: Option<Stored>,
}

/// A request owns either retained observations or fixed-size weighted totals.
/// Sharing conversion or pair-shaped syntax does not choose this storage.
enum Stored {
    Kept(Box<[Kept; 1]>),
    Weighted(Box<[weighted::Accumulator; 1]>),
}

fn boxed_state<T>(value: T) -> Option<Box<[T; 1]>> {
    let mut slot = Vec::new();
    command_memory::reserve_exact(&mut slot, 1).ok()?;
    slot.push(value);
    Box::<[T; 1]>::try_from(slot.into_boxed_slice()).ok()
}

/// The values an Operation keeps from every record, and what it computes
/// from them once.
#[derive(Default)]
struct Kept {
    samples: samples::Samples,
    text_samples: text_samples::TextSamples,
    pair_samples: paired::PairSamples,
    raw_deviation: Option<numerics::Value>,
    moments: moments::Cache,
    dispersion: dispersion::Dispersion,
}

impl State {
    /// Fresh state, with a store for kept values when `keeps`; `None`
    /// without memory for it.
    fn new(kind: Kind, keeps: bool) -> Option<Self> {
        let stored = if kind == Kind::Wmean {
            Some(Stored::Weighted(boxed_state(weighted::Accumulator::new())?))
        } else if keeps {
            Some(Stored::Kept(boxed_state(Kept::default())?))
        } else {
            None
        };
        Some(Self {
            value: integer(0),
            count: 0,
            parsed: None,
            maximum: integer(0),
            text: scalar_text::ScalarText::default(),
            stored,
        })
    }

    fn kept(&self) -> &Kept {
        match self
            .stored
            .as_ref()
            .expect("Operations that keep values own a store")
        {
            Stored::Kept(kept) => &kept[0],
            Stored::Weighted(_) => unreachable!("weighted totals do not retain samples"),
        }
    }

    fn kept_mut(&mut self) -> &mut Kept {
        match self
            .stored
            .as_mut()
            .expect("Operations that keep values own a store")
        {
            Stored::Kept(kept) => &mut kept[0],
            Stored::Weighted(_) => unreachable!("weighted totals do not retain samples"),
        }
    }

    fn weighted(&mut self) -> &mut weighted::Accumulator {
        match self.stored.as_mut().expect("weighted request owns totals") {
            Stored::Weighted(weighted) => &mut weighted[0],
            Stored::Kept(_) => unreachable!("weighted requests own fixed-size totals"),
        }
    }

    fn reset(&mut self) {
        self.value = integer(0);
        self.maximum = integer(0);
        self.count = 0;
        self.parsed = None;
        self.text.clear();
        if let Some(stored) = &mut self.stored {
            let kept = match stored {
                Stored::Kept(kept) => &mut kept[0],
                Stored::Weighted(weighted) => {
                    weighted[0] = weighted::Accumulator::new();
                    return;
                }
            };
            kept.samples.clear();
            kept.raw_deviation = None;
            kept.moments.clear();
            kept.dispersion.clear();
            kept.text_samples.clear();
            kept.pair_samples.reset();
        }
    }
}

fn apply_named(plan: &mut Plan, target: named_fields::Target, field: u64) {
    match (&mut plan.selector, target) {
        (Selector::Single(value), named_fields::Target::Single)
        | (Selector::Pair { left: value, .. }, named_fields::Target::Left)
        | (Selector::Pair { right: value, .. }, named_fields::Target::Right) => *value = field,
        _ => unreachable!("named selector target matches parsed selector"),
    }
}

/// Work that Operations on the same selector share: each Operation of a
/// family reads it from the family's first Operation on its selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Family {
    /// Paired samples (`sample_source`) of paired Operations.
    Pairs,
    /// A single field's conversion (`conversion_source`), which paired
    /// Operations also read for each side (`pair_conversion`).
    Conversion,
    /// Sums (`sum_source`) of `sum` and `mean`.
    Sums,
    /// Dispersion (`dispersion_source`).
    Dispersion,
    /// Sorted samples (`sample_source`) of order statistics and modes.
    SortedSamples,
    /// Deviation samples (`sample_source`) of `madraw` and `mad`.
    Deviation,
    /// Moment samples (`sample_source`) of moments and normality tests.
    Moments,
}

/// How an Operation takes part in a family on a selector.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    /// It shares the family's work.
    Member,
    /// A paired Operation reading the conversion of its left or right field.
    Reads(usize),
}

impl Family {
    /// The families whose work `kind` shares, besides paired samples.
    fn of(kind: Kind) -> impl Iterator<Item = Self> {
        let shared = if matches!(kind, Kind::Sum | Kind::Mean) {
            Some(Self::Sums)
        } else if kind.is_dispersion() {
            Some(Self::Dispersion)
        } else if kind.uses_shared_sorted_samples() {
            Some(Self::SortedSamples)
        } else if kind.is_mad() {
            Some(Self::Deviation)
        } else if kind.is_moment() || kind.is_normality() {
            Some(Self::Moments)
        } else {
            None
        };
        kind.converts_numeric_field()
            .then_some(Self::Conversion)
            .into_iter()
            .chain(shared)
    }
}

/// Links each Operation to the first earlier Operation whose work it shares:
/// entries sorted by family, selector and request order put that Operation
/// first in each run, in O(n log n) for n Operations.
fn link_shared_fields(operations: &mut [Plan]) -> Result<(), Failure> {
    let selector = |selector| match selector {
        Selector::Single(field) => (0, field, 0),
        Selector::Pair { left, right } => (1, left, right),
    };
    let mut entries = Vec::new();
    command_memory::reserve(
        &mut entries,
        operations
            .len()
            .checked_mul(3)
            .ok_or_else(|| unsupported("command memory allocation failed"))?,
    )?;
    for (index, operation) in operations.iter_mut().enumerate() {
        operation.sum_source = None;
        operation.sample_source = None;
        operation.dispersion_source = None;
        operation.conversion_source = None;
        operation.pair_conversion = [None, None];
        if let Selector::Pair { left, right } = operation.selector {
            if operation.kind.keeps_pair_samples() {
                entries.push((
                    Family::Pairs,
                    selector(operation.selector),
                    index,
                    Role::Member,
                ));
            }
            for (side, field) in [left, right].into_iter().enumerate() {
                entries.push((
                    Family::Conversion,
                    selector(Selector::Single(field)),
                    index,
                    Role::Reads(side),
                ));
            }
            continue;
        }
        for family in Family::of(operation.kind) {
            entries.push((family, selector(operation.selector), index, Role::Member));
        }
    }
    entries.sort_unstable();
    for run in entries.chunk_by(|a, b| (a.0, a.1) == (b.0, b.1)) {
        // The run's first member so far: the earliest Operation doing the work.
        let mut first = None;
        for &(family, _, index, role) in run {
            let operation = &mut operations[index];
            match role {
                Role::Reads(side) => operation.pair_conversion[side] = first,
                Role::Member if first.is_none() => first = Some(index),
                Role::Member => match family {
                    Family::Conversion => operation.conversion_source = first,
                    Family::Sums => operation.sum_source = first,
                    Family::Dispersion => operation.dispersion_source = first,
                    Family::Pairs | Family::SortedSamples | Family::Deviation | Family::Moments => {
                        operation.sample_source = first
                    }
                },
            }
        }
    }
    Ok(())
}

// Inlined into each record source's one caller (`OperationSet::collect`,
// `collect_fields`), where the source's fields stay in registers.
#[inline(always)]
fn collect<'r>(
    record: &mut impl Fields<'r>,
    plans: &[Plan],
    states: &mut [State],
    line: u64,
    options: &options::Options,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
) -> Result<bool, Failure> {
    let mut keep_line = false;
    let mut random = random;
    for (index, plan) in plans.iter().enumerate() {
        let (previous, remaining) = states.split_at_mut(index);
        let state = &mut remaining[0];
        if let Some(source) = plan.dispersion_source {
            state.count = previous[source].count;
            continue;
        }
        if let Some(source) = plan.sample_source {
            state.count = previous[source].count;
            continue;
        }
        if let Some(source) = plan.sum_source {
            state.value = previous[source].value;
            state.count = previous[source].count;
            continue;
        }
        if let Selector::Pair { left, right } = plan.selector {
            let [left_source, right_source] = plan.pair_conversion;
            let mut parse = |source: Option<usize>, field| match source {
                Some(source) => Ok(previous[source].parsed),
                None => record.number(field, line, options),
            };
            if plan.kind == Kind::Wmean {
                let value = parse(left_source, left)?;
                let weight = parse(right_source, right)?;
                state.weighted().push(value, weight, right, line)?;
                continue;
            }
            let samples = &mut state.kept_mut().pair_samples;
            if let Some(value) = parse(left_source, left)? {
                samples
                    .push_left(value)
                    .map_err(|error| sample_failure(plan.kind, error))?;
            }
            if let Some(value) = parse(right_source, right)? {
                samples
                    .push_right(value)
                    .map_err(|error| sample_failure(plan.kind, error))?;
            }
            continue;
        }
        let Selector::Single(field) = plan.selector else {
            unreachable!("paired selectors were handled first")
        };
        if matches!(
            plan.kind,
            Kind::Path(_)
                | Kind::Checksum(_)
                | Kind::Base64
                | Kind::Debase64
                | Kind::Getnum(_)
                | Kind::Strbin(_)
        ) {
            let bytes = record.text(field, line, options)?;
            if !(options.narm && field_policy::is_na(bytes)) {
                collect_row_text(plan, state, bytes, line, field, options)?;
            }
            continue;
        }
        if matches!(
            plan.kind,
            Kind::Count
                | Kind::Countunique
                | Kind::Unique
                | Kind::Collapse
                | Kind::Cut
                | Kind::First
                | Kind::Last
                | Kind::Rand
        ) {
            let bytes = record.text(field, line, options)?;
            if collect_text(plan, state, bytes, options.narm, random.as_deref_mut())? {
                keep_line = true;
            }
            continue;
        }
        // The first numeric request owns conversion, not the transformed accumulator.
        // Store None too, so a skipped NA never reuses the previous record's value.
        let parsed = match plan.conversion_source {
            Some(source) => previous[source].parsed,
            None => record.number(field, line, options)?,
        };
        state.parsed = parsed;
        let Some(value) = parsed else {
            continue;
        };
        if matches!(plan.kind, Kind::Rounding(_) | Kind::Bin(_)) {
            collect_row_number(plan, state, value)?;
            continue;
        }
        state.count = state
            .count
            .checked_add(1)
            .ok_or_else(|| unsupported("operation count exceeds u64 limit"))?;
        if plan.kind.is_dispersion() {
            state
                .kept_mut()
                .dispersion
                .push(value)
                .map_err(|error| sample_failure(plan.kind, error))?;
            continue;
        }
        if plan.kind.uses_shared_sorted_samples()
            || plan.kind.is_mad()
            || plan.kind.is_moment()
            || plan.kind.is_normality()
        {
            state
                .kept_mut()
                .samples
                .push_growable(value)
                .map_err(|error| sample_failure(plan.kind, error))?;
            continue;
        }
        if plan.kind == Kind::Range {
            if state.count == 1 {
                state.value = value;
                state.maximum = value;
            } else {
                if numerics::compare(value, state.value) == numerics::Comparison::Less {
                    state.value = value;
                }
                if numerics::compare(value, state.maximum) == numerics::Comparison::Greater {
                    state.maximum = value;
                }
            }
            continue;
        }
        if matches!(
            plan.kind,
            Kind::Min | Kind::Max | Kind::Absmin | Kind::Absmax
        ) {
            if state.count == 1 {
                state.value = value;
            }
            let relation = if matches!(plan.kind, Kind::Absmin | Kind::Absmax) {
                numerics::compare_magnitude(value, state.value)
            } else {
                numerics::compare(value, state.value)
            };
            if matches!(
                (plan.kind, relation),
                (Kind::Min | Kind::Absmin, numerics::Comparison::Less)
                    | (Kind::Max | Kind::Absmax, numerics::Comparison::Greater)
            ) {
                state.value = value;
                keep_line = true;
            }
            continue;
        }
        let value = if matches!(plan.kind, Kind::Ms | Kind::Rms) {
            decimal::multiply(value, value)?
        } else if plan.kind == Kind::Geomean {
            arithmetic.mean_log(value).map_err(numeric_failure)?
        } else if plan.kind == Kind::Harmmean {
            decimal::divide(integer(1), value)?
        } else {
            value
        };
        state.value = decimal::add(state.value, value)?;
    }
    Ok(keep_line)
}

/// Adds a record's field text to a per-row Operation that transforms text
/// (paths, checksums, base64, getnum, strbin): its result for that row. Kept
/// out of the collector, which aggregating Operations run for every record.
#[inline(never)]
fn collect_row_text(
    plan: &Plan,
    state: &mut State,
    bytes: &[u8],
    line: u64,
    field: u64,
    options: &options::Options,
) -> Result<(), Failure> {
    match plan.kind {
        Kind::Path(kind) => state
            .text
            .replace(kind.select(bytes))
            .map_err(scalar_text_failure)?,
        Kind::Checksum(algorithm) => algorithm.store(bytes, &mut state.text)?,
        Kind::Base64 | Kind::Debase64 => base64_fields::transform(
            &mut state.text,
            bytes,
            plan.kind == Kind::Debase64,
            line,
            field,
        )?,
        Kind::Getnum(kind) => {
            state.value = line_numeric::extract(bytes, kind, options.presentation.profile)?
        }
        Kind::Strbin(buckets) => state.value = integer(line_numeric::strbin(bytes, buckets)),
        _ => unreachable!("per-row text Operations"),
    }
    state.count = 1;
    Ok(())
}

/// Adds a record's number to a per-row Operation that transforms numbers
/// (rounding, bin), kept out of the collector as [`collect_row_text`] is.
#[inline(never)]
fn collect_row_number(
    plan: &Plan,
    state: &mut State,
    value: numerics::Value,
) -> Result<(), Failure> {
    state.value = match plan.kind {
        Kind::Rounding(kind) => line_numeric::rounding(value, kind)?,
        Kind::Bin(width) => line_numeric::bin(value, width)?,
        _ => unreachable!("per-row numeric Operations"),
    };
    state.count = 1;
    Ok(())
}

// Inlined into each record source's `collect`, as it was into the one before.
#[inline(always)]
fn collect_text(
    plan: &Plan,
    state: &mut State,
    bytes: &[u8],
    narm: bool,
    random: Option<&mut random::RandomState>,
) -> Result<bool, Failure> {
    if narm && field_policy::is_na(bytes) {
        return Ok(false);
    }
    state.count = state
        .count
        .checked_add(1)
        .ok_or_else(|| unsupported("operation count exceeds u64 limit"))?;
    if matches!(plan.kind, Kind::Countunique | Kind::Unique | Kind::Collapse) {
        state
            .kept_mut()
            .text_samples
            .push(bytes)
            .map_err(|error| text_sample_failure(plan.kind, error))?;
    }
    select_scalar(plan, state, bytes, random)
}

fn select_scalar(
    plan: &Plan,
    state: &mut State,
    bytes: &[u8],
    random: Option<&mut random::RandomState>,
) -> Result<bool, Failure> {
    let selected = match plan.kind {
        Kind::Cut | Kind::Last => true,
        Kind::First => !state.text.is_present(),
        Kind::Rand => {
            let draw = random
                .expect("rand operations require initialized state")
                .draw();
            state.count == 1 || u64::from(draw) % state.count == 0
        }
        _ => false,
    };
    if selected {
        state.text.replace(bytes).map_err(scalar_text_failure)?;
    }
    Ok(selected)
}

fn text_sample_failure(kind: Kind, error: text_samples::Error) -> Failure {
    let reason = match error {
        text_samples::Error::Allocation => "text memory allocation failed",
    };
    unsupported(&format!("{} {reason}", kind.name()))
}

fn quartile(
    sorted: &[numerics::Value],
    which: samples::Quartile,
) -> Result<numerics::Value, Failure> {
    use fastmash_numeric_contract::Raw80;
    match which.select(sorted) {
        Some(samples::Selection::Single(value)) => Ok(value),
        Some(samples::Selection::Between {
            lower,
            upper,
            quarters,
        }) => {
            let raw = match quarters {
                0 => Raw80::new(0, 0),
                1 => Raw80::new(0x3ffd, 1 << 63),
                2 => Raw80::new(0x3ffe, 1 << 63),
                3 => Raw80::new(0x3ffe, 0xc000_0000_0000_0000),
                _ => unreachable!("quarter remainder is less than four"),
            };
            let fraction = numerics::canonical(raw);
            // Preserve GNU's arithmetic even at fraction zero (infinities/NaNs).
            let delta = decimal::subtract(upper, lower)?;
            let product = decimal::multiply(delta, fraction)?;
            decimal::add(lower, product)
        }
        None => Err(unsupported("internal quartile sample state missing")),
    }
}

fn middle_value(
    upper: numerics::Value,
    lower: Option<numerics::Value>,
) -> Result<numerics::Value, Failure> {
    if let Some(lower) = lower {
        let half = numerics::canonical(fastmash_numeric_contract::Raw80::new(0x3ffe, 1 << 63));
        decimal::multiply(decimal::add(upper, lower)?, half)
    } else {
        Ok(upper)
    }
}

fn format_count(count: u64, presentation: &presentation::Presentation) -> Result<Vec<u8>, Failure> {
    // Default14 renders these exact integers without rounding or an exponent.
    if presentation.spec.is_none() && count < 100_000_000_000_000 {
        let mut digits = [0u8; 14];
        let mut start = digits.len();
        let mut remaining = count;
        loop {
            start -= 1;
            digits[start] = b'0' + (remaining % 10) as u8;
            remaining /= 10;
            if remaining == 0 {
                break;
            }
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(digits.len() - start)
            .map_err(|_| conversion_failure(fastmash_conversion::profile::Error::Allocation))?;
        output.extend_from_slice(&digits[start..]);
        return Ok(output);
    }
    presentation.render(numerics::Numerics::value80(integer(count)))
}

pub(super) fn moment_value(
    kind: Kind,
    values: &[numerics::Value],
    arithmetic: &mut numerics::Numerics,
    cache: &mut moments::Cache,
) -> Result<numerics::Value, Failure> {
    match kind {
        Kind::Jarque => jarque_bera(values, arithmetic, cache),
        Kind::Dpo => dagostino_pearson(values, arithmetic, cache),
        Kind::Pskew | Kind::Sskew => skewness(values, kind == Kind::Sskew, arithmetic, cache),
        Kind::Pkurt | Kind::Skurt => {
            excess_kurtosis(values, kind == Kind::Skurt, arithmetic, cache)
        }
        _ => unreachable!("moment finalization requires a moment or normality operation"),
    }
}

/// Retained text result (`N/A` or empty when none); `None` for non-text kinds.
fn text_result<'s>(plan: &Plan, state: &'s State) -> Option<&'s [u8]> {
    let absent: &[u8] = match plan.kind {
        Kind::Cut | Kind::First | Kind::Last | Kind::Rand => b"N/A",
        Kind::Base64 | Kind::Debase64 | Kind::Checksum(_) | Kind::Path(_) => b"",
        _ => return None,
    };
    Some(state.text.output().unwrap_or(absent))
}

/// Completed values retain their meaning until presentation. Borrowed text
/// keeps the ordinary output path from copying every selected text value.
enum Completed<'s> {
    Count(u64),
    Number(numerics::Value),
    Borrowed(&'s [u8]),
    Text(Vec<u8>),
}

impl Completed<'_> {
    fn render(self, presentation: &presentation::Presentation) -> Result<Vec<u8>, Failure> {
        match self {
            Self::Count(count) => format_count(count, presentation),
            Self::Number(value) => presentation.render(numerics::Numerics::value80(value)),
            Self::Text(bytes) => Ok(bytes),
            Self::Borrowed(bytes) => {
                let mut result = Vec::new();
                result
                    .try_reserve_exact(bytes.len())
                    .map_err(|_| scalar_text_failure(scalar_text::Error::Allocation))?;
                result.extend_from_slice(bytes);
                Ok(result)
            }
        }
    }

    fn write(
        self,
        output: &mut impl command_output::CommandOutput,
        separator: u8,
        presentation: &presentation::Presentation,
    ) -> Result<(), Failure> {
        match self {
            Self::Borrowed(bytes) => output.result(bytes, separator),
            Self::Text(bytes) => output.result(&bytes, separator),
            result => output.result(&result.render(presentation)?, separator),
        }
        Ok(())
    }
}

/// The result of the Operation at `at`. Paired, moment, normality,
/// dispersion, ordered-sample and MAD results read the work (samples, pairs,
/// dispersion, cached moments or deviation) they may share with another
/// Operation through its source index; the rest summarize their own state.
fn complete<'s>(
    plans: &[Plan],
    states: &'s mut [State],
    at: usize,
    arithmetic: &mut numerics::Numerics,
    collapse: u8,
    utf8: bool,
    ignore_case: bool,
) -> Result<Completed<'s>, Failure> {
    let plan = &plans[at];
    if let Selector::Pair { left, right } = plan.selector {
        if plan.kind == Kind::Wmean {
            let value = states[at].weighted().result(left, right)?;
            return Ok(Completed::Number(value));
        }
        let kind = match plan.kind {
            Kind::Pcov => paired::Kind::Covariance { sample: false },
            Kind::Scov => paired::Kind::Covariance { sample: true },
            Kind::Ppearson => paired::Kind::Pearson { sample: false },
            Kind::Spearson => paired::Kind::Pearson { sample: true },
            Kind::Dotprod => paired::Kind::DotProduct,
            _ => unreachable!("paired selector has a paired operation kind"),
        };
        let owner = &states[plan.sample_source.unwrap_or(at)];
        let value = owner
            .kept()
            .pair_samples
            .summarize(kind, plan.kind, left, right, arithmetic, utf8)?;
        return Ok(Completed::Number(value));
    }
    if plan.kind.is_moment() || plan.kind.is_normality() {
        let kind = plan.kind;
        let owner = states[plan.sample_source.unwrap_or(at)].kept_mut();
        let value = moment_value(
            kind,
            owner.samples.as_slice(),
            arithmetic,
            &mut owner.moments,
        )?;
        return Ok(Completed::Number(value));
    }
    if plan.kind.is_dispersion() {
        let kind = plan.kind;
        let source = plan.dispersion_source.unwrap_or(at);
        let value = states[source]
            .kept_mut()
            .dispersion
            .variance(matches!(kind, Kind::Svar | Kind::Sstdev))?;
        let value = if matches!(kind, Kind::Pstdev | Kind::Sstdev) {
            arithmetic.sqrt(value).map_err(numeric_failure)?
        } else {
            value
        };
        return Ok(Completed::Number(value));
    }
    if !plan.kind.uses_shared_sorted_samples() && !plan.kind.is_mad() {
        return complete_own_state(plan, &mut states[at], arithmetic, collapse, ignore_case);
    }
    let kind = plan.kind;
    if states[at].count == 0 {
        return Ok(Completed::Number(super::quiet_nan()));
    }
    let source = plan.sample_source.unwrap_or(at);
    if kind.is_mad() {
        let owner = states[source].kept_mut();
        let deviation = match owner.raw_deviation {
            Some(value) => value,
            None => {
                let value = robust_statistics::deviation(&mut owner.samples, kind)?;
                owner.raw_deviation = Some(value);
                value
            }
        };
        let value = robust_statistics::scale(deviation, kind == Kind::Mad)?;
        return Ok(Completed::Number(value));
    }
    let sorted = states[source]
        .kept_mut()
        .samples
        .sorted_once()
        .map_err(|error| sample_failure(kind, error))?;
    let value = match kind {
        Kind::Mode | Kind::Antimode => samples::mode(sorted, kind == Kind::Antimode)
            .ok_or_else(|| unsupported("internal mode sample state missing"))?,
        Kind::Median => {
            let upper = sorted[sorted.len() / 2];
            let lower = (sorted.len() % 2 == 0).then(|| sorted[sorted.len() / 2 - 1]);
            middle_value(upper, lower)?
        }
        Kind::Q1 => quartile(sorted, samples::Quartile::First)?,
        Kind::Q3 => quartile(sorted, samples::Quartile::Third)?,
        Kind::Iqr => {
            let upper = quartile(sorted, samples::Quartile::Third)?;
            let lower = quartile(sorted, samples::Quartile::First)?;
            decimal::subtract(upper, lower)?
        }
        Kind::Percentile(percent) => ordered_statistics::percentile(sorted, percent)?,
        Kind::Trimmean(trim) if trim.is_half() => {
            let upper = sorted[sorted.len() / 2];
            let lower = (sorted.len() % 2 == 0).then(|| sorted[sorted.len() / 2 - 1]);
            middle_value(upper, lower)?
        }
        Kind::Trimmean(trim) => ordered_statistics::trimmed(sorted, trim)?,
        _ => unreachable!("ordered-sample finalizer has a supported kind"),
    };
    Ok(Completed::Number(value))
}

/// The result of an Operation that reads only its own state: text, counts
/// and running values. [`complete`] handles every Operation that may read
/// work shared with another Operation first.
fn complete_own_state<'s>(
    plan: &Plan,
    state: &'s mut State,
    arithmetic: &mut numerics::Numerics,
    collapse: u8,
    ignore_case: bool,
) -> Result<Completed<'s>, Failure> {
    if plan.kind == Kind::Unique {
        return state
            .kept_mut()
            .text_samples
            .unique(collapse, ignore_case)
            .map(Completed::Text)
            .map_err(|error| text_sample_failure(plan.kind, error));
    }
    if plan.kind == Kind::Collapse {
        return state
            .kept_mut()
            .text_samples
            .collapse(collapse)
            .map(Completed::Text)
            .map_err(|error| text_sample_failure(plan.kind, error));
    }
    if plan.kind == Kind::Countunique && state.count != 0 {
        return Ok(Completed::Count(
            state
                .kept_mut()
                .text_samples
                .count_unique(ignore_case)
                .map_err(|error| text_sample_failure(plan.kind, error))? as u64,
        ));
    }
    if let Some(bytes) = text_result(plan, state) {
        return Ok(Completed::Borrowed(bytes));
    }
    if state.count == 0 {
        return Ok(Completed::Number(match plan.kind {
            Kind::Sum | Kind::Count | Kind::Countunique => integer(0),
            Kind::Min | Kind::Absmin => {
                numerics::canonical(fastmash_numeric_contract::Raw80::new(0xffff, 1 << 63))
            }
            Kind::Max | Kind::Absmax => {
                numerics::canonical(fastmash_numeric_contract::Raw80::new(0x7fff, 1 << 63))
            }
            _ => super::quiet_nan(),
        }));
    }
    if plan.kind == Kind::Count {
        return Ok(Completed::Count(state.count));
    }
    let value = if plan.kind == Kind::Range {
        decimal::subtract(state.maximum, state.value)?
    } else if matches!(plan.kind, Kind::Mean | Kind::Geomean | Kind::Ms | Kind::Rms) {
        decimal::mean(state.value, state.count)?
    } else if plan.kind == Kind::Harmmean {
        decimal::divide(integer(state.count), state.value)?
    } else {
        state.value
    };
    let value = if plan.kind == Kind::Geomean {
        arithmetic.mean_exp(value).map_err(numeric_failure)?
    } else if plan.kind == Kind::Rms {
        arithmetic.sqrt(value).map_err(numeric_failure)?
    } else {
        value
    };
    Ok(Completed::Number(value))
}
