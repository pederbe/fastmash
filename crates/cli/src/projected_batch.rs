//! Parallel projection: raw records wait in a batch, are projected on up to
//! `threads` threads once enough have arrived, and are handed on in input
//! order, encoded for a sort chunk. The first failure in input order wins, as
//! when projecting one record at a time, so results and diagnostics do not
//! depend on the thread count. With one thread each record is projected at
//! once, into a buffer the batch reuses.
use super::{Failure, KeyCache, allocation, storage};
use std::ops::Range;

// Records, or raw bytes, per thread below which a batch is projected on the
// calling thread: a part must be worth its thread, whether its records are
// many or long.
const MIN_PART: usize = if cfg!(test) { 2 } else { 4096 };
const MIN_PART_BYTES: usize = if cfg!(test) { 64 } else { 64 << 10 };
// A batch is projected once it holds this many records, or once its raw
// bytes, with `PER_RECORD` bytes for each record, reach a sixth of its share
// of the sort's memory (`share`): a record's projected body is at most about
// five times the record, and each record's end and key cache take 32 bytes.
const RECORDS: usize = if cfg!(test) { 5 } else { 65536 };
const RAW_SHARE: usize = 6;
const PER_RECORD: usize =
    (std::mem::size_of::<usize>() + std::mem::size_of::<(u64, u8, usize)>()).div_ceil(RAW_SHARE);
// Reused buffers larger than this are given back after a record passes.
const REUSED: usize = 64 << 10;

/// The share of a sort's memory `target` that batches projecting on `threads`
/// threads hold: raw records and their projected bodies, a sixteenth of the
/// target, at most 48 MiB. One thread projects each record at once and holds
/// nothing for long.
pub(super) fn share(threads: usize, target: usize) -> usize {
    if threads <= 1 {
        0
    } else {
        (target / 16).min(48 << 20)
    }
}

/// Per-thread scratch space for projection.
#[derive(Default)]
pub(super) struct Scratch {
    pub spans: Vec<Range<usize>>,
    pub sort_keys: storage::SortKeys,
    pub language: storage::LanguageScratch,
}

impl Scratch {
    /// Gives back buffers that a large record grew.
    fn trim(&mut self) {
        let language = &mut self.language;
        if language.bytes.capacity() > REUSED {
            language.bytes = Vec::new();
        }
        if language.text.folded.capacity() > REUSED {
            language.text.folded = String::new();
        }
        if language.text.composed.capacity() > REUSED {
            language.text.composed = String::new();
        }
    }
}

/// A record projected for a sort chunk: its encoded body (`spill::encode`),
/// its key cache and its sequence number.
pub(super) struct Projected<'a> {
    pub(super) body: &'a [u8],
    pub(super) prefix: u64,
    pub(super) key_len: u8,
    pub(super) sequence: u64,
}

/// The projected records of one part of a batch, in input order: their
/// bodies, each one's key cache and end, and the failure that stopped the
/// part, if one did.
#[derive(Default)]
struct Part {
    bytes: Vec<u8>,
    records: Vec<(u64, u8, usize)>,
    failure: Option<Failure>,
}

pub(super) struct Batch {
    bytes: Vec<u8>,
    ends: Vec<usize>,
    threads: usize,
    /// The raw bytes at which a batch is projected.
    limit: usize,
    next_sequence: u64,
    scratch: Scratch,
    single: Vec<u8>,
}

impl Batch {
    /// A batch projecting on up to `threads` threads (at least one), for a
    /// sort whose memory target is `target`.
    pub(super) fn new(threads: usize, target: usize) -> Self {
        let threads = threads.max(1);
        Self {
            bytes: Vec::new(),
            ends: Vec::new(),
            threads,
            limit: (share(threads, target) / RAW_SHARE).max(1),
            next_sequence: 0,
            scratch: Scratch::default(),
            single: Vec::new(),
        }
    }

    /// Takes the next raw record; projected records reach `accept` in input
    /// order: at once with one thread, a batch at a time otherwise. `project`
    /// appends a record's body to its buffer and returns the record's key
    /// cache.
    pub(super) fn push(
        &mut self,
        record: &[u8],
        project: &(impl Fn(&[u8], &mut Scratch, &mut Vec<u8>) -> Result<KeyCache, Failure> + Sync),
        mut accept: impl FnMut(Projected<'_>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        if self.threads == 1 {
            self.single.clear();
            let (prefix, key_len) = project(record, &mut self.scratch, &mut self.single)?;
            let sequence = self.next_sequence;
            self.next_sequence = sequence.checked_add(1).ok_or_else(allocation)?;
            let result = accept(Projected {
                body: &self.single,
                prefix,
                key_len,
                sequence,
            });
            if self.single.capacity() > REUSED {
                self.single = Vec::new();
            }
            self.scratch.trim();
            return result;
        }
        self.bytes
            .try_reserve(record.len())
            .map_err(|_| allocation())?;
        self.bytes.extend_from_slice(record);
        self.ends.try_reserve(1).map_err(|_| allocation())?;
        self.ends.push(self.bytes.len());
        let held = self.bytes.len() + self.ends.len() * PER_RECORD;
        if self.ends.len() >= RECORDS || held >= self.limit {
            self.finish(project, accept)?;
        }
        Ok(())
    }

