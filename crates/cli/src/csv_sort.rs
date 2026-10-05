//! Stable ordering and bounded runs of complete decoded CSV Records. The ordinary
//! sort's comparison keys, buffers and live target/growth policy are reused.
use super::{memory, spill, storage};
#[path = "csv_spill.rs"]
mod runs;
use crate::{
    Failure,
    binding::Binding,
    command_memory,
    command_output::CommandOutput,
    csv_input::{Intake, Location, Record, Retained, View},
    grouping, intake, locale, numerics, options, random, unsupported,
};
#[cfg(test)]
pub(crate) use runs::{RunFault, fail_run};
use std::{cmp::Ordering, io::BufRead, mem::size_of};

fn capacity() -> Failure {
    unsupported("CSV sorting capacity exceeded")
}

#[derive(Default)]
struct Keys {
    bytes: Vec<u8>,
    ends: Vec<usize>,
}

impl Keys {
    fn retained_bytes(&self) -> Option<usize> {
        self.ends
            .capacity()
            .checked_mul(size_of::<usize>())?
            .checked_add(self.bytes.capacity())
    }

    fn view(&self) -> KeyView<'_> {
        KeyView {
            bytes: &self.bytes,
            ends: &self.ends,
        }
    }
}

#[derive(Default)]
struct KeyBuilder {
    cache: storage::SortKeys,
    text: storage::KeyText,
    result: Keys,
}

impl KeyBuilder {
    fn build(
        &mut self,
        record: &Record,
        keys: &[u64],
        options: &options::Options,
        collator: &locale::Collator,
    ) -> Result<(), Failure> {
        keys.len().checked_mul(2).ok_or_else(capacity)?;
        storage::language_segments(
            keys.len(),
            |at| locale::text(record.sort_field(keys[at])),
            |at| record.sort_field(keys[at]),
            options.ignore_case,
            collator,
            &mut self.cache,
            &mut self.text,
            &mut self.result.bytes,
            &mut self.result.ends,
        )
    }

    fn retained_bytes(&self) -> Option<usize> {
        self.cache
            .retained_bytes()?
            .checked_add(self.text.folded.capacity())?
            .checked_add(self.text.composed.capacity())?
            .checked_add(self.result.retained_bytes()?)
    }
}

struct Stored {
    record: Record,
    keys: Keys,
}

#[derive(Clone, Copy)]
struct StoredView<'r> {
    record: View<'r>,
    keys: KeyView<'r>,
}

#[derive(Clone, Copy, Default)]
struct KeyView<'r> {
    bytes: &'r [u8],
    ends: &'r [usize],
}

impl KeyView<'_> {
    fn compare(self, other: Self) -> Ordering {
        let mut start = 0;
        let mut other_start = 0;
        for (&end, &other_end) in self.ends.iter().zip(other.ends) {
            let order = self.bytes[start..end].cmp(&other.bytes[other_start..other_end]);
            if order != Ordering::Equal {
                return order;
            }
            start = end;
            other_start = other_end;
        }
        self.ends.len().cmp(&other.ends.len())
    }
}

impl Stored {
    fn view(&self) -> StoredView<'_> {
        StoredView {
            record: self.record.view(),
            keys: self.keys.view(),
        }
    }
}

struct KeyRanges {
    bytes: std::ops::Range<usize>,
    ends: std::ops::Range<usize>,
}

/// The two real chunk layouts share one retention lifecycle. C chunks own no
/// language vectors or per-Record language ranges.
trait ChunkKeys: Default {
    type Ranges;
    fn storage(&self) -> Option<&Keys> {
        None
    }
    fn storage_mut(&mut self) -> Option<&mut Keys> {
        None
    }
    fn append(&mut self, pending: KeyView<'_>) -> Self::Ranges;
    fn view(&self, ranges: &Self::Ranges) -> KeyView<'_>;
}

impl ChunkKeys for () {
    type Ranges = ();
    fn append(&mut self, _pending: KeyView<'_>) {}
    fn view(&self, _ranges: &()) -> KeyView<'_> {
        KeyView::default()
    }
}

impl ChunkKeys for Keys {
    type Ranges = KeyRanges;
    fn storage(&self) -> Option<&Keys> {
        Some(self)
    }
    fn storage_mut(&mut self) -> Option<&mut Keys> {
        Some(self)
    }
    fn append(&mut self, pending: KeyView<'_>) -> KeyRanges {
        let start = (self.bytes.len(), self.ends.len());
        self.bytes.extend_from_slice(pending.bytes);
        self.ends.extend_from_slice(pending.ends);
        KeyRanges {
            bytes: start.0..self.bytes.len(),
            ends: start.1..self.ends.len(),
        }
    }
    fn view(&self, ranges: &KeyRanges) -> KeyView<'_> {
        KeyView {
            bytes: &self.bytes[ranges.bytes.clone()],
            ends: &self.ends[ranges.ends.clone()],
        }
    }
}

