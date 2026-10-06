//! Input that can be read again: [`Rewind`] for input that can return to an
//! earlier point, such as standard input on a regular file, and [`Replay`],
//! which holds input that cannot, such as a pipe, so that hash grouping can
//! give it to a sort to read again (ADR 0008).

use super::standard_io;
use std::io::{self, BufRead};

/// Input that can be read again from an earlier point: standard input on a
/// regular file, and in-memory test input. Other input cannot rewind.
pub(super) trait Rewind: BufRead {
    /// A mark at the next unread byte, or `None` when the input cannot be
    /// read again from it.
    fn mark(&mut self) -> Option<u64> {
        None
    }
    /// Reads on from `mark` again.
    fn rewind(&mut self, _mark: u64) -> io::Result<()> {
        Err(io::ErrorKind::Unsupported.into())
    }
    /// The input's length in bytes, where known.
    fn length(&mut self) -> Option<u64> {
        None
    }
    /// The memory held to read the input again ([`Replay`]).
    fn held(&self) -> usize {
        0
    }
}

/// The size of a [`Replay`] block.
pub(super) const BLOCK: usize = 1 << 20;

#[cfg(test)]
thread_local! {
    /// A smaller block, to test reading across blocks.
    static TEST_BLOCK: std::cell::Cell<usize> = const { std::cell::Cell::new(BLOCK) };
    /// Whether the next block cannot be allocated.
    static TEST_NO_BLOCK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Sets this thread's [`Replay`] block size, for tests.
#[cfg(test)]
pub(super) fn test_block(size: usize) {
    TEST_BLOCK.with(|block| block.set(size));
}

/// The size of the next [`Replay`] block.
fn block_size() -> usize {
    #[cfg(test)]
    return TEST_BLOCK.with(std::cell::Cell::get);
    #[cfg(not(test))]
    BLOCK
}

/// A zeroed block of `size` bytes, or `None` where it cannot be allocated.
fn new_block(size: usize) -> Option<Vec<u8>> {
    #[cfg(test)]
    if TEST_NO_BLOCK.with(std::cell::Cell::get) {
        return None;
    }
    let mut block = Vec::new();
    block.try_reserve_exact(size).ok()?;
    block.resize(size, 0);
    Some(block)
}

/// Input that cannot rewind, such as a pipe, held as it is read so that it
/// can be read once more from its start, or any other input, passed through.
/// Hash grouping reads through it; where it gives up, the sort reads the held
/// blocks, each released once read, and then the rest. The end of input and a
/// read error are held too and met again in their place, so the second reading
/// sees what the first saw and never reads past an end it met. Where a block
/// cannot be allocated, reading fails with nothing held lost.
pub(super) struct Replay<'a, R> {
    inner: &'a mut R,
    state: State,
}

enum State {
    Pass,
    Hold(Held),
    Again(Held),
}

#[derive(Default)]
struct Held {
    /// Full-size blocks and the bytes read into each.
    blocks: std::collections::VecDeque<(Vec<u8>, usize)>,
    /// The next unread byte of the front block (again) or the back one (hold).
    at: usize,
    /// The bytes the blocks take.
    bytes: usize,
    /// While read again, the bytes of the blocks not yet released, shared
    /// with the sort that reads them ([`Replay::hand_over`]).
    unread: Unread,
    /// How the input ended while held, if it did.
    end: Option<End>,
}

enum End {
    Eof,
    Error(Option<i32>, io::ErrorKind),
}

impl<R> Replay<'_, R> {
    /// Whether the input is being held to be read again.
    pub(super) fn holding(&self) -> bool {
        matches!(self.state, State::Hold(_))
    }
}

/// The bytes a [`Replay`] still holds while its input is read again.
pub(super) type Unread = Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>;

impl<'a, R: Rewind> Replay<'a, R> {
    /// Holds `inner` where `hold` and it cannot rewind; else passes it through.
    pub(super) fn new(inner: &'a mut R, hold: bool) -> Self {
        let state = if hold && inner.mark().is_none() {
            State::Hold(Held::default())
        } else {
            State::Pass
        };
        Self { inner, state }
    }

