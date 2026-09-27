//! Source-derived errno effects; no operating-system errno or fenv is accessed.

use crate::bytes::FiniteToken;
use crate::convert::{Conversion, Direction};
use crate::profile::Profile;
use fastmash_numeric_contract::{BasisKind, Fact};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Preserve,
    SetErange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    RecognizedZeroPreserve,
    NoConversionPreserve,
    LiteralSpecialPreserve,
    NaNPayloadLexicalPreserve,
    HexadecimalHelper,
    DecimalNonTinyFinitePreserve,
    OneSignificantDigitExponent,
    DecimalConservativeExtreme,
    DecimalRouteNotQualified,
    SignedInteger64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    DecimalRouteNotQualified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotApplicable {}

#[derive(Clone, Copy, Debug)]
pub struct SourceBasis {
    pub kind: BasisKind,
    pub route: Route,
}

pub type EffectFact = Fact<Effect, SourceBasis, Unavailable, NotApplicable>;

#[derive(Clone, Copy, Debug)]
pub struct LogicalErrno {
    pub entry: i32,
    pub effect: EffectFact,
    pub result: Option<i32>,
    pub new_range_event: Option<bool>,
    pub route: Route,
}

impl LogicalErrno {
    pub fn known(entry: i32, effect: Effect, route: Route, profile: Profile) -> Self {
        Self {
            entry,
            effect: Fact::Known {
                value: effect,
                basis: SourceBasis {
                    kind: BasisKind::SourcePrediction,
                    route,
                },
            },
            result: Some(if effect == Effect::Preserve {
                entry
            } else {
                profile.erange()
            }),
            new_range_event: Some(effect == Effect::SetErange),
            route,
        }
    }

    pub fn unqualified(entry: i32) -> Self {
        Self {
            entry,
            effect: Fact::Unavailable(Unavailable::DecimalRouteNotQualified),
            result: None,
            new_range_event: None,
            route: Route::DecimalRouteNotQualified,
        }
    }
}

pub fn finite(
    token: &FiniteToken<'_>,
    converted: &Conversion,
    entry: i32,
    profile: Profile,
) -> LogicalErrno {
    if converted.exact_zero {
        return LogicalErrno::known(
            entry,
            Effect::Preserve,
            Route::RecognizedZeroPreserve,
            profile,
        );
    }
    let helper = if converted.overflow || (converted.ordinary_p64_tiny && converted.target_inexact)
    {
        Effect::SetErange
    } else {
        Effect::Preserve
    };
    if token.radix() == 16 {
        return LogicalErrno::known(entry, helper, Route::HexadecimalHelper, profile);
    }
    if matches!(
        converted.shortcut,
        Some(Direction::Overflow | Direction::UnderflowToZero)
    ) {
        return LogicalErrno::known(
            entry,
            Effect::SetErange,
            Route::DecimalConservativeExtreme,
            profile,
        );
    }
    if converted.exact_at_least_min_normal && !converted.overflow {
        return LogicalErrno::known(
            entry,
            Effect::Preserve,
            Route::DecimalNonTinyFinitePreserve,
            profile,
        );
    }
    if token.one_digit_tiny_decimal() {
        return LogicalErrno::known(entry, helper, Route::OneSignificantDigitExponent, profile);
    }
    LogicalErrno::unqualified(entry)
}
