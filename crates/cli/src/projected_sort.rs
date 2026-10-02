//! Stable sorting with projected text or original numerical record payloads.
use super::{
    command_output::CommandOutput,
    intake::{self, Intake},
    *,
};
use std::{
    cmp::Ordering,
    io::{self, BufRead},
};
#[path = "projected_batch.rs"]
mod batch;
#[path = "projected_hash.rs"]
mod hash;
pub(super) use hash::{Setting as Grouping, eligible as hash_eligible, setting as grouping};
#[path = "projected_memory.rs"]
mod memory;
#[path = "projected_spill.rs"]
mod spill;
#[path = "projected_storage.rs"]
mod storage;
#[cfg(test)]
use storage::{Build, Keys};
use storage::{Original, Packed, Storage};

fn allocation() -> Failure {
    unsupported("projected sort memory allocation failed")
}

/// Marks a key layout that the eight-byte prefix cannot fully encode.
const LONG_KEY: u8 = 9;

/// A record's key cache (`key_cache`): its key prefix and key length.
type KeyCache = (u64, u8);

/// The sort's key cache: the first key's first eight bytes, zero-padded, and
/// the key's length when that prefix holds the whole key layout (one key of
/// at most eight bytes), else `LONG_KEY`.
fn key_cache(first: Option<&[u8]>, key_count: usize) -> KeyCache {
    let mut prefix = [0; 8];
    let mut short_key_len = LONG_KEY;
    if let Some(key) = first {
        let count = key.len().min(prefix.len());
        prefix[..count].copy_from_slice(&key[..count]);
        if key_count == 1 && key.len() <= prefix.len() {
            short_key_len = key.len() as u8;
        }
    }
    (u64::from_be_bytes(prefix), short_key_len)
}

