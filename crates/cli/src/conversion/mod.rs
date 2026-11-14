//! Binary80 decimal conversion and formatting, without native floating-point calls.
//!
//! Values and byte results are separate from source-derived errno policy.

#![forbid(unsafe_code)]
// Retain conversion evidence and reference paths exercised by the module tests.
#![expect(
    dead_code,
    reason = "conversion evidence includes test-only observations"
)]

#[cfg(test)]
mod default_format_tests;
#[cfg(test)]
mod generated_tests;

pub(crate) mod bytes;
pub(crate) mod caller_policy;
pub(crate) mod convert;
pub(crate) mod errno_rules;
pub(crate) mod field_policy;
pub(crate) mod format;
pub(crate) mod integer;
pub(crate) mod presentation;
pub(crate) mod profile;
pub(crate) mod records;
