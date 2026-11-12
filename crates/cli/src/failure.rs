//! Failures and their exit statuses: an Error (1), a Refusal (77) or an
//! Internal failure (70; other internal failures refuse with 77), and which
//! status a command exits with when one failure follows another.

use super::terminal_style::{Role, Style};
use std::io::{self, Write};

#[derive(Clone)]
pub(super) struct Failure {
    pub(super) status: i32,
    pub(super) message: Vec<u8>,
    pub(super) hint: Option<&'static [u8]>,
}
impl Failure {
    /// Select guidance where the cause is known, independently of its wording.
    pub(super) fn with_hint(mut self, hint: &'static [u8]) -> Self {
        // An empty message reports allocation failure instead of the original cause.
        if !self.message.is_empty() {
            self.hint = Some(hint);
        }
        self
    }
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
        hint: None,
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

/// Writes the existing failure prefix without allocating, resetting even if
/// the prefix fails. Startup can still attempt its fallback body independently.
pub(super) fn write_prefix(
    writer: &mut impl Write,
    program: &[u8],
    style: Style,
) -> io::Result<()> {
    let prefix = writer
        .write_all(style.start(Role::Failure))
        .and_then(|()| {
            if style.enabled() {
                writer.write_all("✗ ".as_bytes())
            } else {
                Ok(())
            }
        })
        .and_then(|()| writer.write_all(program))
        .and_then(|()| writer.write_all(b": "));
    // A partially written styled prefix can leave a terminal colored. Attempt
    // reset even on failure, while retaining the original write error.
    let reset = writer.write_all(style.reset());
    prefix?;
    reset
}

/// Writes a failure's existing prefix and body without allocating.
/// Reset before the body so supplied byte content and hints stay unstyled.
pub(super) fn write(
    writer: &mut impl Write,
    program: &[u8],
    error: &Failure,
    style: Style,
) -> io::Result<()> {
    write_prefix(writer, program, style)?;
    writer.write_all(reported(error))?;
    if let Some(hint) = error.hint {
        writer.write_all(b"hint: ")?;
        writer.write_all(hint)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()
}

/// Static reporting text shared by the producers that select it.
pub(super) mod hints {
    pub const OPERATION: &[u8] = b"choose an operation and field, for example fastmash sum 1";
    pub const INVALID_OPERATION: &[u8] =
        b"use fastmash --help to list operations, for example sum 1 or mean 2";
    pub const MISSING_FIELD: &[u8] =
        b"supply a field number starting at 1, or use --header-in with a header name";
    pub const FIELD_RANGE: &[u8] = b"use ascending numeric endpoints, for example 2-4. Use commas for separate fields, such as 2,4";
    pub const FIELD_PAIR: &[u8] =
        b"write pairs as LEFT:RIGHT, for example 1:2 or 1:2,3:4. Pairs cannot contain ranges";
    pub const FIELD: &[u8] =
        b"field numbers start at 1. Named fields need an input header and --header-in or -H";
    pub const SEED: &[u8] = b"use a nonnegative decimal integer, for example --seed=7";
    pub const ROUND: &[u8] =
        b"use --round=1 through --round=50. For no decimal places, use --format=%.0f";
    pub const DELIMITER: &[u8] = b"use one literal byte, such as ','. Escapes such as '\\t' are not expanded. Tab is the default input separator";
    pub const PAIR_REQUIRED: &[u8] = b"write each field pair as LEFT:RIGHT, for example 1:2";
    pub const PAIR_FORBIDDEN: &[u8] =
        b"use a single field, a list or a range, for example sum 1,2 or sum 1-3";
    pub const FORMAT: &[u8] = b"use one %e, %f, %g or %a conversion, for example --format=%.2f. The format string may contain at most 99 bytes";
    pub const PERCENTILE: &[u8] =
        b"use an integer percentile from 1 through 100, for example perc:95 2";
    pub const STRBIN: &[u8] = b"use a positive integer bucket count, for example strbin:10 1";
    pub const GETNUM: &[u8] =
        b"getnum types are n, i, h, o, p and d. For example, getnum:i 1 extracts a signed integer";
    pub const SEPARATE_OPERATIONS: &[u8] = b"run per-row operations and aggregate operations in separate commands. Per-row operations cannot use grouping keys";
    pub const SELECTION: &[u8] = b"for example, top:5 2 selects up to 5 records ranked by field 2";
    pub const COMPARISON_MODIFIER: &[u8] = b"rank selects a result and limit caps ranked keys. Both numbers start at 1. For example, rank 1 percent limit 5";
    pub const COMPARE_INPUTS: &[u8] = b"supply two input paths, an operation and fields, for example compare before.tsv after.tsv sum 2";
    pub const COMPARE_STDIN: &[u8] =
        b"use '-' for one input only. Save the other dataset in a file";
    pub const COMPARE_OPERATIONS: &[u8] =
        b"use a numerical summary such as sum 2, mean 2 or wmean 2:3";
    pub const COMPARISON_RANK: &[u8] = b"rank counts calculated results from 1 in request order, excluding keys and the derived report columns";
    pub const HEALTH_RULE: &[u8] = b"supply a rule before validate, for example health type 1 number validate or health width 3 validate";
    pub const HEALTH_TYPE: &[u8] = b"write type FIELD TYPE, for example health type 1 number";
    pub const RESULT_HEADERS: &[u8] = b"add --header-out for generated labels. Use -H only when the first input record names the fields";
    pub const RESULT_INDEX: &[u8] = b"give each calculated result at most one name, for example --result-name=1:total --result-name=2:average";
    pub const RESULT_SEPARATOR: &[u8] =
        b"remove separator bytes from the name, or use --csv-out to quote names containing them";
    pub const CSV_OUTPUT: &[u8] = b"CSV output uses commas. Remove --output-delimiter, or omit CSV output to choose a text delimiter";
    pub const CSV_INPUT: &[u8] = b"CSV input uses its own comma and quote rules. Remove explicit text separators and --skip-comments";
    pub const CSV_RECORD_END: &[u8] = b"CSV uses newline or CRLF record endings. Remove --zero-terminated, or use ordinary text instead of CSV";
    pub const CSV_VNLOG: &[u8] =
        b"choose quoted CSV or --vnlog annotated text. They cannot be used together";
    pub const LANGUAGE_SORT: &[u8] = b"LC_ALL overrides LC_COLLATE. If LC_ALL is set, copy its value to LC_NUMERIC and unset LC_ALL before setting LC_COLLATE=C";
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
    Failure {
        status: 1,
        message,
        hint: None,
    }
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
        hint: None,
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
    use super::*;
    use crate::terminal_style::{ColorMode, Destination, Detection};

    fn style(mode: ColorMode) -> Style {
        Detection::default().style(mode, Destination::Stderr)
    }

    struct Sink {
        bytes: Vec<u8>,
        fail_at: Option<usize>,
        zero: bool,
        fail_flush: bool,
    }
    impl Write for Sink {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let remaining = self.fail_at.map_or(usize::MAX, |at| at - self.bytes.len());
            if remaining == 0 {
                return Err(io::Error::from_raw_os_error(28));
            }
            if self.zero {
                return Ok(0);
            }
            let count = bytes.len().min(2).min(remaining);
            self.bytes.extend_from_slice(&bytes[..count]);
            Ok(count)
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush {
                Err(io::Error::from_raw_os_error(28))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn failure_presentation_keeps_all_body_bytes_and_outcome_text() {
        for status in [1, 77, 70] {
            let error = Failure {
                status,
                message: b"first\nsecond\0\xff\x1b[36m\nhint: use --help\n".to_vec(),
                hint: None,
            };
            for (mode, prefix) in [
                (
                    ColorMode::Always,
                    b"\x1b[1;38;2;255;92;87m\xe2\x9c\x97 fastmash: \x1b[0m".as_slice(),
                ),
                (ColorMode::Never, b"fastmash: "),
            ] {
                let mut bytes = Vec::new();
                write(&mut bytes, b"fastmash", &error, style(mode)).unwrap();
                assert_eq!(bytes, [prefix, &error.message].concat());
                assert_eq!(error.status, status);
            }
        }
        let mut bytes = Vec::new();
        write(
            &mut bytes,
            b"\xffprogram",
            &unsupported("checked limit"),
            style(ColorMode::Always),
        )
        .unwrap();
        assert_eq!(
            bytes,
            b"\x1b[1;38;2;255;92;87m\xe2\x9c\x97 \xffprogram: \x1b[0mchecked limit\n"
        );
    }

    #[test]
    fn failure_presentation_keeps_the_allocation_failure_fallback() {
        MESSAGE_FAILS.with(|fails| fails.set(true));
        let refusal =
            unsupported("unavailable memory for this diagnostic").with_hint(hints::LANGUAGE_SORT);
        assert!(refusal.message.is_empty());
        let mut bytes = Vec::new();
        write(&mut bytes, b"fastmash", &refusal, style(ColorMode::Always)).unwrap();
        assert_eq!(
            bytes,
            b"\x1b[1;38;2;255;92;87m\xe2\x9c\x97 fastmash: \x1b[0mmemory allocation failed\n"
        );
        assert_eq!(refusal.status, 77);
    }

    #[test]
    fn selected_guidance_survives_cause_changes_and_follows_context() {
        let original = crate::grammar::parse(&["sum".into()]).err().unwrap();
        let mut error = original.clone();
        error.message = b"changed cause with raw bytes: \0\xff\n".to_vec();
        append(&mut error, &[b"source context\n"]);
        for mode in [ColorMode::Never, ColorMode::Always] {
            let mut bytes = Vec::new();
            write(&mut bytes, b"fastmash", &error, style(mode)).unwrap();
            let mut expected = Vec::new();
            write_prefix(&mut expected, b"fastmash", style(mode)).unwrap();
            expected.extend_from_slice(b"changed cause with raw bytes: \0\xff\nsource context\nhint: supply a field number starting at 1, or use --header-in with a header name\n");
            assert_eq!(bytes, expected);
            assert_eq!(error.status, 1);
        }
    }

    #[test]
    fn reporting_does_not_infer_guidance_from_cause_text() {
        let error = failure(b"invalid seed\n".to_vec());
        let mut bytes = Vec::new();
        write(&mut bytes, b"fastmash", &error, style(ColorMode::Never)).unwrap();
        assert_eq!(bytes, b"fastmash: invalid seed\n");
    }

    #[test]
    fn allocation_failure_fallback_uses_default_stderr_detection() {
        for stderr_terminal in [false, true] {
            for (environment, eligible) in [
                (vec![], true),
                (vec![("NO_COLOR", "")], true),
                (vec![("NO_COLOR", "1")], false),
                (vec![("TERM", "dumb")], false),
            ] {
                let mut command = std::process::Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "failure::tests::argument_collection_child",
                        "--nocapture",
                    ])
                    .env_clear()
                    .env("LC_ALL", "C")
                    .env("TERM", "xterm")
                    .env("FASTMASH_UNIT_ARGUMENT_FAILURE", "1")
                    .envs(environment.iter().copied());
                let output =
                    crate::test_terminal::output(&mut command, b"", false, stderr_terminal);
                assert_eq!(output.status.code(), Some(77), "{output:?}");
                assert_eq!(
                    output
                        .stderr
                        .starts_with(b"\x1b[1;38;2;255;92;87m\xe2\x9c\x97 fastmash: \x1b[0m"),
                    stderr_terminal && eligible,
                    "{environment:?}: {output:?}"
                );
                assert_eq!(
                    crate::test_terminal::strip_styles(&output.stderr),
                    [
                        if stderr_terminal && eligible {
                            "✗ ".as_bytes()
                        } else {
                            b""
                        },
                        b"fastmash: memory allocation failed\n"
                    ]
                    .concat()
                );
            }
        }
    }

    #[test]
    fn argument_collection_child() {
        if std::env::var_os("FASTMASH_UNIT_ARGUMENT_FAILURE").is_none() {
            return;
        }
        crate::command_memory::FAIL_RESERVATION.with(|at| at.set(Some(0)));
        MESSAGE_FAILS.with(|fails| fails.set(true));
        crate::run_process();
        panic!("argument collection failure must exit");
    }

    #[test]
    fn failure_presentation_preserves_short_zero_write_and_flush_failures() {
        for error in [
            numeric_failure(fastmash_portable_numerics::NumericFailure::Invariant),
            crate::grammar::parse(&["sum".into()]).err().unwrap(),
        ] {
            for mode in [ColorMode::Never, ColorMode::Always] {
                let mut expected = Vec::new();
                write(&mut expected, b"fastmash", &error, style(mode)).unwrap();
                for fail_at in 0..expected.len() {
                    let mut sink = Sink {
                        bytes: Vec::new(),
                        fail_at: Some(fail_at),
                        zero: false,
                        fail_flush: false,
                    };
                    assert_eq!(
                        write(&mut sink, b"fastmash", &error, style(mode))
                            .unwrap_err()
                            .raw_os_error(),
                        Some(28)
                    );
                    assert_eq!(sink.bytes, expected[..fail_at]);
                }
                let mut sink = Sink {
                    bytes: Vec::new(),
                    fail_at: None,
                    zero: true,
                    fail_flush: false,
                };
                assert_eq!(
                    write(&mut sink, b"fastmash", &error, style(mode))
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::WriteZero
                );
                assert!(sink.bytes.is_empty());
                sink.zero = false;
                sink.fail_flush = true;
                assert_eq!(
                    write(&mut sink, b"fastmash", &error, style(mode))
                        .unwrap_err()
                        .raw_os_error(),
                    Some(28)
                );
                assert_eq!(sink.bytes, expected);
            }
        }
    }

    #[test]
    fn a_partial_prefix_attempts_reset_and_keeps_the_first_error() {
        struct Transient {
            bytes: Vec<u8>,
            fail_at: Option<usize>,
            fail_reset: bool,
        }
        impl Write for Transient {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.fail_at == Some(self.bytes.len()) {
                    self.fail_at = None;
                    return Err(io::Error::from_raw_os_error(28));
                }
                if self.fail_at.is_none() && self.fail_reset {
                    return Err(io::Error::from_raw_os_error(5));
                }
                let remaining = self.fail_at.map_or(usize::MAX, |at| at - self.bytes.len());
                let count = bytes.len().min(2).min(remaining);
                self.bytes.extend_from_slice(&bytes[..count]);
                Ok(count)
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("a failed prefix stops before the body and flush")
            }
        }
        let prefix = b"\x1b[1;38;2;255;92;87m\xe2\x9c\x97 fastmash: ";
        for fail_at in 0..prefix.len() {
            for fail_reset in [false, true] {
                let mut sink = Transient {
                    bytes: Vec::new(),
                    fail_at: Some(fail_at),
                    fail_reset,
                };
                assert_eq!(
                    write(
                        &mut sink,
                        b"fastmash",
                        &unsupported("checked limit"),
                        style(ColorMode::Always),
                    )
                    .unwrap_err()
                    .raw_os_error(),
                    Some(28)
                );
                let reset = if fail_reset {
                    b"".as_slice()
                } else {
                    b"\x1b[0m"
                };
                assert_eq!(sink.bytes, [&prefix[..fail_at], reset].concat());
                let mut startup_sink = Transient {
                    bytes: Vec::new(),
                    fail_at: Some(fail_at),
                    fail_reset,
                };
                assert_eq!(
                    write_prefix(&mut startup_sink, b"fastmash", style(ColorMode::Always))
                        .unwrap_err()
                        .raw_os_error(),
                    Some(28)
                );
                // Startup keeps the fallback body independent of prefix errors.
                let refusal = Failure {
                    status: 77,
                    message: Vec::new(),
                    hint: None,
                };
                let body = reported(&refusal);
                let result = startup_sink.write_all(body);
                if fail_reset {
                    assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
                } else {
                    result.unwrap();
                    assert_eq!(
                        startup_sink.bytes,
                        [&prefix[..fail_at], reset, body].concat()
                    );
                }
            }
        }
    }

    #[test]
    fn styled_internal_failure_completion_keeps_precedence_and_later_attempts() {
        for mode in [ColorMode::Never, ColorMode::Always] {
            for diagnostic_fails in [false, true] {
                for close_error in [28, 5] {
                    let mut output = Vec::new();
                    let mut closed = false;
                    let mut diagnostics = Vec::new();
                    let mut attempts = 0;
                    let buffer =
                        crate::buffered_stdout::BufferedStdout::new(&mut output, 128, false)
                            .unwrap();
                    let internal =
                        numeric_failure(fastmash_portable_numerics::NumericFailure::Invariant);
                    let status = crate::command_output::complete(
                        buffer,
                        Err(internal),
                        |_| {
                            closed = true;
                            Err(io::Error::from_raw_os_error(close_error))
                        },
                        &mut |failure| {
                            attempts += 1;
                            if diagnostic_fails {
                                let mut sink = Sink {
                                    bytes: Vec::new(),
                                    fail_at: Some(2),
                                    zero: false,
                                    fail_flush: false,
                                };
                                write(&mut sink, b"fastmash", failure, style(mode)).is_ok()
                            } else {
                                write(&mut diagnostics, b"fastmash", failure, style(mode)).is_ok()
                            }
                        },
                    );
                    assert_eq!(status, if diagnostic_fails { 1 } else { 70 });
                    assert!(closed);
                    assert_eq!(attempts, 2);
                    if !diagnostic_fails {
                        let prefix = if mode == ColorMode::Always {
                            b"\x1b[1;38;2;255;92;87m\xe2\x9c\x97 fastmash: \x1b[0m".as_slice()
                        } else {
                            b"fastmash: ".as_slice()
                        };
                        let later = if close_error == 28 {
                            b"write error: No space left on device\n".as_slice()
                        } else {
                            b"unsupported output I/O error\n".as_slice()
                        };
                        assert_eq!(
                            diagnostics,
                            [
                                prefix,
                                b"internal numerical invariant failure\n",
                                prefix,
                                later
                            ]
                            .concat()
                        );
                    }
                }
            }
        }
    }

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