struct Entry<K: ChunkKeys> {
    bytes: std::ops::Range<usize>,
    ends: std::ops::Range<usize>,
    location: Location,
    keys: K::Ranges,
}

struct Packed<K: ChunkKeys> {
    bytes: Vec<u8>,
    ends: Vec<usize>,
    entries: Vec<Entry<K>>,
    keys: K,
    target: memory::Target,
}

impl<K: ChunkKeys> Packed<K> {
    fn new(target: memory::Target) -> Self {
        Self {
            bytes: Vec::new(),
            ends: Vec::new(),
            entries: Vec::new(),
            keys: K::default(),
            target,
        }
    }

    fn view(&self, entry: &Entry<K>) -> StoredView<'_> {
        StoredView {
            record: View {
                bytes: &self.bytes[entry.bytes.clone()],
                ends: &self.ends[entry.ends.clone()],
                location: entry.location,
            },
            keys: self.keys.view(&entry.keys),
        }
    }

    fn retained_bytes(&self) -> Option<usize> {
        self.ends
            .capacity()
            .checked_mul(size_of::<usize>())?
            .checked_add(self.bytes.capacity())?
            .checked_add(self.entries.capacity().checked_mul(size_of::<Entry<K>>())?)?
            .checked_add(self.keys.storage().map_or(Some(0), Keys::retained_bytes)?)
    }

    fn admit(&mut self, required: usize) -> bool {
        while required > spill::chunk_memory(self.target.bytes, 0) {
            let next = self
                .target
                .grows
                .then(|| memory::grow(self.target.bytes, self.entries.len() as u64))
                .flatten()
                .filter(|&bytes| bytes > self.target.bytes);
            let Some(bytes) = next else {
                self.target.grows = false;
                return false;
            };
            self.target.bytes = bytes;
        }
        true
    }

    fn sort(&mut self, order: &Order<'_>) {
        let (bytes, ends, keys) = (&self.bytes, &self.ends, &self.keys);
        self.entries.sort_unstable_by(|a, b| {
            let view = |entry: &Entry<K>| StoredView {
                record: View {
                    bytes: &bytes[entry.bytes.clone()],
                    ends: &ends[entry.ends.clone()],
                    location: entry.location,
                },
                keys: keys.view(&entry.keys),
            };
            order.compare_views(view(a), view(b))
        });
    }

    fn spill(&mut self, runs: &mut runs::Runs, order: &Order<'_>) -> Result<(), Failure> {
        self.target.grows = false;
        runs.buffer = spill::run_buffer(self.target.bytes);
        self.sort(order);
        let run = runs.write(self.entries.iter().map(|entry| self.view(entry)))?;
        // Return the entire chunk before registration can decode merge heads.
        self.bytes = Vec::new();
        self.ends = Vec::new();
        self.entries = Vec::new();
        self.keys = K::default();
        runs.add(run, order)
    }

    fn push(
        &mut self,
        record: &mut Record,
        mut builder: Option<&mut KeyBuilder>,
        runs: &mut runs::Runs,
        order: &Order<'_>,
    ) -> Result<(), Failure> {
        let scratch = record
            .retained_bytes()
            .and_then(|bytes| {
                bytes.checked_add(builder.as_ref().map_or(Some(0), |b| b.retained_bytes())?)
            })
            .ok_or_else(capacity)?;
        loop {
            let key_lengths = if let Some(keys) = self.keys.storage() {
                let pending = &builder.as_ref().unwrap().result;
                [
                    keys.bytes
                        .len()
                        .checked_add(pending.bytes.len())
                        .ok_or_else(capacity)?,
                    keys.ends
                        .len()
                        .checked_add(pending.ends.len())
                        .ok_or_else(capacity)?,
                ]
            } else {
                [0; 2]
            };
            let lengths = [
                self.bytes.len().checked_add(record.bytes.len()),
                self.ends.len().checked_add(record.ends.len()),
                self.entries.len().checked_add(1),
            ];
            let lengths = [
                lengths[0].ok_or_else(capacity)?,
                lengths[1].ok_or_else(capacity)?,
                lengths[2].ok_or_else(capacity)?,
                key_lengths[0],
                key_lengths[1],
            ];
            let existing = [
                self.bytes.capacity(),
                self.ends.capacity(),
                self.entries.capacity(),
                self.keys.storage().map_or(0, |keys| keys.bytes.capacity()),
                self.keys.storage().map_or(0, |keys| keys.ends.capacity()),
            ];
            let widths = [
                1,
                size_of::<usize>(),
                size_of::<Entry<K>>(),
                1,
                size_of::<usize>(),
            ];
            let mut desired = std::array::from_fn::<_, 5, _>(|at| existing[at].max(lengths[at]));
            let required = desired
                .iter()
                .zip(widths)
                .try_fold(scratch, |total, (&count, width)| {
                    total.checked_add(count.checked_mul(width)?)
                })
                .ok_or_else(capacity)?;
            if !self.admit(required) && !self.entries.is_empty() {
                self.spill(runs, order)?;
                continue;
            }
            // Optional spare growth never consumes the required payload's share.
            let mut spare = spill::chunk_memory(self.target.bytes, required);
            for at in 0..5 {
                if desired[at] > existing[at] {
                    let growth = existing[at]
                        .checked_add((existing[at] / 8).max(64))
                        .ok_or_else(capacity)?;
                    let extra = growth.saturating_sub(desired[at]).min(spare / widths[at]);
                    desired[at] = desired[at].checked_add(extra).ok_or_else(capacity)?;
                    spare -= extra * widths[at];
                }
            }
            if desired[0] > self.bytes.capacity() {
                let additional = desired[0] - self.bytes.len();
                command_memory::reserve_exact(&mut self.bytes, additional)?;
            }
            if desired[1] > self.ends.capacity() {
                let additional = desired[1] - self.ends.len();
                command_memory::reserve_exact(&mut self.ends, additional)?;
            }
            if desired[2] > self.entries.capacity() {
                let additional = desired[2] - self.entries.len();
                command_memory::reserve_exact(&mut self.entries, additional)?;
            }
            if let Some(keys) = self.keys.storage_mut() {
                if desired[3] > keys.bytes.capacity() {
                    let additional = desired[3] - keys.bytes.len();
                    command_memory::reserve_exact(&mut keys.bytes, additional)?;
                }
                if desired[4] > keys.ends.capacity() {
                    let additional = desired[4] - keys.ends.len();
                    command_memory::reserve_exact(&mut keys.ends, additional)?;
                }
            }
            let actual = self
                .retained_bytes()
                .and_then(|bytes| bytes.checked_add(scratch))
                .ok_or_else(capacity)?;
            if !self.admit(actual) && !self.entries.is_empty() {
                self.spill(runs, order)?;
                continue;
            }
            let entry = Entry {
                bytes: self.bytes.len()..lengths[0],
                ends: self.ends.len()..lengths[1],
                location: record.location,
                keys: self.keys.append(
                    builder
                        .as_ref()
                        .map_or(KeyView::default(), |b| b.result.view()),
                ),
            };
            self.bytes.extend_from_slice(&record.bytes);
            self.ends.extend_from_slice(&record.ends);
            self.entries.push(entry);
            // Safely stored oversized input must not leave producer scratch
            // consuming the whole share for every following small Record.
            if scratch >= spill::chunk_memory(self.target.bytes, 0) {
                *record = Record::default();
                if let Some(builder) = builder.as_deref_mut() {
                    *builder = KeyBuilder::default();
                }
            }
            let actual = self
                .retained_bytes()
                .and_then(|bytes| {
                    bytes
                        .checked_add(record.retained_bytes()?)?
                        .checked_add(builder.as_ref().map_or(Some(0), |b| b.retained_bytes())?)
                })
                .ok_or_else(capacity)?;
            if !self.admit(actual) {
                self.spill(runs, order)?;
            }
            return Ok(());
        }
    }
    fn emit<'s>(
        &'s mut self,
        runs: &mut runs::Runs,
        order: &Order<'_>,
        mut consume: impl FnMut(Retained<'s>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        self.sort(order);
        if runs.is_empty() {
            for entry in &self.entries {
                consume(Retained::Borrowed(self.view(entry).record))?;
            }
            return Ok(());
        }
        if !self.entries.is_empty() {
            self.spill(runs, order)?;
        }
        let buffer = runs.buffer;
        std::mem::replace(runs, runs::Runs::new(buffer))
            .emit(order, |stored| consume(Retained::Owned(stored.record)))
    }
}

