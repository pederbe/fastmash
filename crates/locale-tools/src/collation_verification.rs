//! Known-format verification bookkeeping from supplied observations.
//!
//! No locale selection, native execution or Unicode planning occurs here.

use crate::Result;
use crate::collation_plan::OrderDifference;
use serde::de::{Error as _, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;

pub type CaseOutcome = std::result::Result<OrderDifference, [String; 2]>;

#[derive(Debug)]
pub struct Observation {
    pub name: String,
    pub alphabet: CaseOutcome,
    pub digits: CaseOutcome,
    /// Complete script maps in producer order. None denotes a failed case.
    pub scripts: Vec<(String, Option<BTreeSet<String>>)>,
}

#[derive(Debug)]
pub struct Report {
    document: Document,
    verified: usize,
    observed: usize,
    variant: i64,
    overall: usize,
}

impl Report {
    /// Update the original producer's known-format document, preserving dynamic
    /// map insertion positions. The first supplied en_US provides the baseline.
    ///
    /// Prior variants must be first at top level. Existing optional metadata
    /// positions are retained, and missing fields append in producer order.
    /// Other records require their producer field order. Unknown prior fields,
    /// duplicate JSON keys and unsupported shapes/orders are refused before a
    /// report is returned. This is not arbitrary-prior-JSON compatibility.
    ///
    /// Comparison vectors retain their established sorted unique contract.
    /// Scripts are complete ordered maps. Native statuses/decoding and
    /// per-locale Python-style console messages remain the caller's concern.
    /// Equivalent aggregated duplicates are retained until bookkeeping, while
    /// conflicting aggregated duplicates return an error.
    pub fn update(
        prior: Option<&[u8]>,
        candidates: &[u8],
        variant: i64,
        observations: Vec<Observation>,
    ) -> Result<Self> {
        let mut document = match prior {
            Some(raw) => serde_json::from_slice::<Document>(raw)?,
            None => Document::empty(),
        };
        let candidates: Candidates = serde_json::from_slice(candidates)?;
        let baseline_observation = observations
            .iter()
            .find(|item| item.name == "en_US")
            .ok_or("verification observations have no en_US baseline")?;
        let mut baseline = Members::empty();
        for (block, words) in &baseline_observation.scripts {
            baseline.insert(
                block.clone(),
                words
                    .clone()
                    .ok_or_else(|| format!("en_US baseline block {block:?} failed"))?,
            );
        }

        let mut raw: Vec<_> = observations.into_iter().map(Raw::from).collect();
        raw.sort_by(|left, right| left.name.cmp(&right.name));
        for pair in raw.windows(2) {
            if pair[0].name == pair[1].name
                && (pair[0].failures != pair[1].failures || pair[0].scripts != pair[1].scripts)
            {
                return Err(
                    format!("conflicting aggregated observations for {:?}", pair[0].name).into(),
                );
            }
        }
        let mut run = Members::empty();
        for mut observation in raw {
            let candidate = candidates.locales.get(&observation.name).ok_or_else(|| {
                format!("observed locale {:?} is not a candidate", observation.name)
            })?;
            let mut extra = Members::empty();
            for (block, words) in &observation.scripts.0 {
                let reference = baseline
                    .get(block)
                    .ok_or_else(|| format!("missing en_US baseline block {block:?}"))?;
                let value = match words {
                    None => Some(Extra::Error(ScriptError::Error)),
                    Some(words) => {
                        let count = words.difference(reference).count();
                        (count != 0).then_some(Extra::Count(count))
                    }
                };
                if let Some(value) = value {
                    extra.insert(block.clone(), value);
                }
            }
            if !candidate.alphabet_present()? {
                for (block, value) in &extra.0 {
                    observation
                        .failures
                        .insert(format!("block:{block}"), Failure::Script(value.clone()));
                }
            }
            let verified = observation.failures.0.is_empty();
            let tag = candidate.tag(variant)?.clone();
            run.insert(
                observation.name,
                StoredLocale {
                    verified,
                    tag,
                    failures: observation.failures,
                    other_scripts: extra,
                },
            );
        }
        let verified = run.0.iter().filter(|(_, entry)| entry.verified).count();
        let observed = run.0.len();
        document.set_metadata(candidates.icu, candidates.glibc);
        let selected = document.variants.ensure(
            variant.to_string(),
            Variant {
                locales: Members::empty(),
                baseline_displaced: None,
            },
        );
        selected.baseline_displaced = Some(Members(
            baseline
                .0
                .iter()
                .map(|(block, words)| (block.clone(), words.iter().cloned().collect()))
                .collect(),
        ));
        for (name, result) in run.0 {
            selected.locales.insert(name, result);
        }

        let mut final_locales = Members::empty();
        for (name, candidate) in candidates.locales {
            let tag = (0..candidate.tags.len()).find_map(|index| {
                document
                    .variants
                    .get(&index.to_string())
                    .and_then(|saved| saved.locales.get(&name))
                    .filter(|saved| saved.verified)
                    .map(|saved| saved.tag.clone())
            });
            final_locales.insert(
                name,
                FinalLocale {
                    tag,
                    alphabet_data: candidate.alphabet_present()?,
                },
            );
        }
        let overall = final_locales
            .0
            .iter()
            .filter(|(_, entry)| entry.tag.as_ref().is_some_and(|tag| !tag.is_empty()))
            .count();
        if document.locales.is_none() {
            document.order.push(TopField::Locales);
        }
        document.locales = Some(final_locales);
        Ok(Self {
            document,
            verified,
            observed,
            variant,
            overall,
        })
    }

    pub fn json(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut output, formatter);
        self.document.serialize(&mut serializer)?;
        output.push(b'\n');
        Ok(output)
    }

    /// The final summary only, without LF or per-locale console messages.
    pub fn summary_line(&self) -> String {
        format!(
            "{} of {} locales verified with candidate {}; {} locales verified overall",
            self.verified, self.observed, self.variant, self.overall
        )
    }
}

