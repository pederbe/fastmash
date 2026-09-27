//! Binary80 decimal conversion and formatting, without native floating-point calls.
//!
//! Values and byte results are separate from source-derived errno policy.

#![forbid(unsafe_code)]

pub mod bytes;
pub mod caller_policy;
pub mod convert;
pub mod errno_rules;
pub mod field_policy;
pub mod format;
pub mod integer;
pub mod presentation;
pub mod profile;
pub mod records;
