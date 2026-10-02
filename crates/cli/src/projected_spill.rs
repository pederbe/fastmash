//! Anonymous sorted runs with logarithmic live file count.
//!
//! Run encoding. A run starts with the shape that every record of one sort
//! shares: a storage byte (0 packed, 1 original), then LEB128 varints of the
//! key count and the segment count. Each record follows as
//!
//! ```text
//! varint zigzag(sequence - previous sequence)   wrapping; previous is 0 at first
//! varint field count
//! varint segment length                         for each segment
//! segment bytes                                 concatenated
//! ```
//!
//! Runs are in key order, so sequences are not monotone: the records of one
//! key ascend by the gap between their lines, and a new key jumps either way.
//! The zigzag delta is smaller than the plain sequence whenever keys repeat
//! within a run, and at most one byte larger when they do not.
use super::batch::Projected;
#[cfg(test)]
use super::storage::Build;
use super::storage::{Keys, Mixed, Segments, View};
use super::{
    Failure, KeyCache, LONG_KEY, Original, Packed, Record, Storage, allocation, failure, key_cache,
    unsupported,
};
use std::{
    fs::File,
    io::{self, BufRead, Read, Seek, Write},
    marker::PhantomData,
};

fn io_error(error: impl std::fmt::Display) -> Failure {
    failure(format!("sort temporary I/O error: {error}\n").into_bytes())
}
fn corrupt() -> Failure {
    io_error("invalid run")
}

fn temporary() -> Result<File, Failure> {
    fastmash_sort_process::anonymous_file().map_err(io_error)
}
pub(super) trait Codec: Storage + Send {
    fn segments(&self) -> &Segments;
    fn from_segments(segments: Segments, keys: usize) -> Self;
}
impl Codec for Packed {
    fn segments(&self) -> &Segments {
        &self.segments
    }
    fn from_segments(segments: Segments, keys: usize) -> Self {
        Packed { segments, keys }
    }
}
impl Codec for Original {
    fn segments(&self) -> &Segments {
        &self.segments
    }
    fn from_segments(segments: Segments, _: usize) -> Self {
        Original { segments }
    }
}

/// The key and segment counts that every record of a run shares.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Shape {
    keys: usize,
    segments: usize,
}
impl Shape {
    /// The shape of original records with `keys` keys: the keys, then the
    /// original.
    pub(super) fn original(keys: usize) -> Result<Self, Failure> {
        let segments = keys.checked_add(1).ok_or_else(allocation)?;
        Ok(Self { keys, segments })
    }
    /// The shape of language records with `keys` keys: each key's sort key
    /// and identity (`storage::language_segments`), then the original.
    pub(super) fn language(keys: usize) -> Result<Self, Failure> {
        let keys = keys.checked_mul(2).ok_or_else(allocation)?;
        let segments = keys.checked_add(1).ok_or_else(allocation)?;
        Ok(Self { keys, segments })
    }
}

/// The longest LEB128 encoding of a `u64`.
const VARINT: usize = 10;
/// Writes `value` as a LEB128 varint at `at`, returning the new end.
fn put(buffer: &mut [u8], mut at: usize, mut value: u64) -> usize {
    while value >= 0x80 {
        buffer[at] = value as u8 | 0x80;
        value >>= 7;
        at += 1;
    }
    buffer[at] = value as u8;
    at + 1
}
fn zigzag(delta: u64) -> u64 {
    (delta << 1) ^ ((delta as i64 >> 63) as u64)
}
fn unzigzag(value: u64) -> u64 {
    (value >> 1) ^ (value & 1).wrapping_neg()
}
/// Reads a LEB128 varint: in one pass when the buffer holds all of it, else
/// (split across a refill, or cut short) a byte at a time.
fn varint(input: &mut impl BufRead) -> Result<u64, Failure> {
    let buffer = input.fill_buf().map_err(io_error)?;
    let mut value = 0u64;
    for (at, &byte) in buffer.iter().take(VARINT).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * at);
        if byte < 0x80 {
            // The tenth byte may hold only bit 63.
            if at == VARINT - 1 && byte > 1 {
                return Err(corrupt());
            }
            input.consume(at + 1);
            return Ok(value);
        }
    }
    if buffer.len() >= VARINT {
        return Err(corrupt());
    }
    split_varint(input)
}
#[cold]
fn split_varint(input: &mut impl BufRead) -> Result<u64, Failure> {
    let mut value = 0u64;
    for at in 0..VARINT {
        let Some(&byte) = input.fill_buf().map_err(io_error)?.first() else {
            return Err(io_error("truncated run"));
        };
        input.consume(1);
        if at == VARINT - 1 && byte > 1 {
            return Err(corrupt());
        }
        value |= u64::from(byte & 0x7f) << (7 * at);
        if byte < 0x80 {
            return Ok(value);
        }
    }
    Err(corrupt())
}
fn size(input: &mut impl BufRead) -> Result<usize, Failure> {
    usize::try_from(varint(input)?).map_err(|_| allocation())
}

/// Writes records to a run: the shape before the first, then each record.
struct Encoder<W: Write> {
    output: W,
    shape: Option<Shape>,
    previous: u64,
}
impl<W: Write> Encoder<W> {
    fn new(output: W) -> Self {
        Self {
            output,
            shape: None,
            previous: 0,
        }
    }
    #[cfg(test)]
    fn write<S: Codec>(&mut self, record: &Record<S>) -> io::Result<()> {
        let segments = record.data.segments();
        let shape = Shape {
            keys: record.key_count(),
            segments: segments.count(),
        };
        self.write_segments(
            S::ORIGINAL,
            shape,
            (record.sequence, record.fields),
            (0..shape.segments).map(|at| segments.segment(at).len()),
            segments.payload(),
        )
    }
    /// Writes a record from a merge (decoded or viewed) of a sort of `shape`.
    fn write_mixed<S: Codec>(
        &mut self,
        record: &Record<Mixed<'_, S>>,
        shape: Shape,
    ) -> io::Result<()> {
        let payload = match &record.data {
            Mixed::Owned(storage) => storage.segments().payload(),
            Mixed::View(view) => view.payload(),
        };
        self.write_segments(
            S::ORIGINAL,
            shape,
            (record.sequence, record.fields),
            (0..shape.segments).map(|at| record.data.segment(at).len()),
            payload,
        )
    }
    /// Writes a chunk's record, viewed in its arena.
    fn write_view(
        &mut self,
        original: bool,
        shape: Shape,
        numbers: (u64, u64),
        view: &View<'_>,
    ) -> io::Result<()> {
        self.write_segments(
            original,
            shape,
            numbers,
            (0..shape.segments).map(|at| view.bytes(at).len()),
            view.payload(),
        )
    }
    /// Writes one record: its sequence and field count (`numbers`), its
    /// segment lengths and its concatenated segments.
    fn write_segments(
        &mut self,
        original: bool,
        shape: Shape,
        (sequence, fields): (u64, u64),
        lengths: impl Iterator<Item = usize>,
        payload: &[u8],
    ) -> io::Result<()> {
        // Varints gather here and usually reach the output in one call; the
        // payload follows in one more.
        let mut buffer = [0; 8 * VARINT];
        let mut at = 0;
        match self.shape {
            Some(expected) if expected == shape => {}
            Some(_) => return Err(io::Error::other("records of different shapes in one run")),
            None => {
                buffer[0] = u8::from(original);
                at = put(&mut buffer, 1, shape.keys as u64);
                at = put(&mut buffer, at, shape.segments as u64);
                self.shape = Some(shape);
            }
        }
        let delta = sequence.wrapping_sub(self.previous);
        at = put(&mut buffer, at, zigzag(delta));
        at = put(&mut buffer, at, fields);
        for length in lengths {
            if at > buffer.len() - VARINT {
                self.output.write_all(&buffer[..at])?;
                at = 0;
            }
            at = put(&mut buffer, at, length as u64);
        }
        self.output.write_all(&buffer[..at])?;
        self.output.write_all(payload)?;
        self.previous = sequence;
        Ok(())
    }
}
impl Encoder<RunWriter<File>> {
    /// A new run file, written through a buffer of `buffer` bytes.
    fn run(buffer: usize) -> Result<Self, Failure> {
        Ok(Self::new(RunWriter::new(buffer, temporary()?)?))
    }
    fn finish(self) -> Result<File, Failure> {
        complete(self.output)
    }
}

/// The buffer of a run file, of `size` bytes, or the allocation refusal.
fn run_buffer_memory(size: usize) -> Result<Vec<u8>, Failure> {
    #[cfg(test)]
    if FAILING_RUN_BUFFER.with(|at| {
        let fails = at.get() == Some(0);
        at.set(at.get().and_then(|count| count.checked_sub(1)));
        fails
    }) {
        return Err(allocation());
    }
    let mut buffer = Vec::new();
    buffer.try_reserve_exact(size).map_err(|_| allocation())?;
    Ok(buffer)
}

/// Writes a run file through a buffer as `BufWriter` does, with the same
/// writes to the file for a run that completes, but refuses when the buffer
/// cannot be allocated instead of aborting. A run that fails is abandoned:
/// dropping the writer does not flush it.
struct RunWriter<W: Write> {
    inner: W,
    /// The buffered bytes; its capacity is the buffer's size.
    buffer: Vec<u8>,
}
impl<W: Write> RunWriter<W> {
    /// Writes to `inner` through a buffer of `size` bytes.
    fn new(size: usize, inner: W) -> Result<Self, Failure> {
        Ok(Self {
            buffer: run_buffer_memory(size)?,
            inner,
        })
    }
    fn spare(&self) -> usize {
        // The length never passes the capacity: no overflow check needed.
        self.buffer.capacity().wrapping_sub(self.buffer.len())
    }
    /// Writes the buffered bytes on. A run whose write fails is abandoned,
    /// so the buffer is emptied either way.
    fn flush_buffer(&mut self) -> io::Result<()> {
        let written = self.inner.write_all(&self.buffer);
        self.buffer.clear();
        written
    }
    /// Writes the buffered bytes on and returns the writer.
    fn into_inner(mut self) -> io::Result<W> {
        self.flush_buffer()?;
        Ok(self.inner)
    }
    #[cold]
    #[inline(never)]
    fn write_all_cold(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() > self.spare() {
            self.flush_buffer()?;
        }
        // Bytes that fill the buffer go on at once, not through it.
        if bytes.len() >= self.buffer.capacity() {
            self.inner.write_all(bytes)
        } else {
            self.buffer.extend_from_slice(bytes);
            Ok(())
        }
    }
}
impl<W: Write> Write for RunWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.spare() {
            self.flush_buffer()?;
        }
        if bytes.len() >= self.buffer.capacity() {
            self.inner.write(bytes)
        } else {
            self.buffer.extend_from_slice(bytes);
            Ok(bytes.len())
        }
    }
    #[inline]
    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.len() < self.spare() {
            self.buffer.extend_from_slice(bytes);
            Ok(())
        } else {
            self.write_all_cold(bytes)
        }
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flush_buffer()?;
        self.inner.flush()
    }
}

