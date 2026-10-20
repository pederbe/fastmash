//! Unicode 16 inputs and parsing for the retained glibc collation generator.

use crate::Result;
use crate::glibc_collation::Pair;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use unicode_general_category::{GeneralCategory, get_general_category};
use unicode_normalization::UnicodeNormalization;

pub const UNICODE_VERSION: &str = "16.0.0";

#[derive(Debug, Eq, PartialEq)]
pub struct Common {
    pub ignorable: Vec<u32>,
    pub contractions: Vec<(String, char)>,
}

fn codepoint(raw: &str) -> Result<char> {
    if raw.is_empty()
        || !raw
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c))
    {
        return Err("invalid Unicode scalar spelling".into());
    }
    char::from_u32(u32::from_str_radix(raw, 16)?).ok_or_else(|| "invalid Unicode scalar".into())
}

impl Common {
    pub fn parse(raw: &str) -> Result<Self> {
        let mut ignorable = Vec::new();
        let mut contractions = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("<U")
                && let Some((hex, suffix)) = rest.split_once('>')
                && (4..=8).contains(&hex.len())
                && suffix.starts_with(&format!(" IGNORE;IGNORE;IGNORE;<U{hex}>"))
            {
                let point = codepoint(hex)? as u32;
                if !seen.insert(point) {
                    return Err("an ignorable character is listed twice".into());
                }
                ignorable.push(point);
            }
            let Some(rest) = line.strip_prefix("collating-element <") else {
                continue;
            };
            let Some((_, rest)) = rest.split_once("> from \"") else {
                continue;
            };
            let Some((parts, name)) = rest.split_once("\" % decomposition of ") else {
                continue;
            };
            let mut remaining = parts;
            let mut sequence = String::new();
            while !remaining.is_empty() {
                let rest = remaining
                    .strip_prefix("<U")
                    .ok_or("invalid collating-element sequence")?;
                let (hex, rest) = rest
                    .split_once('>')
                    .ok_or("unterminated collating-element scalar")?;
                sequence.push(codepoint(hex)?);
                remaining = rest;
            }
            if sequence.is_empty() {
                return Err("empty collating-element sequence".into());
            }
            let letter = unicode_names2::character(name.trim())
                .ok_or_else(|| format!("unknown Unicode 16 name: {name}"))?;
            if contractions.insert(sequence, letter).is_some() {
                return Err("a contraction is listed twice".into());
            }
        }
        if contractions.is_empty() {
            return Err("no contractions".into());
        }
        Ok(Self {
            ignorable,
            contractions: contractions.into_iter().collect(),
        })
    }

    /// Replace the longest contraction at each scalar boundary.
    pub fn compose(&self, text: &str) -> String {
        let mut output = String::new();
        let mut rest = text;
        while !rest.is_empty() {
            if let Some((sequence, letter)) = self
                .contractions
                .iter()
                .filter(|(sequence, _)| rest.starts_with(sequence))
                .max_by_key(|(sequence, _)| sequence.chars().count())
            {
                output.push(*letter);
                rest = &rest[sequence.len()..];
            } else {
                let ch = rest.chars().next().unwrap();
                output.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
        output
    }
}

pub fn letters() -> Vec<u32> {
    (0x80..0x10000)
        .filter_map(char::from_u32)
        .filter(|ch| {
            matches!(
                get_general_category(*ch),
                GeneralCategory::UppercaseLetter
                    | GeneralCategory::LowercaseLetter
                    | GeneralCategory::TitlecaseLetter
                    | GeneralCategory::ModifierLetter
                    | GeneralCategory::OtherLetter
                    | GeneralCategory::DecimalNumber
                    | GeneralCategory::LetterNumber
                    | GeneralCategory::OtherNumber
            )
        })
        .map(u32::from)
        .collect()
}

pub fn decompositions() -> BTreeMap<u32, String> {
    (0x80..0x110000)
        .filter(|point| !(0xAC00..=0xD7A3).contains(point))
        .filter_map(char::from_u32)
        .filter_map(|ch| {
            let parts: String = ch.to_string().nfd().collect();
            (parts.chars().count() > 1).then_some((ch as u32, parts))
        })
        .collect()
}

pub fn decomposition_ranges(decompositions: &BTreeMap<u32, String>) -> Vec<(u32, u32, usize)> {
    let mut ranges: Vec<(u32, u32, usize)> = Vec::new();
    for (&point, parts) in decompositions {
        let length = parts.chars().count();
        match ranges.last_mut() {
            Some((_, end, prior)) if *end + 1 == point && *prior == length => *end = point,
            _ => ranges.push((point, point, length)),
        }
    }
    ranges
}

#[derive(Debug, Eq, PartialEq, serde::Serialize)]
pub struct Observation {
    pub weighed: Vec<u32>,
    pub other_weighed: Vec<u32>,
    pub no_fourth: Vec<u32>,
    pub reordered: BTreeMap<u32, u32>,
}

const CONTEXTS: [(&str, &str); 10] = [
    ("", ""),
    ("a", "b"),
    ("", "b"),
    ("", " b"),
    ("", "-b"),
    ("", ".b"),
    ("", "1"),
    ("", ","),
    ("", "  b"),
    ("a", ""),
];

fn scalar(point: u32) -> Result<char> {
    char::from_u32(point).ok_or_else(|| "invalid probe scalar".into())
}

/// Apply the retained probe predicates. Native comparison and admission belong
/// to the supplied reference, and every returned batch must be complete.
pub fn probe(
    common: &Common,
    candidates: &[u32],
    decomposed: &BTreeMap<u32, String>,
    mut compare: impl FnMut(&str, &[Pair]) -> Result<Vec<Ordering>>,
) -> Result<Observation> {
    let mut ask = |label: &str, pairs: &[Pair]| -> Result<Vec<Ordering>> {
        let orders = compare(label, pairs)?;
        if orders.len() != pairs.len() {
            return Err(format!("{label}: incomplete comparison batch").into());
        }
        Ok(orders)
    };
    let mut pairs = Vec::new();
    for (parts, letter) in &common.contractions {
        pairs.push(Pair {
            left: parts.clone(),
            right: letter.to_string(),
        });
        pairs.push(Pair {
            left: format!("a{parts}b"),
            right: format!("a{letter}b"),
        });
    }
    let orders = ask("contractions", &pairs)?;
    if let Some(index) = orders.iter().position(|order| *order != Ordering::Equal) {
        return Err(format!(
            "contraction {:?} does not tie with {:?}",
            pairs[index].left, pairs[index].right
        )
        .into());
    }

    let points: Vec<_> = decomposed.keys().copied().collect();
    let mut pairs = Vec::new();
    for &point in &points {
        let ch = scalar(point)?;
        pairs.push(Pair {
            left: format!("a{ch}b"),
            right: format!("a{}b", decomposed[&point]),
        });
    }
    let orders = ask("decomposition-ties", &pairs)?;
    let mut context_points = Vec::new();
    let mut pairs = Vec::new();
    for (&point, order) in points.iter().zip(orders) {
        let ch = scalar(point)?;
        let parts = &decomposed[&point];
        if order == Ordering::Equal {
            if common.compose(parts) != ch.to_string() {
                return Err(format!("U+{point:04X} ties with its decomposition").into());
            }
            continue;
        }
        let base = parts.chars().next().ok_or("empty decomposition")? as u32;
        if point > base {
            continue;
        }
        context_points.push((point, base));
        for (before, after) in CONTEXTS {
            pairs.push(Pair {
                left: format!("{before}{ch}{after}"),
                right: format!("{before}{parts}{after}"),
            });
        }
    }
    let orders = ask("decomposition-contexts", &pairs)?;
    let mut reordered = BTreeMap::new();
    for ((point, base), orders) in context_points
        .into_iter()
        .zip(orders.as_chunks::<{ CONTEXTS.len() }>().0)
    {
        if orders.iter().all(|order| *order == Ordering::Greater) {
            reordered.insert(point, base);
        }
    }

    let points: Vec<_> = common
        .ignorable
        .iter()
        .copied()
        .filter(|point| *point != 0)
        .collect();
    let mut pairs = Vec::new();
    for &point in &points {
        let ch = scalar(point)?;
        for right in ["aa", "ac", "a0"] {
            pairs.push(Pair {
                left: format!("a{ch}b"),
                right: right.into(),
            });
        }
    }
    let orders = ask("ignorable-levels", &pairs)?;
    let mut weighed = Vec::new();
    let mut other_weighed = Vec::new();
    for (point, orders) in points.iter().copied().zip(orders.as_chunks::<3>().0) {
        if orders[0] == Ordering::Greater && orders[1] == Ordering::Less {
            continue;
        }
        if orders[2] == Ordering::Less {
            weighed.push(point);
        } else {
            other_weighed.push(point);
        }
    }

    let rest: Vec<_> = points
        .into_iter()
        .filter(|point| !weighed.contains(point))
        .collect();
    let mut pairs = Vec::new();
    for &first in &weighed {
        for &ignored in &rest {
            let first = scalar(first)?;
            let ignored = scalar(ignored)?;
            pairs.push(Pair {
                left: format!("a{first}{ignored}b"),
                right: format!("a{ignored}{first}b"),
            });
        }
    }
    let orders = ask("weighed-below-ignorables", &pairs)?;
    if orders.iter().any(|order| *order != Ordering::Less) {
        return Err("a weighed-first character does not rank below every ignorable".into());
    }
    let mut pairs = Vec::new();
    let mut indices = Vec::new();
    for (at, &first) in weighed.iter().enumerate() {
        for &second in &weighed[at + 1..] {
            indices.push((first, second));
            let first = scalar(first)?;
            let second = scalar(second)?;
            pairs.push(Pair {
                left: format!("a{first}{second}b"),
                right: format!("a{second}{first}b"),
            });
        }
    }
    let orders = ask("weighed-order", &pairs)?;
    let mut matrix = BTreeMap::new();
    for ((first, second), order) in indices.into_iter().zip(orders) {
        matrix.insert((first, second), order);
        matrix.insert((second, first), order.reverse());
    }
    weighed.sort_by(|a, b| {
        if a == b {
            Ordering::Equal
        } else {
            matrix[&(*a, *b)]
        }
    });
    for (at, &first) in weighed.iter().enumerate() {
        for &second in &weighed[at + 1..] {
            if matrix[&(first, second)] == Ordering::Greater {
                return Err("inconsistent weighed-character order".into());
            }
        }
    }

    let mut pairs = Vec::new();
    for &point in candidates {
        let letter = scalar(point)?;
        pairs.push(Pair {
            left: format!("a{letter}"),
            right: format!("a-{letter}"),
        });
    }
    let orders = ask("fourth-level-letters", &pairs)?;
    let no_fourth = candidates
        .iter()
        .copied()
        .zip(orders)
        .filter_map(|(point, order)| (order == Ordering::Less).then_some(point))
        .collect();
    Ok(Observation {
        weighed,
        other_weighed,
        no_fourth,
        reordered,
    })
}
