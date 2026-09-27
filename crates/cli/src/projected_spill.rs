//! Anonymous sorted runs with logarithmic live file count.
use super::{Failure, Original, Packed, Record, Storage, allocation, failure, unsupported};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, BufWriter, Read, Seek, Write},
};

fn io_error(error: impl std::fmt::Display) -> Failure {
    failure(format!("sort temporary I/O error: {error}\n").into_bytes())
}

fn temporary() -> Result<File, Failure> {
    fastmash_sort_process::anonymous_file().map_err(io_error)
}
fn number(output: &mut impl Write, value: u64) -> io::Result<()> {
    output.write_all(&value.to_le_bytes())
}
pub(super) trait Codec: Storage {
    fn values(&self) -> usize;
    fn write_payload(&self, output: &mut impl Write) -> io::Result<()>;
    fn read_payload(input: &mut impl Read, keys: usize, values: usize) -> Result<Self, Failure>;
}
fn write<S: Codec>(record: &Record<S>, output: &mut impl Write) -> io::Result<()> {
    for value in [
        record.sequence,
        record.fields,
        u64::from(record.comment),
        record.key_count() as u64,
        record.data.values() as u64,
        u64::from(S::ORIGINAL),
    ] {
        number(output, value)?;
    }
    record.data.write_payload(output)
}
fn read_number(input: &mut impl Read) -> Result<u64, Failure> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes).map_err(io_error)?;
    Ok(u64::from_le_bytes(bytes))
}
fn read<S: Codec>(input: &mut impl BufRead) -> Result<Option<Record<S>>, Failure> {
    if input.fill_buf().map_err(io_error)?.is_empty() {
        return Ok(None);
    }
    let sequence = read_number(input)?;
    let fields = read_number(input)?;
    let comment = match read_number(input)? {
        0 => false,
        1 => true,
        _ => return Err(io_error("invalid run flag")),
    };
    let keys = read_number(input)?;
    let values = read_number(input)?;
    let original = read_number(input)?;
    if original != u64::from(S::ORIGINAL) || (S::ORIGINAL && values != 1) {
        return Err(io_error("invalid run payload"));
    }
    keys.checked_add(values).ok_or_else(allocation)?;
    let keys = usize::try_from(keys).map_err(|_| allocation())?;
    let values = usize::try_from(values).map_err(|_| allocation())?;
    Ok(Some(Record::new(
        S::read_payload(input, keys, values)?,
        fields,
        sequence,
        comment,
    )))
}
impl Codec for Packed {
    fn values(&self) -> usize {
        self.segments.count() - self.keys
    }
    fn write_payload(&self, output: &mut impl Write) -> io::Result<()> {
        for at in 0..self.segments.count() {
            number(output, self.segment(at).len() as u64)?;
        }
        output.write_all(self.segments.payload())
    }
    fn read_payload(input: &mut impl Read, keys: usize, values: usize) -> Result<Self, Failure> {
        let count = keys.checked_add(values).ok_or_else(allocation)?;
        let mut segments = super::storage::Segments::heap(count, 0)?;
        let header = segments.header();
        let mut total = header;
        for at in 0..count {
            let length = usize::try_from(read_number(input)?).map_err(|_| allocation())?;
            total = total.checked_add(length).ok_or_else(allocation)?;
            segments.set_end(at, total);
        }
        let mut bytes = segments.into_vec();
        bytes
            .try_reserve_exact(total - header)
            .map_err(|_| allocation())?;
        bytes.resize(total, 0);
        input.read_exact(&mut bytes[header..]).map_err(io_error)?;
        let segments = super::storage::Segments::from_heap(bytes, count);
        Ok(Packed { keys, segments })
    }
}
impl Codec for Original {
    fn values(&self) -> usize {
        1
    }
    fn write_payload(&self, output: &mut impl Write) -> io::Result<()> {
        for at in 0..self.segments.count() {
            let value = self.segment(at);
            number(output, value.len() as u64)?;
            output.write_all(value)?;
        }
        Ok(())
    }
    fn read_payload(input: &mut impl Read, keys: usize, _: usize) -> Result<Self, Failure> {
        let count = keys.checked_add(1).ok_or_else(allocation)?;
        let mut segments = super::storage::Segments::heap(count, 0)?;
        for at in 0..count {
            let length = usize::try_from(read_number(input)?).map_err(|_| allocation())?;
            let bytes = segments.heap_bytes_mut()?;
            let start = bytes.len();
            let end = start.checked_add(length).ok_or_else(allocation)?;
            bytes.try_reserve(length).map_err(|_| allocation())?;
            bytes.resize(end, 0);
            input.read_exact(&mut bytes[start..]).map_err(io_error)?;
            segments.set_end(at, end);
        }
        Ok(Original { segments })
    }
}
fn complete(writer: BufWriter<File>) -> Result<File, Failure> {
    let mut file = writer.into_inner().map_err(io_error)?;
    file.rewind().map_err(io_error)?;
    Ok(file)
}
// Eight inputs remove the extra merge pass of million-row many-key and
// 1.5-million-row spills at the default chunk target with unchanged chunk
// memory; decoded heads and read buffers scale with the fan-in
// (measured when it was chosen). A merge policy, not an input-size limit.
const MERGE_INPUTS: usize = 8;