/// Reads a run file through a buffer as `BufReader` does, with the same reads
/// from the file, but refuses when the buffer cannot be allocated instead of
/// aborting.
struct RunReader<R: Read> {
    inner: R,
    /// The buffer: its capacity is its size, and all of it is initialized
    /// (zeroed) at the first refill, as safe code reads only into
    /// initialized bytes.
    buffer: Vec<u8>,
    /// The unread bytes are `buffer[at..end]`. Every method keeps
    /// `at <= end <= buffer.len()`, so the unread bytes are sliced without
    /// bounds checks, as `BufReader` slices them: a record's decoding reads
    /// them several times, and the checks cost a few percent of a spilling
    /// sort.
    at: usize,
    end: usize,
}
impl<R: Read> RunReader<R> {
    /// Reads from `inner` through a buffer of `size` bytes.
    fn new(size: usize, inner: R) -> Result<Self, Failure> {
        Ok(Self {
            inner,
            buffer: run_buffer_memory(size)?,
            at: 0,
            end: 0,
        })
    }
    fn into_inner(self) -> R {
        self.inner
    }
    /// Refills the empty buffer: one read of up to all of it.
    #[cold]
    #[inline(never)]
    fn refill(&mut self) -> io::Result<()> {
        (self.at, self.end) = (0, 0);
        let size = self.buffer.capacity();
        self.buffer.resize(size, 0);
        let count = self.inner.read(&mut self.buffer)?;
        // `read` returns at most the length it was given; the bound keeps
        // `end` within the buffer even for a reader that does not.
        self.end = count.min(self.buffer.len());
        Ok(())
    }
    /// The unread bytes.
    #[inline]
    fn unread(&self) -> &[u8] {
        // SAFETY: `at <= end <= buffer.len()`, which every method keeps: `new`
        // and `refill` start from 0, `refill` bounds `end` by the length,
        // `consume` bounds `at` by `end`, and `read_exact` advances `at` by
        // at most the unread length.
        unsafe { self.buffer.get_unchecked(self.at..self.end) }
    }
    /// `Read`'s own loop over `read`, as `BufReader` falls back to when the
    /// buffer holds too few bytes.
    #[cold]
    #[inline(never)]
    fn read_exact_unbuffered(&mut self, out: &mut [u8]) -> io::Result<()> {
        Unbuffered(self).read_exact(out)
    }
}
impl<R: Read> Read for RunReader<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        // A read of a buffer's worth or more into an empty buffer bypasses it.
        if self.at == self.end && out.len() >= self.buffer.capacity() {
            (self.at, self.end) = (0, 0);
            return self.inner.read(out);
        }
        let unread = self.fill_buf()?;
        let count = unread.len().min(out.len());
        out[..count].copy_from_slice(&unread[..count]);
        self.consume(count);
        Ok(count)
    }
    // Always inlined, as `BufReader`'s is into the decoding of a record.
    #[inline(always)]
    fn read_exact(&mut self, out: &mut [u8]) -> io::Result<()> {
        if let Some(unread) = self.unread().get(..out.len()) {
            out.copy_from_slice(unread);
            // No overflow check: `at + out.len()` is at most `end`.
            self.at = self.at.wrapping_add(out.len());
            return Ok(());
        }
        self.read_exact_unbuffered(out)
    }
}
impl<R: Read> BufRead for RunReader<R> {
    #[inline]
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.at >= self.end {
            self.refill()?;
        }
        Ok(self.unread())
    }
    #[inline]
    fn consume(&mut self, amount: usize) {
        // As `BufReader` does; the bound keeps `at <= end` even for a sum
        // that wraps, which no length within the buffer makes.
        self.at = self.at.wrapping_add(amount).min(self.end);
    }
}
/// A reader's `read` alone, for `Read`'s provided `read_exact`.
struct Unbuffered<'a, R: Read>(&'a mut RunReader<R>);
impl<R: Read> Read for Unbuffered<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.0.read(out)
    }
}

/// Reads the records of a run in order.
struct Decoder<R: BufRead> {
    input: R,
    shape: Option<Shape>,
    previous: u64,
}
impl<R: BufRead> Decoder<R> {
    fn new(input: R) -> Self {
        Self {
            input,
            shape: None,
            previous: 0,
        }
    }
    fn next<S: Codec>(&mut self) -> Result<Option<Record<S>>, Failure> {
        if self.input.fill_buf().map_err(io_error)?.is_empty() {
            return Ok(None);
        }
        let shape = match self.shape {
            Some(shape) => shape,
            None => {
                let shape = self.read_shape::<S>()?;
                self.shape = Some(shape);
                shape
            }
        };
        let sequence = self
            .previous
            .wrapping_add(unzigzag(varint(&mut self.input)?));
        let fields = varint(&mut self.input)?;
        let input = &mut self.input;
        let mut segments = Segments::with_lengths(shape.segments, || size(input))?;
        self.input
            .read_exact(segments.payload_mut())
            .map_err(io_error)?;
        self.previous = sequence;
        Ok(Some(Record::new(
            S::from_segments(segments, shape.keys),
            fields,
            sequence,
        )))
    }
    fn read_shape<S: Codec>(&mut self) -> Result<Shape, Failure> {
        let mut storage = 0;
        self.input
            .read_exact(std::slice::from_mut(&mut storage))
            .map_err(io_error)?;
        let keys = size(&mut self.input)?;
        let segments = size(&mut self.input)?;
        let valid = if S::ORIGINAL {
            keys.checked_add(1) == Some(segments)
        } else {
            keys <= segments
        };
        if storage != u8::from(S::ORIGINAL) || !valid {
            return Err(corrupt());
        }
        Ok(Shape { keys, segments })
    }
}
fn complete(writer: RunWriter<File>) -> Result<File, Failure> {
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
    static FAILED_RESERVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The run buffers allocated before one fails, if one is to fail.
    static FAILING_RUN_BUFFER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Run files open with a buffer at once: a merge's inputs and its output.
const RUN_BUFFERS: usize = MERGE_INPUTS + 1;

/// The buffer of each run file that a sort of memory target `target` writes
/// or reads: a 128th of the target, from 64 bytes to 256 KiB. Larger buffers
/// take fewer system calls; the chunk leaves `RUN_BUFFERS` of them out of
/// its memory (`chunk_memory`).
fn run_buffer(target: usize) -> usize {
    (target / 128).clamp(64, 256 << 10)
}

/// The memory that the chunk of a sort of memory target `target` may take
/// when its records' producer holds `share` bytes: the rest of the target
/// after the run files' buffers.
pub(super) fn chunk_memory(target: usize, share: usize) -> usize {
    target
        .saturating_sub(share)
        .saturating_sub(RUN_BUFFERS * run_buffer(target))
}

/// Merges one level's full set of `runs` into a run, through buffers of
/// `buffer` bytes.
fn merge<S: Codec>(runs: Vec<File>, shape: Shape, buffer: usize) -> Result<File, Failure> {
    let mut sources = Vec::new();
    sources
        .try_reserve_exact(runs.len())
        .map_err(|_| allocation())?;
    for file in runs {
        sources.push(Source::Run(Decoder::new(RunReader::new(buffer, file)?)));
    }
    let mut output = Encoder::run(buffer)?;
    merge_into::<S>(sources, |record| {
        #[cfg(test)]
        MERGED_RECORDS.with(|count| count.set(count.get() + 1));
        output.write_mixed(&record, shape).map_err(io_error)
    })?;
    output.finish()
}

// Compact chunks. A chunk's records are appended to one arena, each as
//
//   varint  index in the chunk (its sequence minus the chunk's first)
//   varint  field count
//   byte    end-table width
//   the end table and the segments, as a `View`
//
// with a 16-byte `Entry` each. A stable LSD radix sort orders the entries by
// (key prefix, key length), skipping the digits that every entry shares:
// `Record::compare`'s order for key layouts that the prefix holds. Groups of
// equal prefix whose keys it does not hold are then ordered word by word:
// by the next eight bytes of their keys, each round reading each record once
// (`refine`). Entries arrive in sequence order and ties keep that order,
// which is the sequence tie-break. A spilled chunk becomes a run in the
// format above; the chunk still in memory reaches the merge and the consumer
// as views, so no record is copied or allocated on its own.

/// Records prefetched ahead of the one being read: sorted entries visit the
/// arena out of order.
const PREFETCH: usize = 16;
/// Chunks below this many entries sort by comparison instead of by radix.
const RADIX_MIN: usize = 256;
/// Key groups below this many entries are ordered by comparing their keys.
const REFINE_MIN: usize = 16;
/// Key bytes after which a group is ordered by comparing its keys: a long
/// shared prefix makes word rounds cost more than comparisons.
const REFINE_DEPTH: usize = 64;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Entry {
    prefix: u64,
    /// The record's arena offset << 8 | the key length of `key_cache`.
    meta: u64,
}
impl Entry {
    #[inline]
    fn offset(&self) -> usize {
        (self.meta >> 8) as usize
    }
    #[inline]
    fn key_len(&self) -> u8 {
        self.meta as u8
    }
    #[inline]
    fn key(&self) -> KeyCache {
        (self.prefix, self.key_len())
    }
    /// The chunk order: key prefix and length, then arena offset, which
    /// follows the sequence.
    #[inline]
    fn order(&self) -> (u64, u8, usize) {
        (self.prefix, self.key_len(), self.offset())
    }
    /// Radix digit `d`, least significant first: the key length, then the
    /// prefix bytes from the last.
    #[inline]
    fn digit(&self, d: usize) -> usize {
        if d == 0 {
            usize::from(self.key_len())
        } else {
            ((self.prefix >> (8 * (d - 1))) & 0xff) as usize
        }
    }
}

/// Each radix digit's bucket counts over `entries`.
fn histogram(entries: &[Entry]) -> [[usize; 256]; 9] {
    let mut histogram = [[0usize; 256]; 9];
    for entry in entries {
        for (d, counts) in histogram.iter_mut().enumerate() {
            counts[entry.digit(d)] += 1;
        }
    }
    histogram
}

/// Stable LSD radix sort of `entries` by (prefix, key length), skipping the
/// digits every entry shares (`histogram` holds the counts), with `scratch`
/// as long. Returns whether the sorted entries ended in `scratch`.
fn radix(entries: &mut [Entry], scratch: &mut [Entry], histogram: &[[usize; 256]; 9]) -> bool {
    let n = entries.len();
    let mut in_scratch = false;
    for (d, counts) in histogram.iter().enumerate() {
        if counts.contains(&n) {
            continue;
        }
        let mut position = [0usize; 256];
        let mut sum = 0;
        for (slot, &count) in position.iter_mut().zip(counts) {
            *slot = sum;
            sum += count;
        }
        let (source, target) = if in_scratch {
            (&scratch[..], &mut entries[..])
        } else {
            (&entries[..], &mut scratch[..])
        };
        for entry in source {
            let bucket = entry.digit(d);
            target[position[bucket]] = *entry;
            position[bucket] += 1;
        }
        in_scratch = !in_scratch;
    }
    in_scratch
}

/// Reads a varint that the chunk wrote.
#[inline]
fn arena_varint(arena: &[u8], mut at: usize) -> (u64, usize) {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = arena[at];
        at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte < 0x80 {
            return (value, at);
        }
        shift += 7;
    }
}

/// The chunk record at `offset`: its index in the chunk, its field count and
/// its segments.
#[inline]
fn arena_record(arena: &[u8], offset: usize, shape: Shape) -> (u64, u64, View<'_>) {
    let (index, at) = arena_varint(arena, offset);
    let (fields, at) = arena_varint(arena, at);
    let width = arena[at];
    let view = View::new(&arena[at + 1..], width, shape.segments, shape.keys);
    (index, fields, view)
}

/// Starts loading the record at `offset` (and the next cache line) into
/// cache: the sorted order visits the arena out of order (reading six million
/// short records in sorted order took 146 ms with it and 303 ms without, in a
/// micro-benchmark).
#[inline(always)]
fn prefetch(arena: &[u8], offset: usize) {
    use std::arch::x86_64::{_MM_HINT_T0, _mm_prefetch};
    let pointer = arena.as_ptr().wrapping_add(offset).cast::<i8>();
    // SAFETY: `_mm_prefetch` needs SSE, which every x86-64 processor has. The
    // pointer may pass the arena's end (wrapping arithmetic keeps that
    // defined), but a prefetch never dereferences it and cannot fault.
    unsafe {
        _mm_prefetch::<_MM_HINT_T0>(pointer);
        _mm_prefetch::<_MM_HINT_T0>(pointer.wrapping_add(64));
    }
}

/// The eight key bytes of record `entry` at `offset` into key `key`, as a
/// sort word, with a length digit: the bytes left in the key when that is at
/// most eight, else `LONG_KEY`.
#[inline]
fn word(arena: &[u8], entry: &Entry, shape: Shape, key: usize, offset: usize) -> Entry {
    let bytes = &arena_record(arena, entry.offset(), shape).2.bytes(key)[offset..];
    let mut word = [0; 8];
    let n = bytes.len().min(8);
    word[..n].copy_from_slice(&bytes[..n]);
    let digit = if bytes.len() <= 8 {
        bytes.len() as u8
    } else {
        LONG_KEY
    };
    Entry {
        prefix: u64::from_be_bytes(word),
        meta: ((entry.offset() as u64) << 8) | u64::from(digit),
    }
}

/// Orders `group` by its complete keys, then by arena offset.
fn compare_group(arena: &[u8], group: &mut [Entry], shape: Shape) {
    group.sort_unstable_by(|a, b| {
        let left = arena_record(arena, a.offset(), shape).2;
        let right = arena_record(arena, b.offset(), shape).2;
        left.compare_keys(&right).then(a.offset().cmp(&b.offset()))
    });
}

/// A group of entries tied so far: their range, and the key and the byte
/// offset into it at which they may differ. They agree on every earlier key.
type Group = (std::ops::Range<usize>, usize, usize);

