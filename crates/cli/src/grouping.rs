//! Calculation over prepared records: collect Operations, manage consecutive
//! Groups when requested, and finish the result output. Routes retain input,
//! field Binding and header preparation.
#[cfg(test)]
use super::selected_field;
use super::{
    Failure, Kind, OperationSet, command_memory, command_output, failure, field_policy,
    indexed_field, named_fields, numerics, options, random, records, text_order,
};

/// What the Group loop needs from a record, however it was read or sorted.
/// Lookups take the record mutably, so an adapter can keep what it learns
/// about where the record's fields are.
pub(super) trait GroupRecord {
    /// Field `key` (1-based), with the input's field diagnostics for `line`.
    fn field(&mut self, key: u64, line: u64, options: &options::Options) -> Result<&[u8], Failure>;
    /// Fields `first` and `second`, located in that order.
    fn field_pair(
        &mut self,
        first: u64,
        second: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<(&[u8], &[u8]), Failure>;
    /// Whether this record belongs to `previous`'s Group. Keys are compared in
    /// request order, so a mismatch on an earlier key makes later keys irrelevant.
    /// `previous` is mutable so an adapter can keep what it learns about the
    /// representative (such as where its keys are) for the next comparison.
    fn same_group(
        &mut self,
        previous: &mut Self,
        keys: &[u64],
        line: u64,
        options: &options::Options,
    ) -> Result<bool, Failure>;
    /// The complete record, where it is kept.
    fn original(&self) -> Option<&[u8]>;
    /// Copies each source field before the results. Decoded records supply
    /// their own field boundaries instead of an ordinary text separator.
    fn full_prefix(
        &self,
        output: &mut impl command_output::CommandOutput,
        options: &options::Options,
    ) {
        let Some(bytes) = self.original() else {
            unreachable!("full output retains original records")
        };
        command_output::full_prefix(output, bytes, options);
    }
    /// Adds this record to the Operations. True when it must become the
    /// Group's representative (for example for `last`).
    fn collect(
        &mut self,
        operations: &mut OperationSet,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure>;
}

/// The state a calculation writes through.
pub(super) struct Context<'a, O> {
    pub operations: &'a mut OperationSet,
    pub keys: &'a [u64],
    pub options: &'a options::Options,
    pub arithmetic: &'a mut numerics::Numerics,
    pub random: Option<&'a mut random::RandomState>,
    pub output: &'a mut O,
}

/// Collects records as they arrive; `finish` writes the remaining results and
/// completes their output.
pub(super) struct Calculation<R> {
    state: Collected<R>,
}

enum Collected<R> {
    EmptyUngrouped,
    Ungrouped,
    Grouped(Option<R>),
}

impl<'a, O> Context<'a, O> {
    pub(super) fn new(
        operations: &'a mut OperationSet,
        keys: &'a [u64],
        options: &'a options::Options,
        arithmetic: &'a mut numerics::Numerics,
        random: Option<&'a mut random::RandomState>,
        output: &'a mut O,
    ) -> Self {
        Self {
            operations,
            keys,
            options,
            arithmetic,
            random,
            output,
        }
    }
}

impl<R: GroupRecord> Calculation<R> {
    pub(super) fn new(keys: &[u64], options: &options::Options) -> Self {
        Self {
            state: if !keys.is_empty() || options.full || options.linewise {
                Collected::Grouped(None)
            } else {
                Collected::EmptyUngrouped
            },
        }
    }

    /// Adds the next record (line `line`). Returns a record the caller no
    /// longer needs, so its storage can be reused for the next one.
    pub(super) fn push<O: command_output::CommandOutput>(
        &mut self,
        mut record: R,
        line: u64,
        context: &mut Context<'_, O>,
    ) -> Result<Option<R>, Failure> {
        Ok(if self.add(&mut record, line, context)? {
            let Collected::Grouped(representative) = &mut self.state else {
                unreachable!("only a Group retains a representative")
            };
            representative.replace(record)
        } else {
            Some(record)
        })
    }

