//! Hash grouping: sorted Groups (`-s -g`) without sorting the records. Each
//! record joins its class, the records with its grouping keys, whose own
//! Operations collect it in input order, as the stable sort hands it on; at
//! the end only the classes are sorted, in the sort's order, and written as
//! Groups.
//!
//! GNU sorts with `sort -s` (datamash.c `open_input`), so its Groups are
//! exactly the classes, each in input order, where every key is present and
//! free of NUL (GNU compares keys with `strncmp`) and records carry no vnlog
//! annotations. With `-W`, GNU's sort keys keep the blanks before each field,
//! which Groups ignore, so a class is its keys with those blanks, and keys
//! that differ only in them make hash grouping give up. In a language
//! locale, glibc ties some keys spelled differently, which the stable sort
//! then keeps in input order, so keys that differ only so make it give up
//! too. Anything else (a missing or NUL key, a key a language sort refuses, a
//! failing Operation, a read error, or more classes than hash grouping pays
//! for) makes hash grouping give up before it writes anything. It runs before the choice between the
//! in-process and the system sort, reading the Input header itself, so that
//! the caller can set the input back to where it started and sort it either
//! way as if hash grouping had never run.
use super::{memory, storage};
use crate::{
    Failure, OperationSet,
    binding::Binding,
    command_output::CommandOutput,
    grouping, indexed_field,
    intake::{self, Intake},
    locale, numerics, options, random, records, replay, unsupported,
};
use std::{cmp::Ordering, io::BufRead};

/// When hash grouping is used: the `FASTMASH_GROUPING` setting, which
/// tests and measurements use to compare the two ways of grouping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Setting {
    /// Always sort (`sort`).
    Sort,
    /// Hash grouping where it is exact (the default).
    Hash,
    /// Hash grouping that gives up after this many records, to test the
    /// sort that follows (`hash:restart=N`).
    RestartAfter(u64),
}

/// The setting `FASTMASH_GROUPING` gives as `value`.
pub(crate) fn setting(value: Option<&std::ffi::OsStr>) -> Result<Setting, Failure> {
    let Some(value) = value else {
        return Ok(Setting::Hash);
    };
    match value.to_str() {
        Some("sort") => Some(Setting::Sort),
        Some("hash") => Some(Setting::Hash),
        Some(value) => value
            .strip_prefix("hash:restart=")
            .and_then(|records| records.parse().ok())
            .map(Setting::RestartAfter),
        None => None,
    }
    .ok_or_else(|| unsupported("FASTMASH_GROUPING must be sort or hash"))
}

/// Whether hash grouping gives exactly the sorted Groups of this command,
/// once its grouping keys are resolved.
pub(crate) fn eligible(options: &options::Options, operations: &OperationSet) -> bool {
    options.groups_are_sort_key_classes()
        && operations
            .requests()
            .all(|(kind, _)| !kind.crosses_groups())
}

/// What hash grouping did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    /// It wrote the Groups.
    Written,
    /// It wrote nothing, for this reason: read the input again through the
    /// sort.
    Restart(&'static str),
}

#[cfg(test)]
thread_local! {
    /// This thread's latest outcome, for tests.
    pub(super) static LAST: std::cell::Cell<Option<Outcome>> = const { std::cell::Cell::new(None) };
}

const HEADER: Outcome = Outcome::Restart("the Input header does not resolve every name");
const READ: Outcome = Outcome::Restart("reading the input failed");
const MISSING: Outcome = Outcome::Restart("a grouping key is missing");
const NUL: Outcome = Outcome::Restart("a grouping key contains NUL");
const TEXT: Outcome = Outcome::Restart("a grouping key is not text the collation reads");
const FAILED: Outcome = Outcome::Restart("an Operation failed");
const SPREAD: Outcome = Outcome::Restart("too few records per Group");
const UNCACHED: Outcome = Outcome::Restart("the Groups' state outgrows the processor's caches");
const MEMORY: Outcome = Outcome::Restart("the Groups outgrow the sort's memory");
const ALLOCATION: Outcome = Outcome::Restart("memory allocation failed");
const TEST: Outcome = Outcome::Restart("restart requested");
const BLANKS: Outcome = Outcome::Restart("grouping keys differ only in the blanks before them");
const NEWLINE: Outcome =
    Outcome::Restart("a record holds a newline, which the sort takes for a blank");
const SPELLINGS: Outcome =
    Outcome::Restart("grouping keys differ only in spellings the collation ties");

/// Records read before first judging whether classes repeat enough to pay
/// for hash grouping; the judgment is repeated each time the records read
/// double.
const WINDOW: u64 = 1 << 13;

/// The fewest records per class, as estimated, for which hash grouping
/// pays: below it, class upkeep costs more than the sort.
const RECORDS_PER_CLASS: f64 = 8.0;

/// Class state beyond this many bytes misses the processor's caches on most
/// records, unless records of a class come together; the sort is then faster
/// (measured on a laptop processor with an 8 MiB cache: 50,000 classes in
/// random order hash 1.5 times as fast as the sort, 187,500 classes 1.75
/// times as slow).
const CACHED_STATE: usize = 16 << 20;

/// How much more memory the classes take than their accounted bytes: the
/// unused capacity of the vectors that hold them, and allocator overhead.
const SLACK: usize = 2;

