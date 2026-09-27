//! Command output and explicit finalization for standard output.
use super::{
    Failure, Operation, failure, headers, linux, options, os_failure, records, standard_io,
    unsupported,
};
use headers::output::{Buffered, Completion};
use std::{
    io::{self, IsTerminal, Write},
    marker::PhantomData,
    os::unix::fs::{FileTypeExt, MetadataExt},
    rc::Rc,
};

pub(super) trait CommandOutput {
    fn cell(&mut self, _row: &[u8], _column: &[u8], _value: &[u8]) -> Result<(), Failure> {
        unreachable!("crosstab uses buffered output")
    }
    fn end(&mut self, _options: &options::Options) -> Result<(), Failure> {
        Ok(())
    }
    fn first(
        &mut self,
        _record: &[u8],
        _operations: &[Operation],
        _keys: &[u64],
    ) -> Result<(), Failure> {
        Ok(())
    }
    fn key(&mut self, _bytes: &[u8], _separator: u8) {
        unreachable!("grouping uses buffered output")
    }
    fn result(&mut self, _bytes: &[u8], _separator: u8) {
        unreachable!("grouping uses buffered output")
    }
    fn row(&mut self, fields: &[Vec<u8>], separator: u8) -> Result<(), Failure>;
    fn empty(&mut self) -> Result<(), Failure>;
}

pub(super) fn full_prefix(
    output: &mut impl CommandOutput,
    record: &[u8],
    options: &options::Options,
) {
    for span in records::fields(record, options.input) {
        output.key(
            &record[span.start..span.start + span.length],
            options.output,
        );
    }
}

pub(super) fn warn_full(options: &options::Options, program: &[u8]) {
    if options.full && !options.linewise {
        // GNU's compatibility warning is not a fatal diagnostic. GNU prints its
        // own name; Fastmash prints the invoked name, like other diagnostics.
        let mut stderr = standard_io::Stderr;
        let _ = stderr.write_all(program).and_then(|()| {
            stderr.write_all(b": Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n")
        });
    }
}

pub(super) struct Plain<'a, W> {
    pub writer: &'a mut W,
    pub record_end: u8,
}
impl<W: Write> CommandOutput for Plain<'_, W> {
    fn row(&mut self, fields: &[Vec<u8>], separator: u8) -> Result<(), Failure> {
        let mut bytes = Vec::new();
        let length = fields
            .iter()
            .try_fold(fields.len().max(1), |length, field| {
                length.checked_add(field.len())
            })
            .ok_or_else(|| unsupported("output memory allocation failed"))?;
        bytes
            .try_reserve_exact(length)
            .map_err(|_| unsupported("output memory allocation failed"))?;
        for (at, field) in fields.iter().enumerate() {
            if at != 0 {
                bytes.push(separator);
            }
            bytes.extend_from_slice(field);
        }
        bytes.push(self.record_end);
        records::write_output(self.writer, &bytes).map_err(|e| os_failure(&e, false))
    }
    fn empty(&mut self) -> Result<(), Failure> {
        self.writer.flush().map_err(|e| os_failure(&e, false))
    }
}

