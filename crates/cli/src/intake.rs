//! Text Record intake: how text Modes take in their Records.
//!
//! GNU datamash reads all input through one function (text-lines.c
//! `line_record_fread`); Fastmash text Modes use an [`Intake`]. Strict CSV
//! calculations decode Records separately in [`super::csv_input`]. Its text
//! policy names the record terminator, which Records are skipped (a
//! [`Filter`]) and whether the first accepted Record is the Input header. The
//! intake yields that header once, annotation-stripped, then data Records,
//! and counts the Records it accepts (the line numbers of diagnostics). A read
//! error ends the input like end of file; it is reported by
//! [`Intake::finish`] once the Mode has finished (datamash.c `close_input`).
//!
//! Records are read into the caller's buffer and never copied: a vnlog data
//! view is a prefix of the raw Record.
use super::{Failure, annotated, failure, options::Options, os_failure, records, standard_io};
use std::io::{self, BufRead, Write};

/// Which Records an intake skips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Filter {
    /// Every Record is data.
    None,
    /// `-C`: Records whose first byte after blanks is `#` or `;`.
    Comments,
    /// `--vnlog`: blank Records and `#` comments; a data Record ends at its
    /// first `#`. The Input header is read under the vnlog prologue rules:
    /// blank, `##` and `#!` Records are skipped, and data before a `# ` header
    /// is an error.
    Vnlog,
}

impl Filter {
    /// The filter the command's options select.
    pub(super) fn of(options: &Options) -> Self {
        if options.vnlog {
            Self::Vnlog
        } else if options.skip_comments {
            Self::Comments
        } else {
            Self::None
        }
    }
}

/// Whether the first accepted Record is the Input header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Header {
    None,
    First,
}

impl Header {
    /// The first accepted Record is the Input header exactly with `-H`/`--header-in`.
    pub(super) fn of(options: &Options) -> Self {
        if options.header_in {
            Self::First
        } else {
            Self::None
        }
    }
}

/// One data Record, in the caller's buffer.
pub(super) struct Record<'b> {
    bytes: &'b [u8],
    data: usize,
}

impl<'b> Record<'b> {
    /// The whole Record as read, without its terminator.
    pub(super) fn raw(&self) -> &'b [u8] {
        self.bytes
    }
    /// The Record's fields: the whole Record, except that vnlog data stops
    /// before its first `#` and trailing blanks.
    pub(super) fn data(&self) -> &'b [u8] {
        &self.bytes[..self.data]
    }
}

/// What the intake made of one Record.
pub(super) enum Accepted {
    Skipped,
    /// The Input header, now annotation-stripped in the buffer.
    Header,
    /// A data Record whose data view is the buffer's first `usize` bytes.
    Data(usize),
}

pub(super) struct Intake<'p> {
    terminator: u8,
    filter: Filter,
    header_pending: bool,
    /// The diagnostic line number; see [`Intake::line`].
    lines: u64,
    ended: bool,
    error: Option<io::Error>,
    /// The program name while GNU's `--full` deprecation warning is pending.
    warning: Option<&'p [u8]>,
}

impl<'p> Intake<'p> {
    pub(super) fn new(options: &Options, header: Header) -> Self {
        Self {
            terminator: options.record_end,
            filter: Filter::of(options),
            header_pending: header == Header::First,
            lines: 0,
            ended: false,
            error: None,
            warning: None,
        }
    }

    /// Replaces the options' filter.
    pub(super) fn with_filter(mut self, filter: Filter) -> Self {
        self.filter = filter;
        self
    }

    /// Continues the line count after `lines` Records another intake accepted
    /// (an Input header read before sorting).
    pub(super) fn after(mut self, lines: u64) -> Self {
        self.lines = lines;
        self
    }

    /// Arms GNU's deprecation warning for `--full` with Operations that are
    /// not Per-row operations. It is written once, when the data phase begins, as in
    /// datamash.c `process_file`: at once when there is no Input header to
    /// read, after the header has been resolved, or at the first data read
    /// when the input ended before a header.
    pub(super) fn warn_full(mut self, options: &Options, program: &'p [u8]) -> Self {
        if options.warns_full() {
            self.warning = Some(program);
            if !self.header_pending {
                self.warn();
            }
        }
        self
    }

    /// The line number diagnostics report for the latest Record: the Records
    /// accepted so far, the Input header included. Filtered Records (`-C`
    /// comments, vnlog annotations) are not counted, as in GNU datamash, so
    /// this is not a count of input lines.
    pub(super) fn line(&self) -> u64 {
        self.lines
    }

    /// The read error that ended the input, if one did.
    pub(super) fn read_error(&self) -> Option<&io::Error> {
        self.error.as_ref()
    }

    /// Reads the Input header into `buffer`, then resolves it with `resolve`
    /// before the data phase begins. False when the input ends first.
    pub(super) fn header<R: BufRead + ?Sized>(
        &mut self,
        reader: &mut R,
        buffer: &mut Vec<u8>,
        resolve: impl FnOnce(&[u8]) -> Result<(), Failure>,
    ) -> Result<bool, Failure> {
        self.header_captured(reader, buffer, |_| (), |bytes, ()| resolve(bytes))
    }

