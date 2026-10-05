//! Strict byte CSV intake and decoded Record ownership. Quote validation,
//! blank Records, BOM-as-data, checked growth and original locations share
//! one incremental parser, so no permissive decoding pass can lose them.
use super::{
    Failure, OperationSet, command_memory, command_output, decimal, failure, field_policy,
    grouping, intake, numerics, options, random, text_order, unsupported,
};
use std::io::{self, BufRead};

/// Original input coordinates, kept with decoded fields through retention.
#[derive(Clone, Copy, Default)]
pub(super) struct Location {
    pub record: u64,
    pub line: u64,
}

impl Location {
    pub(super) fn annotate(self, mut error: Failure) -> Failure {
        if error.status == 1 {
            failure::append(
                &mut error,
                &[format!(
                    "CSV record {} starts at physical line {}\n",
                    self.record, self.line
                )
                .as_bytes()],
            );
        }
        error
    }
}

/// Contiguous decoded bytes with explicit field ends, including empty fields.
#[derive(Default)]
pub(super) struct Record {
    pub(super) bytes: Vec<u8>,
    pub(super) ends: Vec<usize>,
    pub location: Location,
}

impl Record {
    pub(super) fn view(&self) -> View<'_> {
        View {
            bytes: &self.bytes,
            ends: &self.ends,
            location: self.location,
        }
    }

    /// Absent fields stay absent for validation, but sort like empty keys.
    pub(super) fn sort_field(&self, field: u64) -> &[u8] {
        self.view().sort_field(field)
    }

    /// Allocated payload and field ends, excluding the Record descriptor.
    pub(super) fn retained_bytes(&self) -> Option<usize> {
        self.ends
            .capacity()
            .checked_mul(std::mem::size_of::<usize>())?
            .checked_add(self.bytes.capacity())
    }

    pub(super) fn field(&self, field: u64) -> Result<&[u8], Failure> {
        self.view().field(field)
    }

    pub(super) fn fields(&self) -> impl Iterator<Item = &[u8]> {
        self.view().fields()
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        self.bytes
            .len()
            .checked_add(bytes.len())
            .ok_or_else(|| unsupported("command memory allocation failed"))?;
        if self.bytes.capacity() - self.bytes.len() < bytes.len() {
            command_memory::reserve(&mut self.bytes, bytes.len())?;
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn end_field(&mut self) -> Result<(), Failure> {
        if self.ends.len() == self.ends.capacity() {
            command_memory::reserve(&mut self.ends, 1)?;
        }
        self.ends.push(self.bytes.len());
        Ok(())
    }
}

/// Complete decoded Fields borrowed from streaming or packed Record storage.
#[derive(Clone, Copy)]
pub(super) struct View<'r> {
    pub(super) bytes: &'r [u8],
    pub(super) ends: &'r [usize],
    pub(super) location: Location,
}

impl<'r> View<'r> {
    pub(super) fn sort_field(self, field: u64) -> &'r [u8] {
        self.get(field).unwrap_or_default()
    }

    fn get(self, field: u64) -> Option<&'r [u8]> {
        let index = usize::try_from(field).ok()?.checked_sub(1)?;
        let end = *self.ends.get(index)?;
        let start = if index == 0 { 0 } else { self.ends[index - 1] };
        Some(&self.bytes[start..end])
    }

    pub(super) fn field(self, field: u64) -> Result<&'r [u8], Failure> {
        let Some(bytes) = self.get(field) else {
            return Err(super::missing_field(
                field,
                self.location.record,
                self.ends.len() as u64,
            ));
        };
        Ok(bytes)
    }

    pub(super) fn fields(self) -> impl Iterator<Item = &'r [u8]> {
        let mut start = 0;
        self.ends.iter().map(move |&end| {
            let field = &self.bytes[start..end];
            start = end;
            field
        })
    }

    pub(super) fn to_owned(self) -> Result<Record, Failure> {
        let mut record = Record {
            location: self.location,
            ..Record::default()
        };
        command_memory::reserve_exact(&mut record.bytes, self.bytes.len())?;
        command_memory::reserve_exact(&mut record.ends, self.ends.len())?;
        record.bytes.extend_from_slice(self.bytes);
        record.ends.extend_from_slice(self.ends);
        Ok(record)
    }
}

