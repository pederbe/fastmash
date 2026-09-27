//! The limited numeric scanner and extract-number policies from the frozen source.

use crate::bytes::{ParseView, digit, space};
use crate::convert;
use crate::errno_rules::{Effect, LogicalErrno, Route};
use crate::field_policy::FieldRange;
use crate::profile::{Error, Profile, copy_bytes};
use crate::records::{self, OwnedView, ParseFailure, Parsed};
use fastmash_numeric_contract::{IntegerOperand, Value80};

#[derive(Clone, Debug)]
pub struct IntegerPhase {
    pub view: OwnedView,
    pub value: IntegerOperand,
    pub recognized: bool,
    pub consumed: usize,
    pub original_consumed: usize,
    pub errno: LogicalErrno,
}

pub fn signed_integer(
    view: ParseView<'_>,
    base: u8,
    entry: i32,
    profile: Profile,
) -> Result<IntegerPhase, Error> {
    if !matches!(base, 8 | 10 | 16) {
        return Err(Error::InternalInvariant);
    }
    // All fallible diagnostic ownership is obtained before scanning digits.
    let captured = OwnedView::capture(view)?;
    let bytes = view.effective()?;
    let mut at = 0;
    while bytes.get(at).is_some_and(|&b| space(b)) {
        at += 1;
    }
    let negative = bytes.get(at) == Some(&b'-');
    if matches!(bytes.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    if base == 16
        && bytes.get(at) == Some(&b'0')
        && matches!(bytes.get(at + 1), Some(b'x' | b'X'))
        && bytes.get(at + 2).is_some_and(|&b| digit(b, 16).is_some())
    {
        at += 2;
    }
    let first_digit = at;
    let limit = if negative {
        1_u64 << 63
    } else {
        i64::MAX as u64
    };
    let mut magnitude = 0_u64;
    let mut overflow = false;
    while let Some(value) = bytes.get(at).and_then(|&b| digit(b, base)) {
        if !overflow {
            let base = u64::from(base);
            if magnitude > limit / base
                || (magnitude == limit / base && u64::from(value) > limit % base)
            {
                overflow = true;
                magnitude = limit;
            } else {
                magnitude = magnitude * base + u64::from(value);
            }
        }
        at += 1;
    }
    let recognized = at != first_digit;
    let consumed = if recognized { at } else { 0 };
    let value = if !recognized {
        0
    } else if negative {
        if magnitude == 1_u64 << 63 {
            i64::MIN
        } else {
            -(magnitude as i64)
        }
    } else {
        magnitude as i64
    };
    Ok(IntegerPhase {
        view: captured,
        value: IntegerOperand::I64(value),
        recognized,
        consumed,
        original_consumed: view
            .origin_start
            .checked_add(consumed)
            .ok_or(Error::InvalidView)?,
        errno: LogicalErrno::known(
            entry,
            if overflow {
                Effect::SetErange
            } else {
                Effect::Preserve
            },
            Route::SignedInteger64,
            profile,
        ),
    })
}

#[derive(Clone, Debug)]
pub enum PrimitivePhase {
    Integer(IntegerPhase),
    Floating(Parsed),
}

#[derive(Clone, Copy, Debug)]
pub enum TokenValue {
    Integer(IntegerOperand),
    Float(Value80),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScannerRejection {
    NonDigitStart,
    AlphabeticTail,
    Range,
    RequiredFactUnavailable,
}

#[derive(Clone, Debug)]
pub struct ScannerResult {
    pub phases: Vec<PrimitivePhase>,
    pub value: Option<TokenValue>,
    pub consumed: Option<usize>,
    pub final_errno: Option<i32>,
    pub rejection: Option<ScannerRejection>,
}

#[derive(Clone, Debug)]
pub struct ScannerFailure {
    pub error: Error,
    pub partial: ScannerResult,
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

#[expect(
    clippy::result_large_err,
    reason = "the detailed failure retains completed phases and an in-progress parse"
)]
pub fn scanner(
    record_id: &str,
    bytes: &[u8],
    entry: i32,
    profile: Profile,
) -> Result<ScannerResult, ScannerFailure> {
    let view = ParseView {
        bytes,
        record_id,
        generation: 0,
        origin_start: 0,
        phase: "scanner-integer",
        appended_nul: false,
    };
    let mut result = ScannerResult {
        phases: Vec::new(),
        value: None,
        consumed: None,
        final_errno: None,
        rejection: None,
    };
    #[expect(
        clippy::result_large_err,
        reason = "the local failure preserves evidence without allocating during error handling"
    )]
    let work = (|| -> Result<(), StepFailure> {
        let effective = view.effective()?;
        if !effective.first().is_some_and(u8::is_ascii_digit) {
            result.rejection = Some(ScannerRejection::NonDigitStart);
            result.consumed = Some(0);
            result.final_errno = Some(entry);
            return Ok(());
        }
        result
            .phases
            .try_reserve_exact(2)
            .map_err(|_| Error::Allocation)?;
        let integer = signed_integer(view, 10, entry, profile)?;
        result.consumed = Some(integer.consumed);
        result.final_errno = integer.errno.result;
        result.value = Some(TokenValue::Integer(integer.value));
        result.phases.push(PrimitivePhase::Integer(integer));
        if effective.get(result.consumed.ok_or(Error::InternalInvariant)?) == Some(&b'.') {
            let entry = result.final_errno.ok_or(Error::InternalInvariant)?;
            // An integer token has not been selected if floating retry is needed.
            // Keep its phase, but leave the final result unproduced until retry.
            result.value = None;
            result.consumed = None;
            result.final_errno = None;
            let floating = records::parse(
                ParseView {
                    phase: "scanner-floating",
                    ..view
                },
                profile,
                entry,
            )?;
            result.consumed = Some(floating.consumed);
            result.final_errno = floating.errno.result;
            result.value = floating.value.map(TokenValue::Float);
            result.phases.push(PrimitivePhase::Floating(floating));
        }
        if effective
            .get(result.consumed.ok_or(Error::InternalInvariant)?)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            result.rejection = Some(ScannerRejection::AlphabeticTail);
        } else if result.final_errno == Some(profile.erange()) {
            result.rejection = Some(ScannerRejection::Range);
        } else if result.final_errno.is_none() || result.value.is_none() {
            result.rejection = Some(ScannerRejection::RequiredFactUnavailable);
        }
        if result.rejection.is_some() {
            result.value = None;
        }
        Ok(())
    })();
    match work {
        Ok(()) => Ok(result),
        Err(StepFailure::Other(error)) => Err(ScannerFailure {
            error,
            partial: result,
            failed_parse: None,
        }),
        Err(StepFailure::Parse(failure)) => Err(ScannerFailure {
            error: failure.error,
            partial: result,
            failed_parse: Some(failure),
        }),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GetnumMode {
    Natural,
    Integer,
    PositiveHex,
    Octal,
    PositiveDecimal,
    SignedDecimal,
}

impl GetnumMode {
    fn source(self) -> (&'static [u8], u8, bool) {
        match self {
            Self::Natural => (b"0123456789", 10, false),
            Self::Integer => (b"-+0123456789", 10, false),
            Self::PositiveHex => (b"0123456789abcdefABCDEF", 16, false),
            Self::Octal => (b"01234567", 8, false),
            Self::PositiveDecimal => (b".0123456789", 10, true),
            Self::SignedDecimal => (b"+-.0123456789", 10, true),
        }
    }
}

#[derive(Clone, Debug)]
pub struct GetnumResult {
    pub span: Option<FieldRange>,
    pub phase: Option<PrimitivePhase>,
    pub value: Option<Value80>,
    pub final_errno: Option<i32>,
    pub zero_on_error: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct GetnumFailure {
    pub error: Error,
    pub partial: GetnumResult,
    pub failed_parse: Option<ParseFailure>,
}

#[expect(
    clippy::result_large_err,
    reason = "the detailed failure retains the selected span and an in-progress parse"
)]
pub fn getnum(
    record_id: &str,
    bytes: &[u8],
    mode: GetnumMode,
    profile: Profile,
) -> Result<GetnumResult, GetnumFailure> {
    let mut result = GetnumResult {
        span: None,
        phase: None,
        value: None,
        final_errno: None,
        zero_on_error: None,
    };
    #[expect(
        clippy::result_large_err,
        reason = "the local failure preserves evidence without allocating during error handling"
    )]
    let work = (|| -> Result<(), StepFailure> {
        let original = ParseView {
            bytes,
            record_id,
            generation: 0,
            origin_start: 0,
            phase: "getnum",
            appended_nul: false,
        };
        let effective = original.effective()?;
        let (set, base, floating) = mode.source();
        let start = effective
            .iter()
            .position(|b| set.contains(b))
            .unwrap_or(effective.len());
        let length = effective[start..]
            .iter()
            .take_while(|b| set.contains(b))
            .count();
        result.span = Some(FieldRange { start, length });
        let bounded = copy_bytes(&effective[start..start + length])?;
        let view = ParseView {
            bytes: &bounded,
            record_id,
            generation: 1,
            origin_start: start,
            phase: if floating {
                "getnum-floating"
            } else {
                "getnum-integer"
            },
            appended_nul: true,
        };
        if floating {
            let parsed = records::parse(view, profile, 0)?;
            result.value = parsed.value;
            result.final_errno = parsed.errno.result;
            result.phase = Some(PrimitivePhase::Floating(parsed));
        } else {
            let parsed = signed_integer(view, base, 0, profile)?;
            let IntegerOperand::I64(integer) = parsed.value else {
                return Err(Error::InternalInvariant.into());
            };
            result.final_errno = parsed.errno.result;
            result.phase = Some(PrimitivePhase::Integer(parsed));
            // Retain the primitive before the exact promotion, including its
            // saturated integer when the caller will substitute zero on errno.
            result.value = Some(convert::integer_value(integer)?);
        }
        result.zero_on_error = result.final_errno.map(|errno| errno != 0);
        if result.zero_on_error == Some(true) {
            result.value = Some(convert::zero(false)?);
        } else if result.final_errno.is_none() {
            result.value = None;
        }
        Ok(())
    })();
    match work {
        Ok(()) => Ok(result),
        Err(StepFailure::Other(error)) => Err(GetnumFailure {
            error,
            partial: result,
            failed_parse: None,
        }),
        Err(StepFailure::Parse(failure)) => Err(GetnumFailure {
            error: failure.error,
            partial: result,
            failed_parse: Some(failure),
        }),
    }
}
