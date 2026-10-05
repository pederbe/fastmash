//! Private bounded Record selection, independent of the source Record format.
use super::{
    Failure, binding, buffered_stdout::BufferedStdout, command_memory, command_output, csv_input,
    decimal, failure, field_policy, indexed_field, intake, numeric_failure, numerics,
    options::Options, os_failure, projected_sort, records, text_order,
};
use std::{cmp::Ordering, io::BufRead};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Highest,
    Lowest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub direction: Direction,
    pub count: u64,
}

struct Candidate<R> {
    rank: numerics::Value,
    ordinal: u64,
    record: R,
}

/// A heap whose root is the worst retained candidate. Record ownership is
/// acquired only when a candidate enters the selection.
pub(super) struct Collector<R> {
    selection: Selection,
    candidates: Vec<Candidate<R>>,
}

impl<R> Collector<R> {
    pub(super) fn new(selection: Selection) -> Self {
        Self {
            selection,
            candidates: Vec::new(),
        }
    }

    fn order(
        direction: Direction,
        left: (numerics::Value, u64),
        right: (numerics::Value, u64),
    ) -> Result<Ordering, Failure> {
        let numeric = match numerics::compare(left.0, right.0) {
            numerics::Comparison::Less => Ordering::Less,
            numerics::Comparison::Equal => Ordering::Equal,
            numerics::Comparison::Greater => Ordering::Greater,
            numerics::Comparison::Unordered => {
                return Err(numeric_failure(numerics::NumericFailure::Invariant));
            }
        };
        let numeric = match direction {
            Direction::Highest => numeric.reverse(),
            Direction::Lowest => numeric,
        };
        Ok(numeric.then_with(|| left.1.cmp(&right.1)))
    }

    fn compare(&self, left: usize, right: usize) -> Result<Ordering, Failure> {
        let a = &self.candidates[left];
        let b = &self.candidates[right];
        Self::order(
            self.selection.direction,
            (a.rank, a.ordinal),
            (b.rank, b.ordinal),
        )
    }

    pub(super) fn offer(
        &mut self,
        rank: numerics::Value,
        ordinal: u64,
        record: impl FnOnce() -> Result<R, Failure>,
    ) -> Result<(), Failure> {
        let full = self.candidates.len() as u64 == self.selection.count;
        if full {
            let worst = &self.candidates[0];
            if Self::order(
                self.selection.direction,
                (rank, ordinal),
                (worst.rank, worst.ordinal),
            )? != Ordering::Less
            {
                return Ok(());
            }
        }
        let candidate = Candidate {
            rank,
            ordinal,
            record: record()?,
        };
        if full {
            self.candidates[0] = candidate;
            self.sift_down(self.candidates.len())?;
        } else {
            if self.candidates.len() == self.candidates.capacity() {
                command_memory::reserve(&mut self.candidates, 1)?;
            }
            self.candidates.push(candidate);
            let mut child = self.candidates.len() - 1;
            while child != 0 {
                let parent = (child - 1) / 2;
                if self.compare(child, parent)? != Ordering::Greater {
                    break;
                }
                self.candidates.swap(child, parent);
                child = parent;
            }
        }
        Ok(())
    }

    fn sift_down(&mut self, length: usize) -> Result<(), Failure> {
        let mut parent = 0;
        // Testing for an internal node first keeps 2*parent+1 in range.
        while parent < length / 2 {
            let mut child = parent * 2 + 1;
            if child + 1 < length && self.compare(child + 1, child)? == Ordering::Greater {
                child += 1;
            }
            if self.compare(child, parent)? != Ordering::Greater {
                break;
            }
            self.candidates.swap(child, parent);
            parent = child;
        }
        Ok(())
    }

    pub(super) fn emit(
        mut self,
        mut output: impl FnMut(R) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        // Heap sorting gives best-first order without a second candidate array
        // or the allocation required by a stable general-purpose sort.
        for length in (2..=self.candidates.len()).rev() {
            self.candidates.swap(0, length - 1);
            self.sift_down(length - 1)?;
        }
        for candidate in self.candidates {
            output(candidate.record)?;
        }
        Ok(())
    }
}

pub(super) fn run(
    reader: &mut (impl BufRead + ?Sized),
    writer: &mut impl command_output::Transport,
    options: &Options,
    selection: Selection,
    binding: binding::Selection,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let (capacity, line_buffered) = writer.buffering();
    let mut output = BufferedStdout::new(writer, capacity, line_buffered)
        .map_err(|error| os_failure(&error, false))?;
    let result = if options.csv_in {
        collect_decoded(reader, &mut output, options, selection, binding)
    } else {
        collect(reader, &mut output, options, selection, binding)
    };
    Ok(command_output::complete(
        output,
        result,
        command_output::Transport::close,
        report,
    ))
}

