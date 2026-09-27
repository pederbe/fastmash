//! Stable sorting with projected text or original numerical record payloads.
use super::{command_output::CommandOutput, *};
use std::{cmp::Ordering, io::BufRead};
#[path = "projected_spill.rs"]
mod spill;
#[path = "projected_storage.rs"]
mod storage;
use storage::{Original, Packed, Storage};

pub(super) fn supports(operations: &[Operation]) -> bool {
    operations.iter().all(|op| {
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
    })
}
fn allocation() -> Failure {
    unsupported("projected sort memory allocation failed")
}
fn copy(bytes: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut out = Vec::new();
    out.try_reserve_exact(bytes.len())
        .map_err(|_| allocation())?;
    out.extend_from_slice(bytes);
    Ok(out)
}
struct Record<S: Storage> {
    data: S,
    prefix: u64,
    fields: u64,
    sequence: u64,
    comment: bool,
    short_key_len: u8,
}
impl<S: Storage> Record<S> {
    fn key_cache(data: &S) -> (u64, u8) {
        let mut prefix = [0; 8];
        // Nine marks a key layout that the eight-byte prefix cannot fully encode.
        let mut short_key_len = 9;
        if data.key_count() != 0 {
            let key = data.segment(0);
            let count = key.len().min(prefix.len());
            prefix[..count].copy_from_slice(&key[..count]);
            if data.key_count() == 1 && key.len() <= prefix.len() {
                short_key_len = key.len() as u8;
            }
        }
        (u64::from_be_bytes(prefix), short_key_len)
    }
    fn new(data: S, fields: u64, sequence: u64, comment: bool) -> Self {
        let (prefix, short_key_len) = Self::key_cache(&data);
        Self {
            prefix,
            short_key_len,
            data,
            fields,
            sequence,
            comment,
        }
    }
    fn segment(&self, at: usize) -> &[u8] {
        self.data.segment(at)
    }
    fn key_count(&self) -> usize {
        self.data.key_count()
    }
    fn original(&self) -> Option<&[u8]> {
        self.data.original()
    }
    fn compare(&self, other: &Self) -> Ordering {
        self.prefix.cmp(&other.prefix).then_with(|| {
            let keys = if self.short_key_len <= 8 && other.short_key_len <= 8 {
                self.short_key_len.cmp(&other.short_key_len)
            } else {
                self.data.compare_keys(&other.data)
            };
            keys.then(self.sequence.cmp(&other.sequence))
        })
    }
    fn owned_bytes(&self) -> Result<usize, Failure> {
        self.data.owned_bytes()
    }
    fn field<'a>(
        &'a self,
        layout: &[u64],
        field: u64,
        line: u64,
        input: records::Separator,
    ) -> Result<&'a [u8], Failure> {
        if field > self.fields {
            return Err(failure(
                format!(
                    "invalid input: field {field} requested, line {line} has only {} fields\n",
                    self.fields
                )
                .into_bytes(),
            ));
        }
        if let Some(bytes) = self.original() {
            let span = selected_field(bytes, field, line, input)?;
            Ok(&bytes[span.start..span.start + span.length])
        } else {
            Ok(self.segment(
                self.key_count()
                    + layout
                        .binary_search(&field)
                        .expect("selected field in projection"),
            ))
        }
    }
}

