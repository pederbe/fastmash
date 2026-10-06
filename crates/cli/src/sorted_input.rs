//! The external Sort route: the system `sort`, run by the sort supervisor,
//! from its startup checks to completion. A caller checks the route (`admit`), which
//! the startup order requires before any sort starts, prepares the input
//! (`Sorting`), starts the sort by its keys and runs its calculation over the
//! sorted records (`Sorted::run`), which completes the session.
use super::{
    Failure, append, failure,
    intake::{self, Intake},
    options, os_failure, records, standard_io, unsupported, unsupported_hint,
};
use fastmash_sort_process::{Message, Session, StartError};
use std::{
    fs::File,
    io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::fd::BorrowedFd,
    process::ChildStdout,
};

/// Whether the system `sort` can run safely and work here
/// (`fastmash_sort_process::available`): not with a separator byte from
/// 0x80; ignored or blocked `HUP`, `INT` or `TERM`, as under `nohup`; no
/// pidfds or `close_range`; no executable supervisor beside fastmash; no
/// runnable `/usr/bin/sort`; or no writable, searchable temporary directory.
pub(super) fn available(input: records::Separator) -> bool {
    fastmash_sort_process::available(separator(input))
}

fn separator(input: records::Separator) -> Option<u8> {
    match input {
        records::Separator::Whitespace => None,
        records::Separator::Literal(b) => Some(b),
    }
}

/// Evidence that the external route passed its startup checks (`admit`),
/// which every sort needs.
pub(super) struct Cleared(());

/// Admits the external route: `locale` sorts, and the process can run the
/// sort supervisor.
pub(super) fn admit(locale: &super::locale::Policy) -> Result<Cleared, Failure> {
    locale.sorting()?;
    fastmash_sort_process::admit()
        .map_err(|_| unsupported("sort process runtime requirements are not met"))?;
    Ok(Cleared(()))
}

/// Standard input on its way to the system sort: the Input header, where it
/// was read before sorting, and the errno of a failed header read, which
/// completes the sort's "read error (on close)" as GNU datamash's does.
pub(super) struct Sorting {
    header: Option<Vec<u8>>,
    header_errno: Option<i32>,
}

impl Sorting {
    /// Checks standard input, then with `--header-in` reads its Input header,
    /// leaving every later byte for the sort.
    pub(super) fn prepare(_: Cleared, options: &options::Options) -> Result<Self, Failure> {
        standard_io::check_input_access().map_err(|error| os_failure(&error, true))?;
        let mut sorting = Self {
            header: None,
            header_errno: None,
        };
        if !options.header_in {
            return Ok(sorting);
        }
        // SAFETY: standard_io's preinitializer keeps descriptor 0 open for the
        // whole process, so the borrow cannot outlive it.
        let stdin = unsafe { BorrowedFd::borrow_raw(0) };
        #[cfg(target_os = "linux")]
        let input = fastmash_sort_process::linux::duplicate(stdin);
        #[cfg(target_os = "macos")]
        let input = stdin.try_clone_to_owned();
        let input = input.map_err(|_| unsupported("unable to prepare sorting input"))?;
        let mut reader = HeaderInput::new(File::from(input))?;
        let mut intake = Intake::new(options, intake::Header::First);
        let mut record = Vec::new();
        let found = intake.header(&mut reader, &mut record, |_| Ok(()))?;
        reader
            .put_back()
            .map_err(|error| os_failure(&error, true))?;
        if found {
            sorting.header = Some(record);
            return Ok(sorting);
        }
        match intake
            .read_error()
            .map(|error| (error, error.raw_os_error()))
        {
            None => {}
            // The system sort then reads the same input and reports its own
            // error; the header's errno completes "read error (on close)".
            Some((_, Some(errno @ (5 | 21)))) => sorting.header_errno = Some(errno),
            Some((error, _)) => return Err(os_failure(error, true)),
        }
        Ok(sorting)
    }

    /// Standard input whose Input header the process already looked for
    /// itself and did not find; `header_errno` is the errno of a failed read.
    pub(super) fn read(_: Cleared, header_errno: Option<i32>) -> Self {
        Self {
            header: None,
            header_errno,
        }
    }

    /// The Input header read before sorting, which binds named fields.
    pub(super) fn header(&self) -> Option<&[u8]> {
        self.header.as_deref()
    }

    /// A requested Input header ended at clean EOF, rather than a read failure.
    /// Keeping the recorded errno here prevents callers from treating failed
    /// preparation as an empty calculation domain.
    pub(super) fn clean_header_eof(&self, options: &options::Options) -> bool {
        options.header_in && self.header.is_none() && self.header_errno.is_none()
    }