    /// Adds the next record (line `line`) as `push` does, but in place: a
    /// record is moved only when it becomes the Group's representative, and
    /// `record` then holds the previous representative for the next read, or,
    /// for the first record, new storage from `fresh`. Moving every record in
    /// and out costs a short-record job about a sixth of its cycles.
    pub(super) fn push_in_place<O: command_output::CommandOutput>(
        &mut self,
        record: &mut R,
        fresh: impl FnOnce() -> Result<R, Failure>,
        line: u64,
        context: &mut Context<'_, O>,
    ) -> Result<(), Failure> {
        if self.add(record, line, context)? {
            let Collected::Grouped(representative) = &mut self.state else {
                unreachable!("only a Group retains a representative")
            };
            match representative {
                Some(previous) => std::mem::swap(previous, record),
                None => *representative = Some(std::mem::replace(record, fresh()?)),
            }
        }
        Ok(())
    }

    /// Collects `record`, transitioning Groups when requested. True when it
    /// must become the Group's representative.
    #[inline(always)]
    fn add<O: command_output::CommandOutput>(
        &mut self,
        record: &mut R,
        line: u64,
        context: &mut Context<'_, O>,
    ) -> Result<bool, Failure> {
        // Once ungrouped input has started, the Record loop only collects.
        // Marking the first Record separately avoids a state write per Record.
        if matches!(self.state, Collected::Ungrouped) {
            record.collect(
                context.operations,
                line,
                context.options,
                context.arithmetic,
                context.random.as_deref_mut(),
            )?;
            return Ok(false);
        }
        let Collected::Grouped(representative) = &mut self.state else {
            record.collect(
                context.operations,
                line,
                context.options,
                context.arithmetic,
                context.random.as_deref_mut(),
            )?;
            self.state = Collected::Ungrouped;
            return Ok(false);
        };
        let new_group = match representative {
            None => true,
            Some(previous) => {
                context.options.linewise
                    || !record.same_group(previous, context.keys, line, context.options)?
            }
        };
        if new_group {
            if let Some(previous) = representative {
                write_group(previous, line, context)?;
            }
            context.operations.reset();
        }
        if context.operations.has_weighted_mean() {
            // A preceding Group completes first. Then every required key is
            // checked before this Record can omit a weighted pair, even when
            // --full or an earlier unequal key would otherwise hide it.
            for &key in context.keys {
                record.field(key, line, context.options)?;
            }
        }
        let keep = record.collect(
            context.operations,
            line,
            context.options,
            context.arithmetic,
            context.random.as_deref_mut(),
        )?;
        Ok(new_group || keep)
    }