    /// Hands the input to a sort: input still held is read again from where
    /// holding began, and no more is held, so a sort never reads on while
    /// holding. Gives the bytes held to be read again, which the sort's
    /// memory must leave room for.
    pub(super) fn hand_over(&mut self) -> io::Result<Unread> {
        if self.holding() {
            self.rewind(0)?;
        }
        Ok(match &self.state {
            State::Again(held) => held.unread.clone(),
            _ => None,
        })
    }
}

impl<R: BufRead> io::Read for Replay<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        self.consume(n);
        Ok(n)
    }
}

impl<R: BufRead> BufRead for Replay<'_, R> {
    // Input passed through, the common case, costs one branch.
    #[inline]
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if let State::Pass = self.state {
            return self.inner.fill_buf();
        }
        self.fill_held()
    }

    #[inline]
    fn consume(&mut self, amount: usize) {
        match &mut self.state {
            State::Pass => self.inner.consume(amount),
            State::Hold(held) | State::Again(held) => held.at += amount,
        }
    }
}

impl<R: BufRead> Replay<'_, R> {
    #[inline(never)]
    fn fill_held(&mut self) -> io::Result<&[u8]> {
        // Settle the state first, then lend the bytes.
        match &mut self.state {
            State::Pass => {}
            State::Hold(held) => {
                let unread = held
                    .blocks
                    .back()
                    .is_some_and(|&(_, filled)| held.at < filled);
                if !unread {
                    match held.end {
                        Some(End::Eof) => return Ok(&[]),
                        Some(End::Error(os, kind)) => return Err(error(os, kind)),
                        None => {}
                    }
                    if held
                        .blocks
                        .back()
                        .is_none_or(|(block, filled)| *filled == block.len())
                    {
                        let size = block_size();
                        let block = new_block(size)
                            .ok_or_else(|| io::Error::from(io::ErrorKind::OutOfMemory))?;
                        held.blocks.push_back((block, 0));
                        held.bytes += size;
                        held.at = 0;
                    }
                    let (block, filled) = held.blocks.back_mut().expect("a block");
                    match self.inner.read(&mut block[*filled..]) {
                        Ok(0) => {
                            held.end = Some(End::Eof);
                            return Ok(&[]);
                        }
                        Ok(n) => *filled += n,
                        Err(e) => {
                            held.end = Some(End::Error(e.raw_os_error(), e.kind()));
                            return Err(e);
                        }
                    }
                }
            }
            State::Again(held) => {
                while held
                    .blocks
                    .front()
                    .is_some_and(|&(_, filled)| held.at == filled)
                {
                    if let Some((block, _)) = held.blocks.pop_front()
                        && let Some(unread) = &held.unread
                    {
                        unread.fetch_sub(block.len(), std::sync::atomic::Ordering::Relaxed);
                    }
                    held.at = 0;
                }
                if held.blocks.is_empty() {
                    match held.end {
                        // Held on, so that no read goes past the end.
                        Some(End::Eof) => return Ok(&[]),
                        Some(End::Error(os, kind)) => {
                            self.state = State::Pass;
                            return Err(error(os, kind));
                        }
                        None => self.state = State::Pass,
                    }
                }
            }
        }
        match &self.state {
            State::Pass => self.inner.fill_buf(),
            State::Hold(held) => {
                let (block, filled) = held.blocks.back().expect("a block");
                Ok(&block[held.at..*filled])
            }
            State::Again(held) => {
                let (block, filled) = held.blocks.front().expect("a block");
                Ok(&block[held.at..*filled])
            }
        }
    }
}

impl<R: Rewind> Rewind for Replay<'_, R> {
    fn mark(&mut self) -> Option<u64> {
        match self.state {
            State::Pass => self.inner.mark(),
            // Held input is read again from where holding began.
            State::Hold(ref held) if held.blocks.is_empty() => Some(0),
            _ => None,
        }
    }
    fn rewind(&mut self, mark: u64) -> io::Result<()> {
        match std::mem::replace(&mut self.state, State::Pass) {
            State::Pass => self.inner.rewind(mark),
            State::Hold(mut held) if mark == 0 => {
                held.at = 0;
                held.unread = Some(std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(
                    held.bytes,
                )));
                self.state = State::Again(held);
                Ok(())
            }
            state => {
                self.state = state;
                Err(io::ErrorKind::Unsupported.into())
            }
        }
    }
    fn length(&mut self) -> Option<u64> {
        match self.state {
            State::Pass => self.inner.length(),
            _ => None,
        }
    }
    fn held(&self) -> usize {
        match &self.state {
            State::Hold(held) => held.bytes,
            _ => 0,
        }
    }
}