/// Packed representatives borrow their chunk; merge representatives own the
/// head moved from the run before that source advances.
pub(super) enum Retained<'r> {
    Borrowed(View<'r>),
    Owned(Record),
}

impl Retained<'_> {
    pub(super) fn view(&self) -> View<'_> {
        match self {
            Self::Borrowed(view) => *view,
            Self::Owned(record) => record.view(),
        }
    }

    pub(super) fn into_owned(self) -> Result<Record, Failure> {
        match self {
            Self::Borrowed(view) => view.to_owned(),
            Self::Owned(record) => Ok(record),
        }
    }

    pub(super) fn reusable(self) -> Option<Record> {
        match self {
            Self::Borrowed(_) => None,
            Self::Owned(record) => Some(record),
        }
    }
}

/// Operation field access shared by ordinary calculations and comparison.
pub(super) struct Fields<'r>(pub(super) View<'r>);
impl<'r> super::operation_set::Fields<'r> for Fields<'r> {
    fn text(&mut self, field: u64, _: u64, _: &options::Options) -> Result<&'r [u8], Failure> {
        self.0.field(field)
    }
    fn number(
        &mut self,
        field: u64,
        _: u64,
        options: &options::Options,
    ) -> Result<Option<numerics::Value>, Failure> {
        let bytes = self.0.field(field)?;
        decimal::number(
            bytes,
            field_policy::FieldRange {
                start: 0,
                length: bytes.len(),
            },
            options.narm,
            options.presentation.profile,
        )
        .map_err(|error| error.report(bytes, field, self.0.location.record))
    }
}

trait Decoded {
    fn decoded(&self) -> View<'_>;
}
impl Decoded for Record {
    fn decoded(&self) -> View<'_> {
        self.view()
    }
}
impl Decoded for Retained<'_> {
    fn decoded(&self) -> View<'_> {
        self.view()
    }
}

impl<R: Decoded> grouping::GroupRecord for R {
    fn field(&mut self, field: u64, _: u64, _: &options::Options) -> Result<&[u8], Failure> {
        let view = self.decoded();
        view.field(field)
            .map_err(|error| view.location.annotate(error))
    }
    fn field_pair(
        &mut self,
        left: u64,
        right: u64,
        _: u64,
        _: &options::Options,
    ) -> Result<(&[u8], &[u8]), Failure> {
        let view = self.decoded();
        Ok((view.field(left)?, view.field(right)?))
    }
    fn same_group(
        &mut self,
        previous: &mut Self,
        keys: &[u64],
        _: u64,
        options: &options::Options,
    ) -> Result<bool, Failure> {
        let current = self.decoded();
        let previous = previous.decoded();
        for &key in keys {
            let bytes = current
                .field(key)
                .map_err(|error| current.location.annotate(error))?;
            let prior = previous
                .field(key)
                .map_err(|error| previous.location.annotate(error))?;
            if !text_order::same_group(bytes, prior, options.ignore_case) {
                return Ok(false);
            }
        }
        Ok(true)
    }
    fn original(&self) -> Option<&[u8]> {
        None
    }
    fn full_prefix(
        &self,
        output: &mut impl command_output::CommandOutput,
        options: &options::Options,
    ) {
        for field in self.decoded().fields() {
            output.key(field, options.output);
        }
    }
    fn collect(
        &mut self,
        operations: &mut OperationSet,
        _: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        let view = self.decoded();
        operations
            .collect_decoded(
                &mut Fields(view),
                view.location.record,
                options,
                arithmetic,
                random,
            )
            .map_err(|error| view.location.annotate(error))
    }
}

#[derive(Clone, Copy)]
enum State {
    Start,
    Unquoted,
    Quoted,
    Closed,
    Cr { line: u64, byte: u64 },
}

