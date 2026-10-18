//! Unicode 15.0.0 alphabet and fixed-block collation word lists.
//!
//! Strings retain their scalar sequences without normalization. Case mappings,
//! categories and contextual properties come from pinned data, independently
//! of the compiler's Unicode tables. Native execution remains separate.

use crate::Result;
use icu_properties_15::{GeneralCategory, maps, sets};
use std::collections::BTreeSet;

const ASCII: &str = "abcdefghijklmnopqrstuvwxyz";

/// One fixed workload's inclusive ranges, in their original traversal order.
/// These are selected ranges rather than complete Unicode blocks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Block {
    pub name: &'static str,
    pub ranges: &'static [(char, char)],
}

/// All 29 fixed workloads, in their retained report order.
pub const BLOCKS: &[Block] = &[
    Block {
        name: "latin",
        ranges: &[
            ('\u{41}', '\u{5a}'),
            ('\u{61}', '\u{7a}'),
            ('\u{c0}', '\u{17f}'),
            ('\u{218}', '\u{21b}'),
            ('\u{18f}', '\u{18f}'),
            ('\u{259}', '\u{259}'),
        ],
    },
    Block {
        name: "vietnamese",
        ranges: &[
            ('\u{1a0}', '\u{1a1}'),
            ('\u{1af}', '\u{1b0}'),
            ('\u{1ea0}', '\u{1ef9}'),
        ],
    },
    Block {
        name: "greek",
        ranges: &[('\u{386}', '\u{3ce}')],
    },
    Block {
        name: "cyrillic",
        ranges: &[('\u{400}', '\u{4ff}')],
    },
    Block {
        name: "armenian",
        ranges: &[('\u{531}', '\u{586}')],
    },
    Block {
        name: "hebrew",
        ranges: &[('\u{5d0}', '\u{5ea}')],
    },
    Block {
        name: "arabic",
        ranges: &[('\u{620}', '\u{64a}'), ('\u{671}', '\u{6d3}')],
    },
    Block {
        name: "devanagari",
        ranges: &[('\u{904}', '\u{939}'), ('\u{958}', '\u{961}')],
    },
    Block {
        name: "bengali",
        ranges: &[('\u{985}', '\u{9b9}')],
    },
    Block {
        name: "gurmukhi",
        ranges: &[('\u{a05}', '\u{a39}')],
    },
    Block {
        name: "gujarati",
        ranges: &[('\u{a85}', '\u{ab9}')],
    },
    Block {
        name: "oriya",
        ranges: &[('\u{b05}', '\u{b39}')],
    },
    Block {
        name: "tamil",
        ranges: &[('\u{b85}', '\u{bb9}')],
    },
    Block {
        name: "telugu",
        ranges: &[('\u{c05}', '\u{c39}')],
    },
    Block {
        name: "kannada",
        ranges: &[('\u{c85}', '\u{cb9}')],
    },
    Block {
        name: "malayalam",
        ranges: &[('\u{d05}', '\u{d3a}')],
    },
    Block {
        name: "sinhala",
        ranges: &[('\u{d85}', '\u{dc6}')],
    },
    Block {
        name: "thai",
        ranges: &[('\u{e01}', '\u{e2e}')],
    },
    Block {
        name: "lao",
        ranges: &[('\u{e81}', '\u{eae}')],
    },
    Block {
        name: "tibetan",
        ranges: &[('\u{f40}', '\u{f6c}')],
    },
    Block {
        name: "myanmar",
        ranges: &[('\u{1000}', '\u{102a}')],
    },
    Block {
        name: "georgian",
        ranges: &[('\u{10d0}', '\u{10fa}')],
    },
    Block {
        name: "ethiopic",
        ranges: &[('\u{1200}', '\u{1248}')],
    },
    Block {
        name: "khmer",
        ranges: &[('\u{1780}', '\u{17b3}')],
    },
    Block {
        name: "thaana",
        ranges: &[('\u{780}', '\u{7a5}')],
    },
    Block {
        name: "hiragana",
        ranges: &[('\u{3041}', '\u{3096}')],
    },
    Block {
        name: "katakana",
        ranges: &[('\u{30a1}', '\u{30fa}')],
    },
    Block {
        name: "hangul",
        ranges: &[('\u{ac00}', '\u{b530}')],
    },
    Block {
        name: "han",
        ranges: &[('\u{4e00}', '\u{5058}')],
    },
];

/// Plan words from optional standard and auxiliary exemplar strings.
///
/// Missing and empty slices both contribute no exemplars. Empty strings and
/// strings containing several scalars remain literal entries. The pair branch
/// uses the deduplicated standard-plus-ASCII count before auxiliary or Han
/// additions and is deliberately unfiltered when that count is at most 120.
/// Language matching is exact and only selects the four explicit Han cases.
pub fn alphabet_words(
    alphabet: Option<&[String]>,
    auxiliary: Option<&[String]>,
    language: &str,
) -> Vec<String> {
    let mut letters: BTreeSet<String> = alphabet.unwrap_or_default().iter().cloned().collect();
    letters.extend(ASCII.chars().map(|ch| ch.to_string()));
    let mut extra: BTreeSet<String> = auxiliary.unwrap_or_default().iter().cloned().collect();
    if matches!(language, "cmn" | "hak" | "lzh" | "nan")
        || letters.iter().any(|letter| cjk_unified_name(letter))
    {
        for (low, high) in [
            ('\u{3400}', '\u{3464}'),
            ('\u{20000}', '\u{20064}'),
            ('\u{f900}', '\u{f964}'),
        ] {
            extra.extend(
                (low..=high)
                    .filter(|&ch| word_char(ch))
                    .map(|ch| ch.to_string()),
            );
        }
    }
    extra.extend(letters.iter().cloned());
    let mut words = BTreeSet::new();
    for letter in &extra {
        add_with_filler(&mut words, letter, "a");
    }
    if letters.len() <= 120 {
        for left in &letters {
            for right in &letters {
                let pair = format!("{left}{right}");
                words.insert(capitalize(&pair));
                words.insert(pair);
            }
        }
    }
    words.into_iter().collect()
}