fn error(os: Option<i32>, kind: io::ErrorKind) -> io::Error {
    os.map_or_else(|| kind.into(), io::Error::from_raw_os_error)
}

impl Rewind for &[u8] {}

impl<T: AsRef<[u8]>> Rewind for io::Cursor<T> {
    fn mark(&mut self) -> Option<u64> {
        Some(self.position())
    }
    fn rewind(&mut self, mark: u64) -> io::Result<()> {
        self.set_position(mark);
        Ok(())
    }
    fn length(&mut self) -> Option<u64> {
        Some(self.get_ref().as_ref().len() as u64)
    }
}

impl Rewind for io::BufReader<standard_io::Stdin> {
    fn mark(&mut self) -> Option<u64> {
        standard_io::regular_input_length()?;
        io::Seek::stream_position(self).ok()
    }
    fn rewind(&mut self, mark: u64) -> io::Result<()> {
        io::Seek::seek(self, io::SeekFrom::Start(mark)).map(drop)
    }
    fn length(&mut self) -> Option<u64> {
        standard_io::regular_input_length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Input that cannot rewind, reading out `script` in order: bytes, the
    /// end of input (empty) or an error, then whatever follows, as a
    /// terminal or a FIFO with a second writer can.
    struct Script {
        steps: std::collections::VecDeque<io::Result<Vec<u8>>>,
        current: Vec<u8>,
        at: usize,
    }

    impl Script {
        fn new(steps: Vec<io::Result<Vec<u8>>>) -> Self {
            Self {
                steps: steps.into(),
                current: Vec::new(),
                at: 0,
            }
        }
    }

    impl io::Read for Script {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let available = self.fill_buf()?;
            let n = available.len().min(buf.len());
            buf[..n].copy_from_slice(&available[..n]);
            self.consume(n);
            Ok(n)
        }
    }