#[cfg(test)]
thread_local! {
    static MERGED_RECORDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static CHUNK_RECORDS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A sorted merge input: a run file or the final chunk still in memory.
enum Source<S: Codec> {
    Run(BufReader<File>),
    Memory(std::vec::IntoIter<Record<S>>),
}
impl<S: Codec> Source<S> {
    fn next(&mut self) -> Result<Option<Record<S>>, Failure> {
        match self {
            Self::Run(reader) => read(reader),
            Self::Memory(records) => {
                let record = records.next();
                if record.is_none() {
                    // Release the exhausted chunk's slots before the merge ends.
                    *records = Vec::new().into_iter();
                }
                Ok(record)
            }
        }
    }
}
fn run_sources<S: Codec>(files: Vec<File>) -> impl Iterator<Item = Source<S>> {
    files
        .into_iter()
        .map(|file| Source::Run(BufReader::new(file)))
}

fn merge<S: Codec>(runs: Vec<File>) -> Result<File, Failure> {
    merge_sources::<S>(run_sources(runs).collect())
}
fn merge_sources<S: Codec>(mut sources: Vec<Source<S>>) -> Result<File, Failure> {
    if let [Source::Run(_)] = sources.as_slice()
        && let Some(Source::Run(reader)) = sources.pop()
    {
        // An unread single run is already the merged file.
        let mut file = reader.into_inner();
        file.rewind().map_err(io_error)?;
        return Ok(file);
    }
    let mut output = BufWriter::new(temporary()?);
    merge_into::<S>(sources, |record| {
        #[cfg(test)]
        MERGED_RECORDS.with(|count| count.set(count.get() + 1));
        write(&record, &mut output).map_err(io_error)
    })?;
    complete(output)
}
fn merge_into<S: Codec>(
    sources: Vec<Source<S>>,
    mut consume: impl FnMut(Record<S>) -> Result<(), Failure>,
) -> Result<(), Failure> {
    debug_assert!(sources.len() <= MERGE_INPUTS);
    let mut streams = Vec::new();
    streams
        .try_reserve_exact(sources.len())
        .map_err(|_| allocation())?;
    for mut source in sources {
        let head = source.next()?;
        streams.push((source, head));
    }
    while let Some(at) = streams
        .iter()
        .enumerate()
        .filter_map(|(at, (_, head))| head.as_ref().map(|record| (at, record)))
        .min_by(|(_, left), (_, right)| left.compare(right))
        .map(|(at, _)| at)
    {
        consume(streams[at].1.take().unwrap())?;
        streams[at].1 = streams[at].0.next()?;
    }
    Ok(())
}

pub(super) struct Sorter<S: Codec> {
    target: usize,
    owned: usize,
    records: Vec<Record<S>>,
    levels: Vec<Vec<File>>,
}
impl<S: Codec> Sorter<S> {
    pub(super) fn new() -> Result<Self, Failure> {
        let target = match std::env::var_os("FASTMASH_SORT_MEMORY_BYTES") {
            None => 64 * 1024 * 1024,
            Some(value) => value
                .to_str()
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|s| s.parse::<usize>().ok())
                .filter(|&n| n > 0)
                .ok_or_else(|| {
                    unsupported("FASTMASH_SORT_MEMORY_BYTES must be a positive byte count")
                })?,
        };
        Ok(Self {
            target,
            owned: 0,
            records: Vec::new(),
            levels: Vec::new(),
        })
    }
    pub(super) fn push(&mut self, record: Record<S>) -> Result<(), Failure> {
        let owned = record.owned_bytes()?;
        let total = self
            .owned
            .checked_add(owned)
            .and_then(|n| {
                n.checked_add(
                    self.records
                        .capacity()
                        .checked_mul(std::mem::size_of::<Record<S>>())?,
                )
            })
            .ok_or_else(allocation)?;
        if !self.records.is_empty() && total > self.target {
            self.flush()?;
        }
        self.records.try_reserve(1).map_err(|_| allocation())?;
        self.owned = self.owned.checked_add(owned).ok_or_else(allocation)?;
        self.records.push(record);
        let total = self
            .owned
            .checked_add(
                self.records
                    .capacity()
                    .checked_mul(std::mem::size_of::<Record<S>>())
                    .ok_or_else(allocation)?,
            )
            .ok_or_else(allocation)?;
        if total >= self.target {
            self.flush()?;
        }
        Ok(())
    }
    fn write_chunk(&mut self) -> Result<Option<File>, Failure> {
        if self.records.is_empty() {
            return Ok(None);
        }
        self.records.sort_unstable_by(Record::compare);
        let mut output = BufWriter::new(temporary()?);
        for record in self.records.drain(..) {
            #[cfg(test)]
            CHUNK_RECORDS.with(|count| count.set(count.get() + 1));
            write(&record, &mut output).map_err(io_error)?;
        }
        // Release the chunk's vector allocation as well as its payloads.
        self.records = Vec::new();
        self.owned = 0;
        complete(output).map(Some)
    }
    fn flush(&mut self) -> Result<(), Failure> {
        let Some(mut run) = self.write_chunk()? else {
            return Ok(());
        };
        let mut at = 0;
        loop {
            if at == self.levels.len() {
                self.levels.try_reserve(1).map_err(|_| allocation())?;
                self.levels.push(Vec::new());
            }
            self.levels[at].try_reserve(1).map_err(|_| allocation())?;
            self.levels[at].push(run);
            if self.levels[at].len() < MERGE_INPUTS {
                break;
            }
            run = merge::<S>(std::mem::take(&mut self.levels[at]))?;
            at += 1;
        }
        Ok(())
    }
    pub(super) fn emit(
        mut self,
        mut consume: impl FnMut(Record<S>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        if self.levels.is_empty() {
            self.records.sort_unstable_by(Record::compare);
            for record in self.records {
                consume(record)?;
            }
        } else {
            // Fold smaller levels first. The final level and folded tail stream
            // directly to the consumer, with at most MERGE_INPUTS decoded heads. The
            // final chunk joins the first merge from memory, never as a run.
            self.records.sort_unstable_by(Record::compare);
            let mut tail =
                (!self.records.is_empty()).then(|| Source::Memory(self.records.into_iter()));
            let mut levels = self
                .levels
                .into_iter()
                .filter(|runs| !runs.is_empty())
                .peekable();
            while let Some(files) = levels.next() {
                let mut sources = Vec::new();
                sources
                    .try_reserve_exact(files.len() + 1)
                    .map_err(|_| allocation())?;
                sources.extend(run_sources(files));
                sources.extend(tail.take());
                if levels.peek().is_none() {
                    return merge_into(sources, consume);
                }
                tail = Some(Source::Run(BufReader::new(merge_sources(sources)?)));
            }
            if let Some(mut source) = tail {
                while let Some(record) = source.next()? {
                    consume(record)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    fn record<S: Codec>(sequence: u64) -> Record<S> {
        let bytes = [(sequence % 3) as u8, b'\t', b'v', 0, 0xff, sequence as u8];
        let mut row = super::super::project::<S>(
            &bytes,
            &[2],
            &[1],
            super::super::records::Separator::Literal(b'\t'),
            sequence,
            false,
            false,
            false,
            super::super::last_selected::<S>(&[2], &[1]),
        )
        .ok()
        .unwrap();
        row.comment = sequence.is_multiple_of(5);
        row
    }
    fn levels<S: Codec>() {
        // These short records are stored inline; runs read them back on the heap.
        assert_eq!(record::<S>(0).owned_bytes().ok().unwrap(), 0);
        for count in [
            0, 1, 3, 6, 7, 9, 10, 12, 13, 15, 16, 21, 23, 24, 45, 46, 48, 49, 51, 52, 193,
        ] {
            let mut sorter = Sorter::<S> {
                target: usize::MAX,
                owned: 0,
                records: Vec::new(),
                levels: Vec::new(),
            };
            for sequence in 0..count {
                assert!(sorter.push(record::<S>(sequence)).is_ok());
                if (sequence + 1).is_multiple_of(3) {
                    assert!(sorter.flush().is_ok());
                }
            }
            let mut actual = Vec::new();
            sorter
                .emit(|row| {
                    let mut encoded = Vec::new();
                    write(&row, &mut encoded).unwrap();
                    actual.push(encoded);
                    Ok(())
                })
                .unwrap_or_else(|_| panic!("expected sorted records"));
            let mut expected: Vec<_> = (0..count).map(record::<S>).collect();
            expected.sort_unstable_by(Record::compare);
            let expected: Vec<_> = expected
                .iter()
                .map(|row| {
                    let mut bytes = Vec::new();
                    write(row, &mut bytes).unwrap();
                    bytes
                })
                .collect();
            assert_eq!(actual, expected, "count={count}");
        }
    }
    #[test]
    fn final_merge_preserves_keys_ties_and_payloads_across_levels() {
        levels::<Packed>();
        levels::<Original>();
    }
    #[test]
    fn final_folding_does_not_rewrite_a_larger_run_with_the_smaller_level() {
        for tail in [false, true] {
            let mut sorter = Sorter::<Packed> {
                target: usize::MAX,
                owned: 0,
                records: Vec::new(),
                levels: Vec::new(),
            };
            MERGED_RECORDS.with(|count| count.set(0));
            // One-record runs: (F-1) merges of F fill the upper level with F-1
            // runs, and F-1 more runs stay in the lower level.
            let fan = MERGE_INPUTS;
            let records = fan * fan - 1;
            for sequence in 0..records as u64 {
                sorter.push(record(sequence)).ok().unwrap();
                sorter.flush().ok().unwrap();
            }
            assert_eq!(
                sorter.levels.iter().map(Vec::len).collect::<Vec<_>>(),
                [MERGE_INPUTS - 1, MERGE_INPUTS - 1]
            );
            let merged = (fan - 1) * fan;
            MERGED_RECORDS.with(|count| assert_eq!(count.get(), merged));
            if tail {
                sorter.push(record(records as u64)).ok().unwrap();
            }
            let mut consumed = 0;
            sorter
                .emit(|_| {
                    consumed += 1;
                    Ok(())
                })
                .ok()
                .unwrap();
            assert_eq!(consumed, records + usize::from(tail));
            // Only the lower runs (and optional tail) need another write.
            MERGED_RECORDS.with(|count| assert_eq!(count.get(), consumed));
        }
    }
    fn memory_tail<S: Codec>(levels: usize) {
        // Chunks of three records; MERGE_INPUTS runs fold into the next level,
        // so one flush leaves one level and MERGE_INPUTS + 1 leave a run on
        // each of two levels.
        let flushed = 3 * if levels == 1 { 1 } else { MERGE_INPUTS + 1 };
        for tail in [0, 1, 4] {
            let mut sorter = Sorter::<S> {
                target: usize::MAX,
                owned: 0,
                records: Vec::new(),
                levels: Vec::new(),
            };
            CHUNK_RECORDS.with(|count| count.set(0));
            for sequence in 0..(flushed + tail) as u64 {
                sorter.push(record(sequence)).ok().unwrap();
                if sequence < flushed as u64 && (sequence + 1).is_multiple_of(3) {
                    sorter.flush().ok().unwrap();
                }
            }
            assert_eq!(
                sorter.levels.iter().filter(|runs| !runs.is_empty()).count(),
                levels
            );
            let mut sequences = Vec::new();
            sorter
                .emit(|row| {
                    sequences.push(row.sequence);
                    Ok(())
                })
                .ok()
                .unwrap();
            let mut expected: Vec<_> = (0..(flushed + tail) as u64).map(record::<S>).collect();
            expected.sort_unstable_by(Record::compare);
            let expected: Vec<_> = expected.iter().map(|row| row.sequence).collect();
            assert_eq!(sequences, expected, "levels={levels} tail={tail}");
            // Only flushed chunks reach runs; the tail merges from memory.
            CHUNK_RECORDS.with(|count| assert_eq!(count.get(), flushed, "tail={tail}"));
        }
    }
    #[test]
    fn spilled_sorts_merge_the_final_chunk_from_memory_at_every_level() {
        for levels in [1, 2] {
            memory_tail::<Packed>(levels);
            memory_tail::<Original>(levels);
        }
    }
    #[test]
    fn single_memory_source_merges_into_a_run() {
        let records: Vec<Record<Packed>> = (0..3).map(record).collect();
        let file = merge_sources::<Packed>(vec![Source::Memory(records.into_iter())])
            .ok()
            .unwrap();
        let mut reader = BufReader::new(file);
        let mut count = 0;
        while read::<Packed>(&mut reader).ok().unwrap().is_some() {
            count += 1;
        }
        assert_eq!(count, 3);
    }
    #[test]
    fn memory_tail_failures_propagate_beside_runs() {
        let mut truncated = temporary().ok().unwrap();
        let mut encoded = Vec::new();
        write(&record::<Packed>(0), &mut encoded).unwrap();
        truncated.write_all(&encoded[..encoded.len() - 1]).unwrap();
        truncated.rewind().unwrap();
        let tail = |from| {
            (from..from + 3)
                .map(record::<Packed>)
                .collect::<Vec<_>>()
                .into_iter()
        };
        let sources = vec![
            Source::Run(BufReader::new(truncated)),
            Source::Memory(tail(1)),
        ];
        assert!(merge_into::<Packed>(sources, |_| Ok(())).is_err());
        let mut consumed = 0;
        let sources = vec![Source::Memory(tail(1))];
        assert!(
            merge_into::<Packed>(sources, |_| {
                consumed += 1;
                if consumed == 2 {
                    return Err(failure(b"consumer failed\n".to_vec()));
                }
                Ok(())
            })
            .is_err()
        );
        assert_eq!(consumed, 2);
    }
    fn failures<S: Codec>() {
        let mut bytes = Vec::new();
        write(&record::<S>(0), &mut bytes).unwrap();
        bytes.push(0);
        let mut left = temporary().ok().unwrap();
        left.write_all(&bytes).unwrap();
        left.rewind().unwrap();
        let mut consumed = 0;
        assert!(
            merge_into::<S>(
                run_sources(vec![left, temporary().ok().unwrap()]).collect(),
                |_| {
                    consumed += 1;
                    Ok(())
                }
            )
            .is_err()
        );
        assert_eq!(consumed, 1);
        let mut left = temporary().ok().unwrap();
        write(&record::<S>(0), &mut left).unwrap();
        left.rewind().unwrap();
        assert!(
            merge_into::<S>(
                run_sources(vec![left, temporary().ok().unwrap()]).collect(),
                |_| Err(failure(b"consumer failed\n".to_vec()))
            )
            .is_err()
        );
        let mut encoded = Vec::new();
        write(&record::<S>(0), &mut encoded).unwrap();
        for end in 1..encoded.len() {
            assert!(read::<S>(&mut io::Cursor::new(&encoded[..end])).is_err());
        }
        let mut full = OpenOptions::new().write(true).open("/dev/full").unwrap();
        assert!(write(&record::<S>(0), &mut full).is_err());
        let mut broken = temporary().ok().unwrap();
        broken.write_all(&encoded[..encoded.len() - 1]).unwrap();
        broken.rewind().unwrap();
        assert!(merge::<S>(vec![broken, temporary().ok().unwrap()]).is_err());
    }
    #[test]
    fn streaming_merge_and_truncated_runs_report_failures() {
        failures::<Packed>();
        failures::<Original>();
    }
    fn full_fan_in_failures<S: Codec>() {
        for broken_at in 0..MERGE_INPUTS {
            let mut runs = Vec::new();
            for at in 0..MERGE_INPUTS {
                let mut run = temporary().ok().unwrap();
                let mut encoded = Vec::new();
                write(&record::<S>(at as u64), &mut encoded).unwrap();
                if at == broken_at {
                    encoded.pop();
                }
                run.write_all(&encoded).unwrap();
                run.rewind().unwrap();
                runs.push(run);
            }
            let mut consumed = 0;
            assert!(
                merge_into::<S>(run_sources(runs).collect(), |_| {
                    consumed += 1;
                    Ok(())
                })
                .is_err()
            );
            assert_eq!(consumed, 0);
        }
        let mut runs = Vec::new();
        for at in 0..MERGE_INPUTS {
            let mut run = temporary().ok().unwrap();
            write(&record::<S>(at as u64), &mut run).unwrap();
            run.rewind().unwrap();
            runs.push(run);
        }
        let mut consumed = 0;
        let error = merge_into::<S>(run_sources(runs).collect(), |_| {
            consumed += 1;
            if consumed == 2 {
                Err(failure(b"consumer failed\n".to_vec()))
            } else {
                Ok(())
            }
        })
        .err()
        .unwrap();
        assert_eq!(error.message, b"consumer failed\n");
        assert_eq!(consumed, 2);
    }
    #[test]
    fn full_fan_in_initialization_and_partial_consumer_failures_propagate() {
        full_fan_in_failures::<Packed>();
        full_fan_in_failures::<Original>();
        let full = OpenOptions::new().write(true).open("/dev/full").unwrap();
        let mut output = BufWriter::new(full);
        output.write_all(b"buffered bytes").unwrap();
        assert!(complete(output).is_err());
    }
    #[test]
    fn invalid_run_metadata_and_length_overflow_are_errors() {
        for header in [
            [0, 2, 2, 1, 1, 0],
            [0, 2, 0, 1, 0, 1],
            [0, 2, 0, u64::MAX, 1, 0],
            [0, 2, 0, u64::MAX, 0, 0],
        ] {
            let mut bytes = Vec::new();
            for value in header {
                number(&mut bytes, value).unwrap();
            }
            assert!(read::<Packed>(&mut io::Cursor::new(bytes)).is_err());
        }
        let mut bytes = Vec::new();
        for value in [0, 2, 0, 1, 1, 0, u64::MAX, 1] {
            number(&mut bytes, value).unwrap();
        }
        assert!(read::<Packed>(&mut io::Cursor::new(bytes)).is_err());
        let mut packed = Vec::new();
        write(&record::<Packed>(0), &mut packed).unwrap();
        assert!(read::<Original>(&mut io::Cursor::new(packed)).is_err());
        let mut original = Vec::new();
        write(&record::<Original>(0), &mut original).unwrap();
        assert!(read::<Packed>(&mut io::Cursor::new(original)).is_err());
    }
    fn prefix_order<S: Codec>() {
        let mut rows = Vec::new();
        let mut inputs = vec![
            Vec::new(),
            vec![0],
            vec![0, 0],
            vec![0xfe],
            b"abcdefgh0".to_vec(),
            b"abcdefgh1".to_vec(),
        ];
        for length in 0..=10 {
            inputs.push(vec![b'a'; length]);
            let mut nul = vec![b'a'; length];
            nul.push(0);
            inputs.push(nul);
        }
        for (sequence, bytes) in inputs.iter().enumerate() {
            for keys in 0..=2 {
                let spans = vec![0..bytes.len(); keys];
                let data = S::build(bytes, bytes, &spans, keys, sequence % 2 == 0)
                    .ok()
                    .unwrap();
                let row = Record::new(data, keys as u64, sequence as u64, false);
                let mut wire = Vec::new();
                write(&row, &mut wire).unwrap();
                let decoded = read::<S>(&mut io::Cursor::new(&wire))
                    .ok()
                    .unwrap()
                    .unwrap();
                assert_eq!(row.prefix, decoded.prefix);
                assert_eq!(row.short_key_len, decoded.short_key_len);
                rows.push(decoded);
                rows.push(row);
            }
        }
        // Equal first keys must still consult later keys, independently of sequence.
        for bytes in [b"same\ta".as_slice(), b"same\tb"] {
            let data = S::build(bytes, bytes, &[0..4, 5..6], 2, false)
                .ok()
                .unwrap();
            rows.push(Record::new(data, 2, 0, false));
        }
        for a in &rows {
            for b in &rows {
                assert_eq!(
                    a.compare(b),
                    a.data
                        .compare_keys(&b.data)
                        .then(a.sequence.cmp(&b.sequence))
                );
            }
        }
    }
    #[test]
    fn cached_prefix_order_matches_complete_keys_after_spill_roundtrip() {
        prefix_order::<Packed>();
        prefix_order::<Original>();
    }
}
