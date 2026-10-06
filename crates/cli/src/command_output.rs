//! Command output and explicit finalization for standard output.
use super::buffered_stdout::{BufferedStdout, Completion};
use super::{
    Failure, OperationSet, failure, headers, options, os_failure, platform, records, standard_io,
    unsupported,
};
#[cfg(target_os = "linux")]
use std::{
    io::IsTerminal,
    os::unix::fs::{FileTypeExt, MetadataExt},
};
use std::{
    io::{self, Write},
    marker::PhantomData,
    rc::Rc,
};

/// Where a calculation writes: implemented by [`Results`].
pub(super) trait CommandOutput {
    fn cell(&mut self, row: &[u8], column: &[u8], value: &[u8]) -> Result<(), Failure>;
    fn end(&mut self, options: &options::Options) -> Result<(), Failure>;
    fn first(
        &mut self,
        record: &[u8],
        operations: &OperationSet,
        keys: &[u64],
    ) -> Result<(), Failure>;
    fn key(&mut self, bytes: &[u8], separator: u8);
    fn first_decoded(
        &mut self,
        record: super::csv_input::View<'_>,
        operations: &OperationSet,
        keys: &[u64],
    ) -> Result<(), Failure>;
    fn result(&mut self, bytes: &[u8], separator: u8);
    fn row(&mut self, fields: &[Vec<u8>], separator: u8) -> Result<(), Failure>;
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

/// A calculation's output through [`BufferedStdout`]: the output header, named
/// from the Input header or generated, then result rows, Full rows or a
/// crosstab table.
pub(super) struct Results<'a, W> {
    pub buffer: BufferedStdout<'a, W>,
    table: Option<super::crosstab::Table>,
    input: records::Separator,
    output: u8,
    csv: bool,
    record_end: u8,
    named: bool,
    enabled: bool,
    full: bool,
    vnlog: bool,
    profile: fastmash_conversion::profile::Profile,
    result_names: &'a super::result_names::ResultNames,
}
impl<'a, W: Write> Results<'a, W> {
    pub fn new(buffer: BufferedStdout<'a, W>, options: &'a options::Options) -> Self {
        Self {
            buffer,
            table: options.crosstab.then(super::crosstab::Table::default),
            input: options.input,
            output: options.output,
            csv: options.csv_out,
            record_end: options.record_end,
            named: options.header_in,
            enabled: options.header_out,
            full: options.full,
            vnlog: options.vnlog,
            profile: options.presentation.profile,
            result_names: &options.result_names,
        }
    }
}
impl<W: Write> Results<'_, W> {
    fn full_header<'r>(&mut self, fields: impl Iterator<Item = &'r [u8]>) {
        for (index, bytes) in fields.enumerate() {
            let generated = (!self.named).then(|| headers::generated(index as u64 + 1));
            let label = generated
                .as_ref()
                .map_or_else(|| headers::label(bytes), |label| label.as_bytes());
            if self.csv {
                self.buffer.csv_field(label);
            } else {
                self.buffer.formatted(label);
            }
            self.buffer.emit(headers::Part::Separator(self.output));
        }
    }

    /// The Output header of a sorted record that keeps only its selected
    /// fields (never with `--full`, nor named from an Input header):
    /// `field` looks each one up.
    pub fn first_projected<'r>(
        &mut self,
        field: impl FnMut(u64) -> Result<&'r [u8], Failure>,
        operations: &OperationSet,
        keys: &[u64],
    ) -> Result<(), Failure> {
        if !self.enabled {
            return Ok(());
        }
        debug_assert!(!self.full && !self.vnlog && !self.named);
        self.selected(field, operations, keys)
    }

    /// The Output header's Grouping keys, unless `--full` printed the whole
    /// record, then its requests.
    fn selected<'r>(
        &mut self,
        mut field: impl FnMut(u64) -> Result<&'r [u8], Failure>,
        operations: &OperationSet,
        keys: &[u64],
    ) -> Result<(), Failure> {
        for &key in keys.iter().filter(|_| !self.full) {
            let bytes = field(key)?;
            if self.csv {
                let generated = (!self.named).then(|| headers::generated(key));
                let label = generated
                    .as_ref()
                    .map_or_else(|| headers::label(bytes), |label| label.as_bytes());
                self.buffer.csv_parts(&[b"GroupBy(", label, b")"]);
            } else if self.named {
                self.buffer.group_header(headers::label(bytes));
            } else {
                self.buffer.group_header(headers::generated(key).as_bytes());
            }
            self.buffer.emit(headers::Part::Separator(self.output));
        }
        let mut requests = Vec::new();
        super::command_memory::reserve(&mut requests, operations.len())?;
        requests.extend(operations.requests());
        if self.csv {
            return headers::render_fields(
                &requests,
                field,
                (b',', b'\n', self.profile),
                self.named,
                self.result_names,
                |header, separator| {
                    header.with_fragments(|fragments| self.buffer.csv_parts(fragments));
                    self.buffer.emit(headers::Part::Separator(separator));
                },
            );
        }
        headers::render_requests(
            &requests,
            field,
            (self.output, self.record_end, self.profile),
            self.named,
            self.result_names,
            |part| self.buffer.emit(part),
        )
    }
}

