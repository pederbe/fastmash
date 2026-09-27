//! Owned operation records. Serialization and reference comparisons are separate.

use crate::bytes::{self, Lexeme, ParseView, Terminator};
use crate::convert::{self, RoundEvidence};
use crate::errno_rules::{self, Effect, LogicalErrno, Route};
use crate::profile::{Error, Profile, copy_bytes};
use fastmash_numeric_contract::Value80;

#[derive(Clone, Debug)]
pub struct OwnedView {
    pub bytes: Vec<u8>,
    pub record_id: String,
    pub generation: u32,
    pub origin_start: usize,
    pub phase: &'static str,
    pub nul_offset: usize,
    pub terminator: Terminator,
}

impl OwnedView {
    pub fn capture(view: ParseView<'_>) -> Result<Self, Error> {
        view.effective()?;
        let mut record_id = String::new();
        record_id
            .try_reserve_exact(view.record_id.len())
            .map_err(|_| Error::Allocation)?;
        record_id.push_str(view.record_id);
        Ok(Self {
            bytes: copy_bytes(view.bytes)?,
            record_id,
            generation: view.generation,
            origin_start: view.origin_start,
            phase: view.phase,
            nul_offset: view.nul_offset(),
            terminator: view.terminator(),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    NoConversion,
    Decimal,
    Hexadecimal,
    Infinity,
    BareNan,
    PayloadNan,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseDisposition {
    Returned,
    DeclinedNaNPayloadConstruction,
}

#[derive(Clone, Debug)]
pub struct Parsed {
    pub view: OwnedView,
    pub disposition: ParseDisposition,
    pub category: Category,
    pub recognized: bool,
    pub value: Option<Value80>,
    pub consumed: usize,
    pub original_consumed: usize,
    pub errno: LogicalErrno,
    pub rounding: Option<RoundEvidence>,
}

#[derive(Clone, Debug, Default)]
pub struct ParsePartial {
    pub view: Option<OwnedView>,
    pub category: Option<Category>,
    pub recognized: Option<bool>,
    pub value: Option<Value80>,
    pub consumed: Option<usize>,
    pub original_consumed: Option<usize>,
    pub errno: Option<LogicalErrno>,
    pub rounding: Option<RoundEvidence>,
}

#[derive(Clone, Debug)]
pub struct ParseFailure {
    pub error: Error,
    pub entry_errno: i32,
    pub partial: ParsePartial,
}

#[expect(
    clippy::result_large_err,
    reason = "the detailed failure retains the known parse prefix without allocating during error handling"
)]
pub fn parse(
    view: ParseView<'_>,
    profile: Profile,
    entry_errno: i32,
) -> Result<Parsed, ParseFailure> {
    let mut partial = ParsePartial::default();
    let result = (|| -> Result<Parsed, Error> {
        // Admission precedes both diagnostic copying and coefficient work. Once
        // this owned view exists, later failures retain it without another parse.
        partial.view = Some(OwnedView::capture(view)?);
        let lexical = bytes::lex(view, profile)?;
        partial.consumed = Some(lexical.consumed);
        partial.original_consumed = Some(
            view.origin_start
                .checked_add(lexical.consumed)
                .ok_or(Error::InvalidView)?,
        );
        let mut disposition = ParseDisposition::Returned;
        match lexical.lexeme {
            Lexeme::NoConversion => {
                partial.category = Some(Category::NoConversion);
                partial.recognized = Some(false);
                partial.errno = Some(LogicalErrno::known(
                    entry_errno,
                    Effect::Preserve,
                    Route::NoConversionPreserve,
                    profile,
                ));
                partial.value = Some(convert::zero(false)?);
            }
            Lexeme::Infinity { negative } => {
                partial.recognized = Some(true);
                partial.category = Some(Category::Infinity);
                partial.errno = Some(LogicalErrno::known(
                    entry_errno,
                    Effect::Preserve,
                    Route::LiteralSpecialPreserve,
                    profile,
                ));
                partial.value = Some(convert::infinity(negative)?);
            }
            Lexeme::Nan {
                negative,
                complete_payload,
            } => {
                partial.recognized = Some(true);
                if complete_payload {
                    partial.category = Some(Category::PayloadNan);
                    disposition = ParseDisposition::DeclinedNaNPayloadConstruction;
                    partial.errno = Some(LogicalErrno::known(
                        entry_errno,
                        Effect::Preserve,
                        Route::NaNPayloadLexicalPreserve,
                        profile,
                    ));
                } else {
                    partial.category = Some(Category::BareNan);
                    partial.errno = Some(LogicalErrno::known(
                        entry_errno,
                        Effect::Preserve,
                        Route::LiteralSpecialPreserve,
                        profile,
                    ));
                    partial.value = Some(convert::quiet_nan(negative)?);
                }
            }
            Lexeme::Finite(token) => {
                partial.recognized = Some(true);
                partial.category = Some(if token.radix() == 10 {
                    Category::Decimal
                } else {
                    Category::Hexadecimal
                });
                let converted = match convert::finite(&token) {
                    Ok(converted) => converted,
                    Err(failure) => {
                        partial.value = failure.partial.value;
                        partial.rounding = failure.partial.rounding;
                        return Err(failure.error);
                    }
                };
                partial.value = Some(converted.value);
                partial.errno = Some(errno_rules::finite(
                    &token,
                    &converted,
                    entry_errno,
                    profile,
                ));
                partial.rounding = Some(converted.rounding);
            }
        }
        let (
            Some(category),
            Some(recognized),
            Some(consumed),
            Some(original_consumed),
            Some(errno),
        ) = (
            partial.category,
            partial.recognized,
            partial.consumed,
            partial.original_consumed,
            partial.errno,
        )
        else {
            return Err(Error::InternalInvariant);
        };
        if partial.value.is_none()
            && disposition != ParseDisposition::DeclinedNaNPayloadConstruction
        {
            return Err(Error::InternalInvariant);
        }
        let view = partial.view.take().ok_or(Error::InternalInvariant)?;
        Ok(Parsed {
            view,
            disposition,
            category,
            recognized,
            value: partial.value,
            consumed,
            original_consumed,
            errno,
            rounding: partial.rounding.take(),
        })
    })();
    result.map_err(|error| ParseFailure {
        error,
        entry_errno,
        partial,
    })
}