/// With `-W`, the sort keys keep the blanks before each field, and a class
/// is its keys with those blanks, as the sort compares them. Two classes
/// whose keys differ only in the blanks may be one Group or two, so hash
/// grouping gives up there; with `-z`, the sort also takes a newline for a
/// blank. The loop's spans never reach this state, so that the loop other
/// jobs run is compiled as before.
struct Blanks {
    /// Whether records end at NUL (`-z`).
    newlines: bool,
    /// The classes' keys without the blanks before them.
    fields: Table,
    scratch: Vec<u8>,
}

impl Blanks {
    fn new(newlines: bool) -> Self {
        Self {
            newlines,
            fields: Table::new(),
            scratch: Vec::new(),
        }
    }

    /// Admits a new class, of `count` sort keys joined in `tuple`: the bytes
    /// it takes, or why hash grouping gives up.
    #[cold]
    #[inline(never)]
    fn admit(&mut self, tuple: &[u8], count: usize) -> Result<usize, Outcome> {
        // The keys without the blanks before them, joined as `tuple` joins.
        self.scratch.clear();
        self.scratch
            .try_reserve(tuple.len())
            .map_err(|_| ALLOCATION)?;
        for key in keys_of(tuple, count) {
            let blank = key.iter().take_while(|&&b| b == b' ' || b == b'\t').count();
            let key = &key[blank..];
            if count > 1 {
                self.scratch
                    .extend_from_slice(&(key.len() as u64).to_le_bytes());
            }
            self.scratch.extend_from_slice(key);
        }
        let key = self.scratch.as_slice();
        let hash = self.fields.hash(key);
        if self.fields.find(key, hash).is_some() {
            return Err(BLANKS);
        }
        let bytes = key.len() + 16;
        self.fields.insert(key, hash).ok_or(ALLOCATION)?;
        Ok(bytes)
    }
}

/// In a language locale, glibc ties keys that spell a letter in parts with
/// keys that write it whole (`и` and a combining breve, and `й`;
/// [`locale::compose`]) at every level, so the stable sort keeps their
/// records in input order: a Group per run of one spelling. Hash grouping
/// gives up when a new class's keys, so composed, are another class's.
struct Spellings {
    /// The composed keys of the classes that composing changed.
    composed: Table,
    /// Whether `composed` holds any.
    any: bool,
    scratch: Vec<u8>,
    text: String,
}

impl Spellings {
    fn new() -> Self {
        Self {
            composed: Table::new(),
            any: false,
            scratch: Vec::new(),
            text: String::new(),
        }
    }

    /// Admits a new class, of `count` sort keys joined in `tuple`, beside
    /// the `classes` admitted before: the bytes it takes, or why hash
    /// grouping gives up.
    fn admit(&mut self, tuple: &[u8], count: usize, classes: &Table) -> Result<usize, Outcome> {
        let Self {
            composed,
            any,
            scratch,
            text,
        } = self;
        // The keys composed, joined as `tuple` joins.
        scratch.clear();
        let mut changed = false;
        for key in keys_of(tuple, count) {
            let key = match locale::text(key).map(|key| locale::compose(key, text)) {
                Ok(Ok(true)) => {
                    changed = true;
                    text.as_bytes()
                }
                Ok(Ok(false)) => key,
                Ok(Err(locale::KeyTooLarge)) => return Err(ALLOCATION),
                Err(_) => return Err(TEXT),
            };
            scratch.try_reserve(key.len() + 8).map_err(|_| ALLOCATION)?;
            if count > 1 {
                scratch.extend_from_slice(&(key.len() as u64).to_le_bytes());
            }
            scratch.extend_from_slice(key);
        }
        let key = scratch.as_slice();
        if !changed {
            // Keys composing leaves alone tie only with a class it changed.
            return match *any && composed.find(key, composed.hash(key)).is_some() {
                true => Err(SPELLINGS),
                false => Ok(0),
            };
        }
        let hash = composed.hash(key);
        if composed.find(key, hash).is_some() || classes.find(key, classes.hash(key)).is_some() {
            return Err(SPELLINGS);
        }
        composed.insert(key, hash).ok_or(ALLOCATION)?;
        *any = true;
        Ok(key.len() + 16)
    }
}

/// A class's representative: its first record, or the latest one its
/// Operations kept, and its first record once a later one replaced it, where
/// a generated Output header needs that.
struct Representative {
    record: grouping::Raw,
    first: Option<Vec<u8>>,
}

/// The classes' key tuples, found by hash: open addressing over class
/// numbers, with each class's tuple and hash stored in class order.
struct Table {
    hasher: random::TableState,
    /// Each slot holds a class number plus one, or 0 when empty.
    slots: Vec<u32>,
    hashes: Vec<u64>,
    tuples: Vec<u8>,
    ends: Vec<usize>,
}

impl Table {
    fn new() -> Self {
        Self {
            hasher: random::TableState::default(),
            slots: Vec::new(),
            hashes: Vec::new(),
            tuples: Vec::new(),
            ends: Vec::new(),
        }
    }

    fn len(&self) -> usize {
        self.ends.len()
    }

    fn tuple(&self, class: usize) -> &[u8] {
        let start = class
            .checked_sub(1)
            .map_or(0, |previous| self.ends[previous]);
        &self.tuples[start..self.ends[class]]
    }

    fn hash(&self, tuple: &[u8]) -> u64 {
        std::hash::BuildHasher::hash_one(&self.hasher, tuple)
    }