/// Plan a named fixed workload, using its first retained letter as filler.
/// Unknown names are errors. Names are neither normalized nor corrected.
pub fn block_words(name: &str) -> Result<Vec<String>> {
    let block = BLOCKS
        .iter()
        .find(|block| block.name == name)
        .ok_or_else(|| format!("unknown collation block {name:?}"))?;
    let letters: Vec<char> = block
        .ranges
        .iter()
        .flat_map(|&(low, high)| low..=high)
        .filter(|&ch| word_char(ch))
        .collect();
    // Every fixed workload has a retained letter. Choose the filler from the
    // original ranges before forming case variants, including Latin's A.
    let filler = letters[0].to_string();
    let mut words = BTreeSet::new();
    for ch in letters {
        add_with_filler(&mut words, &ch.to_string(), &filler);
    }
    if name == "latin" {
        for left in ASCII.chars() {
            for right in ASCII.chars() {
                words.insert(format!("{left}{right}"));
            }
        }
    }
    Ok(words.into_iter().collect())
}

fn word_char(ch: char) -> bool {
    matches!(
        maps::general_category().get(ch),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
            | GeneralCategory::DecimalNumber
            | GeneralCategory::LetterNumber
            | GeneralCategory::OtherNumber
    )
}

fn add_with_filler(words: &mut BTreeSet<String>, letter: &str, filler: &str) {
    for variant in [
        letter.to_owned(),
        lowercase(letter),
        uppercase(letter),
        capitalize(letter),
    ] {
        if variant.chars().all(word_char) {
            words.insert(format!("{variant}{filler}"));
            words.insert(format!("{filler}{variant}"));
            words.insert(variant);
        }
    }
}

// Assigned name-pattern endpoints from Unicode 15.0.0 DerivedName.txt and
// CPython 3.12.14 Modules/unicodedata.c. This is narrower than Unified_Ideograph.
// https://www.unicode.org/Public/15.0.0/ucd/extracted/DerivedName.txt
fn cjk_unified_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    chars.next().is_none()
        && matches!(ch,
            '\u{3400}'..='\u{4dbf}'
                | '\u{4e00}'..='\u{9fff}'
                | '\u{20000}'..='\u{2a6df}'
                | '\u{2a700}'..='\u{2b739}'
                | '\u{2b740}'..='\u{2b81d}'
                | '\u{2b820}'..='\u{2cea1}'
                | '\u{2ceb0}'..='\u{2ebe0}'
                | '\u{30000}'..='\u{3134a}'
                | '\u{31350}'..='\u{323af}'
        )
}

fn lowercase(original: &str) -> String {
    let mut output = String::new();
    for (offset, ch) in original.char_indices() {
        append_mapping(&mut output, ch, lower_mapping(original, offset, ch));
    }
    output
}

fn uppercase(original: &str) -> String {
    let mut output = String::new();
    for ch in original.chars() {
        append_mapping(&mut output, ch, unicode_case_mapping::to_uppercase(ch));
    }
    output
}

fn capitalize(original: &str) -> String {
    let mut output = String::new();
    for (offset, ch) in original.char_indices() {
        if offset == 0 {
            append_mapping(&mut output, ch, unicode_case_mapping::to_titlecase(ch));
        } else {
            append_mapping(&mut output, ch, lower_mapping(original, offset, ch));
        }
    }
    output
}

fn lower_mapping(original: &str, offset: usize, ch: char) -> [u32; 2] {
    if ch == '\u{3a3}' && final_sigma(original, offset) {
        ['\u{3c2}' as u32, 0]
    } else {
        unicode_case_mapping::to_lowercase(ch)
    }
}

// Default Final_Sigma uses the ORIGINAL whole string, including capitalize's
// original head. Skip Case_Ignorable before testing Cased, even for overlap.
// https://www.unicode.org/Public/15.0.0/ucd/SpecialCasing.txt
fn final_sigma(original: &str, offset: usize) -> bool {
    let cased = sets::cased();
    let ignorable = sets::case_ignorable();
    let before = original[..offset]
        .chars()
        .rev()
        .find(|&ch| !ignorable.contains(ch));
    let after = original[offset + '\u{3a3}'.len_utf8()..]
        .chars()
        .find(|&ch| !ignorable.contains(ch));
    before.is_some_and(|ch| cased.contains(ch)) && !after.is_some_and(|ch| cased.contains(ch))
}

fn append_mapping<const N: usize>(output: &mut String, original: char, mapping: [u32; N]) {
    if mapping.iter().all(|&value| value == 0) {
        output.push(original);
    } else {
        for value in mapping.into_iter().filter(|&value| value != 0) {
            // Generated case mappings contain only Unicode scalar values.
            output.push(char::from_u32(value).expect("Unicode case mapping scalar"));
        }
    }
}