#[derive(Deserialize)]
struct Candidates {
    icu: String,
    glibc: String,
    locales: BTreeMap<String, Candidate>,
}

#[derive(Deserialize)]
struct Candidate {
    tags: Vec<String>,
    alphabet: serde_json::Value,
}

impl Candidate {
    fn alphabet_present(&self) -> Result<bool> {
        match &self.alphabet {
            serde_json::Value::Null => Ok(false),
            serde_json::Value::Array(_) => Ok(true),
            _ => Err("candidate alphabet must be an array or null".into()),
        }
    }

    fn tag(&self, variant: i64) -> Result<&String> {
        let index = if variant < 0 {
            usize::try_from(variant.unsigned_abs())
                .ok()
                .and_then(|distance| self.tags.len().checked_sub(distance))
        } else {
            usize::try_from(variant).ok()
        };
        index
            .and_then(|index| self.tags.get(index))
            .ok_or_else(|| format!("candidate variant {variant} is unavailable").into())
    }
}

struct Raw {
    name: String,
    failures: Members<Failure>,
    scripts: Members<Option<BTreeSet<String>>>,
}

impl From<Observation> for Raw {
    fn from(observation: Observation) -> Self {
        let mut failures = Members::empty();
        for (label, outcome) in [
            ("alphabet", observation.alphabet),
            ("digits", observation.digits),
        ] {
            let failure = match outcome {
                Err(error) => Some(Failure::Failed(Failed { error })),
                Ok(difference) if !difference.equal => Some(Failure::Moved(Moved {
                    displaced: difference.displaced.len(),
                    first: difference.displaced.into_iter().take(40).collect(),
                })),
                Ok(_) => None,
            };
            if let Some(failure) = failure {
                failures.insert(label.into(), failure);
            }
        }
        let mut scripts = Members::empty();
        for (block, words) in observation.scripts {
            scripts.insert(block, words);
        }
        Self {
            name: observation.name,
            failures,
            scripts,
        }
    }
}

// Typed maps within this document retain insertion positions. Equality follows
// the original dictionaries, independently of their serialization order.
#[derive(Debug)]
struct Members<T>(Vec<(String, T)>);

impl<T> Members<T> {
    fn empty() -> Self {
        Self(Vec::new())
    }

