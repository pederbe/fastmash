//! Shared exact observations from native command transports.
use crate::fixture::{Expected, Held, decode, encode};
use crate::held_output::Destination;
use serde::Serialize;
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Debug, Serialize)]
pub struct Observed {
    pub argv: Vec<String>,
    // Hex arguments retain invalid UTF-8 without relying on a JSON encoder's
    // replacement-character display. Actual launch always uses these raw bytes.
    pub argv_hex: Vec<String>,
    pub executable: PathBuf,
    pub env: BTreeMap<String, String>,
    pub process_group: u32,
    pub returncode: i32,
    pub timed_out: bool,
    #[cfg(windows)]
    pub spawn_millis: u128,
    #[cfg(windows)]
    pub elapsed_millis: u128,
    pub overflow: bool,
    pub cleanup_confirmed: bool,
    pub stdin_held_until_exit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held: Option<Held>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<Destination>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acknowledged: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_error_mechanism: Option<String>,
    pub stdout_hex: String,
    pub stderr_hex: String,
}

impl Observed {
    pub fn matches(&self, expected: &Expected) -> bool {
        !self.timed_out
            && !self.overflow
            && self.cleanup_confirmed
            && self.returncode == expected.returncode
            && self.stdout_hex == expected.stdout_hex
            && normalize_sort_name(&self.stderr_hex) == expected.stderr_hex
            && self.held == expected.held
    }
}

pub(crate) fn normalize_sort_name(stderr: &str) -> String {
    // Exactly the existing multiline prefix normalization, with no lossy UTF-8
    // conversion or replacement inside a diagnostic's body.
    let Ok(bytes) = decode(stderr) else {
        return stderr.to_owned();
    };
    let mut result = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        if let Some(rest) = line.strip_prefix(b"/usr/bin/sort: ") {
            result.extend_from_slice(b"sort: ");
            result.extend_from_slice(rest);
        } else {
            result.extend_from_slice(line);
        }
    }
    encode(&result)
}