    #[inline(always)]
    fn find(&self, tuple: &[u8], hash: u64) -> Option<usize> {
        let mask = self.slots.len().checked_sub(1)?;
        let mut at = hash as usize & mask;
        loop {
            let class = (self.slots[at] as usize).checked_sub(1)?;
            if self.hashes[class] == hash && self.tuple(class) == tuple {
                return Some(class);
            }
            at = (at + 1) & mask;
        }
    }

    /// Adds a class for `tuple`, returning its number, or `None` without
    /// memory for it.
    fn insert(&mut self, tuple: &[u8], hash: u64) -> Option<usize> {
        let class = self.len();
        if u32::try_from(class + 1).is_err()
            || self.hashes.try_reserve(1).is_err()
            || self.ends.try_reserve(1).is_err()
            || self.tuples.try_reserve(tuple.len()).is_err()
        {
            return None;
        }
        if (class + 1) * 2 > self.slots.len() {
            let size = (self.slots.len() * 2).max(64);
            let mut slots = Vec::new();
            slots.try_reserve_exact(size).ok()?;
            slots.resize(size, 0);
            self.slots = slots;
            for (class, &hash) in self.hashes.iter().enumerate() {
                Self::place(&mut self.slots, class, hash);
            }
        }
        self.tuples.extend_from_slice(tuple);
        self.ends.push(self.tuples.len());
        self.hashes.push(hash);
        Self::place(&mut self.slots, class, hash);
        Some(class)
    }

    fn place(slots: &mut [u32], class: usize, hash: u64) {
        let mask = slots.len() - 1;
        let mut at = hash as usize & mask;
        while slots[at] != 0 {
            at = (at + 1) & mask;
        }
        slots[at] = class as u32 + 1;
    }
}

/// Why the classes seen so far promise that hash grouping costs more than
/// the sort, if they do: too few records per class, or, where records rarely
/// follow one of their class (fewer than half of `recurring`), classes whose
/// state outgrows the processor's caches (`hot` bytes each).
///
/// The input's records are estimated from its `length` in bytes, and its
/// classes from the classes seen once and twice: the Chao1 estimate of the
/// classes not yet seen, of which the rest of the input is expected to show a
/// share (Chao and others, "Rarefaction and extrapolation with Hill numbers",
/// 2014).
fn spread(
    counts: &[u64],
    records: u64,
    recurring: u64,
    hot: usize,
    consumed: u64,
    length: u64,
) -> Option<Outcome> {
    let (mut once, mut twice) = (0u64, 0u64);
    for &count in counts {
        match count {
            1 => once += 1,
            2 => twice += 1,
            _ => {}
        }
    }
    let (once, twice, seen) = (once as f64, twice as f64, records as f64);
    let unseen = if twice > 0.0 {
        once * once / (2.0 * twice)
    } else {
        once * (once - 1.0).max(0.0) / 2.0
    };
    let total = seen * length as f64 / consumed.max(1) as f64;
    // Each further record is of an unseen class with probability
    // once / (seen * unseen + once).
    let rest = (total - seen).max(0.0);
    let shown = if unseen > 0.0 {
        -unseen * (rest * (-once / (seen * unseen + once)).ln_1p()).exp_m1()
    } else {
        0.0
    };
    let classes = counts.len() as f64 + shown;
    if total < RECORDS_PER_CLASS * classes {
        Some(SPREAD)
    } else if 2 * recurring < records && classes * hot as f64 > CACHED_STATE as f64 {
        Some(UNCACHED)
    } else {
        None
    }
}

/// A record's grouping keys as one key: the key alone, or each key after
/// its length. Keys are ASCII-uppercased with `-i`, as the sort compares them.
#[inline(always)]
fn tuple<'a>(
    record: &'a [u8],
    spans: &[std::ops::Range<usize>],
    fold: bool,
    scratch: &'a mut Vec<u8>,
) -> Option<&'a [u8]> {
    if let [span] = spans
        && !fold
    {
        return Some(&record[span.clone()]);
    }
    joined(record, spans, fold, scratch)
}

/// [`tuple`] of several keys, or with `-i`, inlined into the loop as `tuple`
/// is.
#[inline(always)]
fn joined<'a>(
    record: &[u8],
    spans: &[std::ops::Range<usize>],
    fold: bool,
    scratch: &'a mut Vec<u8>,
) -> Option<&'a [u8]> {
    scratch.clear();
    for span in spans {
        let key = &record[span.clone()];
        scratch.try_reserve(key.len() + 8).ok()?;
        if spans.len() > 1 {
            scratch.extend_from_slice(&(key.len() as u64).to_le_bytes());
        }
        let start = scratch.len();
        scratch.extend_from_slice(key);
        if fold {
            scratch[start..].make_ascii_uppercase();
        }
    }
    Some(scratch)
}

/// The `count` keys of a class's `tuple`.
fn keys_of(tuple: &[u8], count: usize) -> impl Iterator<Item = &[u8]> {
    let mut rest = tuple;
    (0..count).map(move |_| {
        if count == 1 {
            return std::mem::take(&mut rest);
        }
        let (length, tail) = rest.split_at(8);
        let length = u64::from_le_bytes(length.try_into().expect("eight bytes")) as usize;
        let (key, tail) = tail.split_at(length);
        rest = tail;
        key
    })
}

