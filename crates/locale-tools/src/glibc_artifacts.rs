//! Checked aggregation and deterministic emission of native glibc observations.

use crate::Result;
use crate::collation::Row;
use crate::glibc_generation::{Common, Observation, UNICODE_VERSION, decomposition_ranges};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub fn ranges(points: impl IntoIterator<Item = u32>) -> Vec<(u32, u32)> {
    let points: BTreeSet<_> = points.into_iter().collect();
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for point in points {
        match ranges.last_mut() {
            Some((_, end)) if end.checked_add(1) == Some(point) => *end = point,
            _ => ranges.push((point, point)),
        }
    }
    ranges
}

#[derive(Serialize)]
struct Locale {
    weighed: Vec<u32>,
    other_weighed: Vec<u32>,
    no_fourth: Vec<(u32, u32)>,
}

// Preserve scalar sequence order, including supplementary scalars whose hex
// spelling would otherwise sort ahead of a smaller four-digit scalar.
struct Contractions(Vec<(String, String)>);
impl Serialize for Contractions {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (parts, letter) in &self.0 {
            map.serialize_entry(parts, letter)?;
        }
        map.end()
    }
}

#[derive(Serialize)]
struct Data {
    source: String,
    source_sha256: String,
    unicode_decompositions: &'static str,
    ignorable: usize,
    contractions: Contractions,
    no_fourth_everywhere: Vec<(u32, u32)>,
    reordered_everywhere: Vec<String>,
    locales: BTreeMap<String, Locale>,
}

pub struct Artifacts {
    pub json: Vec<u8>,
    pub rust: String,
    pub summary: String,
}