struct Record<S: Storage> {
    data: S,
    prefix: u64,
    fields: u64,
    sequence: u64,
    short_key_len: u8,
}
impl<S: Storage> Record<S> {
    fn key_cache(data: &S) -> KeyCache {
        let count = data.key_count();
        key_cache((count != 0).then(|| data.segment(0)), count)
    }
    fn new(data: S, fields: u64, sequence: u64) -> Self {
        let (prefix, short_key_len) = Self::key_cache(&data);
        Self {
            prefix,
            short_key_len,
            data,
            fields,
            sequence,
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
            let keys = if self.short_key_len < LONG_KEY && other.short_key_len < LONG_KEY {
                self.short_key_len.cmp(&other.short_key_len)
            } else {
                self.data.compare_keys(&other.data)
            };
            keys.then(self.sequence.cmp(&other.sequence))
        })
    }
    fn field<'a>(
        &'a self,
        layout: &[u64],
        field: u64,
        line: u64,
        input: records::Separator,
    ) -> Result<&'a [u8], Failure> {
        self.indexed_field(
            layout,
            &mut records::FieldIndex::default(),
            field,
            line,
            input,
        )
    }
    /// [`Record::field`], locating the fields of an original record through
    /// `fields`, its index.
    fn indexed_field<'a>(
        &'a self,
        layout: &[u64],
        fields: &mut records::FieldIndex,
        field: u64,
        line: u64,
        input: records::Separator,
    ) -> Result<&'a [u8], Failure> {
        if field > self.fields {
            return Err(missing_field(field, line, self.fields));
        }
        if let Some(bytes) = self.original() {
            let span = indexed_field(bytes, fields, field, line, input)?;
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

/// The positions of `keys` in field order (request order among equal
/// keys), so a projection finds every key in one pass over the fields.
fn key_order(keys: &[u64]) -> Result<Vec<usize>, Failure> {
    let mut order = Vec::new();
    order
        .try_reserve_exact(keys.len())
        .map_err(|_| allocation())?;
    order.extend(0..keys.len());
    order.sort_unstable_by_key(|&at| (keys[at], at));
    Ok(order)
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn project<S: Build>(
    bytes: &[u8],
    layout: &[u64],
    keys: &[u64],
    input: records::Separator,
    sequence: u64,
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
        vnlog,
        ignore_case,
        last_selected,
        &mut Vec::new(),
    )
}

#[cfg(test)]
#[allow(clippy::too_many_arguments)]
fn project_with_scratch<S: Build>(
    bytes: &[u8],
    layout: &[u64],
    keys: &[u64],
    input: records::Separator,
    sequence: u64,
    vnlog: bool,
    ignore_case: bool,
    last_selected: u64,
    spans: &mut Vec<std::ops::Range<usize>>,
) -> Result<Record<S>, Failure> {
    let data = if vnlog { annotated::data(bytes) } else { bytes };
    let count = project_spans::<S>(
        bytes,
        layout,
        keys,
        &key_order(keys)?,
        input,
        vnlog,
        false,
        last_selected,
        spans,
        None,
    )?;
    Ok(Record::new(
        S::build(bytes, data, spans, keys.len(), ignore_case)?,
        count,
        sequence,
    ))
}

/// Fills `spans` with the projected spans of `bytes`: each key's, then (for
/// packed storage) each layout field's. With `identities`, which must hold one
/// empty span per key, also each key's field span within the data, without the
/// leading blanks or annotation spans that key spans may include. With
/// `zero_terminated` blank-separated records that hold a newline, key spans
/// and identities are the fields `sort -z` finds ([`sort_fields`]) instead.
/// `order` is the keys' [`key_order`]. Returns the record's field count.
#[allow(clippy::too_many_arguments)]
fn project_spans<S: Storage>(
    bytes: &[u8],
    layout: &[u64],
    keys: &[u64],
    order: &[usize],
    input: records::Separator,
    vnlog: bool,
    zero_terminated: bool,
    last_selected: u64,
    spans: &mut Vec<std::ops::Range<usize>>,
    mut identities: Option<&mut [std::ops::Range<usize>]>,
) -> Result<u64, Failure> {
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
    // The next key in field order; keys before the first field (0 for an
    // unresolved name) match no field.
    let mut next = order.partition_point(|&at| keys[at] == 0);
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
        while let Some(&at) = order.get(next)
            && keys[at] == count
        {
            if !vnlog {
                spans[at] = start..span.start + span.length;
            }
            if let Some(identities) = identities.as_deref_mut() {
                identities[at] = span.start..span.start + span.length;
            }
            next += 1;
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
    // With `-z`, GNU sort also splits blank-separated keys at a newline
    // (`sort -z` without `-t`), where datamash's fields do not.
    let newline_blanks = zero_terminated && input == records::Separator::Whitespace;
    if vnlog || (newline_blanks && memchr::memchr(b'\n', data).is_some()) {
        // Raw input determines sort keys; annotations are removed only from data.
        let source = if vnlog { bytes } else { data };
        let mut next = order.partition_point(|&at| keys[at] == 0);
        if newline_blanks {
            // A field of newlines alone joins the next sort field, so a key
            // can lie past the last one: its sort key is then empty.
            let end = source.len();
            for &at in &order[next..] {
                spans[at] = end..end;
                if !vnlog && let Some(identities) = identities.as_deref_mut() {
                    identities[at] = end..end;
                }
            }
        }
        let mut place = |index: usize, span: std::ops::Range<usize>| {
            while let Some(&at) = order.get(next)
                && keys[at] == index as u64 + 1
            {
                // A language key's identity is then its sort field, as its
                // field is otherwise: without the blanks before it.
                if !vnlog && let Some(identities) = identities.as_deref_mut() {
                    let blanks = source[span.clone()]
                        .iter()
                        .take_while(|&&b| matches!(b, b' ' | b'\t' | b'\n'))
                        .count();
                    identities[at] = span.start + blanks..span.end;
                }
                spans[at] = span.clone();
                next += 1;
            }
        };
        if newline_blanks {
            for (index, span) in sort_fields(source).enumerate() {
                place(index, span);
            }
        } else {
            let mut previous_end = 0;
            for (index, span) in records::fields(source, input).enumerate() {
                place(index, previous_end..span.start + span.length);
                previous_end = span.start + span.length;
            }
        }
    }
    Ok(count)
}

/// The fields of blank-separated `record` as `sort -z` finds them, each with
/// the blanks before it: a newline is a blank there too.
fn sort_fields(record: &[u8]) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    let blank = |b: &u8| matches!(b, b' ' | b'\t' | b'\n');
    let mut end = 0;
    std::iter::from_fn(move || {
        if end == record.len() {
            return None;
        }
        let start = end;
        end += record[end..].iter().take_while(|b| blank(b)).count();
        end += record[end..].iter().take_while(|b| !blank(b)).count();
        Some(start..end)
    })
}

/// The first key field, in field order, that language sorting cannot read.
fn identity_error(
    data: &[u8],
    keys: &[u64],
    identities: &[std::ops::Range<usize>],
) -> Option<Failure> {
    let mut first: Option<(u64, usize, Failure)> = None;
    for (at, span) in identities.iter().enumerate() {
        if first.as_ref().is_some_and(|(key, _, _)| *key <= keys[at]) {
            continue;
        }
        if let Err(error) = locale::text(&data[span.clone()]) {
            first = Some((keys[at], at, error));
        }
    }
    first.map(|(_, _, error)| error)
}

/// A raw Record's language key segments, in `scratch.language` (`bytes`
/// split at `ends`; [`storage::language_segments`]): each key is sorted by
/// its projected key span and identified by its field. Invalid keys are
/// refused in field order. Returns the record's field count.
fn language_keys(
    bytes: &[u8],
    keys: &[u64],
    order: &[usize],
    options: &options::Options,
    collator: &locale::Collator,
    last_selected: u64,
    scratch: &mut batch::Scratch,
) -> Result<u64, Failure> {
    let batch::Scratch {
        spans,
        sort_keys,
        language,
    } = scratch;
    let storage::LanguageScratch {
        identities,
        bytes: out,
        ends,
        text,
    } = language;
    let data = if options.vnlog {
        annotated::data(bytes)
    } else {
        bytes
    };
    identities.clear();
    identities
        .try_reserve_exact(keys.len())
        .map_err(|_| allocation())?;
    identities.resize(keys.len(), 0..0);
    let fields = project_spans::<Original>(
        bytes,
        &[],
        keys,
        order,
        options.input,
        options.vnlog,
        options.record_end == 0,
        last_selected,
        spans,
        Some(identities),
    )?;
    // Annotated key spans come from the raw record, so they are checked
    // separately after the fields. Otherwise a key span is its field, possibly
    // after ASCII blanks, and is valid exactly when the field is.
    if options.vnlog
        && let Some(error) = identity_error(data, keys, identities)
    {
        return Err(error);
    }
    let identities = &*identities;
    storage::language_segments(
        keys.len(),
        |at| match locale::text(&bytes[spans[at].clone()]) {
            Ok(text) => Ok(text),
            Err(error) if options.vnlog => Err(error),
            Err(error) => Err(identity_error(data, keys, identities).unwrap_or(error)),
        },
        |at| &data[identities[at].clone()],
        options.ignore_case,
        collator,
        sort_keys,
        text,
        out,
        ends,
    )?;
    Ok(fields)
}

/// Projects a raw Record for a language sort as the record storage built it
/// (`Build::language`): its key segments (`language_keys`), then the data.
#[cfg(test)]
fn project_language<S: Build>(
    bytes: &[u8],
    keys: &[u64],
    options: &options::Options,
    collator: &locale::Collator,
    last_selected: u64,
    sequence: u64,
    scratch: &mut batch::Scratch,
) -> Result<Record<S>, Failure> {
    let order = key_order(keys)?;
    let fields = language_keys(
        bytes,
        keys,
        &order,
        options,
        collator,
        last_selected,
        scratch,
    )?;
    let data = if options.vnlog {
        annotated::data(bytes)
    } else {
        bytes
    };
    let language = &scratch.language;
    Ok(Record::new(
        S::language(&language.bytes, &language.ends, data)?,
        fields,
        sequence,
    ))
}

/// Which bytes a sorted record keeps whole.
#[derive(Clone, Copy)]
enum Kept {
    /// The data: the record without a vnlog annotation.
    Data,
    /// The raw record, annotation and all (rmdup hands it on).
    Raw,
}

/// Projects a raw Record for a language sort into a chunk body (`spill::encode`):
/// its key segments (`language_keys`), then the kept bytes. Returns its key
/// cache.
#[allow(clippy::too_many_arguments)]
fn language_body(
    bytes: &[u8],
    keys: &[u64],
    order: &[usize],
    options: &options::Options,
    collator: &locale::Collator,
    last_selected: u64,
    kept: Kept,
    scratch: &mut batch::Scratch,
    out: &mut Vec<u8>,
) -> Result<KeyCache, Failure> {
    let fields = language_keys(
        bytes,
        keys,
        order,
        options,
        collator,
        last_selected,
        scratch,
    )?;
    let kept = match kept {
        Kept::Raw => bytes,
        Kept::Data if options.vnlog => annotated::data(bytes),
        Kept::Data => bytes,
    };
    let storage::LanguageScratch {
        bytes: segments,
        ends,
        ..
    } = &scratch.language;
    let payload = segments
        .len()
        .checked_add(kept.len())
        .ok_or_else(allocation)?;
    spill::encode(
        out,
        fields,
        (ends.len() + 1, payload),
        (ends.len(), 0),
        |at| match at.checked_sub(1) {
            _ if at == ends.len() => kept,
            None => &segments[..ends[0]],
            Some(previous) => &segments[ends[previous]..ends[at]],
        },
    )
}

/// Projects a raw Record for a byte-order sort of original records into a
/// chunk body: its key segments (ASCII-uppercased with `-i`) and the kept
/// bytes. Returns its key cache.
#[allow(clippy::too_many_arguments)]
fn bytes_body(
    bytes: &[u8],
    keys: &[u64],
    order: &[usize],
    options: &options::Options,
    last_selected: u64,
    kept: Kept,
    scratch: &mut batch::Scratch,
    out: &mut Vec<u8>,
) -> Result<KeyCache, Failure> {
    let spans = &mut scratch.spans;
    let fields = project_spans::<Original>(
        bytes,
        &[],
        keys,
        order,
        options.input,
        options.vnlog,
        options.record_end == 0,
        last_selected,
        spans,
        None,
    )?;
    let kept = match kept {
        Kept::Raw => bytes,
        Kept::Data if options.vnlog => annotated::data(bytes),
        Kept::Data => bytes,
    };
    let segment = |at: usize| -> &[u8] {
        if at == keys.len() {
            kept
        } else {
            &bytes[spans[at].clone()]
        }
    };
    let mut payload = 0usize;
    for at in 0..=keys.len() {
        payload = payload
            .checked_add(segment(at).len())
            .ok_or_else(allocation)?;
    }
    let fold = if options.ignore_case { keys.len() } else { 0 };
    spill::encode(
        out,
        fields,
        (keys.len() + 1, payload),
        (keys.len(), fold),
        segment,
    )
}

/// Sorts the Records `intake` accepts and hands them on raw, each in turn in
/// the same buffer: rmdup reads a second Input header from them, as GNU does
/// from its sort pipe.
pub(super) fn sorted_rows(
    reader: &mut impl BufRead,
    mut intake: Intake<'_>,
    options: &options::Options,
    keys: &[u64],
    mut consume: impl FnMut(&mut Vec<u8>) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let collator = options.locale.collator()?;
    let target = memory::target(options.sort_memory.as_deref())?;
    let mut bytes = Vec::new();
    let last = last_selected::<Original>(&[], keys);
    let order = key_order(keys)?;
    let order = &order[..];
    let (shape, threads) = match collator {
        Some(_) => (
            spill::Shape::language(keys.len())?,
            fastmash_sort_process::sort_threads() as usize,
        ),
        None => (spill::Shape::original(keys.len())?, 1),
    };
    let mut sorter = sorter::<Original>(target, threads);
    let project =
        |raw: &[u8], scratch: &mut batch::Scratch, out: &mut Vec<u8>| match collator.as_ref() {
            Some(collator) => language_body(
                raw,
                keys,
                order,
                options,
                collator,
                last,
                Kept::Raw,
                scratch,
                out,
            ),
            None => bytes_body(raw, keys, order, options, last, Kept::Raw, scratch, out),
        };
    let mut batch = batch::Batch::new(threads, target.bytes);
    let mut accept = |record: batch::Projected<'_>| sorter.push_body(record, shape);
    while let Some(record) = intake.next(reader, &mut bytes)? {
        batch.push(record.raw(), &project, &mut accept)?;
    }
    batch.finish(&project, &mut accept)?;
    // Like the system sort, a sort whose input failed yields no records.
    if intake.read_error().is_some() {
        return intake.finish();
    }
    drop(batch);
    let mut sorted = sorter.finish();
    sorted.emit(|record| {
        let original = record.data.original().unwrap_or_default();
        bytes.clear();
        bytes
            .try_reserve(original.len())
            .map_err(|_| allocation())?;
        bytes.extend_from_slice(original);
        consume(&mut bytes)
    })
}

fn equal_group<S: Storage>(
    a: &mut Sorted<'_, S>,
    b: &mut Sorted<'_, S>,
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
        let (a, b) = if stored && key != 0 && key <= a.record.fields && key <= b.record.fields {
            (a.record.segment(at), b.record.segment(at))
        } else {
            // Preserve request-order diagnostics, including the earlier-key
            // mismatch that makes later missing keys irrelevant here.
            (
                a.record
                    .indexed_field(a.layout, &mut a.fields, key, line, options.input)?,
                b.record
                    .indexed_field(b.layout, &mut b.fields, key, line, options.input)?,
            )
        };
        if !text_order::same_group(a, b, options.ignore_case) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether numeric Operations can read packed fields, which hold each field
/// alone. GNU reads a number from the field's start on through the rest of
/// the record (`decimal::number`): an ASCII letter or digit, or the radix
/// point, as the separator continues many numbers into the next field. Any
/// other separator continues only a rare few (`runs_on`), whose numbers the
/// intake reads from the whole record (`Crossings`).
fn packs_numbers(options: &options::Options) -> bool {
    match options.input {
        records::Separator::Whitespace => true,
        records::Separator::Literal(byte) => {
            !byte.is_ascii_alphanumeric() && byte != options.presentation.profile.radix()
        }
    }
}

/// Whether GNU's parse of numeric field `part` might run on past its end into
/// `separator`, one that [`packs_numbers`] admits: a blank or sign separator
/// after a field of blanks (skipped as leading space), a sign after an
/// exponent letter, `(` after `nan`, or `)` or `_` inside `nan(...)`.
fn runs_on(part: &[u8], separator: u8) -> bool {
    let blanks = || {
        !part.is_empty()
            && part
                .iter()
                .all(|&b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
    };
    match separator {
        b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r' => blanks(),
        b'+' | b'-' => blanks() || matches!(part.last(), Some(b'e' | b'E' | b'p' | b'P')),
        b'(' => matches!(part.last(), Some(b'n' | b'N')),
        b')' | b'_' => part.contains(&b'('),
        _ => false,
    }
}

/// The numbers GNU reads differently from packed fields than from the field
/// alone, by record sequence and field: their outcome as the whole record
/// gives it. Only [`runs_on`] fields can be among them.
type Crossings = std::collections::HashMap<
    (u64, u64),
    Result<Option<numerics::Value>, decimal::NumberError>,
    random::TableState,
>;

/// Records in `crossings` how GNU reads the numeric fields of one record
/// (`data`, sequence `sequence`) whose packed `spans` (at `positions`, by
/// field) it could read differently alone.
fn record_crossings(
    data: &[u8],
    spans: &[std::ops::Range<usize>],
    positions: &[(u64, usize)],
    separator: u8,
    sequence: u64,
    options: &options::Options,
    crossings: &mut Crossings,
) -> Result<(), Failure> {
    for &(field, at) in positions {
        let span = spans[at].clone();
        let part = &data[span.clone()];
        if !runs_on(part, separator) {
            continue;
        }
        let (narm, profile) = (options.narm, options.presentation.profile);
        let whole = decimal::number(
            data,
            field_policy::FieldRange {
                start: span.start,
                length: span.len(),
            },
            narm,
            profile,
        );
        let alone = decimal::number(
            part,
            field_policy::FieldRange {
                start: 0,
                length: part.len(),
            },
            narm,
            profile,
        );
        let same = match (&whole, &alone) {
            (Ok(_), Ok(_)) => true,
            (Err(a), Err(b)) => a.same(b),
            _ => false,
        };
        if !same {
            crossings.try_reserve(1).map_err(|_| allocation())?;
            crossings.insert((sequence, field), whole);
        }
    }
    Ok(())
}

/// A packed record's fields as the Operations read them: numbers from the
/// field alone, except those in `crossings`.
struct Projected<'a, S: Storage> {
    record: &'a Record<S>,
    layout: &'a [u64],
    crossings: &'a Crossings,
    separator: Option<u8>,
}

impl<'a, S: Storage> operation_set::Fields<'a> for Projected<'a, S> {
    #[inline]
    fn text(
        &mut self,
        field: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<&'a [u8], Failure> {
        self.record.field(self.layout, field, line, options.input)
    }
    #[inline]
    fn number(
        &mut self,
        field: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<Option<numerics::Value>, Failure> {
        let part = self.text(field, line, options)?;
        if !self.crossings.is_empty()
            && let Some(separator) = self.separator
            && runs_on(part, separator)
            && let Some(outcome) = self.crossings.get(&(self.record.sequence, field))
        {
            return outcome
                .clone()
                .map_err(|error| error.report(part, field, line));
        }
        decimal::field(
            part,
            field_policy::FieldRange {
                start: 0,
                length: part.len(),
            },
            field,
            line,
            options.narm,
            options.presentation.profile,
        )
    }
}

/// A sorted record with the layout its projected fields follow, the index
/// of an original record's fields, and the numbers GNU would read on past a
/// packed field (`Crossings`).
struct Sorted<'l, S: Storage> {
    record: Record<S>,
    layout: &'l [u64],
    fields: records::FieldIndex,
    crossings: &'l Crossings,
    /// Whether every Operation reads one field's text (`NativeSort::Packed`).
    texts: bool,
}

impl<S: Storage> grouping::GroupRecord for Sorted<'_, S> {
    fn field(&mut self, key: u64, line: u64, options: &options::Options) -> Result<&[u8], Failure> {
        self.record
            .indexed_field(self.layout, &mut self.fields, key, line, options.input)
    }
    fn field_pair(
        &mut self,
        first: u64,
        second: u64,
        line: u64,
        options: &options::Options,
    ) -> Result<(&[u8], &[u8]), Failure> {
        let a =
            self.record
                .indexed_field(self.layout, &mut self.fields, first, line, options.input)?;
        let b = self.record.indexed_field(
            self.layout,
            &mut self.fields,
            second,
            line,
            options.input,
        )?;
        Ok((a, b))
    }
    fn same_group(
        &mut self,
        previous: &mut Self,
        keys: &[u64],
        line: u64,
        options: &options::Options,
    ) -> Result<bool, Failure> {
        equal_group(self, previous, keys, line, options)
    }
    fn original(&self) -> Option<&[u8]> {
        self.record.original()
    }
    fn collect(
        &mut self,
        operations: &mut OperationSet,
        line: u64,
        options: &options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut random::RandomState>,
    ) -> Result<bool, Failure> {
        if let Some(bytes) = self.record.original() {
            return operations.collect(bytes, &mut self.fields, line, options, arithmetic, random);
        }
        let separator = match options.input {
            records::Separator::Literal(byte) => Some(byte),
            records::Separator::Whitespace => None,
        };
        operations.collect_fields(
            &mut Projected {
                record: &self.record,
                layout: self.layout,
                crossings: self.crossings,
                separator,
            },
            self.texts,
            line,
            options,
            arithmetic,
        )
    }
}