    /// Writes the last Group or ungrouped results, then completes the output.
    /// Empty input still completes the output without a result row.
    pub(super) fn finish<O: command_output::CommandOutput>(
        mut self,
        line: u64,
        context: &mut Context<'_, O>,
    ) -> Result<(), Failure> {
        match &mut self.state {
            Collected::Grouped(Some(previous)) => write_group(previous, line, context)?,
            Collected::Ungrouped => context.operations.write_ungrouped_results(
                context.output,
                context.arithmetic,
                context.options,
            )?,
            Collected::EmptyUngrouped | Collected::Grouped(None) => {}
        }
        context.output.end(context.options)
    }
}

/// Writes a finished Group: its keys (or Full row, or crosstab cell) and its
/// Operation results, from `record`, its representative.
pub(super) fn write_group<R: GroupRecord, O: command_output::CommandOutput>(
    record: &mut R,
    line: u64,
    context: &mut Context<'_, O>,
) -> Result<(), Failure> {
    write_completed_group(
        record,
        line,
        context.operations.completion(),
        context.keys,
        context.options,
        context.arithmetic,
        context.output,
    )
}

/// Writes either an ordinary or retained Group through the same completion
/// capability, with Group presentation and diagnostics remaining here.
pub(super) fn write_completed_group<R: GroupRecord, O: command_output::CommandOutput>(
    record: &mut R,
    line: u64,
    mut completion: super::operation_set::GroupCompletion<'_>,
    keys: &[u64],
    options: &options::Options,
    arithmetic: &mut numerics::Numerics,
    output: &mut O,
) -> Result<(), Failure> {
    if options.crosstab {
        let (row, column) = record.field_pair(keys[0], keys[1], line, options)?;
        let value = completion.result(0, arithmetic, options)?;
        return output.cell(row, column, &value);
    }
    if options.full {
        record.full_prefix(output, options);
    }
    for &key in keys.iter().filter(|_| !options.full) {
        output.key(record.field(key, line, options)?, options.output);
    }
    completion
        .write_results(output, arithmetic, options)
        .map_err(|(kind, mut error)| {
            if kind == Kind::Wmean && !keys.is_empty() {
                // Completion has a Group, not an offending source Record.
                // Context is best effort if the failure left no memory.
                let mut description = b"group keys: ".to_vec();
                for (index, &key) in keys.iter().enumerate() {
                    let Ok(bytes) = record.field(key, line, options) else {
                        return error;
                    };
                    let label = format!("{}field {key}=", if index == 0 { "" } else { ", " });
                    let size = label
                        .len()
                        .saturating_add(bytes.len().saturating_mul(4))
                        .saturating_add(7);
                    if description.try_reserve_exact(size).is_err() {
                        return error;
                    }
                    description.extend_from_slice(label.as_bytes());
                    named_fields::quote_with(bytes, &mut description, options.locale.utf8);
                }
                description.push(b'\n');
                failure::append(&mut error, &[&description]);
            }
            error
        })
}

/// A record as read from unsorted (or already sorted) input, with its field
/// index. The positions of its keys are found on first use and kept while it
/// is the representative, so each following record locates only its own keys.
#[derive(Default)]
pub(super) struct Raw {
    bytes: Vec<u8>,
    key_spans: Vec<Option<field_policy::FieldRange>>,
    fields: records::FieldIndex,
}

impl Raw {
    /// Empty storage with room for the positions of `keys` grouping keys, so
    /// the key cache never grows, and `fields`, an index for the selected
    /// fields ([`records::FieldIndex::selecting`]).
    pub(super) fn for_keys(keys: usize, fields: records::FieldIndex) -> Result<Self, Failure> {
        let mut key_spans = Vec::new();
        command_memory::reserve(&mut key_spans, keys)?;
        Ok(Self {
            bytes: Vec::new(),
            key_spans,
            fields,
        })
    }
    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    /// The record's storage, to read the next record into; the cached key
    /// positions and field spans no longer apply.
    pub(super) fn bytes_mut(&mut self) -> &mut Vec<u8> {
        self.key_spans.clear();
        self.fields.clear();
        &mut self.bytes
    }
    /// Replaces the field index, once the selected fields are known.
    pub(super) fn index_fields(&mut self, fields: records::FieldIndex) {
        self.fields = fields;
    }
}

impl GroupRecord for Raw {
    fn field(&mut self, key: u64, line: u64, options: &options::Options) -> Result<&[u8], Failure> {
        let span = indexed_field(&self.bytes, &mut self.fields, key, line, options.input)?;
        Ok(&self.bytes[span.start..span.start + span.length])
    }
    fn field_pair(
        &mut self,
        first: u64,
        second: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<(&[u8], &[u8]), Failure> {
        let a = indexed_field(&self.bytes, &mut self.fields, first, line, options.input)?;
        let b = indexed_field(&self.bytes, &mut self.fields, second, line, options.input)?;
        Ok((
            &self.bytes[a.start..a.start + a.length],
            &self.bytes[b.start..b.start + b.length],
        ))
    }
    fn same_group(
        &mut self,
        previous: &mut Self,
        keys: &[u64],
        line: u64,
        options: &options::Options,
    ) -> Result<bool, Failure> {
        if previous.key_spans.len() != keys.len() {
            if previous.key_spans.capacity() < keys.len() {
                command_memory::reserve(&mut previous.key_spans, keys.len())?;
            }
            previous.key_spans.resize(keys.len(), None);
        }
        for (&key, cached) in keys.iter().zip(previous.key_spans.iter_mut()) {
            // The current record's key is located before the previous one's,
            // as before caching, so diagnostics and their order are unchanged.
            let a = indexed_field(&self.bytes, &mut self.fields, key, line, options.input)?;
            let b = match *cached {
                Some(span) => span,
                None => *cached.insert(indexed_field(
                    &previous.bytes,
                    &mut previous.fields,
                    key,
                    line,
                    options.input,
                )?),
            };
            if !text_order::same_group(
                &self.bytes[a.start..a.start + a.length],
                &previous.bytes[b.start..b.start + b.length],
                options.ignore_case,
            ) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn original(&self) -> Option<&[u8]> {
        Some(&self.bytes)
    }
    fn collect(
        &mut self,
        operations: &mut OperationSet,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        operations.collect(
            &self.bytes,
            &mut self.fields,
            line,
            options,
            arithmetic,
            random,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calculated(args: &[&str], rows: &[&[u8]], in_place: bool) -> Vec<u8> {
        let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
        let options::Action::Calculate(mut options) =
            options::parse(&args, b"fastmash", false).ok().unwrap()
        else {
            panic!("calculation")
        };
        let command = super::super::grammar::command(&options.operands, options.group.as_ref())
            .ok()
            .unwrap();
        options.linewise = command.mode == super::super::grammar::Mode::Line;
        options.crosstab = command.mode == super::super::grammar::Mode::Crosstab;
        let mut binding =
            super::super::binding::Binding::new(b"fastmash", command.operations, command.keys)
                .ok()
                .unwrap();
        binding.numbered().ok().unwrap();
        let mut arithmetic = numerics::Numerics::with(binding.operations.needs(false).numerics)
            .ok()
            .unwrap();
        let mut bytes = Vec::new();
        let buffer = super::super::buffered_stdout::BufferedStdout::new(&mut bytes, 4096, false)
            .ok()
            .unwrap();
        let mut output = command_output::Results::new(buffer, &options);
        let mut calculation = Calculation::new(&binding.keys, &options);
        let mut context = Context::new(
            &mut binding.operations,
            &binding.keys,
            &options,
            &mut arithmetic,
            None,
            &mut output,
        );
        let mut spare = Raw::for_keys(binding.keys.len(), records::FieldIndex::default())
            .ok()
            .unwrap();
        for (index, row) in rows.iter().enumerate() {
            spare.bytes_mut().clear();
            spare.bytes_mut().extend_from_slice(row);
            if in_place {
                let storage = spare.bytes().as_ptr();
                calculation
                    .push_in_place(
                        &mut spare,
                        || {
                            assert!(
                                !binding.keys.is_empty() || options.full || options.linewise,
                                "ungrouped calculation must reuse the caller's record"
                            );
                            Raw::for_keys(binding.keys.len(), records::FieldIndex::default())
                        },
                        index as u64 + 1,
                        &mut context,
                    )
                    .ok()
                    .unwrap();
                if binding.keys.is_empty() && !options.full && !options.linewise {
                    assert_eq!(spare.bytes().as_ptr(), storage);
                }
            } else {
                spare = calculation
                    .push(spare, index as u64 + 1, &mut context)
                    .ok()
                    .unwrap()
                    .unwrap_or_default();
            }
        }
        calculation
            .finish(rows.len() as u64, &mut context)
            .ok()
            .unwrap();
        let done = output.buffer.finish(|_| Ok(()));
        assert!(done.first_error.is_none() && done.flush_error.is_none());
        bytes
    }

    #[test]
    fn calculation_collects_and_completes_through_both_record_interfaces() {
        type Job<'a> = (&'a [&'a str], &'a [&'a [u8]], &'a [u8]);
        let jobs: &[Job<'_>] = &[
            // `last` requests a representative, but ungrouped input still
            // reuses its record and writes one aggregate at completion.
            (
                &["count", "1", "collapse", "1", "last", "1"],
                &[b"a", b"b"],
                b"2\ta,b\tb\n",
            ),
            (
                &["--full", "-g", "1", "last", "2"],
                &[b"a\t1", b"a\t2", b"b\t3"],
                b"a\t2\t2\nb\t3\t3\n",
            ),
            (&["cut", "1"], &[b"a", b"b"], b"a\nb\n"),
            // The last Group must reach the table before output completion.
            (
                &["crosstab", "1,2", "count", "3"],
                &[b"a\tx\t1", b"a\tx\t2", b"a\ty\t3", b"b\tx\t4"],
                b"\tx\ty\na\t2\t1\nb\t1\tN/A\n",
            ),
            (&["count", "1"], &[], b""),
            (&["-g", "1", "count", "2"], &[], b""),
        ];
        for &(args, rows, expected) in jobs {
            for in_place in [false, true] {
                assert_eq!(calculated(args, rows, in_place), expected, "{args:?}");
            }
        }
    }

    fn options() -> options::Options {
        let args: Vec<std::ffi::OsString> = ["-g", "1", "count", "1"]
            .into_iter()
            .map(Into::into)
            .collect();
        let options::Action::Calculate(options) =
            options::parse(&args, b"fastmash", false).ok().unwrap()
        else {
            unreachable!()
        };
        *options
    }

    fn raw_with(bytes: &[u8], keep: bool) -> Raw {
        let fields = if keep {
            records::FieldIndex::always_keeping(1..=5)
        } else {
            records::FieldIndex::default()
        };
        let mut record = Raw::for_keys(2, fields).ok().unwrap();
        record.bytes_mut().extend_from_slice(bytes);
        record
    }

    #[test]
    fn group_changes_reuse_cached_previous_key_positions() {
        let options = options();
        for keep in [false, true] {
            let raw = |bytes: &[u8]| raw_with(bytes, keep);
            let mut previous = raw(b"a\tx\t1");
            assert!(
                raw(b"a\tx\t2")
                    .same_group(&mut previous, &[1, 2], 2, &options)
                    .ok()
                    .unwrap()
            );
            assert!(previous.key_spans.iter().all(Option::is_some));
            // A differing first key returns before the second key is located.
            let mut fresh = raw(b"a\tx\t1");
            assert!(
                !raw(b"b\tx")
                    .same_group(&mut fresh, &[1, 2], 3, &options)
                    .ok()
                    .unwrap()
            );
            assert!(fresh.key_spans[0].is_some() && fresh.key_spans[1].is_none());
            // A previous record without the key fails at first use, as uncached.
            let mut short = raw(b"a");
            let error = raw(b"a\tx")
                .same_group(&mut short, &[2], 4, &options)
                .err()
                .unwrap();
            let uncached = selected_field(b"a", 2, 4, options.input).err().unwrap();
            assert_eq!(error.message, uncached.message);
            assert!(short.key_spans[0].is_none());
            // New bytes forget the cached positions and kept spans.
            assert_eq!(previous.field(3, 2, &options).ok().unwrap(), b"1");
            let bytes = previous.bytes_mut();
            bytes.clear();
            bytes.extend_from_slice(b"b\ty\t2\t3");
            assert!(previous.key_spans.is_empty());
            let (first, fourth) = previous.field_pair(1, 4, 5, &options).ok().unwrap();
            assert_eq!((first, fourth), (b"b".as_slice(), b"3".as_slice()));
            let error = previous.field_pair(2, 5, 5, &options).err().unwrap();
            assert_eq!(
                error.message,
                b"invalid input: field 5 requested, line 5 has only 4 fields\n"
            );
        }
    }

    /// Both adapters give the same Groups: on input that is already sorted,
    /// the native Sort route (`-s`) and reading as-is write the same output.
    #[test]
    fn raw_and_sorted_records_group_alike() {
        let input: &[u8] = b"0\tv\tw\na\t1\tx\na\t2\ty\nb\t3\tz\nc\t4\tx\nc\t5\tx\n";
        // Each job, with its expected output where the test states it.
        let jobs: &[(&[&str], Option<&[u8]>)] = &[
            (
                &[
                    "-H", "-g", "1", "count", "2", "sum", "2", "first", "3", "last", "3",
                ],
                None,
            ),
            (&["-H", "--full", "-g", "1", "count", "2"], None),
            (&["--header-in", "-g", "1,3", "unique", "2"], None),
            (&["-H", "crosstab", "1,3", "sum", "2"], None),
            (&["--header-out", "-g", "1", "collapse", "3"], None),
            // The first data record becomes the representative; later ones
            // are swapped with it (`last`) or read into the spare storage.
            (
                &["-g", "1", "count", "2", "last", "3"],
                Some(b"0\t1\tw\na\t2\ty\nb\t1\tz\nc\t2\tx\n"),
            ),
            // `last` keeps the Group's last record for --full, as GNU swaps
            // it in on a kept line (datamash.c process_file).
            (
                &["--full", "-g", "1", "last", "2"],
                Some(b"0\tv\tw\tv\na\t2\ty\t2\nb\t3\tz\t3\nc\t5\tx\t5\n"),
            ),
        ];
        for &(job, expected) in jobs {
            let outputs: Vec<(i32, Vec<u8>)> = [false, true]
                .into_iter()
                .map(|sorted| {
                    let mut args: Vec<std::ffi::OsString> = job.iter().map(Into::into).collect();
                    if sorted {
                        args.insert(0, "-s".into());
                    }
                    let mut reader = input;
                    let mut output = Vec::new();
                    let status = super::super::run(
                        &mut reader,
                        &mut output,
                        &args,
                        b"fastmash",
                        &mut |_| true,
                    )
                    .ok()
                    .unwrap();
                    (status, output)
                })
                .collect();
            assert_eq!(outputs[0], outputs[1], "{job:?}");
            assert_eq!(outputs[0].0, 0, "{job:?}");
            if let Some(expected) = expected {
                assert_eq!(outputs[0].1, expected, "{job:?}");
            }
        }
    }
}
