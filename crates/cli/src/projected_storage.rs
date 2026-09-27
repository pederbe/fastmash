//! Owned segments with byte-encoded offsets and reusable projection ranges.
use super::{Failure, allocation};
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

    /// Append the sort key of `text` to `out`.
    fn write(
        &mut self,
        collator: &icu_collator::CollatorBorrowed<'_>,
        text: &str,
        out: &mut Vec<u8>,
    ) {
        if text.len() > Self::LONGEST {
            collator.write_sort_key_to(text, out).unwrap();
            return;
        }
        if self.slots.is_empty() && self.slots.try_reserve_exact(Self::SLOTS).is_ok() {
            self.slots.resize_with(Self::SLOTS, Default::default);
        }
        // FNV-1a picks the slot; any hash is correct, since hits compare bytes.
        let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        });
        let Some(slot) = self.slots.get_mut(hash as usize % Self::SLOTS) else {
            collator.write_sort_key_to(text, out).unwrap();
            return;
        };
        let (cached, key) = slot;
        // An empty slot holds no key: every sort key has at least one byte.
        if !key.is_empty() && cached == text.as_bytes() {
            out.extend_from_slice(key);
            return;
        }
        let start = out.len();
        collator.write_sort_key_to(text, out).unwrap();
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
    }
}

pub(super) trait Storage: Sized {
    const ORIGINAL: bool;
    fn build(
        raw: &[u8],
        data: &[u8],
        spans: &[Range<usize>],
        keys: usize,
        fold: bool,
    ) -> Result<Self, Failure>;
    fn key_count(&self) -> usize;
    fn segment(&self, at: usize) -> &[u8];
    fn original(&self) -> Option<&[u8]>;
    fn compare_keys(&self, other: &Self) -> Ordering;
    fn owned_bytes(&self) -> Result<usize, Failure>;
    fn language_keys(
        &mut self,
        _: &icu_collator::CollatorBorrowed<'_>,
        _: &mut SortKeys,
        _: Vec<Vec<u8>>,
    ) -> Result<(), Failure> {
        Err(super::unsupported(
            "internal language sorter requires original records",
        ))
    }
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
    /// Heap storage, whose bytes `heap_bytes_mut` exposes for incremental filling.
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
    /// Heap storage from bytes already holding `count` offsets and payload.
    pub(super) fn from_heap(bytes: Vec<u8>, count: usize) -> Self {
        Self::Heap { bytes, count }
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
    /// Heap bytes owned beyond the record itself.
    pub(super) fn capacity(&self) -> usize {
        match self {
            Self::Inline { .. } => 0,
            Self::Heap { bytes, .. } => bytes.capacity(),
        }
    }
    /// The heap bytes, moving inline storage to the heap first.
    pub(super) fn heap_bytes_mut(&mut self) -> Result<&mut Vec<u8>, Failure> {
        if let Self::Inline { .. } = self {
            let mut heap = Self::heap(self.count(), self.payload().len())?;
            let shift = heap.header() - self.header();
            for at in 0..self.count() {
                heap.set_end(at, self.end(at) + shift);
            }
            if let Self::Heap { bytes, .. } = &mut heap {
                bytes.extend_from_slice(self.payload());
            }
            *self = heap;
        }
        match self {
            Self::Heap { bytes, .. } => Ok(bytes),
            Self::Inline { .. } => unreachable!("moved to the heap above"),
        }
    }
    pub(super) fn into_vec(self) -> Vec<u8> {
        match self {
            Self::Inline { len, data, .. } => data[..usize::from(len)].to_vec(),
            Self::Heap { bytes, .. } => bytes,
        }
    }
    /// Appends segment `at`; inline records were sized for it by `new`.
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
    fn compare(&self, other: &Self, keys: usize, other_keys: usize) -> Ordering {
        for at in 0..keys.min(other_keys) {
            let order = self.segment(at).cmp(other.segment(at));
            if order != Ordering::Equal {
                return order;
            }
        }
        keys.cmp(&other_keys)
    }
}

pub(super) struct Packed {
    pub(super) segments: Segments,
    pub(super) keys: usize,
}
impl Storage for Packed {
    const ORIGINAL: bool = false;
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
    fn key_count(&self) -> usize {
        self.keys
    }
    fn segment(&self, at: usize) -> &[u8] {
        self.segments.segment(at)
    }
    fn original(&self) -> Option<&[u8]> {
        None
    }
    fn compare_keys(&self, other: &Self) -> Ordering {
        self.segments
            .compare(&other.segments, self.keys, other.keys)
    }
    fn owned_bytes(&self) -> Result<usize, Failure> {
        Ok(self.segments.capacity())
    }
}

pub(super) struct Original {
    pub(super) segments: Segments,
}
impl Original {
    pub(super) fn replace_original(&mut self, bytes: &[u8]) -> Result<(), Failure> {
        // Keep the key segments and replace the original in heap bytes.
        let keys = if self.key_count() == 0 {
            0
        } else {
            self.segments.end(self.key_count() - 1) - self.segments.header()
        };
        let header = self.segments.count() * std::mem::size_of::<usize>();
        let heap = self.segments.heap_bytes_mut()?;
        heap.truncate(header + keys);
        heap.try_reserve_exact(bytes.len())
            .map_err(|_| allocation())?;
        self.segments.append(self.key_count(), bytes, false);
        Ok(())
    }
    pub(super) fn into_original(self) -> Vec<u8> {
        let at = self.key_count();
        let start = if at == 0 {
            self.segments.header()
        } else {
            self.segments.end(at - 1)
        };
        let end = self.segments.end(at);
        let mut bytes = self.segments.into_vec();
        bytes.copy_within(start..end, 0);
        bytes.truncate(end - start);
        bytes
    }
}
impl Storage for Original {
    const ORIGINAL: bool = true;
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
    fn language_keys(
        &mut self,
        collator: &icu_collator::CollatorBorrowed<'_>,
        sort_keys: &mut SortKeys,
        identity: Vec<Vec<u8>>,
    ) -> Result<(), Failure> {
        let mut keys = Vec::new();
        keys.try_reserve_exact(
            self.key_count()
                .checked_add(identity.len())
                .ok_or_else(allocation)?,
        )
        .map_err(|_| allocation())?;
        for at in 0..self.key_count() {
            let text = super::locale::text(self.segment(at))?;
            let mut key = Vec::new();
            sort_keys.write(collator, text, &mut key);
            keys.push(key);
        }
        keys.extend(identity);
        let raw = self.original().unwrap();
        let length = keys.iter().try_fold(raw.len(), |n, key| {
            n.checked_add(key.len()).ok_or_else(allocation)
        })?;
        let mut segments =
            Segments::new(keys.len().checked_add(1).ok_or_else(allocation)?, length)?;
        for (at, key) in keys.iter().enumerate() {
            segments.append(at, key, false);
        }
        segments.append(keys.len(), raw, false);
        self.segments = segments;
        Ok(())
    }
    fn key_count(&self) -> usize {
        self.segments.count() - 1
    }
    fn segment(&self, at: usize) -> &[u8] {
        self.segments.segment(at)
    }
    fn original(&self) -> Option<&[u8]> {
        Some(self.segment(self.key_count()))
    }
    fn compare_keys(&self, other: &Self) -> Ordering {
        self.segments
            .compare(&other.segments, self.key_count(), other.key_count())
    }
    fn owned_bytes(&self) -> Result<usize, Failure> {
        Ok(self.segments.capacity())
    }
}

#[cfg(test)]
// One-element span lists are single-field projections, not whole ranges.
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

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
                    collator
                        .write_sort_key_to(text.as_str(), &mut direct)
                        .unwrap();
                    let mut cached = b"prefix".to_vec();
                    cache.write(&collator, text, &mut cached);
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

