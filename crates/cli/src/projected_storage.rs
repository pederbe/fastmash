//! Owned segments with byte-encoded offsets and reusable projection ranges.
use super::{Failure, allocation};
use crate::locale;
use std::{cmp::Ordering, ops::Range};

/// Recent collation sort keys by key text. Grouping keys usually repeat, and a
/// sort key depends only on its text, so reuse is exact. Fixed size: 256 slots
/// of keys up to 64 bytes, allocated on first use. A miss adds a hash, a
/// comparison and a copy into the slot's reused buffers; a slot whose buffers
/// cannot grow is left empty.
#[derive(Default)]
pub(super) struct SortKeys {
    slots: Vec<(Vec<u8>, Vec<u8>)>,
}

impl SortKeys {
    const SLOTS: usize = 256;
    const LONGEST: usize = 64;

    /// Actual cache allocations, including unused slot capacity.
    pub(super) fn retained_bytes(&self) -> Option<usize> {
        self.slots.iter().try_fold(
            self.slots
                .capacity()
                .checked_mul(std::mem::size_of::<(Vec<u8>, Vec<u8>)>())?,
            |bytes, (text, key)| {
                bytes
                    .checked_add(text.capacity())?
                    .checked_add(key.capacity())
            },
        )
    }

    /// Append the sort key of `text` to `out`.
    pub(super) fn write(
        &mut self,
        collator: &locale::Collator,
        text: &str,
        out: &mut Vec<u8>,
    ) -> Result<(), Failure> {
        if text.len() > Self::LONGEST {
            return sort_key(collator, text, out);
        }
        if self.slots.is_empty() && self.slots.try_reserve_exact(Self::SLOTS).is_ok() {
            self.slots.resize_with(Self::SLOTS, Default::default);
        }
        // FNV-1a picks the slot; any hash is correct, since hits compare bytes.
        let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        let Some(slot) = self.slots.get_mut(hash as usize % Self::SLOTS) else {
            return sort_key(collator, text, out);
        };
        let (cached, key) = slot;
        // An empty slot holds no key: every sort key has at least one byte.
        if !key.is_empty() && cached == text.as_bytes() {
            out.try_reserve(key.len()).map_err(|_| allocation())?;
            out.extend_from_slice(key);
            return Ok(());
        }
        let start = out.len();
        sort_key(collator, text, out)?;
        let computed = &out[start..];
        cached.clear();
        key.clear();
        if cached.try_reserve(text.len()).is_ok() && key.try_reserve(computed.len()).is_ok() {
            cached.extend_from_slice(text.as_bytes());
            key.extend_from_slice(computed);
        } else {
            cached.clear();
            key.clear();
        }
        Ok(())
    }
}

/// Appends the collation sort key of `text` to `out`, refusing if `out`
/// cannot grow.
fn sort_key(collator: &locale::Collator, text: &str, out: &mut Vec<u8>) -> Result<(), Failure> {
    collator
        .write_key(text, out)
        .map_err(|locale::KeyTooLarge| allocation())
}

/// Segments whose first `key_count` are a record's grouping keys, as sorting
/// stores them.
pub(super) trait Keys {
    fn key_count(&self) -> usize;
    fn segment(&self, at: usize) -> &[u8];
    /// Orders by the key segments in turn, then by key count.
    fn compare_keys(&self, other: &Self) -> Ordering {
        let (a, b) = (self.key_count(), other.key_count());
        for at in 0..a.min(b) {
            let order = self.segment(at).cmp(other.segment(at));
            if order != Ordering::Equal {
                return order;
            }
        }
        a.cmp(&b)
    }
}

/// A record's segments: its grouping keys, then its projected fields (packed)
/// or its original bytes.
pub(super) trait Storage: Keys + Sized {
    const ORIGINAL: bool;
    fn original(&self) -> Option<&[u8]>;
}

