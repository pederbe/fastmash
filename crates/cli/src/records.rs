use fastmash_conversion::field_policy::FieldRange;
use std::io::{self, BufRead, Write};

pub enum ReadError {
    Allocation,
    Io(io::Error),
}

pub fn is_comment(bytes: &[u8]) -> bool {
    matches!(
        bytes.iter().find(|&&b| !matches!(b, b' ' | b'\t')),
        Some(b'#' | b';')
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Separator {
    Literal(u8),
    Whitespace,
}

pub struct Fields<'a> {
    bytes: &'a [u8],
    separator: Separator,
    position: usize,
    finished: bool,
}

pub fn fields(bytes: &[u8], separator: Separator) -> Fields<'_> {
    Fields {
        bytes,
        separator,
        position: 0,
        finished: bytes.is_empty(),
    }
}

impl Iterator for Fields<'_> {
    type Item = FieldRange;

    fn next(&mut self) -> Option<Self::Item> {
        if self.finished {
            return None;
        }
        let start;
        let end;
        match self.separator {
            Separator::Literal(delimiter) => {
                start = self.position;
                end = self.bytes[start..]
                    .iter()
                    .position(|&b| b == delimiter)
                    .map_or(self.bytes.len(), |offset| start + offset);
                self.finished = end == self.bytes.len();
                self.position = end + usize::from(!self.finished);
            }
            Separator::Whitespace => {
                // C/POSIX isblank includes space and tab, not every ASCII space.
                while self
                    .bytes
                    .get(self.position)
                    .is_some_and(|b| matches!(b, b' ' | b'\t'))
                {
                    self.position += 1;
                }
                start = self.position;
                while self
                    .bytes
                    .get(self.position)
                    .is_some_and(|b| !matches!(b, b' ' | b'\t'))
                {
                    self.position += 1;
                }
                end = self.position;
                self.finished = end == self.bytes.len();
                // A later call skips trailing blanks and yields one empty field.
            }
        }
        Some(FieldRange {
            start,
            length: end - start,
        })
    }
}

#[cfg(test)]
pub fn read_record(
    reader: &mut (impl BufRead + ?Sized),
    record: &mut Vec<u8>,
) -> Result<bool, ReadError> {
    read_record_terminated(reader, record, b'\n')
}

/// Reads one Record into `record`, without its terminator; false at the end of
/// input. A read error discards a partial Record.
pub fn read_record_terminated(
    reader: &mut (impl BufRead + ?Sized),
    record: &mut Vec<u8>,
    terminator: u8,
) -> Result<bool, ReadError> {
    record.clear();
    loop {
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(error) => {
                record.clear();
                return Err(ReadError::Io(error));
            }
        };
        if available.is_empty() {
            return Ok(!record.is_empty());
        }
        let boundary = memchr::memchr(terminator, available);
        let length = boundary.unwrap_or(available.len());
        record
            .try_reserve(length)
            .map_err(|_| ReadError::Allocation)?;
        record.extend_from_slice(&available[..length]);
        reader.consume(length + usize::from(boundary.is_some()));
        if boundary.is_some() {
            return Ok(true);
        }
    }
}

/// Records shorter than this are searched without vectorized separator scanning.
const SHORT_RECORD: usize = 64;

/// The wanted field given the positions of every separator, or the field count.
fn field_at_separators(
    bytes: &[u8],
    wanted: u64,
    separators: impl Iterator<Item = usize>,
) -> Result<FieldRange, u64> {
    if bytes.is_empty() {
        return Err(0);
    }
    let (mut start, mut count) = (0, 1);
    for end in separators {
        if count == wanted {
            return Ok(FieldRange {
                start,
                length: end - start,
            });
        }
        (start, count) = (end + 1, count + 1);
    }
    if count == wanted {
        Ok(FieldRange {
            start,
            length: bytes.len() - start,
        })
    } else {
        Err(count)
    }
}