// One invocation-local value: keep the collator inline rather than add an
// allocation merely to reduce the C variant's stack footprint.
#[allow(clippy::large_enum_variant)]
enum Rows {
    C(Packed<()>),
    Language {
        packed: Packed<Keys>,
        builder: KeyBuilder,
        collator: locale::Collator,
    },
}

struct Order<'a> {
    keys: &'a [u64],
    fold: bool,
    language: bool,
}
impl Order<'_> {
    fn compare(&self, a: &Stored, b: &Stored) -> Ordering {
        self.compare_views(a.view(), b.view())
    }

    fn compare_views(&self, a: StoredView<'_>, b: StoredView<'_>) -> Ordering {
        if self.language {
            a.keys
                .compare(b.keys)
                .then(a.record.location.record.cmp(&b.record.location.record))
        } else {
            self.compare_fields(a.record, b.record)
        }
    }

    fn compare_fields(&self, a: View<'_>, b: View<'_>) -> Ordering {
        self.keys
            .iter()
            .find_map(|&key| {
                let (a, b) = (a.sort_field(key), b.sort_field(key));
                let order = if self.fold {
                    // GNU sort -f and the native sorter uppercase. Lowercase
                    // would place punctuation differently against letters.
                    a.iter()
                        .map(u8::to_ascii_uppercase)
                        .cmp(b.iter().map(u8::to_ascii_uppercase))
                } else {
                    a.cmp(b)
                };
                (order != Ordering::Equal).then_some(order)
            })
            .unwrap_or(Ordering::Equal)
            .then(a.location.record.cmp(&b.location.record))
    }
}