fn last_selected<S: Storage>(layout: &[u64], keys: &[u64]) -> u64 {
    keys.iter()
        .chain(if S::ORIGINAL { &[][..] } else { layout })
        .copied()
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn project<S: Storage>(
    bytes: &[u8],
    layout: &[u64],
    keys: &[u64],
    input: records::Separator,
    sequence: u64,
    skip_comments: bool,
    vnlog: bool,
    ignore_case: bool,
    last_selected: u64,
) -> Result<Record<S>, Failure> {
    project_with_scratch::<S>(
        bytes,
        layout,
        keys,
        input,
        sequence,
        skip_comments,
        vnlog,
        ignore_case,
        last_selected,
        &mut Vec::new(),
    )
}

#[allow(clippy::too_many_arguments)]
fn project_with_scratch<S: Storage>(
    bytes: &[u8],
    layout: &[u64],
    keys: &[u64],
    input: records::Separator,
    sequence: u64,
    skip_comments: bool,
    vnlog: bool,
    ignore_case: bool,
    last_selected: u64,
    spans: &mut Vec<std::ops::Range<usize>>,
) -> Result<Record<S>, Failure> {
    let data = if vnlog { annotated::data(bytes) } else { bytes };
    let count = keys
        .len()
        .checked_add(if S::ORIGINAL { 0 } else { layout.len() })
        .ok_or_else(allocation)?;
    spans.clear();
    spans.try_reserve_exact(count).map_err(|_| allocation())?;
    spans.resize(count, 0..0);
    let mut count = 0u64;
    let mut previous_end = 0;
    for span in records::fields(data, input) {
        count = count.checked_add(1).ok_or_else(allocation)?;
        if !S::ORIGINAL
            && let Ok(at) = layout.binary_search(&count)
        {
            spans[keys.len() + at] = span.start..span.start + span.length;
        }
        let start = if input == records::Separator::Whitespace {
            previous_end
        } else {
            span.start
        };
        for (at, &key) in keys.iter().enumerate() {
            if key == count && !vnlog {
                spans[at] = start..span.start + span.length;
            }
        }
        previous_end = span.start + span.length;
        if !vnlog
            && count >= last_selected
            && let records::Separator::Literal(delimiter) = input
        {
            // Remaining delimiters each introduce one field, including a final
            // empty field. No remaining ranges are needed by this projection.
            let remaining = memchr::memchr_iter(delimiter, &data[previous_end..]).count();
            count = count
                .checked_add(u64::try_from(remaining).map_err(|_| allocation())?)
                .ok_or_else(allocation)?;
            break;
        }
    }
    if vnlog {
        // Raw input determines sort keys; annotations are removed only from data.
        let mut previous_end = 0;
        for (index, span) in records::fields(bytes, input).enumerate() {
            for (at, &key) in keys.iter().enumerate() {
                if key == index as u64 + 1 {
                    spans[at] = previous_end..span.start + span.length;
                }
            }
            previous_end = span.start + span.length;
        }
    }
    Ok(Record::new(
        S::build(bytes, data, spans, keys.len(), ignore_case)?,
        count,
        sequence,
        !vnlog && skip_comments && records::is_comment(bytes),
    ))
}
fn language_keys<S: Storage>(
    record: &mut Record<S>,
    raw: &[u8],
    keys: &[u64],
    options: &options::Options,
    collator: &icu_collator::CollatorBorrowed<'_>,
    sort_keys: &mut storage::SortKeys,
) -> Result<(), Failure> {
    let data = if options.vnlog {
        annotated::data(raw)
    } else {
        raw
    };
    let mut identity = Vec::new();
    identity
        .try_reserve_exact(keys.len())
        .map_err(|_| allocation())?;
    identity.resize_with(keys.len(), Vec::new);
    for (index, span) in records::fields(data, options.input).enumerate() {
        for (at, key) in keys.iter().enumerate() {
            if *key == index as u64 + 1 {
                let bytes = &data[span.start..span.start + span.length];
                locale::text(bytes)?;
                identity[at] = copy(bytes)?;
                if options.ignore_case {
                    identity[at].make_ascii_lowercase();
                }
            }
        }
    }
    record.data.language_keys(collator, sort_keys, identity)?;
    (record.prefix, record.short_key_len) = Record::key_cache(&record.data);
    Ok(())
}

// Dedup needs raw records, including VNLog's second header, after the same sort.
pub(super) fn sorted_rows(
    reader: &mut impl BufRead,
    options: &options::Options,
    keys: &[u64],
    mut consume: impl FnMut(Vec<u8>) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let collator = options
        .locale
        .collator()?
        .ok_or_else(|| unsupported("internal language sorter policy missing"))?;
    let mut sort_keys = storage::SortKeys::default();
    let mut sorter = spill::Sorter::<Original>::new()?;
    let mut bytes = Vec::new();
    let mut sequence = 0u64;
    let mut projection_spans = Vec::new();
    let last_selected = last_selected::<Original>(&[], keys);
    let read_error = loop {
        match read(reader, &mut bytes, options.record_end) {
            Ok(false) => break None,
            Err(error) => break Some(error),
            Ok(true) => {}
        }
        if options.vnlog && annotated::skip_data(&bytes) {
            continue;
        }
        let mut record = project_with_scratch::<Original>(
            &bytes,
            &[],
            keys,
            options.input,
            sequence,
            options.skip_comments,
            options.vnlog,
            options.ignore_case,
            last_selected,
            &mut projection_spans,
        )?;
        language_keys(
            &mut record,
            &bytes,
            keys,
            options,
            &collator,
            &mut sort_keys,
        )?;
        if options.vnlog {
            record.data.replace_original(&bytes)?;
        }
        sorter.push(record)?;
        sequence = sequence.checked_add(1).ok_or_else(allocation)?;
    };
    sorter.emit(|record| consume(record.data.into_original()))?;
    if let Some(error) = read_error {
        return Err(error);
    }
    Ok(())
}

fn equal_group<S: Storage>(
    a: &Record<S>,
    b: &Record<S>,
    layout: &[u64],
    keys: &[u64],
    line: u64,
    options: &options::Options,
) -> Result<bool, Failure> {
    // Byte-order literal keys already retain exactly these field bytes (possibly
    // ASCII-folded). Whitespace, annotations and language keys have other spans
    // or representations, so they must still select the original fields.
    let stored = matches!(options.input, records::Separator::Literal(_))
        && !options.vnlog
        && options.locale.collation == locale::Collation::Bytes;
    for (at, &key) in keys.iter().enumerate() {
        let (a, b) = if stored && key != 0 && key <= a.fields && key <= b.fields {
            (a.segment(at), b.segment(at))
        } else {
            // Preserve request-order diagnostics, including the earlier-key
            // mismatch that makes later missing keys irrelevant here.
            (
                a.field(layout, key, line, options.input)?,
                b.field(layout, key, line, options.input)?,
            )
        };
        if !text_order::same_group(a, b, options.ignore_case) {
            return Ok(false);
        }
    }
    Ok(true)
}

struct Selection<'a> {
    layout: &'a [u64],
    keys: &'a [u64],
}