    /// Projects the waiting records (numbered from the batch's first sequence
    /// number) and hands them to `accept` in input order.
    pub(super) fn finish(
        &mut self,
        project: &(impl Fn(&[u8], &mut Scratch, &mut Vec<u8>) -> Result<KeyCache, Failure> + Sync),
        mut accept: impl FnMut(Projected<'_>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let count = self.ends.len();
        let first = self.next_sequence;
        let (bytes, ends) = (&self.bytes, &self.ends);
        let record = |at: usize| -> &[u8] {
            let start = if at == 0 { 0 } else { ends[at - 1] };
            &bytes[start..ends[at]]
        };
        // Stops at the first failure: later records in the part are never used.
        let part = |from: usize, to: usize| -> Part {
            let mut scratch = Scratch::default();
            let mut out = Part::default();
            if out.records.try_reserve_exact(to - from).is_err() {
                out.failure = Some(allocation());
                return out;
            }
            for at in from..to {
                match project(record(at), &mut scratch, &mut out.bytes) {
                    Ok((prefix, key_len)) => out.records.push((prefix, key_len, out.bytes.len())),
                    Err(failure) => {
                        out.failure = Some(failure);
                        break;
                    }
                }
            }
            out
        };
        let worth = (count / MIN_PART).max(bytes.len() / MIN_PART_BYTES);
        let size = count.div_ceil(self.threads.min(worth).min(count).max(1));
        // Recounted from the size, so that no part starts past the end.
        let parts = count.div_ceil(size.max(1)).max(1);
        let results = if parts == 1 {
            vec![part(0, count)]
        } else {
            std::thread::scope(|scope| {
                let spawned: Vec<_> = (0..parts)
                    .map(|p| {
                        let (from, to) = (p * size, ((p + 1) * size).min(count));
                        let part = &part;
                        // A part whose thread cannot start runs here instead.
                        std::thread::Builder::new()
                            .spawn_scoped(scope, move || part(from, to))
                            .map_err(|_| (from, to))
                    })
                    .collect();
                spawned
                    .into_iter()
                    .map(|handle| match handle {
                        Ok(handle) => handle.join().expect("projection thread"),
                        Err((from, to)) => part(from, to),
                    })
                    .collect::<Vec<_>>()
            })
        };
        let mut sequence = first;
        for part in results {
            let mut start = 0;
            for &(prefix, key_len, end) in &part.records {
                accept(Projected {
                    body: &part.bytes[start..end],
                    prefix,
                    key_len,
                    sequence,
                })?;
                sequence += 1;
                start = end;
            }
            if let Some(failure) = part.failure {
                return Err(failure);
            }
        }
        self.next_sequence = first.checked_add(count as u64).ok_or_else(allocation)?;
        self.bytes.clear();
        self.ends.clear();
        // A batch that a large record grew gives its memory back.
        if self.bytes.capacity() > self.limit.saturating_mul(2).max(REUSED) {
            self.bytes = Vec::new();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(batch: &mut Batch, rows: &[&[u8]]) -> Result<Vec<(u64, Vec<u8>)>, Failure> {
        let mut seen = Vec::new();
        for row in rows {
            batch.push(row, &project, |r| {
                seen.push((r.sequence, r.body.to_vec()));
                Ok(())
            })?;
        }
        batch.finish(&project, |r| {
            seen.push((r.sequence, r.body.to_vec()));
            Ok(())
        })?;
        Ok(seen)
    }
    fn project(bytes: &[u8], _: &mut Scratch, out: &mut Vec<u8>) -> Result<KeyCache, Failure> {
        if bytes == b"bad" {
            return Err(super::super::failure(
                format!("bad record {}\n", String::from_utf8_lossy(bytes)).into_bytes(),
            ));
        }
        out.extend_from_slice(bytes);
        Ok((bytes.len() as u64, 9))
    }

    #[test]
    fn records_keep_input_order_and_sequence_on_any_thread_count() {
        let rows: Vec<Vec<u8>> = (0..23).map(|i| format!("k{i}\t{i}").into_bytes()).collect();
        let rows: Vec<&[u8]> = rows.iter().map(Vec::as_slice).collect();
        let expected: Vec<(u64, Vec<u8>)> = rows
            .iter()
            .enumerate()
            .map(|(at, row)| (at as u64, row.to_vec()))
            .collect();
        for threads in [1, 2, 3, 8] {
            let seen = numbers(&mut Batch::new(threads, usize::MAX), &rows)
                .ok()
                .unwrap();
            assert_eq!(seen, expected, "{threads}");
        }
    }

    #[test]
    fn the_first_failure_in_input_order_wins() {
        let mut rows: Vec<&[u8]> = vec![b"a\t1"; 12];
        rows[3] = b"bad";
        rows[9] = b"bad";
        for threads in [1, 4] {
            let mut accepted = 0;
            let mut batch = Batch::new(threads, usize::MAX);
            let mut result = Ok(());
            for row in &rows {
                result = batch.push(row, &project, |_| {
                    accepted += 1;
                    Ok(())
                });
                if result.is_err() {
                    break;
                }
            }
            if result.is_ok() {
                result = batch.finish(&project, |_| {
                    accepted += 1;
                    Ok(())
                });
            }
            assert_eq!(
                result.err().unwrap().message,
                b"bad record bad\n",
                "{threads}"
            );
            assert_eq!(accepted, 3, "{threads}");
        }
    }

    #[test]
    fn batches_of_long_records_are_projected_on_several_threads() {
        use std::{collections::HashSet, sync::Mutex};
        let threads = Mutex::new(HashSet::new());
        let tracked = |bytes: &[u8], scratch: &mut Scratch, out: &mut Vec<u8>| {
            threads.lock().unwrap().insert(std::thread::current().id());
            project(bytes, scratch, out)
        };
        // Two records are too few for two parts by count, not by bytes.
        let mut batch = Batch::new(4, usize::MAX);
        for row in [[b'a'; 300], [b'b'; 300]] {
            batch.push(&row, &tracked, |_| Ok(())).ok().unwrap();
        }
        batch.finish(&tracked, |_| Ok(())).ok().unwrap();
        let threads = threads.into_inner().unwrap();
        assert_eq!(threads.len(), 2);
        assert!(!threads.contains(&std::thread::current().id()));
    }

    #[test]
    fn every_record_is_projected_once_however_the_batch_is_split() {
        // Long records split a batch by bytes into more parts than its
        // records fill evenly: 5 records on 4 threads make parts of 2, 2
        // and 1, and no part may start past the end.
        for threads in 1..=8 {
            for count in 1..=24 {
                let rows: Vec<Vec<u8>> = (0..count)
                    .map(|i| vec![b'a' + (i % 26) as u8; 100 + i])
                    .collect();
                let rows: Vec<&[u8]> = rows.iter().map(Vec::as_slice).collect();
                let expected: Vec<(u64, Vec<u8>)> = rows
                    .iter()
                    .enumerate()
                    .map(|(at, row)| (at as u64, row.to_vec()))
                    .collect();
                let seen = numbers(&mut Batch::new(threads, usize::MAX), &rows)
                    .ok()
                    .unwrap();
                assert_eq!(seen, expected, "{threads} threads, {count} records");
            }
        }
    }

    #[test]
    fn batches_scale_with_their_share_and_give_back_large_buffers() {
        // A 64 KiB target on four threads: a 4 KiB share, projected in
        // batches of about 682 raw bytes, three records of 300 bytes.
        let target = 64 << 10;
        assert_eq!(share(4, target), 4096);
        assert_eq!(share(1, target), 0);
        assert_eq!(share(8, usize::MAX), 48 << 20);
        let mut batch = Batch::new(4, target);
        let mut sizes = Vec::new();
        let mut seen = 0;
        for _ in 0..90 {
            batch
                .push(&[b'x'; 300], &project, |_| {
                    seen += 1;
                    Ok(())
                })
                .ok()
                .unwrap();
            sizes.push(batch.bytes.len());
        }
        assert_eq!(seen, 90);
        assert!(sizes.iter().all(|&size| size < 900), "{sizes:?}");
        // A record larger than the batch goes through alone, and the batch
        // gives its buffer back.
        batch
            .push(&vec![b'y'; 1 << 20], &project, |record| {
                assert_eq!(record.body.len(), 1 << 20);
                Ok(())
            })
            .ok()
            .unwrap();
        assert!(batch.bytes.capacity() <= REUSED);
        let mut single = Batch::new(1, target);
        single
            .push(&vec![b'z'; 1 << 20], &project, |_| Ok(()))
            .ok()
            .unwrap();
        assert!(single.single.capacity() <= REUSED);
    }
}
