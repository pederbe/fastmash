//! Native installation checks shared by archive and package callers.
#![forbid(unsafe_code)]

pub mod bytes;

#[cfg(unix)]
pub mod installed;
#[cfg(unix)]
mod notices;
#[cfg(unix)]
pub mod packages;
#[cfg(unix)]
mod receipt;