    fn get(&self, name: &str) -> Option<&T> {
        self.0
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value)
    }

    fn insert(&mut self, name: String, value: T) {
        if let Some((_, old)) = self.0.iter_mut().find(|(key, _)| *key == name) {
            *old = value;
        } else {
            self.0.push((name, value));
        }
    }

    fn ensure(&mut self, name: String, initial: T) -> &mut T {
        let index = match self.0.iter().position(|(key, _)| *key == name) {
            Some(index) => index,
            None => {
                self.0.push((name, initial));
                self.0.len() - 1
            }
        };
        &mut self.0[index].1
    }
}

impl<T: PartialEq> PartialEq for Members<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self
                .0
                .iter()
                .all(|(key, value)| other.get(key) == Some(value))
    }
}

impl<T: Serialize> Serialize for Members<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, value) in &self.0 {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Members<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct MapVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for MapVisitor<T> {
            type Value = Members<T>;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an ordered verification map")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut members = Members::empty();
                while let Some((name, value)) = map.next_entry::<String, T>()? {
                    if members.get(&name).is_some() {
                        return Err(A::Error::custom("duplicate map key"));
                    }
                    members.0.push((name, value));
                }
                Ok(members)
            }
        }
        deserializer.deserialize_map(MapVisitor(PhantomData))
    }
}

fn field<'de, A: MapAccess<'de>, T: Deserialize<'de>>(
    map: &mut A,
    name: &'static str,
) -> std::result::Result<T, A::Error> {
    match map.next_key::<String>()? {
        Some(key) if key == name => map.next_value(),
        _ => Err(A::Error::custom(format!(
            "unsupported record field order, expected {name}"
        ))),
    }
}

fn end<'de, A: MapAccess<'de>>(map: &mut A) -> std::result::Result<(), A::Error> {
    if let Some(key) = map.next_key::<String>()? {
        return Err(A::Error::custom(format!(
            "unsupported record field {key:?}"
        )));
    }
    Ok(())
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Failure {
    Moved(Moved),
    Failed(Failed),
    Script(Extra),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
enum Extra {
    Count(usize),
    Error(ScriptError),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
enum ScriptError {
    #[serde(rename = "error")]
    Error,
}

impl<'de> Deserialize<'de> for ScriptError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Literal;
        impl<'de> Visitor<'de> for Literal {
            type Value = ScriptError;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("the string \"error\"")
            }
            fn visit_str<E: serde::de::Error>(
                self,
                value: &str,
            ) -> std::result::Result<ScriptError, E> {
                if value == "error" {
                    Ok(ScriptError::Error)
                } else {
                    Err(E::custom("expected the string \"error\""))
                }
            }
        }
        deserializer.deserialize_str(Literal)
    }
}

#[derive(Debug, PartialEq, Serialize)]
struct Failed {
    error: [String; 2],
}

impl<'de> Deserialize<'de> for Failed {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Record;
        impl<'de> Visitor<'de> for Record {
            type Value = Failed;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an error record")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Failed, A::Error> {
                let error = field(&mut map, "error")?;
                end(&mut map)?;
                Ok(Failed { error })
            }
        }
        deserializer.deserialize_map(Record)
    }
}

#[derive(Debug, PartialEq, Serialize)]
struct Moved {
    displaced: usize,
    first: Vec<String>,
}

impl<'de> Deserialize<'de> for Moved {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Record;
        impl<'de> Visitor<'de> for Record {
            type Value = Moved;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a displaced/first record")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Moved, A::Error> {
                let displaced = field(&mut map, "displaced")?;
                let first = field(&mut map, "first")?;
                end(&mut map)?;
                Ok(Moved { displaced, first })
            }
        }
        deserializer.deserialize_map(Record)
    }
}

#[derive(Debug, Serialize)]
struct StoredLocale {
    verified: bool,
    tag: String,
    failures: Members<Failure>,
    other_scripts: Members<Extra>,
}

impl<'de> Deserialize<'de> for StoredLocale {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Record;
        impl<'de> Visitor<'de> for Record {
            type Value = StoredLocale;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a verified/tag/failures/other_scripts record")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<StoredLocale, A::Error> {
                let verified = field(&mut map, "verified")?;
                let tag = field(&mut map, "tag")?;
                let failures: Members<Failure> = field(&mut map, "failures")?;
                let other_scripts = field(&mut map, "other_scripts")?;
                end(&mut map)?;
                for (label, value) in &failures.0 {
                    let supported = if label == "alphabet" || label == "digits" {
                        matches!(value, Failure::Moved(_) | Failure::Failed(_))
                    } else {
                        label.starts_with("block:") && matches!(value, Failure::Script(_))
                    };
                    if !supported {
                        return Err(A::Error::custom("unsupported failure label/shape"));
                    }
                }
                Ok(StoredLocale {
                    verified,
                    tag,
                    failures,
                    other_scripts,
                })
            }
        }
        deserializer.deserialize_map(Record)
    }
}