/// Storage built from a projection: the records of the reference sorts that
/// tests compare sort chunks with.
#[cfg(test)]
pub(super) trait Build: Storage {
    fn build(
        raw: &[u8],
        data: &[u8],
        spans: &[Range<usize>],
        keys: usize,
        fold: bool,
    ) -> Result<Self, Failure>;
    /// Storage whose key segments are `keys`, split at `ends`, followed by
    /// `original`.
    fn language(_keys: &[u8], _ends: &[usize], _original: &[u8]) -> Result<Self, Failure> {
        Err(super::unsupported(
            "internal language sorter requires original records",
        ))
    }
}

/// A record in a sort chunk's arena: a table of the segments' cumulative end
/// offsets, `width` bytes each (little-endian), then the segments. The width
/// is the smallest of 1, 2, 4 and 8 bytes that holds the segments' total
/// length, so a small record costs one byte per segment and a record of any
/// size or segment count fits.
#[derive(Clone, Copy, Debug)]
pub(super) struct View<'a> {
    /// The end table.
    table: &'a [u8],
    /// The segments.
    payload: &'a [u8],
    /// The first segment's end, which is read most.
    first: usize,
    width: u8,
    count: u32,
    keys: u32,
}

impl<'a> View<'a> {
    /// The end-table width for segments of `length` bytes in total.
    pub(super) fn width(length: usize) -> u8 {
        if length <= 0xff {
            1
        } else if length <= 0xffff {
            2
        } else if length <= 0xffff_ffff {
            4
        } else {
            8
        }
    }
    /// Appends `end` to an end table of width `width`.
    pub(super) fn put_end(out: &mut Vec<u8>, end: usize, width: u8) {
        match width {
            1 => out.push(end as u8),
            2 => out.extend_from_slice(&(end as u16).to_le_bytes()),
            4 => out.extend_from_slice(&(end as u32).to_le_bytes()),
            _ => out.extend_from_slice(&(end as u64).to_le_bytes()),
        }
    }
    #[inline]
    fn read(table: &[u8], width: u8, at: usize) -> usize {
        match width {
            1 => usize::from(table[at]),
            2 => usize::from(u16::from_le_bytes([table[2 * at], table[2 * at + 1]])),
            4 => {
                let bytes = &table[4 * at..4 * at + 4];
                u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize
            }
            _ => u64::from_le_bytes(table[8 * at..8 * at + 8].try_into().unwrap()) as usize,
        }
    }
    /// The record of `count` segments (the first `keys` of them keys) whose
    /// end table, of `width`-byte entries, starts `bytes`.
    #[inline]
    pub(super) fn new(bytes: &'a [u8], width: u8, count: usize, keys: usize) -> Self {
        let (table, rest) = bytes.split_at(count * usize::from(width));
        let (first, length) = match count {
            0 => (0, 0),
            1 => {
                let end = Self::read(table, width, 0);
                (end, end)
            }
            _ => (
                Self::read(table, width, 0),
                Self::read(table, width, count - 1),
            ),
        };
        Self {
            table,
            payload: &rest[..length],
            first,
            width,
            count: count as u32,
            keys: keys as u32,
        }
    }
    /// The end of segment `at`.
    #[inline]
    fn end(&self, at: usize) -> usize {
        if at == 0 {
            self.first
        } else if at + 1 == self.count as usize {
            self.payload.len()
        } else {
            Self::read(self.table, self.width, at)
        }
    }
    /// Segment `at`, borrowed from the arena rather than from the view.
    #[inline]
    pub(super) fn bytes(&self, at: usize) -> &'a [u8] {
        let start = if at == 0 { 0 } else { self.end(at - 1) };
        &self.payload[start..self.end(at)]
    }
    /// Every segment's bytes.
    pub(super) fn payload(&self) -> &'a [u8] {
        self.payload
    }
}

impl Keys for View<'_> {
    #[inline]
    fn key_count(&self) -> usize {
        self.keys as usize
    }
    #[inline]
    fn segment(&self, at: usize) -> &[u8] {
        self.bytes(at)
    }
}

/// A sorted record's storage: decoded from a run, or viewed in the chunk
/// that stayed in memory.
pub(super) enum Mixed<'a, S> {
    Owned(S),
    View(View<'a>),
}

