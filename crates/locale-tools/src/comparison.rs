//! Historical numeric-locale comparison plans, without native execution.
//!
//! The original plan appends `.UTF-8` after any modifier. Retaining that
//! spelling does not establish that a live reference admits the locale.

use crate::{Numeric, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write;

const FORMATS: [&str; 16] = [
    "%'.2f",
    "%'14.2f",
    "%'014.2f",
    "%-'16.3f",
    "%+'.0f",
    "%' 025.3f",
    "%'Lf",
    "%'g",
    "%'.10g",
    "%'.20g",
    "%'e",
    "%'a",
    "%.3f",
    "%g",
    "%'20.4f",
    "%'.0f",
];
const VALUES: [&str; 21] = [
    "0",
    "1",
    "-1",
    "12",
    "123",
    "1234",
    "12345",
    "123456",
    "1234567",
    "12345678",
    "123456789",
    "1234567890",
    "98765432109876",
    "-4321987.5",
    "0.125",
    "1e20",
    "-2.5e-7",
    "999999.995",
    "1000000",
    "-0.5",
    "123456789012345678901234567890",
];

#[derive(Debug, Eq, PartialEq)]
pub struct Plan {
    pub locales: Vec<PlannedLocale>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct PlannedLocale {
    pub name: String,
    /// Historical spelling only. It does not establish live locale admission.
    pub locale: String,
    pub rules: Numeric,
    each: Vec<u8>,
    column: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Case<'a> {
    /// Arguments after argv[0]. No process or environment is selected here.
    pub arguments: Vec<String>,
    pub input: &'a [u8],
}

impl PlannedLocale {
    /// Iterate in the original case order. Cases borrow two shared payloads.
    pub fn cases(&self) -> impl ExactSizeIterator<Item = Case<'_>> {
        (0..21).map(|index| {
            let arguments: Vec<String> = match index {
                0 => ["-g", "2", "sum", "1"].map(str::to_owned).into(),
                1..=16 => ["--format", FORMATS[index - 1], "-g", "2", "sum", "1"]
                    .map(str::to_owned)
                    .into(),
                17 => ["sum", "1", "mean", "1"].map(str::to_owned).into(),
                18 => vec![
                    format!("trimmean:0{}2", self.rules.decimal_point),
                    "1".into(),
                ],
                19 => ["perc:90", "1"].map(str::to_owned).into(),
                20 => ["--format", "%'.3f", "sum", "1"].map(str::to_owned).into(),
                _ => unreachable!("the fixed comparison case range has 21 entries"),
            };
            Case {
                arguments,
                input: if index < 17 { &self.each } else { &self.column },
            }
        })
    }
}

impl Plan {
    /// Select the first sorted name for every exact raw numeric triple, or
    /// every admitted name when `every` is true. Multi-byte decimals are omitted.
    pub fn from_json(json: &[u8], every: bool) -> Result<Self> {
        #[derive(Deserialize)]
        struct Input {
            #[serde(default)]
            bare_utf8: Vec<String>,
            locales: BTreeMap<String, Numeric>,
        }
        let input: Input = serde_json::from_slice(json)?;
        let mut seen = Vec::new();
        let mut locales = Vec::new();
        for (name, rules) in input.locales {
            if rules.decimal_point.len() != 1 || (!every && seen.contains(&rules)) {
                continue;
            }
            if !every {
                seen.push(rules.clone());
            }
            let locale = if input.bare_utf8.contains(&name) {
                name.clone()
            } else {
                format!("{name}.UTF-8")
            };
            let (each, column) = payloads(&rules.decimal_point);
            locales.push(PlannedLocale {
                name,
                locale,
                rules,
                each,
                column,
            });
        }
        Ok(Self { locales })
    }

    pub fn comparisons(&self) -> usize {
        self.locales.len() * 21
    }
}

fn payloads(point: &str) -> (Vec<u8>, Vec<u8>) {
    let mut each = String::new();
    let mut column = String::new();
    for (index, value) in VALUES.into_iter().enumerate() {
        let value = value.replace('.', point);
        writeln!(each, "{value}\t{index}").expect("writing to a String cannot fail");
        if index < VALUES.len() - 1 {
            writeln!(column, "{value}").expect("writing to a String cannot fail");
        }
    }
    (each.into_bytes(), column.into_bytes())
}

/// Replace the two program names only at ASCII byte-word boundaries.
pub fn normalize_stderr(stderr: &[u8]) -> Vec<u8> {
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut output = Vec::with_capacity(stderr.len());
    let mut index = 0;
    while index < stderr.len() {
        let remaining = &stderr[index..];
        let token = remaining.starts_with(b"datamash") || remaining.starts_with(b"fastmash");
        if token
            && (index == 0 || !word(stderr[index - 1]))
            && remaining.get(8).is_none_or(|&byte| !word(byte))
        {
            output.extend_from_slice(b"PROG");
            index += 8;
        } else {
            output.push(stderr[index]);
            index += 1;
        }
    }
    output
}
