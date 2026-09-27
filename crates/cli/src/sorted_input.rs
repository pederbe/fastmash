//! Input-header preparation and completion for the fixed external sorter.
use super::{Failure, failure, options, os_failure, records, unsupported, unsupported_hint};
pub(super) use fastmash_sort_process::Session;
use fastmash_sort_process::StartError;
use std::{
    fs::File,
    io::{self, BufReader},
    os::fd::BorrowedFd,
};

pub(super) enum Header {
    Unprepared,
    Record(Vec<u8>),
    Empty,
    ReadError(io::Error),
}

pub(super) fn admit() -> Result<(), Failure> {
    admit_locale()?;
    fastmash_sort_process::admit()
        .map_err(|_| unsupported("sort process runtime requirements are not met"))
}

pub(super) fn admit_locale() -> Result<(), Failure> {
    super::locale::Policy::from_env().sorting()
}

pub(super) fn prepare(options: &options::Options) -> Result<Header, Failure> {
    if !options.header_in {
        return Ok(Header::Unprepared);
    }
    // SAFETY: standard_io's preinitializer keeps descriptor 0 open for the
    // whole process, so the borrow cannot outlive it.
    let stdin = unsafe { BorrowedFd::borrow_raw(0) };
    let input = fastmash_sort_process::linux::duplicate(stdin)
        .map_err(|_| unsupported("unable to prepare sorting input"))?;
    // Capacity one leaves every byte after the header terminator for the sorter.
    let mut reader = BufReader::with_capacity(1, File::from(input));
    let mut record = Vec::new();
    loop {
        match records::read_record_terminated(
            &mut reader,
            &mut record,
            usize::MAX,
            options.record_end,
        ) {
            Ok(false) => return Ok(Header::Empty),
            Ok(true) => {
                if options.vnlog {
                    if !super::annotated::prepare(&mut record, true)? {
                        continue;
                    }
                } else if options.skip_comments && records::is_comment(&record) {
                    continue;
                }
                return Ok(Header::Record(record));
            }
            Err(records::ReadError::Capacity) => {
                return Err(unsupported("record exceeds addressable size"));
            }
            Err(records::ReadError::Allocation) => {
                return Err(unsupported("record memory allocation failed"));
            }
            Err(records::ReadError::Io(error)) if error.raw_os_error() == Some(21) => {
                return Ok(Header::ReadError(error));
            }
            Err(records::ReadError::Io(error)) => return Err(os_failure(&error, true)),
        }
    }
}

pub(super) fn start(
    keys: &[u64],
    input: records::Separator,
    record_end: u8,
    ignore_case: bool,
) -> Result<Session, Failure> {
    let separator = match input {
        records::Separator::Whitespace => None,
        records::Separator::Literal(b) => Some(b),
    };
    Session::start(keys, separator, record_end == 0, ignore_case).map_err(|error| {
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
            StartError::Other(_) => unsupported(REASON),
        }
    })
}

pub(super) fn complete(
    session: &mut Session,
    result: Result<(), Failure>,
    header_errno: Option<i32>,
) -> Result<(), Failure> {
    let done = session.finish(result.is_err());
    // Keep the calculation diagnostic, but never hide unconfirmed cleanup.
    if let Err(mut error) = result {
        if !done.as_ref().is_ok_and(|reply| reply.cleanup == 0) {
            error
                .message
                .extend_from_slice(b"sort cleanup could not be confirmed\n");
        }
        return Err(error);
    }
    let done = done.map_err(|_| unsupported("sort supervisor completion failed"))?;
    if done.cleanup != 0 {
        return Err(unsupported("sort temporary cleanup failed"));
    }
    if done.status != 0 {
        return Err(failure(if header_errno == Some(21) {
            b"read error (on close): Is a directory\n".to_vec()
        } else {
            b"read error (on close)\n".to_vec()
        }));
    }
    Ok(())
}