/// Reads one logical Record at a time without holding the dataset or raw CSV.
pub(super) struct Intake {
    record: u64,
    line: u64,
    byte: u64,
    ended: bool,
    error: Option<io::Error>,
}
impl Intake {
    pub(super) fn new() -> Self {
        Self {
            record: 0,
            line: 1,
            byte: 1,
            ended: false,
            error: None,
        }
    }
    pub(super) fn next(
        &mut self,
        reader: &mut (impl BufRead + ?Sized),
        record: &mut Record,
    ) -> Result<bool, Failure> {
        record.bytes.clear();
        record.ends.clear();
        if self.ended {
            return Ok(false);
        }
        record.location = Location {
            record: self
                .record
                .checked_add(1)
                .ok_or_else(|| unsupported("CSV record count exceeds u64 limit"))?,
            line: self.line,
        };
        let mut state = State::Start;
        let mut started = false;
        loop {
            let available = match reader.fill_buf() {
                Ok(bytes) => bytes,
                Err(error) => {
                    self.ended = true;
                    self.error = Some(error);
                    return Ok(false);
                }
            };
            if available.is_empty() {
                self.ended = true;
                match state {
                    State::Quoted => {
                        return Err(self.syntax(
                            record.location,
                            "unexpected end of input in quoted field",
                            None,
                        ));
                    }
                    State::Cr { line, byte } => {
                        return Err(self.syntax(
                            record.location,
                            "bare CR outside quoted field",
                            Some((line, byte)),
                        ));
                    }
                    _ if !started => return Ok(false),
                    _ => {
                        record.end_field()?;
                        self.record = record.location.record;
                        return Ok(true);
                    }
                }
            }
            let mut consumed = 0;
            let mut complete = false;
            while consumed < available.len() {
                let tail = &available[consumed..];
                // Closed quotes and pending CR always validate the next byte.
                // Quoted commas and CR are data; LF still changes the location.
                let ordinary = match state {
                    State::Start | State::Unquoted => tail
                        .iter()
                        .position(|&byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
                        .unwrap_or(tail.len()),
                    State::Quoted => tail
                        .iter()
                        .position(|&byte| matches!(byte, b'"' | b'\n'))
                        .unwrap_or(tail.len()),
                    State::Closed | State::Cr { .. } => 0,
                };
                if ordinary != 0 {
                    record.append(&tail[..ordinary])?;
                    let width = u64::try_from(ordinary)
                        .map_err(|_| unsupported("CSV byte position exceeds u64 limit"))?;
                    self.byte = self
                        .byte
                        .checked_add(width)
                        .ok_or_else(|| unsupported("CSV byte position exceeds u64 limit"))?;
                    // The span is bounded by the remaining slice.
                    consumed += ordinary;
                    started = true;
                    if matches!(state, State::Start) {
                        state = State::Unquoted;
                    }
                    continue;
                }
                let byte = available[consumed];
                started = true;
                match state {
                    State::Cr { line, byte: column } => {
                        if byte != b'\n' {
                            return Err(self.syntax(
                                record.location,
                                "bare CR outside quoted field",
                                Some((line, column)),
                            ));
                        }
                        complete = true;
                    }
                    State::Quoted => {
                        if byte == b'"' {
                            state = State::Closed;
                        } else {
                            record.append(&[byte])?;
                        }
                    }
                    State::Start | State::Unquoted | State::Closed => match byte {
                        b',' => {
                            record.end_field()?;
                            state = State::Start;
                        }
                        b'\n' => {
                            record.end_field()?;
                            complete = true;
                        }
                        b'\r' => {
                            record.end_field()?;
                            state = State::Cr {
                                line: self.line,
                                byte: self.byte,
                            };
                        }
                        b'"' => match state {
                            State::Start => state = State::Quoted,
                            State::Closed => {
                                record.append(b"\"")?;
                                state = State::Quoted;
                            }
                            _ => {
                                return Err(self.syntax(
                                    record.location,
                                    "quote in unquoted field",
                                    Some((self.line, self.byte)),
                                ));
                            }
                        },
                        _ if matches!(state, State::Closed) => {
                            return Err(self.syntax(
                                record.location,
                                "byte after closing quote",
                                Some((self.line, self.byte)),
                            ));
                        }
                        _ => {
                            record.append(&[byte])?;
                            state = State::Unquoted;
                        }
                    },
                }
                if byte == b'\n' {
                    self.line = self
                        .line
                        .checked_add(1)
                        .ok_or_else(|| unsupported("CSV physical line count exceeds u64 limit"))?;
                    self.byte = 1;
                } else {
                    self.byte = self
                        .byte
                        .checked_add(1)
                        .ok_or_else(|| unsupported("CSV byte position exceeds u64 limit"))?;
                }
                consumed += 1;
                if complete {
                    break;
                }
            }
            reader.consume(consumed);
            if complete {
                self.record = record.location.record;
                return Ok(true);
            }
        }
    }
    fn syntax(&self, location: Location, reason: &str, detected: Option<(u64, u64)>) -> Failure {
        let detected = detected.map_or_else(String::new, |(line, byte)| {
            format!(" at physical line {line} byte {byte}")
        });
        failure(
            format!(
                "invalid CSV: {reason}{detected}; record {} starts at physical line {}\n",
                location.record, location.line
            )
            .into_bytes(),
        )
    }
    pub(super) fn finish(self) -> Result<(), Failure> {
        self.error
            .map_or(Ok(()), |error| Err(intake::read_failure(&error)))
    }
}

pub(super) fn calculate(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut impl command_output::CommandOutput,
    options: &options::Options,
    mut binding: super::binding::Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
) -> Result<(), Failure> {
    if options.sort && !binding.keys.is_empty() {
        return super::projected_sort::calculate_decoded(
            reader, output, options, binding, arithmetic, random,
        );
    }
    let mut warning = options.warns_full().then_some(binding.program);
    if !options.header_in
        && let Some(program) = warning.take()
    {
        intake::write_full_warning(program);
    }
    let mut intake = Intake::new();
    let mut spare = Record::default();
    let mut random = random;
    let mut calculation = grouping::Calculation::new(&binding.keys, options);
    while intake.next(reader, &mut spare)? {
        let line = spare.location.record;
        if line == 1 {
            if options.header_in {
                binding.decoded_header(&spare, options)?;
                if let Some(program) = warning.take() {
                    intake::write_full_warning(program);
                }
            } else {
                binding.numbered()?;
            }
            output.first_decoded(spare.view(), &binding.operations, &binding.keys)?;
            if options.header_in {
                continue;
            }
        }
        let mut context = grouping::Context::new(
            &mut binding.operations,
            &binding.keys,
            options,
            arithmetic,
            random.as_deref_mut(),
            output,
        );
        calculation.push_in_place(&mut spare, || Ok(Record::default()), line, &mut context)?;
    }
    if let Some(program) = warning {
        intake::write_full_warning(program);
    }
    let mut context = grouping::Context::new(
        &mut binding.operations,
        &binding.keys,
        options,
        arithmetic,
        None,
        output,
    );
    calculation.finish(0, &mut context)?;
    intake.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_location_counters_refuse_overflow_during_spans_and_control_bytes() {
        for (mut intake, bytes, message) in [
            (
                Intake {
                    byte: u64::MAX - 1,
                    ..Intake::new()
                },
                b"xy".as_slice(),
                b"CSV byte position exceeds u64 limit\n".as_slice(),
            ),
            (
                Intake {
                    byte: u64::MAX - 1,
                    ..Intake::new()
                },
                b"\"xy",
                b"CSV byte position exceeds u64 limit\n",
            ),
            (
                Intake {
                    line: u64::MAX,
                    ..Intake::new()
                },
                b"\n",
                b"CSV physical line count exceeds u64 limit\n",
            ),
            (
                Intake {
                    record: u64::MAX,
                    ..Intake::new()
                },
                b"x",
                b"CSV record count exceeds u64 limit\n",
            ),
        ] {
            let error = intake
                .next(&mut std::io::Cursor::new(bytes), &mut Record::default())
                .expect_err("location overflow must refuse the Record");
            assert_eq!(error.status, 77);
            assert_eq!(error.message, message);
        }
    }
}