    /// Captures caller-bounded raw bytes before vnlog strips an accepted header.
    /// Captures for skipped prologue Records are discarded immediately.
    pub(super) fn header_captured<R: BufRead + ?Sized, C>(
        &mut self,
        reader: &mut R,
        buffer: &mut Vec<u8>,
        mut capture: impl FnMut(&[u8]) -> C,
        resolve: impl FnOnce(&[u8], C) -> Result<(), Failure>,
    ) -> Result<bool, Failure> {
        while self.header_pending {
            if !self.read(reader, buffer)? {
                return Ok(false);
            }
            let captured = capture(buffer);
            if let Accepted::Header = self.accept(buffer)? {
                resolve(buffer, captured)?;
                self.warn();
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Reads the next data Record into `buffer`; `None` at the end of input.
    #[inline]
    pub(super) fn next<'b, R: BufRead + ?Sized>(
        &mut self,
        reader: &mut R,
        buffer: &'b mut Vec<u8>,
    ) -> Result<Option<Record<'b>>, Failure> {
        let data = loop {
            if !self.read(reader, buffer)? {
                return Ok(None);
            }
            match self.accept(buffer)? {
                Accepted::Data(data) => break data,
                Accepted::Skipped => {}
                Accepted::Header => unreachable!("the Input header is read by `header`"),
            }
        };
        Ok(Some(Record {
            bytes: buffer,
            data,
        }))
    }

    /// Applies the policy to one Record read elsewhere (such as a sorted
    /// Record), counting it when it is accepted.
    #[inline]
    pub(super) fn accept(&mut self, buffer: &mut Vec<u8>) -> Result<Accepted, Failure> {
        if self.header_pending {
            return self.accept_header(buffer);
        }
        let data = match self.filter {
            Filter::None => buffer.len(),
            Filter::Comments if records::is_comment(buffer) => return Ok(Accepted::Skipped),
            Filter::Comments => buffer.len(),
            Filter::Vnlog if annotated::skip_data(buffer) => return Ok(Accepted::Skipped),
            Filter::Vnlog => annotated::data(buffer).len(),
        };
        self.count()?;
        Ok(Accepted::Data(data))
    }

    #[cold]
    fn accept_header(&mut self, buffer: &mut Vec<u8>) -> Result<Accepted, Failure> {
        let accepted = match self.filter {
            Filter::None => true,
            Filter::Comments => !records::is_comment(buffer),
            Filter::Vnlog => annotated::header(buffer)?,
        };
        if !accepted {
            return Ok(Accepted::Skipped);
        }
        self.count()?;
        self.header_pending = false;
        Ok(Accepted::Header)
    }

    #[inline]
    fn count(&mut self) -> Result<(), Failure> {
        self.lines = self
            .lines
            .checked_add(1)
            .ok_or_else(|| super::unsupported("record count exceeds u64 limit"))?;
        Ok(())
    }

    /// Reads one Record. The end of input, or a read error, is final.
    #[inline]
    fn read<R: BufRead + ?Sized>(
        &mut self,
        reader: &mut R,
        buffer: &mut Vec<u8>,
    ) -> Result<bool, Failure> {
        if self.ended {
            // The input ended while its Input header was sought; the data
            // phase begins with nothing to read.
            self.warn();
            return Ok(false);
        }
        match records::read_record_terminated(reader, buffer, self.terminator) {
            Ok(true) => Ok(true),
            Ok(false) => {
                self.ended = true;
                Ok(false)
            }
            Err(records::ReadError::Io(error)) => {
                self.ended = true;
                self.error = Some(error);
                Ok(false)
            }
            Err(records::ReadError::Allocation) => {
                Err(super::unsupported("record memory allocation failed"))
            }
        }
    }

    #[cold]
    fn warn(&mut self) {
        if let Some(program) = self.warning.take() {
            write_full_warning(program);
        }
    }

    /// Reports the read error that ended the input, if one did.
    pub(super) fn finish(self) -> Result<(), Failure> {
        self.error.map_or(Ok(()), |error| Err(read_failure(&error)))
    }
}

/// Writes GNU's deprecation warning for `--full` with Operations that are
/// not Per-row operations: a compatibility warning, not a fatal diagnostic.
/// GNU prints its own name; Fastmash prints the invoked name, like other
/// diagnostics.
#[cold]
pub(super) fn write_full_warning(program: &[u8]) {
    let mut stderr = standard_io::Stderr;
    let _ = stderr.write_all(program).and_then(|()| {
        stderr.write_all(b": Using -f/--full with non-linewise operations is deprecated and will be disabled in a future release.\n")
    });
}

/// The diagnostic for a read error that ended the input.
pub(super) fn read_failure(error: &io::Error) -> Failure {
    if error.raw_os_error() == Some(5) {
        failure(b"read error: Input/output error\n".to_vec())
    } else {
        os_failure(error, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options;

    fn options(args: &[&str]) -> Options {
        let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
        match options::parse(&args, b"fastmash", false) {
            Ok(options::Action::Calculate(options)) => *options,
            _ => panic!("calculation options"),
        }
    }

    /// One data Record's raw bytes, data view and line.
    type Row = (Vec<u8>, Vec<u8>, u64);

    /// The header (if any), then each data Record.
    fn take(options: &Options, header: Header, input: &[u8]) -> (Option<Vec<u8>>, Vec<Row>) {
        let mut reader = input;
        let mut intake = Intake::new(options, header);
        let mut buffer = Vec::new();
        let found = intake
            .header(&mut reader, &mut buffer, |_| Ok(()))
            .ok()
            .unwrap()
            .then(|| buffer.clone());
        let mut rows = Vec::new();
        while let Some(record) = intake.next(&mut reader, &mut buffer).ok().unwrap() {
            rows.push((record.raw().to_vec(), record.data().to_vec(), intake.line()));
        }
        (found, rows)
    }

    #[test]
    fn filters_skip_records_and_lines_count_only_accepted_ones() {
        let input = b"h\n#c\n ;d\n1 # x\n\n";
        let (header, rows) = take(&options(&["-C", "count", "1"]), Header::First, input);
        assert_eq!(header.as_deref(), Some(b"h".as_slice()));
        assert_eq!(
            rows,
            [
                (b"1 # x".to_vec(), b"1 # x".to_vec(), 2),
                (b"".to_vec(), b"".to_vec(), 3)
            ]
        );
        let (header, rows) = take(&options(&["count", "1"]), Header::None, b"#c\n;d\n");
        assert_eq!(header, None);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].2, 2);
    }

    #[test]
    fn vnlog_header_is_stripped_and_data_views_are_raw_prefixes() {
        let input = b"## meta\n\n#!x\n#  a b  \n1 2 # note\n#c\n;3 4\n";
        let (header, rows) = take(&options(&["--vnlog", "count", "a"]), Header::First, input);
        assert_eq!(header.as_deref(), Some(b"a b".as_slice()));
        assert_eq!(
            rows,
            [
                (b"1 2 # note".to_vec(), b"1 2".to_vec(), 2),
                (b";3 4".to_vec(), b";3 4".to_vec(), 3)
            ]
        );
        // Without a header phase, vnlog skips the header like any comment.
        let (_, rows) = take(&options(&["--vnlog", "count", "a"]), Header::None, input);
        assert_eq!(rows.len(), 2);
        let mut intake = Intake::new(&options(&["--vnlog", "count", "a"]), Header::First);
        let error = intake
            .header(&mut b"1 2\n".as_slice(), &mut Vec::new(), |_| Ok(()))
            .err()
            .unwrap();
        assert_eq!(
            error.message,
            b"invalid vnlog data: received record before header: '1 2'\n"
        );
    }

    #[test]
    fn pushed_records_follow_the_same_policy() {
        let options = options(&["--vnlog", "count", "a"]);
        let mut intake = Intake::new(&options, Header::First).after(1);
        let mut kinds = Vec::new();
        for record in [b"#late".as_slice(), b"# p q", b"#x", b"a 1 #n"] {
            let mut buffer = record.to_vec();
            kinds.push(match intake.accept(&mut buffer).ok().unwrap() {
                Accepted::Skipped => "skipped".to_owned(),
                Accepted::Header => format!("header {}", String::from_utf8_lossy(&buffer)),
                Accepted::Data(data) => {
                    format!("data {}", String::from_utf8_lossy(&buffer[..data]))
                }
            });
        }
        assert_eq!(kinds, ["header late", "skipped", "skipped", "data a 1"]);
        assert_eq!(intake.line(), 3);
    }

    /// Yields one Record and a partial one, then fails; asked again, it panics.
    struct Failing(u8);
    impl io::Read for Failing {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            unreachable!()
        }
    }
    impl BufRead for Failing {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            self.0 += 1;
            match self.0 {
                1 => Ok(b"a
partial"),
                2 => Err(io::Error::from_raw_os_error(5)),
                _ => panic!("read after the input ended"),
            }
        }
        fn consume(&mut self, amount: usize) {
            assert_eq!(amount, 2);
        }
    }

    #[test]
    fn a_read_error_ends_the_input_once_and_is_reported_last() {
        let options = options(&["count", "1"]);
        let mut intake = Intake::new(&options, Header::None);
        let mut reader = Failing(0);
        let mut buffer = Vec::new();
        assert_eq!(
            intake
                .next(&mut reader, &mut buffer)
                .ok()
                .unwrap()
                .map(|r| r.raw()),
            Some(b"a".as_slice())
        );
        assert!(
            intake
                .next(&mut reader, &mut buffer)
                .ok()
                .unwrap()
                .is_none()
        );
        // Ended: the reader is not asked again.
        assert!(
            intake
                .next(&mut reader, &mut buffer)
                .ok()
                .unwrap()
                .is_none()
        );
        assert_eq!(
            intake.read_error().and_then(io::Error::raw_os_error),
            Some(5)
        );
        let error = intake.finish().err().unwrap();
        assert_eq!(
            (error.status, error.message),
            (1, b"read error: Input/output error\n".to_vec())
        );
    }
}