/// Reads the Input header and the data records, collecting them by class,
/// then writes the Input header and each class's Group in the sort's order,
/// unless it restarts: then nothing is written, and the Operations' state is
/// fresh. Errors after the Groups begin to be written are the Command's
/// (`Err`). Memory up to `target` (grown as it allows) holds the classes and
/// any input the reader holds to read again; `remaining` is the input's
/// length, where known.
#[allow(clippy::too_many_arguments)]
pub(super) fn calculate<O: CommandOutput>(
    reader: &mut impl replay::Rewind,
    options: &options::Options,
    binding: &mut Binding<'_>,
    collator: Option<&locale::Collator>,
    arithmetic: &mut numerics::Numerics,
    output: &mut O,
    target: memory::Target,
    setting: Setting,
    remaining: Option<u64>,
) -> Result<Outcome, Failure> {
    let mut progress = (0, 0);
    let outcome = classify(
        reader,
        options,
        binding,
        collator,
        arithmetic,
        output,
        target,
        setting,
        remaining,
        &mut progress,
    )?;
    #[cfg(test)]
    LAST.with(|last| last.set(Some(outcome)));
    if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
        let (records, groups) = progress;
        let message = match outcome {
            Outcome::Written => format!("hash grouping: {groups} Groups of {records} records\n"),
            Outcome::Restart(reason) => {
                format!("hash grouping: sorting instead after {records} records: {reason}\n")
            }
        };
        let _ = std::io::Write::write_all(&mut std::io::stderr(), message.as_bytes());
    }
    Ok(outcome)
}