/// Check every cross-locale invariant before constructing either artifact.
/// These are generated candidates. The caller owns review and table updates.
pub fn emit(
    source_name: &str,
    source: &[u8],
    common: &Common,
    decomposed: &BTreeMap<u32, String>,
    rows: &[Row],
    observations: &BTreeMap<String, Observation>,
) -> Result<Artifacts> {
    if rows.is_empty() || rows.len() != observations.len() {
        return Err("sorting locales and observations must be nonempty and complete".into());
    }
    let mut names = BTreeSet::new();
    let mut tags: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for row in rows {
        if !names.insert(&row.name) || !observations.contains_key(&row.name) {
            return Err("duplicate or missing sorting locale".into());
        }
        tags.entry(&row.tag).or_default().push(&row.name);
    }
    let first = observations.values().next().unwrap();
    let mut no_fourth: BTreeSet<_> = first.no_fourth.iter().copied().collect();
    for observation in observations.values() {
        if observation.reordered != first.reordered {
            return Err(
                "the letters placed after their decomposition differ between locales".into(),
            );
        }
        let current: BTreeSet<_> = observation.no_fourth.iter().copied().collect();
        no_fourth.retain(|point| current.contains(point));
    }
    let rank: BTreeMap<_, _> = common
        .ignorable
        .iter()
        .enumerate()
        .map(|(rank, &point)| (point, rank))
        .collect();
    if rank.len() != common.ignorable.len() {
        return Err("duplicate ignorable character".into());
    }
    let mut ignored: Vec<(u32, u32, u16)> = Vec::new();
    for (&point, &rank) in &rank {
        let rank = u16::try_from(rank)
            .map_err(|_| "ignorable rank exceeds the table's u16 representation")?;
        match ignored.last_mut() {
            Some((start, end, initial))
                if end.checked_add(1) == Some(point)
                    && u32::from(*initial) + (*end - *start) + 1 == u32::from(rank) =>
            {
                *end = point
            }
            _ => ignored.push((point, point, rank)),
        }
    }
    let decomposition_ranges = decomposition_ranges(decomposed);
    for &(_, _, length) in &decomposition_ranges {
        u8::try_from(length)
            .map_err(|_| "decomposition length exceeds the table's u8 representation")?;
    }
    let locales: BTreeMap<_, _> = observations
        .iter()
        .map(|(name, observation)| {
            (
                name.clone(),
                Locale {
                    weighed: observation.weighed.clone(),
                    other_weighed: observation.other_weighed.clone(),
                    no_fourth: ranges(
                        observation
                            .no_fourth
                            .iter()
                            .copied()
                            .filter(|p| !no_fourth.contains(p)),
                    ),
                },
            )
        })
        .collect();
    let mut locale_rows = Vec::new();
    for (tag, names) in tags {
        let shape = &locales[names[0]];
        if names.iter().any(|name| {
            locales[*name].weighed != shape.weighed || locales[*name].no_fourth != shape.no_fourth
        }) {
            return Err(format!(
                "the locales sorting with {tag} ({}) differ",
                names.join(", ")
            )
            .into());
        }
        if !shape.weighed.is_empty() || !shape.no_fourth.is_empty() {
            let points = shape
                .weighed
                .iter()
                .map(|p| format!("0x{p:04X}"))
                .collect::<Vec<_>>()
                .join(", ");
            let spans = shape
                .no_fourth
                .iter()
                .map(|(a, b)| format!("(0x{a:04X}, 0x{b:04X})"))
                .collect::<Vec<_>>()
                .join(", ");
            locale_rows.push(format!("    ({tag:?}, &[{points}], &[{spans}]),"));
        }
    }
    let common_ranges = ranges(no_fourth);
    let data = Data {
        source: source_name.into(),
        source_sha256: Sha256::digest(source)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect(),
        unicode_decompositions: UNICODE_VERSION,
        ignorable: common.ignorable.len(),
        contractions: Contractions(
            common
                .contractions
                .iter()
                .map(|(parts, letter)| {
                    (
                        parts
                            .chars()
                            .map(|ch| format!("{:04X}", ch as u32))
                            .collect::<Vec<_>>()
                            .join(" "),
                        format!("{:04X}", *letter as u32),
                    )
                })
                .collect(),
        ),
        no_fourth_everywhere: common_ranges,
        reordered_everywhere: first.reordered.keys().map(|p| format!("{p:04X}")).collect(),
        locales,
    };
    let mut json = Vec::new();
    data.serialize(&mut serde_json::Serializer::with_formatter(
        &mut json,
        serde_json::ser::PrettyFormatter::with_indent(b" "),
    ))?;
    json.push(b'\n');
    let mut rust = String::new();
    rust.push_str("//! Generated by fastmash-generate-glibc-collation from glibc's\n");
    rust.push_str("//! iso14651_t1_common and its compiled sorting locales; do not edit. How\n");
    rust.push_str("//! glibc collates the characters apart from letters, which the Unicode\n");
    rust.push_str("//! collator orders differently.\n");
    rust.push('\n');
    rust.push_str(
        "/// Characters glibc ignores at the first three levels, by code point: ranges\n",
    );
    rust.push_str("/// `(first, last, rank)` of consecutive characters whose fourth-level ranks\n");
    rust.push_str("/// (their order in iso14651_t1_common) are consecutive from `rank`.\n");
    rust.push_str("#[rustfmt::skip]\n");
    rust.push_str("pub(super) static IGNORED: &[(u32, u32, u16)] = &[\n");
    for (a, b, rank) in &ignored {
        writeln!(rust, "    (0x{a:04X}, 0x{b:04X}, {rank}),")?;
    }
    rust.push_str("];\n");
    rust.push('\n');
    rust.push_str(
        "/// Letters and digits without a fourth-level weight in every sorting locale.\n",
    );
    rust.push_str("#[rustfmt::skip]\n");
    rust.push_str("pub(super) static NO_FOURTH: &[(u32, u32)] = &[\n");
    for (a, b) in &data.no_fourth_everywhere {
        writeln!(rust, "    (0x{a:04X}, 0x{b:04X}),")?;
    }
    rust.push_str("];\n");
    rust.push('\n');
    rust.push_str("/// Characters whose canonical decomposition (Unicode 16.0.0) is more\n");
    rust.push_str("/// than one character: ranges `(first, last, length)`. The Unicode collator\n");
    rust.push_str("/// reads them decomposed, so the fourth level counts them so too.\n");
    rust.push_str("#[rustfmt::skip]\n");
    rust.push_str("pub(super) static DECOMPOSED: &[(u32, u32, u8)] = &[\n");
    for (a, b, length) in &decomposition_ranges {
        writeln!(rust, "    (0x{a:04X}, 0x{b:04X}, {length}),")?;
    }
    rust.push_str("];\n");
    rust.push('\n');
    rust.push_str("/// Sequences glibc collates as the letter they spell in parts, with that\n");
    rust.push_str("/// letter, sorted by sequence: iso14651_t1_common's \"decomposition of\"\n");
    rust.push_str("/// collating elements, which tie with the letter at every level in every\n");
    rust.push_str("/// sorting locale.\n");
    rust.push_str("#[rustfmt::skip]\n");
    rust.push_str("pub(super) static CONTRACTIONS: &[(&str, char)] = &[\n");
    for (parts, letter) in &common.contractions {
        let parts = parts
            .chars()
            .map(|ch| format!("\\u{{{:04X}}}", ch as u32))
            .collect::<String>();
        writeln!(rust, "    (\"{parts}\", '\\u{{{:04X}}}'),", *letter as u32)?;
    }
    rust.push_str("];\n");
    rust.push('\n');
    rust.push_str("/// Letters coded below the first character of their canonical decomposition\n");
    rust.push_str("/// that glibc places after it, where the collator ties the two, the same\n");
    rust.push_str("/// in every sorting locale: `(letter, base)`, by letter, where `base` is\n");
    rust.push_str("/// that first character.\n");
    rust.push_str("#[rustfmt::skip]\n");
    rust.push_str("pub(super) static REORDERED: &[(u32, u32)] = &[\n");
    for (letter, base) in &first.reordered {
        writeln!(rust, "    (0x{letter:04X}, 0x{base:04X}),")?;
    }
    rust.push_str("];\n");
    rust.push('\n');
    rust.push_str("/// A sorting locale's Unicode collation tag, the ignorable characters it\n");
    rust.push_str("/// weighs first at the first level, before every letter and digit (in their\n");
    rust.push_str("/// fourth-level order, which is below every other ignorable), and its\n");
    rust.push_str("/// further letters and digits without a fourth-level weight.\n");
    rust.push_str(
        "pub(super) type Locale = (&'static str, &'static [u32], &'static [(u32, u32)]);\n",
    );
    rust.push('\n');
    rust.push_str("/// Per sorting locale, sorted by the Unicode collation tag it sorts with\n");
    rust.push_str("/// (the locales that share a tag behave alike), where it has any.\n");
    rust.push_str("#[rustfmt::skip]\n");
    rust.push_str("pub(super) static LOCALES: &[Locale] = &[\n");
    for row in locale_rows {
        writeln!(rust, "{row}")?;
    }
    rust.push_str("];\n");
    let summary = format!(
        "{} ignorable characters in {} ranges; {} ranges without a fourth level everywhere; {} locales weigh an ignorable lowest, {} otherwise",
        common.ignorable.len(),
        ignored.len(),
        data.no_fourth_everywhere.len(),
        observations
            .values()
            .filter(|o| !o.weighed.is_empty())
            .count(),
        observations
            .values()
            .filter(|o| !o.other_weighed.is_empty())
            .count()
    );
    Ok(Artifacts {
        json,
        rust,
        summary,
    })
}