    impl BufRead for Script {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.at == self.current.len() {
                match self.steps.pop_front() {
                    Some(Ok(bytes)) => (self.current, self.at) = (bytes, 0),
                    Some(Err(error)) => return Err(error),
                    None => (self.current, self.at) = (Vec::new(), 0),
                }
            }
            Ok(&self.current[self.at..])
        }
        fn consume(&mut self, amount: usize) {
            self.at += amount;
        }
    }

    impl Rewind for Script {}

    /// Reads `reader` to its end or its first error.
    fn drain(reader: &mut impl BufRead, limit: usize) -> (Vec<u8>, Option<i32>) {
        let mut out = Vec::new();
        while out.len() < limit {
            match reader.fill_buf() {
                Ok([]) => return (out, None),
                Ok(bytes) => {
                    let n = bytes.len().min(limit - out.len());
                    out.extend_from_slice(&bytes[..n]);
                    reader.consume(n);
                }
                Err(error) => return (out, error.raw_os_error()),
            }
        }
        (out, None)
    }

    #[test]
    fn held_input_is_read_again_exactly_once_rewound() {
        // Several blocks, in pieces of every size.
        let input: Vec<u8> = (0..3 * BLOCK + 12_345).map(|i| (i % 251) as u8).collect();
        let pieces = input
            .chunks(65_536 + 7)
            .map(|piece| Ok(piece.to_vec()))
            .collect();
        let mut script = Script::new(pieces);
        let mut replay = Replay::new(&mut script, true);
        assert_eq!(replay.mark(), Some(0));
        assert_eq!(replay.length(), None);
        let (first, _) = drain(&mut replay, BLOCK + 99);
        assert_eq!(first, input[..BLOCK + 99]);
        assert_eq!(replay.held(), 2 * BLOCK);
        replay.rewind(0).unwrap();
        assert_eq!(replay.held(), 0);
        assert_eq!(drain(&mut replay, usize::MAX), (input, None));
        // Rewindable input passes through, and so does input not held.
        let mut file = io::Cursor::new(b"abc".to_vec());
        let mut replay = Replay::new(&mut file, true);
        assert_eq!(replay.mark(), Some(0));
        assert_eq!(replay.length(), Some(3));
        let mut script = Script::new(vec![Ok(b"abc".to_vec())]);
        let mut replay = Replay::new(&mut script, false);
        assert_eq!(replay.mark(), None);
        assert_eq!(drain(&mut replay, usize::MAX), (b"abc".to_vec(), None));
    }

    #[test]
    fn a_sort_reads_held_input_again_and_holds_no_more() {
        let input: Vec<u8> = (0..2 * BLOCK + 77).map(|i| (i % 251) as u8).collect();
        // Handed over part way, as if hash grouping had given up without
        // rewinding, or before anything was read.
        for read in [BLOCK + 5, 0] {
            let pieces = input
                .chunks(65_536)
                .map(|piece| Ok(piece.to_vec()))
                .collect();
            let mut script = Script::new(pieces);
            let mut replay = Replay::new(&mut script, true);
            assert_eq!(drain(&mut replay, read).0, input[..read]);
            let held = replay.held();
            let unread = replay.hand_over().unwrap().expect("held input");
            assert!(!replay.holding());
            assert_eq!(unread.load(std::sync::atomic::Ordering::Relaxed), held);
            assert_eq!(drain(&mut replay, usize::MAX), (input.clone(), None));
            assert_eq!(unread.load(std::sync::atomic::Ordering::Relaxed), 0);
        }
        // Input passed through is handed over as it is.
        let mut file = io::Cursor::new(b"abc".to_vec());
        let mut replay = Replay::new(&mut file, true);
        assert_eq!(drain(&mut replay, 1).0, b"a");
        assert!(replay.hand_over().unwrap().is_none());
        assert_eq!(drain(&mut replay, usize::MAX).0, b"bc");
    }

    #[test]
    fn the_end_and_errors_are_met_again_in_their_place() {
        // Nothing after an end of input is read, even where more would come.
        let mut script = Script::new(vec![
            Ok(b"a\n".to_vec()),
            Ok(Vec::new()),
            Ok(b"b\n".to_vec()),
        ]);
        let mut replay = Replay::new(&mut script, true);
        assert_eq!(drain(&mut replay, usize::MAX), (b"a\n".to_vec(), None));
        replay.rewind(0).unwrap();
        assert_eq!(drain(&mut replay, usize::MAX), (b"a\n".to_vec(), None));
        assert_eq!(drain(&mut replay, usize::MAX), (Vec::new(), None));
        // An error ends the first reading and the second at the same byte;
        // what follows it is read only after.
        let mut script = Script::new(vec![
            Ok(b"a\nb".to_vec()),
            Err(io::Error::from_raw_os_error(5)),
            Ok(b"c\n".to_vec()),
        ]);
        let mut replay = Replay::new(&mut script, true);
        assert_eq!(drain(&mut replay, usize::MAX), (b"a\nb".to_vec(), Some(5)));
        replay.rewind(0).unwrap();
        assert_eq!(drain(&mut replay, usize::MAX), (b"a\nb".to_vec(), Some(5)));
        assert_eq!(drain(&mut replay, usize::MAX), (b"c\n".to_vec(), None));
        // Only where holding began can it be read again.
        let mut script = Script::new(vec![Ok(b"a".to_vec())]);
        let mut replay = Replay::new(&mut script, true);
        drain(&mut replay, 1);
        assert_eq!(replay.mark(), None);
        assert!(replay.rewind(1).is_err());
        assert!(replay.rewind(0).is_ok());
    }

    #[test]
    fn a_block_that_cannot_be_allocated_fails_the_read_and_loses_nothing() {
        TEST_BLOCK.with(|size| size.set(4));
        let mut script = Script::new(vec![Ok(b"abcdefghij".to_vec())]);
        let mut replay = Replay::new(&mut script, true);
        let (first, _) = drain(&mut replay, 8);
        assert_eq!(first, b"abcdefgh");
        assert_eq!(replay.held(), 8);
        TEST_NO_BLOCK.with(|fails| fails.set(true));
        let error = replay.fill_buf().map(<[u8]>::to_vec).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::OutOfMemory);
        TEST_NO_BLOCK.with(|fails| fails.set(false));
        TEST_BLOCK.with(|size| size.set(BLOCK));
        // The held bytes, then what was never read, as if nothing had failed.
        replay.rewind(0).unwrap();
        assert_eq!(
            drain(&mut replay, usize::MAX),
            (b"abcdefghij".to_vec(), None)
        );
    }
}