/// [`calculate`], counting the records read and the classes in `progress`.
/// Inlined into its one caller: out of line, its loop hashes measurably
/// slower.
#[inline(always)]
#[allow(clippy::too_many_arguments)]
fn classify<O: CommandOutput>(
    reader: &mut impl replay::Rewind,
    options: &options::Options,
    binding: &mut Binding<'_>,
    collator: Option<&locale::Collator>,
    arithmetic: &mut numerics::Numerics,
    output: &mut O,
    target: memory::Target,
    setting: Setting,
    remaining: Option<u64>,
    progress: &mut (u64, usize),
) -> Result<Outcome, Failure> {
    let restart_after = match setting {
        Setting::RestartAfter(records) => Some(records),
        _ => None,
    };
    // The sort route reads the Input header again after a restart, so the
    // header's errors and GNU's --full warning are left to it: the warning is
    // written only once the Groups are.
    let mut intake = Intake::new(options, intake::Header::of(options));
    let mut header = Vec::new();
    let Some(found) = read_header(reader, &mut intake, &mut header, binding, options) else {
        return Ok(HEADER);
    };
    let Binding {
        program,
        operations,
        keys,
        ..
    } = binding;
    let keys = &*keys;
    let header_lines = intake.line();
    let fold = options.ignore_case;
    let generated_header = options.header_out && !options.header_in;
    let width = operations.len();
    let kept_numbers = operations.kept_numbers();
    let mut kept_text = Vec::new();
    if kept_text.try_reserve_exact(width).is_err() {
        return Ok(ALLOCATION);
    }
    kept_text.extend(operations.kept_text());
    let keeps = kept_numbers != 0 || !kept_text.is_empty();
    let class_bytes = operations.state_bytes()
        + std::mem::size_of::<Representative>()
        + 4 * std::mem::size_of::<u64>();
    let mut budget = target.bytes;
    let mut used = 0usize;
    let mut table = Table::new();
    let mut blanks = (options.input == records::Separator::Whitespace)
        .then(|| Blanks::new(options.record_end == 0));
    let mut spellings = collator.is_some().then(Spellings::new);
    // Each class's Group state (`width` Operations' state, class after class),
    // its record count and its representative.
    let (mut states, mut counts, mut representatives) = (Vec::new(), Vec::new(), Vec::new());
    let mut index = records::FieldIndex::selecting(keys.iter().copied().chain(operations.fields()));
    let mut spans = Vec::new();
    if spans.try_reserve_exact(keys.len()).is_err() {
        return Ok(ALLOCATION);
    }
    let (mut scratch, mut last_tuple, mut bytes) = (Vec::new(), Vec::new(), Vec::new());
    // The last record's class, and how many records followed one of their
    // class.
    let (mut last, mut recurring) = (None, 0u64);
    let (mut records, mut consumed, mut judged) = (0u64, 0u64, WINDOW);
    let mut held = 0;
    loop {
        let record = match intake.next(reader, &mut bytes) {
            Ok(Some(record)) => record,
            Ok(None) => break,
            Err(_) => return Ok(READ),
        };
        *progress = (records, counts.len());
        if restart_after == Some(records) {
            return Ok(TEST);
        }
        records += 1;
        let line = intake.line();
        let raw = record.raw();
        consumed += raw.len() as u64 + 1;
        if reader.held() != held {
            used = used.saturating_add(reader.held() - held);
            held = reader.held();
            if !within(&mut budget, used, target, records) {
                return Ok(MEMORY);
            }
        }
        index.clear();
        spans.clear();
        for &key in keys {
            let Ok(span) = indexed_field(raw, &mut index, key, line, options.input) else {
                return Ok(MISSING);
            };
            let span = span.start..span.start + span.length;
            if memchr::memchr(0, &raw[span.clone()]).is_some() {
                return Ok(NUL);
            }
            spans.push(span);
        }
        if let Some(blanks) = &blanks {
            if blanks.newlines && memchr::memchr(b'\n', raw).is_some() {
                return Ok(NEWLINE);
            }
            // Each sort key starts after the previous field, over the blanks
            // before its own.
            for span in &mut spans {
                span.start = raw[..span.start]
                    .iter()
                    .rposition(|&b| b != b' ' && b != b'\t')
                    .map_or(0, |at| at + 1);
            }
        }
        let Some(tuple) = tuple(raw, &spans, fold, &mut scratch) else {
            return Ok(ALLOCATION);
        };
        let class = match last {
            Some(class) if last_tuple == tuple => {
                recurring += 1;
                class
            }
            _ => {
                let hash = table.hash(tuple);
                let class = match table.find(tuple, hash) {
                    Some(class) => class,
                    None => {
                        if collator.is_some()
                            && spans
                                .iter()
                                .any(|span| locale::text(&raw[span.clone()]).is_err())
                        {
                            return Ok(TEXT);
                        }
                        used = used.saturating_add(
                            SLACK.saturating_mul(class_bytes + tuple.len() + raw.len()),
                        );
                        if !within(&mut budget, used, target, records) {
                            return Ok(MEMORY);
                        }
                        let Some(representative) = representative(raw, keys.len()) else {
                            return Ok(ALLOCATION);
                        };
                        if !operations.fresh_states(&mut states)
                            || counts.try_reserve(1).is_err()
                            || representatives.try_reserve(1).is_err()
                        {
                            return Ok(ALLOCATION);
                        }
                        if let Some(blanks) = &mut blanks {
                            match blanks.admit(tuple, keys.len()) {
                                Ok(bytes) => {
                                    used = used.saturating_add(SLACK.saturating_mul(bytes))
                                }
                                Err(outcome) => return Ok(outcome),
                            }
                        }
                        if let Some(spellings) = &mut spellings {
                            match spellings.admit(tuple, keys.len(), &table) {
                                Ok(bytes) => {
                                    used = used.saturating_add(SLACK.saturating_mul(bytes))
                                }
                                Err(outcome) => return Ok(outcome),
                            }
                        }
                        let Some(class) = table.insert(tuple, hash) else {
                            return Ok(ALLOCATION);
                        };
                        counts.push(0u64);
                        representatives.push(representative);
                        class
                    }
                };
                last_tuple.clear();
                if last_tuple.try_reserve(tuple.len()).is_err() {
                    return Ok(ALLOCATION);
                }
                last_tuple.extend_from_slice(tuple);
                last = Some(class);
                class
            }
        };
        if keeps {
            let mut kept = kept_numbers;
            for &field in &kept_text {
                let length = index
                    .field(raw, field, options.input)
                    .map_or(raw.len(), |span| span.length);
                kept = kept.saturating_add(length + 1);
            }
            used = used.saturating_add(SLACK.saturating_mul(kept));
            if !within(&mut budget, used, target, records) {
                return Ok(MEMORY);
            }
        }
        let Ok(keep) = operations.collect_into(
            &mut states[class * width..(class + 1) * width],
            raw,
            &mut index,
            line,
            options,
            arithmetic,
        ) else {
            return Ok(FAILED);
        };
        if keep && counts[class] != 0 {
            let representative = &mut representatives[class];
            // A first record kept apart, and a longer representative, take
            // more memory.
            let kept = if generated_header && representative.first.is_none() {
                representative.first = Some(std::mem::take(representative.record.bytes_mut()));
                raw.len()
            } else {
                raw.len()
                    .saturating_sub(representative.record.bytes().len())
            };
            if kept != 0 {
                used = used.saturating_add(SLACK.saturating_mul(kept));
                if !within(&mut budget, used, target, records) {
                    return Ok(MEMORY);
                }
            }
            let bytes = representative.record.bytes_mut();
            bytes.clear();
            if bytes.try_reserve(raw.len()).is_err() {
                return Ok(ALLOCATION);
            }
            bytes.extend_from_slice(raw);
        }
        counts[class] += 1;
        if records == judged {
            let hot = operations.state_bytes()
                + 4 * std::mem::size_of::<u64>()
                + table.tuples.len() / table.len().max(1);
            // Input of unknown length, such as a pipe, is taken to go on
            // for as long again as what has been read.
            let length = remaining.unwrap_or(consumed.saturating_mul(2));
            if let Some(outcome) = spread(&counts, records, recurring, hot, consumed, length) {
                return Ok(outcome);
            }
            judged = judged.saturating_mul(2);
        }
    }
    *progress = (records, counts.len());
    // The sort route reports a read error its own way.
    if intake.read_error().is_some() {
        return Ok(READ);
    }
    // Sorting the classes takes an order, and in a language locale each
    // class's collation keys (about three bytes per key byte).
    let sorting = std::mem::size_of::<usize>()
        + if collator.is_some() {
            4 * table.tuples.len() / table.len().max(1) + 64
        } else {
            0
        };
    used = used.saturating_add(SLACK.saturating_mul(sorting.saturating_mul(table.len())));
    if !within(&mut budget, used, target, records) {
        return Ok(MEMORY);
    }
    // The sort would refuse what hash grouping cannot allocate only if it
    // needs as much itself.
    let Ok(order) = sorted(&table, keys.len(), fold, collator) else {
        return Ok(ALLOCATION);
    };
    if options.warns_full() {
        intake::write_full_warning(program);
    }
    if found {
        output.first(&header, operations, keys)?;
    }
    let mut line = header_lines;
    for (written, &class) in order.iter().enumerate() {
        let representative = &mut representatives[class];
        if written == 0 && generated_header {
            let first = representative
                .first
                .as_deref()
                .unwrap_or(representative.record.bytes());
            output.first(first, operations, keys)?;
        }
        line += counts[class];
        // The sort ends a Group at the next Group's first record.
        let next = if written + 1 < order.len() {
            line + 1
        } else {
            line
        };
        operations.swap_states(&mut states[class * width..(class + 1) * width]);
        let mut context =
            grouping::Context::new(operations, keys, options, arithmetic, None, output);
        grouping::write_group(&mut representative.record, next, &mut context)?;
    }
    output.end(options)?;
    Ok(Outcome::Written)
}

