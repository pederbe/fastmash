//! Punctuation reports from supplied comparison observations.

use crate::Result;
use crate::collation_plan::OrderDifference;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Debug)]
pub enum PunctuationOutcome {
    Compared(OrderDifference),
    /// Already decoded candidate and reference diagnostics, in that order.
    Failed([String; 2]),
}

#[derive(Debug)]
pub struct Report {
    word_count: usize,
    outcomes: BTreeMap<String, PunctuationOutcome>,
}

impl Report {
    /// Compared observations carry the full sorted unique displacement list
    /// supplied by the order comparison. Its equality bit is not an admission
    /// condition for punctuation reporting.
    pub fn new(word_count: usize, outcomes: BTreeMap<String, PunctuationOutcome>) -> Self {
        Self {
            word_count,
            outcomes,
        }
    }

    /// Original report shape, one-space indentation, raw UTF-8 and final LF.
    pub fn json(&self) -> Result<Vec<u8>> {
        let mut output = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut output, formatter);
        Document {
            words: self.word_count,
            locales: Entries(&self.outcomes),
        }
        .serialize(&mut serializer)?;
        output.push(b'\n');
        Ok(output)
    }

    /// Errors precede numeric buckets, whose decimal text uses lexical order.
    /// The returned line excludes its final LF.
    pub fn summary_line(&self) -> String {
        let mut counts = BTreeMap::<String, usize>::new();
        for outcome in self.outcomes.values() {
            let key = match outcome {
                PunctuationOutcome::Compared(difference) => difference.displaced.len().to_string(),
                PunctuationOutcome::Failed(_) => "error".into(),
            };
            *counts.entry(key).or_default() += 1;
        }
        // The original tuple-string sort puts a quoted key before decimal keys.
        let mut buckets = Vec::new();
        if let Some(count) = counts.remove("error") {
            buckets.push(format!("error: {count}"));
        }
        buckets.extend(
            counts
                .into_iter()
                .map(|(key, count)| format!("{key}: {count}")),
        );
        format!(
            "{} locales, {} words; locales by displaced words: {}",
            self.outcomes.len(),
            self.word_count,
            buckets.join(", ")
        )
    }
}

#[derive(Serialize)]
struct Document<'a> {
    words: usize,
    locales: Entries<'a>,
}

struct Entries<'a>(&'a BTreeMap<String, PunctuationOutcome>);

#[derive(Serialize)]
#[serde(untagged)]
enum Entry<'a> {
    Compared {
        displaced: usize,
        words: &'a [String],
    },
    Failed {
        error: &'a [String; 2],
    },
}

impl Serialize for Entries<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (name, outcome) in self.0 {
            let entry = match outcome {
                PunctuationOutcome::Compared(difference) => Entry::Compared {
                    displaced: difference.displaced.len(),
                    words: &difference.displaced[..difference.displaced.len().min(40)],
                },
                PunctuationOutcome::Failed(error) => Entry::Failed { error },
            };
            map.serialize_entry(name, &entry)?;
        }
        map.end()
    }
}