    /// Starts the system sort of standard input by `keys` (field 0 for a key
    /// that stayed unresolved, which the sort then reports).
    pub(super) fn start(self, keys: &[u64], options: &options::Options) -> Result<Sorted, Failure> {
        let session = Session::start(
            keys,
            separator(options.input),
            options.record_end == 0,
            options.ignore_case,
        )
        .map_err(|error| {
            const REASON: &str = "unable to start sort supervisor";
            match error {
                StartError::MissingCompanion(_) => unsupported_hint(
                    REASON,
                    "install fastmash-sort-supervisor in the same directory as fastmash",
                ),
                // The native spill reports an unusable TMPDIR the same way.
                StartError::Temporary { .. } => {
                    failure(format!("sort temporary I/O error: {error}\n").into_bytes())
                }
                StartError::Mismatch => unsupported_hint(
                    REASON,
                    "install fastmash and fastmash-sort-supervisor from the same release",
                ),
                StartError::Other(_) => unsupported(REASON),
            }
        })?;
        Ok(Sorted {
            session,
            header: self.header,
            header_errno: self.header_errno,
        })
    }
}

/// A running system sort.
pub(super) struct Sorted {
    session: Session,
    header: Option<Vec<u8>>,
    header_errno: Option<i32>,
}

impl Sorted {
    /// Runs `calculate` over the sorted records, given the Input header read
    /// before sorting, then completes the session: its cleanup, the sort's
    /// status, and the name of a signal that ended it.
    pub(super) fn run(
        mut self,
        calculate: impl FnOnce(&mut BufReader<ChildStdout>, Option<Vec<u8>>) -> Result<(), Failure>,
    ) -> Result<(), Failure> {
        let result = calculate(self.session.reader(), self.header.take());
        let done = self.session.finish(result.is_err());
        settle(
            done,
            result,
            self.header_errno,
            &mut super::standard_io::Stderr,
        )
    }
}

/// Standard input while its Input header is read, before the system sort
/// takes it over: every byte after the header is left for the sort. A
/// regular file is read ahead, a chunk at a time, and `put_back` then sets
/// its offset back to just after the bytes consumed. Other input, such as a
/// pipe, cannot be put back, so it is read a byte at a time, as GNU datamash
/// reads it (datamash.c open_input, unbuffered stdin).
struct HeaderInput {
    file: File,
    buffer: Vec<u8>,
    /// The unread bytes are `buffer[at..end]`.
    at: usize,
    end: usize,
}

impl HeaderInput {
    /// The read-ahead of a regular file.
    const AHEAD: usize = 64 << 10;

    fn new(mut file: File) -> Result<Self, Failure> {
        // A regular file whose offset can be read can also be set.
        let regular =
            file.metadata().is_ok_and(|about| about.is_file()) && file.stream_position().is_ok();
        let size = if regular { Self::AHEAD } else { 1 };
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(size)
            .map_err(|_| unsupported("record memory allocation failed"))?;
        buffer.resize(size, 0);
        Ok(Self {
            file,
            buffer,
            at: 0,
            end: 0,
        })
    }

    /// Returns the bytes read ahead but not consumed to the file, so that
    /// its offset is just after the bytes consumed.
    fn put_back(mut self) -> io::Result<()> {
        let ahead = self.end - self.at;
        if ahead != 0 {
            // At most AHEAD, so the offset fits.
            self.file.seek(SeekFrom::Current(-(ahead as i64)))?;
        }
        Ok(())
    }
}

impl Read for HeaderInput {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let unread = self.fill_buf()?;
        let count = unread.len().min(out.len());
        out[..count].copy_from_slice(&unread[..count]);
        self.consume(count);
        Ok(count)
    }
}

impl BufRead for HeaderInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.at == self.end {
            let count = self.file.read(&mut self.buffer)?;
            (self.at, self.end) = (0, count.min(self.buffer.len()));
        }
        Ok(&self.buffer[self.at..self.end])
    }
    fn consume(&mut self, amount: usize) {
        self.at = self.at.saturating_add(amount).min(self.end);
    }
}