/// Reads the Input header, where there is one, into `header`, resolving the
/// Command's names and binding its Operations to their fields: whether there
/// was one, or `None` when a name is not resolved or reading fails. Kept
/// apart from the record loop, whose code it would otherwise crowd.
#[inline(never)]
fn read_header(
    reader: &mut impl BufRead,
    intake: &mut Intake<'_>,
    header: &mut Vec<u8>,
    binding: &mut Binding<'_>,
    options: &options::Options,
) -> Option<bool> {
    if !options.header_in && binding.numbered().is_err() {
        return None;
    }
    let found = intake
        .header(reader, header, |record| binding.header(record, options))
        .ok()?;
    (!binding.unresolved_keys()).then_some(found)
}

/// Whether `used` bytes fit the budget, which grows as memory allows.
fn within(budget: &mut usize, used: usize, target: memory::Target, records: u64) -> bool {
    while used > *budget {
        match target
            .grows
            .then(|| memory::grow(*budget, records))
            .flatten()
        {
            Some(grown) if grown > *budget => *budget = grown,
            _ => return false,
        }
    }
    true
}

/// A new class's representative, its first record `raw`.
fn representative(raw: &[u8], keys: usize) -> Option<Representative> {
    let mut record = grouping::Raw::for_keys(keys, records::FieldIndex::default()).ok()?;
    let bytes = record.bytes_mut();
    bytes.try_reserve_exact(raw.len()).ok()?;
    bytes.extend_from_slice(raw);
    Some(Representative {
        record,
        first: None,
    })
}