fn finish<S: Storage>(
    record: &Record<S>,
    selection: Selection<'_>,
    operations: &mut [Operation],
    line: u64,
    options: &options::Options,
    arithmetic: &mut numerics::Numerics,
    output: &mut impl CommandOutput,
) -> Result<(), Failure> {
    if options.crosstab {
        let row = record.field(selection.layout, selection.keys[0], line, options.input)?;
        let column = record.field(selection.layout, selection.keys[1], line, options.input)?;
        let value = summarize_cached_at(
            operations,
            0,
            arithmetic,
            options.collapse,
            &options.presentation,
            options.ignore_case,
        )?;
        return output.cell(row, column, &value);
    }
    if options.full {
        let Some(bytes) = record.original() else {
            unreachable!("full output retains original records")
        };
        command_output::full_prefix(output, bytes, options);
    }
    for &key in selection.keys.iter().filter(|_| !options.full) {
        output.key(
            record.field(selection.layout, key, line, options.input)?,
            options.output,
        );
    }
    write_results(output, operations, arithmetic, options)
}

// Generated headers still validate each original field before emitting its part.
fn generated_header<W: Write, S: Storage>(
    record: &Record<S>,
    layout: &[u64],
    keys: &[u64],
    operations: &[Operation],
    options: &options::Options,
    output: &mut command_output::Header<'_, W>,
) -> Result<(), Failure> {
    use headers::Part;
    if let Some(bytes) = record.original() {
        return output.first(bytes, operations, keys);
    }
    if options.full {
        let Some(bytes) = record.original() else {
            unreachable!("full output retains original records")
        };
        return output.first(bytes, operations, keys);
    }
    for &key in keys {
        record.field(layout, key, 1, options.input)?;
        output
            .buffer
            .group_header(format!("field-{key}").as_bytes());
        output.buffer.emit(Part::Separator(options.output));
    }
    for (at, op) in operations.iter().enumerate() {
        let Selector::Single(field) = op.selector else {
            unreachable!()
        };
        record.field(layout, field, 1, options.input)?;
        output
            .buffer
            .emit(Part::Operation(grammar::name(op.kind).as_bytes()));
        headers::parameter(op.kind, options.presentation.profile, |part| {
            output.buffer.emit(part)
        })?;
        output
            .buffer
            .emit(Part::Name(format!("field-{field}").as_bytes()));
        output.buffer.emit(Part::Close);
        output
            .buffer
            .emit(Part::Separator(if at + 1 == operations.len() {
                options.record_end
            } else {
                options.output
            }));
    }
    Ok(())
}