fn collect(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    options: &Options,
    selection: Selection,
    mut binding: binding::Selection,
) -> Result<(), Failure> {
    let mut collector = Collector::new(selection);
    let mut intake = intake::Intake::new(options, intake::Header::of(options));
    let mut buffer = Vec::new();
    let header = intake.header(reader, &mut buffer, |record| {
        binding.header(record, options)
    })?;
    if header && options.header_out {
        copied_header(output, &buffer, options);
    }
    let mut groups = Adjacent::new(binding.keys().len())?;
    let mut index =
        records::FieldIndex::selecting(binding.keys().iter().copied().chain([binding.ranking()]));
    let mut visit = |record: &[u8], ordinal: u64, phase| {
        if phase == projected_sort::RecordPhase::ObserveSource {
            if ordinal == 1 && options.header_out {
                copied_header(output, record, options);
            }
            return Ok(());
        }
        index.clear();
        if groups.observe(record, &mut index, binding.keys(), ordinal, options)? {
            let completed = std::mem::replace(&mut collector, Collector::new(selection));
            emit(completed, output, options)?;
        }
        let field = binding.ranking();
        let span = indexed_field(record, &mut index, field, ordinal, options.input)?;
        let Some(rank) = decimal::field(
            record,
            span,
            field,
            ordinal,
            options.narm,
            options.locale.numeric,
        )?
        else {
            return Ok(());
        };
        ordered_rank(rank, field, ordinal)?;
        collector.offer(rank, ordinal, || {
            let mut bytes = Vec::new();
            command_memory::reserve(&mut bytes, record.len())?;
            bytes.extend_from_slice(record);
            Ok(bytes)
        })
    };
    if options.sort && !binding.keys().is_empty() {
        projected_sort::sorted_records(reader, intake, options, binding.keys(), &mut visit)?;
    } else {
        while let Some(record) = intake.next(reader, &mut buffer)? {
            let ordinal = intake.line();
            visit(
                record.data(),
                ordinal,
                projected_sort::RecordPhase::ObserveSource,
            )?;
            visit(record.data(), ordinal, projected_sort::RecordPhase::Consume)?;
        }
        // A read failure must not publish the incomplete domain's winners.
        intake.finish()?;
    }
    emit(collector, output, options)
}

fn collect_decoded(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    options: &Options,
    selection: Selection,
    mut binding: binding::Selection,
) -> Result<(), Failure> {
    let mut intake = csv_input::Intake::new();
    let mut record = csv_input::Record::default();
    let mut selected = Decoded::new(selection, binding.keys().len())?;
    let mut sorting = (options.sort && !binding.keys().is_empty())
        .then(|| projected_sort::DecodedSorting::new(options))
        .transpose()?;
    while intake.next(reader, &mut record)? {
        if !observe_decoded(&record, output, options, &mut binding)? {
            if sorting.is_some() {
                // A wide header's spare buffers do not belong to data chunks.
                record = csv_input::Record::default();
            }
            continue;
        }
        if let Some(sorting) = &mut sorting {
            sorting.push(&mut record, binding.keys(), options)?;
        } else {
            record = selected
                .push(
                    csv_input::Retained::Owned(record),
                    output,
                    options,
                    &binding,
                )?
                .unwrap_or_default();
        }
    }
    intake.finish()?;
    drop(record);
    if let Some(mut sorting) = sorting {
        sorting.emit(binding.keys(), options, |record| {
            selected.push(record, output, options, &binding).map(|_| ())
        })?;
    }
    selected.finish(output, options)
}