/// The result of a job whose calculation over the sorted input gave
/// `result`, given the supervisor's final reply `done`. A sort killed by a
/// signal is named on `stderr` first (`killed`).
fn settle(
    done: io::Result<Message>,
    result: Result<(), Failure>,
    header_errno: Option<i32>,
    stderr: &mut impl Write,
) -> Result<(), Failure> {
    // Keep the calculation diagnostic, but never hide unconfirmed cleanup.
    if let Err(mut error) = result {
        if !done.as_ref().is_ok_and(|reply| reply.cleanup == 0) {
            append(&mut error, &[b"sort cleanup could not be confirmed\n"]);
        }
        return Err(error);
    }
    let done = done.map_err(|_| unsupported("sort supervisor completion failed"))?;
    if done.cleanup != 0 {
        return Err(unsupported("sort temporary cleanup failed"));
    }
    if done.status != 0 {
        if let Some(line) = killed(done.status) {
            // A diagnostic before the one returned, which fails as well if
            // standard error does.
            let _ = stderr.write_all(line.as_bytes());
        }
        // GNU reports the errno left by its earlier header read (datamash.c
        // close_input).
        return Err(failure(match header_errno {
            Some(21) => b"read error (on close): Is a directory\n".to_vec(),
            Some(5) => b"read error (on close): Input/output error\n".to_vec(),
            _ => b"read error (on close)\n".to_vec(),
        }));
    }
    Ok(())
}

