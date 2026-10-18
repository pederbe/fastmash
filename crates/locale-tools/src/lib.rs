#![forbid(unsafe_code)]

//! Deterministic locale table generation, separate from native queries.

#[cfg(unix)]
mod capture;
pub mod collation;
#[cfg(unix)]
mod collation_command;
pub mod collation_plan;
pub mod collation_punctuation;
pub mod collation_unicode;
pub mod collation_verification;
pub mod comparison;
#[cfg(unix)]
pub mod executor;
#[cfg(unix)]
pub mod native;
#[cfg(unix)]
pub mod punctuation_executor;
#[cfg(unix)]
pub mod verification_executor;

use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};
use std::error::Error;
use std::fmt::Write;

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Reference {
    pub glibc: String,
    pub machine: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Numeric {
    pub decimal_point: String,
    pub thousands_sep: String,
    pub grouping: Vec<i64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Locale {
    pub name: String,
    pub numeric: Numeric,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Snapshot {
    pub reference: Reference,
    pub bare_utf8: Vec<String>,
    /// First normalized-key position, with the last sorted source's value.
    pub locales: Vec<Locale>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct CompiledLocale {
    pub name: Vec<u8>,
    pub decimal: u8,
    pub thousands: Vec<u8>,
    pub grouping: Vec<u8>,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Tables {
    pub numeric: Vec<CompiledLocale>,
    pub bare_utf8: Vec<Vec<u8>>,
    pub refused: Vec<Vec<u8>>,
}

/// `directories` contains exact directory names, before normalization. The
/// query sees each retained source name in sorted order, including collisions.
pub fn collect(
    reference: Reference,
    mut directories: Vec<String>,
    mut query: impl FnMut(&str) -> Result<Numeric>,
) -> Result<Snapshot> {
    directories.sort();
    directories.retain(|name| !name.starts_with("C."));
    let bare_utf8 = directories
        .iter()
        .filter(|name| !name.contains('.'))
        .cloned()
        .collect();
    let mut locales: Vec<Locale> = Vec::new();
    for directory in directories {
        let name = directory.replace(".UTF-8", "");
        let numeric = query(&directory)?;
        if let Some(existing) = locales.iter_mut().find(|locale| locale.name == name) {
            existing.numeric = numeric;
        } else {
            locales.push(Locale { name, numeric });
        }
    }
    Ok(Snapshot {
        reference,
        bare_utf8,
        locales,
    })
}

/// Parse the three numeric values from a `locale -k LC_NUMERIC` reply.
pub fn parse_numeric(text: &str) -> Result<Numeric> {
    let mut decimal = None;
    let mut thousands = None;
    let mut grouping = None;
    for line in text.lines() {
        if let Some((key, value)) = line.split_once('=') {
            match key {
                "decimal_point" => decimal = Some(value),
                "thousands_sep" => thousands = Some(value),
                "grouping" => grouping = Some(value),
                _ => {}
            }
        }
    }
    let grouping = grouping.ok_or("locale reply has no grouping")?;
    Ok(Numeric {
        decimal_point: serde_json::from_str(decimal.ok_or("locale reply has no decimal_point")?)?,
        thousands_sep: serde_json::from_str(thousands.ok_or("locale reply has no thousands_sep")?)?,
        grouping: if grouping.is_empty() {
            Vec::new()
        } else {
            grouping
                .split(';')
                .map(|size| Ok(size.trim().parse()?))
                .collect::<Result<Vec<i64>>>()?
        },
    })
}

impl Snapshot {
    pub fn tables(&self) -> Result<Tables> {
        let mut ordered: Vec<_> = self.locales.iter().collect();
        ordered.sort_by(|left, right| left.name.cmp(&right.name));
        let mut numeric = Vec::new();
        let mut refused = Vec::new();
        for locale in ordered {
            let bytes = locale.numeric.decimal_point.as_bytes();
            if bytes.len() != 1 {
                refused.push(locale.name.as_bytes().to_vec());
                continue;
            }
            let grouping = locale
                .numeric
                .grouping
                .iter()
                .map(|&size| {
                    u8::try_from(if size == -1 { 127 } else { size })
                        .map_err(|_| "locale grouping is not one byte".into())
                })
                .collect::<Result<_>>()?;
            numeric.push(CompiledLocale {
                name: locale.name.as_bytes().to_vec(),
                decimal: bytes[0],
                thousands: locale.numeric.thousands_sep.as_bytes().to_vec(),
                grouping,
            });
        }
        let bare_utf8 = self
            .bare_utf8
            .iter()
            .filter(|name| numeric.iter().any(|locale| locale.name == name.as_bytes()))
            .map(|name| name.as_bytes().to_vec())
            .collect();
        Ok(Tables {
            numeric,
            bare_utf8,
            refused,
        })
    }

    /// Raw locale keys follow source-directory insertion order, not the sorted
    /// object-map order used by serde_json::Value's default map implementation.
    pub fn json(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut output, formatter);
        self.serialize(&mut serializer)?;
        output.push(b'\n');
        Ok(output)
    }

    pub fn rust_source(&self) -> Result<String> {
        if self.reference.glibc.contains('\r') || self.reference.glibc.contains('\n') {
            return Err("the glibc reference must be a single version line".into());
        }
        let tables = self.tables()?;
        let mut output = format!(
            "//! Generated by fastmash-generate-numeric-locales from {} compiled\n\
//! locales; do not edit. Number rules (decimal point, thousands separator and\n\
//! grouping) of every glibc locale, with or without a modifier (compiled in\n\
//! UTF-8), whose decimal point is one byte; the others are in `REFUSED`.\n\
use fastmash_conversion::profile::Profile;\n\n\
/// Sorted by name for binary search.\n\
pub(super) static NUMERIC: &[(&[u8], Profile)] = &[\n",
            self.reference.glibc,
        );
        for (index, locale) in tables.numeric.iter().enumerate() {
            if index != 0 {
                output.push('\n');
            }
            write!(
                output,
                "    ({}, Profile::new({}[0], {}, {})),",
                rust_bytes(&locale.name),
                rust_bytes(&[locale.decimal]),
                rust_bytes(&locale.thousands),
                rust_bytes(&locale.grouping)
            )?;
        }
        output.push_str(
            "\n];\n\n/// Names whose default character set is UTF-8, so glibc needs no\n\
/// codeset; sorted.\npub(super) static BARE_UTF8: &[&[u8]] = &[\n",
        );
        write_names(&mut output, &tables.bare_utf8)?;
        output.push_str(
            "\n];\n\n/// glibc locales whose decimal point is not one byte, whose numbers\n\
/// Fastmash refuses; sorted.\npub(super) static REFUSED: &[&[u8]] = &[\n",
        );
        write_names(&mut output, &tables.refused)?;
        output.push_str("\n];\n");
        Ok(output)
    }
}

fn rust_bytes(bytes: &[u8]) -> String {
    let mut output = String::from("b\"");
    for byte in bytes {
        write!(output, "\\x{byte:02x}").expect("writing to a String cannot fail");
    }
    output.push('"');
    output
}

fn write_names(output: &mut String, names: &[Vec<u8>]) -> std::fmt::Result {
    for (index, name) in names.iter().enumerate() {
        if index != 0 {
            output.push('\n');
        }
        write!(output, "    {},", rust_bytes(name))?;
    }
    Ok(())
}

impl Serialize for Snapshot {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        struct Ordered<'a>(&'a [Locale]);
        impl Serialize for Ordered<'_> {
            fn serialize<S: Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                let mut map = serializer.serialize_map(Some(self.0.len()))?;
                for locale in self.0 {
                    map.serialize_entry(&locale.name, &locale.numeric)?;
                }
                map.end()
            }
        }
        let mut object = serializer.serialize_struct("NumericLocales", 4)?;
        object.serialize_field("glibc", &self.reference.glibc)?;
        object.serialize_field("machine", &self.reference.machine)?;
        object.serialize_field("bare_utf8", &self.bare_utf8)?;
        object.serialize_field("locales", &Ordered(&self.locales))?;
        object.end()
    }
}