/// The generated Output header of the first sorted record, which checks each
/// selected field of the original record as it names it.
fn generated_header<W: Write, S: Storage>(
    record: &Record<S>,
    layout: &[u64],
    keys: &[u64],
    operations: &OperationSet,
    options: &options::Options,
    output: &mut command_output::Results<'_, W>,
) -> Result<(), Failure> {
    // Original records (every --full sort among them) render as usual.
    if let Some(bytes) = record.original() {
        return output.first(bytes, operations, keys);
    }
    output.first_projected(
        |field| record.field(layout, field, 1, options.input),
        operations,
        keys,
    )
}

/// Hash grouping of sorted Groups (`-s -g`) with `setting`, for a Command it
/// is eligible for ([`hash_eligible`]): the status of the Command when it
/// wrote the Groups, or `None` when it gave up, having set standard input
/// back to where it started and written nothing, so that the sort runs as if
/// it had not.
pub(super) fn group_by_hash(
    reader: &mut impl replay::Rewind,
    transport: &mut impl command_output::Transport,
    options: &options::Options,
    binding: &mut binding::Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    setting: Grouping,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<Option<i32>, Failure> {
    // Settings the sort route reports on its own are left to it.
    let (Ok(target), Ok(collator)) = (
        memory::target(options.sort_memory.as_deref()),
        options.locale.collator(),
    ) else {
        return Ok(None);
    };
    let Some(mark) = reader.mark() else {
        return Ok(None);
    };
    let remaining = reader.length().map(|length| length.saturating_sub(mark));
    let (capacity, line_buffered) = transport.buffering();
    let buffer = buffered_stdout::BufferedStdout::new(transport, capacity, line_buffered)
        .map_err(|e| os_failure(&e, false))?;
    let mut output = command_output::Results::new(buffer, options);
    let result = hash::calculate(
        reader,
        options,
        binding,
        collator.as_ref(),
        arithmetic,
        &mut output,
        target,
        setting,
        remaining,
    );
    if let Ok(hash::Outcome::Restart(_)) = result {
        drop(output);
        // The sort follows; give back what the classes took.
        malloc_arenas::trim();
        reader
            .rewind(mark)
            .map_err(|error| intake::read_failure(&error))?;
        return Ok(None);
    }
    Ok(Some(command_output::complete(
        output.buffer,
        result.map(drop),
        command_output::Transport::close,
        report,
    )))
}

/// Sorts in process, keeping only the selected fields of each record where
/// `packed` ([`packed`]), and runs the Command over the sorted records. The
/// sort's memory leaves room for what the reader still holds (`unread`).
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    reader: &mut impl BufRead,
    transport: &mut impl command_output::Transport,
    options: &options::Options,
    binding: binding::Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    packed: bool,
    unread: replay::Unread,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let (capacity, line_buffered) = transport.buffering();
    let buffer = buffered_stdout::BufferedStdout::new(transport, capacity, line_buffered)
        .map_err(|e| os_failure(&e, false))?;
    let mut output = command_output::Results::new(buffer, options);
    let result = if packed {
        calculate_records::<_, Packed>(
            reader,
            options,
            binding,
            arithmetic,
            random,
            unread,
            &mut output,
        )
    } else {
        calculate_records::<_, Original>(
            reader,
            options,
            binding,
            arithmetic,
            random,
            unread,
            &mut output,
        )
    };
    Ok(command_output::complete(
        output.buffer,
        result,
        command_output::Transport::close,
        report,
    ))
}

