use fastmash_conversion::field_policy::FieldRange;
use std::io::{self, BufRead, Write};

pub enum ReadError {
    Capacity,
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
    read_record_limit(reader, record, 8192)
}

#[cfg(test)]
pub fn read_record_limit(
    reader: &mut (impl BufRead + ?Sized),
    record: &mut Vec<u8>,
    limit: usize,
) -> Result<bool, ReadError> {
    read_record_terminated(reader, record, limit, b'\n')
}

pub fn read_record_terminated(
    reader: &mut (impl BufRead + ?Sized),
    record: &mut Vec<u8>,
    limit: usize,
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
        if record
            .len()
            .checked_add(length)
            .is_none_or(|size| size > limit)
        {
            return Err(ReadError::Capacity);
        }
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
                        read_record_terminated(&mut reader, &mut record, usize::MAX, terminator),
                        Ok(true)
                    ));
                    assert_eq!(record, expected);
                }
                assert!(matches!(
                    read_record_terminated(&mut reader, &mut record, usize::MAX, terminator),
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
    fn selected_boundaries_survive_every_buffer_chunk_size() {
        for terminator in [b'\n', 0] {
            let other = if terminator == 0 { b'\n' } else { 0 };
            let input = [terminator, b'a', other, 0xff, terminator, b'b'];
            for capacity in 1..=input.len() {
                let mut reader = io::BufReader::with_capacity(capacity, io::Cursor::new(input));
                let mut record = Vec::new();
                for expected in [b"".as_slice(), &[b'a', other, 0xff], b"b"] {
                    assert!(matches!(
                        read_record_terminated(&mut reader, &mut record, usize::MAX, terminator),
                        Ok(true)
                    ));
                    assert_eq!(record, expected);
                }
                assert!(matches!(
                    read_record_terminated(&mut reader, &mut record, usize::MAX, terminator),
                    Ok(false)
                ));
                assert!(matches!(
                    read_record_terminated(
                        &mut BrokenReader { sent: false },
                        &mut record,
                        usize::MAX,
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
    fn physical_record_limit_applies_before_comment_recognition() {
        for newline in [false, true] {
            for length in [8192, 8193] {
                let mut bytes = vec![b' '; length];
                bytes[length - 1] = b'#';
                if newline {
                    bytes.push(b'\n');
                }
                let mut buffer = Vec::new();
                let result = read_record(&mut io::Cursor::new(bytes), &mut buffer);
                if length == 8192 {
                    assert!(matches!(result, Ok(true)));
                    assert!(is_comment(&buffer));
                } else {
                    assert!(matches!(result, Err(ReadError::Capacity)));
                }
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
    fn record_boundaries_and_cap() {
        let mut reader = io::Cursor::new(b"\n1\r\n2");
        let mut buffer = Vec::new();
        for expected in [b"".as_slice(), b"1\r", b"2"] {
            assert!(matches!(read_record(&mut reader, &mut buffer), Ok(true)));
            assert_eq!(buffer, expected);
        }
        assert!(matches!(read_record(&mut reader, &mut buffer), Ok(false)));
        assert!(matches!(
            read_record(&mut io::Cursor::new(vec![b'1'; 8193]), &mut buffer),
            Err(ReadError::Capacity)
        ));
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