pub(super) fn run(
    reader: &mut impl BufRead,
    transport: &mut impl command_output::Transport,
    options: &options::Options,
    fields: CalculationFields<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let (capacity, line_buffered) = transport.buffering();
    let buffer = headers::output::Buffered::new(transport, capacity, line_buffered)
        .map_err(|e| os_failure(&e, false))?;
    let mut output = command_output::Header::new(buffer, options);
    let result = calculate(reader, options, fields, arithmetic, random, &mut output);
    Ok(command_output::complete(
        output.buffer,
        result,
        command_output::Transport::close,
        report,
    ))
}

fn read(reader: &mut impl BufRead, bytes: &mut Vec<u8>, record_end: u8) -> Result<bool, Failure> {
    records::read_record_terminated(reader, bytes, usize::MAX, record_end).map_err(|error| {
        match error {
            records::ReadError::Io(error) => os_failure(&error, true),
            _ => allocation(),
        }
    })
}

fn calculate<W: Write>(
    reader: &mut impl BufRead,
    options: &options::Options,
    fields: CalculationFields<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    output: &mut command_output::Header<'_, W>,
) -> Result<(), Failure> {
    if options.full
        || options.locale.language()
        || fields
            .operations
            .iter()
            .any(|op| op.kind.uses_borrowed_number())
    {
        calculate_records::<W, Original>(reader, options, fields, arithmetic, random, output)
    } else {
        calculate_records::<W, Packed>(reader, options, fields, arithmetic, random, output)
    }
}
fn calculate_records<W: Write, S: spill::Codec>(
    reader: &mut impl BufRead,
    options: &options::Options,
    fields: CalculationFields<'_>,
    arithmetic: &mut numerics::Numerics,
    mut random: Option<&mut random::RandomState>,
    output: &mut command_output::Header<'_, W>,
) -> Result<(), Failure> {
    let CalculationFields {
        program,
        mut operations,
        names,
        mut keys,
        key_names,
    } = fields;

    let mut sorter = spill::Sorter::<S>::new()?;
    let collator = options.locale.collator()?;
    let mut sort_keys = storage::SortKeys::default();
    let mut bytes = Vec::new();
    if options.header_in {
        loop {
            match read(reader, &mut bytes, options.record_end) {
                Ok(true) => {}
                Ok(false) if keys.contains(&0) => {
                    // GNU still invokes sort with unresolved named keys when
                    // no header exists. Let the installed sorter own its error.
                    command_output::warn_full(options, program);
                    sorted_input::admit()?;
                    let mut session = sorted_input::start(
                        &keys,
                        options.input,
                        options.record_end,
                        options.ignore_case,
                    )?;
                    output.end(options)?;
                    return sorted_input::complete(&mut session, Ok(()), None);
                }
                done => {
                    command_output::warn_full(options, program);
                    return done.and_then(|_| output.end(options));
                }
            }
            if options.vnlog {
                if annotated::prepare(&mut bytes, true)? {
                    break;
                }
            } else if !options.skip_comments || !records::is_comment(&bytes) {
                break;
            }
        }
        for (index, target, field) in
            named_fields::resolve_with(names, &bytes, options.input, options.locale.utf8)?
        {
            apply_named(&mut operations[index], target, field);
        }
        for (index, _, field) in
            named_fields::resolve_with(key_names, &bytes, options.input, options.locale.utf8)?
        {
            keys[index] = field;
        }
        command_output::warn_full(options, program);
        output.first(&bytes, &operations, &keys)?;
    } else {
        command_output::warn_full(options, program);
    }
    link_shared_fields(&mut operations);
    let mut layout = Vec::new();
    layout
        .try_reserve_exact(
            keys.len()
                .checked_add(operations.len())
                .ok_or_else(allocation)?,
        )
        .map_err(|_| allocation())?;
    layout.extend_from_slice(&keys);
    for op in &operations {
        match op.selector {
            Selector::Single(field) => layout.push(field),
            Selector::Pair { left, right } => {
                layout.try_reserve(2).map_err(|_| allocation())?;
                layout.extend([left, right]);
            }
        }
    }
    layout.sort_unstable();
    layout.dedup();
    let last_selected = last_selected::<S>(&layout, &keys);
    let mut sequence = 0u64;
    let mut projection_spans = Vec::new();
    let read_error = loop {
        match read(reader, &mut bytes, options.record_end) {
            Ok(false) => break None,
            Err(error) => break Some(error),
            Ok(true) => {}
        }
        if options.vnlog && annotated::skip_data(&bytes) {
            continue;
        }
        let mut record = project_with_scratch::<S>(
            &bytes,
            &layout,
            &keys,
            options.input,
            sequence,
            options.skip_comments,
            options.vnlog,
            options.ignore_case,
            last_selected,
            &mut projection_spans,
        )?;
        if let Some(collator) = &collator {
            language_keys(
                &mut record,
                &bytes,
                &keys,
                options,
                collator,
                &mut sort_keys,
            )?;
        }
        sorter.push(record)?;
        sequence = sequence.checked_add(1).ok_or_else(allocation)?;
    };
    drop(bytes);
    let mut representative: Option<Record<S>> = None;
    let mut line = u64::from(options.header_in);
    sorter.emit(|record| {
        if record.comment {
            return Ok(());
        }
        line = line.checked_add(1).ok_or_else(allocation)?;
        if line == 1 && options.header_out && !options.header_in {
            generated_header(&record, &layout, &keys, &operations, options, output)?;
        }
        let new_group = match &representative {
            None => true,
            Some(previous) => !equal_group(&record, previous, &layout, &keys, line, options)?,
        };
        if new_group {
            if let Some(previous) = &representative {
                finish(
                    previous,
                    Selection {
                        layout: &layout,
                        keys: &keys,
                    },
                    &mut operations,
                    line,
                    options,
                    arithmetic,
                    output,
                )?;
            }
            for op in &mut operations {
                op.reset();
            }
        }
        let mut keep = false;
        match record.original() {
            Some(bytes) => {
                keep = collect(
                    bytes,
                    &mut operations,
                    line,
                    options,
                    arithmetic,
                    random.as_deref_mut(),
                )?;
            }
            None => {
                for op in &mut operations {
                    let Selector::Single(field) = op.selector else {
                        unreachable!()
                    };
                    keep |= collect_text(
                        op,
                        record.field(&layout, field, line, options.input)?,
                        options.narm,
                        None,
                    )?;
                }
            }
        }
        if new_group || keep {
            representative = Some(record);
        }
        Ok(())
    })?;
    if let Some(previous) = &representative {
        finish(
            previous,
            Selection {
                layout: &layout,
                keys: &keys,
            },
            &mut operations,
            line,
            options,
            arithmetic,
            output,
        )?;
    }
    output.end(options)?;
    if let Some(error) = read_error {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod compact_tests {
    use super::*;
    #[test]
    fn language_conversion_invalidates_short_single_key_cache() {
        let options::Action::Calculate(mut options) =
            options::parse(&["count".into(), "1".into()], b"fastmash", false)
                .ok()
                .unwrap()
        else {
            panic!("calculation")
        };
        options.locale.collation = locale::Collation::Language("en-US");
        let collator = options.locale.collator().ok().unwrap().unwrap();
        let mut rows = Vec::new();
        for bytes in [b"a".as_slice(), b"A", b"ab", b"a-b"] {
            let mut row =
                project::<Original>(bytes, &[1], &[1], options.input, 0, false, false, false, 1)
                    .ok()
                    .unwrap();
            assert_eq!(row.short_key_len, bytes.len() as u8);
            language_keys(
                &mut row,
                bytes,
                &[1],
                &options,
                &collator,
                &mut storage::SortKeys::default(),
            )
            .ok()
            .unwrap();
            assert_eq!(row.key_count(), 2);
            assert_eq!(row.short_key_len, 9);
            rows.push(row);
        }
        for a in &rows {
            for b in &rows {
                assert_eq!(a.compare(b), a.data.compare_keys(&b.data));
            }
        }
    }
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn short_key_cache_preserves_record_footprint() {
        assert_eq!(std::mem::size_of::<Record<Original>>(), 64);
        assert_eq!(std::mem::size_of::<Record<Packed>>(), 72);
    }
    fn stored_group_equivalence<S: Storage>() {
        let options::Action::Calculate(mut options) =
            options::parse(&["count".into(), "1".into()], b"fastmash", false)
                .ok()
                .unwrap()
        else {
            panic!("calculation")
        };
        let inputs: &[&[u8]] = &[
            b"",
            b"a",
            b"A",
            b"a\tb\t",
            b"A\tb\t",
            b"z\tc",
            b"a\0x\tb",
            b"a\0y\tb",
            b"a\0yy\tb",
            b"\xff\t\xfe",
        ];
        for delimiter in [0, b'\t', b'.', 0xff] {
            options.input = records::Separator::Literal(delimiter);
            for fold in [false, true] {
                options.ignore_case = fold;
                for keys in [vec![1], vec![2, 1, 2], vec![1, 3], vec![3, 1]] {
                    let layout = vec![1, 2, 3];
                    for &left in inputs {
                        for &right in inputs {
                            let make = |bytes| {
                                project::<S>(
                                    bytes,
                                    &layout,
                                    &keys,
                                    options.input,
                                    0,
                                    false,
                                    false,
                                    fold,
                                    last_selected::<S>(&layout, &keys),
                                )
                                .ok()
                                .unwrap()
                            };
                            let a = make(left);
                            let b = make(right);
                            let old = || -> Result<bool, Failure> {
                                for &key in &keys {
                                    if !text_order::same_group(
                                        a.field(&layout, key, 7, options.input)?,
                                        b.field(&layout, key, 7, options.input)?,
                                        fold,
                                    ) {
                                        return Ok(false);
                                    }
                                }
                                Ok(true)
                            };
                            assert_eq!(
                                equal_group(&a, &b, &layout, &keys, 7, &options)
                                    .map_err(|e| e.message),
                                old().map_err(|e| e.message)
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn stored_group_keys_preserve_raw_comparison_and_missing_error_order() {
        stored_group_equivalence::<Original>();
        stored_group_equivalence::<Packed>();
    }
    fn tail_counts<S: Storage>() {
        for delimiter in [0, b'\t', b',', b' ', 0xff] {
            let input = records::Separator::Literal(delimiter);
            for length in [0, 1, 15, 16, 17, 31, 32, 33, 63, 64, 65, 4097] {
                let mut raw = vec![b'x'; length];
                for at in (0..length).step_by(3) {
                    raw[at] = delimiter;
                }
                for (layout, keys) in [
                    (vec![1, 2], vec![1]),
                    (vec![1, 2, 4], vec![3, 1, 3]),
                    (vec![1, 2, 9000], vec![1]),
                    (vec![2], vec![9000]),
                    (vec![2], vec![]),
                ] {
                    let optimized = project::<S>(
                        &raw,
                        &layout,
                        &keys,
                        input,
                        0,
                        false,
                        false,
                        false,
                        last_selected::<S>(&layout, &keys),
                    )
                    .ok()
                    .unwrap();
                    // An unreachable selected index forces the existing full
                    // field walk, independently of delimiter counting.
                    let full = project::<S>(
                        &raw,
                        &layout,
                        &keys,
                        input,
                        0,
                        false,
                        false,
                        false,
                        u64::MAX,
                    )
                    .ok()
                    .unwrap();
                    assert_eq!(optimized.fields, full.fields);
                    assert_eq!(optimized.compare(&full), Ordering::Equal);
                    assert_eq!(optimized.original(), full.original());
                    for field in layout.iter().copied().chain([9001]) {
                        let observed = optimized
                            .field(&layout, field, 7, input)
                            .map_err(|e| (e.status, e.message));
                        let expected = full
                            .field(&layout, field, 7, input)
                            .map_err(|e| (e.status, e.message));
                        assert_eq!(observed, expected);
                    }
                }
            }
        }
    }
    #[test]
    fn unused_tail_keeps_fields_keys_payloads_and_missing_field_errors() {
        tail_counts::<Packed>();
        tail_counts::<Original>();
    }

    fn boundaries<S: Storage>() {
        let layout = [1, 2, 3, 4];
        let input = records::Separator::Literal(b'\t');
        let row = project::<S>(
            b"aB\t\t\0\xff",
            &layout,
            &[3, 1, 3, 4],
            input,
            7,
            false,
            false,
            true,
            last_selected::<S>(&layout, &[3, 1, 3, 4]),
        )
        .ok()
        .unwrap();
        assert_eq!(row.segment(0), b"\0\xff");
        assert_eq!(row.segment(1), b"AB");
        assert_eq!(row.segment(2), b"\0\xff");
        assert_eq!(row.segment(3), b"");
        assert_eq!(row.field(&layout, 1, 8, input).ok().unwrap(), b"aB");
        assert_eq!(row.field(&layout, 2, 8, input).ok().unwrap(), b"");
        assert!(row.field(&layout, 4, 8, input).is_err());
        if S::ORIGINAL {
            assert_eq!(row.original(), Some(b"aB\t\t\0\xff".as_slice()));
        }
        let make = |raw: &[u8], keys: &[u64], sequence| {
            project::<S>(
                raw,
                &[1, 2],
                keys,
                input,
                sequence,
                false,
                false,
                false,
                last_selected::<S>(&[1, 2], keys),
            )
            .ok()
            .unwrap()
        };
        let a = make(b"a\tbc", &[1, 2], 1);
        let b = make(b"ab\tc", &[1, 2], 0);
        let short = make(b"a\tbc", &[1], 9);
        let tied = make(b"a\tbc", &[1, 2], 2);
        assert_eq!(a.compare(&b), Ordering::Less);
        assert_eq!(short.compare(&a), Ordering::Less);
        assert_eq!(a.compare(&tied), Ordering::Less);
        assert_eq!(a.compare(&a), Ordering::Equal);
        assert_eq!(b.compare(&a), Ordering::Greater);
    }
    #[test]
    fn projection_preserves_distinct_keys_and_raw_payloads() {
        boundaries::<Packed>();
        boundaries::<Original>();
    }
    fn annotated<S: Storage>() {
        let row = project::<S>(
            b"  aB  cD # note",
            &[1, 2, 3],
            &[2, 3, 1],
            records::Separator::Whitespace,
            0,
            false,
            true,
            true,
            last_selected::<S>(&[1, 2, 3], &[2, 3, 1]),
        )
        .ok()
        .unwrap();
        assert_eq!(row.segment(0), b"  CD");
        assert_eq!(row.segment(1), b" #");
        assert_eq!(row.segment(2), b"  AB");
        assert_eq!(row.fields, 2);
        assert_eq!(
            row.field(&[1, 2, 3], 2, 1, records::Separator::Whitespace)
                .ok()
                .unwrap(),
            b"cD"
        );
        if S::ORIGINAL {
            assert_eq!(row.original(), Some(b"  aB  cD".as_slice()));
        }
    }
    #[test]
    fn whitespace_and_vnlog_keep_raw_keys_separate() {
        annotated::<Packed>();
        annotated::<Original>();
    }
    fn reusable_projection<S: spill::Codec>() {
        let mut scratch = Vec::new();
        let keys = [3, 1, 3, 4];
        let layout = [1, 2, 3, 4];
        let mut rows = Vec::new();
        for (sequence, bytes) in [b"b\t2\tx".as_slice(), b"a", b"\t\t\t"]
            .into_iter()
            .enumerate()
        {
            rows.push(
                project_with_scratch::<S>(
                    bytes,
                    &layout,
                    &keys,
                    records::Separator::Literal(b'\t'),
                    sequence as u64,
                    false,
                    false,
                    true,
                    last_selected::<S>(&layout, &keys),
                    &mut scratch,
                )
                .ok()
                .unwrap(),
            );
        }
        assert_eq!(
            (
                rows[0].segment(0),
                rows[0].segment(1),
                rows[0].segment(2),
                rows[0].segment(3)
            ),
            (
                b"X".as_slice(),
                b"B".as_slice(),
                b"X".as_slice(),
                b"".as_slice()
            )
        );
        assert_eq!(
            (rows[1].segment(0), rows[1].segment(1), rows[1].segment(2)),
            (b"".as_slice(), b"A".as_slice(), b"".as_slice())
        );
        assert!((0..4).all(|at| rows[2].segment(at).is_empty()));
        assert_eq!(
            rows[0]
                .field(&layout, 3, 1, records::Separator::Literal(b'\t'))
                .ok()
                .unwrap(),
            b"x"
        );
        assert!(
            rows[1]
                .field(&layout, 3, 2, records::Separator::Literal(b'\t'))
                .is_err()
        );
        // Reconstruct the established spill payload independently of storage.
        let segments: Vec<&[u8]> = if S::ORIGINAL {
            vec![b"X", b"B", b"X", b"", b"b\t2\tx"]
        } else {
            vec![b"X", b"B", b"X", b"", b"b", b"2", b"x", b""]
        };
        let mut expected = Vec::new();
        if S::ORIGINAL {
            for value in &segments {
                expected.extend_from_slice(&(value.len() as u64).to_le_bytes());
                expected.extend_from_slice(value);
            }
        } else {
            for value in &segments {
                expected.extend_from_slice(&(value.len() as u64).to_le_bytes());
            }
            for value in &segments {
                expected.extend_from_slice(value);
            }
        }
        let mut encoded = Vec::new();
        rows[0].data.write_payload(&mut encoded).unwrap();
        assert_eq!(encoded, expected);
        let decoded = S::read_payload(&mut expected.as_slice(), 4, if S::ORIGINAL { 1 } else { 4 })
            .ok()
            .unwrap();
        for (at, bytes) in segments.iter().enumerate() {
            assert_eq!(decoded.segment(at), *bytes);
        }
    }
    #[test]
    fn projection_scratch_does_not_leak_fields_or_change_spill_payloads() {
        reusable_projection::<Original>();
        reusable_projection::<Packed>();
    }
    #[test]
    fn original_replacement_and_extraction_preserve_keys_and_owned_bytes() {
        let mut record = project::<Original>(
            b"  aB cD # note",
            &[],
            &[2, 1],
            records::Separator::Whitespace,
            0,
            false,
            true,
            true,
            2,
        )
        .ok()
        .unwrap();
        let original = b"  aB cD # note plus a much longer annotation";
        record.data.replace_original(original).ok().unwrap();
        assert_eq!(record.segment(0), b" CD");
        assert_eq!(record.segment(1), b"  AB");
        assert_eq!(record.data.into_original(), original);
        let mut empty = Original::build(b"raw", b"raw", &[], 0, false).ok().unwrap();
        empty.replace_original(b"").ok().unwrap();
        assert_eq!(empty.into_original(), b"");
    }
}
