#![forbid(unsafe_code)]

use rust_icu_sys::ULocDataLocaleType::ULOC_VALID_LOCALE;
use rust_icu_ures::UResourceBundle;
use serde::Serialize;
use std::collections::BTreeMap;

pub const ICU_VERSION: &str = env!("FASTMASH_REFERENCE_ICU");
pub const ICU_MAJOR: &str = "78";
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Debug, Eq, PartialEq, Serialize)]
pub struct Alphabet {
    pub alphabet: Option<Vec<String>>,
    pub auxiliary: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
pub struct Exemplars {
    pub icu: String,
    pub locales: BTreeMap<String, Alphabet>,
}

#[derive(Debug)]
pub struct Refusal(pub String);

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Refusal {}

fn letters(pattern: &str) -> Result<Vec<String>> {
    // Check only unsupported syntax here. The maintained parser owns decoding,
    // set operations, escapes, ranges and multicharacter strings. Its property
    // tables must never substitute for the identified host ICU reference.
    let mut chars = pattern.chars().peekable();
    while let Some(ch) = chars.next() {
        let unsupported = match ch {
            '\\' => matches!(chars.next(), Some('p' | 'P')),
            '[' => chars.peek() == Some(&':'),
            '$' => true,
            _ => false,
        };
        if unsupported {
            return Err(
                Refusal("ICU exemplar properties and variables are unsupported".into()).into(),
            );
        }
    }
    let (set, consumed) = icu_unicodeset_parse::parse(pattern)
        .map_err(|error| Refusal(format!("unsupported ICU exemplar pattern: {error:?}")))?;
    if consumed != pattern.len() {
        return Err(Refusal("ICU exemplar pattern has unparsed trailing data".into()).into());
    }
    let mut letters: Vec<_> = set
        .code_points()
        .iter_chars()
        .map(|c| c.to_string())
        .collect();
    let mut strings: Vec<_> = set.strings().iter().map(str::to_owned).collect();
    // ICU USet enumerates strings by UTF-16 code units, after scalar ranges.
    strings.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    letters.extend(strings);
    Ok(letters)
}

fn alphabet(name: &str) -> Result<Alphabet> {
    let bundle = UResourceBundle::try_new(None, name)?;
    let actual = bundle.get_locale_by_type(ULOC_VALID_LOCALE)?;
    if actual.split('_').next() != name.split('_').next() {
        return Ok(Alphabet {
            alphabet: None,
            auxiliary: None,
        });
    }
    let query = |key| letters(&bundle.get_string_by_key(key)?);
    Ok(Alphabet {
        alphabet: Some(query("ExemplarCharacters")?),
        auxiliary: Some(query("AuxExemplarCharacters")?),
    })
}

pub fn supported() -> bool {
    ICU_VERSION.split('.').next() == Some(ICU_MAJOR)
}

pub fn collect(names: impl IntoIterator<Item = String>) -> Result<Exemplars> {
    if !supported() {
        return Err(Refusal(format!(
            "ICU {ICU_MAJOR} required, built with ICU {ICU_VERSION}"
        ))
        .into());
    }
    let mut locales = BTreeMap::new();
    for name in names {
        if name.is_empty() || name.contains(['\0', '\n', '\r']) {
            return Err("locale names must be nonempty and contain no NUL or line breaks".into());
        }
        locales.insert(
            name.clone(),
            alphabet(&name)
                .map_err(|error| Refusal(format!("ICU exemplar query for {name:?}: {error}")))?,
        );
    }
    if locales.is_empty() {
        return Err("at least one reference locale is required".into());
    }
    Ok(Exemplars {
        icu: ICU_MAJOR.into(),
        locales,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_patterns_preserve_ranges_escapes_and_string_order() {
        assert_eq!(
            letters(r"[c a-b \u00e9 {dz} {cs}]").unwrap(),
            ["a", "b", "c", "é", "cs", "dz"]
        );
        assert!(letters("[]").unwrap().is_empty());
        assert_eq!(
            letters("[{\u{e000}x}{\u{10000}x}]").unwrap(),
            ["\u{10000}x", "\u{e000}x"]
        );
    }

    #[test]
    fn malformed_partial_and_property_patterns_refuse() {
        for pattern in ["[a", "[a][b]", r"[\p{Alphabetic}]", "[[:Letter:]]"] {
            assert!(letters(pattern).is_err(), "{pattern:?}");
        }
    }

    #[test]
    fn installed_icu_queries_follow_parents_and_reject_foreign_language_fallback() {
        let english = alphabet("en").unwrap();
        assert_eq!(
            english.alphabet.unwrap(),
            ('a'..='z').map(|c| c.to_string()).collect::<Vec<_>>()
        );
        assert_eq!(alphabet("en_AA").unwrap(), alphabet("en").unwrap());
        assert_eq!(alphabet("zz_ZZ").unwrap().alphabet, None);
        assert!(
            alphabet("es_MX")
                .unwrap()
                .alphabet
                .unwrap()
                .contains(&"ñ".into())
        );
    }
}