/// Locate one field, counting the whole record only for a missing-field diagnostic.
pub fn field(bytes: &[u8], wanted: u64, separator: Separator) -> Result<FieldRange, u64> {
    if let Separator::Literal(delimiter) = separator {
        // One pass over the separators, equivalent to `fields`. Short records
        // are scanned directly; a vectorized search only pays off on long ones.
        return if bytes.len() < SHORT_RECORD {
            field_at_separators(
                bytes,
                wanted,
                (0..bytes.len()).filter(|&at| bytes[at] == delimiter),
            )
        } else {
            field_at_separators(bytes, wanted, memchr::memchr_iter(delimiter, bytes))
        };
    }
    let mut count = 0;
    for span in fields(bytes, separator) {
        count += 1;
        if count == wanted {
            return Ok(span);
        }
    }
    Err(count)
}

/// How far into a record, on average, the fields before the furthest selected
/// one must lie for keeping spans to pay (see [`keeps_spans`]).
const RESCANNED_FIELDS: u64 = 4;

/// Whether lookups of `selected` (ascending and distinct) keep spans (see
/// [`FieldIndex`]). Scanning to each field rescans the fields before it; one
/// pass saves that, but keeping spans costs each record its own upkeep. It
/// pays with more than two fields when the fields before the furthest lie on
/// average at least [`RESCANNED_FIELDS`] fields into the record: not for
/// fields 1-3, 1-6 or 1, 4 and 8, but for 1-8, 6-8 or 1, 500 and 1000.
fn keeps_spans(selected: &[u64]) -> bool {
    let [before @ .., _] = selected else {
        return false;
    };
    before.len() >= 2
        && before
            .iter()
            .fold(0u64, |sum, &field| sum.saturating_add(field))
            >= RESCANNED_FIELDS.saturating_mul(before.len() as u64)
}

/// Locates the selected fields of one record. When it keeps spans
/// ([`keeps_spans`]), a record is scanned at most once, no further than the
/// furthest field requested unless fewer than `SHORT_RECORD` bytes remain,
/// keeping the spans of the selected fields it passes: selecting k fields of
/// a w-field record costs O(w + k) time rather than O(k·w), and O(k) memory.
/// Otherwise, and for a field not selected, each lookup scans from the
/// record's start as [`field`] does. Results equal [`field`]'s either way.
#[derive(Debug, Default)]
pub struct FieldIndex(Option<Box<[Kept; 1]>>);

/// The state of a [`FieldIndex`] that keeps spans, behind one pointer so
/// that records carrying an index stay small to move.
#[derive(Debug)]
struct Kept {
    /// The selected fields, ascending and distinct.
    selected: Vec<u64>,
    /// The spans of the first `spans.len()` selected fields.
    spans: Vec<FieldRange>,
    /// The fields scanned so far.
    scanned: u64,
    /// Where the scan continues.
    position: usize,
    /// Whether the scan reached the record's end: `scanned` is its field count.
    complete: bool,
    /// The slot after the last one looked up, tried first.
    next: usize,
}

impl FieldIndex {
    /// An index for lookups of `fields` (in any order, with repeats), which
    /// keeps spans when [`keeps_spans`] and memory allow.
    pub fn selecting(fields: impl Iterator<Item = u64> + Clone) -> Self {
        match Self::distinct(fields) {
            Some(selected) if keeps_spans(&selected) => Self::keeping(selected),
            _ => Self::default(),
        }
    }

    /// An index that keeps the spans of `fields` whatever [`keeps_spans`]
    /// says, for tests of kept spans on any fields.
    #[cfg(test)]
    pub fn always_keeping(fields: impl Iterator<Item = u64> + Clone) -> Self {
        Self::keeping(Self::distinct(fields).unwrap())
    }

    /// `fields` ascending and distinct, or None without memory for them.
    fn distinct(fields: impl Iterator<Item = u64> + Clone) -> Option<Vec<u64>> {
        let mut selected = Vec::new();
        selected.try_reserve_exact(fields.clone().count()).ok()?;
        // No record has field 0 (an unresolved name).
        selected.extend(fields.filter(|&field| field != 0));
        selected.sort_unstable();
        selected.dedup();
        Some(selected)
    }

    /// An index for another record, selecting the same fields.
    pub fn fresh(&self) -> Self {
        let Some(kept) = &self.0 else {
            return Self::default();
        };
        let mut selected = Vec::new();
        if selected.try_reserve_exact(kept[0].selected.len()).is_err() {
            return Self::default();
        }
        selected.extend_from_slice(&kept[0].selected);
        Self::keeping(selected)
    }