/// Orders the entries that the prefix sort leaves tied on keys that their
/// prefix does not hold (`LONG_KEY`), most significant bytes first: a round
/// reads the next eight key bytes of each member of a tied group (the first
/// key's after its prefix, then every later key's, as `compare_keys` orders
/// them) into `words`, sorts those as entries, and splits the group where
/// they differ. Small groups, groups still tied `REFINE_DEPTH` bytes into a key,
/// and any group without memory for its words, are ordered by comparing keys.
fn refine(arena: &[u8], entries: &mut [Entry], words: &mut Vec<Entry>, shape: Shape) {
    // Records without keys all tie.
    if shape.keys == 0 {
        return;
    }
    let mut groups: Vec<Group> = Vec::new();
    let mut at = 0;
    while at < entries.len() {
        let key = entries[at].key();
        let mut end = at + 1;
        while end < entries.len() && entries[end].key() == key {
            end += 1;
        }
        if key.1 == LONG_KEY && end - at > 1 {
            // One key longer than its prefix continues after it; several
            // keys start again, now with the first key's length.
            let offset = if shape.keys == 1 { 8 } else { 0 };
            tied(
                arena,
                &mut groups,
                &mut entries[at..end],
                (at..end, 0, offset),
                shape,
            );
            while let Some(group) = groups.pop() {
                round(arena, entries, words, &mut groups, group, shape);
            }
            // Rounds leave sort words in the entries: restore their key cache.
            for entry in &mut entries[at..end] {
                *entry = Entry {
                    prefix: key.0,
                    meta: ((entry.offset() as u64) << 8) | u64::from(LONG_KEY),
                };
            }
        }
        at = end;
    }
}

/// Queues a tied group (`members`, the entries in its range) for a round, or
/// orders it by comparing keys.
fn tied(arena: &[u8], groups: &mut Vec<Group>, members: &mut [Entry], group: Group, shape: Shape) {
    let (range, _, offset) = &group;
    if range.len() < REFINE_MIN || *offset >= REFINE_DEPTH || groups.try_reserve(1).is_err() {
        compare_group(arena, members, shape);
    } else {
        groups.push(group);
    }
}

/// One round of `refine`: sorts `group` by its members' words and queues the
/// members that still tie.
fn round(
    arena: &[u8],
    entries: &mut [Entry],
    words: &mut Vec<Entry>,
    groups: &mut Vec<Group>,
    (range, key, offset): Group,
    shape: Shape,
) {
    let start = range.start;
    let group = &mut entries[range];
    words.clear();
    if words.try_reserve_exact(group.len()).is_err() {
        compare_group(arena, group, shape);
        return;
    }
    // After the first round the members are in key order, which visits the
    // arena out of order.
    words.extend(group.iter().enumerate().map(|(at, entry)| {
        if let Some(ahead) = group.get(at + PREFETCH) {
            prefetch(arena, ahead.offset());
        }
        word(arena, entry, shape, key, offset)
    }));
    // The group itself is the radix scratch; the sorted words replace its
    // entries (they hold the same offsets).
    if words.len() >= RADIX_MIN {
        let counts = histogram(words);
        if !radix(words, group, &counts) {
            group.copy_from_slice(words);
        }
    } else {
        words.sort_unstable_by_key(Entry::order);
        group.copy_from_slice(words);
    }
    // Members that still tie on this word go on to the next word, or to the
    // next key when this one ended.
    let mut run = 0;
    for at in 1..=group.len() {
        if at == group.len() || group[at].key() != group[run].key() {
            let long = group[run].key_len() == LONG_KEY;
            if at - run > 1 && (long || key + 1 < shape.keys) {
                let next = if long {
                    (key, offset + 8)
                } else {
                    (key + 1, 0)
                };
                let tie = (start + run..start + at, next.0, next.1);
                tied(arena, groups, &mut group[run..at], tie, shape);
            }
            run = at;
        }
    }
}

#[cfg(test)]
thread_local! {
    /// The largest memory of a chunk of several records when it spilled.
    static SPILLED_MEMORY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// The chunks spilled.
    static SPILLED_CHUNKS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The largest memory of a spilled chunk (`SPILLED_MEMORY`) since the last call.
#[cfg(test)]
pub(super) fn spilled_memory() -> usize {
    SPILLED_MEMORY.with(|max| max.replace(0))
}

/// A sort: its chunk in memory, and its runs in levels of up to
/// `MERGE_INPUTS`. The chunk's memory is the arena's capacity and, per entry
/// slot, the entry and its radix scratch; with the share of the target that
/// the records' producer holds (`batch::share`) and the run files' buffers,
/// it stays within the target (`chunk_memory`). The arena and the entries
/// grow by an eighth, each within its share of the memory left (as it shares
/// the chunk's use), and the chunk spills when its next growth would pass
/// its memory, unless the sort's growth (`memory::grow`) gives it more first.
/// A chunk that cannot grow spills and gives its memory back, and later
/// chunks keep to half the memory it asked for; a record larger than the
/// chunk's memory spills at once.
pub(super) struct Sorter<S: Codec> {
    /// The memory the chunk may take.
    limit: usize,
    /// Until it declines: given the memory of a full chunk and the records
    /// before the next one, the memory the chunk may take instead of
    /// spilling (`memory::grow`).
    grow: Option<fn(usize, u64) -> Option<usize>>,
    /// The buffer of each run file (`run_buffer`).
    buffer: usize,
    arena: Vec<u8>,
    entries: Vec<Entry>,
    scratch: Vec<Entry>,
    /// The sequence of the chunk's first record.
    base: u64,
    shape: Option<Shape>,
    levels: Vec<Vec<File>>,
    /// Input still held to be read again, which the chunk leaves room for.
    unread: super::replay::Unread,
    storage: PhantomData<S>,
}

/// The largest size of a chunk record's body with `count` segments of
/// `payload` bytes in total.
fn body_size(count: usize, payload: usize) -> Result<usize, Failure> {
    count
        .checked_mul(usize::from(View::width(payload)))
        .and_then(|table| table.checked_add(payload))
        .and_then(|size| size.checked_add(VARINT + 1))
        .ok_or_else(allocation)
}

/// Appends a chunk record's body to `out`: its field count, then its `count`
/// segments of `payload` bytes in total, with their end table (`View`). The
/// first `fold` segments are ASCII-uppercased. Returns the key cache of a
/// record whose first `keys` segments are its keys.
pub(super) fn encode<'s>(
    out: &mut Vec<u8>,
    fields: u64,
    (count, payload): (usize, usize),
    (keys, fold): (usize, usize),
    segment: impl Fn(usize) -> &'s [u8],
) -> Result<KeyCache, Failure> {
    // Views count segments in 32 bits.
    u32::try_from(count).map_err(|_| allocation())?;
    out.try_reserve(body_size(count, payload)?)
        .map_err(|_| allocation())?;
    let width = View::width(payload);
    let mut header = [0; VARINT + 1];
    let at = put(&mut header, 0, fields);
    header[at] = width;
    out.extend_from_slice(&header[..=at]);
    let mut end = 0usize;
    for at in 0..count {
        end += segment(at).len();
        View::put_end(out, end, width);
    }
    debug_assert_eq!(end, payload);
    let first = out.len();
    for at in 0..count {
        let start = out.len();
        out.extend_from_slice(segment(at));
        if at < fold {
            out[start..].make_ascii_uppercase();
        }
    }
    let key = (keys != 0 && count != 0).then(|| &out[first..first + segment(0).len()]);
    Ok(key_cache(key, keys))
}