/// The sorter of a sort of memory target `target` on `threads` projection
/// threads, whose chunk grows as its memory target allows.
fn sorter<S: spill::Codec>(target: memory::Target, threads: usize) -> spill::Sorter<S> {
    spill::Sorter::with_target(target, batch::share(threads, target.bytes))
}

/// Whether a sort keeps only the selected fields (Packed storage): not with
/// --full, which prints whole records, nor in a language locale, whose sorter
/// keeps original records, and for numbers only where [`packs_numbers`].
pub(crate) fn packed(options: &options::Options, native: operation_set::NativeSort) -> bool {
    !options.full
        && !options.locale.language()
        && match native {
            operation_set::NativeSort::Packed => true,
            operation_set::NativeSort::Original => packs_numbers(options),
            operation_set::NativeSort::Unsupported => false,
        }
}
fn calculate_records<W: Write, S: spill::Codec>(
    reader: &mut impl BufRead,
    options: &options::Options,
    mut binding: binding::Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    mut random: Option<&mut random::RandomState>,
    unread: replay::Unread,
    output: &mut command_output::Results<'_, W>,
) -> Result<(), Failure> {
    let target = memory::target(options.sort_memory.as_deref())?;
    let collator = options.locale.collator()?;
    let mut bytes = Vec::new();
    let mut intake =
        Intake::new(options, intake::Header::of(options)).warn_full(options, binding.program);
    if !options.header_in {
        binding.numbered()?;
    }
    if intake.header(reader, &mut bytes, |header| binding.header(header, options))? {
        output.first(&bytes, &binding.operations, &binding.keys)?;
    } else if options.header_in && binding.unresolved_keys() {
        // GNU sorts even without an Input header, then reads it again from
        // the sort pipe and warns about --full only then (datamash.c
        // process_file). With unresolved named keys, the installed sorter
        // owns the resulting error.
        let errno = intake.read_error().and_then(io::Error::raw_os_error);
        let sorting = sorted_input::Sorting::read(sorted_input::admit(&options.locale)?, errno);
        return sorting.start(&binding.keys, options)?.run(|reader, _| {
            let mut sorted =
                Intake::new(options, intake::Header::First).warn_full(options, binding.program);
            sorted
                .header(reader, &mut bytes, |_| Ok(()))
                .and_then(|_| sorted.next(reader, &mut bytes).map(|_| ()))
                .and_then(|()| output.end(options))
        });
    }
    let binding::Binding {
        mut operations,
        keys,
        ..
    } = binding;
    let mut layout = Vec::new();
    layout
        .try_reserve_exact(
            keys.len()
                .checked_add(operations.len())
                .ok_or_else(allocation)?,
        )
        .map_err(|_| allocation())?;
    layout.extend_from_slice(&keys);
    for (_, selector) in operations.requests() {
        match selector {
            Selector::Single(field) => layout.push(field),
            Selector::Pair { left, right } => {
                layout.try_reserve(2).map_err(|_| allocation())?;
                layout.extend([left, right]);
            }
        }
    }
    layout.sort_unstable();
    layout.dedup();
    let order = key_order(&keys)?;
    // Original records' fields are looked up for keys and Operations.
    let index = records::FieldIndex::selecting(layout.iter().copied());
    // Language sorts project on several threads: collation keys make that
    // worth its batches.
    let threads = if collator.is_some() {
        fastmash_sort_process::sort_threads() as usize
    } else {
        1
    };
    let mut sorter = sorter::<S>(target, threads);
    sorter.leave_room(unread);
    let mut crossings = Crossings::default();
    let texts = operations.native_sort() == operation_set::NativeSort::Packed;
    match collator.as_ref() {
        // Byte-order sorts: records go straight from the intake into the
        // chunk.
        None => {
            let last = last_selected::<S>(&layout, &keys);
            // Packed numeric fields that GNU could read on past: by field,
            // their positions among a record's spans.
            let mut positions = Vec::new();
            let separator = match options.input {
                records::Separator::Literal(byte) if !S::ORIGINAL => {
                    for field in operations.numeric_fields() {
                        let at = layout
                            .binary_search(&field)
                            .expect("numeric field in projection");
                        positions.try_reserve(1).map_err(|_| allocation())?;
                        positions.push((field, keys.len() + at));
                    }
                    (!positions.is_empty()).then_some(byte)
                }
                _ => None,
            };
            let mut spans = Vec::new();
            let mut sequence = 0u64;
            // Skipped Records never reach key projection.
            while let Some(record) = intake.next(reader, &mut bytes)? {
                let raw = record.raw();
                let data = if options.vnlog {
                    annotated::data(raw)
                } else {
                    raw
                };
                let fields = project_spans::<S>(
                    raw,
                    &layout,
                    &keys,
                    &order,
                    options.input,
                    options.vnlog,
                    options.record_end == 0,
                    last,
                    &mut spans,
                    None,
                )?;
                if let Some(separator) = separator {
                    record_crossings(
                        data,
                        &spans,
                        &positions,
                        separator,
                        sequence,
                        options,
                        &mut crossings,
                    )?;
                }
                sorter.push(
                    raw,
                    data,
                    &spans,
                    keys.len(),
                    options.ignore_case,
                    fields,
                    sequence,
                )?;
                sequence = sequence.checked_add(1).ok_or_else(allocation)?;
            }
        }
        // Language sorts: batches of records are projected on `threads`
        // threads.
        Some(collator) => {
            if !S::ORIGINAL {
                return Err(unsupported(
                    "internal language sorter requires original records",
                ));
            }
            let shape = spill::Shape::language(keys.len())?;
            let last = last_selected::<S>(&layout, &keys);
            let project = |raw: &[u8], scratch: &mut batch::Scratch, out: &mut Vec<u8>| {
                language_body(
                    raw,
                    &keys,
                    &order,
                    options,
                    collator,
                    last,
                    Kept::Data,
                    scratch,
                    out,
                )
            };
            let mut batch = batch::Batch::new(threads, target.bytes);
            let mut accept = |record: batch::Projected<'_>| sorter.push_body(record, shape);
            // Skipped Records never reach key projection.
            while let Some(record) = intake.next(reader, &mut bytes)? {
                batch.push(record.raw(), &project, &mut accept)?;
            }
            batch.finish(&project, &mut accept)?;
        }
    }
    drop(bytes);
    let mut sorted = sorter.finish();
    let mut grouping = grouping::Grouping::new();
    // Line numbers follow the sorted order, as GNU counts the Records it reads
    // back from its sort pipe, so the intake's count (input order) would name
    // the wrong Record; only the Input header keeps its place.
    let mut line = u64::from(options.header_in);
    // The field index for the next record: that of a record grouping no
    // longer holds, else a fresh one.
    let mut spare = None;
    // Like the system sort, a sort whose input failed yields no records.
    if intake.read_error().is_none() {
        sorted.emit(|record| {
            line = line.checked_add(1).ok_or_else(allocation)?;
            if line == 1 && options.header_out && !options.header_in {
                generated_header(&record, &layout, &keys, &operations, options, output)?;
            }
            let mut context = grouping::Context::new(
                &mut operations,
                &keys,
                options,
                arithmetic,
                random.as_deref_mut(),
                output,
            );
            let mut fields = spare.take().unwrap_or_else(|| index.fresh());
            fields.clear();
            let sorted = Sorted {
                record,
                layout: &layout,
                fields,
                crossings: &crossings,
                texts,
            };
            if let Some(unused) = grouping.push(sorted, line, &mut context)? {
                spare = Some(unused.fields);
            }
            Ok(())
        })?;
    }
    let mut context =
        grouping::Context::new(&mut operations, &keys, options, arithmetic, None, output);
    grouping.finish(line, &mut context)?;
    output.end(options)?;
    intake.finish()
}