    /// An index keeping the spans of `selected` (ascending and distinct), or
    /// scanning directly without memory for its state.
    fn keeping(selected: Vec<u64>) -> Self {
        let mut kept = Vec::new();
        if kept.try_reserve_exact(1).is_err() {
            return Self::default();
        }
        kept.push(Kept {
            selected,
            spans: Vec::new(),
            scanned: 0,
            position: 0,
            complete: false,
            next: 0,
        });
        Self(kept.into_boxed_slice().try_into().ok())
    }

    /// Forgets the spans, for the next record.
    #[inline]
    pub fn clear(&mut self) {
        if let Some(kept) = &mut self.0 {
            kept[0].clear();
        }
    }

    /// Field `wanted` of `bytes`, or the record's field count when it has no
    /// such field. Every lookup since the last `clear` must pass the same
    /// record and separator.
    #[inline]
    pub fn field(
        &mut self,
        bytes: &[u8],
        wanted: u64,
        separator: Separator,
    ) -> Result<FieldRange, u64> {
        match &mut self.0 {
            None => field(bytes, wanted, separator),
            Some(kept) => kept[0].field(bytes, wanted, separator),
        }
    }
}

impl Kept {
    #[inline]
    fn clear(&mut self) {
        self.spans.clear();
        self.scanned = 0;
        self.position = 0;
        self.complete = false;
        self.next = 0;
    }

    // Out of line, so that callers scanning directly stay small.
    #[inline(never)]
    fn field(
        &mut self,
        bytes: &[u8],
        wanted: u64,
        separator: Separator,
    ) -> Result<FieldRange, u64> {
        // Lookups mostly follow field order: the slot after the last one.
        let slot = if self.selected.get(self.next) == Some(&wanted) {
            self.next
        } else {
            match self.selected.binary_search(&wanted) {
                Ok(slot) => slot,
                Err(_) => return field(bytes, wanted, separator),
            }
        };
        self.next = slot + 1;
        match self.spans.get(slot) {
            Some(span) => Ok(*span),
            None => self.extend(bytes, slot, separator),
        }
    }

    /// Scans on to the selected field at `slot`, or to the record's end.
    fn extend(
        &mut self,
        bytes: &[u8],
        slot: usize,
        separator: Separator,
    ) -> Result<FieldRange, u64> {
        if !self.complete {
            // The rest of a short record is scanned at once for every selected
            // field in it, rather than once per lookup.
            let start = self.position;
            let short = bytes.len() - start < SHORT_RECORD;
            let last = if short { self.selected.len() - 1 } else { slot };
            let kept = if bytes.is_empty() {
                self.complete = true;
                true
            } else {
                match separator {
                    Separator::Literal(delimiter) if short => self.scan_separators(
                        bytes,
                        last,
                        (start..bytes.len()).filter(|&at| bytes[at] == delimiter),
                    ),
                    Separator::Literal(delimiter) => self.scan_separators(
                        bytes,
                        last,
                        memchr::memchr_iter(delimiter, &bytes[start..]).map(|at| start + at),
                    ),
                    Separator::Whitespace => self.scan_blanks(bytes, last),
                }
            };
            if !kept {
                // Without memory for another span, forget them and scan directly.
                let wanted = self.selected[slot];
                self.clear();
                return field(bytes, wanted, separator);
            }
        }
        match self.spans.get(slot) {
            Some(span) => Ok(*span),
            None => Err(self.scanned),
        }
    }

    /// Scans the fields ending at `separators` (positions from the scan's
    /// position on), then the record's last field, until the selected field
    /// at `slot` is kept. False when a span could not be kept.
    fn scan_separators(
        &mut self,
        bytes: &[u8],
        slot: usize,
        separators: impl Iterator<Item = usize>,
    ) -> bool {
        let mut start = self.position;
        for end in separators {
            if !self.pass(start, end) {
                return false;
            }
            start = end + 1;
            if self.spans.len() > slot {
                self.position = start;
                return true;
            }
        }
        if !self.pass(start, bytes.len()) {
            return false;
        }
        (self.position, self.complete) = (bytes.len(), true);
        true
    }