impl<S: Codec> Sorter<S> {
    /// A sort of memory target `target`, whose records' producer holds
    /// `share` bytes of it.
    pub(super) fn new(target: usize, share: usize) -> Self {
        Self {
            limit: chunk_memory(target, share),
            grow: None,
            buffer: run_buffer(target),
            arena: Vec::new(),
            entries: Vec::new(),
            scratch: Vec::new(),
            base: 0,
            shape: None,
            levels: Vec::new(),
            unread: None,
            storage: PhantomData,
        }
    }
    /// Leaves room in the chunk's memory for the input a reader still holds
    /// to be read again (`unread`), which shrinks as the sort reads it. The
    /// memory already granted to the held input is the chunk's to take as the
    /// input is read, so the chunk may take at least as much.
    pub(super) fn leave_room(&mut self, unread: super::replay::Unread) {
        if let Some(unread) = &unread {
            self.limit = self
                .limit
                .max(unread.load(std::sync::atomic::Ordering::Relaxed));
        }
        self.unread = unread;
    }
    /// The memory the chunk has, and the input still held beside it.
    fn used(&self) -> usize {
        let held = self.unread.as_ref().map_or(0, |unread| {
            unread.load(std::sync::atomic::Ordering::Relaxed)
        });
        self.memory().saturating_add(held)
    }
    /// A sort of memory target `target`, whose chunk may grow when it fills
    /// if the target allows (`memory::grow`), and whose records' producer
    /// holds `share` bytes of it.
    pub(super) fn with_target(target: super::memory::Target, share: usize) -> Self {
        Self {
            grow: target
                .grows
                .then_some(super::memory::grow as fn(usize, u64) -> Option<usize>),
            ..Self::new(target.bytes, share)
        }
    }
    /// Gives a full chunk the memory its growth allows, before the record
    /// after `records` others: whether it has more now. A growth that
    /// declines is not asked again; while input is still held beside the
    /// chunk ([`Sorter::leave_room`]), it waits.
    fn grow(&mut self, records: u64) -> bool {
        let Some(grow) = self.grow else {
            return false;
        };
        // While input is still held, the chunk keeps to its memory, and
        // asks again once the held input is read.
        if self.used() > self.memory() {
            return false;
        }
        match grow(self.memory(), records) {
            Some(memory) if memory > self.limit => {
                self.limit = memory;
                true
            }
            _ => {
                self.grow = None;
                false
            }
        }
    }
    /// The chunk's memory.
    fn memory(&self) -> usize {
        let slots = self
            .entries
            .capacity()
            .saturating_add(self.entries.capacity().max(self.scratch.capacity()));
        self.arena
            .capacity()
            .saturating_add(slots.saturating_mul(std::mem::size_of::<Entry>()))
    }
    fn reserve(&mut self, arena: usize, entries: usize) -> Result<(), ()> {
        #[cfg(test)]
        if FAILED_RESERVATIONS.with(|count| count.replace(count.get().saturating_sub(1)) > 0) {
            return Err(());
        }
        self.arena.try_reserve_exact(arena).map_err(drop)?;
        self.entries.try_reserve_exact(entries).map_err(drop)
    }
    /// Gives the chunk's memory back.
    fn release(&mut self) {
        self.arena = Vec::new();
        self.entries = Vec::new();
        self.scratch = Vec::new();
    }
    /// Room for a record of `need` arena bytes and its entry, after `records`
    /// others. A chunk that is full spills, unless its growth (`grow`) gives
    /// it more memory. One that cannot grow (under a memory limit, say)
    /// spills too, gives its memory back and keeps to half the memory it
    /// asked for from then on (its memory with the growth), without growing
    /// again, and the growth is retried once; an empty chunk that cannot grow
    /// refuses.
    fn room(&mut self, need: usize, records: u64) -> Result<(), Failure> {
        let slot = 2 * std::mem::size_of::<Entry>();
        let mut spilled = false;
        loop {
            let arena_short = self.arena.capacity() - self.arena.len() < need;
            let entries_full = self.entries.len() == self.entries.capacity();
            if !arena_short && !entries_full {
                return Ok(());
            }
            // Each part takes the share of the memory left that it has of
            // the chunk's use, with this record: neither takes what the
            // other will need, and the chunk fills its memory.
            // Held input leaves the chunk the rest, but never less than a
            // quarter of its memory, so that it does not spill record by
            // record while held input fills the limit.
            let free = self
                .limit
                .saturating_sub(self.used())
                .max((self.limit / 4).saturating_sub(self.memory()));
            let arena_used = self.arena.len().saturating_add(need) as u128;
            let slots_used = (self.entries.len() as u128 + 1) * slot as u128;
            let arena_free = (free as u128 * arena_used / (arena_used + slots_used)) as usize;
            let arena_step = if arena_short {
                (self.arena.capacity() / 8)
                    .max(4096)
                    .min(arena_free)
                    .max(need)
            } else {
                0
            };
            let entries_step = if entries_full {
                (self.entries.capacity() / 8)
                    .max(64)
                    .min((free - arena_free) / slot)
                    .max(1)
            } else {
                0
            };
            let growth = arena_step.saturating_add(entries_step.saturating_mul(slot));
            if growth > free && !spilled && !self.entries.is_empty() {
                if self.grow(records) {
                    continue;
                }
                self.flush()?;
                spilled = true;
                continue;
            }
            let requested = self.memory().saturating_add(growth);
            if self.reserve(arena_step, entries_step).is_ok() {
                return Ok(());
            }
            if spilled || self.entries.is_empty() {
                return Err(allocation());
            }
            self.flush()?;
            self.release();
            // Half of what failed, not half the chunk's memory: a large
            // record's growth can dwarf that, and would leave every later
            // chunk a fraction of what memory allows.
            self.limit = self.limit.min(requested / 2);
            self.grow = None;
            spilled = true;
        }
    }
    /// Starts a record of `need` body bytes and shape `shape`: makes room,
    /// writes its index, and returns its arena offset.
    fn start(&mut self, shape: Shape, need: usize, sequence: u64) -> Result<u64, Failure> {
        match self.shape {
            None => self.shape = Some(shape),
            Some(expected) if expected == shape => {}
            Some(_) => return Err(unsupported("internal sort records of different shapes")),
        }
        // Records arrive in sequence order, numbered from zero.
        self.room(need.checked_add(VARINT).ok_or_else(allocation)?, sequence)?;
        if self.entries.is_empty() {
            self.base = sequence;
        }
        let offset = u64::try_from(self.arena.len())
            .ok()
            .filter(|&offset| offset < 1 << 56)
            .ok_or_else(allocation)?;
        let mut index = [0; VARINT];
        let end = put(&mut index, 0, sequence.wrapping_sub(self.base));
        self.arena.extend_from_slice(&index[..end]);
        Ok(offset)
    }
    /// Files the record started at `offset` with its key cache.
    fn end(&mut self, offset: u64, (prefix, key_len): KeyCache) -> Result<(), Failure> {
        self.entries.push(Entry {
            prefix,
            meta: (offset << 8) | u64::from(key_len),
        });
        // Only a record larger than the chunk's room grows it past its limit:
        // such a record spills at once.
        if self.memory() > self.limit {
            self.flush()?;
        }
        Ok(())
    }
    /// Appends a projected record: every span of `raw` (packed), or the
    /// `keys` key spans and `data` (original), with its key segments
    /// ASCII-uppercased when `fold`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn push(
        &mut self,
        raw: &[u8],
        data: &[u8],
        spans: &[std::ops::Range<usize>],
        keys: usize,
        fold: bool,
        fields: u64,
        sequence: u64,
    ) -> Result<(), Failure> {
        let count = if S::ORIGINAL {
            keys.checked_add(1).ok_or_else(allocation)?
        } else {
            spans.len()
        };
        let segment = |at: usize| -> &[u8] {
            if S::ORIGINAL && at == keys {
                data
            } else {
                &raw[spans[at].clone()]
            }
        };
        let mut payload = 0usize;
        for at in 0..count {
            payload = payload
                .checked_add(segment(at).len())
                .ok_or_else(allocation)?;
        }
        let shape = Shape {
            keys,
            segments: count,
        };
        let offset = self.start(shape, body_size(count, payload)?, sequence)?;
        let fold = if fold { keys } else { 0 };
        let cache = encode(
            &mut self.arena,
            fields,
            (count, payload),
            (keys, fold),
            segment,
        )?;
        self.end(offset, cache)
    }
    /// Appends a record projected elsewhere (`encode`), of shape `shape`.
    pub(super) fn push_body(&mut self, record: Projected<'_>, shape: Shape) -> Result<(), Failure> {
        let offset = self.start(shape, record.body.len(), record.sequence)?;
        self.arena.extend_from_slice(record.body);
        self.end(offset, (record.prefix, record.key_len))
    }
    /// Sorts the chunk's entries.
    fn sort(&mut self) {
        let n = self.entries.len();
        // The scratch takes the entries' capacity, so the two keep equal
        // capacities when the sort swaps them. Without it, or for a small
        // chunk, an unstable sort that breaks ties by arena offset gives the
        // same order and needs no memory. Entries that share every digit are
        // in order already.
        self.scratch.clear();
        if n >= RADIX_MIN {
            let counts = histogram(&self.entries);
            if counts.iter().any(|digit| !digit.contains(&n)) {
                if self
                    .scratch
                    .try_reserve_exact(self.entries.capacity())
                    .is_ok()
                {
                    self.scratch.resize(n, Entry::default());
                    if radix(&mut self.entries, &mut self.scratch, &counts) {
                        std::mem::swap(&mut self.entries, &mut self.scratch);
                    }
                    self.scratch.clear();
                } else {
                    self.entries.sort_unstable_by_key(Entry::order);
                }
            }
        } else {
            self.entries.sort_unstable_by_key(Entry::order);
        }
        if let Some(shape) = self.shape {
            refine(&self.arena, &mut self.entries, &mut self.scratch, shape);
        }
    }
    /// Writes the chunk as a run and files the run in the levels.
    fn flush(&mut self) -> Result<(), Failure> {
        let Some(shape) = self.shape else {
            return Ok(());
        };
        if self.entries.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        {
            SPILLED_CHUNKS.with(|count| count.set(count.get() + 1));
            if self.entries.len() > 1 {
                let memory = self.memory();
                SPILLED_MEMORY.with(|max| max.set(max.get().max(memory)));
            }
        }
        self.sort();
        let mut output = Encoder::run(self.buffer)?;
        let (arena, entries) = (&self.arena, &self.entries);
        for (at, entry) in entries.iter().enumerate() {
            if let Some(ahead) = entries.get(at + PREFETCH) {
                prefetch(arena, ahead.offset());
            }
            #[cfg(test)]
            CHUNK_RECORDS.with(|count| count.set(count.get() + 1));
            let (index, fields, view) = arena_record(arena, entry.offset(), shape);
            let numbers = (self.base.wrapping_add(index), fields);
            output
                .write_view(S::ORIGINAL, shape, numbers, &view)
                .map_err(io_error)?;
        }
        let mut run = output.finish()?;
        let (records, bytes) = (self.entries.len(), self.arena.len());
        self.arena.clear();
        self.entries.clear();
        if self.memory() > self.limit {
            // A record larger than the limit grew the chunk past it.
            self.release();
        } else {
            // The next chunk reuses the memory, split between the arena and
            // the entries as this chunk used them: the one that did not fill
            // gives its unused part to the other.
            if records < self.entries.capacity() / 4 * 3 {
                self.entries.shrink_to(records + records / 8);
                self.scratch.shrink_to(self.entries.capacity());
            }
            if bytes < self.arena.capacity() / 4 * 3 {
                self.arena.shrink_to(bytes + bytes / 8);
            }
        }
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
            run = merge::<S>(std::mem::take(&mut self.levels[at]), shape, self.buffer)?;
            at += 1;
        }
        Ok(())
    }
    /// Sorts the chunk that stays in memory. When earlier chunks spilled, it
    /// gives back the memory it kept for growth, for the merge and the
    /// consumer.
    pub(super) fn finish(mut self) -> Finished<S> {
        self.sort();
        self.scratch = Vec::new();
        if !self.levels.is_empty() {
            self.arena.shrink_to_fit();
            self.entries.shrink_to_fit();
        }
        Finished {
            arena: self.arena,
            entries: self.entries,
            base: self.base,
            shape: self.shape,
            levels: self.levels,
            buffer: self.buffer,
            storage: PhantomData,
        }
    }
}

/// A sort whose records have all arrived: its runs, and its last chunk,
/// sorted, in memory.
pub(super) struct Finished<S: Codec> {
    arena: Vec<u8>,
    entries: Vec<Entry>,
    base: u64,
    shape: Option<Shape>,
    levels: Vec<Vec<File>>,
    /// The buffer of each run file (`run_buffer`).
    buffer: usize,
    storage: PhantomData<S>,
}

impl<S: Codec> Finished<S> {
    /// Hands every record to `consume` in order: the chunk's records as
    /// views, merged with the runs. The final level and the folded smaller
    /// levels stream to the consumer, with at most `MERGE_INPUTS` decoded
    /// heads; the chunk joins the first merge from memory, never as a run.
    pub(super) fn emit<'a>(
        &'a mut self,
        mut consume: impl FnMut(Record<Mixed<'a, S>>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let levels = std::mem::take(&mut self.levels);
        let this: &'a Self = self;
        let Some(shape) = this.shape else {
            return Ok(());
        };
        let mut tail = Tail {
            arena: &this.arena,
            entries: &this.entries,
            at: 0,
            base: this.base,
            shape,
        };
        let mut levels = levels
            .into_iter()
            .filter(|runs| !runs.is_empty())
            .peekable();
        if levels.peek().is_none() {
            while let Some(record) = tail.next() {
                consume(record)?;
            }
            return Ok(());
        }
        let mut tail = (!this.entries.is_empty()).then_some(Source::Tail(tail, PhantomData));
        while let Some(files) = levels.next() {
            let mut sources = Vec::new();
            sources
                .try_reserve_exact(files.len() + 1)
                .map_err(|_| allocation())?;
            for file in files {
                sources.push(Source::Run(Decoder::new(RunReader::new(
                    this.buffer,
                    file,
                )?)));
            }
            sources.extend(tail.take());
            if levels.peek().is_none() {
                return merge_into(sources, consume);
            }
            let file = if let [Source::Run(_)] = sources.as_slice()
                && let Some(Source::Run(decoder)) = sources.pop()
            {
                // An unread single run is already the merged file.
                let mut file = decoder.input.into_inner();
                file.rewind().map_err(io_error)?;
                file
            } else {
                let mut output = Encoder::run(this.buffer)?;
                merge_into(sources, |record| {
                    #[cfg(test)]
                    MERGED_RECORDS.with(|count| count.set(count.get() + 1));
                    output.write_mixed(&record, shape).map_err(io_error)
                })?;
                output.finish()?
            };
            tail = Some(Source::Run(Decoder::new(RunReader::new(
                this.buffer,
                file,
            )?)));
        }
        Ok(())
    }
}

/// The chunk that stayed in memory, as a merge input of views.
struct Tail<'a> {
    arena: &'a [u8],
    entries: &'a [Entry],
    at: usize,
    base: u64,
    shape: Shape,
}
impl<'a> Tail<'a> {
    #[inline]
    fn next<S: Storage>(&mut self) -> Option<Record<Mixed<'a, S>>> {
        let entry = self.entries.get(self.at)?;
        if let Some(ahead) = self.entries.get(self.at + PREFETCH) {
            prefetch(self.arena, ahead.offset());
        }
        self.at += 1;
        let (index, fields, view) = arena_record(self.arena, entry.offset(), self.shape);
        Some(Record {
            data: Mixed::View(view),
            prefix: entry.prefix,
            fields,
            sequence: self.base.wrapping_add(index),
            short_key_len: entry.key_len(),
        })
    }
}

/// A merge input: a run, or the chunk that stayed in memory.
enum Source<'a, S: Codec> {
    Run(Decoder<RunReader<File>>),
    Tail(Tail<'a>, PhantomData<S>),
}
impl<'a, S: Codec> Source<'a, S> {
    fn next(&mut self) -> Result<Option<Record<Mixed<'a, S>>>, Failure> {
        match self {
            Self::Run(decoder) => Ok(decoder.next::<S>()?.map(|record| Record {
                data: Mixed::Owned(record.data),
                prefix: record.prefix,
                fields: record.fields,
                sequence: record.sequence,
                short_key_len: record.short_key_len,
            })),
            Self::Tail(tail, _) => Ok(tail.next()),
        }
    }
}

/// Merges the sources into `consume`. Each source is dropped as soon as it is
/// exhausted, which closes (and so frees) an anonymous run file, before the
/// merge ends.
fn merge_into<'a, S: Codec>(
    sources: Vec<Source<'a, S>>,
    mut consume: impl FnMut(Record<Mixed<'a, S>>) -> Result<(), Failure>,
) -> Result<(), Failure> {
    debug_assert!(sources.len() <= MERGE_INPUTS);
    let mut streams = Vec::new();
    streams
        .try_reserve_exact(sources.len())
        .map_err(|_| allocation())?;
    for mut source in sources {
        if let Some(head) = source.next()? {
            streams.push((source, Some(head)));
        }
    }
    while let Some(at) = streams
        .iter()
        .enumerate()
        .filter_map(|(at, (_, head))| head.as_ref().map(|record| (at, record)))
        .min_by(|(_, left), (_, right)| left.compare(right))
        .map(|(at, _)| at)
    {
        consume(streams[at].1.take().unwrap())?;
        match streams[at].0.next()? {
            Some(next) => streams[at].1 = Some(next),
            // `remove` keeps the remaining streams in order.
            None => drop(streams.remove(at)),
        }
    }
    Ok(())
}

/// One record as a run of its own.
#[cfg(test)]
pub(super) fn encode_one<S: Codec>(record: &Record<S>) -> Vec<u8> {
    let mut encoder = Encoder::new(Vec::new());
    encoder.write(record).unwrap();
    encoder.output
}
/// The only record of a run.
#[cfg(test)]
pub(super) fn decode_one<S: Codec>(bytes: &[u8]) -> Result<Record<S>, Failure> {
    let mut decoder = Decoder::new(bytes);
    let record = decoder.next()?.ok_or_else(corrupt)?;
    match decoder.next::<S>()? {
        None => Ok(record),
        Some(_) => Err(corrupt()),
    }
}

