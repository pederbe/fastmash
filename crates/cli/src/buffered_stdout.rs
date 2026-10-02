//! Standard output, buffered as GNU datamash's stdio buffers it: a bounded
//! buffer (the output's block size, line-buffered on a terminal) over an
//! unbuffered, borrowed transport. Every Mode writes through it. It keeps the
//! first write error and what finalization needs, so write failures are
//! reported in GNU's order (`command_output::complete`).
use super::headers::Part;
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IoFailure {
    pub code: Option<i32>,
    pub kind: io::ErrorKind,
}
impl From<io::Error> for IoFailure {
    fn from(error: io::Error) -> Self {
        Self {
            code: error.raw_os_error(),
            kind: error.kind(),
        }
    }
}

#[derive(Debug)]
pub(crate) struct Completion {
    pub first_error: Option<IoFailure>,
    pub flush_error: Option<IoFailure>,
    pub close_error: Option<IoFailure>,
    pub pending_before_finish: bool,
    pub unsupported_write: bool,
}

pub(crate) struct BufferedStdout<'a, W> {
    writer: &'a mut W,
    pending: Vec<u8>,
    capacity: usize,
    line_buffered: bool,
    active: bool,
    first_error: Option<IoFailure>,
    unsupported_write: bool,
}

impl<'a, W: Write> BufferedStdout<'a, W> {
    pub fn new(writer: &'a mut W, capacity: usize, line_buffered: bool) -> io::Result<Self> {
        if !(1..=8192).contains(&capacity) {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        Ok(Self {
            writer,
            pending: Vec::with_capacity(capacity),
            capacity,
            line_buffered,
            active: false,
            first_error: None,
            unsupported_write: false,
        })
    }

    fn write_bytes(&mut self, mut bytes: &[u8]) -> Result<(), IoFailure> {
        while !bytes.is_empty() {
            let failure = match self.writer.write(bytes) {
                Ok(0) => io::Error::from(io::ErrorKind::WriteZero).into(),
                Ok(n) if n <= bytes.len() => {
                    bytes = &bytes[n..];
                    continue;
                }
                Ok(_) => io::Error::from(io::ErrorKind::InvalidData).into(),
                Err(error) => error.into(),
            };
            self.first_error.get_or_insert(failure);
            self.unsupported_write |= !matches!(failure.code, Some(9 | 28));
            return Err(failure);
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), IoFailure> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let mut bytes = std::mem::take(&mut self.pending);
        let result = self.write_bytes(&bytes);
        // A failed transfer consumes this buffer; later logical calls start anew.
        bytes.clear();
        self.pending = bytes;
        result
    }

    fn character(&mut self, byte: u8) -> Result<(), IoFailure> {
        self.active = true;
        if self.pending.len() == self.capacity {
            self.flush()?;
        }
        self.pending.push(byte);
        if self.line_buffered && byte == b'\n' {
            self.flush()?;
        }
        Ok(())
    }

    fn string(&mut self, bytes: &[u8]) -> Result<(), IoFailure> {
        if bytes.is_empty() {
            return Ok(());
        }
        let space = if self.active {
            self.capacity - self.pending.len()
        } else {
            0
        };
        let newline = if self.line_buffered && space >= bytes.len() {
            bytes.iter().rposition(|b| *b == b'\n')
        } else {
            None
        };
        let take = newline.map_or(space.min(bytes.len()), |at| at + 1);
        self.pending.extend_from_slice(&bytes[..take]);
        let rest = &bytes[take..];
        if !rest.is_empty() || newline.is_some() {
            self.active = true;
            self.flush()?;
            let direct = if self.capacity < 128 {
                rest.len()
            } else {
                rest.len() / self.capacity * self.capacity
            };
            self.write_bytes(&rest[..direct])?;
            for &byte in &rest[direct..] {
                self.character(byte)?;
            }
        }
        Ok(())
    }

    fn writable(&self) -> bool {
        self.active && !self.line_buffered && self.pending.len() < self.capacity
    }

    fn prefixed(&mut self, prefix: u8, name: &[u8]) -> Result<(), IoFailure> {
        let length = name.len() + 1;
        let mut at = 0;
        let mut stage = if self.writable() { 0 } else { 128 };
        while at < length {
            if stage != 0 {
                let mut bytes = [0; 128];
                let n = stage.min(length - at);
                for (offset, byte) in bytes[..n].iter_mut().enumerate() {
                    *byte = if at + offset == 0 {
                        prefix
                    } else {
                        name[at + offset - 1]
                    };
                }
                if stage == 1 {
                    self.character(bytes[0])?;
                } else {
                    self.string(&bytes[..n])?;
                }
                at += n;
                stage = if self.writable() { 0 } else { 128 };
            } else {
                let n = (self.capacity - self.pending.len()).min(length - at);
                let end = at + n;
                if at == 0 {
                    self.pending.push(prefix);
                    at += 1;
                }
                self.pending.extend_from_slice(&name[at - 1..end - 1]);
                at = end;
                if self.pending.len() == self.capacity {
                    stage = 1;
                }
            }
        }
        Ok(())
    }

    pub fn emit(&mut self, part: Part<'_>) {
        // Errors belong to finalization; rendering must continue to later fields.
        let _ = match part {
            Part::Operation(bytes) => self.string(bytes),
            Part::Name(bytes) => self.prefixed(b'(', bytes),
            Part::PairName(bytes) => self.prefixed(b',', bytes),
            Part::Parameter(bytes) => self.prefixed(b':', bytes),
            Part::Close => self.character(b')'),
            Part::Separator(byte) => self.character(byte),
        };
    }

    pub fn before_diagnostic(&mut self) {
        let _ = self.flush();
    }

    pub fn formatted(&mut self, bytes: &[u8]) {
        let _ = self.string(bytes);
    }

    /// A full fwrite span, including any embedded NUL bytes.
    pub fn raw(&mut self, bytes: &[u8]) {
        let _ = self.string(bytes);
    }

    /// GNU emits GroupBy(name) as one printf call with its own staging buffer.
    pub fn group_header(&mut self, name: &[u8]) {
        let mut bytes = b"GroupBy(".to_vec();
        bytes.extend_from_slice(name);
        bytes.push(b')');
        let mut at = 0;
        let mut stage = if self.writable() { 0 } else { 128 };
        while at < bytes.len() {
            if stage == 0 {
                let n = (self.capacity - self.pending.len()).min(bytes.len() - at);
                self.pending.extend_from_slice(&bytes[at..at + n]);
                at += n;
                if self.pending.len() == self.capacity {
                    stage = 1;
                }
            } else {
                let n = stage.min(bytes.len() - at);
                let result = if stage == 1 {
                    self.character(bytes[at])
                } else {
                    self.string(&bytes[at..at + n])
                };
                if result.is_err() {
                    break;
                }
                at += n;
                stage = if self.writable() { 0 } else { 128 };
            }
        }
    }

    pub fn finish(mut self, close: impl FnOnce(&mut W) -> io::Result<()>) -> Completion {
        let pending_before_finish = !self.pending.is_empty();
        let flush_error = self.flush().err();
        let close_error = close(self.writer).err().map(IoFailure::from);
        Completion {
            first_error: self.first_error,
            flush_error,
            close_error,
            pending_before_finish,
            unsupported_write: self.unsupported_write,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, headers::render};
    use std::collections::VecDeque;

    enum Step {
        Short(usize),
        Error(i32),
        Zero,
    }
    #[derive(Default)]
    struct Sink {
        bytes: Vec<u8>,
        attempts: Vec<Vec<u8>>,
        steps: VecDeque<Step>,
        full: bool,
        closes: usize,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.attempts.push(bytes.to_vec());
            let n = match self.steps.pop_front() {
                Some(Step::Short(n)) => n.min(bytes.len()),
                Some(Step::Error(code)) => return Err(io::Error::from_raw_os_error(code)),
                Some(Step::Zero) => return Ok(0),
                None if self.full => return Err(io::Error::from_raw_os_error(28)),
                None => bytes.len(),
            };
            self.bytes.extend_from_slice(&bytes[..n]);
            Ok(n)
        }
        fn flush(&mut self) -> io::Result<()> {
            panic!("transport is unbuffered")
        }
    }
    fn close(sink: &mut Sink) -> io::Result<()> {
        sink.closes += 1;
        Ok(())
    }

    #[test]
    fn percentile_parameter_crosses_small_buffers() {
        for capacity in 1..=8 {
            for percent in [1, 95, 99] {
                for line_buffered in [false, true] {
                    let mut sink = Sink::default();
                    let mut output =
                        BufferedStdout::new(&mut sink, capacity, line_buffered).unwrap();
                    let result = render(
                        b"value",
                        &[(Kind::Percentile(percent), 1)],
                        crate::records::Separator::Literal(b'\t'),
                        b'\t',
                        true,
                        1,
                        |part| output.emit(part),
                    );
                    assert!(result.is_ok());
                    let completion = output.finish(close);
                    assert!(completion.first_error.is_none());
                    assert_eq!(sink.bytes, format!("perc:{percent}(value)\n").as_bytes());
                }
            }
        }
    }

    #[test]
    fn percentile_parameter_failure_keeps_later_header_calls() {
        for remaining in 0..=2 {
            let mut sink = Sink {
                steps: VecDeque::from([Step::Short(1), Step::Error(28)]),
                ..Sink::default()
            };
            let mut output = BufferedStdout::new(&mut sink, 4, false).unwrap();
            output.emit(Part::Operation(b"p"));
            output.raw(&vec![b'x'; 4 - remaining]);
            output.emit(Part::Parameter(b"95"));
            output.emit(Part::Name(b"value"));
            output.emit(Part::Close);
            output.emit(Part::Separator(b'\n'));
            let completion = output.finish(close);
            assert_eq!(completion.first_error.unwrap().code, Some(28));
            assert!(completion.flush_error.is_none());
            assert_eq!(sink.bytes, b"p(value)\n");
            assert_eq!(
                sink.attempts[1],
                [b"xxxx".as_slice(), b"xxx:", b"xx:9"][remaining]
            );
        }
    }

    #[test]
    fn pipe_and_terminal_progress_match_reference_observations() {
        for (line, capacity) in [(false, 4096), (true, 1024)] {
            for (length, count, held) in [
                (1, 1, if line { 7 } else { 0 }),
                (8192, 1, if line { 8198 } else { 8192 }),
                (1, 2, if line { 14 } else { 0 }),
            ] {
                let mut sink = Sink::default();
                let mut output = BufferedStdout::new(&mut sink, capacity, line).unwrap();
                assert!(
                    render(
                        &vec![b'a'; length],
                        &vec![(Kind::Sum, 1); count],
                        super::super::records::Separator::Literal(b'\t'),
                        b'\n',
                        true,
                        1,
                        |p| output.emit(p)
                    )
                    .is_ok()
                );
                assert_eq!(output.writer.bytes.len(), held);
                let result = output.finish(close);
                assert!(result.first_error.is_none() && result.close_error.is_none());
                let label = format!("sum({})", "a".repeat(length));
                assert_eq!(
                    sink.bytes,
                    (vec![label; count].join("\n") + "\n").as_bytes()
                );
                assert_eq!(sink.closes, 1);
            }
        }
    }

    #[test]
    fn full_device_error_depends_on_final_pending_bytes() {
        for (length, pending) in [(4085, true), (4086, true), (4087, false), (8192, true)] {
            let mut sink = Sink {
                full: true,
                ..Sink::default()
            };
            let mut output = BufferedStdout::new(&mut sink, 4096, false).unwrap();
            assert!(
                render(
                    &vec![b'a'; length],
                    &[(Kind::Geomean, 1)],
                    super::super::records::Separator::Literal(b'\t'),
                    b'\t',
                    true,
                    1,
                    |p| output.emit(p)
                )
                .is_ok()
            );
            let result = output.finish(close);
            assert_eq!(result.pending_before_finish, pending);
            assert_eq!(result.first_error.unwrap().code, Some(28));
            assert_eq!(result.flush_error.is_some(), pending);
            assert!(result.close_error.is_none());
        }
    }

    #[test]
    fn diagnostic_flush_retains_primary_error_and_plain_secondary_state() {
        let mut sink = Sink {
            full: true,
            ..Sink::default()
        };
        let mut output = BufferedStdout::new(&mut sink, 4096, false).unwrap();
        let error = render(
            b"a",
            &[(Kind::Sum, 1), (Kind::Mean, 2)],
            super::super::records::Separator::Literal(b'\t'),
            b'\t',
            true,
            7,
            |p| output.emit(p),
        )
        .err()
        .unwrap();
        assert_eq!(
            error.message,
            b"invalid input: field 2 requested, line 7 has only 1 fields\n"
        );
        output.before_diagnostic();
        let result = output.finish(close);
        assert_eq!(result.first_error.unwrap().code, Some(28));
        assert!(!result.pending_before_finish);
        assert!(result.flush_error.is_none() && result.close_error.is_none());
        assert_eq!(sink.attempts, vec![b"sum(a)\t".to_vec()]);
    }

    #[test]
    fn short_writes_continue_with_the_unwritten_suffix() {
        let mut sink = Sink {
            steps: VecDeque::from([Step::Short(1), Step::Short(2)]),
            ..Sink::default()
        };
        let mut output = BufferedStdout::new(&mut sink, 4, false).unwrap();
        output.emit(Part::Name(b"abc"));
        let result = output.finish(close);
        assert!(result.first_error.is_none());
        assert_eq!(sink.bytes, b"(abc");
        assert_eq!(
            sink.attempts,
            vec![b"(abc".to_vec(), b"abc".to_vec(), b"c".to_vec()]
        );
    }

    #[test]
    fn full_buffer_name_staging_preserves_later_close_after_recovery() {
        let mut sink = Sink {
            steps: VecDeque::from([Step::Short(128), Step::Error(28)]),
            ..Sink::default()
        };
        let mut output = BufferedStdout::new(&mut sink, 128, false).unwrap();
        for name in [&vec![b'a'; 119][..], &vec![b'b'; 127][..]] {
            output.emit(Part::Operation(b"sum"));
            output.emit(Part::Name(name));
            output.emit(Part::Close);
            output.emit(Part::Separator(if name[0] == b'a' { b'\t' } else { b'\n' }));
        }
        let result = output.finish(close);
        assert_eq!(
            sink.attempts.iter().map(Vec::len).collect::<Vec<_>>(),
            [128, 128, 2]
        );
        assert_eq!(
            sink.bytes,
            format!("sum({})\tsum)\n", "a".repeat(119)).as_bytes()
        );
        assert_eq!(result.first_error.unwrap().code, Some(28));
        assert!(result.flush_error.is_none());
    }

    #[test]
    fn initial_string_and_subsequent_name_respect_capacity_boundaries() {
        for capacity in [1, 127, 128, 129, 1024, 4096, 8192] {
            let mut sink = Sink::default();
            let mut output = BufferedStdout::new(&mut sink, capacity, false).unwrap();
            output.before_diagnostic();
            output.emit(Part::Operation(b"sum"));
            assert_eq!(output.writer.attempts.len(), usize::from(capacity < 128));
            if capacity < 128 {
                assert_eq!(output.writer.attempts[0], b"sum");
            }
            output.emit(Part::Name(&vec![b'a'; 260]));
            output.emit(Part::Close);
            output.emit(Part::Separator(b'\n'));
            assert!(output.finish(close).first_error.is_none());
            assert_eq!(sink.bytes, format!("sum({})\n", "a".repeat(260)).as_bytes());
        }
    }

    #[test]
    fn line_buffer_string_uses_last_newline_when_transfer_fits() {
        let mut sink = Sink::default();
        let mut output = BufferedStdout::new(&mut sink, 129, true).unwrap();
        output.emit(Part::Operation(b"sum"));
        output.emit(Part::Name(b"a\nb\nc"));
        assert_eq!(output.writer.attempts, vec![b"sum(a\nb\n".to_vec()]);
        assert!(output.finish(close).first_error.is_none());
        assert_eq!(sink.bytes, b"sum(a\nb\nc");
    }

    #[test]
    fn interrupted_and_zero_writes_are_not_retried() {
        for step in [Step::Error(4), Step::Zero] {
            let mut sink = Sink {
                steps: VecDeque::from([step]),
                ..Sink::default()
            };
            let mut output = BufferedStdout::new(&mut sink, 4096, false).unwrap();
            output.emit(Part::Operation(b"sum"));
            output.before_diagnostic();
            let result = output.finish(close);
            assert!(matches!(
                result.first_error.unwrap().kind,
                io::ErrorKind::Interrupted | io::ErrorKind::WriteZero
            ));
            assert_eq!(sink.attempts.len(), 1);
            assert!(sink.bytes.is_empty());
        }
    }

    #[test]
    fn close_failure_and_pending_before_successful_flush_remain_distinct() {
        for pending in [false, true] {
            let mut sink = Sink::default();
            let mut output = BufferedStdout::new(&mut sink, 4096, false).unwrap();
            if pending {
                output.emit(Part::Close);
            }
            let result = output.finish(|s| {
                s.closes += 1;
                Err(io::Error::from_raw_os_error(9))
            });
            assert_eq!(result.pending_before_finish, pending);
            assert_eq!(result.close_error.unwrap().code, Some(9));
            assert!(result.first_error.is_none() && result.flush_error.is_none());
            assert_eq!(sink.closes, 1);
        }
    }

    #[test]
    fn maximum_header_streams_without_truncation() {
        let mut sink = Sink::default();
        let mut output = BufferedStdout::new(&mut sink, 4096, false).unwrap();
        assert!(
            render(
                &vec![b'a'; 8192],
                &[(Kind::Geomean, 1); 16],
                super::super::records::Separator::Literal(b'\t'),
                b'\t',
                true,
                1,
                |p| output.emit(p)
            )
            .is_ok()
        );
        assert!(output.finish(close).first_error.is_none());
        let label = format!("geomean({})", "a".repeat(8192));
        assert_eq!(sink.bytes, (vec![label; 16].join("\t") + "\n").as_bytes());
        assert_eq!(sink.bytes.len(), 131232);
    }

    #[test]
    fn partial_transfer_error_discards_suffix_but_keeps_later_parts() {
        let mut sink = Sink {
            steps: VecDeque::from([Step::Short(2), Step::Error(28)]),
            ..Sink::default()
        };
        let mut output = BufferedStdout::new(&mut sink, 128, false).unwrap();
        output.emit(Part::Operation(b"sum"));
        output.emit(Part::Name(&[b'a'; 200]));
        output.emit(Part::Close);
        output.emit(Part::Separator(b'\n'));
        let result = output.finish(close);
        assert_eq!(sink.bytes, b"su)\n");
        assert_eq!(sink.attempts.len(), 3);
        assert_eq!(
            sink.attempts[0],
            format!("sum({}", "a".repeat(124)).as_bytes()
        );
        assert_eq!(sink.attempts[1], sink.attempts[0][2..]);
        assert_eq!(sink.attempts[2], b")\n");
        assert_eq!(result.first_error.unwrap().code, Some(28));
        assert!(result.flush_error.is_none() && result.close_error.is_none());
    }

    #[test]
    fn first_write_final_flush_and_close_keep_independent_errors() {
        let mut sink = Sink {
            steps: VecDeque::from([Step::Error(28), Step::Error(5)]),
            ..Sink::default()
        };
        let mut output = BufferedStdout::new(&mut sink, 128, false).unwrap();
        output.emit(Part::Name(&vec![b'a'; 300]));
        output.emit(Part::Close);
        output.emit(Part::Separator(b'\n'));
        let result = output.finish(|s| {
            s.closes += 1;
            Err(io::Error::from_raw_os_error(9))
        });
        assert_eq!(result.first_error.unwrap().code, Some(28));
        assert_eq!(result.flush_error.unwrap().code, Some(5));
        assert_eq!(result.close_error.unwrap().code, Some(9));
        assert!(result.pending_before_finish);
        assert_eq!(sink.closes, 1);
    }

    #[test]
    fn small_line_buffers_keep_direct_transfer_and_newline_boundaries() {
        for capacity in [1, 127, 128, 129] {
            let mut sink = Sink::default();
            let mut output = BufferedStdout::new(&mut sink, capacity, true).unwrap();
            output.emit(Part::Operation(b"sum"));
            output.emit(Part::Name(b"a\nb\nc"));
            assert!(output.finish(close).first_error.is_none());
            assert_eq!(sink.bytes, b"sum(a\nb\nc");
            let expected: &[&[u8]] = match capacity {
                1 => &[b"sum", b"(", b"a\nb\nc"],
                127 => &[b"sum", b"(a\nb\n", b"c"],
                _ => &[b"sum(a\nb\n", b"c"],
            };
            assert_eq!(
                sink.attempts.iter().map(Vec::as_slice).collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn dropping_buffer_does_not_flush_or_close_transport() {
        let mut sink = Sink::default();
        {
            let mut output = BufferedStdout::new(&mut sink, 4096, false).unwrap();
            output.emit(Part::Operation(b"sum"));
        }
        assert!(sink.attempts.is_empty());
        assert_eq!(sink.closes, 0);
    }
}
