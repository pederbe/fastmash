//! The Group loop shared by every route. Consecutive records with equal
//! grouping keys form a Group; when a Group ends, its keys (or Full row, or
//! crosstab cell) and its Operation results are written.
#[cfg(test)]
use super::selected_field;
use super::{
    Failure, OperationSet, command_memory, command_output, field_policy, indexed_field, numerics,
    options, random, records, text_order,
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

/// The state a Group loop writes through.
pub(super) struct Context<'a, O> {
    pub operations: &'a mut OperationSet,
    pub keys: &'a [u64],
    pub options: &'a options::Options,
    pub arithmetic: &'a mut numerics::Numerics,
    pub random: Option<&'a mut random::RandomState>,
    pub output: &'a mut O,
}

/// Groups records as they arrive; `finish` writes the last Group.
pub(super) struct Grouping<R> {
    representative: Option<R>,
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

impl<R: GroupRecord> Grouping<R> {
    pub(super) fn new() -> Self {
        Self {
            representative: None,
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
            self.representative.replace(record)
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
            match &mut self.representative {
                Some(previous) => std::mem::swap(previous, record),
                None => self.representative = Some(std::mem::replace(record, fresh()?)),
            }
        }
        Ok(())
    }

    /// Groups and collects `record`. True when it must become the Group's
    /// representative.
    #[inline(always)]
    fn add<O: command_output::CommandOutput>(
        &mut self,
        record: &mut R,
        line: u64,
        context: &mut Context<'_, O>,
    ) -> Result<bool, Failure> {
        let new_group = match &mut self.representative {
            None => true,
            Some(previous) => {
                context.options.linewise
                    || !record.same_group(previous, context.keys, line, context.options)?
            }
        };
        if new_group {
            if let Some(previous) = &mut self.representative {
                write_group(previous, line, context)?;
            }
            context.operations.reset();
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

    /// Writes the last Group. False when no record arrived.
    pub(super) fn finish<O: command_output::CommandOutput>(
        mut self,
        line: u64,
        context: &mut Context<'_, O>,
    ) -> Result<bool, Failure> {
        match &mut self.representative {
            Some(previous) => write_group(previous, line, context).map(|()| true),
            None => Ok(false),
        }
    }
}

/// Writes a finished Group: its keys (or Full row, or crosstab cell) and its
/// Operation results, from `record`, its representative.
pub(super) fn write_group<R: GroupRecord, O: command_output::CommandOutput>(
    record: &mut R,
    line: u64,
    context: &mut Context<'_, O>,
) -> Result<(), Failure> {
    let options = context.options;
    if options.crosstab {
        let (row, column) = record.field_pair(context.keys[0], context.keys[1], line, options)?;
        let value = context.operations.result(0, context.arithmetic, options)?;
        return context.output.cell(row, column, &value);
    }
    if options.full {
        let Some(bytes) = record.original() else {
            unreachable!("full output retains original records")
        };
        command_output::full_prefix(context.output, bytes, options);
    }
    for &key in context.keys.iter().filter(|_| !options.full) {
        context
            .output
            .key(record.field(key, line, options)?, options.output);
    }
    context
        .operations
        .write_results(context.output, context.arithmetic, options)
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
    /// Adds this record to the Operations outside any Group.
    pub(super) fn collect_ungrouped(
        &mut self,
        operations: &mut OperationSet,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        GroupRecord::collect(self, operations, line, options, arithmetic, random)
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
