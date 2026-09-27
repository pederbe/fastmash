//! Validation and first-occurrence deduplication over streaming records.
use super::{
    Failure, annotated, command_memory::reserve, failure, grammar::Field,
    headers::output::Buffered, named_fields, options::Options, os_failure, records, sorted_input,
    standard_io, unsupported,
};
use std::{
    collections::HashSet,
    io::{BufRead, Write},
};

fn read(
    reader: &mut (impl BufRead + ?Sized),
    record: &mut Vec<u8>,
    options: &Options,
) -> Result<bool, Failure> {
    records::read_record_terminated(reader, record, usize::MAX, options.record_end).map_err(|e| {
        match e {
            records::ReadError::Io(e) => os_failure(&e, true),
            _ => unsupported("record memory allocation failed"),
        }
    })
}
fn skip(record: &[u8], options: &Options) -> bool {
    if options.vnlog {
        annotated::skip_data(record)
    } else {
        options.skip_comments && records::is_comment(record)
    }
}
fn view<'a>(record: &'a [u8], options: &Options) -> &'a [u8] {
    if options.vnlog {
        annotated::data(record)
    } else {
        record
    }
}
fn excerpt(line: u64, width: u64, record: &[u8]) {
    let mut stderr = standard_io::Stderr;
    let _ = write!(stderr, "line {line} ({width} fields):\n  ");
    let _ = stderr.write_all(record);
    let _ = stderr.write_all(b"\n");
}