#[cfg(test)]
mod tests {
    use super::super::batch::{Batch, Scratch, share};
    use super::*;
    use std::fs::OpenOptions;
    use std::io::{BufReader, BufWriter};
    fn record<S: Codec + Build>(sequence: u64) -> Record<S> {
        let bytes = [(sequence % 3) as u8, b'\t', b'v', 0, 0xff, sequence as u8];
        super::super::project::<S>(
            &bytes,
            &[2],
            &[1],
            super::super::records::Separator::Literal(b'\t'),
            sequence,
            false,
            false,
            super::super::last_selected::<S>(&[2], &[1]),
        )
        .ok()
        .unwrap()
    }
    fn encode_all<'a, S: Codec + Build + 'a>(
        records: impl IntoIterator<Item = &'a Record<S>>,
    ) -> Vec<u8> {
        let mut encoder = Encoder::new(Vec::new());
        for record in records {
            encoder.write(record).unwrap();
        }
        encoder.output
    }
    fn decode_all<S: Codec + Build>(input: impl BufRead) -> Result<Vec<Record<S>>, Failure> {
        let mut decoder = Decoder::new(input);
        let mut records = Vec::new();
        while let Some(record) = decoder.next()? {
            records.push(record);
        }
        Ok(records)
    }
    fn run_file(bytes: &[u8]) -> File {
        let mut file = temporary().ok().unwrap();
        file.write_all(bytes).unwrap();
        file.rewind().unwrap();
        file
    }
    fn assert_same<S: Codec + Build>(left: &Record<S>, right: &Record<S>) {
        assert_eq!(left.sequence, right.sequence);
        assert_eq!(left.fields, right.fields);
        assert_eq!(left.key_count(), right.key_count());
        assert_eq!(left.prefix, right.prefix);
        assert_eq!(left.short_key_len, right.short_key_len);
        let (a, b) = (left.data.segments(), right.data.segments());
        assert_eq!(a.count(), b.count());
        for at in 0..a.count() {
            assert_eq!(a.segment(at), b.segment(at), "segment {at}");
        }
    }
    fn run_sources<S: Codec + Build>(files: Vec<File>) -> Vec<Source<'static, S>> {
        files
            .into_iter()
            .map(|file| Source::Run(Decoder::new(RunReader::new(8 << 10, file).ok().unwrap())))
            .collect()
    }
    /// The shape of `record`'s records.
    const ROW_SHAPE: Shape = Shape {
        keys: 1,
        segments: 2,
    };
    /// `record`'s rows, and their projection.
    fn row(sequence: u64) -> Vec<u8> {
        vec![(sequence % 3) as u8, b'\t', b'v', 0, 0xff, sequence as u8]
    }
    const ROW_JOB: Job<'static> = Job {
        keys: &[1],
        layout: &[2],
        input: super::super::records::Separator::Literal(b'\t'),
        fold: false,
    };
    /// Every record of a sort, described, as it emits them.
    fn emitted<S: Codec + Build>(sorter: Sorter<S>, segments: usize) -> Vec<Described> {
        let mut out = Vec::new();
        sorter
            .finish()
            .emit(|record| {
                out.push(describe(&record, segments));
                Ok(())
            })
            .ok()
            .unwrap();
        out
    }
    fn levels<S: Codec + Build>() {
        for count in [
            0, 1, 3, 6, 7, 9, 10, 12, 13, 15, 16, 21, 23, 24, 45, 46, 48, 49, 51, 52, 193,
        ] {
            let mut sorter = Sorter::<S>::new(usize::MAX, 0);
            let rows: Vec<Vec<u8>> = (0..count).map(row).collect();
            for (sequence, row) in rows.iter().enumerate() {
                ROW_JOB.push(&mut sorter, row, sequence as u64);
                if (sequence + 1).is_multiple_of(3) {
                    sorter.flush().ok().unwrap();
                }
            }
            let expected = ROW_JOB.reference::<S>(&rows);
            assert!(
                emitted(sorter, ROW_JOB.segments::<S>()) == expected,
                "count={count}"
            );
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
            let mut sorter = Sorter::<Packed>::new(usize::MAX, 0);
            MERGED_RECORDS.with(|count| count.set(0));
            // One-record runs: (F-1) merges of F fill the upper level with F-1
            // runs, and F-1 more runs stay in the lower level.
            let fan = MERGE_INPUTS;
            let records = fan * fan - 1;
            for sequence in 0..records as u64 {
                ROW_JOB.push(&mut sorter, &row(sequence), sequence);
                sorter.flush().ok().unwrap();
            }
            assert_eq!(
                sorter.levels.iter().map(Vec::len).collect::<Vec<_>>(),
                [MERGE_INPUTS - 1, MERGE_INPUTS - 1]
            );
            let merged = (fan - 1) * fan;
            MERGED_RECORDS.with(|count| assert_eq!(count.get(), merged));
            if tail {
                ROW_JOB.push(&mut sorter, &row(records as u64), records as u64);
            }
            let mut consumed = 0;
            sorter
                .finish()
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
    fn memory_tail<S: Codec + Build>(levels: usize) {
        // Chunks of three records; MERGE_INPUTS runs fold into the next level,
        // so one flush leaves one level and MERGE_INPUTS + 1 leave a run on
        // each of two levels.
        let flushed = 3 * if levels == 1 { 1 } else { MERGE_INPUTS + 1 };
        for tail in [0, 1, 4] {
            let mut sorter = Sorter::<S>::new(usize::MAX, 0);
            CHUNK_RECORDS.with(|count| count.set(0));
            let rows: Vec<Vec<u8>> = (0..(flushed + tail) as u64).map(row).collect();
            for (sequence, row) in rows.iter().enumerate() {
                ROW_JOB.push(&mut sorter, row, sequence as u64);
                if sequence < flushed && (sequence + 1).is_multiple_of(3) {
                    sorter.flush().ok().unwrap();
                }
            }
            assert_eq!(
                sorter.levels.iter().filter(|runs| !runs.is_empty()).count(),
                levels
            );
            let expected = ROW_JOB.reference::<S>(&rows);
            assert!(
                emitted(sorter, ROW_JOB.segments::<S>()) == expected,
                "levels={levels} tail={tail}"
            );
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
    fn exhausted_merge_inputs_are_released_before_the_merge_ends() {
        // Two runs whose key ranges do not overlap: the first is exhausted
        // (and closed) while the second still has records to merge.
        let mut early: Vec<Record<Packed>> = (0..6).map(record).collect();
        early.retain(|row| row.sequence % 3 == 0);
        let late: Vec<Record<Packed>> = (0..6)
            .map(record)
            .filter(|row| row.sequence % 3 == 2)
            .collect();
        let first = run_file(&encode_all(&early));
        let second = run_file(&encode_all(&late));
        let descriptor = {
            use std::os::fd::AsRawFd;
            first.as_raw_fd()
        };
        // Parallel tests may reuse the descriptor number for another file,
        // and the file system its inode number once the file is gone: a
        // second descriptor keeps the file, so a new one cannot look like it.
        let kept = first.try_clone().unwrap();
        let link = format!("/proc/self/fd/{descriptor}");
        let target = std::fs::read_link(&link).unwrap();
        let open = || std::fs::read_link(&link).is_ok_and(|now| now == target);
        let mut seen = Vec::new();
        merge_into::<Packed>(run_sources(vec![first, second]), |row| {
            seen.push((row.sequence, open()));
            Ok(())
        })
        .ok()
        .unwrap();
        drop(kept);
        // Emitting the first run's last record reads its end, and closes it.
        assert_eq!(seen, [(0, true), (3, true), (2, false), (5, false)]);
    }
    #[test]
    fn memory_tail_failures_propagate_beside_runs() {
        // A truncated run beside the chunk in memory fails the merge.
        let mut sorter = Sorter::<Packed>::new(usize::MAX, 0);
        ROW_JOB.push(&mut sorter, &row(0), 0);
        sorter.flush().ok().unwrap();
        let encoded = encode_one(&record::<Packed>(0));
        sorter.levels[0][0] = run_file(&encoded[..encoded.len() - 1]);
        for sequence in 1..4 {
            ROW_JOB.push(&mut sorter, &row(sequence), sequence);
        }
        assert!(sorter.finish().emit(|_| Ok(())).is_err());
        // A consumer failure stops the emission of the chunk in memory.
        let mut sorter = Sorter::<Packed>::new(usize::MAX, 0);
        for sequence in 1..4 {
            ROW_JOB.push(&mut sorter, &row(sequence), sequence);
        }
        let mut consumed = 0;
        assert!(
            sorter
                .finish()
                .emit(|_| {
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
    fn failures<S: Codec + Build>() {
        let mut bytes = encode_one(&record::<S>(0));
        bytes.push(0);
        let left = run_file(&bytes);
        let mut consumed = 0;
        assert!(
            merge_into::<S>(run_sources(vec![left, temporary().ok().unwrap()]), |_| {
                consumed += 1;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(consumed, 1);
        let left = run_file(&encode_one(&record::<S>(0)));
        assert!(
            merge_into::<S>(run_sources(vec![left, temporary().ok().unwrap()]), |_| Err(
                failure(b"consumer failed\n".to_vec())
            ))
            .is_err()
        );
        let encoded = encode_one(&record::<S>(0));
        for end in 1..encoded.len() {
            assert!(decode_all::<S>(&encoded[..end]).is_err());
        }
        let full = OpenOptions::new().write(true).open("/dev/full").unwrap();
        assert!(Encoder::new(full).write(&record::<S>(0)).is_err());
        let broken = run_file(&encoded[..encoded.len() - 1]);
        let pair = vec![broken, temporary().ok().unwrap()];
        assert!(merge::<S>(pair, ROW_SHAPE, 1024).is_err());
    }
    #[test]
    fn streaming_merge_and_truncated_runs_report_failures() {
        failures::<Packed>();
        failures::<Original>();
    }
    fn full_fan_in_failures<S: Codec + Build>() {
        for broken_at in 0..MERGE_INPUTS {
            let mut runs = Vec::new();
            for at in 0..MERGE_INPUTS {
                let mut encoded = encode_one(&record::<S>(at as u64));
                if at == broken_at {
                    encoded.pop();
                }
                runs.push(run_file(&encoded));
            }
            let mut consumed = 0;
            assert!(
                merge_into::<S>(run_sources(runs), |_| {
                    consumed += 1;
                    Ok(())
                })
                .is_err()
            );
            assert_eq!(consumed, 0);
        }
        let runs = (0..MERGE_INPUTS)
            .map(|at| run_file(&encode_one(&record::<S>(at as u64))))
            .collect();
        let mut consumed = 0;
        let error = merge_into::<S>(run_sources(runs), |_| {
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
        let mut output = RunWriter::new(8 << 10, full).ok().unwrap();
        output.write_all(b"buffered bytes").unwrap();
        assert!(complete(output).is_err());
    }
    fn varints(values: &[u64]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for &value in values {
            let mut buffer = [0; VARINT];
            let end = put(&mut buffer, 0, value);
            bytes.extend_from_slice(&buffer[..end]);
        }
        bytes
    }
    #[test]
    fn invalid_run_metadata_and_length_overflow_are_errors() {
        let huge = u64::MAX;
        // Storage byte, key count, segment count; then sequence, field count
        // and lengths of one record.
        for (storage, rest) in [
            (1, vec![1, 2, 0, 2, 1, 1]),    // packed run read as original
            (0, vec![2, 1, 0, 2, 1]),       // more keys than segments
            (0, vec![huge, huge, 0, 2]),    // unallocatable offsets
            (0, vec![0, 2, 0, 2, huge, 1]), // payload length overflow
            (0, vec![0, 1, 0, 2, huge >> 1]),
            (0, vec![0, 30, 0, 2, huge]), // many segments, on the heap
            (2, vec![0, 1, 0, 2, 0]),     // unknown storage
        ] {
            let mut bytes = vec![storage];
            bytes.extend(varints(&rest));
            assert!(decode_all::<Packed>(bytes.as_slice()).is_err(), "{rest:?}");
        }
        for rest in [vec![1, 3, 0, 1, 1, 1, 1], vec![1, 1, 0, 1, 1]] {
            let mut bytes = vec![1];
            bytes.extend(varints(&rest));
            assert!(
                decode_all::<Original>(bytes.as_slice()).is_err(),
                "{rest:?}"
            );
        }
        // Overlong varints: eleven bytes, or a tenth byte beyond bit 63; both
        // whole in the buffer and split across refills.
        let mut overlong = vec![0, 0, 1];
        overlong.extend([0x80; 10]);
        overlong.push(0);
        let mut high = vec![0, 0, 1];
        high.extend([0xff; 9]);
        high.push(2);
        for bytes in [overlong, high] {
            assert!(decode_all::<Packed>(bytes.as_slice()).is_err());
            assert!(decode_all::<Packed>(BufReader::with_capacity(4, bytes.as_slice())).is_err());
        }
        let packed = encode_one(&record::<Packed>(0));
        assert!(decode_all::<Original>(packed.as_slice()).is_err());
        let original = encode_one(&record::<Original>(0));
        assert!(decode_all::<Packed>(original.as_slice()).is_err());
        // A run holds records of one shape.
        let mut encoder = Encoder::new(Vec::new());
        encoder.write(&record::<Packed>(0)).unwrap();
        let other = Packed::build(b"a\tb", b"a\tb", &[0..1, 2..3], 2, false)
            .ok()
            .unwrap();
        assert!(encoder.write(&Record::new(other, 2, 1)).is_err());
    }
    /// Records of every shape used by the tests below: empty fields and
    /// fields across the one-, two- and three-byte length boundaries.
    fn sample<S: Codec + Build>(state: &mut u64, keys: usize) -> Record<S> {
        const LENGTHS: [usize; 9] = [0, 1, 7, 127, 128, 16383, 16384, 20000, 3];
        let mut next = || {
            *state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *state >> 33
        };
        let segments = if S::ORIGINAL { keys + 1 } else { keys + 2 };
        let mut raw = Vec::new();
        let mut spans = Vec::new();
        for _ in 0..segments {
            let length = LENGTHS[next() as usize % LENGTHS.len()];
            let start = raw.len();
            raw.extend((0..length).map(|at| (at as u64 + next()) as u8));
            spans.push(start..raw.len());
        }
        let data = if S::ORIGINAL {
            // The original is the last span.
            let original = raw[spans[keys].clone()].to_vec();
            S::build(&raw, &original, &spans[..keys], keys, false)
        } else {
            S::build(&raw, &raw, &spans, keys, false)
        }
        .ok()
        .unwrap();
        let sequence = match next() % 4 {
            0 => u64::MAX - next() % 3,
            1 => next() % 3,
            _ => next() << 20 | next(),
        };
        let fields = [0, 1, 300, u64::MAX][next() as usize % 4];
        Record::new(data, fields, sequence)
    }
    fn round_trip<S: Codec + Build>() {
        let mut state = 7;
        for keys in [0, 1, 2, 5] {
            for count in [1, 2, 17] {
                let records: Vec<Record<S>> =
                    (0..count).map(|_| sample(&mut state, keys)).collect();
                let bytes = encode_all(&records);
                for capacity in [1, 3, 8192] {
                    let decoded =
                        decode_all::<S>(BufReader::with_capacity(capacity, bytes.as_slice()))
                            .ok()
                            .unwrap();
                    assert_eq!(decoded.len(), records.len());
                    for (left, right) in records.iter().zip(&decoded) {
                        assert_same(left, right);
                    }
                }
            }
        }
        // More segments than an inline record holds.
        let raw: Vec<u8> = (0..60).collect();
        let spans: Vec<_> = (0..30).map(|at| 2 * at..2 * at + 2).collect();
        let wide = if S::ORIGINAL {
            S::build(&raw, &raw, &spans, 30, false)
        } else {
            S::build(&raw, &raw, &spans, 10, false)
        }
        .ok()
        .unwrap();
        let wide = Record::new(wide, 30, u64::MAX);
        assert_same(&wide, &decode_one::<S>(&encode_one(&wide)).ok().unwrap());
    }
    #[test]
    fn runs_round_trip_lengths_counts_and_sequences_at_varint_boundaries() {
        round_trip::<Packed>();
        round_trip::<Original>();
        for value in [0, 1, 127, 128, 16383, 16384, u64::MAX >> 1, u64::MAX] {
            let bytes = varints(&[value]);
            assert_eq!(varint(&mut bytes.as_slice()).ok().unwrap(), value);
            let mut split = BufReader::with_capacity(1, bytes.as_slice());
            assert_eq!(varint(&mut split).ok().unwrap(), value);
            for delta in [0, 1, u64::MAX, u64::MAX >> 1, (u64::MAX >> 1) + 1] {
                assert_eq!(unzigzag(zigzag(delta)), delta);
            }
        }
        assert_eq!(zigzag(u64::MAX), 1);
        assert_eq!(zigzag(1), 2);
    }
    fn truncation<S: Codec + Build>() {
        let mut state = 11;
        let records: Vec<Record<S>> = (0..6).map(|_| sample(&mut state, 1)).collect();
        let mut encoder = Encoder::new(Vec::new());
        let mut ends = vec![0];
        for record in &records {
            encoder.write(record).unwrap();
            ends.push(encoder.output.len());
        }
        let bytes = encoder.output;
        for end in 0..=bytes.len() {
            let result = decode_all::<S>(BufReader::with_capacity(5, &bytes[..end]));
            match ends.iter().position(|&boundary| boundary == end) {
                Some(complete) => assert_eq!(result.ok().unwrap().len(), complete),
                None => assert!(result.is_err(), "end {end}"),
            }
        }
        // Corrupt bytes decode to an error or to other records, never a panic.
        let small: Vec<Record<S>> = (0..4).map(record).collect();
        let bytes = encode_all(&small);
        for at in 0..bytes.len() {
            for value in [0, 1, 0x7f, 0x80, 0xff, bytes[at] ^ 1] {
                let mut corrupt = bytes.clone();
                corrupt[at] = value;
                let _ = decode_all::<S>(corrupt.as_slice());
            }
        }
    }
    #[test]
    fn truncated_and_corrupt_runs_are_errors_not_panics() {
        truncation::<Packed>();
        truncation::<Original>();
    }
    /// One chunk of the grouped count and collapse memory job, as a run:
    /// rows of a key among 50,000 and two values, from a linear congruential
    /// generator.
    fn typical_run<S: Codec + Build>(rows: usize, layout: &[u64]) -> (usize, usize, usize, usize) {
        let mut x = 12345u64;
        let mut input = 0;
        let mut records = Vec::new();
        // A later chunk: sequences past 2^21 take four bytes as plain varints.
        for sequence in 3_000_000..3_000_000 + rows as u64 {
            x = (x * 69069 + 1) % 4294967296;
            let line = format!(
                "k{:06}\t{}\t{}",
                (x / 65536) % 50000,
                x % 1000,
                (x / 1000) % 977
            );
            input += line.len() + 1;
            records.push(
                super::super::project::<S>(
                    line.as_bytes(),
                    layout,
                    &[1],
                    super::super::records::Separator::Literal(b'\t'),
                    sequence,
                    false,
                    false,
                    super::super::last_selected::<S>(layout, &[1]),
                )
                .ok()
                .unwrap(),
            );
        }
        records.sort_unstable_by(Record::compare);
        let run = encode_all(&records).len();
        // The earlier fixed-width encoding: five u64 headers and a u64 per
        // segment.
        let fixed: usize = records
            .iter()
            .map(|row| 40 + 8 * row.data.segments().count() + row.data.segments().payload().len())
            .sum();
        // The same run with the plain sequence in place of the delta.
        let mut previous = 0;
        let (mut delta, mut plain) = (0, 0);
        for row in &records {
            delta += varints(&[zigzag(row.sequence.wrapping_sub(previous))]).len();
            plain += varints(&[row.sequence]).len();
            previous = row.sequence;
        }
        (input, run, fixed, run - delta + plain)
    }
    #[test]
    fn typical_rows_spill_near_their_input_size() {
        // One chunk of the 6M-row job at the 64 MiB target.
        let rows = 1 << 19;
        // The job's layout holds the group field as well as the value, so its
        // packed records store the key twice. Original records hold the key
        // and the whole line.
        for (name, layout, bound) in [
            ("packed key+value", &[2][..], 1.5),
            ("packed job layout", &[1, 2][..], 1.6),
            ("original", &[][..], 2.0),
        ] {
            let (input, run, fixed, plain) = if layout.is_empty() {
                typical_run::<Original>(rows, layout)
            } else {
                typical_run::<Packed>(rows, layout)
            };
            let per = |bytes: usize| bytes as f64 / rows as f64;
            println!(
                "{name}: input {:.2} B/row; run {:.2} B/record ({:.2}x), plain sequence {:.2}, \
                 fixed-width {:.2} ({:.2}x)",
                per(input),
                per(run),
                run as f64 / input as f64,
                per(plain),
                per(fixed),
                fixed as f64 / input as f64
            );
            assert!(run < plain, "the delta is smaller for repeating keys");
            assert!(
                (run as f64) < bound * input as f64,
                "{name}: {run} vs {input}"
            );
        }
    }
    fn prefix_order<S: Codec + Build>() {
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
                let row = Record::new(data, keys as u64, sequence as u64);
                let decoded = decode_one::<S>(&encode_one(&row)).ok().unwrap();
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
            rows.push(Record::new(data, 2, 0));
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

    /// A sorted record's sequence, field count, key cache and segments.
    type Described = (u64, u64, u64, u8, Vec<Vec<u8>>);
    fn describe<T: Storage>(record: &Record<T>, segments: usize) -> Described {
        (
            record.sequence,
            record.fields,
            record.prefix,
            record.short_key_len,
            (0..segments)
                .map(|at| record.data.segment(at).to_vec())
                .collect(),
        )
    }
    /// A job's projection for chunk tests: keys, layout, separator, folding.
    struct Job<'a> {
        keys: &'a [u64],
        layout: &'a [u64],
        input: super::super::records::Separator,
        fold: bool,
    }
    impl Job<'_> {
        fn segments<S: Storage>(&self) -> usize {
            self.keys.len() + if S::ORIGINAL { 1 } else { self.layout.len() }
        }
        fn push<S: Codec + Build>(&self, sorter: &mut Sorter<S>, row: &[u8], sequence: u64) {
            self.try_push(sorter, row, sequence).ok().unwrap();
        }
        fn try_push<S: Codec + Build>(
            &self,
            sorter: &mut Sorter<S>,
            row: &[u8],
            sequence: u64,
        ) -> Result<(), Failure> {
            let mut spans = Vec::new();
            let last = super::super::last_selected::<S>(self.layout, self.keys);
            let fields = super::super::project_spans::<S>(
                row,
                self.layout,
                self.keys,
                &super::super::key_order(self.keys)?,
                self.input,
                false,
                false,
                last,
                &mut spans,
                None,
            )?;
            sorter.push(
                row,
                row,
                &spans,
                self.keys.len(),
                self.fold,
                fields,
                sequence,
            )
        }
        /// The rows' order under the record comparator, by a stable sort
        /// of independently built records.
        fn reference<S: Codec + Build>(&self, rows: &[Vec<u8>]) -> Vec<Described> {
            let last = super::super::last_selected::<S>(self.layout, self.keys);
            let mut records: Vec<Record<S>> = rows
                .iter()
                .enumerate()
                .map(|(sequence, row)| {
                    super::super::project::<S>(
                        row,
                        self.layout,
                        self.keys,
                        self.input,
                        sequence as u64,
                        false,
                        self.fold,
                        last,
                    )
                    .ok()
                    .unwrap()
                })
                .collect();
            records.sort_by(Record::compare);
            let segments = self.segments::<S>();
            records.iter().map(|row| describe(row, segments)).collect()
        }
        /// The rows as the chunked sorter emits them at `target`.
        fn sorted<S: Codec + Build>(&self, rows: &[Vec<u8>], target: usize) -> Vec<Described> {
            let mut sorter = Sorter::<S>::new(target, 0);
            for (sequence, row) in rows.iter().enumerate() {
                self.push(&mut sorter, row, sequence as u64);
            }
            let segments = self.segments::<S>();
            let mut sorted = sorter.finish();
            let mut out = Vec::new();
            sorted
                .emit(|record| {
                    out.push(describe(&record, segments));
                    Ok(())
                })
                .ok()
                .unwrap();
            out
        }
    }
    /// Rows of `fields` fields from pieces across the prefix boundaries:
    /// empty, short, eight and more bytes, NUL and high bytes, mixed case.
    fn rows(state: &mut u64, count: usize, fields: usize, separator: u8) -> Vec<Vec<u8>> {
        const PIECES: &[&[u8]] = &[
            b"",
            b"a",
            b"A",
            b"ab",
            b"abcdefg",
            b"abcdefgh",
            b"ABCDEFGH",
            b"abcdefghi",
            b"abcdefgh\0",
            b"abcdefgh\0x",
            b"abcdefghij-long-key",
            b"\0",
            b"a\0",
            b"zz",
            b"\xff\xfe",
            b"k01",
        ];
        let mut next = |bound: usize| {
            *state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (*state >> 33) as usize % bound
        };
        (0..count)
            .map(|_| {
                let mut row = Vec::new();
                for field in 0..1 + next(fields) {
                    if field > 0 {
                        row.push(separator);
                    }
                    row.extend_from_slice(PIECES[next(PIECES.len())]);
                }
                row
            })
            .collect()
    }
    fn chunks_match_the_record_order<S: Codec + Build>() {
        use super::super::records::Separator;
        let mut state = 127;
        for (keys, layout) in [
            (&[1u64][..], &[1u64, 2][..]),
            (&[2, 1][..], &[1, 2, 3][..]),
            (&[3][..], &[2, 3][..]),
            (&[1, 3, 1][..], &[1, 3][..]),
        ] {
            for (input, separator) in [
                (Separator::Literal(b'\t'), b'\t'),
                (Separator::Whitespace, b' '),
            ] {
                for fold in [false, true] {
                    let job = Job {
                        keys,
                        layout,
                        input,
                        fold,
                    };
                    let rows = rows(&mut state, 1500, 4, separator);
                    let expected = job.reference::<S>(&rows);
                    for target in [usize::MAX, 1 << 16, 4096, 700, 1] {
                        spilled_memory();
                        assert!(
                            job.sorted::<S>(&rows, target) == expected,
                            "keys {keys:?} fold {fold} target {target}"
                        );
                        // Every chunk of several records spilled within its
                        // part of the target.
                        assert!(spilled_memory() <= chunk_memory(target, 0));
                    }
                }
            }
        }
    }
    #[test]
    fn chunks_emit_the_record_comparator_order_at_every_target() {
        chunks_match_the_record_order::<Packed>();
        chunks_match_the_record_order::<Original>();
    }
    /// Rows whose keys share long prefixes, as dates, numbered names and
    /// timestamps do: ties across eight-byte words, keys ending inside and at
    /// the end of a word, NUL bytes, and keys sharing more than
    /// `REFINE_DEPTH` bytes.
    fn long_key_rows(state: &mut u64, count: usize) -> Vec<Vec<u8>> {
        let mut next = |bound: u64| {
            *state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((*state >> 33) % bound) as usize
        };
        (0..count)
            .map(|n| {
                let mut row = match n % 6 {
                    0 => format!("2026-09-{:02}", next(30) + 1).into_bytes(),
                    1 => format!("prefix00_{:06}", next(5000)).into_bytes(),
                    2 => format!(
                        "2026-09-{:02}T{:02}:{:02}:{:02}",
                        next(3) + 1,
                        next(24),
                        next(4),
                        next(60)
                    )
                    .into_bytes(),
                    3 => {
                        let mut key = vec![b'x'; 60 + next(12)];
                        key.push(b'a' + next(3) as u8);
                        key
                    }
                    4 => {
                        let mut key = b"abcdefgh".repeat(1 + next(3));
                        key.extend_from_slice(&b"\0\0x"[..next(4)]);
                        key
                    }
                    _ => format!("prefix00_{:06}", next(40)).into_bytes()[..9 + next(7)].to_vec(),
                };
                row.extend_from_slice(format!("\t{}\t{n}", next(1000)).as_bytes());
                row
            })
            .collect()
    }
    fn long_keys_match_the_record_order<S: Codec + Build>() {
        use super::super::records::Separator;
        let mut state = 31;
        let rows = long_key_rows(&mut state, 6000);
        for (keys, layout) in [
            (&[1u64][..], &[1u64, 3][..]),
            (&[1, 2][..], &[1, 2, 3][..]),
            (&[2, 1][..], &[1, 2][..]),
        ] {
            for fold in [false, true] {
                let job = Job {
                    keys,
                    layout,
                    input: Separator::Literal(b'\t'),
                    fold,
                };
                let expected = job.reference::<S>(&rows);
                for target in [usize::MAX, 1 << 16, 4096] {
                    assert!(
                        job.sorted::<S>(&rows, target) == expected,
                        "keys {keys:?} fold {fold} target {target}"
                    );
                }
            }
        }
    }
    #[test]
    fn long_keys_that_share_their_prefix_sort_word_by_word() {
        long_keys_match_the_record_order::<Packed>();
        long_keys_match_the_record_order::<Original>();
    }
    #[test]
    fn radix_sort_is_stable_by_prefix_and_key_length() {
        let mut state = 5u64;
        let mut entries: Vec<Entry> = (0..5000u64)
            .map(|at| {
                state = state
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                Entry {
                    prefix: (((state >> 40) % 7) << 56) | ((state >> 20) % 3),
                    meta: (at << 8) | ((state >> 60) % 10),
                }
            })
            .collect();
        let mut expected = entries.clone();
        expected.sort_by_key(Entry::key);
        let mut scratch = vec![Entry::default(); entries.len()];
        let counts = histogram(&entries);
        if radix(&mut entries, &mut scratch, &counts) {
            entries = scratch;
        }
        assert_eq!(entries, expected);
        // A digit every entry shares is skipped; one pass leaves the result
        // in the scratch, which then becomes the entries.
        let mut one: Vec<Entry> = (0..300u64)
            .map(|at| Entry {
                prefix: (at % 5) << 8,
                meta: (at << 8) | 3,
            })
            .collect();
        let mut expected = one.clone();
        expected.sort_by_key(Entry::key);
        let mut scratch = vec![Entry::default(); one.len()];
        let counts = histogram(&one);
        assert!(radix(&mut one, &mut scratch, &counts));
        assert_eq!(scratch, expected);
    }
    #[test]
    fn chunk_memory_stays_within_the_target_and_oversized_records_spill() {
        use super::super::records::Separator;
        let job = Job {
            keys: &[1],
            layout: &[1, 2],
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        let mut state = 9;
        let rows = rows(&mut state, 20000, 3, b'\t');
        // The run buffers scale with the target.
        assert_eq!(run_buffer(64 << 20), 256 << 10);
        assert_eq!(run_buffer(1 << 20), 8 << 10);
        assert_eq!(run_buffer(1), 64);
        assert_eq!(
            chunk_memory(1 << 20, 1 << 16),
            (1 << 20) - (1 << 16) - 9 * (8 << 10)
        );
        assert_eq!(chunk_memory(5000, 0), 5000 - 9 * 64);
        assert_eq!(chunk_memory(500, 0), 0);
        // With the producer's share and the run buffers, the chunk stays
        // within the target at every record, and spills when most of its
        // part is full.
        for (target, share) in [
            (1 << 20, 0),
            (1 << 20, 1 << 16),
            (1 << 16, 0),
            (1 << 16, 4096),
            (20000, 0),
            (5000, 0),
        ] {
            spilled_memory();
            let mut sorter = Sorter::<Packed>::new(target, share);
            let part = chunk_memory(target, share);
            assert_eq!(sorter.limit, part);
            for (sequence, row) in rows.iter().enumerate() {
                job.push(&mut sorter, row, sequence as u64);
                assert!(sorter.memory() <= part, "{target}");
            }
            let spilled = spilled_memory();
            assert!(
                spilled <= part && spilled > part / 10 * 8,
                "{target} {share}: {spilled}"
            );
        }
        // A record larger than the chunk's memory spills at once, and the
        // next chunk starts from nothing again.
        let target = 1600;
        assert_eq!(chunk_memory(target, 0), 1 << 10);
        let mut sorter = Sorter::<Original>::new(target, 0);
        let long = [b'x'; 2000];
        job.push(&mut sorter, &long, 0);
        assert_eq!(sorter.levels.iter().map(Vec::len).sum::<usize>(), 1);
        assert!(sorter.entries.is_empty() && sorter.memory() == 0);
        job.push(&mut sorter, b"a\t1", 1);
        assert_eq!(sorter.levels.iter().map(Vec::len).sum::<usize>(), 1);
        assert_eq!(sorter.entries.len(), 1);
    }
    /// Short rows `k<n>\t<n>` in a scattered key order.
    fn short_rows(count: usize) -> Vec<Vec<u8>> {
        (0..count)
            .map(|n| format!("k{:05}\t{n}", n * 7919 % count).into_bytes())
            .collect()
    }
    #[test]
    fn a_record_as_large_as_the_target_leaves_later_chunks_their_size() {
        use super::super::records::Separator;
        let job = Job {
            keys: &[1],
            layout: &[1, 2],
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        let target = 1 << 16;
        let rows = short_rows(20000);
        let mut long = b"a\t".to_vec();
        long.resize(2 * target, b'x');
        let chunks = |first: Option<&[u8]>| {
            SPILLED_CHUNKS.with(|count| count.set(0));
            let mut sorter = Sorter::<Packed>::new(target, 0);
            let all = first.into_iter().chain(rows.iter().map(Vec::as_slice));
            for (sequence, row) in all.enumerate() {
                job.push(&mut sorter, row, sequence as u64);
            }
            SPILLED_CHUNKS.with(std::cell::Cell::get)
        };
        let plain = chunks(None);
        assert!(plain < 40, "{plain}");
        // The long record spills alone, and its memory is given back.
        assert!(chunks(Some(&long)) <= plain + 2);

        // Through a batch on several threads, as language sorts and rmdup
        // project.
        let shape = Shape {
            keys: 1,
            segments: 2,
        };
        let project = |raw: &[u8], _: &mut Scratch, out: &mut Vec<u8>| {
            let key = raw.split(|&byte| byte == b'\t').next().unwrap_or_default();
            let parts = [key, raw];
            encode(out, 2, (2, key.len() + raw.len()), (1, 0), |at| parts[at])
        };
        for threads in [1, 3] {
            let mut counts = Vec::new();
            for first in [None, Some(&long[..])] {
                SPILLED_CHUNKS.with(|count| count.set(0));
                let mut sorter = Sorter::<Original>::new(target, share(threads, target));
                let mut batch = Batch::new(threads, target);
                let mut accept = |record: Projected<'_>| sorter.push_body(record, shape);
                for row in first.into_iter().chain(rows.iter().map(Vec::as_slice)) {
                    batch.push(row, &project, &mut accept).ok().unwrap();
                }
                batch.finish(&project, &mut accept).ok().unwrap();
                counts.push(SPILLED_CHUNKS.with(std::cell::Cell::get));
            }
            assert!(
                counts[0] < 40 && counts[1] <= counts[0] + 2,
                "{threads}: {counts:?}"
            );
        }
    }
    thread_local! {
        /// The memory and records of each call of the growths below.
        static GROWTH_CALLS: std::cell::RefCell<Vec<(usize, u64)>> =
            const { std::cell::RefCell::new(Vec::new()) };
    }
    fn to_16_mib(memory: usize, records: u64) -> Option<usize> {
        GROWTH_CALLS.with(|calls| calls.borrow_mut().push((memory, records)));
        Some(16 << 20)
    }
    fn double(memory: usize, records: u64) -> Option<usize> {
        GROWTH_CALLS.with(|calls| calls.borrow_mut().push((memory, records)));
        Some(memory * 2)
    }
    fn decline(memory: usize, records: u64) -> Option<usize> {
        GROWTH_CALLS.with(|calls| calls.borrow_mut().push((memory, records)));
        None
    }
    #[test]
    fn a_full_chunk_takes_the_memory_its_growth_gives_until_it_declines() {
        use super::super::records::Separator;
        let job = Job {
            keys: &[1],
            layout: &[1, 2],
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        let rows = short_rows(60000);
        let expected = job.reference::<Packed>(&rows);
        let start = 1 << 16;
        let sorted = |grow: Option<fn(usize, u64) -> Option<usize>>, failures: usize| {
            SPILLED_CHUNKS.with(|count| count.set(0));
            GROWTH_CALLS.with(|calls| calls.borrow_mut().clear());
            let mut sorter = Sorter::<Packed>::new(start, 0);
            sorter.grow = grow;
            let mut limits = vec![sorter.limit];
            for (sequence, row) in rows.iter().enumerate() {
                if sequence == 100 {
                    FAILED_RESERVATIONS.with(|count| count.set(failures));
                }
                job.push(&mut sorter, row, sequence as u64);
                if limits.last() != Some(&sorter.limit) {
                    limits.push(sorter.limit);
                }
                assert!(sorter.memory() <= sorter.limit);
            }
            FAILED_RESERVATIONS.with(|count| count.set(0));
            let spilled = SPILLED_CHUNKS.with(std::cell::Cell::get);
            let segments = job.segments::<Packed>();
            let mut out = Vec::new();
            sorter
                .finish()
                .emit(|record| {
                    out.push(describe(&record, segments));
                    Ok(())
                })
                .ok()
                .unwrap();
            assert!(out == expected);
            (limits, spilled, GROWTH_CALLS.with(|calls| calls.take()))
        };
        // Without a growth the chunk spills within its first memory.
        let (limits, fixed, calls) = sorted(None, 0);
        assert_eq!(limits, [chunk_memory(start, 0)]);
        assert!(fixed > 10, "{fixed}");
        assert!(calls.is_empty());
        // A full chunk takes the memory its growth gives, and holds the rest.
        let (limits, spilled, calls) = sorted(Some(to_16_mib), 0);
        assert_eq!(limits, [chunk_memory(start, 0), 16 << 20]);
        assert_eq!(spilled, 0);
        assert_eq!(calls.len(), 1);
        assert!(
            calls[0].0 <= chunk_memory(start, 0) && calls[0].1 > 1000,
            "{calls:?}"
        );
        // A growth is asked at every fill until the chunk holds the input.
        let (limits, spilled, calls) = sorted(Some(double), 0);
        assert!(
            limits.len() > 3 && calls.len() == limits.len() - 1,
            "{limits:?}"
        );
        assert_eq!(spilled, 0);
        // One that declines leaves the chunk as it was, and is not asked again.
        let (limits, declined, calls) = sorted(Some(decline), 0);
        assert_eq!(limits, [chunk_memory(start, 0)]);
        assert_eq!((declined, calls.len()), (fixed, 1));
        // A chunk that could not grow keeps to half its size, and grows no more.
        let (limits, failed, calls) = sorted(Some(to_16_mib), 1);
        assert!(limits.len() == 2 && limits[1] < limits[0], "{limits:?}");
        assert!(failed > fixed, "{failed} {fixed}");
        assert!(calls.is_empty());
    }
    #[test]
    fn a_chunk_that_cannot_grow_spills_and_retries_once() {
        use super::super::records::Separator;
        let job = Job {
            keys: &[1],
            layout: &[1, 2],
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        let rows: Vec<Vec<u8>> = (0..65)
            .rev()
            .map(|n| format!("k{n}\t{n}").into_bytes())
            .collect();
        let mut spilled = Sorter::<Packed>::new(usize::MAX, 0);
        for (sequence, row) in rows[..64].iter().enumerate() {
            job.push(&mut spilled, row, sequence as u64);
        }
        // The 65th entry needs the chunk to grow, by 64 entry slots (an
        // eighth of the entries is fewer). The chunk spills, gives its memory
        // back, and keeps to half the memory it asked for from then on.
        assert_eq!(spilled.entries.len(), 64);
        assert_eq!(spilled.entries.capacity(), 64);
        let requested = spilled.memory() + 64 * 2 * std::mem::size_of::<Entry>();
        FAILED_RESERVATIONS.with(|count| count.set(1));
        job.push(&mut spilled, &rows[64], 64);
        assert_eq!(spilled.levels.iter().map(Vec::len).sum::<usize>(), 1);
        assert_eq!(spilled.entries.len(), 1);
        assert_eq!(spilled.limit, requested / 2);
        assert!(spilled.memory() <= spilled.limit);
        let segments = job.segments::<Packed>();
        let mut actual = Vec::new();
        spilled
            .finish()
            .emit(|record| {
                actual.push(describe(&record, segments));
                Ok(())
            })
            .ok()
            .unwrap();
        assert!(actual == job.reference::<Packed>(&rows));

        // Nothing to spill, or a second failure after the spill: the chunk
        // refuses.
        let mut empty = Sorter::<Packed>::new(usize::MAX, 0);
        FAILED_RESERVATIONS.with(|count| count.set(1));
        let error = job.try_push(&mut empty, &rows[0], 0).err().unwrap();
        assert_eq!(error.message, allocation().message);
        let mut twice = Sorter::<Packed>::new(usize::MAX, 0);
        for (sequence, row) in rows[..64].iter().enumerate() {
            job.push(&mut twice, row, sequence as u64);
        }
        FAILED_RESERVATIONS.with(|count| count.set(2));
        let error = job.try_push(&mut twice, &rows[64], 64).err().unwrap();
        assert_eq!(error.message, allocation().message);
        assert_eq!(twice.levels.iter().map(Vec::len).sum::<usize>(), 1);
        FAILED_RESERVATIONS.with(|count| count.set(0));
    }
    #[test]
    fn a_large_record_that_cannot_be_stored_leaves_later_chunks_room() {
        use super::super::records::Separator;
        let job = Job {
            keys: &[1],
            layout: &[1, 2],
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        let target = 16 << 20;
        let part = chunk_memory(target, 0);
        let mut long = b"m\t".to_vec();
        long.resize(4 << 20, b'x');
        let rows = short_rows(20000);
        let mut sorter = Sorter::<Packed>::new(target, 0);
        let mut all = Vec::new();
        for row in &rows[..64] {
            job.push(&mut sorter, row, all.len() as u64);
            all.push(row.clone());
        }
        // The long record's growth fails once, with a few KiB in the chunk:
        // the chunk spills and the retry stores the record, which spills
        // alone. Later chunks keep to half the memory the chunk asked for,
        // which the record's growth makes up.
        let held = sorter.memory();
        assert!(held < 16 << 10, "{held}");
        FAILED_RESERVATIONS.with(|count| count.set(1));
        job.push(&mut sorter, &long, all.len() as u64);
        FAILED_RESERVATIONS.with(|count| count.set(0));
        all.push(long.clone());
        assert_eq!(sorter.levels.iter().map(Vec::len).sum::<usize>(), 2);
        assert!(
            sorter.limit > long.len() / 2 && sorter.limit <= part / 2,
            "{}",
            sorter.limit
        );
        // So the short rows, about 1 MiB of chunk, fit in one chunk, where
        // half of the few KiB would have spilled hundreds.
        SPILLED_CHUNKS.with(|count| count.set(0));
        for row in &rows[64..] {
            job.push(&mut sorter, row, all.len() as u64);
            all.push(row.clone());
        }
        assert_eq!(SPILLED_CHUNKS.with(std::cell::Cell::get), 0);
        let segments = job.segments::<Packed>();
        let mut actual = Vec::new();
        sorter
            .finish()
            .emit(|record| {
                actual.push(describe(&record, segments));
                Ok(())
            })
            .ok()
            .unwrap();
        assert!(actual == job.reference::<Packed>(&all));
    }
    #[test]
    fn views_hold_records_of_any_size_and_segment_count() {
        for (length, width) in [(0, 1), (255, 1), (256, 2), (65535, 2), (65536, 4)] {
            assert_eq!(View::width(length), width);
        }
        assert_eq!(View::width(1 << 32), 8);
        use super::super::records::Separator;
        // Forty keys (41 segments) and a record past 64 KiB, with a 2-byte and a
        // 4-byte end table, in memory and through runs.
        let keys: Vec<u64> = (1..=40).collect();
        let layout: Vec<u64> = (1..=41).collect();
        let job = Job {
            keys: &keys,
            layout: &layout,
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        let wide = |n: usize, fill: u8| {
            let mut row: Vec<u8> = (0..40)
                .flat_map(|k| [b'k', b'0' + (k % 3) as u8, b'\t'])
                .collect();
            row.extend(std::iter::repeat_n(fill, n));
            row
        };
        let rows = vec![
            wide(300, b'b'),
            wide(70000, b'a'),
            wide(10, b'c'),
            wide(300, b'a'),
        ];
        for target in [usize::MAX, 1] {
            assert!(job.sorted::<Original>(&rows, target) == job.reference::<Original>(&rows));
            assert!(job.sorted::<Packed>(&rows, target) == job.reference::<Packed>(&rows));
        }
    }
    /// A file under a buffer: the size of each write or read that reaches
    /// it, the bytes written, and the bytes to read (`at` on), at most `most`
    /// bytes a call.
    struct Calls {
        sizes: Vec<usize>,
        bytes: Vec<u8>,
        at: usize,
        most: usize,
    }
    impl Calls {
        fn new(bytes: Vec<u8>, most: usize) -> Self {
            Self {
                sizes: Vec::new(),
                bytes,
                at: 0,
                most,
            }
        }
    }
    impl Write for Calls {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.sizes.push(bytes.len());
            let count = bytes.len().min(self.most);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Read for Calls {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.sizes.push(out.len());
            let count = out.len().min(self.most).min(self.bytes.len() - self.at);
            out[..count].copy_from_slice(&self.bytes[self.at..self.at + count]);
            self.at += count;
            Ok(count)
        }
    }
    #[test]
    fn run_buffers_make_the_file_calls_that_std_buffers_make() {
        let mut state = 3u64;
        let mut next = |bound: usize| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize % bound.max(1)
        };
        for size in [64, 100, 4096] {
            for most in [usize::MAX, 100, 7] {
                // Writes around the buffer's size and its room left, whole
                // and single.
                let mut ours = RunWriter::new(size, Calls::new(Vec::new(), most))
                    .ok()
                    .unwrap();
                let mut theirs = BufWriter::with_capacity(size, Calls::new(Vec::new(), most));
                for round in 0..400 {
                    let spare = theirs.capacity() - theirs.buffer().len();
                    let lengths = [
                        0,
                        1,
                        spare.saturating_sub(1),
                        spare,
                        spare + 1,
                        size,
                        size + 1,
                        3 * size,
                        next(size),
                    ];
                    let length = lengths[next(lengths.len())];
                    let bytes: Vec<u8> = (0..length).map(|at| (at + round) as u8).collect();
                    if next(4) == 0 {
                        assert_eq!(
                            ours.write(&bytes).ok(),
                            theirs.write(&bytes).ok(),
                            "{size} {most} {round}"
                        );
                    } else {
                        ours.write_all(&bytes).unwrap();
                        theirs.write_all(&bytes).unwrap();
                    }
                }
                let ours = ours.into_inner().ok().unwrap();
                let theirs = theirs.into_inner().ok().unwrap();
                assert_eq!(ours.sizes, theirs.sizes, "{size} {most}");
                assert!(ours.bytes == theirs.bytes);

                // Refills, reads and exact reads of every size, to the end
                // and past it.
                let bytes: Vec<u8> = (0..20_000u32).map(|at| (at * 7 % 251) as u8).collect();
                let mut ours = RunReader::new(size, Calls::new(bytes.clone(), most))
                    .ok()
                    .unwrap();
                let mut theirs = BufReader::with_capacity(size, Calls::new(bytes, most));
                for round in 0..600 {
                    let lengths = [0, 1, size - 1, size, size + 1, 3 * size, next(size)];
                    let length = lengths[next(lengths.len())];
                    match next(3) {
                        0 => {
                            let unread = ours.fill_buf().map(<[u8]>::to_vec).ok();
                            assert_eq!(unread, theirs.fill_buf().map(<[u8]>::to_vec).ok());
                            let amount = next(unread.map_or(0, |unread| unread.len()) + 2);
                            ours.consume(amount);
                            theirs.consume(amount);
                        }
                        1 => {
                            let (mut left, mut right) = (vec![0; length], vec![0; length]);
                            assert_eq!(
                                ours.read(&mut left).ok(),
                                theirs.read(&mut right).ok(),
                                "{size} {most} {round}"
                            );
                            assert!(left == right);
                        }
                        _ => {
                            let (mut left, mut right) = (vec![0; length], vec![0; length]);
                            let described = |result: io::Result<()>| {
                                result.map_err(|error| (error.kind(), error.to_string()))
                            };
                            assert_eq!(
                                described(ours.read_exact(&mut left)),
                                described(theirs.read_exact(&mut right)),
                                "{size} {most} {round}"
                            );
                            assert!(left == right);
                        }
                    }
                }
                assert_eq!(ours.into_inner().sizes, theirs.into_inner().sizes);
            }
        }
    }
    #[test]
    fn a_run_buffer_that_cannot_be_allocated_refuses() {
        use super::super::records::Separator;
        let job = Job {
            keys: &[1],
            layout: &[1, 2],
            input: Separator::Literal(b'\t'),
            fold: false,
        };
        // At a 4 KiB target, chunks of a few dozen records spill: runs are
        // written, merged into a second level, and merged with the chunk
        // left in memory, each through run buffers.
        let rows = short_rows(3000);
        let expected = job.reference::<Packed>(&rows);
        let segments = job.segments::<Packed>();
        let sort = || -> Result<Vec<Described>, Failure> {
            let mut sorter = Sorter::<Packed>::new(4096, 0);
            for (sequence, row) in rows.iter().enumerate() {
                job.try_push(&mut sorter, row, sequence as u64)?;
            }
            let mut out = Vec::new();
            sorter.finish().emit(|record| {
                out.push(describe(&record, segments));
                Ok(())
            })?;
            Ok(out)
        };
        let (mut refusals, mut completed) = (0, false);
        for at in 0..10_000 {
            FAILING_RUN_BUFFER.with(|count| count.set(Some(at)));
            let result = sort();
            let failed = FAILING_RUN_BUFFER
                .with(|count| count.replace(None))
                .is_none();
            match result {
                Err(error) => {
                    assert!(failed);
                    assert_eq!(error.status, 77);
                    assert_eq!(error.message, allocation().message);
                    refusals += 1;
                }
                Ok(out) => {
                    assert!(!failed && out == expected);
                    completed = true;
                    break;
                }
            }
        }
        assert!(completed && refusals > 4 * RUN_BUFFERS, "{refusals}");
    }
}
