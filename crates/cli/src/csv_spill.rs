//! Private disposable CSV runs. Each frame holds six little-endian u64 values
//! (original Record/line, field/key counts, payload/key byte lengths), checked
//! end tables, then decoded payloads. Its preceding u64 is the body length.
//! Run bytes are never fed to the CSV parser or ordinary text field splitting.
use super::super::spill::{MERGE_INPUTS, RunReader, RunWriter, temporary};
use super::{Keys, Order, Record, Stored, StoredView, capacity};
use crate::{Failure, command_memory, csv_input::Location, failure};
use std::{
    fs::File,
    io::{self, Read, Seek, Write},
};

fn io_error(error: impl std::fmt::Display) -> Failure {
    failure(format!("sort temporary I/O error: {error}\n").into_bytes())
}
fn corrupt() -> Failure {
    Failure {
        status: 70,
        message: b"internal CSV sort run failure\n".to_vec(),
    }
}
fn read_error(error: io::Error) -> Failure {
    if error.kind() == io::ErrorKind::UnexpectedEof {
        corrupt()
    } else {
        io_error(error)
    }
}
// The Command harness controls input/output transports. These test-only hooks
// cover otherwise inaccessible temporary I/O, without environment switches.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunFault {
    Write,
    Read,
    Merge,
}
#[cfg(test)]
thread_local! {
    static FAULT: std::cell::Cell<Option<(RunFault, usize, i32)>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(crate) fn fail_run(fault: Option<(RunFault, usize, i32)>) {
    FAULT.with(|slot| slot.set(fault));
}
#[cfg(test)]
fn check(fault: RunFault) -> Result<(), Failure> {
    FAULT.with(|slot| match slot.get() {
        Some((expected, 0, code)) if expected == fault => {
            slot.set(None);
            Err(io_error(io::Error::from_raw_os_error(code)))
        }
        Some((expected, at, code)) if expected == fault => {
            slot.set(Some((expected, at - 1, code)));
            Ok(())
        }
        _ => Ok(()),
    })
}

fn write(output: &mut impl Write, stored: StoredView<'_>) -> Result<(), Failure> {
    #[cfg(test)]
    check(RunFault::Write)?;
    let record = stored.record;
    let keys = stored.keys;
    let table = record
        .ends
        .len()
        .checked_add(keys.ends.len())
        .and_then(|n| n.checked_mul(8))
        .ok_or_else(capacity)?;
    let length = 48usize
        .checked_add(table)
        .and_then(|n| n.checked_add(record.bytes.len()))
        .and_then(|n| n.checked_add(keys.bytes.len()))
        .ok_or_else(capacity)?;
    for value in [
        length as u64,
        record.location.record,
        record.location.line,
        record.ends.len() as u64,
        keys.ends.len() as u64,
        record.bytes.len() as u64,
        keys.bytes.len() as u64,
    ] {
        output.write_all(&value.to_le_bytes()).map_err(io_error)?;
    }
    for &end in record.ends.iter().chain(keys.ends) {
        output
            .write_all(&(end as u64).to_le_bytes())
            .map_err(io_error)?;
    }
    output.write_all(record.bytes).map_err(io_error)?;
    output.write_all(keys.bytes).map_err(io_error)
}

struct Decoder<R> {
    input: R,
    remaining: u64,
    key_count: usize,
}
impl<R: Read + Seek> Decoder<R> {
    fn new(input: R, remaining: u64, key_count: usize) -> Self {
        Self {
            input,
            remaining,
            key_count,
        }
    }
    fn number(&mut self) -> Result<u64, Failure> {
        let mut bytes = [0; 8];
        self.input.read_exact(&mut bytes).map_err(read_error)?;
        Ok(u64::from_le_bytes(bytes))
    }
    fn validate_ends(&mut self, count: usize, bytes: usize) -> Result<(), Failure> {
        let mut previous = 0;
        for _ in 0..count {
            let end = usize::try_from(self.number()?).map_err(|_| corrupt())?;
            if end < previous || end > bytes {
                return Err(corrupt());
            }
            previous = end;
        }
        if previous != bytes {
            return Err(corrupt());
        }
        Ok(())
    }
    fn ends(&mut self, count: usize) -> Result<Vec<usize>, Failure> {
        let mut ends = Vec::new();
        command_memory::reserve_exact(&mut ends, count)?;
        for _ in 0..count {
            ends.push(usize::try_from(self.number()?).map_err(|_| corrupt())?);
        }
        Ok(ends)
    }
    fn bytes(&mut self, count: usize) -> Result<Vec<u8>, Failure> {
        let mut bytes = Vec::new();
        command_memory::reserve_exact(&mut bytes, count)?;
        bytes.resize(count, 0);
        self.input.read_exact(&mut bytes).map_err(read_error)?;
        Ok(bytes)
    }
    fn next(&mut self) -> Result<Option<Stored>, Failure> {
        #[cfg(test)]
        check(RunFault::Read)?;
        if self.remaining == 0 {
            return Ok(None);
        }
        // Verify every declared shape/size against bytes actually in the run,
        // before any Record storage is reserved. Short frames are Internal.
        if self.remaining < 8 {
            return Err(corrupt());
        }
        let frame = self.number()?;
        if frame < 48 || frame > self.remaining - 8 {
            return Err(corrupt());
        }
        let record = self.number()?;
        let line = self.number()?;
        let fields = self.number()?;
        let keys = self.number()?;
        let payload = self.number()?;
        let key_bytes = self.number()?;
        if record == 0 || line < record || fields == 0 || keys != self.key_count as u64 {
            return Err(corrupt());
        }
        let length = fields
            .checked_add(keys)
            .and_then(|n| n.checked_mul(8))
            .and_then(|n| n.checked_add(48))
            .and_then(|n| n.checked_add(payload))
            .and_then(|n| n.checked_add(key_bytes));
        if length != Some(frame) {
            return Err(corrupt());
        }
        let fields = usize::try_from(fields).map_err(|_| corrupt())?;
        let payload = usize::try_from(payload).map_err(|_| corrupt())?;
        let key_bytes = usize::try_from(key_bytes).map_err(|_| corrupt())?;
        // End tables are checked in a bounded read pass before even reserving
        // their vectors, so a corrupt endpoint cannot become Capacity. Only
        // the private immutable anonymous file is revisited, never the input.
        let table = self.input.stream_position().map_err(io_error)?;
        self.validate_ends(fields, payload)?;
        self.validate_ends(self.key_count, key_bytes)?;
        self.input
            .seek(io::SeekFrom::Start(table))
            .map_err(io_error)?;
        let ends = self.ends(fields)?;
        let key_ends = self.ends(self.key_count)?;
        let bytes = self.bytes(payload)?;
        let key_data = self.bytes(key_bytes)?;
        self.remaining -= frame + 8;
        Ok(Some(Stored {
            record: Record {
                bytes,
                ends,
                location: Location { record, line },
            },
            keys: Keys {
                bytes: key_data,
                ends: key_ends,
            },
        }))
    }
}

fn key_count(order: &Order<'_>) -> Result<usize, Failure> {
    if order.language {
        order.keys.len().checked_mul(2).ok_or_else(capacity)
    } else {
        Ok(0)
    }
}
fn decoder(
    file: File,
    buffer: usize,
    order: &Order<'_>,
) -> Result<Decoder<RunReader<File>>, Failure> {
    let length = file.metadata().map_err(io_error)?.len();
    Ok(Decoder::new(
        RunReader::new(buffer, file)?,
        length,
        key_count(order)?,
    ))
}
fn complete(output: RunWriter<File>) -> Result<File, Failure> {
    let mut file = output.into_inner().map_err(io_error)?;
    file.rewind().map_err(io_error)?;
    Ok(file)
}

struct Source {
    decoder: Decoder<RunReader<File>>,
    head: Option<Stored>,
}
fn merge_into(
    files: Vec<File>,
    buffer: usize,
    order: &Order<'_>,
    mut consume: impl FnMut(Stored) -> Result<(), Failure>,
) -> Result<(), Failure> {
    #[cfg(test)]
    check(RunFault::Merge)?;
    let mut sources = Vec::new();
    command_memory::reserve_exact(&mut sources, files.len())?;
    for file in files {
        let mut decoder = decoder(file, buffer, order)?;
        let head = decoder.next()?;
        sources.push(Source { decoder, head });
    }
    loop {
        let mut best: Option<usize> = None;
        for (at, source) in sources.iter().enumerate() {
            if let Some(head) = &source.head
                && best.is_none_or(|prior| {
                    order
                        .compare(head, sources[prior].head.as_ref().unwrap())
                        .is_lt()
                })
            {
                best = Some(at);
            }
        }
        let Some(at) = best else {
            return Ok(());
        };
        let source = &mut sources[at];
        consume(source.head.take().unwrap())?;
        source.head = source.decoder.next()?;
    }
}
fn merge(files: Vec<File>, buffer: usize, order: &Order<'_>) -> Result<File, Failure> {
    let mut output = RunWriter::new(buffer, temporary()?)?;
    merge_into(files, buffer, order, |stored| {
        write(&mut output, stored.view())
    })?;
    complete(output)
}

/// At most seven runs per level. A full level folds to the next immediately,
/// bounding live descriptors logarithmically and merge heads to eight Records.
pub(super) struct Runs {
    levels: Vec<Vec<File>>,
    pub(super) buffer: usize,
}
impl Runs {
    pub(super) fn new(buffer: usize) -> Self {
        Self {
            levels: Vec::new(),
            buffer,
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }
    pub(super) fn write<'r>(
        &self,
        values: impl Iterator<Item = StoredView<'r>>,
    ) -> Result<File, Failure> {
        let mut output = RunWriter::new(self.buffer, temporary()?)?;
        for stored in values {
            write(&mut output, stored)?;
        }
        complete(output)
    }
    pub(super) fn add(&mut self, mut run: File, order: &Order<'_>) -> Result<(), Failure> {
        let mut at = 0;
        loop {
            if at == self.levels.len() {
                command_memory::reserve(&mut self.levels, 1)?;
                self.levels.push(Vec::new());
            }
            command_memory::reserve(&mut self.levels[at], 1)?;
            self.levels[at].push(run);
            if self.levels[at].len() < MERGE_INPUTS {
                return Ok(());
            }
            run = merge(std::mem::take(&mut self.levels[at]), self.buffer, order)?;
            at += 1;
        }
    }
    pub(super) fn emit(
        self,
        order: &Order<'_>,
        consume: impl FnMut(Stored) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let mut levels = self
            .levels
            .into_iter()
            .filter(|files| !files.is_empty())
            .peekable();
        let mut tail = None;
        while let Some(mut files) = levels.next() {
            if let Some(run) = tail.take() {
                command_memory::reserve(&mut files, 1)?;
                files.push(run);
            }
            if levels.peek().is_none() {
                return merge_into(files, self.buffer, order, consume);
            }
            tail = Some(if files.len() == 1 {
                files.pop().unwrap()
            } else {
                merge(files, self.buffer, order)?
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn run_reader_seek_keeps_frame_positions_after_buffered_read_ahead() {
        let frame: Vec<u8> = [64u64, 1, 1, 1, 0, 8, 0, 8]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .chain(*b"abcdefgh")
            .collect();
        let bytes = frame.repeat(3);
        for buffer in [1, 8, 17, 64, 256] {
            let reader = RunReader::new(buffer, io::Cursor::new(&bytes))
                .unwrap_or_else(|_| panic!("buffer"));
            let mut input = Decoder::new(reader, bytes.len() as u64, 0);
            for at in 1..=3 {
                assert_eq!(
                    input
                        .next()
                        .unwrap_or_else(|_| panic!("frame {at}"))
                        .unwrap()
                        .record
                        .bytes,
                    b"abcdefgh"
                );
                assert_eq!(
                    input.input.stream_position().unwrap(),
                    (at * frame.len()) as u64
                );
            }
            assert!(input.next().unwrap_or_else(|_| panic!("end")).is_none());
        }
    }
    #[test]
    fn invalid_frames_are_internal_before_payload_allocation() {
        let header = [64u64, 1, 1, 1, 0, 8, 0, 8];
        let valid: Vec<u8> = header
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .chain(*b"abcdefgh")
            .collect();
        for end in 1..valid.len() {
            let mut input = Decoder::new(io::Cursor::new(&valid[..end]), end as u64, 0);
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(0)));
            assert_eq!(input.next().err().unwrap().status, 70, "truncated at {end}");
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        }
        for (at, value) in [
            (0, u64::MAX),
            (0, 47),
            (1, 0),
            (2, 0),
            (3, 0),
            (3, u64::MAX),
            (4, 1),
            (5, u64::MAX),
            (7, 9),
            (7, 7),
        ] {
            let mut bytes = valid.clone();
            bytes[at * 8..at * 8 + 8].copy_from_slice(&value.to_le_bytes());
            let mut input = Decoder::new(io::Cursor::new(bytes.as_slice()), bytes.len() as u64, 0);
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(0)));
            assert_eq!(
                input.next().err().unwrap().status,
                70,
                "word {at} value {value}"
            );
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        }
        let mut input = Decoder::new(io::Cursor::new(valid.as_slice()), valid.len() as u64, 0);
        assert_eq!(
            input
                .next()
                .unwrap_or_else(|_| panic!("valid frame"))
                .unwrap()
                .record
                .bytes,
            b"abcdefgh"
        );
        assert!(
            input
                .next()
                .unwrap_or_else(|_| panic!("end of run"))
                .is_none()
        );
    }

    #[test]
    fn language_segment_frames_are_checked_before_payload_allocation() {
        // One complete Field and two language segments, all with relative ends.
        let valid: Vec<u8> = [79u64, 1, 1, 1, 2, 3, 4, 3, 2, 4]
            .into_iter()
            .flat_map(u64::to_le_bytes)
            .chain(*b"a,bKEYS")
            .collect();
        for (at, value) in [(4, 1), (6, u64::MAX), (8, 5), (9, 1), (9, 3)] {
            let mut bytes = valid.clone();
            bytes[at * 8..at * 8 + 8].copy_from_slice(&value.to_le_bytes());
            let mut input = Decoder::new(io::Cursor::new(&bytes), bytes.len() as u64, 2);
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(0)));
            assert_eq!(input.next().err().unwrap().status, 70);
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
        }
        let mut input = Decoder::new(io::Cursor::new(&valid), valid.len() as u64, 2);
        let stored = input
            .next()
            .unwrap_or_else(|_| panic!("valid frame"))
            .unwrap();
        assert_eq!(stored.record.bytes, b"a,b");
        assert_eq!(stored.record.ends, [3]);
        assert_eq!(stored.keys.bytes, b"KEYS");
        assert_eq!(stored.keys.ends, [2, 4]);
        assert!(input.next().unwrap_or_else(|_| panic!("end")).is_none());
    }
}