/// Source observation stays separate from consumption so sorting cannot
/// change the copied header's schema or its Binding.
fn observe_decoded(
    record: &csv_input::Record,
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    options: &Options,
    binding: &mut binding::Selection,
) -> Result<bool, Failure> {
    if record.location.record == 1 {
        if options.header_in {
            binding.decoded_header(record, options)?;
        }
        if options.header_out {
            copied_header_fields(output, record.fields(), options);
        }
        if options.header_in {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Consumes complete decoded Records with their original Location. Intake
/// and, when used, sorting remain responsible for validating the full source.
struct Decoded {
    selection: Selection,
    collector: Collector<csv_input::Record>,
    groups: Adjacent,
}

impl Decoded {
    fn new(selection: Selection, keys: usize) -> Result<Self, Failure> {
        Ok(Self {
            selection,
            collector: Collector::new(selection),
            groups: Adjacent::new(keys)?,
        })
    }

    fn push(
        &mut self,
        record: csv_input::Retained<'_>,
        output: &mut BufferedStdout<'_, impl std::io::Write>,
        options: &Options,
        binding: &binding::Selection,
    ) -> Result<Option<csv_input::Record>, Failure> {
        let view = record.view();
        if self
            .groups
            .observe_fields(binding.keys(), |key| view.field(key), options)
            .map_err(|error| view.location.annotate(error))?
        {
            let completed = std::mem::replace(&mut self.collector, Collector::new(self.selection));
            Self::emit(completed, output, options)?;
        }
        let field = binding.ranking();
        let ordinal = view.location.record;
        let bytes = view
            .field(field)
            .map_err(|error| view.location.annotate(error))?;
        let rank = decimal::number(
            bytes,
            field_policy::FieldRange {
                start: 0,
                length: bytes.len(),
            },
            options.narm,
            options.presentation.profile,
        )
        .map_err(|error| view.location.annotate(error.report(bytes, field, ordinal)))?;
        let Some(rank) = rank else {
            return Ok(record.reusable());
        };
        ordered_rank(rank, field, ordinal).map_err(|error| view.location.annotate(error))?;
        let mut record = Some(record);
        self.collector
            .offer(rank, ordinal, || record.take().unwrap().into_owned())?;
        Ok(record.and_then(csv_input::Retained::reusable))
    }

    fn finish(
        self,
        output: &mut BufferedStdout<'_, impl std::io::Write>,
        options: &Options,
    ) -> Result<(), Failure> {
        Self::emit(self.collector, output, options)
    }

    fn emit(
        collector: Collector<csv_input::Record>,
        output: &mut BufferedStdout<'_, impl std::io::Write>,
        options: &Options,
    ) -> Result<(), Failure> {
        collector.emit(|record| {
            copied_record(output, record.fields(), options);
            Ok(())
        })
    }
}

fn ordered_rank(rank: numerics::Value, field: u64, ordinal: u64) -> Result<(), Failure> {
    if matches!(
        numerics::Numerics::value80(rank).classify(),
        fastmash_numeric_contract::ValueClass::Nan { .. }
    ) {
        return Err(failure(
            format!(
                "invalid input: field {field} in line {ordinal} has unordered NaN ranking value\n"
            )
            .into_bytes(),
        ));
    }
    Ok(())
}

/// Only the current Group's keys are retained, independently of its winners.
/// Every required key is located before equality can short-circuit or narm omit.
struct Adjacent {
    keys: Vec<Vec<u8>>,
    started: bool,
}

impl Adjacent {
    fn new(count: usize) -> Result<Self, Failure> {
        let mut keys = Vec::new();
        if count != 0 {
            command_memory::reserve(&mut keys, count)?;
            keys.resize_with(count, Vec::new);
        }
        Ok(Self {
            keys,
            started: false,
        })
    }

    fn observe(
        &mut self,
        record: &[u8],
        index: &mut records::FieldIndex,
        keys: &[u64],
        line: u64,
        options: &Options,
    ) -> Result<bool, Failure> {
        self.observe_fields(
            keys,
            |field| {
                let span = indexed_field(record, index, field, line, options.input)?;
                Ok(&record[span.start..span.start + span.length])
            },
            options,
        )
    }

    fn observe_fields<'r>(
        &mut self,
        keys: &[u64],
        mut field: impl FnMut(u64) -> Result<&'r [u8], Failure>,
        options: &Options,
    ) -> Result<bool, Failure> {
        if keys.is_empty() {
            return Ok(false);
        }
        let mut changed = !self.started;
        for (prior, &key) in self.keys.iter().zip(keys) {
            let bytes = field(key)?;
            changed |= !text_order::same_group(prior, bytes, options.ignore_case);
        }
        if changed {
            for (saved, &key) in self.keys.iter_mut().zip(keys) {
                let bytes = field(key)?;
                saved.clear();
                if saved.capacity() < bytes.len() {
                    command_memory::reserve(saved, bytes.len())?;
                }
                saved.extend_from_slice(bytes);
            }
            self.started = true;
        }
        Ok(changed)
    }
}

fn emit(
    collector: Collector<Vec<u8>>,
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    options: &Options,
) -> Result<(), Failure> {
    collector.emit(|record| {
        copied_record(
            output,
            records::fields(&record, options.input)
                .map(|span| &record[span.start..span.start + span.length]),
            options,
        );
        Ok(())
    })
}

fn copied_header(
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    record: &[u8],
    options: &Options,
) {
    copied_header_fields(
        output,
        records::fields(record, options.input)
            .map(|span| &record[span.start..span.start + span.length]),
        options,
    );
}

fn copied_header_fields<'r>(
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    fields: impl Iterator<Item = &'r [u8]>,
    options: &Options,
) {
    let mut fields = fields.enumerate().peekable();
    if fields.peek().is_none() {
        output.emit(super::headers::Part::Separator(options.record_end));
    }
    while let Some((index, bytes)) = fields.next() {
        if options.header_in {
            copied_field(
                output,
                super::headers::label(bytes),
                fields.peek().is_none(),
                options,
            );
        } else {
            copied_field(
                output,
                super::headers::generated(index as u64 + 1).as_bytes(),
                fields.peek().is_none(),
                options,
            );
        }
    }
}

fn copied_record<'r>(
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    fields: impl Iterator<Item = &'r [u8]>,
    options: &Options,
) {
    let mut fields = fields.peekable();
    while let Some(bytes) = fields.next() {
        copied_field(output, bytes, fields.peek().is_none(), options);
    }
}

fn copied_field(
    output: &mut BufferedStdout<'_, impl std::io::Write>,
    bytes: &[u8],
    last: bool,
    options: &Options,
) {
    if options.csv_out {
        output.csv_field(bytes);
    } else {
        output.raw(bytes);
    }
    output.emit(super::headers::Part::Separator(if last {
        options.record_end
    } else {
        options.output
    }));
}
