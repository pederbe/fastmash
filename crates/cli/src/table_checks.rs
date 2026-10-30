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
    records, standard_io, unsupported,
};
use std::io::{BufRead, Write};

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
    let key = keys
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
    super::sort_route::trace("native sort of whole records");
    native_dedup(reader, output, options, key)
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
    state: TableState,
    seen: KeyTable,
    /// Whether every Record still to come has a key of its own.
    distinct: bool,
}
impl Dedup {
    fn new(key: Field) -> Self {
        Self {
            key,
            state: TableState::default(),
            seen: KeyTable::default(),
            distinct: false,
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
        if self.distinct {
            row(output, data, options);
            return Ok(());
        }
        let key = bytes.split(|b| *b == 0).next().unwrap();
        if self.seen.needs_growth() {
            self.seen.grow()?;
        }
        let hash = std::hash::BuildHasher::hash_one(&self.state, key);
        // Under half full, an unlimited probe always ends.
        let Probe::Empty(at) = self.seen.probe(hash, key, usize::MAX) else {
            return Ok(());
        };
        self.seen.insert(at, hash, key)?;
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
        // An input failure takes precedence over an unresolved named key.
        intake.finish()?;
        return Err(failure(
            b"missing input header for named grouping key\n".to_vec(),
        ));
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
    let mut repeats = Repeats::new(options)?;
    let distinct = std::cell::Cell::new(repeats.distinct);
    let mut admit = |key: &[u8]| {
        let admitted = repeats.admit(key);
        distinct.set(repeats.distinct);
        admitted
    };
    super::projected_sort::sorted_rows(reader, unsorted, options, &[number], &mut admit, |record| {
        // Every Record has been admitted before the first is consumed.
        state.distinct = distinct.get();
        match sorted.accept(record)? {
            Accepted::Skipped => Ok(()),
            Accepted::Header => state.header(record, output, options),
            Accepted::Data(data) => state.row(&record[..data], sorted.line(), output, options),
        }
    })
}

/// Byte keys in one arena, found through an open-addressing table of their
/// hashes, each with a count of the Records that kept it.
#[derive(Default)]
struct KeyTable {
    slots: Vec<Slot>,
    entries: Vec<Entry>,
    keys: Vec<u8>,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    /// Zero marks an empty slot; stored hashes have their low bit set.
    hash: u64,
    entry: usize,
}

struct Entry {
    start: usize,
    len: usize,
    count: u8,
}

enum Probe {
    /// The key's entry.
    Found(usize),
    /// The key is absent, and may be stored in this slot.
    Empty(usize),
    /// The key is absent or lies beyond the probes allowed.
    Exhausted,
}

impl KeyTable {
    const FIRST: usize = 1024;

    fn probe(&self, hash: u64, key: &[u8], limit: usize) -> Probe {
        if self.slots.is_empty() {
            return Probe::Exhausted;
        }
        let hash = hash | 1;
        let mask = self.slots.len() - 1;
        let mut at = hash as usize & mask;
        for _ in 0..limit.min(self.slots.len()) {
            let slot = &self.slots[at];
            if slot.hash == 0 {
                return Probe::Empty(at);
            }
            if slot.hash == hash {
                let entry = &self.entries[slot.entry];
                if &self.keys[entry.start..entry.start + entry.len] == key {
                    return Probe::Found(slot.entry);
                }
            }
            at = (at + 1) & mask;
        }
        Probe::Exhausted
    }

    /// Whether one more key would fill over half the slots.
    fn needs_growth(&self) -> bool {
        (self.entries.len() + 1) * 2 > self.slots.len()
    }

    /// The slots after the next growth.
    fn grown(&self) -> usize {
        (self.slots.len() * 2).max(Self::FIRST)
    }

    /// The memory held with `slots` slots and `more` key bytes.
    fn memory(&self, slots: usize, more: usize) -> Option<usize> {
        let entries = (self.entries.len() + 1).checked_mul(std::mem::size_of::<Entry>())?;
        slots
            .checked_mul(std::mem::size_of::<Slot>())?
            .checked_add(entries)?
            .checked_add(self.keys.len())?
            .checked_add(more)
    }

    fn grow(&mut self) -> Result<(), Failure> {
        let size = self.grown();
        let mut slots = Vec::new();
        reserve(&mut slots, size)?;
        slots.resize(size, Slot::default());
        let mask = size - 1;
        for slot in self.slots.iter().filter(|slot| slot.hash != 0) {
            let mut at = slot.hash as usize & mask;
            while slots[at].hash != 0 {
                at = (at + 1) & mask;
            }
            slots[at] = *slot;
        }
        self.slots = slots;
        Ok(())
    }

    fn insert(&mut self, at: usize, hash: u64, key: &[u8]) -> Result<(), Failure> {
        let start = self.keys.len();
        reserve(&mut self.keys, key.len())?;
        reserve(&mut self.entries, 1)?;
        self.keys.extend_from_slice(key);
        self.slots[at] = Slot {
            hash: hash | 1,
            entry: self.entries.len(),
        };
        self.entries.push(Entry {
            start,
            len: key.len(),
            count: 1,
        });
        Ok(())
    }
}

/// Sorted rmdup prints a key's first Record in sort order. The sort is stable,
/// so a later Record with the same sort key bytes always follows an earlier
/// one with the same duplicate key, and cannot be printed: it need not be
/// sorted. When a second Input header is read from the sorted Records, its key
/// is not marked seen, so one further occurrence of every key is kept. Empty
/// keys, which a missing field also has, are always kept: dropped Records then
/// sort after any Record whose field is missing, leaving its line number.
/// Equal sort keys hold equal fields except where `-z` blank-separated sort
/// keys end at a newline; no Records are dropped there.
///
/// Keys are tracked up to a share of the sort memory. A key that hashes into
/// a crowded part of the table stays untracked, so no input slows lookups.
/// Once no more keys fit, lookups stop if they drop too few Records to pay
/// for themselves.
struct Repeats {
    /// Whether every admitted Record has a duplicate key of its own: its sort
    /// key was new, holds the whole field and was tracked.
    distinct: bool,
    seed: u64,
    table: KeyTable,
    kept: u8,
    budget: usize,
    full: bool,
    looked: u64,
    dropped: u64,
}

#[derive(PartialEq, Eq)]
enum Admitted {
    /// The first Record with a tracked key that is its whole field.
    First,
    Other,
    Dropped,
}

/// The probes a lookup makes before leaving its key untracked.
const REPEAT_PROBES: usize = 16;

impl Repeats {
    fn new(options: &Options) -> Result<Self, Failure> {
        let target = super::projected_sort::memory_target(options.sort_memory.as_deref())?;
        // With -z, blank-separated sort keys also end at a newline, which
        // fields do not: equal sort keys can then hold different fields.
        let splits = options.record_end == 0 && options.input == records::Separator::Whitespace;
        let kept = if options.header_in { 2 } else { 1 };
        let budget = if splits { 0 } else { target / 4 };
        Ok(Self {
            distinct: kept == 1
                && budget > 0
                && !options.vnlog
                && matches!(options.input, records::Separator::Literal(_)),
            seed: std::hash::BuildHasher::hash_one(&TableState::default(), 0u64),
            table: KeyTable::default(),
            kept,
            budget,
            full: false,
            looked: 0,
            dropped: 0,
        })
    }

    fn admit(&mut self, key: &[u8]) -> Result<bool, Failure> {
        let admitted = self.track(key)?;
        if admitted == Admitted::Other {
            self.distinct = false;
        }
        Ok(admitted != Admitted::Dropped)
    }

    fn track(&mut self, key: &[u8]) -> Result<Admitted, Failure> {
        if key.is_empty() || self.budget == 0 {
            return Ok(Admitted::Other);
        }
        self.looked += 1;
        if self.full && self.looked.is_multiple_of(65536) && self.dropped * 16 < self.looked {
            // Lookups now cost more than the sorting they save.
            self.budget = 0;
            self.table = KeyTable::default();
            return Ok(Admitted::Other);
        }
        if self.table.slots.is_empty() && !self.grow()? {
            return Ok(Admitted::Other);
        }
        let hash = quick_hash(self.seed, key);
        let at = match self.table.probe(hash, key, REPEAT_PROBES) {
            Probe::Found(entry) => {
                let entry = &mut self.table.entries[entry];
                if entry.count >= self.kept {
                    self.dropped += 1;
                    return Ok(Admitted::Dropped);
                }
                entry.count += 1;
                return Ok(Admitted::Other);
            }
            Probe::Exhausted => return Ok(Admitted::Other),
            Probe::Empty(at) => at,
        };
        if self.full {
            return Ok(Admitted::Other);
        }
        let at = if self.table.needs_growth() {
            if !self.grow()? {
                return Ok(Admitted::Other);
            }
            match self.table.probe(hash, key, REPEAT_PROBES) {
                Probe::Empty(at) => at,
                _ => return Ok(Admitted::Other),
            }
        } else {
            at
        };
        if self
            .table
            .memory(self.table.slots.len(), key.len())
            .is_none_or(|memory| memory > self.budget)
        {
            self.full = true;
            return Ok(Admitted::Other);
        }
        self.table.insert(at, hash, key)?;
        // A NUL ends the duplicate key early, so it may equal another's.
        Ok(if memchr::memchr(0, key).is_some() {
            Admitted::Other
        } else {
            Admitted::First
        })
    }

    /// Doubles the table, or marks it full when that would exceed the budget.
    fn grow(&mut self) -> Result<bool, Failure> {
        if self.full
            || self
                .table
                .memory(self.table.grown(), 0)
                .is_none_or(|memory| memory > self.budget)
        {
            self.full = true;
            return Ok(false);
        }
        self.table.grow()?;
        Ok(true)
    }
}

/// A seeded multiplicative hash, cheap for the short keys typical of
/// duplicate removal. Its quality matters only for speed: the probe limit
/// bounds the cost of keys that collide.
fn quick_hash(seed: u64, key: &[u8]) -> u64 {
    const MULTIPLIER: u64 = 0x9e37_79b9_7f4a_7c15;
    let mix = |hash: u64, word: u64| {
        let product = u128::from(hash ^ word) * u128::from(MULTIPLIER);
        (product as u64) ^ ((product >> 64) as u64)
    };
    let mut hash = mix(seed, key.len() as u64);
    let (words, rest) = key.as_chunks::<8>();
    for word in words {
        hash = mix(hash, u64::from_le_bytes(*word));
    }
    if !rest.is_empty() {
        let mut word = [0; 8];
        word[..rest.len()].copy_from_slice(rest);
        hash = mix(hash, u64::from_le_bytes(word));
    }
    mix(hash, seed)
}
