//! Failures and their exit statuses: an Error (1), a Refusal (77) or an
//! Internal failure (70; other internal failures refuse with 77), and which
//! status a command exits with when one failure follows another.

use std::io;

#[derive(Clone)]
pub(super) struct Failure {
    pub(super) status: i32,
    pub(super) message: Vec<u8>,
}
#[cfg(test)]
thread_local! {
    /// Leaves the next refusal no memory for its message.
    pub(super) static MESSAGE_FAILS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
pub(super) fn unsupported(reason: &str) -> Failure {
    // An allocation failure can leave no memory for its own message: the
    // refusal then has none, and its report names the cause (`reported`).
    let mut message = Vec::new();
    #[cfg(test)]
    let fails = MESSAGE_FAILS.with(|fails| fails.replace(false));
    #[cfg(not(test))]
    let fails = false;
    if !fails && message.try_reserve_exact(reason.len() + 1).is_ok() {
        message.extend_from_slice(reason.as_bytes());
        message.push(b'\n');
    }
    Failure {
        status: 77,
        message,
    }
}
/// What a refusal that had no memory for its message reports.
const NO_MEMORY: &[u8] = b"memory allocation failed\n";
/// The text that reports `error`: its message, or the cause of a refusal
/// that had no memory for one.
pub(super) fn reported(error: &Failure) -> &[u8] {
    if error.message.is_empty() {
        NO_MEMORY
    } else {
        &error.message
    }
}
/// Appends `parts` to `error`'s message, where there is memory for them. A
/// refusal that had no memory for its message first takes the cause its
/// report names (`reported`), so that the parts follow the cause instead of
/// standing in for it.
pub(super) fn append(error: &mut Failure, parts: &[&[u8]]) {
    let cause = if error.message.is_empty() {
        NO_MEMORY
    } else {
        b""
    };
    let size = parts
        .iter()
        .fold(cause.len(), |size, part| size.saturating_add(part.len()));
    if error.message.try_reserve_exact(size).is_ok() {
        error.message.extend_from_slice(cause);
        for part in parts {
            error.message.extend_from_slice(part);
        }
    }
}
pub(super) fn unsupported_hint(reason: &str, hint: &str) -> Failure {
    let mut error = unsupported(reason);
    append(&mut error, &[b"hint: ", hint.as_bytes(), b"\n"]);
    error
}
pub(super) fn failure(message: Vec<u8>) -> Failure {
    Failure { status: 1, message }
}
pub(super) fn numeric_failure(error: fastmash_portable_numerics::NumericFailure) -> Failure {
    use fastmash_portable_numerics::NumericFailure;
    let (status, message) = match error {
        NumericFailure::UnsupportedDomain => (
            77,
            "numerical operation result is outside the supported domain\n",
        ),
        NumericFailure::Capacity => (77, "numerical arithmetic capacity exceeded\n"),
        NumericFailure::Allocation => (77, "numerical arithmetic memory allocation failed\n"),
        NumericFailure::Invariant => (70, "internal numerical invariant failure\n"),
        NumericFailure::TableIdentity => (70, "numerical table identity check failed\n"),
    };
    Failure {
        status,
        message: message.as_bytes().to_vec(),
    }
}
pub(super) fn conversion_failure(error: fastmash_conversion::profile::Error) -> Failure {
    use fastmash_conversion::profile::Error;
    unsupported(match error {
        Error::InputCapacity => "numeric conversion input exceeds 8192-byte limit",
        Error::OutputCapacity => "numeric output exceeds 16384-byte limit",
        Error::CoefficientCapacity => "numeric conversion coefficient capacity exceeded",
        Error::IntermediateCapacity => "numeric conversion intermediate capacity exceeded",
        Error::PowerCapacity => "numeric conversion power capacity exceeded",
        Error::ShiftCapacity => "numeric conversion shift capacity exceeded",
        Error::UnsupportedProfile => "unsupported numerical conversion profile",
        Error::UnsupportedFormat => "unsupported numerical output format",
        Error::Allocation => "numeric conversion memory allocation failed",
        Error::InvalidView | Error::InternalInvariant | Error::Encoding(_) => {
            "internal numeric conversion failure"
        }
    })
}
pub(super) fn os_failure(error: &io::Error, reading: bool) -> Failure {
    match (reading, error.raw_os_error()) {
        (true, Some(9)) => failure(b"read error: Bad file descriptor\n".to_vec()),
        (false, Some(9)) => failure(b"write error: Bad file descriptor\n".to_vec()),
        (true, Some(21)) => failure(b"read error: Is a directory\n".to_vec()),
        (false, Some(28)) => failure(b"write error: No space left on device\n".to_vec()),
        (true, _) => unsupported("unsupported input I/O error"),
        (false, _) => unsupported("unsupported output I/O error"),
    }
}

/// The exit status when a failure with status `later` follows an earlier
/// outcome with status `earlier` (0 for success), such as a failure to
/// complete the output after the command's own failure: an Internal failure
/// (70) outranks a Refusal (77), which outranks an Error (1). A failure to
/// report a diagnostic is not combined: it makes the status 1 whatever came
/// before, as GNU's `close_stdout` does.
pub(super) fn combine(earlier: i32, later: i32) -> i32 {
    let rank = |status| match status {
        0 => 0,
        70 => 3,
        77 => 2,
        _ => 1,
    };
    if rank(later) > rank(earlier) {
        later
    } else {
        earlier
    }
}

#[cfg(test)]
mod tests {
    use super::combine;

    #[test]
    fn an_internal_failure_outranks_a_refusal_which_outranks_an_error() {
        for (earlier, later, status) in [
            (0, 1, 1),
            (0, 77, 77),
            (0, 70, 70),
            (1, 1, 1),
            (1, 77, 77),
            (77, 1, 77),
            (70, 1, 70),
            (70, 77, 70),
            (77, 70, 70),
        ] {
            assert_eq!(combine(earlier, later), status, "{earlier} then {later}");
        }
    }
}