impl<W: Write> CommandOutput for Results<'_, W> {
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
        operations: &OperationSet,
        keys: &[u64],
    ) -> Result<(), Failure> {
        if !self.enabled {
            return Ok(());
        }
        if self.vnlog {
            self.buffer.formatted(b"# ");
        }
        if self.full {
            self.full_header(
                records::fields(record, self.input)
                    .map(|span| &record[span.start..span.start + span.length]),
            );
        }
        // One pass over the record serves every key and request.
        let mut index =
            records::FieldIndex::selecting(keys.iter().copied().chain(operations.fields()));
        let input = self.input;
        let field = |field| {
            super::indexed_field(record, &mut index, field, 1, input)
                .map(|span| &record[span.start..span.start + span.length])
        };
        self.selected(field, operations, keys)
    }
    fn key(&mut self, bytes: &[u8], separator: u8) {
        if self.csv {
            self.buffer.csv_field(bytes);
            self.buffer.emit(headers::Part::Separator(b','));
        } else {
            self.buffer.raw(bytes);
            self.buffer.emit(headers::Part::Separator(separator));
        }
    }
    fn first_decoded(
        &mut self,
        record: super::csv_input::View<'_>,
        operations: &OperationSet,
        keys: &[u64],
    ) -> Result<(), Failure> {
        if !self.enabled {
            return Ok(());
        }
        if self.full {
            self.full_header(record.fields());
        }
        self.selected(|field| record.field(field), operations, keys)
            .map_err(|error| record.location.annotate(error))
    }
    fn result(&mut self, bytes: &[u8], separator: u8) {
        if self.csv {
            self.buffer.csv_field(bytes);
            self.buffer
                .emit(headers::Part::Separator(if separator == self.record_end {
                    b'\n'
                } else {
                    b','
                }));
        } else {
            self.buffer.formatted(bytes);
            self.buffer.emit(headers::Part::Separator(separator));
        }
    }
    fn row(&mut self, fields: &[Vec<u8>], separator: u8) -> Result<(), Failure> {
        for (at, field) in fields.iter().enumerate() {
            self.result(
                field,
                if at + 1 == fields.len() {
                    self.record_end
                } else {
                    separator
                },
            );
        }
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
    mut buffer: BufferedStdout<'_, W>,
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
        status = super::failure::combine(status, error.status);
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
    #[cfg(target_os = "linux")]
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
    #[cfg(target_os = "macos")]
    pub fn buffering(&self) -> (usize, bool) {
        let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
        // SAFETY: fstat initializes one stat through its valid output pointer.
        if unsafe { libc::fstat(1, metadata.as_mut_ptr()) } != 0 {
            return (8192, false);
        }
        // SAFETY: successful fstat initialized the entire metadata structure.
        let metadata = unsafe { metadata.assume_init() };
        let size = metadata.st_blksize;
        // SAFETY: isatty only inspects the inherited standard-output descriptor.
        let terminal =
            metadata.st_mode & libc::S_IFMT == libc::S_IFCHR && unsafe { libc::isatty(1) } == 1;
        (
            if size > 0 && size < 8192 {
                size as usize
            } else {
                8192
            },
            terminal,
        )
    }
    pub fn close(&mut self) -> io::Result<()> {
        let Some(fd) = self.fd.take() else {
            return Ok(());
        };
        let result = platform::close(fd);
        if standard_io::originally_closed(fd) {
            Err(io::Error::from_raw_os_error(9))
        } else {
            result
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
    fn result_rows_preserve_empty_columns_and_raw_bytes() {
        use super::{BufferedStdout, CommandOutput, Results, completion_failure, options};
        let options::Action::Calculate(options) =
            options::parse(&["count".into(), "1".into()], b"fastmash", false)
                .ok()
                .unwrap()
        else {
            panic!("calculation")
        };
        for (fields, separator, expected) in [
            (vec![b"".as_slice()], b',', b"\n".as_slice()),
            (vec![b"", b""], b',', b",\n"),
            (vec![b"", b"x"], b',', b",x\n"),
            (vec![b"", b"", b"x"], b',', b",,x\n"),
            (vec![b"x", b"", b"y"], b',', b"x,,y\n"),
            (vec![b"x", b""], b',', b"x,\n"),
            (vec![b"x", b"y"], b',', b"x,y\n"),
            (vec![b"", b"\0\xff", b""], 0, b"\0\0\xff\0\n"),
        ] {
            let fields: Vec<_> = fields.into_iter().map(<[u8]>::to_vec).collect();
            for capacity in [1, 2, 4096] {
                let mut bytes = Vec::new();
                let buffer = BufferedStdout::new(&mut bytes, capacity, false).unwrap();
                let mut output = Results::new(buffer, &options);
                assert!(output.row(&fields, separator).is_ok());
                assert!(completion_failure(output.buffer.finish(|_| Ok(()))).is_none());
                assert_eq!(bytes, expected);
            }
        }
    }

    use super::*;
    use crate::buffered_stdout::IoFailure;
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
            // An Internal failure outranks an output Error or Refusal after it.
            (70, 28, true, 70),
            (70, 5, true, 70),
            (70, 28, false, 1),
        ] {
            let events = Rc::new(RefCell::new(Vec::new()));
            let mut sink = Sink {
                events: events.clone(),
                code,
            };
            let mut buffer = BufferedStdout::new(&mut sink, 4096, false).unwrap();
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
            let mut buffer = BufferedStdout::new(&mut sink, 128, false).unwrap();
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