/// The classes in the sort's order: keys in byte order (ASCII-uppercased
/// with `-i`), or in a language locale by the sort's key segments
/// (`storage::language_segments`).
fn sorted(
    table: &Table,
    count: usize,
    fold: bool,
    collator: Option<&locale::Collator>,
) -> Result<Vec<usize>, Failure> {
    let mut order = Vec::new();
    order
        .try_reserve_exact(table.len())
        .map_err(|_| super::allocation())?;
    order.extend(0..table.len());
    let Some(collator) = collator else {
        order.sort_unstable_by(|&a, &b| {
            keys_of(table.tuple(a), count).cmp(keys_of(table.tuple(b), count))
        });
        return Ok(order);
    };
    let mut sort_keys = storage::SortKeys::default();
    let mut text = storage::KeyText::default();
    let mut segments = Vec::new();
    segments
        .try_reserve_exact(table.len())
        .map_err(|_| super::allocation())?;
    let mut keys = Vec::new();
    keys.try_reserve_exact(count)
        .map_err(|_| super::allocation())?;
    for class in 0..table.len() {
        keys.clear();
        keys.extend(keys_of(table.tuple(class), count));
        let (mut bytes, mut ends) = (Vec::new(), Vec::new());
        // Keys are ASCII-uppercased with `-i` already, as the sort reads them.
        storage::language_segments(
            count,
            |at| locale::text(keys[at]),
            |at| keys[at],
            fold,
            collator,
            &mut sort_keys,
            &mut text,
            &mut bytes,
            &mut ends,
        )?;
        segments.push((bytes, ends));
    }
    let segment = |at: usize, n: usize| {
        let (bytes, ends) = &segments[at];
        &bytes[n.checked_sub(1).map_or(0, |previous| ends[previous])..ends[n]]
    };
    order.sort_unstable_by(|&a, &b| {
        (0..2 * count)
            .map(|n| segment(a, n).cmp(segment(b, n)))
            .find(|order| *order != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    });
    Ok(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command's exit status or failure, its output and its reported
    /// diagnostics.
    type Run = (Result<i32, (i32, Vec<u8>)>, Vec<u8>, Vec<Vec<u8>>);

    fn run(args: &[&str], input: &[u8], setting: Setting) -> (Run, Option<Outcome>) {
        run_from(args, input, setting, false)
    }

    /// [`run`] with input from a file, or `piped`, from input that cannot
    /// rewind, which hash grouping holds.
    fn run_from(
        args: &[&str],
        input: &[u8],
        setting: Setting,
        piped: bool,
    ) -> (Run, Option<Outcome>) {
        let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
        let mut output = Vec::new();
        let mut reported = Vec::new();
        LAST.with(|last| last.set(None));
        let grouping = match setting {
            Setting::Sort => "sort".to_owned(),
            Setting::Hash => "hash".to_owned(),
            Setting::RestartAfter(records) => format!("hash:restart={records}"),
        };
        let environment = crate::Environment {
            grouping: Some(grouping.into()),
            pipe_grouping: piped.then(|| "hash".into()),
            ..crate::Environment::from_env()
        };
        let mut report = |error: &Failure| {
            reported.push(error.message.clone());
            true
        };
        let result = if piped {
            // Small blocks, so that records cross them.
            crate::replay::test_block(7);
            let mut reader = input;
            let result = crate::run_in(
                &mut reader,
                &mut output,
                &args,
                b"fastmash",
                &mut report,
                environment,
            );
            crate::replay::test_block(crate::replay::BLOCK);
            result
        } else {
            let mut reader = std::io::Cursor::new(input);
            crate::run_in(
                &mut reader,
                &mut output,
                &args,
                b"fastmash",
                &mut report,
                environment,
            )
        };
        let result = result.map_err(|error| (error.status, error.message));
        ((result, output, reported), LAST.with(std::cell::Cell::get))
    }

    /// A small deterministic generator (xorshift64*).
    struct Draws(u64);
    impl Draws {
        fn next(&mut self, below: usize) -> usize {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 33) as usize % below
        }
        fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
            items[self.next(items.len())]
        }
    }

    const KEYS: &[&str] = &[
        "a",
        "A",
        "b",
        "B",
        "",
        "ab",
        "aB",
        "z z",
        "é",
        "É",
        "_",
        "-",
        "e\u{301}",
        "й",
        "и\u{306}",
        "L\u{b7}",
        "Ŀ",
    ];
    const VALUES: &[&str] = &[
        "1", "2", "-3", "0.5", "1e3", "-0", "0", "nan", "inf", " 7", "10", "x", "",
    ];

    /// A record of up to five fields; now and then one with a NUL key, a
    /// comment or a field too few.
    fn record(draws: &mut Draws) -> String {
        match draws.next(40) {
            0 => return "a\0b\t1\t2\n".to_owned(),
            1 => return "#c\t1\n".to_owned(),
            _ => {}
        }
        let fields = if draws.next(12) == 0 {
            1 + draws.next(2)
        } else {
            3 + draws.next(3)
        };
        let mut record = String::new();
        // Mostly tabs; now and then other blanks, which -W splits at and
        // keeps in its sort keys.
        const SEPARATORS: &[&str] = &["\t", "\t", "\t", "\t", " ", "  ", "\t "];
        if draws.next(8) == 0 {
            record.push(' ');
        }
        for field in 0..fields {
            if field != 0 {
                record.push_str(draws.pick(SEPARATORS));
            }
            record.push_str(if field < 2 || draws.next(4) == 0 {
                draws.pick(KEYS)
            } else {
                draws.pick(VALUES)
            });
        }
        record.push('\n');
        record
    }

    /// Commands every generated input runs, hash grouping's exclusions among
    /// them.
    const COMMANDS: &[&[&str]] = &[
        &["-s", "-g", "1", "count", "1"],
        &["-s", "-g", "1", "sum", "3"],
        &["-s", "-i", "-g", "1", "first", "3", "last", "4"],
        &["-s", "--full", "-g", "1", "last", "3"],
        &["-s", "-g", "1,2", "unique", "3"],
        &["-s", "--header-out", "-g", "2", "collapse", "1"],
        &[
            "-s",
            "-g",
            "1",
            "median",
            "3",
            "mode",
            "3",
            "countunique",
            "4",
        ],
        &["-s", "-g", "2", "mean", "3", "sstdev", "3"],
        &["-s", "crosstab", "1,2"],
        &["-s", "-i", "crosstab", "1,2", "first", "3"],
        &["-s", "-H", "-g", "1", "sum", "3"],
        &["-s", "-C", "-g", "1", "count", "1"],
        &[
            "-s", "--narm", "-g", "1", "sum", "3", "min", "3", "max", "3",
        ],
        &["-s", "-g", "1", "pcov", "3:4"],
        &["-s", "--full", "-i", "-g", "1", "first", "3"],
        &["-s", "-g", "2,1", "absmin", "3", "range", "3"],
        &["-s", "--header-out", "--full", "-g", "1", "count", "1"],
        &["-s", "-i", "--header-in", "-g", "1", "countunique", "2"],
        &["-s", "-t", "a", "-g", "1", "count", "1"],
        &["-s", "-i", "-t", "A", "-g", "1,2", "count", "1"],
        &["-s", "-S", "7", "-g", "1", "rand", "3"],
        &["-s", "-W", "-g", "1", "count", "1"],
        &["-s", "-W", "-i", "-g", "2,1", "sum", "3"],
        &["-s", "-W", "--header-in", "-g", "1", "first", "2"],
        &["-s", "-z", "-W", "-g", "1", "count", "1"],
        &["-s", "-g", "1", "md5", "3"],
        &[
            "-s", "--narm", "-g", "1", "pskew", "3", "q1", "3", "perc:90", "3",
        ],
    ];

    /// On generated inputs, hash grouping, and the sort after any restart,
    /// write exactly what the sort writes, diagnostics and status included.
    #[test]
    fn hash_grouping_and_its_restarts_write_what_the_sort_writes() {
        let mut draws = Draws(0x9e37_79b9_7f4a_7c15);
        let mut written = 0;
        for _ in 0..300 {
            let records = draws.next(14);
            let input: String = (0..records).map(|_| record(&mut draws)).collect();
            for command in COMMANDS {
                let (sorted, _) = run(command, input.as_bytes(), Setting::Sort);
                for setting in [
                    Setting::Hash,
                    Setting::RestartAfter(0),
                    Setting::RestartAfter(1),
                    Setting::RestartAfter(records as u64 / 2),
                ] {
                    for piped in [false, true] {
                        let (hashed, outcome) = run_from(command, input.as_bytes(), setting, piped);
                        assert_eq!(
                            hashed, sorted,
                            "{command:?} with {setting:?} on {input:?}, piped {piped} ({outcome:?})"
                        );
                        written += usize::from(outcome == Some(Outcome::Written));
                    }
                }
            }
        }
        // Hash grouping itself writes many of them.
        assert!(written > 300 * COMMANDS.len() / 4, "{written}");
    }

    #[test]
    fn exclusions_and_anomalies_leave_the_groups_to_the_sort() {
        let cases: &[(&[&str], &str, Option<Outcome>)] = &[
            (
                &["-s", "-g", "1", "sum", "2"],
                "a\t1\nb\t2\na\t3\n",
                Some(Outcome::Written),
            ),
            (&["-s", "-g", "1", "rand", "2"], "a\t1\n", None),
            (
                &["-s", "-W", "-g", "1", "sum", "2"],
                "a 1\nb\t2\na  3\n",
                Some(Outcome::Written),
            ),
            (
                &["-s", "-W", "-g", "2", "sum", "1"],
                "1 a\n2  a\n",
                Some(BLANKS),
            ),
            (
                &["-s", "-W", "-g", "1", "sum", "2"],
                "a 1\n a 2\n",
                Some(BLANKS),
            ),
            (
                &["-s", "-z", "-W", "-g", "1", "sum", "2"],
                "a\nb 1\0",
                Some(NEWLINE),
            ),
            (
                &["-s", "--vnlog", "-g", "x", "sum", "y"],
                "# x y\na 1\n",
                None,
            ),
            (&["-s", "-g", "2", "sum", "1"], "1\tb\n2\n", Some(MISSING)),
            (&["-s", "-g", "1", "sum", "2"], "a\0\t1\n", Some(NUL)),
            (&["-s", "-g", "1", "sum", "2"], "a\t1\na\tx\n", Some(FAILED)),
        ];
        for &(args, input, expected) in cases {
            let (hashed, outcome) = run(args, input.as_bytes(), Setting::Hash);
            assert_eq!(outcome, expected, "{args:?} on {input:?}");
            assert_eq!(hashed, run(args, input.as_bytes(), Setting::Sort).0);
        }
    }

    /// Input whose records are nearly all in Groups of their own goes to the
    /// sort after the first window of records.
    #[test]
    fn inputs_of_single_record_groups_go_to_the_sort() {
        let input: String = (0..WINDOW + 10).map(|n| format!("{n}\t1\n")).collect();
        let args = ["-s", "-g", "1", "count", "2"];
        let (hashed, outcome) = run(&args, input.as_bytes(), Setting::Hash);
        assert_eq!(outcome, Some(SPREAD));
        assert_eq!(hashed, run(&args, input.as_bytes(), Setting::Sort).0);
    }

    /// Near its end, an input is expected to show few classes it has not
    /// yet shown, however many unseen classes its singletons promise: here
    /// 6,000 classes seen once promise 36,000 more, too many to cache, but
    /// the last 5% of the input shows about 300 of them.
    #[test]
    fn the_rest_of_an_input_shows_a_share_of_its_unseen_classes() {
        let counts: Vec<u64> = [(1, 6000), (2, 500), (200, 1500)]
            .into_iter()
            .flat_map(|(count, classes)| std::iter::repeat_n(count, classes))
            .collect();
        let records = counts.iter().sum();
        let consumed = 100 * records;
        let length = consumed / 20 * 21;
        assert_eq!(spread(&counts, records, 0, 430, consumed, length), None);
        // Early on, the rest of the input is expected to show them all.
        assert_eq!(
            spread(&counts, records, 0, 430, consumed, 100 * consumed),
            Some(UNCACHED)
        );
    }

    /// Classes sort as the sort orders keys: shorter keys first among equal
    /// prefixes, and bytes above 0x7f last.
    #[test]
    fn classes_follow_the_sort_order() {
        let mut table = Table::new();
        for key in [&b"b"[..], b"", b"ab", b"a", b"\xc3\xa9", b"_", b"A"] {
            let hash = table.hash(key);
            assert_eq!(table.find(key, hash), None);
            let class = table.insert(key, hash).unwrap();
            assert_eq!(table.find(key, hash), Some(class));
        }
        let order = sorted(&table, 1, false, None).ok().unwrap();
        let keys: Vec<&[u8]> = order.iter().map(|&at| table.tuple(at)).collect();
        assert_eq!(keys, [&b""[..], b"A", b"_", b"a", b"ab", b"b", b"\xc3\xa9"]);
    }

    /// The table finds every class it holds, across its growth.
    #[test]
    fn the_table_finds_its_classes_as_it_grows() {
        let mut table = Table::new();
        let keys: Vec<Vec<u8>> = (0..5000u32).map(|n| n.to_string().into_bytes()).collect();
        for (class, key) in keys.iter().enumerate() {
            let hash = table.hash(key);
            assert_eq!(table.insert(key, hash), Some(class));
        }
        for (class, key) in keys.iter().enumerate() {
            assert_eq!(table.find(key, table.hash(key)), Some(class));
            assert_eq!(table.tuple(class), &key[..]);
        }
        assert_eq!(table.find(b"x", table.hash(b"x")), None);
    }
}