/// The existing decoded sort's chunks and runs, shared by calculation and
/// selection. Source headers and Binding stay with the calling Command.
pub(crate) struct Sorting {
    rows: Rows,
    runs: runs::Runs,
}

impl Sorting {
    pub(crate) fn new(options: &options::Options) -> Result<Self, Failure> {
        let target = memory::target(options.sort_memory.as_deref())?;
        let collator = options.locale.collator()?;
        Ok(Self {
            rows: if let Some(collator) = collator {
                Rows::Language {
                    packed: Packed::new(target),
                    builder: KeyBuilder::default(),
                    collator,
                }
            } else {
                Rows::C(Packed::new(target))
            },
            runs: runs::Runs::new(spill::run_buffer(target.bytes)),
        })
    }

    pub(crate) fn push(
        &mut self,
        record: &mut Record,
        keys: &[u64],
        options: &options::Options,
    ) -> Result<(), Failure> {
        let order = Order {
            keys,
            fold: options.ignore_case,
            language: matches!(self.rows, Rows::Language { .. }),
        };
        match &mut self.rows {
            Rows::C(rows) => rows.push(record, None, &mut self.runs, &order),
            Rows::Language {
                packed,
                builder,
                collator,
            } => {
                builder.build(record, keys, options, collator)?;
                packed.push(record, Some(builder), &mut self.runs, &order)
            }
        }
    }

    pub(crate) fn emit<'s>(
        &'s mut self,
        keys: &[u64],
        options: &options::Options,
        consume: impl FnMut(Retained<'s>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let order = Order {
            keys,
            fold: options.ignore_case,
            language: matches!(self.rows, Rows::Language { .. }),
        };
        match &mut self.rows {
            Rows::C(rows) => rows.emit(&mut self.runs, &order, consume),
            Rows::Language {
                packed, builder, ..
            } => {
                *builder = KeyBuilder::default();
                packed.emit(&mut self.runs, &order, consume)
            }
        }
    }
}

pub(crate) fn calculate(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut impl CommandOutput,
    options: &options::Options,
    mut binding: Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    mut random: Option<&mut random::RandomState>,
) -> Result<(), Failure> {
    let mut sorting = Sorting::new(options)?;
    let mut warning = options.warns_full().then_some(binding.program);
    if !options.header_in {
        binding.numbered()?;
        if let Some(program) = warning.take() {
            intake::write_full_warning(program);
        }
    }
    let mut intake = Intake::new();
    let mut record = Record::default();
    while intake.next(reader, &mut record)? {
        if record.location.record == 1 && options.header_in {
            binding.decoded_header(&record, options)?;
            if let Some(program) = warning.take() {
                intake::write_full_warning(program);
            }
            output.first_decoded(record.view(), &binding.operations, &binding.keys)?;
            // Header parsing has its own growth policy; its spare buffers must
            // not enlarge the first data Record retained for sorting.
            record = Record::default();
            continue;
        }
        sorting.push(&mut record, &binding.keys, options)?;
    }
    if let Some(program) = warning {
        intake::write_full_warning(program);
    }
    // An input transport error cannot become a successful partial sorted report.
    intake.finish()?;
    drop(record);
    let mut calculation = grouping::Calculation::new(&binding.keys, options);
    let mut first = true;
    sorting.emit(&binding.keys, options, |record| {
        if first && !options.header_in {
            output.first_decoded(record.view(), &binding.operations, &binding.keys)?;
        }
        first = false;
        let line = record.view().location.record;
        let mut context = grouping::Context::new(
            &mut binding.operations,
            &binding.keys,
            options,
            arithmetic,
            random.as_deref_mut(),
            output,
        );
        calculation.push(record, line, &mut context).map(|_| ())
    })?;
    let mut context = grouping::Context::new(
        &mut binding.operations,
        &binding.keys,
        options,
        arithmetic,
        None,
        output,
    );
    calculation.finish(0, &mut context)
}
