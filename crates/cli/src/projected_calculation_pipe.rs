//! Overlap a native Spill merge with calculation through an anonymous pipe.
//! Only selected fields need text again. Empty unselected fields preserve
//! selector positions and field-count diagnostics. Numeric crossings use the
//! serial collector instead, since their original neighbours must be retained.
#![forbid(unsafe_code)]
use super::command_output::CommandOutput;
use super::grouping::GroupRecord;
use super::operation_set::Fields;
use super::{
    Failure, Intake, Selector, failure, grouping, intake, numerics, operation_set, records, spill,
    unsupported,
};
use std::io::{BufRead, Write};

const BUFFER: usize = 64 << 10;
pub(super) const MEMORY: usize = 3 * BUFFER;

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

fn read_numbers(
    reader: &mut impl BufRead,
) -> Result<Option<[Option<numerics::Value>; 2]>, Failure> {
    if reader.fill_buf().map_err(pipe_error)?.is_empty() {
        return Ok(None);
    }
    let mut mask = [0];
    reader.read_exact(&mut mask).map_err(pipe_error)?;
    if mask[0] & !3 != 0 {
        return Err(unsupported("invalid native sort pipe values"));
    }
    let mut numbers = [None; 2];
    for (at, number) in numbers.iter_mut().enumerate() {
        if mask[0] & (1 << at) != 0 {
            let mut raw = [0; 10];
            reader.read_exact(&mut raw).map_err(pipe_error)?;
            let value = fastmash_numeric_contract::Value80::from_le_bytes(raw)
                .map_err(|_| unsupported("invalid native sort pipe value"))?;
            *number = Some(
                numerics::Numerics::admit(value)
                    .map_err(|_| unsupported("invalid native sort pipe value"))?,
            );
        }
    }
    Ok(Some(numbers))
}

fn pipe_error(error: impl std::fmt::Display) -> Failure {
    failure(format!("native sort pipe I/O error: {error}\n").into_bytes())
}

/// Read, write and kernel pipe buffers share the otherwise unused final-merge
/// writer allowance. Accumulation, random draws and output stay on the caller.
/// `None` leaves sort emission and calculation untouched for the serial route.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn calculate<S: spill::Codec, O: CommandOutput>(
    sorted: &mut spill::Finished<S>,
    layout: &[u64],
    context: &mut grouping::Context<'_, O>,
) -> Result<Option<()>, Failure> {
    let (options, keys) = (context.options, context.keys);
    // Leave one conversion on the caller for a lone paired Operation. With
    // several numeric Operations, parsing two fields balances the extra work.
    // Cache the later field of a lone pair, leaving the caller a shorter scan.
    let width = context
        .operations
        .requests()
        .filter(|(kind, selector)| {
            kind.converts_numeric_field() || matches!(selector, Selector::Pair { .. })
        })
        .take(2)
        .count()
        .max(1);
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
    let Ok((reader, writer)) = std::io::pipe() else {
        return Ok(None);
    };
    if !matches!(
        nix::fcntl::fcntl(&reader, nix::fcntl::FcntlArg::F_GETPIPE_SZ),
        Ok(size) if size > 0 && size as usize <= BUFFER
    ) {
        return Ok(None);
    }
    let mut reader = spill::RunReader::new(BUFFER, reader)?;
    let mut writer = spill::RunWriter::new(BUFFER, writer)?;
    let index = records::FieldIndex::selecting(layout.iter().copied());
    let mut spare = Prepared::new(keys.len(), index.fresh(), fields)?;
    std::thread::scope(|scope| {
        let merge = std::thread::Builder::new()
            .name("sort-merge".into())
            .spawn_scoped(scope, move || {
                // Cancellation closes our reader. Keep that pipe's SIGPIPE
                // local to this disposable thread, leaving stdout's default
                // signal behaviour unchanged on the calling thread.
                use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
                let mut blocked = SigSet::empty();
                blocked.add(Signal::SIGPIPE);
                pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&blocked), None)
                    .map_err(|_| unsupported("unable to block native sort pipe signal"))?;
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
                    let mask =
                        u8::from(numbers[0].is_some()) | (u8::from(numbers[1].is_some()) << 1);
                    writer.write_all(&[mask]).map_err(pipe_error)?;
                    for value in numbers.into_iter().flatten() {
                        writer
                            .write_all(&numerics::Numerics::value80(value).to_le_bytes())
                            .map_err(pipe_error)?;
                    }
                    if let Some(bytes) = record.original() {
                        writer.write_all(bytes).map_err(pipe_error)?;
                    } else {
                        let mut selected = layout.iter().enumerate().peekable();
                        for field in 1..=record.fields {
                            if field > 1 {
                                writer.write_all(b"\t").map_err(pipe_error)?;
                            }
                            if selected.peek().is_some_and(|(_, number)| **number == field) {
                                let (at, _) = selected.next().unwrap();
                                writer
                                    .write_all(record.segment(record.key_count() + at))
                                    .map_err(pipe_error)?;
                            } else if record.fields == 1 {
                                // An empty byte string has zero fields. Keep
                                // one unselected field present for diagnostics.
                                writer.write_all(b"_").map_err(pipe_error)?;
                            }
                        }
                    }
                    writer.write_all(&[options.record_end]).map_err(pipe_error)
                })?;
                writer.into_inner().map_err(pipe_error)?;
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
                let numbers = match read_numbers(&mut reader) {
                    Ok(Some(numbers)) => numbers,
                    Ok(None) => break,
                    Err(error) => {
                        transport_error = Some(error);
                        break;
                    }
                };
                if intake.next(&mut reader, spare.raw.bytes_mut())?.is_none() {
                    transport_error = Some(pipe_error("missing native sort pipe record"));
                    break;
                }
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
                let mut frame = vec![mask];
                for at in 0..2 {
                    if mask & (1 << at) != 0 {
                        frame.extend_from_slice(&expected.to_le_bytes());
                    }
                }
                let mut reader = std::io::Cursor::new(frame);
                let values = read_numbers(&mut reader).ok().unwrap().unwrap();
                for (at, value) in values.into_iter().enumerate() {
                    assert_eq!(value.is_some(), mask & (1 << at) != 0);
                    if let Some(value) = value {
                        assert!(numerics::Numerics::value80(value).same_bits(expected));
                    }
                }
                assert!(read_numbers(&mut reader).ok().unwrap().is_none());
            }
        }
    }
    #[test]
    fn incomplete_or_invalid_cached_numbers_are_rejected() {
        for frame in [vec![4], vec![1], vec![3; 11]] {
            assert!(read_numbers(&mut std::io::Cursor::new(frame)).is_err());
        }
        for bits in [1u128 << 64, 0x7fff8000000000000001] {
            let mut frame = vec![1];
            frame.extend_from_slice(
                &fastmash_numeric_contract::Raw80::try_from_bits(bits)
                    .unwrap()
                    .to_le_bytes(),
            );
            assert!(read_numbers(&mut std::io::Cursor::new(frame)).is_err());
        }
    }
}