impl<S: Storage> Keys for Mixed<'_, S> {
    #[inline]
    fn key_count(&self) -> usize {
        match self {
            Self::Owned(storage) => storage.key_count(),
            Self::View(view) => view.key_count(),
        }
    }
    #[inline]
    fn segment(&self, at: usize) -> &[u8] {
        match self {
            Self::Owned(storage) => storage.segment(at),
            Self::View(view) => view.bytes(at),
        }
    }
}

impl<S: Storage> Storage for Mixed<'_, S> {
    const ORIGINAL: bool = S::ORIGINAL;
    #[inline]
    fn original(&self) -> Option<&[u8]> {
        S::ORIGINAL.then(|| self.segment(self.key_count()))
    }
}

/// Per-thread buffers for building language records.
#[derive(Default)]
pub(super) struct LanguageScratch {
    /// Each key's field span within the data.
    pub(super) identities: Vec<Range<usize>>,
    /// The key segments ([`language_segments`]).
    pub(super) bytes: Vec<u8>,
    /// The end of each key segment in `bytes`.
    pub(super) ends: Vec<usize>,
    /// Key texts as they are read.
    pub(super) text: KeyText,
}

/// Buffers for a key text as language sorting reads it.
#[derive(Default)]
pub(super) struct KeyText {
    /// The text case-folded.
    pub(super) folded: String,
    /// The text with glibc's contractions composed.
    pub(super) composed: String,
}

/// Appends a record's language key segments to `out`, ending each in `ends`:
/// per key its collation sort key and then its identity, so that keys glibc's
/// `strcoll` ties leave the order to the next key, as GNU `sort` does.
///
/// Key `at` is sorted by the sort key of `text(at)`, ASCII-uppercased with
/// `fold`. Its identity is `identity(at)`, the key field (ASCII-lowercased
/// with `fold`), which orders the spellings the collator ties as glibc's
/// fourth level does: by code point, except for the letters glibc places
/// otherwise against their decomposition ([`locale::Collator::write_identity`]).
/// Both are read as the sort reads them (uppercased with `fold`) with
/// glibc's contractions composed ([`locale::compose`]), which glibc ties with
/// the letters they spell, so keys spelled so tie on every segment and stay
/// in input order, as in the stable sort GNU runs.
#[allow(clippy::too_many_arguments)]
pub(super) fn language_segments<'a>(
    count: usize,
    mut text: impl FnMut(usize) -> Result<&'a str, Failure>,
    identity: impl Fn(usize) -> &'a [u8],
    fold: bool,
    collator: &locale::Collator,
    sort_keys: &mut SortKeys,
    scratch: &mut KeyText,
    out: &mut Vec<u8>,
    ends: &mut Vec<usize>,
) -> Result<(), Failure> {
    let KeyText { folded, composed } = scratch;
    out.clear();
    ends.clear();
    ends.try_reserve_exact(2 * count)
        .map_err(|_| allocation())?;
    for at in 0..count {
        let original = text(at)?;
        let mut text = original;
        if fold {
            folded.clear();
            folded.try_reserve(text.len()).map_err(|_| allocation())?;
            folded.push_str(text);
            folded.make_ascii_uppercase();
            text = folded.as_str();
        }
        if compose(text, composed)? {
            text = composed.as_str();
        }
        sort_keys.write(collator, text, out)?;
        ends.push(out.len());
        let field = identity(at);
        let start = out.len();
        // The identity is usually the key text, read just so; otherwise it is
        // read as the sort reads it: GNU `sort -f` uppercases.
        let read = if field == original.as_bytes() {
            Some(text)
        } else if field.is_ascii() {
            None
        } else if let Ok(mut written) = std::str::from_utf8(field) {
            if fold {
                folded.clear();
                folded
                    .try_reserve(written.len())
                    .map_err(|_| allocation())?;
                folded.push_str(written);
                folded.make_ascii_uppercase();
                written = folded.as_str();
            }
            if compose(written, composed)? {
                written = composed.as_str();
            }
            Some(written)
        } else {
            None
        };
        let placed = match read {
            Some(read) => collator
                .write_identity(read, out)
                .map_err(|locale::KeyTooLarge| allocation())?,
            None => false,
        };
        if !placed {
            let bytes = read.map_or(field, str::as_bytes);
            out.try_reserve(bytes.len()).map_err(|_| allocation())?;
            out.extend_from_slice(bytes);
        }
        if fold {
            out[start..].make_ascii_lowercase();
        }
        ends.push(out.len());
    }
    Ok(())
}