#[cfg(test)]
mod compact_tests {
    use super::*;
    fn calculation_options() -> Box<options::Options> {
        let options::Action::Calculate(options) =
            options::parse(&["count".into(), "1".into()], b"fastmash", false)
                .ok()
                .unwrap()
        else {
            panic!("calculation")
        };
        options
    }
    fn language_record(
        bytes: &[u8],
        keys: &[u64],
        options: &options::Options,
        collator: &locale::Collator,
        scratch: &mut batch::Scratch,
    ) -> Result<Record<Original>, Failure> {
        project_language::<Original>(
            bytes,
            keys,
            options,
            collator,
            last_selected::<Original>(&[], keys),
            3,
            scratch,
        )
    }
    /// A sorted record's sequence, field count and segments.
    fn described<T: Storage>(record: &Record<T>, segments: usize) -> (u64, u64, Vec<Vec<u8>>) {
        (
            record.sequence,
            record.fields,
            (0..segments)
                .map(|at| record.data.segment(at).to_vec())
                .collect(),
        )
    }
    /// Language records projected in batches on any thread count into chunks
    /// at any target emit the order of the record comparator (stable sort)
    /// over independently built language records.
    #[test]
    fn language_chunks_emit_the_record_comparator_order() {
        let words: &[&str] = &[
            "apple",
            "Apple",
            "APPLE",
            "\u{e9}clair",
            "Eclair",
            "e\u{301}clair",
            "stra\u{df}e",
            "Strasse",
            "\u{f6}lfeld",
            "Olfeld",
            "a-b",
            "ab",
            "a b",
            "",
            "k01",
            "k1",
            "angstrom",
            "\u{c5}ngstr\u{f6}m",
            "caf\u{e9}",
            "cafe",
            "\u{1f600}",
        ];
        let mut state = 7u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize % bound
        };
        let long = "x".repeat(70);
        let rows: Vec<Vec<u8>> = (0..1200)
            .map(|at| {
                // Dates share long collation key prefixes, in groups large
                // enough for word rounds.
                let date = format!("2026-09-{:02}", 1 + next(28));
                let key = if at % 97 == 0 {
                    long.as_str()
                } else if at % 3 == 0 {
                    date.as_str()
                } else {
                    words[next(words.len())]
                };
                format!("{key}\t{}\t{}", next(9), words[next(words.len())]).into_bytes()
            })
            .collect();
        let mut options = calculation_options();
        for tag in ["en-US", "de-DE"] {
            options.locale.collation = locale::Collation::Language(tag);
            let collator = options.locale.collator().ok().unwrap().unwrap();
            for fold in [false, true] {
                options.ignore_case = fold;
                for keys in [&[1u64][..], &[3, 1][..]] {
                    let last = last_selected::<Original>(&[], keys);
                    let shape = spill::Shape::language(keys.len()).ok().unwrap();
                    let segments = 2 * keys.len() + 1;
                    let mut scratch = batch::Scratch::default();
                    let mut records: Vec<Record<Original>> = rows
                        .iter()
                        .enumerate()
                        .map(|(sequence, row)| {
                            project_language::<Original>(
                                row,
                                keys,
                                &options,
                                &collator,
                                last,
                                sequence as u64,
                                &mut scratch,
                            )
                            .ok()
                            .unwrap()
                        })
                        .collect();
                    records.sort_by(Record::compare);
                    let expected: Vec<_> =
                        records.iter().map(|row| described(row, segments)).collect();
                    for target in [usize::MAX, 1 << 16, 4096, 1] {
                        for threads in [1, 3] {
                            let mut sorter = spill::Sorter::<Original>::new(
                                target,
                                batch::share(threads, target),
                            );
                            let order = key_order(keys).ok().unwrap();
                            let project =
                                |raw: &[u8], scratch: &mut batch::Scratch, out: &mut Vec<u8>| {
                                    language_body(
                                        raw,
                                        keys,
                                        &order,
                                        &options,
                                        &collator,
                                        last,
                                        Kept::Data,
                                        scratch,
                                        out,
                                    )
                                };
                            let mut accept =
                                |record: batch::Projected<'_>| sorter.push_body(record, shape);
                            let mut batch = batch::Batch::new(threads, target);
                            for row in &rows {
                                batch.push(row, &project, &mut accept).ok().unwrap();
                            }
                            batch.finish(&project, &mut accept).ok().unwrap();
                            let mut actual = Vec::new();
                            sorter
                                .finish()
                                .emit(|record| {
                                    actual.push(described(&record, segments));
                                    Ok(())
                                })
                                .ok()
                                .unwrap();
                            assert!(
                                actual == expected,
                                "{tag} fold {fold} keys {keys:?} target {target} threads {threads}"
                            );
                            // Every chunk of several records spilled within
                            // its part of the target, beside the batches'
                            // share and the run buffers.
                            let part = spill::chunk_memory(target, batch::share(threads, target));
                            assert!(spill::spilled_memory() <= part);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn language_records_do_not_keep_the_byte_key_cache() {
        let mut options = calculation_options();
        options.locale.collation = locale::Collation::Language("en-US");
        let collator = options.locale.collator().ok().unwrap().unwrap();
        let mut rows = Vec::new();
        for bytes in [b"a".as_slice(), b"A", b"ab", b"a-b"] {
            let row = language_record(
                bytes,
                &[1],
                &options,
                &collator,
                &mut batch::Scratch::default(),
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
    /// Language key segments as the former two-step build made them: a byte
    /// projection, then identity fields checked and copied in field order,
    /// then each key segment's text checked and its sort key computed, each
    /// followed by its identity.
    fn former_language_segments(
        bytes: &[u8],
        keys: &[u64],
        options: &options::Options,
        collator: &locale::Collator,
    ) -> Result<(Vec<Vec<u8>>, u64), Failure> {
        let record = project::<Original>(
            bytes,
            &[],
            keys,
            options.input,
            0,
            options.vnlog,
            options.ignore_case,
            last_selected::<Original>(&[], keys),
        )?;
        let data = if options.vnlog {
            annotated::data(bytes)
        } else {
            bytes
        };
        let mut identity = vec![Vec::new(); keys.len()];
        for (index, span) in records::fields(data, options.input).enumerate() {
            for (at, key) in keys.iter().enumerate() {
                if *key == index as u64 + 1 {
                    let field = &data[span.start..span.start + span.length];
                    locale::text(field)?;
                    identity[at] = field.to_vec();
                    if options.ignore_case {
                        identity[at].make_ascii_lowercase();
                    }
                }
            }
        }
        let composed = |text: &str| {
            let mut out = String::new();
            locale::compose(text, &mut out).ok().unwrap().then_some(out)
        };
        let mut segments = Vec::new();
        assert_eq!(identity.len(), record.key_count());
        for (at, field) in identity.iter().enumerate() {
            let text = locale::text(record.segment(at))?;
            let text = composed(text).unwrap_or_else(|| text.to_owned());
            let mut key = Vec::new();
            assert!(collator.write_key(&text, &mut key).is_ok());
            segments.push(key);
            // The identity is read uppercased with -i, composed, placed and
            // lowercased.
            let mut read = std::str::from_utf8(field).unwrap().to_owned();
            if options.ignore_case {
                read.make_ascii_uppercase();
            }
            if let Some(read_composed) = composed(&read) {
                read = read_composed;
            }
            let mut written = Vec::new();
            if !collator.write_identity(&read, &mut written).ok().unwrap() {
                written = read.into_bytes();
            }
            if options.ignore_case {
                written.make_ascii_lowercase();
            }
            segments.push(written);
        }
        segments.push(record.original().unwrap().to_vec());
        Ok((segments, record.fields))
    }
    #[test]
    fn language_records_equal_the_former_two_step_build() {
        let pieces: &[&[u8]] = &[
            b"a",
            b"A",
            b"b",
            b"k01",
            b"\xc3\xa9",
            b"E\xcc\x81",
            b"\xc3\x9f",
            b"-",
            b" ",
            b"\t",
            b",",
            b"#",
            b"\0",
            b"\xff",
            b"\xc3",
            b"\xf0\x9f\x98\x80",
            b"z",
            b"Z",
            "\u{438}\u{306}".as_bytes(),
            "\u{439}".as_bytes(),
            "L\u{b7}".as_bytes(),
            "l\u{b7}".as_bytes(),
            "\u{401}".as_bytes(),
            "\u{415}\u{308}".as_bytes(),
            "\u{ed}".as_bytes(),
            "i\u{301}".as_bytes(),
        ];
        let key_sets: &[&[u64]] = &[&[1], &[2], &[2, 1], &[1, 3, 1], &[4], &[3, 2]];
        let mut state = 143u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize % bound
        };
        let mut options = calculation_options();
        let mut scratch = batch::Scratch::default();
        let (mut accepted, mut refused) = (0, 0);
        for tag in ["en-US", "de-DE"] {
            options.locale.collation = locale::Collation::Language(tag);
            let collator = options.locale.collator().ok().unwrap().unwrap();
            for _ in 0..6000 {
                let mut bytes = Vec::new();
                for _ in 0..next(12) {
                    bytes.extend_from_slice(pieces[next(pieces.len())]);
                }
                options.input = [
                    records::Separator::Literal(b'\t'),
                    records::Separator::Literal(b','),
                    records::Separator::Whitespace,
                ][next(3)];
                options.vnlog = next(4) == 0;
                options.ignore_case = next(2) == 0;
                let keys = key_sets[next(key_sets.len())];
                let expected = former_language_segments(&bytes, keys, &options, &collator);
                let observed = language_record(&bytes, keys, &options, &collator, &mut scratch);
                match (expected, observed) {
                    (Ok((segments, fields)), Ok(record)) => {
                        accepted += 1;
                        let stored = record.data.segments.count();
                        assert_eq!(stored, segments.len(), "{bytes:?}");
                        for (at, segment) in segments.iter().enumerate() {
                            assert_eq!(record.segment(at), segment, "{bytes:?} {at}");
                        }
                        assert_eq!(record.fields, fields);
                        assert_eq!(
                            (record.prefix, record.short_key_len),
                            Record::key_cache(&record.data)
                        );
                    }
                    (Err(expected), Err(observed)) => {
                        refused += 1;
                        assert_eq!(expected.message, observed.message, "{bytes:?} {keys:?}");
                    }
                    (expected, observed) => panic!(
                        "{bytes:?} {keys:?}: {:?} vs {:?}",
                        expected.map(|_| ()).map_err(|e| e.message),
                        observed.map(|_| ()).map_err(|e| e.message)
                    ),
                }
            }
        }
        assert!(accepted > 2000 && refused > 2000, "{accepted} {refused}");
    }
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn short_key_cache_preserves_record_footprint() {
        assert_eq!(std::mem::size_of::<Record<Original>>(), 64);
        assert_eq!(std::mem::size_of::<Record<Packed>>(), 72);
    }
    fn stored_group_equivalence<S: Build>() {
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
                            let expected = old().map_err(|e| e.message);
                            let crossings = Crossings::default();
                            for keep in [false, true] {
                                let sorted = |bytes| Sorted {
                                    record: make(bytes),
                                    layout: &layout,
                                    fields: if keep {
                                        records::FieldIndex::always_keeping(layout.iter().copied())
                                    } else {
                                        records::FieldIndex::default()
                                    },
                                    crossings: &crossings,
                                    texts: false,
                                };
                                let (mut a, mut b) = (sorted(left), sorted(right));
                                assert_eq!(
                                    equal_group(&mut a, &mut b, &keys, 7, &options)
                                        .map_err(|e| e.message),
                                    expected
                                );
                            }
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
    fn tail_counts<S: Build>() {
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
                        last_selected::<S>(&layout, &keys),
                    )
                    .ok()
                    .unwrap();
                    // An unreachable selected index forces the existing full
                    // field walk, independently of delimiter counting.
                    let full = project::<S>(&raw, &layout, &keys, input, 0, false, false, u64::MAX)
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

    fn boundaries<S: Build>() {
        let layout = [1, 2, 3, 4];
        let input = records::Separator::Literal(b'\t');
        let row = project::<S>(
            b"aB\t\t\0\xff",
            &layout,
            &[3, 1, 3, 4],
            input,
            7,
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
    fn annotated<S: Build>() {
        let row = project::<S>(
            b"  aB  cD # note",
            &[1, 2, 3],
            &[2, 3, 1],
            records::Separator::Whitespace,
            0,
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
    fn reusable_projection<S: spill::Codec + Build>() {
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
        // Storage, key and segment counts; sequence delta 0, three fields; the
        // one-byte lengths; the concatenated segments.
        let mut expected = vec![u8::from(S::ORIGINAL), 4, segments.len() as u8, 0, 3];
        expected.extend(segments.iter().map(|value| value.len() as u8));
        for value in &segments {
            expected.extend_from_slice(value);
        }
        assert_eq!(spill::encode_one(&rows[0]), expected);
        let decoded = spill::decode_one::<S>(&expected).ok().unwrap();
        for (at, bytes) in segments.iter().enumerate() {
            assert_eq!(decoded.segment(at), *bytes);
        }
    }
    #[test]
    fn projection_scratch_does_not_leak_fields_or_change_spill_payloads() {
        reusable_projection::<Original>();
        reusable_projection::<Packed>();
    }
    /// Key and identity spans as the former projection found them: every
    /// key compared with every field.
    fn former_key_spans(
        bytes: &[u8],
        keys: &[u64],
        input: records::Separator,
        vnlog: bool,
    ) -> (Vec<std::ops::Range<usize>>, Vec<std::ops::Range<usize>>) {
        let data = if vnlog { annotated::data(bytes) } else { bytes };
        let (mut spans, mut identities) = (vec![0..0; keys.len()], vec![0..0; keys.len()]);
        let mut previous_end = 0;
        for (index, span) in records::fields(data, input).enumerate() {
            let start = if input == records::Separator::Whitespace {
                previous_end
            } else {
                span.start
            };
            for (at, &key) in keys.iter().enumerate() {
                if key == index as u64 + 1 {
                    if !vnlog {
                        spans[at] = start..span.start + span.length;
                    }
                    identities[at] = span.start..span.start + span.length;
                }
            }
            previous_end = span.start + span.length;
        }
        if vnlog {
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
        (spans, identities)
    }
    #[test]
    fn keys_in_any_order_project_as_every_key_against_every_field() {
        let mut state = 147u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize % bound
        };
        for input in [
            records::Separator::Literal(b'\t'),
            records::Separator::Whitespace,
        ] {
            for vnlog in [false, true] {
                for _ in 0..3000 {
                    let mut bytes = Vec::new();
                    for at in 0..next(14) {
                        let blanks = [" ", "  ", "\t", " \t"][next(4)];
                        match input {
                            records::Separator::Literal(_) if at > 0 => bytes.push(b'\t'),
                            records::Separator::Whitespace if at > 0 || next(2) == 0 => {
                                bytes.extend_from_slice(blanks.as_bytes())
                            }
                            _ => {}
                        }
                        bytes.extend_from_slice(format!("f{}", next(100)).as_bytes());
                    }
                    if vnlog && next(2) == 0 {
                        bytes.extend_from_slice(b" # note");
                    }
                    let keys: Vec<u64> = (0..next(8)).map(|_| next(16) as u64).collect();
                    let order = key_order(&keys).ok().unwrap();
                    let (mut spans, mut identities) = (Vec::new(), vec![0..0; keys.len()]);
                    let last = last_selected::<Original>(&[], &keys);
                    let fields = project_spans::<Original>(
                        &bytes,
                        &[],
                        &keys,
                        &order,
                        input,
                        vnlog,
                        false,
                        last,
                        &mut spans,
                        Some(&mut identities),
                    )
                    .ok()
                    .unwrap();
                    let data = if vnlog {
                        annotated::data(&bytes)
                    } else {
                        &bytes
                    };
                    assert_eq!(fields, records::fields(data, input).count() as u64);
                    assert_eq!(
                        (spans, identities),
                        former_key_spans(&bytes, &keys, input, vnlog),
                        "{bytes:?} {keys:?} {vnlog}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod packed_number_tests {
    use super::*;
    use operation_set::{Fields, NativeSort, Whole};

    fn options(separator: u8, narm: bool) -> Box<options::Options> {
        let options::Action::Calculate(mut options) =
            options::parse(&["sum".into(), "2".into()], b"fastmash", false)
                .ok()
                .unwrap()
        else {
            panic!("calculation")
        };
        options.input = records::Separator::Literal(separator);
        options.narm = narm;
        options
    }

    fn outcome(result: Result<Option<numerics::Value>, Failure>) -> String {
        match result {
            Ok(value) => format!("ok {value:?}"),
            Err(error) => format!(
                "err {} {}",
                error.status,
                String::from_utf8_lossy(&error.message)
            ),
        }
    }

    #[test]
    fn numbers_pack_unless_the_separator_can_continue_them() {
        for (separator, packs) in [
            (b'\t', true),
            (b' ', true),
            (b',', true),
            (b'+', true),
            (b'(', true),
            (b'e', false),
            (b'X', false),
            (b'1', false),
            (b'.', false),
        ] {
            let mut options = options(separator, false);
            assert_eq!(packed(&options, NativeSort::Original), packs, "{separator}");
            assert!(packed(&options, NativeSort::Packed));
            assert!(!packed(&options, NativeSort::Unsupported));
            options.full = true;
            assert!(!packed(&options, NativeSort::Original));
            assert!(!packed(&options, NativeSort::Packed));
        }
    }

    /// A number read from its packed field, with the intake's crossings, is
    /// the one GNU reads on from the field through the whole record, for every
    /// separator that packs numbers and fields its parse might run on past.
    #[test]
    fn packed_numbers_read_as_whole_records_do() {
        let blanks = " ".repeat(600);
        let blanks_exponent = format!("{}1e", " ".repeat(510));
        let long_nan = format!("nan({}", "a".repeat(600));
        let parts: [&[u8]; 33] = [
            b"",
            b" ",
            b"\t",
            b"\r",
            b"\x0b\x0c",
            blanks.as_bytes(),
            blanks_exponent.as_bytes(),
            b"1",
            b"-2.5",
            b"1e",
            b"1E",
            b"0x1p",
            b"0x1P",
            b"12e",
            b"nan",
            b"NaN",
            b"-nan",
            b"nan(",
            b"nan(ab",
            long_nan.as_bytes(),
            b"inf",
            b"NA",
            b"N/A",
            b"+",
            b"-",
            b"  7",
            b"7  ",
            b"1.5",
            b"1,5",
            b"99999999999999999999999",
            b"1e99999",
            b"5x",
            b"0x",
        ];
        let rests: [&[u8]; 13] = [
            b"", b"5", b"+5", b"-5", b"1e99999", b"(abc)", b"abc)", b"_d)", b" 7", b"\t8", b"e5",
            b")", b"  \t  6",
        ];
        let keys = [1u64];
        let layout = [1u64, 2];
        let order = key_order(&keys).ok().unwrap();
        let last = last_selected::<Packed>(&layout, &keys);
        let mut checked = 0usize;
        for separator in 1u8..=u8::MAX {
            if separator == b'\n' || !packs_numbers(&options(separator, false)) {
                continue;
            }
            for narm in [false, true] {
                let options = options(separator, narm);
                for part in parts {
                    if part.contains(&separator) {
                        continue;
                    }
                    for rest in rests {
                        let mut record = b"k".to_vec();
                        record.push(separator);
                        record.extend_from_slice(part);
                        record.push(separator);
                        record.extend_from_slice(rest);
                        let whole = Whole {
                            record: &record,
                            index: &mut records::FieldIndex::selecting([1, 2].into_iter()),
                        }
                        .number(2, 1, &options);
                        let mut spans = Vec::new();
                        let fields = project_spans::<Packed>(
                            &record,
                            &layout,
                            &keys,
                            &order,
                            options.input,
                            false,
                            false,
                            last,
                            &mut spans,
                            None,
                        )
                        .ok()
                        .unwrap();
                        let mut crossings = Crossings::default();
                        record_crossings(
                            &record,
                            &spans,
                            &[(2, keys.len() + 1)],
                            separator,
                            7,
                            &options,
                            &mut crossings,
                        )
                        .ok()
                        .unwrap();
                        let packed = project::<Packed>(
                            &record,
                            &layout,
                            &keys,
                            options.input,
                            7,
                            false,
                            false,
                            last,
                        )
                        .ok()
                        .unwrap();
                        assert_eq!(packed.fields, fields);
                        let got = Projected {
                            record: &packed,
                            layout: &layout,
                            crossings: &crossings,
                            separator: Some(separator),
                        }
                        .number(2, 1, &options);
                        assert_eq!(
                            outcome(got),
                            outcome(whole),
                            "separator {separator:#04x}, narm {narm}, field {:?}, rest {:?}",
                            String::from_utf8_lossy(part),
                            String::from_utf8_lossy(rest)
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 100_000, "{checked}");
    }
}