pub(super) struct Header<'a, W> {
    pub buffer: Buffered<'a, W>,
    table: Option<super::crosstab::Table>,
    input: records::Separator,
    output: u8,
    record_end: u8,
    named: bool,
    enabled: bool,
    full: bool,
    vnlog: bool,
    profile: fastmash_conversion::profile::Profile,
}
impl<'a, W: Write> Header<'a, W> {
    pub fn new(buffer: Buffered<'a, W>, options: &options::Options) -> Self {
        Self {
            buffer,
            table: options.crosstab.then(super::crosstab::Table::default),
            input: options.input,
            output: options.output,
            record_end: options.record_end,
            named: options.header_in,
            enabled: options.header_out,
            full: options.full,
            vnlog: options.vnlog,
            profile: options.presentation.profile,
        }
    }
}
impl<W: Write> CommandOutput for Header<'_, W> {
    fn cell(&mut self, row: &[u8], column: &[u8], value: &[u8]) -> Result<(), Failure> {
        self.table
            .as_mut()
            .expect("crosstab output owner")
            .insert(row, column, value)
    }
    fn end(&mut self, options: &options::Options) -> Result<(), Failure> {
        if let Some(table) = self.table.take() {
            table.write(&mut self.buffer, options)?;
        }
        Ok(())
    }
    fn first(
        &mut self,
        record: &[u8],
        operations: &[Operation],
        keys: &[u64],
    ) -> Result<(), Failure> {
        if !self.enabled {
            return Ok(());
        }
        if self.vnlog {
            self.buffer.formatted(b"# ");
        }
        if self.full {
            for (index, span) in records::fields(record, self.input).enumerate() {
                if self.named {
                    let name = &record[span.start..span.start + span.length];
                    let end = name.iter().position(|&b| b == 0).unwrap_or(name.len());
                    self.buffer.formatted(&name[..end]);
                } else {
                    self.buffer
                        .formatted(format!("field-{}", index + 1).as_bytes());
                }
                self.buffer.emit(headers::Part::Separator(self.output));
            }
        }
        for &field in keys.iter().filter(|_| !self.full) {
            let span = super::selected_field(record, field, 1, self.input)?;
            let generated;
            let name = if self.named {
                let bytes = &record[span.start..span.start + span.length];
                &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())]
            } else {
                generated = format!("field-{field}");
                generated.as_bytes()
            };
            self.buffer.group_header(name);
            self.buffer.emit(headers::Part::Separator(self.output));
        }
        let mut requests = Vec::new();
        super::command_memory::reserve(&mut requests, operations.len())?;
        requests.extend(operations.iter().map(|op| (op.kind, op.selector)));
        headers::render_selectors(
            record,
            &requests,
            self.input,
            (self.output, self.record_end, self.profile),
            self.named,
            1,
            |part| self.buffer.emit(part),
        )
    }
    fn key(&mut self, bytes: &[u8], separator: u8) {
        self.buffer.raw(bytes);
        self.buffer.emit(headers::Part::Separator(separator));
    }
    fn result(&mut self, bytes: &[u8], separator: u8) {
        self.buffer.formatted(bytes);
        self.buffer.emit(headers::Part::Separator(separator));
    }
    fn row(&mut self, fields: &[Vec<u8>], separator: u8) -> Result<(), Failure> {
        for (at, field) in fields.iter().enumerate() {
            self.buffer.formatted(field);
            self.buffer
                .emit(headers::Part::Separator(if at + 1 == fields.len() {
                    self.record_end
                } else {
                    separator
                }));
        }
        Ok(())
    }
    fn empty(&mut self) -> Result<(), Failure> {
        Ok(())
    }
}

fn completion_failure(done: Completion) -> Option<Failure> {
    let close = done.close_error.filter(|error| {
        error.code != Some(9) || done.first_error.is_some() || done.pending_before_finish
    });
    let last = close.or(done.flush_error);
    if done.unsupported_write || last.is_some_and(|error| !matches!(error.code, Some(9 | 28))) {
        return Some(unsupported("unsupported output I/O error"));
    }
    if let Some(error) = last {
        Some(os_failure(
            &io::Error::from_raw_os_error(error.code.unwrap()),
            false,
        ))
    } else if done.first_error.is_some() {
        Some(failure(b"write error\n".to_vec()))
    } else {
        None
    }
}

pub(super) fn complete<W: Write>(
    mut buffer: Buffered<'_, W>,
    result: Result<(), Failure>,
    close: impl FnOnce(&mut W) -> io::Result<()>,
    report: &mut impl FnMut(&Failure) -> bool,
) -> i32 {
    let mut status = 0;
    let mut diagnostics_ok = true;
    if let Err(error) = result {
        buffer.before_diagnostic();
        status = error.status;
        diagnostics_ok = report(&error);
    }
    if let Some(error) = completion_failure(buffer.finish(close)) {
        status = if status == 77 || error.status == 77 {
            77
        } else {
            1
        };
        // Even failed stderr must not prevent stdout finalization or the second attempt.
        diagnostics_ok = report(&error) && diagnostics_ok;
    }
    if diagnostics_ok { status } else { 1 }
}