    /// Scans blank-separated fields until the selected field at `slot` is
    /// kept. False when a span could not be kept.
    fn scan_blanks(&mut self, bytes: &[u8], slot: usize) -> bool {
        let mut fields = Fields {
            bytes,
            separator: Separator::Whitespace,
            position: self.position,
            finished: false,
        };
        while let Some(span) = fields.next() {
            if !self.pass(span.start, span.start + span.length) {
                return false;
            }
            if self.spans.len() > slot {
                (self.position, self.complete) = (fields.position, fields.finished);
                return true;
            }
        }
        (self.position, self.complete) = (fields.position, true);
        true
    }

    /// Counts the field at `start..end`, keeping its span when it is the next
    /// selected field. False without memory for the span.
    #[inline]
    fn pass(&mut self, start: usize, end: usize) -> bool {
        self.scanned += 1;
        if self.selected.get(self.spans.len()) != Some(&self.scanned) {
            return true;
        }
        if self.spans.len() == self.spans.capacity() && self.spans.try_reserve(1).is_err() {
            return false;
        }
        self.spans.push(FieldRange {
            start,
            length: end - start,
        });
        true
    }
}

pub fn write_output(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    writer.write_all(bytes)?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_a_field_agrees_with_iterating_fields() {
        // The long record puts separators around vector-width boundaries.
        let mut long = vec![b'a'; 100];
        for at in [15, 16, 31, 32, 64, 99] {
            long[at] = b'\t';
        }
        let records: [&[u8]; 13] = [
            b"",
            b"a",
            b"\t",
            b"a\t",
            b"\ta",
            b"a\t\tb",
            b"ab\tc\td\t",
            b"\t\t\t",
            b"x,y\tz",
            b"\xff\xffa\xff",
            &long,
            &long[..63],
            &long[..64],
        ];
        for record in records {
            for separator in [b'\t', b',', 0xff].map(Separator::Literal) {
                let all: Vec<_> = fields(record, separator).collect();
                for wanted in 0..=all.len() as u64 + 1 {
                    let expected = match wanted {
                        1.. if wanted <= all.len() as u64 => Ok(all[wanted as usize - 1]),
                        _ => Err(all.len() as u64),
                    };
                    assert_eq!(
                        field(record, wanted, separator),
                        expected,
                        "{record:?} {wanted}"
                    );
                }
            }
        }
    }
    #[test]
    fn long_record_search_preserves_every_byte_across_buffer_boundaries() {
        for terminator in [0, b'\n', 0xff] {
            let fill = if terminator == 0 { b'\n' } else { 0 };
            let rows: Vec<_> = [0, 15, 16, 17, 31, 32, 33, 63, 64, 65, 4097]
                .into_iter()
                .map(|length| vec![fill; length])
                .collect();
            let mut input = Vec::new();
            for row in &rows {
                input.extend_from_slice(row);
                input.push(terminator);
            }
            input.extend_from_slice(b"tail");
            for capacity in [1, 7, 16, 31, 64, 4096, 8192] {
                let mut reader = io::BufReader::with_capacity(capacity, io::Cursor::new(&input));
                let mut record = Vec::new();
                for expected in rows.iter().map(Vec::as_slice).chain([b"tail".as_slice()]) {
                    assert!(matches!(
                        read_record_terminated(&mut reader, &mut record, terminator),
                        Ok(true)
                    ));
                    assert_eq!(record, expected);
                }
                assert!(matches!(
                    read_record_terminated(&mut reader, &mut record, terminator),
                    Ok(false)
                ));
            }
        }
    }

    #[test]
    fn lookup_matches_full_split_and_counts_only_missing_requests() {
        let alphabet = [b'a', b',', b'\t', 0, 0xff];
        for length in 0..=6 {
            for mut code in 0..5usize.pow(length) {
                let mut bytes = vec![0; length as usize];
                for byte in &mut bytes {
                    *byte = alphabet[code % alphabet.len()];
                    code /= alphabet.len();
                }
                for delimiter in [b',', b'\t', 0, 0xff] {
                    let expected: Vec<_> = if bytes.is_empty() {
                        Vec::new()
                    } else {
                        bytes.split(|&b| b == delimiter).collect()
                    };
                    for wanted in (0..=expected.len() as u64 + 1).chain([u64::MAX]) {
                        let actual = field(&bytes, wanted, Separator::Literal(delimiter))
                            .map(|span| &bytes[span.start..span.start + span.length]);
                        let selected = wanted
                            .checked_sub(1)
                            .and_then(|at| usize::try_from(at).ok())
                            .and_then(|at| expected.get(at))
                            .copied();
                        assert_eq!(actual, selected.ok_or(expected.len() as u64));
                    }
                }
            }
        }
        let wide = [b"early,".as_slice(), &vec![b'x'; 1_000_000], b",,"].concat();
        assert_eq!(field(&wide, 1, Separator::Literal(b',')).unwrap().length, 5);
        assert_eq!(field(&wide, 5, Separator::Literal(b',')).unwrap_err(), 4);
        let many = b"x,".repeat(100_000);
        assert_eq!(
            field(&many, 100_001, Separator::Literal(b','))
                .unwrap()
                .length,
            0
        );
        assert_eq!(
            field(&many, u64::MAX, Separator::Literal(b',')).unwrap_err(),
            100_001
        );
    }

    #[test]
    fn kept_spans_answer_every_lookup_as_direct_scans_do() {
        let alphabet = [b'a', b',', b'\t', b' ', 0xff];
        let mut records: Vec<Vec<u8>> = Vec::new();
        for length in 0..=5 {
            for mut code in 0..5usize.pow(length) {
                let mut bytes = vec![0; length as usize];
                for byte in &mut bytes {
                    *byte = alphabet[code % alphabet.len()];
                    code /= alphabet.len();
                }
                records.push(bytes);
            }
        }
        // Long records put separators around the short-record and vector
        // boundaries, with fields and blank runs of every length.
        let mut long = vec![b'x'; 300];
        for at in [
            0, 1, 15, 16, 31, 62, 63, 64, 65, 66, 128, 200, 201, 202, 299,
        ] {
            long[at] = b',';
        }
        records.push(long.clone());
        records.push(
            long.iter()
                .map(|&b| if b == b',' { b' ' } else { b })
                .collect(),
        );
        records.push(
            long.iter()
                .map(|&b| if b == b',' { b'\t' } else { b })
                .collect(),
        );
        records.push(b"x,".repeat(100));
        records.push(b" \t".repeat(100));
        let orders: [&[u64]; 8] = [
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17],
            &[17, 16, 3, 2, 1],
            &[3, 1, 3, 2, 6, 5, 4],
            &[0, 1, 0],
            &[u64::MAX, 2, u64::MAX],
            &[250, 1, 102, 101, 103],
            &[2, 2, 2],
            &[1],
        ];
        let mut direct = FieldIndex::default();
        for separator in [
            Separator::Literal(b','),
            Separator::Literal(b'\t'),
            Separator::Literal(b' '),
            Separator::Literal(0xff),
            Separator::Whitespace,
        ] {
            for order in orders {
                let chosen = FieldIndex::selecting(order.iter().copied());
                let distinct = FieldIndex::distinct(order.iter().copied()).unwrap();
                assert_eq!(chosen.0.is_none(), !keeps_spans(&distinct));
                // Kept spans also for the orders that scan directly.
                let kept = FieldIndex::always_keeping(order.iter().copied());
                assert!(kept.0.is_some());
                for mut index in [chosen, kept] {
                    for record in &records {
                        // Each record starts afresh, then repeats on kept
                        // spans; fields not selected are looked up in between.
                        index.clear();
                        let others = [0, 4, 18, 64, 101];
                        for &wanted in order.iter().chain(&others).chain(order) {
                            let expected = field(record, wanted, separator);
                            assert_eq!(
                                index.field(record, wanted, separator),
                                expected,
                                "{record:?} {separator:?} {order:?} {wanted}"
                            );
                            assert_eq!(direct.field(record, wanted, separator), expected);
                        }
                        // Only selected fields are kept.
                        if let Some(kept) = &index.0 {
                            assert!(kept[0].spans.len() <= kept[0].selected.len());
                        }
                    }
                }
                assert!(direct.0.is_none());
            }
        }
        // A far field keeps only the selected spans.
        let far = b"1,".repeat(100_000);
        let mut index = FieldIndex::always_keeping([1, 2, 100_000].into_iter());
        let separator = Separator::Literal(b',');
        for wanted in [1, 2, 100_000, 100_001] {
            assert_eq!(
                index.field(&far, wanted, separator),
                field(&far, wanted, separator)
            );
        }
        assert_eq!(index.0.as_ref().map(|kept| kept[0].spans.len()), Some(3));
        assert_eq!(
            index.field(&far, 100_000, separator).unwrap().start,
            199_998
        );
    }

    #[test]
    fn spans_are_kept_when_the_fields_before_the_furthest_lie_far_enough_in() {
        let rescanned = RESCANNED_FIELDS;
        let wide: Vec<u64> = (1..=1_000).collect();
        for (fields, keep) in [
            // One or two distinct fields scan directly.
            (&[][..], false),
            (&[1], false),
            (&[1, 1, 2, 2, 1], false),
            (&[0, 0, 7], false),
            (&[3, 3, 3], false),
            (&[1_000, 2, 1_000], false),
            // Fields near the record's start: scanning to each rescans little.
            (&[1, 2, 3], false),
            (&[3, 2, 1, 2, 0], false),
            (&[1, 2, 3, 4, 5, 6], false),
            (&[1, 4, 8], false),
            (&[1, 2, 100_000], false),
            (&[rescanned - 1, rescanned, 100], false),
            // Fields further in: one pass saves enough.
            (&[rescanned - 1, rescanned + 1, 100], true),
            (&[6, 7, 8], true),
            (&[1, 2, 3, 4, 5, 6, 7, 8], true),
            (&[12, 9, 11, 9, 0], true),
            (&[1, 500, 1_000], true),
            (&wide[..], true),
            (&[u64::MAX, u64::MAX - 1, 2], true),
        ] {
            let index = FieldIndex::selecting(fields.iter().copied());
            assert_eq!(index.0.is_some(), keep, "{fields:?}");
            let distinct = FieldIndex::distinct(fields.iter().copied()).unwrap();
            assert_eq!(keeps_spans(&distinct), keep, "{fields:?}");
        }
    }

    #[test]
    fn selected_boundaries_survive_every_buffer_chunk_size() {
        for terminator in [b'\n', 0] {
            let other = if terminator == 0 { b'\n' } else { 0 };
            let input = [terminator, b'a', other, 0xff, terminator, b'b'];
            for capacity in 1..=input.len() {
                let mut reader = io::BufReader::with_capacity(capacity, io::Cursor::new(input));
                let mut record = Vec::new();
                for expected in [b"".as_slice(), &[b'a', other, 0xff], b"b"] {
                    assert!(matches!(
                        read_record_terminated(&mut reader, &mut record, terminator),
                        Ok(true)
                    ));
                    assert_eq!(record, expected);
                }
                assert!(matches!(
                    read_record_terminated(&mut reader, &mut record, terminator),
                    Ok(false)
                ));
                assert!(matches!(
                    read_record_terminated(
                        &mut BrokenReader { sent: false },
                        &mut record,
                        terminator
                    ),
                    Err(ReadError::Io(_))
                ));
                assert!(record.is_empty());
            }
        }
    }
    #[test]
    fn comment_markers_follow_only_leading_spaces_and_tabs() {
        for bytes in [
            b"#".as_slice(),
            b"; note",
            b" \t # note",
            b"\t ;\0tail",
            b"#\r\x0b\xff",
        ] {
            assert!(is_comment(bytes));
        }
        for bytes in [
            b"".as_slice(),
            b" \t ",
            b"1 # note",
            b"a;note",
            b"\0#",
            b" \t\0;",
            b"\r#",
            b"\x0b#",
            b"\x0c;",
            b"\xa0#",
        ] {
            assert!(!is_comment(bytes));
        }
    }

    #[test]
    fn comments_are_recognized_after_long_blank_runs() {
        for newline in [false, true] {
            for length in [8192, 8193, 100_000] {
                let mut bytes = vec![b' '; length];
                bytes[length - 1] = b'#';
                if newline {
                    bytes.push(b'\n');
                }
                let mut buffer = Vec::new();
                let result = read_record(&mut io::Cursor::new(bytes), &mut buffer);
                assert!(matches!(result, Ok(true)));
                assert!(is_comment(&buffer));
            }
        }
    }

    #[test]
    fn whitespace_spans_preserve_gnu_empty_field_boundaries() {
        for (bytes, expected) in [
            (b"".as_slice(), vec![]),
            (b" \t ", vec![(3, 0)]),
            (b" a\t  b \t", vec![(1, 1), (5, 1), (8, 0)]),
            (b"a\tb", vec![(0, 1), (2, 1)]),
            (b"a\r\x0b\x0c\0\xa0 b", vec![(0, 6), (7, 1)]),
        ] {
            let spans: Vec<_> = fields(bytes, Separator::Whitespace)
                .map(|s| (s.start, s.length))
                .collect();
            assert_eq!(spans, expected);
            for wanted in 1..=spans.len() + 1 {
                let span = field(bytes, wanted as u64, Separator::Whitespace);
                assert_eq!(
                    span.map(|s| (s.start, s.length)),
                    spans.get(wanted - 1).copied().ok_or(spans.len() as u64)
                );
            }
        }
    }

    #[test]
    fn iterators_end_after_final_empty_fields_at_the_record_bound() {
        for (bytes, mode, expected) in [
            (vec![b' '; 8192], Separator::Whitespace, 1),
            (vec![b' '; 8192], Separator::Literal(b' '), 8193),
            (b"a ".repeat(4096), Separator::Whitespace, 4097),
        ] {
            let mut spans = fields(&bytes, mode);
            assert_eq!(spans.by_ref().count(), expected);
            assert!(spans.next().is_none());
            assert!(spans.next().is_none());
        }
    }
    #[test]
    fn literal_separator_spans_keep_empty_fields_and_high_bytes() {
        for delimiter in [b',', b' ', b'\t', b'\r', 0x80, 0xff] {
            let bytes = [delimiter, b'1', delimiter, delimiter];
            for (wanted, start, length) in [(1, 0, 0), (2, 1, 1), (3, 3, 0), (4, 4, 0)] {
                let span = field(&bytes, wanted, Separator::Literal(delimiter)).unwrap();
                assert_eq!((span.start, span.length), (start, length));
            }
            assert_eq!(
                field(&bytes, 5, Separator::Literal(delimiter)).unwrap_err(),
                4
            );
            assert_eq!(field(b"", 1, Separator::Literal(delimiter)).unwrap_err(), 0);
        }
    }
    #[test]
    fn record_boundaries() {
        let mut reader = io::Cursor::new(b"\n1\r\n2");
        let mut buffer = Vec::new();
        for expected in [b"".as_slice(), b"1\r", b"2"] {
            assert!(matches!(read_record(&mut reader, &mut buffer), Ok(true)));
            assert_eq!(buffer, expected);
        }
        assert!(matches!(read_record(&mut reader, &mut buffer), Ok(false)));
    }
    struct BrokenReader {
        sent: bool,
    }
    impl io::Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
    }
    impl BufRead for BrokenReader {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.sent {
                Err(io::Error::from_raw_os_error(5))
            } else {
                Ok(b"partial")
            }
        }
        fn consume(&mut self, _: usize) {
            self.sent = true;
        }
    }
    #[test]
    fn read_error_discards_partial_record() {
        let mut buffer = Vec::new();
        assert!(matches!(
            read_record(&mut BrokenReader { sent: false }, &mut buffer),
            Err(ReadError::Io(_))
        ));
        assert!(buffer.is_empty());
    }
    struct Writer {
        bytes: Vec<u8>,
        zero: bool,
        fail_flush: bool,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.zero {
                return Ok(0);
            }
            self.bytes.push(bytes[0]);
            Ok(1)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::Error::from_raw_os_error(28))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn short_zero_writes_and_flush_failure() {
        let mut w = Writer {
            bytes: Vec::new(),
            zero: false,
            fail_flush: false,
        };
        write_output(&mut w, b"12\n").unwrap();
        assert_eq!(w.bytes, b"12\n");
        w.zero = true;
        assert!(write_output(&mut w, b"x").is_err());
        w.zero = false;
        w.fail_flush = true;
        assert!(write_output(&mut w, b"y").is_err());
    }
}