/// [`locale::compose`], refusing if `out` cannot grow.
fn compose(text: &str, out: &mut String) -> Result<bool, Failure> {
    locale::compose(text, out).map_err(|locale::KeyTooLarge| allocation())
}

/// Short records keep their bytes inline, with one-byte end offsets, so the
/// common small projected record needs no allocation of its own. On 64-bit
/// targets the inline form fits around the heap form's `Vec` capacity niche,
/// so `Segments` stays as small as the heap form alone.
const INLINE: usize = 22;
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<Segments>() == 32);

fn heap_end(bytes: &[u8], at: usize) -> usize {
    let start = at * std::mem::size_of::<usize>();
    usize::from_ne_bytes(
        bytes[start..start + std::mem::size_of::<usize>()]
            .try_into()
            .unwrap(),
    )
}

/// End offsets are encoded as bytes so the allocation requires no alignment or
/// unsafe conversions. Payload bytes start after the complete offset table:
/// one byte per offset inline, `usize` offsets on the heap.
pub(super) enum Segments {
    Inline {
        count: u8,
        len: u8,
        data: [u8; INLINE],
    },
    Heap {
        bytes: Vec<u8>,
        count: usize,
    },
}
impl Segments {
    pub(super) fn new(count: usize, payload: usize) -> Result<Self, Failure> {
        if count
            .checked_add(payload)
            .is_some_and(|size| size <= INLINE)
        {
            return Ok(Self::Inline {
                count: count as u8,
                len: count as u8,
                data: [0; INLINE],
            });
        }
        Self::heap(count, payload)
    }
    /// Heap storage for `count` segments, with room for `payload` bytes.
    pub(super) fn heap(count: usize, payload: usize) -> Result<Self, Failure> {
        let header = count
            .checked_mul(std::mem::size_of::<usize>())
            .ok_or_else(allocation)?;
        let capacity = header.checked_add(payload).ok_or_else(allocation)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| allocation())?;
        bytes.resize(header, 0);
        Ok(Self::Heap { bytes, count })
    }
    /// Storage for `count` segments whose lengths `length` yields in order, with
    /// a zeroed payload for `payload_mut` to fill. Small records stay inline, so
    /// decoding them allocates nothing; a record of more than `INLINE` segments
    /// is always on the heap and takes its ends directly.
    pub(super) fn with_lengths(
        count: usize,
        mut length: impl FnMut() -> Result<usize, Failure>,
    ) -> Result<Self, Failure> {
        let mut segments = if count <= INLINE {
            let mut lengths = [0; INLINE];
            let mut total = 0usize;
            for slot in &mut lengths[..count] {
                *slot = length()?;
                total = total.checked_add(*slot).ok_or_else(allocation)?;
            }
            // `new` checks that the offsets and payload fit, inline or on the heap.
            let mut segments = Self::new(count, total)?;
            let mut end = segments.header();
            for (at, slot) in lengths[..count].iter().enumerate() {
                end += slot;
                segments.set_end(at, end);
            }
            segments
        } else {
            let mut segments = Self::heap(count, 0)?;
            let mut end = segments.header();
            for at in 0..count {
                end = end.checked_add(length()?).ok_or_else(allocation)?;
                segments.set_end(at, end);
            }
            segments
        };
        let end = if segments.count() == 0 {
            segments.header()
        } else {
            segments.end(segments.count() - 1)
        };
        match &mut segments {
            Self::Inline { len, .. } => *len = end as u8,
            Self::Heap { bytes, .. } => {
                bytes
                    .try_reserve_exact(end - bytes.len())
                    .map_err(|_| allocation())?;
                bytes.resize(end, 0);
            }
        }
        Ok(segments)
    }
    /// Every segment's bytes, after the offset table, for filling in place.
    pub(super) fn payload_mut(&mut self) -> &mut [u8] {
        let header = self.header();
        match self {
            Self::Inline { len, data, .. } => &mut data[header..usize::from(*len)],
            Self::Heap { bytes, .. } => &mut bytes[header..],
        }
    }
    pub(super) fn count(&self) -> usize {
        match self {
            Self::Inline { count, .. } => usize::from(*count),
            Self::Heap { count, .. } => *count,
        }
    }
    fn all(&self) -> &[u8] {
        match self {
            Self::Inline { len, data, .. } => &data[..usize::from(*len)],
            Self::Heap { bytes, .. } => bytes,
        }
    }
    pub(super) fn header(&self) -> usize {
        match self {
            Self::Inline { count, .. } => usize::from(*count),
            Self::Heap { count, .. } => count * std::mem::size_of::<usize>(),
        }
    }
    pub(super) fn end(&self, at: usize) -> usize {
        match self {
            Self::Inline { data, .. } => usize::from(data[at]),
            Self::Heap { bytes, .. } => heap_end(bytes, at),
        }
    }
    pub(super) fn set_end(&mut self, at: usize, end: usize) {
        match self {
            Self::Inline { data, .. } => data[at] = end as u8,
            Self::Heap { bytes, .. } => {
                let start = at * std::mem::size_of::<usize>();
                bytes[start..start + std::mem::size_of::<usize>()]
                    .copy_from_slice(&end.to_ne_bytes());
            }
        }
    }
    pub(super) fn segment(&self, at: usize) -> &[u8] {
        // One dispatch per call: this is the sort comparison's hot path.
        match self {
            Self::Inline { count, data, .. } => {
                let start = if at == 0 {
                    usize::from(*count)
                } else {
                    usize::from(data[at - 1])
                };
                &data[start..usize::from(data[at])]
            }
            Self::Heap { bytes, count } => {
                let start = if at == 0 {
                    count * std::mem::size_of::<usize>()
                } else {
                    heap_end(bytes, at - 1)
                };
                &bytes[start..heap_end(bytes, at)]
            }
        }
    }
    /// Every segment's bytes, after the offset table.
    pub(super) fn payload(&self) -> &[u8] {
        &self.all()[self.header()..]
    }
    /// Appends segment `at`; inline records were sized for it by `new`.
    #[cfg(test)]
    fn append(&mut self, at: usize, value: &[u8], fold: bool) {
        let start = self.all().len();
        let end = start + value.len();
        let bytes = match self {
            Self::Inline { len, data, .. } => {
                debug_assert!(end <= INLINE, "inline record sized too small");
                data[start..end].copy_from_slice(value);
                *len = end as u8;
                &mut data[start..end]
            }
            Self::Heap { bytes, .. } => {
                bytes.extend_from_slice(value);
                &mut bytes[start..]
            }
        };
        if fold {
            bytes.make_ascii_uppercase();
        }
        self.set_end(at, end);
    }
}