pub(super) trait Transport: Write {
    fn buffering(&self) -> (usize, bool);
    fn close(&mut self) -> io::Result<()>;
}

#[cfg(test)]
impl Transport for Vec<u8> {
    fn buffering(&self) -> (usize, bool) {
        (8192, false)
    }
    fn close(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The sole owner of fd 1, borrowed by each command mode.
/// Raw syscalls also handle an initially closed descriptor without an invalid OwnedFd.
pub(super) struct Stdout {
    fd: Option<usize>,
    _single_thread: PhantomData<Rc<()>>,
}
impl Stdout {
    pub fn new() -> Self {
        Self {
            fd: Some(1),
            _single_thread: PhantomData,
        }
    }
    pub fn buffering(&self) -> (usize, bool) {
        match std::fs::metadata("/proc/self/fd/1") {
            Ok(meta) => {
                let size = meta.blksize();
                (
                    if size > 0 && size < 8192 {
                        size as usize
                    } else {
                        8192
                    },
                    meta.file_type().is_char_device() && io::stdout().is_terminal(),
                )
            }
            Err(_) => (8192, false),
        }
    }
    pub fn close(&mut self) -> io::Result<()> {
        let Some(fd) = self.fd.take() else {
            return Ok(());
        };
        // Linux close is never retried, including EINTR, because the fd may be reused.
        // SAFETY: close takes no pointers; `fd` was taken from this writer, its owner.
        let result = unsafe { linux::syscall(3, fd, 0, 0, 0) };
        if standard_io::originally_closed(fd) {
            Err(io::Error::from_raw_os_error(9))
        } else if result < 0 {
            Err(io::Error::from_raw_os_error(-result as i32))
        } else {
            Ok(())
        }
    }
}
impl Write for Stdout {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let fd = self.fd.ok_or_else(|| io::Error::from_raw_os_error(9))?;
        standard_io::write(fd, bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Transport for Stdout {
    fn buffering(&self) -> (usize, bool) {
        self.buffering()
    }
    fn close(&mut self) -> io::Result<()> {
        self.close()
    }
}
impl Drop for Stdout {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_rows_preserve_empty_columns() {
        use super::{CommandOutput, Plain};
        for (fields, expected) in [
            (vec![b"".as_slice()], b"\n".as_slice()),
            (vec![b"", b""], b",\n"),
            (vec![b"", b"x"], b",x\n"),
            (vec![b"", b"", b"x"], b",,x\n"),
            (vec![b"x", b"", b"y"], b"x,,y\n"),
            (vec![b"x", b""], b"x,\n"),
            (vec![b"x", b"y"], b"x,y\n"),
        ] {
            let mut bytes = Vec::new();
            let fields: Vec<_> = fields.into_iter().map(<[u8]>::to_vec).collect();
            assert!(
                Plain {
                    writer: &mut bytes,
                    record_end: b'\n'
                }
                .row(&fields, b',')
                .is_ok()
            );
            assert_eq!(bytes, expected);
        }
    }

    #[test]
    fn plain_rows_preserve_raw_bytes_short_writes_and_flush() {
        use super::{CommandOutput, Plain};
        struct Sink {
            bytes: Vec<u8>,
            flushes: usize,
        }
        impl std::io::Write for Sink {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                let count = bytes.len().min(2);
                self.bytes.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }
        let mut sink = Sink {
            bytes: Vec::new(),
            flushes: 0,
        };
        let fields = [b"".to_vec(), b"\0\xff".to_vec(), b"".to_vec()];
        assert!(
            Plain {
                writer: &mut sink,
                record_end: b'\n'
            }
            .row(&fields, b'\0')
            .is_ok()
        );
        assert_eq!(sink.bytes, b"\0\0\xff\0\n");
        assert_eq!(sink.flushes, 1);
    }

    use super::*;
    use headers::output::IoFailure;
    use std::{cell::RefCell, rc::Rc};

    fn io_error(code: i32) -> IoFailure {
        io::Error::from_raw_os_error(code).into()
    }

    #[test]
    fn finalization_distinguishes_prior_pending_and_close_errors() {
        for (first, flush, close, pending, unsupported, expected) in [
            (None, None, None, false, false, None),
            (None, None, Some(9), false, false, None),
            (
                Some(28),
                None,
                None,
                false,
                false,
                Some((1, "write error\n")),
            ),
            (
                Some(28),
                Some(28),
                None,
                true,
                false,
                Some((1, "write error: No space left on device\n")),
            ),
            (
                None,
                None,
                Some(28),
                false,
                false,
                Some((1, "write error: No space left on device\n")),
            ),
            (
                Some(28),
                None,
                Some(9),
                false,
                false,
                Some((1, "write error: Bad file descriptor\n")),
            ),
            (
                None,
                None,
                Some(9),
                true,
                false,
                Some((1, "write error: Bad file descriptor\n")),
            ),
            (
                None,
                Some(28),
                None,
                true,
                true,
                Some((77, "unsupported output I/O error\n")),
            ),
            (
                None,
                None,
                Some(28),
                false,
                true,
                Some((77, "unsupported output I/O error\n")),
            ),
        ] {
            let error = completion_failure(Completion {
                first_error: first.map(io_error),
                flush_error: flush.map(io_error),
                close_error: close.map(io_error),
                pending_before_finish: pending,
                unsupported_write: unsupported,
            });
            assert_eq!(
                error.map(|e| (e.status, String::from_utf8(e.message).unwrap())),
                expected.map(|(s, m)| (s, m.to_owned()))
            );
        }
    }

    #[test]
    fn primary_flush_diagnostic_close_secondary_order_and_status() {
        struct Sink {
            events: Rc<RefCell<Vec<String>>>,
            code: i32,
        }
        impl Write for Sink {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                self.events.borrow_mut().push("flush".into());
                Err(io::Error::from_raw_os_error(self.code))
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("unbuffered")
            }
        }
        for (primary, code, diagnostics_ok, expected) in [
            (1, 28, true, 1),
            (1, 5, true, 77),
            (77, 28, true, 77),
            (77, 28, false, 1),
        ] {
            let events = Rc::new(RefCell::new(Vec::new()));
            let mut sink = Sink {
                events: events.clone(),
                code,
            };
            let mut buffer = Buffered::new(&mut sink, 4096, false).unwrap();
            buffer.formatted(b"pending");
            let status = complete(
                buffer,
                Err(Failure {
                    status: primary,
                    message: b"primary\n".to_vec(),
                }),
                |sink| {
                    sink.events.borrow_mut().push("close".into());
                    Ok(())
                },
                &mut |error| {
                    events
                        .borrow_mut()
                        .push(String::from_utf8(error.message.clone()).unwrap());
                    diagnostics_ok
                },
            );
            assert_eq!(status, expected);
            assert_eq!(
                *events.borrow(),
                [
                    "flush",
                    "primary\n",
                    "close",
                    if code == 28 {
                        "write error\n"
                    } else {
                        "unsupported output I/O error\n"
                    }
                ]
            );
        }
    }

    #[test]
    fn transient_unsupported_write_is_not_erased_by_later_enospc() {
        struct Sink(usize);
        impl Write for Sink {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                self.0 += 1;
                if self.0 == 2 {
                    Ok(0)
                } else {
                    Err(io::Error::from_raw_os_error(28))
                }
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("unbuffered")
            }
        }
        for close_fails in [false, true] {
            let mut sink = Sink(0);
            let mut buffer = Buffered::new(&mut sink, 128, false).unwrap();
            for _ in 0..3 {
                buffer.formatted(b"pending");
                buffer.before_diagnostic();
            }
            buffer.formatted(b"last");
            let result = buffer.finish(|_| {
                if close_fails {
                    Err(io::Error::from_raw_os_error(28))
                } else {
                    Ok(())
                }
            });
            assert_eq!(completion_failure(result).unwrap().status, 77);
        }
    }
}
