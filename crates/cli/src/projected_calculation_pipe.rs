//! Overlap native sorted emission with calculation through reusable buffers.
//! Only selected fields need text again. A `_` in each unselected field
//! preserves selector positions, field-count diagnostics and where a blank
//! field's number would stop. Numeric crossings use the
//! serial collector instead, since their original neighbours must be retained.
#![forbid(unsafe_code)]
use super::command_output::CommandOutput;
use super::grouping::GroupRecord;
use super::operation_set::Fields;
use super::{
    Failure, Intake, Selector, failure, grouping, intake, numerics, operation_set, records, spill,
    unsupported,
};
use std::io::{self, Write};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};

const BUFFER: usize = 64 << 10;
pub(super) const MEMORY: usize = 3 * BUFFER;

#[derive(Clone, Copy)]
struct Entry {
    end: usize,
    numbers: [Option<numerics::Value>; 2],
    complete: bool,
}
// Metadata for both batches uses at most one additional byte-buffer allowance.
// This bounds a batch, not the number or size of Records in the Command.
const ENTRIES: usize = BUFFER / (2 * std::mem::size_of::<Entry>());
#[derive(Default)]
struct Batch {
    bytes: Vec<u8>,
    entries: Vec<Entry>,
}
impl Batch {
    fn new() -> Option<Self> {
        let mut batch = Self::default();
        batch.bytes.try_reserve_exact(BUFFER).ok()?;
        batch.entries.try_reserve_exact(ENTRIES).ok()?;
        Some(batch)
    }
}
// Two checked batches circulate between the threads. Numbers retain their
// admitted type, and Record boundaries need no second delimiter scan.
// Long Records span batches without growing the transfer buffers.
struct Reader {
    incoming: Receiver<Batch>,
    free: SyncSender<Batch>,
    buffer: Batch,
    at: usize,
    byte_at: usize,
}
struct Writer {
    outgoing: SyncSender<Batch>,
    free: Receiver<Batch>,
    buffer: Batch,
    numbers: [Option<numerics::Value>; 2],
}
fn disconnected() -> io::Error {
    io::Error::from(io::ErrorKind::BrokenPipe)
}
fn buffers() -> Option<(Reader, Writer)> {
    let first = Batch::new()?;
    let second = Batch::new()?;
    let (outgoing, incoming) = sync_channel(1);
    let (return_buffer, free) = sync_channel(1);
    return_buffer.send(second).ok()?;
    Some((
        Reader {
            incoming,
            free: return_buffer,
            buffer: Batch::default(),
            at: 0,
            byte_at: 0,
        },
        Writer {
            outgoing,
            free,
            buffer: first,
            numbers: [None; 2],
        },
    ))
}
impl Reader {
    #[cold]
    #[inline(never)]
    fn refill(&mut self) {
        if self.buffer.bytes.capacity() != 0 {
            // The writer may already have finished. Its last buffered
            // bytes remain readable after the return channel closes.
            let _ = self.free.send(std::mem::take(&mut self.buffer));
        }
        self.buffer = self.incoming.recv().unwrap_or_default();
        self.at = 0;
        self.byte_at = 0;
    }
    fn record(
        &mut self,
        bytes: &mut Vec<u8>,
    ) -> Result<Option<[Option<numerics::Value>; 2]>, Failure> {
        bytes.clear();
        let mut partial = false;
        loop {
            if self.at == self.buffer.entries.len() {
                self.refill();
                if self.buffer.entries.is_empty() {
                    return if partial {
                        Err(pipe_error(io::Error::from(io::ErrorKind::UnexpectedEof)))
                    } else {
                        Ok(None)
                    };
                }
            }
            let entry = self.buffer.entries[self.at];
            let part = &self.buffer.bytes[self.byte_at..entry.end];
            bytes
                .try_reserve(part.len())
                .map_err(|_| unsupported("record memory allocation failed"))?;
            bytes.extend_from_slice(part);
            self.at += 1;
            self.byte_at = entry.end;
            if entry.complete {
                return Ok(Some(entry.numbers));
            }
            partial = true;
        }
    }
}
impl Writer {
    fn ready(&mut self) -> io::Result<()> {
        if self.buffer.bytes.capacity() == 0 {
            self.buffer = self.free.recv().map_err(|_| disconnected())?;
            self.buffer.bytes.clear();
            self.buffer.entries.clear();
        }
        Ok(())
    }
    fn begin(&mut self, numbers: [Option<numerics::Value>; 2]) -> io::Result<()> {
        if self.buffer.entries.len() == ENTRIES || self.buffer.bytes.len() == BUFFER {
            self.flush()?;
        }
        self.ready()?;
        self.numbers = numbers;
        Ok(())
    }
    fn end(&mut self, complete: bool) {
        self.buffer.entries.push(Entry {
            end: self.buffer.bytes.len(),
            numbers: self.numbers,
            complete,
        });
    }
    #[cold]
    #[inline(never)]
    fn write_all_cold(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        while !bytes.is_empty() {
            self.ready()?;
            let count = bytes.len().min(BUFFER - self.buffer.bytes.len());
            self.buffer.bytes.extend_from_slice(&bytes[..count]);
            bytes = &bytes[count..];
            if !bytes.is_empty() {
                self.end(false);
                self.flush()?;
            }
        }
        Ok(())
    }
    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len()
            <= self
                .buffer
                .bytes
                .capacity()
                .min(BUFFER)
                .wrapping_sub(self.buffer.bytes.len())
        {
            self.buffer.bytes.extend_from_slice(bytes);
            Ok(())
        } else {
            self.write_all_cold(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        if !self.buffer.entries.is_empty() {
            self.outgoing
                .send(std::mem::take(&mut self.buffer))
                .map_err(|_| disconnected())?;
        }
        Ok(())
    }
}

// At most two numeric fields can be parsed on the merge worker. Other
// fields and all failed conversions still use the ordinary collector.
struct Prepared {
    raw: grouping::Raw,
    numbers: [(u64, Option<numerics::Value>); 2],
}
impl Prepared {
    fn new(keys: usize, index: records::FieldIndex, fields: [u64; 2]) -> Result<Self, Failure> {
        Ok(Self {
            raw: grouping::Raw::for_keys(keys, index)?,
            numbers: fields.map(|field| (field, None)),
        })
    }
}
struct Cached<'r, 'i> {
    whole: operation_set::Whole<'r, 'i>,
    numbers: &'i [(u64, Option<numerics::Value>); 2],
}
impl<'r> Fields<'r> for Cached<'r, '_> {
    fn text(
        &mut self,
        field: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<&'r [u8], Failure> {
        self.whole.text(field, line, options)
    }
    fn number(
        &mut self,
        field: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<Option<numerics::Value>, Failure> {
        if let Some(value) = self
            .numbers
            .iter()
            .find(|(number, _)| *number == field)
            .and_then(|(_, value)| *value)
        {
            return Ok(Some(value));
        }
        self.whole.number(field, line, options)
    }
}
impl GroupRecord for Prepared {
    fn field(
        &mut self,
        field: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<&[u8], Failure> {
        self.raw.field(field, line, options)
    }
    fn field_pair(
        &mut self,
        a: u64,
        b: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<(&[u8], &[u8]), Failure> {
        self.raw.field_pair(a, b, line, options)
    }
    fn same_group(
        &mut self,
        previous: &mut Self,
        keys: &[u64],
        line: u64,
        options: &super::options::Options,
    ) -> Result<bool, Failure> {
        self.raw.same_group(&mut previous.raw, keys, line, options)
    }
    fn original(&self) -> Option<&[u8]> {
        self.raw.original()
    }
    fn collect(
        &mut self,
        operations: &mut super::OperationSet,
        line: u64,
        options: &super::options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut super::random::RandomState>,
    ) -> Result<bool, Failure> {
        operations.collect_fields(
            &mut Cached {
                whole: self.raw.whole(),
                numbers: &self.numbers,
            },
            false,
            line,
            options,
            arithmetic,
            random,
        )
    }
}

fn pipe_error(error: impl std::fmt::Display) -> Failure {
    failure(format!("native sort pipe I/O error: {error}\n").into_bytes())
}

/// Two circulating byte buffers fit within the otherwise unused final-merge
/// writer allowance, including for an in-memory sort. Accumulation, random
/// draws and output stay on the caller.
/// `None` leaves sort emission and calculation untouched for the serial route.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn calculate<S: spill::Codec, O: CommandOutput>(
    sorted: &mut spill::Finished<S>,
    layout: super::FieldLayout<'_>,
    context: &mut grouping::Context<'_, O>,
) -> Result<Option<()>, Failure> {
    let (options, keys) = (context.options, context.keys);
    // In-memory emission leaves time for both conversions. A Spill merge
    // already does more work: leave one conversion on the caller for a lone
    // paired Operation, caching its later field for a shorter caller scan.
    let width = if sorted.in_memory() {
        2
    } else {
        context
            .operations
            .requests()
            .filter(|(kind, selector)| {
                kind.converts_numeric_field() || matches!(selector, Selector::Pair { .. })
            })
            .take(2)
            .count()
            .max(1)
    };
    let mut fields = [0; 2];
    let mut count = 0;
    'fields: for (kind, selector) in context.operations.requests() {
        if !kind.converts_numeric_field() && !matches!(selector, Selector::Pair { .. }) {
            continue;
        }
        let selected = match selector {
            Selector::Single(field) => [Some(field), None],
            Selector::Pair { left, right } if width == 1 => [Some(left.max(right)), None],
            Selector::Pair { left, right } => [Some(left), Some(right)],
        };
        for field in selected.into_iter().flatten() {
            if !fields[..count].contains(&field) {
                fields[count] = field;
                count += 1;
                if count == width {
                    break 'fields;
                }
            }
        }
    }
    // Successful cached numbers need no second text copy unless a key or a
    // text Operation also reads that Field. Failed/skipped conversions keep
    // their original bytes for the ordinary collector and its diagnostics.
    let omit_text = fields.map(|field| {
        !keys.contains(&field)
            && !context.operations.requests().any(|(kind, selector)| {
                matches!(selector, Selector::Single(number) if number == field)
                    && !kind.converts_numeric_field()
            })
    });
    let Some((mut reader, mut writer)) = buffers() else {
        return Ok(None);
    };
    let index = records::FieldIndex::selecting(layout.selected.iter().copied());
    let mut spare = Prepared::new(keys.len(), index.fresh(), fields)?;
    std::thread::scope(|scope| {
        let merge = std::thread::Builder::new()
            .name("sort-merge".into())
            .spawn_scoped(scope, move || {
                let crossings = super::Crossings::default();
                sorted.emit(|record| {
                    let mut whole_index = records::FieldIndex::default();
                    let mut projected = super::Projected {
                        record: &record,
                        layout,
                        crossings: &crossings,
                        separator: Some(b'\t'),
                    };
                    // Cache only successful values, preserving their binary80 bits.
                    // The caller retains conversion errors and their request order.
                    let numbers = fields.map(|field| {
                        (field != 0)
                            .then(|| {
                                if let Some(bytes) = record.original() {
                                    operation_set::Whole {
                                        record: bytes,
                                        index: &mut whole_index,
                                    }
                                    .number(field, 0, options)
                                } else {
                                    projected.number(field, 0, options)
                                }
                                .ok()
                                .flatten()
                            })
                            .flatten()
                    });
                    writer.begin(numbers).map_err(pipe_error)?;
                    if let Some(bytes) = record.original() {
                        writer.write_all(bytes).map_err(pipe_error)?;
                    } else {
                        let mut selected = layout.selected.iter().peekable();
                        for field in 1..=record.fields {
                            if field > 1 {
                                writer.write_all(b"\t").map_err(pipe_error)?;
                            }
                            if selected.peek().is_some_and(|number| **number == field) {
                                selected.next();
                                let cached = fields.iter().enumerate().any(|(at, &number)| {
                                    number == field && omit_text[at] && numbers[at].is_some()
                                });
                                let bytes = if cached {
                                    b"_".as_slice()
                                } else {
                                    record.segment(layout.segment(field, record.key_count()))
                                };
                                writer.write_all(bytes).map_err(pipe_error)?;
                            } else {
                                // A non-blank stand-in keeps the field present
                                // and stops a blank selected field before it
                                // from reading on into a later number.
                                writer.write_all(b"_").map_err(pipe_error)?;
                            }
                        }
                    }
                    writer.end(true);
                    Ok(())
                })?;
                writer.flush().map_err(pipe_error)?;
                Ok(())
            });
        let Ok(merge) = merge else {
            return Ok(None);
        };
        if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
            let _ = crate::standard_io::Stderr.write_all(b"sort calculation: native merge pipe\n");
        }
        let mut intake = Intake::new(options, intake::Header::None)
            .with_filter(intake::Filter::None)
            .after(u64::from(options.header_in));
        let mut calculation = grouping::Calculation::new(keys, options);
        let mut transport_error = None;
        let result = (|| {
            loop {
                let numbers = match reader.record(spare.raw.bytes_mut()) {
                    Ok(Some(numbers)) => numbers,
                    Ok(None) => break,
                    Err(error) if error.status == 77 => return Err(error),
                    Err(error) => {
                        transport_error = Some(error);
                        break;
                    }
                };
                intake.accept(spare.raw.bytes_mut())?;
                for (slot, value) in spare.numbers.iter_mut().zip(numbers) {
                    slot.1 = value;
                }
                let line = intake.line();
                if line == 1 && options.header_out && !options.header_in {
                    context
                        .output
                        .first(spare.raw.bytes(), context.operations, keys)?;
                }
                calculation.push_in_place(
                    &mut spare,
                    || Prepared::new(keys.len(), index.fresh(), fields),
                    line,
                    context,
                )?;
            }
            let line = intake.line();
            intake.finish().map(|()| line)
        })();
        // A calculation error must unblock and join the merge before returning.
        drop(reader);
        let merged = merge
            .join()
            .map_err(|_| unsupported("native sort merge thread failed"))?;
        let line = result?;
        merged?;
        if let Some(error) = transport_error {
            return Err(error);
        }
        context.random = None;
        calculation.finish(line, context).map(Some)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recycled_buffers_preserve_long_writes_short_tails_and_eof() {
        let expected: Vec<_> = (0..BUFFER * 3 + 29).map(|at| (at % 251) as u8).collect();
        let (mut reader, mut writer) = buffers().unwrap();
        std::thread::scope(|scope| {
            let expected = &expected;
            let producer = scope.spawn(move || {
                writer.begin([None; 2]).unwrap();
                writer.write_all(expected).unwrap();
                writer.end(true);
                for at in 0..ENTRIES * 3 {
                    writer.begin([None; 2]).unwrap();
                    writer.write_all(&[at as u8]).unwrap();
                    writer.end(true);
                }
                writer.begin([None; 2]).unwrap();
                writer.end(true);
                writer.flush().unwrap();
            });
            let mut actual = Vec::new();
            assert!(reader.record(&mut actual).ok().unwrap().is_some());
            assert_eq!(&actual, expected);
            for at in 0..ENTRIES * 3 {
                assert!(reader.record(&mut actual).ok().unwrap().is_some());
                assert_eq!(actual, [at as u8]);
            }
            assert!(reader.record(&mut actual).ok().unwrap().is_some());
            assert!(actual.is_empty());
            assert!(reader.record(&mut actual).ok().unwrap().is_none());
            producer.join().unwrap();
        });
    }

    #[test]
    fn dropping_the_reader_unblocks_a_buffered_writer() {
        let (mut reader, mut writer) = buffers().unwrap();
        std::thread::scope(|scope| {
            let producer = scope.spawn(move || {
                writer.begin([None; 2])?;
                writer.write_all(&vec![b'x'; BUFFER * 8])
            });
            reader.refill();
            assert!(!reader.buffer.entries.is_empty());
            drop(reader);
            assert_eq!(
                producer.join().unwrap().unwrap_err().kind(),
                io::ErrorKind::BrokenPipe
            );
        });
    }
    #[test]
    fn cached_numbers_keep_every_binary80_bit() {
        let patterns = [
            0,
            1u128 << 79,
            1,
            (1u128 << 63) - 1,
            (1u128 << 64) | (1u128 << 63),
            0x3fff8000000000000000,
            0x7ffeffffffffffffffff,
            0x7fff8000000000000000,
            0xffff8000000000000000,
            0x7fffc000000000000001,
            0xffffd23456789abcdef0,
        ];
        for mask in 0..4u8 {
            for bits in patterns {
                let expected = fastmash_numeric_contract::Value80::try_from_bits(bits).unwrap();
                let mut numbers = [None; 2];
                for (at, number) in numbers.iter_mut().enumerate() {
                    if mask & (1 << at) != 0 {
                        *number = Some(numerics::Numerics::admit(expected).unwrap());
                    }
                }
                let (mut reader, mut writer) = buffers().unwrap();
                writer.begin(numbers).unwrap();
                writer.end(true);
                writer.flush().unwrap();
                drop(writer);
                let mut bytes = Vec::new();
                let values = reader.record(&mut bytes).ok().unwrap().unwrap();
                for (at, value) in values.into_iter().enumerate() {
                    assert_eq!(value.is_some(), mask & (1 << at) != 0);
                    if let Some(value) = value {
                        assert!(numerics::Numerics::value80(value).same_bits(expected));
                    }
                }
                assert!(reader.record(&mut bytes).ok().unwrap().is_none());
            }
        }
    }
    #[test]
    fn incomplete_record_is_rejected() {
        let (mut reader, mut writer) = buffers().unwrap();
        writer.begin([None; 2]).unwrap();
        writer.write_all(&vec![b'x'; BUFFER + 1]).unwrap();
        drop(writer);
        let error = reader.record(&mut Vec::new()).err().unwrap();
        assert_eq!(error.status, 1);
    }
}