#[derive(Debug, Serialize)]
struct Variant {
    locales: Members<StoredLocale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    baseline_displaced: Option<Members<Vec<String>>>,
}

impl<'de> Deserialize<'de> for Variant {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Record;
        impl<'de> Visitor<'de> for Record {
            type Value = Variant;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a locales/baseline_displaced record")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Variant, A::Error> {
                let locales = field(&mut map, "locales")?;
                let baseline_displaced = match map.next_key::<String>()? {
                    Some(key) if key == "baseline_displaced" => Some(map.next_value()?),
                    Some(_) => return Err(A::Error::custom("unsupported variant field order")),
                    None => None,
                };
                end(&mut map)?;
                Ok(Variant {
                    locales,
                    baseline_displaced,
                })
            }
        }
        deserializer.deserialize_map(Record)
    }
}

#[derive(Debug, Serialize)]
struct FinalLocale {
    tag: Option<String>,
    alphabet_data: bool,
}

impl<'de> Deserialize<'de> for FinalLocale {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Record;
        impl<'de> Visitor<'de> for Record {
            type Value = FinalLocale;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a tag/alphabet_data record")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<FinalLocale, A::Error> {
                let tag = field(&mut map, "tag")?;
                let alphabet_data = field(&mut map, "alphabet_data")?;
                end(&mut map)?;
                Ok(FinalLocale { tag, alphabet_data })
            }
        }
        deserializer.deserialize_map(Record)
    }
}

#[derive(Clone, Copy, Debug)]
enum TopField {
    Variants,
    Icu,
    Glibc,
    Locales,
}

#[derive(Debug)]
struct Document {
    variants: Members<Variant>,
    icu: Option<String>,
    glibc: Option<String>,
    locales: Option<Members<FinalLocale>>,
    order: Vec<TopField>,
}

impl Document {
    fn empty() -> Self {
        Self {
            variants: Members::empty(),
            icu: None,
            glibc: None,
            locales: None,
            order: vec![TopField::Variants],
        }
    }

    fn set_metadata(&mut self, icu: String, glibc: String) {
        if self.icu.is_none() {
            self.order.push(TopField::Icu);
        }
        self.icu = Some(icu);
        if self.glibc.is_none() {
            self.order.push(TopField::Glibc);
        }
        self.glibc = Some(glibc);
    }
}

impl<'de> Deserialize<'de> for Document {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Record;
        impl<'de> Visitor<'de> for Record {
            type Value = Document;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a known verification document with variants first")
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Document, A::Error> {
                let mut document = Document::empty();
                document.variants = field(&mut map, "variants")?;
                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "icu" if document.icu.is_none() => {
                            document.icu = Some(map.next_value()?);
                            document.order.push(TopField::Icu);
                        }
                        "glibc" if document.glibc.is_none() => {
                            document.glibc = Some(map.next_value()?);
                            document.order.push(TopField::Glibc);
                        }
                        "locales" if document.locales.is_none() => {
                            document.locales = Some(map.next_value()?);
                            document.order.push(TopField::Locales);
                        }
                        _ => {
                            return Err(A::Error::custom(
                                "unsupported or duplicate top-level field",
                            ));
                        }
                    }
                }
                Ok(document)
            }
        }
        deserializer.deserialize_map(Record)
    }
}

impl Serialize for Document {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.order.len()))?;
        for key in &self.order {
            match key {
                TopField::Variants => map.serialize_entry("variants", &self.variants)?,
                TopField::Icu => map.serialize_entry("icu", &self.icu)?,
                TopField::Glibc => map.serialize_entry("glibc", &self.glibc)?,
                TopField::Locales => map.serialize_entry("locales", &self.locales)?,
            }
        }
        map.end()
    }
}
