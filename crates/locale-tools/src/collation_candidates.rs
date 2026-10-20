//! Inputs and deterministic projection for the collation candidate workflow.

use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const ICU_MAJOR: &str = "78";

#[derive(Debug, Deserialize)]
pub struct NumericLocales {
    pub glibc: String,
    pub locales: BTreeMap<String, serde_json::Value>,
}

impl NumericLocales {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        let source: Self = serde_json::from_slice(raw)?;
        if source.glibc.is_empty() || source.locales.is_empty() {
            return Err("numeric reference must identify glibc and at least one locale".into());
        }
        for name in source.locales.keys() {
            if name.is_empty() || name.contains(['\0', '\n', '\r']) {
                return Err(
                    "locale names must be nonempty and contain no NUL or line breaks".into(),
                );
            }
        }
        Ok(source)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Alphabet {
    #[serde(deserialize_with = "required_nullable")]
    pub alphabet: Option<Vec<String>>,
    #[serde(deserialize_with = "required_nullable")]
    pub auxiliary: Option<Vec<String>>,
}

fn required_nullable<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error> {
    Option::deserialize(deserializer)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exemplars {
    pub icu: String,
    pub locales: BTreeMap<String, Alphabet>,
}

#[derive(Debug, Serialize)]
pub struct Candidates {
    pub icu: String,
    pub glibc: String,
    pub locales: BTreeMap<String, Candidate>,
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub tags: [String; 2],
    #[serde(flatten)]
    pub letters: Alphabet,
}

impl Exemplars {
    pub fn parse(raw: &[u8], numeric: &NumericLocales) -> Result<Self> {
        let source: Self = serde_json::from_slice(raw)?;
        source.validate(numeric)?;
        Ok(source)
    }

    fn validate(&self, numeric: &NumericLocales) -> Result<()> {
        if self.icu != ICU_MAJOR {
            return Err(format!("expected ICU {ICU_MAJOR} exemplar data").into());
        }
        if !self.locales.keys().eq(numeric.locales.keys()) {
            return Err("exemplar locales do not match the numeric reference".into());
        }
        Ok(())
    }

    pub fn project(
        self,
        numeric: NumericLocales,
        mut uppercase_first: impl FnMut(&str) -> Result<bool>,
    ) -> Result<Candidates> {
        self.validate(&numeric)?;
        let mut locales = BTreeMap::new();
        for (name, letters) in self.locales {
            let suffix = if uppercase_first(&name)? {
                "-u-kf-upper"
            } else {
                ""
            };
            let tag = name.replace('_', "-");
            locales.insert(
                name,
                Candidate {
                    tags: [format!("{tag}{suffix}"), format!("und{suffix}")],
                    letters,
                },
            );
        }
        Ok(Candidates {
            icu: self.icu,
            glibc: numeric.glibc,
            locales,
        })
    }
}
