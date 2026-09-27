//! GNU's field boundary/NA decisions over separately retained parse phases.

use crate::bytes::ParseView;
use crate::profile::{Error, Profile, copy_bytes};
use crate::records::{self, ParseFailure, Parsed};
use fastmash_numeric_contract::Value80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldRange {
    pub start: usize,
    pub length: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldVariant {
    Normal,
    Broken,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPolicy {
    AcceptedFirst,
    AcceptedRetry,
    EmptyField,
    RejectedFirst,
    RejectedRetry,
    RetryLength,
    MissingField,
    SkippedNa,
    Incomplete,
}

#[derive(Clone, Debug)]
pub struct FieldResult {
    pub field: Option<FieldRange>,
    pub variant: FieldVariant,
    pub policy: Option<FieldPolicy>,
    pub phases: Vec<Parsed>,
    pub selected_phase: Option<usize>,
    pub value: Option<Value80>,
}

#[derive(Clone, Debug)]
pub struct FieldFailure {
    pub error: Error,
    pub partial: FieldResult,
    pub failed_parse: Option<ParseFailure>,
}

#[expect(
    clippy::large_enum_variant,
    reason = "variants retain owned failure evidence without allocating on failure paths"
)]
enum StepFailure {
    Other(Error),
    Parse(ParseFailure),
}

impl From<Error> for StepFailure {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

impl From<ParseFailure> for StepFailure {
    fn from(error: ParseFailure) -> Self {
        Self::Parse(error)
    }
}

/// Whether a whole field is one of GNU's case-insensitive missing-value tokens.
pub fn is_na(part: &[u8]) -> bool {
    [b"NA".as_slice(), b"N/A".as_slice(), b"NAN".as_slice()]
        .iter()
        .any(|word| part.eq_ignore_ascii_case(word))
}

#[expect(
    clippy::result_large_err,
    reason = "the detailed failure retains completed phases and an in-progress parse"
)]
pub fn field(
    record_id: &str,
    bytes: &[u8],
    field: Option<FieldRange>,
    variant: FieldVariant,
    narm: bool,
    profile: Profile,
) -> Result<FieldResult, FieldFailure> {
    let mut result = FieldResult {
        field,
        variant,
        policy: None,
        phases: Vec::new(),
        selected_phase: None,
        value: None,
    };
    #[expect(
        clippy::result_large_err,
        reason = "the local failure preserves evidence without allocating during error handling"
    )]
    let work = (|| -> Result<(), StepFailure> {
        let Some(field) = field else {
            result.policy = Some(FieldPolicy::MissingField);
            return Ok(());
        };
        let end = field
            .start
            .checked_add(field.length)
            .ok_or(Error::InvalidView)?;
        let part = bytes.get(field.start..end).ok_or(Error::InvalidView)?;
        if part.is_empty() {
            result.policy = Some(FieldPolicy::EmptyField);
            return Ok(());
        }
        if narm && is_na(part) {
            result.policy = Some(FieldPolicy::SkippedNa);
            return Ok(());
        }
        result
            .phases
            .try_reserve_exact(2)
            .map_err(|_| Error::Allocation)?;
        if variant == FieldVariant::Normal {
            let first = records::parse(
                ParseView {
                    bytes: &bytes[field.start..],
                    record_id,
                    generation: 0,
                    origin_start: field.start,
                    phase: "first-tail",
                    appended_nul: false,
                },
                profile,
                0,
            )?;
            result.phases.push(first);
            let first = &result.phases[0];
            let Some(errno) = first.errno.result else {
                result.policy = Some(FieldPolicy::Incomplete);
                return Ok(());
            };
            if errno == profile.erange() || !first.recognized || first.consumed < field.length {
                result.policy = Some(FieldPolicy::RejectedFirst);
                return Ok(());
            }
            if first.consumed == field.length {
                if first.value.is_none() {
                    result.policy = Some(FieldPolicy::Incomplete);
                    return Ok(());
                }
                result.policy = Some(FieldPolicy::AcceptedFirst);
                result.selected_phase = Some(0);
                result.value = first.value;
                return Ok(());
            }
        }
        if field.length >= 512 {
            result.policy = Some(FieldPolicy::RetryLength);
            return Ok(());
        }
        let bounded = copy_bytes(part)?;
        let retry = records::parse(
            ParseView {
                bytes: &bounded,
                record_id,
                generation: 1,
                origin_start: field.start,
                phase: "bounded-retry",
                appended_nul: true,
            },
            profile,
            0,
        )?;
        result.phases.push(retry);
        let index = result.phases.len() - 1;
        let retry = &result.phases[index];
        let Some(errno) = retry.errno.result else {
            result.policy = Some(FieldPolicy::Incomplete);
            return Ok(());
        };
        if errno == profile.erange() || !retry.recognized || retry.consumed != field.length {
            result.policy = Some(FieldPolicy::RejectedRetry);
        } else if retry.value.is_none() {
            result.policy = Some(FieldPolicy::Incomplete);
        } else {
            result.policy = Some(FieldPolicy::AcceptedRetry);
            result.selected_phase = Some(index);
            result.value = retry.value;
        }
        Ok(())
    })();
    match work {
        Ok(()) => Ok(result),
        Err(StepFailure::Other(error)) => Err(FieldFailure {
            error,
            partial: result,
            failed_parse: None,
        }),
        Err(StepFailure::Parse(failure)) => Err(FieldFailure {
            error: failure.error,
            partial: result,
            failed_parse: Some(failure),
        }),
    }
}