    /// Records of every count and payload size around the inline limit read,
    /// compare and move to the heap exactly like their heap equivalents.
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
                assert_eq!(packed.owned_bytes().ok().unwrap() == 0, fits);
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
                let mut moved = Packed::build(&raw, &raw, &spans, keys, true).ok().unwrap();
                moved.segments.heap_bytes_mut().ok().unwrap();
                assert!(matches!(moved.segments, Segments::Heap { .. }));
                for at in 0..count {
                    assert_eq!(moved.segment(at), packed.segment(at));
                }
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

    #[test]
    fn inline_originals_replace_and_extract_with_keys() {
        let mut record = Original::build(b"k	v", b"k	v", &[0..1], 1, true)
            .ok()
            .unwrap();
        assert_eq!(record.owned_bytes().ok().unwrap(), 0);
        assert_eq!(record.segment(0), b"K");
        record
            .replace_original(b"k	v # a longer note")
            .ok()
            .unwrap();
        assert!(record.owned_bytes().ok().unwrap() > 0);
        assert_eq!(record.segment(0), b"K");
        assert_eq!(record.original(), Some(&b"k	v # a longer note"[..]));
        assert_eq!(record.into_original(), b"k	v # a longer note");
        let inline = Original::build(b"k	v", b"k	v", &[0..1], 1, false)
            .ok()
            .unwrap();
        assert_eq!(inline.into_original(), b"k	v");
    }
}
