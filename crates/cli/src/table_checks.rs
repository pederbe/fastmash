//! Validation and first-occurrence deduplication over streaming records.
use super::{
    Failure,
    buffered_stdout::BufferedStdout,
    command_memory::reserve,
    failure,
    grammar::Field,
    intake::{self, Accepted, Filter, Intake},
    named_fields,
    options::Options,
    random::TableState,
    records, sorted_input, standard_io, unsupported,
};
use std::{
    collections::HashSet,
    io::{BufRead, Write},
};

fn excerpt(line: u64, width: u64, record: &[u8]) {
    let mut stderr = standard_io::Stderr;
    let _ = write!(stderr, "line {line} ({width} fields):\n  ");
    let _ = stderr.write_all(record);
    let _ = stderr.write_all(b"\n");
}

pub(super) fn check<W: Write>(
    reader: &mut impl BufRead,
    output: &mut BufferedStdout<'_, W>,
    options: &Options,
    lines: u64,
    fields: u64,
) -> Result<(), Failure> {
    let mut record = Vec::new();
    let mut previous = Vec::new();
    let mut width = 0;
    // GNU's check reads no Input header (datamash.c tabular_check_file).
    let mut intake = Intake::new(options, intake::Header::None);
    while let Some(row) = intake.next(reader, &mut record)? {
        let current = records::fields(row.data(), options.input).count() as u64;
        let line = intake.line();
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
    }
    let line = intake.line();
    if lines != 0 && line != lines {
        return Err(failure(
            format!("check failed: input had {line} lines (expecting {lines})\n").into_bytes(),
        ));
    }
    let ls = if line == 1 { "" } else { "s" };
    let fs = if width == 1 { "" } else { "s" };
    output.formatted(format!("{line} line{ls}, {width} field{fs}").as_bytes());
    output.raw(&[options.record_end]);
    intake.finish()
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
    output: &mut BufferedStdout<'_, W>,
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
        let intake = Intake::new(options, intake::Header::of(options));
        return dedup_records(reader, output, options, key, intake);
    }
    options.locale.sorting()?;
    // No Operation of rmdup is covered: only its locale or an unusable system
    // sort keeps it in process.
    if super::sort_route::native(options.locale.language(), false, || {
        sorted_input::available(options.input)
    }) {
        return native_dedup(reader, output, options, key);
    }
    let sorting = sorted_input::Sorting::prepare(sorted_input::admit(&options.locale)?, options)?;
    if let Some(record) = sorting.header() {
        resolve(&mut key, record, options)?;
    }
    let number = match &key {
        Field::Number(n) => *n,
        Field::Name(_) => 0,
    };
    sorting.start(&[number], options)?.run(|reader, header| {
        // GNU reads a second header here even after open_input consumed the
        // first (datamash.c remove_dups_in_file).
        let lines = u64::from(header.is_some());
        let sorted = Intake::new(options, intake::Header::of(options)).after(lines);
        dedup_records(reader, output, options, key, sorted)
    })
}

fn row<W: Write>(output: &mut BufferedStdout<'_, W>, record: &[u8], options: &Options) {
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
    seen: HashSet<Vec<u8>, TableState>,
}
impl Dedup {
    fn new(key: Field) -> Self {
        Self {
            key,
            seen: HashSet::default(),
        }
    }
    fn header<W: Write>(
        &mut self,
        header: &[u8],
        output: &mut BufferedStdout<'_, W>,
        options: &Options,
    ) -> Result<(), Failure> {
        resolve(&mut self.key, header, options)?;
        if options.header_out {
            if options.vnlog {
                output.raw(b"# ");
            }
            row(output, header, options);
        }
        Ok(())
    }
    fn row<W: Write>(
        &mut self,
        data: &[u8],
        line: u64,
        output: &mut BufferedStdout<'_, W>,
        options: &Options,
    ) -> Result<(), Failure> {
        let Field::Number(number) = self.key else {
            return Err(unsupported("internal dedup name unresolved"));
        };
        let span = super::selected_field(data, number, line, options.input)?;
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
    output: &mut BufferedStdout<'_, W>,
    options: &Options,
    key: Field,
    mut intake: Intake<'_>,
) -> Result<(), Failure> {
    let mut state = Dedup::new(key);
    let mut record = Vec::new();
    intake.header(reader, &mut record, |header| {
        state.header(header, output, options)
    })?;
    while let Some(data) = intake.next(reader, &mut record)? {
        state.row(data.data(), intake.line(), output, options)?;
    }
    intake.finish()
}
fn native_dedup<W: Write>(
    reader: &mut impl BufRead,
    output: &mut BufferedStdout<'_, W>,
    options: &Options,
    mut key: Field,
) -> Result<(), Failure> {
    let mut intake = Intake::new(options, intake::Header::of(options));
    let mut record = Vec::new();
    if !intake.header(reader, &mut record, |header| {
        resolve(&mut key, header, options)
    })? && matches!(key, Field::Name(_))
    {
        // No rows can be ordered. Preserve the existing fixed-C sorter's
        // unresolved-key diagnostic, not host collation.
        let errno = intake.read_error().and_then(std::io::Error::raw_os_error);
        let sorting = sorted_input::Sorting::read(sorted_input::admit(&options.locale)?, errno);
        return sorting.start(&[0], options)?.run(|_, _| Ok(()));
    }
    let Field::Number(number) = key else {
        return Err(unsupported("internal dedup name unresolved"));
    };
    let mut state = Dedup::new(key);
    // As from GNU's sort pipe, a second Input header is read from the sorted
    // Records (datamash.c remove_dups_in_file). The sort sees every vnlog
    // Record, since any may become that header; -C comments cannot, and are
    // dropped before sorting.
    let mut sorted = Intake::new(options, intake::Header::of(options)).after(intake.line());
    let unsorted = intake.with_filter(if options.vnlog {
        Filter::None
    } else {
        Filter::of(options)
    });
    super::projected_sort::sorted_rows(reader, unsorted, options, &[number], |record| match sorted
        .accept(
        record,
    )? {
        Accepted::Skipped => Ok(()),
        Accepted::Header => state.header(record, output, options),
        Accepted::Data(data) => state.row(&record[..data], sorted.line(), output, options),
    })
}