/// The line naming a sort that the signal in wait status `status` ended, as
/// GNU datamash's users see it: datamash runs sort through `popen`'s
/// `/bin/sh`, and dash (the `/bin/sh` of Debian and Ubuntu, and of the
/// reference profile) reports a command a signal ended before datamash's
/// "read error (on close)". The line is the signal's description, as glibc's
/// `strsignal` gives it untranslated, with " (core dumped)" where the sort
/// dumped core. None for an exit, which sort explains itself, and for SIGINT
/// and SIGPIPE, which dash leaves unreported.
fn killed(status: i32) -> Option<String> {
    const DESCRIPTIONS: [&str; 31] = [
        "Hangup",
        "Interrupt",
        "Quit",
        "Illegal instruction",
        "Trace/breakpoint trap",
        "Aborted",
        "Bus error",
        "Floating point exception",
        "Killed",
        "User defined signal 1",
        "Segmentation fault",
        "User defined signal 2",
        "Broken pipe",
        "Alarm clock",
        "Terminated",
        "Stack fault",
        "Child exited",
        "Continued",
        "Stopped (signal)",
        "Stopped",
        "Stopped (tty input)",
        "Stopped (tty output)",
        "Urgent I/O condition",
        "CPU time limit exceeded",
        "File size limit exceeded",
        "Virtual timer expired",
        "Profiling timer expired",
        "Window changed",
        "I/O possible",
        "Power failure",
        "Bad system call",
    ];
    let signal = status & 0x7f;
    // 0: an exit; 0x7f: a stop, which the supervisor does not wait for.
    if matches!(signal, 0 | 2 | 13 | 0x7f) {
        return None;
    }
    let core = if status & 0x80 != 0 {
        " (core dumped)"
    } else {
        ""
    };
    Some(match signal {
        1..=31 => format!("{}{core}\n", DESCRIPTIONS[signal as usize - 1]),
        // glibc keeps 32 and 33 for itself, so its real-time signals
        // start at 34.
        34..=64 => format!("Real-time signal {}{core}\n", signal - 34),
        _ => format!("Unknown signal {signal}{core}\n"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastmash_sort_process::Kind;

    fn done(status: i32, cleanup: i32) -> io::Result<Message> {
        Ok(Message {
            kind: Kind::Done,
            status,
            cleanup,
        })
    }

    #[test]
    fn unconfirmed_cleanup_follows_the_calculation_diagnostic() {
        for reply in [done(0, 2), Err(io::Error::other("lost"))] {
            let failed = Err(failure(b"primary\n".to_vec()));
            let error = settle(reply, failed, None, &mut Vec::new());
            let error = error.err().unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(
                error.message,
                b"primary\nsort cleanup could not be confirmed\n"
            );
        }
        // A refusal that had no memory for its message keeps its cause first.
        let bare = || Failure {
            status: 77,
            message: Vec::new(),
        };
        let lost = Err(io::Error::other("lost"));
        let error = settle(lost, Err(bare()), None, &mut Vec::new());
        let error = error.err().unwrap();
        assert_eq!(error.status, 77);
        assert_eq!(
            super::super::reported(&error),
            b"memory allocation failed\nsort cleanup could not be confirmed\n"
        );
        // Confirmed cleanup adds nothing.
        let error = settle(done(0, 0), Err(bare()), None, &mut Vec::new());
        let error = error.err().unwrap();
        assert_eq!(
            super::super::reported(&error),
            b"memory allocation failed\n"
        );
    }

    #[test]
    fn a_sort_killed_by_a_signal_is_named_before_read_error_on_close() {
        // Wait statuses: a signal in the low seven bits, 0x80 for a core
        // dump, or an exit status shifted by eight.
        let plain = "read error (on close)\n";
        for (status, errno, named, diagnostic) in [
            (9, None, "Killed\n", plain),
            (15, None, "Terminated\n", plain),
            (11 | 0x80, None, "Segmentation fault (core dumped)\n", plain),
            (
                25,
                Some(5),
                "File size limit exceeded\n",
                "read error (on close): Input/output error\n",
            ),
            (35, None, "Real-time signal 1\n", plain),
            // Unreported, as by dash, and an exit, which sort explains.
            (2, None, "", plain),
            (13, Some(21), "", "read error (on close): Is a directory\n"),
            (2 << 8, None, "", plain),
        ] {
            let mut stderr = Vec::new();
            let error = settle(done(status, 0), Ok(()), errno, &mut stderr);
            let error = error.err().unwrap();
            assert_eq!(error.status, 1, "{status:#x}");
            assert_eq!(error.message, diagnostic.as_bytes(), "{status:#x}");
            assert_eq!(stderr, named.as_bytes(), "{status:#x}");
        }
        // A sort that succeeds, or one whose calculation failed first (and
        // which the supervisor may have ended itself), is not named.
        let mut stderr = Vec::new();
        assert!(settle(done(0, 0), Ok(()), None, &mut stderr).is_ok());
        let failed = Err(failure(b"primary\n".to_vec()));
        assert!(settle(done(9, 0), failed, None, &mut stderr).is_err());
        assert!(stderr.is_empty());
    }

    /// Reads one Record through `input`, then puts back what it read ahead.
    fn header_of(mut input: HeaderInput) -> Vec<u8> {
        let mut record = Vec::new();
        let read = records::read_record_terminated(&mut input, &mut record, b'\n');
        assert!(matches!(read, Ok(true)));
        input.put_back().unwrap();
        record
    }

    #[test]
    fn a_header_read_leaves_every_later_byte_for_the_sort() {
        // A regular file is read ahead a chunk at a time and put back, from
        // any offset, with the header ending before, at or past a chunk's end.
        let path = std::env::temp_dir().join(format!("fastmash-header-{}", std::process::id()));
        let ahead = HeaderInput::AHEAD;
        for (offset, length) in [
            (0, 9),
            (5, 9),
            (0, ahead - 1),
            (0, ahead),
            (3, 3 * ahead + 7),
        ] {
            let header = vec![b'h'; length];
            let mut bytes = vec![b'#'; offset];
            bytes.extend_from_slice(&header);
            bytes.extend_from_slice(b"\nb\t2\na\t1\n");
            std::fs::write(&path, &bytes).unwrap();
            let mut file = File::open(&path).unwrap();
            file.seek(SeekFrom::Start(offset as u64)).unwrap();
            let mut input = HeaderInput::new(file.try_clone().unwrap()).ok().unwrap();
            let chunk = input.fill_buf().unwrap().len();
            assert_eq!(
                chunk,
                (bytes.len() - offset).min(ahead),
                "{offset} {length}"
            );
            assert!(header_of(input) == header, "{offset} {length}");
            let at = file.stream_position().unwrap();
            assert_eq!(at, (offset + length + 1) as u64, "{offset} {length}");
            let mut rest = Vec::new();
            file.read_to_end(&mut rest).unwrap();
            assert_eq!(rest, b"b\t2\na\t1\n", "{offset} {length}");
        }
        std::fs::remove_file(&path).unwrap();
        // A pipe is read a byte at a time: nothing after the header is taken.
        let (reader, mut writer) = io::pipe().unwrap();
        writer.write_all(b"key\tvalue\nb\t2\na\t1\n").unwrap();
        drop(writer);
        let mut pipe = File::from(std::os::fd::OwnedFd::from(reader));
        let mut input = HeaderInput::new(pipe.try_clone().unwrap()).ok().unwrap();
        assert_eq!(input.fill_buf().unwrap(), b"k");
        assert_eq!(header_of(input), b"key\tvalue");
        let mut rest = Vec::new();
        pipe.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"b\t2\na\t1\n");
    }

    #[test]
    fn signals_are_described_as_by_glibc() {
        for (signal, description) in [
            (1, "Hangup"),
            (3, "Quit"),
            (6, "Aborted"),
            (7, "Bus error"),
            (8, "Floating point exception"),
            (10, "User defined signal 1"),
            (14, "Alarm clock"),
            (24, "CPU time limit exceeded"),
            (31, "Bad system call"),
            (32, "Unknown signal 32"),
            (34, "Real-time signal 0"),
            (64, "Real-time signal 30"),
        ] {
            assert_eq!(killed(signal), Some(format!("{description}\n")));
        }
        assert_eq!(killed(6 | 0x80).unwrap(), "Aborted (core dumped)\n");
        assert_eq!(killed(0x7f), None);
    }
}