pub(super) struct Packed {
    pub(super) segments: Segments,
    pub(super) keys: usize,
}
impl Keys for Packed {
    fn key_count(&self) -> usize {
        self.keys
    }
    fn segment(&self, at: usize) -> &[u8] {
        self.segments.segment(at)
    }
}
impl Storage for Packed {
    const ORIGINAL: bool = false;
    fn original(&self) -> Option<&[u8]> {
        None
    }
}
#[cfg(test)]
impl Build for Packed {
    fn build(
        raw: &[u8],
        _: &[u8],
        spans: &[Range<usize>],
        keys: usize,
        fold: bool,
    ) -> Result<Self, Failure> {
        let length = spans.iter().try_fold(0usize, |n, span| {
            n.checked_add(span.len()).ok_or_else(allocation)
        })?;
        let mut segments = Segments::new(spans.len(), length)?;
        for (at, span) in spans.iter().enumerate() {
            segments.append(at, &raw[span.clone()], fold && at < keys);
        }
        Ok(Self { segments, keys })
    }
}

pub(super) struct Original {
    pub(super) segments: Segments,
}
impl Keys for Original {
    fn key_count(&self) -> usize {
        self.segments.count() - 1
    }
    fn segment(&self, at: usize) -> &[u8] {
        self.segments.segment(at)
    }
}
impl Storage for Original {
    const ORIGINAL: bool = true;
    fn original(&self) -> Option<&[u8]> {
        Some(self.segment(self.key_count()))
    }
}
#[cfg(test)]
impl Build for Original {
    fn build(
        raw: &[u8],
        data: &[u8],
        spans: &[Range<usize>],
        keys: usize,
        fold: bool,
    ) -> Result<Self, Failure> {
        let length = spans[..keys].iter().try_fold(data.len(), |n, span| {
            n.checked_add(span.len()).ok_or_else(allocation)
        })?;
        let mut segments = Segments::new(keys.checked_add(1).ok_or_else(allocation)?, length)?;
        for (at, span) in spans[..keys].iter().enumerate() {
            segments.append(at, &raw[span.clone()], fold);
        }
        segments.append(keys, data, false);
        Ok(Self { segments })
    }
    fn language(keys: &[u8], ends: &[usize], original: &[u8]) -> Result<Self, Failure> {
        let count = ends.len().checked_add(1).ok_or_else(allocation)?;
        let length = keys
            .len()
            .checked_add(original.len())
            .ok_or_else(allocation)?;
        let mut segments = Segments::new(count, length)?;
        let mut start = 0;
        for (at, &end) in ends.iter().enumerate() {
            segments.append(at, &keys[start..end], false);
            start = end;
        }
        segments.append(ends.len(), original, false);
        Ok(Self { segments })
    }
}