pub(super) fn check<W: Write>(
    reader: &mut impl BufRead,
    output: &mut Buffered<'_, W>,
    options: &Options,
    lines: u64,
    fields: u64,
) -> Result<(), Failure> {
    let mut record = Vec::new();
    let mut previous = Vec::new();
    let mut line = 0u64;
    let mut width = 0;
    let read_error = loop {
        match read(reader, &mut record, options) {
            Ok(false) => break None,
            Ok(true) => (),
            Err(e) => break Some(e),
        }
        if skip(&record, options) {
            continue;
        }
        line = line
            .checked_add(1)
            .ok_or_else(|| unsupported("record count exceeds u64 limit"))?;
        let current = records::fields(view(&record, options), options.input).count() as u64;
        if fields != 0 && fields != current {
            excerpt(line, current, &record);
            return Err(failure(
                format!("check failed: line {line} has {current} fields (expecting {fields})\n")
                    .into_bytes(),
            ));
        }
        if line > 1 && width != current {
            excerpt(line - 1, width, &previous);
            excerpt(line, current, &record);
            return Err(failure(
                format!(
                    "check failed: line {line} has {current} fields (previous line had {width})\n"
                )
                .into_bytes(),
            ));
        }
        width = current;
        std::mem::swap(&mut record, &mut previous);
    };
    if lines != 0 && line != lines {
        return Err(failure(
            format!("check failed: input had {line} lines (expecting {lines})\n").into_bytes(),
        ));
    }
    let ls = if line == 1 { "" } else { "s" };
    let fs = if width == 1 { "" } else { "s" };
    output.formatted(format!("{line} line{ls}, {width} field{fs}").as_bytes());
    output.raw(&[options.record_end]);
    match read_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn resolve(key: &mut Field, record: &[u8], options: &Options) -> Result<(), Failure> {
    if let Field::Name(name) = key {
        let requests = [named_fields::Named {
            operation: 0,
            target: named_fields::Target::Single,
            name: std::mem::take(name),
        }];
        let resolved =
            named_fields::resolve_with(&requests, record, options.input, options.locale.utf8)?;
        *key = Field::Number(resolved[0].2);
    }
    Ok(())
}

pub(super) fn dedup<W: Write>(
    reader: &mut impl BufRead,
    output: &mut Buffered<'_, W>,
    options: &Options,
    mut keys: Vec<Field>,
) -> Result<(), Failure> {
    let mut key = keys
        .pop()
        .ok_or_else(|| unsupported("internal dedup key missing"))?;
    if matches!(key, Field::Name(_)) && !options.header_in {
        return Err(failure(
            b"-H or --header-in must be used with named columns\n".to_vec(),
        ));
    }
    if !options.sort {
        return dedup_records(reader, output, options, key, 0);
    }
    options.locale.sorting()?;
    if options.locale.language() {
        return language_dedup(reader, output, options, key);
    }
    sorted_input::admit()?;
    standard_io::check_input_access().map_err(|e| os_failure(&e, true))?;
    let prepared = sorted_input::prepare(options)?;
    let mut line = 0;
    let mut errno = None;
    match prepared {
        sorted_input::Header::Record(record) => {
            resolve(&mut key, &record, options)?;
            line = 1;
        }
        sorted_input::Header::ReadError(e) => errno = e.raw_os_error(),
        _ => (),
    }
    let number = match &key {
        Field::Number(n) => *n,
        Field::Name(_) => 0,
    };
    let mut session = sorted_input::start(
        &[number],
        options.input,
        options.record_end,
        options.ignore_case,
    )?;
    // GNU reads a second header here even after open_input consumed the first.
    let result = dedup_records(session.reader(), output, options, key, line);
    sorted_input::complete(&mut session, result, errno)
}

fn row<W: Write>(output: &mut Buffered<'_, W>, record: &[u8], options: &Options) {
    for (at, span) in records::fields(record, options.input).enumerate() {
        if at != 0 {
            output.raw(&[options.output]);
        }
        output.raw(&record[span.start..span.start + span.length]);
    }
    output.raw(&[options.record_end]);
}

struct Dedup {
    key: Field,
    line: u64,
    need_header: bool,
    seen: HashSet<Vec<u8>>,
}
impl Dedup {
    fn new(key: Field, line: u64, options: &Options) -> Self {
        Self {
            key,
            line,
            need_header: options.header_in,
            seen: HashSet::new(),
        }
    }
    fn consume<W: Write>(
        &mut self,
        record: &mut Vec<u8>,
        output: &mut Buffered<'_, W>,
        options: &Options,
    ) -> Result<(), Failure> {
        if self.need_header && options.vnlog {
            if !annotated::prepare(record, true)? {
                return Ok(());
            }
        } else if skip(record, options) {
            return Ok(());
        }
        self.line = self
            .line
            .checked_add(1)
            .ok_or_else(|| unsupported("record count exceeds u64 limit"))?;
        if self.need_header {
            resolve(&mut self.key, record, options)?;
            if options.header_out {
                if options.vnlog {
                    output.raw(b"# ");
                }
                row(output, record, options);
            }
            self.need_header = false;
            return Ok(());
        }
        let Field::Number(number) = self.key else {
            return Err(unsupported("internal dedup name unresolved"));
        };
        let data = view(record, options);
        let span = super::selected_field(data, number, self.line, options.input)?;
        let bytes = &data[span.start..span.start + span.length];
        let key = bytes.split(|b| *b == 0).next().unwrap();
        if self.seen.contains(key) {
            return Ok(());
        }
        self.seen
            .try_reserve(1)
            .map_err(|_| unsupported("dedup key memory allocation failed"))?;
        let mut owned = Vec::new();
        reserve(&mut owned, key.len())?;
        owned.extend_from_slice(key);
        self.seen.insert(owned);
        row(output, data, options);
        Ok(())
    }
}
fn dedup_records<W: Write>(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut Buffered<'_, W>,
    options: &Options,
    key: Field,
    line: u64,
) -> Result<(), Failure> {
    let mut state = Dedup::new(key, line, options);
    let mut record = Vec::new();
    while read(reader, &mut record, options)? {
        state.consume(&mut record, output, options)?;
    }
    Ok(())
}
fn language_dedup<W: Write>(
    reader: &mut impl BufRead,
    output: &mut Buffered<'_, W>,
    options: &Options,
    mut key: Field,
) -> Result<(), Failure> {
    let mut line = 0;
    if options.header_in {
        let mut record = Vec::new();
        loop {
            if !read(reader, &mut record, options)? {
                if matches!(key, Field::Name(_)) {
                    // No rows can be ordered. Preserve the existing fixed-C
                    // sorter's unresolved-key diagnostic, not host collation.
                    sorted_input::admit()?;
                    let mut session = sorted_input::start(
                        &[0],
                        options.input,
                        options.record_end,
                        options.ignore_case,
                    )?;
                    return sorted_input::complete(&mut session, Ok(()), None);
                }
                return Ok(());
            }
            if options.vnlog {
                if !annotated::prepare(&mut record, true)? {
                    continue;
                }
            } else if skip(&record, options) {
                continue;
            }
            resolve(&mut key, &record, options)?;
            line = 1;
            break;
        }
    }
    let Field::Number(number) = key else {
        return Err(unsupported("internal dedup name unresolved"));
    };
    let mut state = Dedup::new(key, line, options);
    super::projected_sort::sorted_rows(reader, options, &[number], |mut record| {
        state.consume(&mut record, output, options)
    })
}