#[cfg(test)]
// One-element span lists are single-field projections, not whole ranges.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    #[test]
    fn a_sort_key_that_cannot_grow_refuses_and_otherwise_is_the_collators() {
        let mut policy = crate::locale::Policy::default();
        policy.collation = crate::locale::Collation::Language("de-DE");
        let collator = policy.collator().ok().unwrap().unwrap();
        let text = "\u{d6}lfeld und Stra\u{df}e, ".repeat(64);
        let mut expected = Vec::new();
        assert!(collator.write_key(&text, &mut expected).is_ok());
        let mut out = Vec::new();
        assert!(sort_key(&collator, &text, &mut out).is_ok());
        assert_eq!(out, expected);
        // Any write of the key can fail to grow the buffer; each refuses.
        for fail_at in 0..4 {
            crate::locale::KEY_WRITES_BEFORE_FAILURE.with(|count| count.set(Some(fail_at)));
            let mut out = Vec::new();
            let error = sort_key(&collator, &text, &mut out).err().unwrap();
            assert_eq!(error.message, allocation().message, "{fail_at}");
        }
        crate::locale::KEY_WRITES_BEFORE_FAILURE.with(|count| count.set(None));
    }

    #[test]
    fn a_sort_key_that_outgrows_its_room_is_the_collators_and_any_write_can_refuse() {
        let mut policy = crate::locale::Policy::default();
        policy.collation = crate::locale::Collation::Language("de-DE");
        let collator = policy.collator().ok().unwrap().unwrap();
        // An expanding ligature: far more than three key bytes per text byte.
        let text = "\u{fdfa}".repeat(8);
        let mut expected = Vec::new();
        assert!(collator.write_key(&text, &mut expected).is_ok());
        assert!(expected.len() > 3 * text.len() + 16, "{}", expected.len());
        // Appended after what the buffer holds, as a projection does.
        let mut out = b"held".to_vec();
        assert!(sort_key(&collator, &text, &mut out).is_ok());
        assert_eq!(out[..4], *b"held");
        assert_eq!(out[4..], expected);
        // Count the key's writes, then fail the first, a middle and the last.
        crate::locale::KEY_WRITES_BEFORE_FAILURE.with(|count| count.set(Some(usize::MAX)));
        assert!(sort_key(&collator, &text, &mut Vec::new()).is_ok());
        let writes = usize::MAX
            - crate::locale::KEY_WRITES_BEFORE_FAILURE.with(|count| count.get().unwrap());
        for fail_at in [0, writes / 2, writes - 1] {
            crate::locale::KEY_WRITES_BEFORE_FAILURE.with(|count| count.set(Some(fail_at)));
            let error = sort_key(&collator, &text, &mut Vec::new()).err().unwrap();
            assert_eq!(error.message, allocation().message, "{fail_at}");
        }
        crate::locale::KEY_WRITES_BEFORE_FAILURE.with(|count| count.set(None));
    }

    #[test]
    fn cached_sort_keys_equal_direct_keys() {
        let long = "k".repeat(SortKeys::LONGEST + 1);
        let mut texts: Vec<String> = (0..300).map(|n| format!("k{n:03}")).collect();
        // Precomposed and decomposed spellings of the same letter, and repeats.
        let extra = [
            "",
            "a-b",
            "A",
            "k001",
            "k001",
            "\u{d6}lfeld",
            "O\u{308}lfeld",
            &long,
            &long,
        ];
        texts.extend(extra.map(String::from));
        for collation in [
            crate::locale::Collation::Language("en-US"),
            crate::locale::Collation::Language("de-DE"),
        ] {
            let mut policy = crate::locale::Policy::default();
            policy.collation = collation;
            let collator = policy.collator().ok().unwrap().unwrap();
            let mut cache = SortKeys::default();
            for round in 0..2 {
                for text in &texts {
                    let mut direct = Vec::new();
                    assert!(collator.write_key(text, &mut direct).is_ok());
                    let mut cached = b"prefix".to_vec();
                    cache.write(&collator, text, &mut cached).ok().unwrap();
                    assert_eq!(&cached[6..], direct, "{text:?} round {round}");
                }
            }
        }
    }

    fn heap_copy(packed: &Packed) -> Packed {
        let count = packed.segments.count();
        let mut segments = Segments::heap(count, packed.segments.payload().len())
            .ok()
            .unwrap();
        for at in 0..count {
            segments.append(at, packed.segment(at), false);
        }
        Packed {
            segments,
            keys: packed.keys,
        }
    }

    /// Records of every count and payload size around the inline limit read
    /// and compare exactly like their heap equivalents.
    #[test]
    fn inline_and_heap_segments_are_interchangeable() {
        let mut inline = 0;
        for count in 1..=INLINE + 1 {
            for payload in 0..=INLINE + 2 {
                let raw: Vec<u8> = (0..payload as u8).map(|b| b'a' + b % 26).collect();
                let cut = |at: usize| payload * at / count;
                let spans: Vec<_> = (0..count).map(|at| cut(at)..cut(at + 1)).collect();
                let keys = count.min(2);
                let packed = Packed::build(&raw, &raw, &spans, keys, true).ok().unwrap();
                let fits = count + payload <= INLINE;
                inline += usize::from(fits);
                assert_eq!(matches!(packed.segments, Segments::Inline { .. }), fits);
                for (at, span) in spans.iter().enumerate() {
                    let expected = &raw[span.clone()];
                    if at < keys {
                        assert_eq!(packed.segment(at), expected.to_ascii_uppercase());
                    } else {
                        assert_eq!(packed.segment(at), expected);
                    }
                }
                let heap = heap_copy(&packed);
                assert_eq!(packed.compare_keys(&heap), Ordering::Equal);
                assert_eq!(packed.segments.payload(), heap.segments.payload());
            }
        }
        // Most small counts and payloads fit inline; guard against a zero limit.
        assert!(inline > 200, "{inline}");
    }

    #[test]
    fn inline_and_heap_records_order_by_keys_then_key_count() {
        let build = |raw: &[u8], spans: &[Range<usize>], keys| {
            Packed::build(raw, raw, spans, keys, false).ok().unwrap()
        };
        let a = build(b"ab", &[0..1, 1..2], 1);
        let b = build(b"b", &[0..1], 1);
        let long = build(&[b'a'; 40], &[0..1, 1..40], 1);
        let two_keys = build(b"ab", &[0..1, 1..2], 2);
        assert!(matches!(long.segments, Segments::Heap { .. }));
        assert_eq!(a.compare_keys(&b), Ordering::Less);
        assert_eq!(heap_copy(&b).compare_keys(&a), Ordering::Greater);
        assert_eq!(a.compare_keys(&long), Ordering::Equal);
        assert_eq!(a.compare_keys(&two_keys), Ordering::Less);
        assert_eq!(heap_copy(&two_keys).compare_keys(&a), Ordering::Greater);
    }
}
