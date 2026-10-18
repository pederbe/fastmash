//! Sequential Unix verification of supplied collation candidates.
//!
//! The variant labels the report. It does not configure the supplied binary.

use crate::Result;
pub use crate::collation_command::Tools;
use crate::collation_command::{Role, invoke};
use crate::collation_plan::{compare_order_outputs, digit_words};
use crate::collation_unicode::{BLOCKS, alphabet_words, block_words};
use crate::collation_verification::{CaseOutcome, Observation, Report};
use serde_json::{Value, json};
use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

/// Run all 31 cases for each selected occurrence, candidate then reference.
/// Empty `requested` selects sorted candidate keys. Explicit requests retain
/// their sequence and duplicates. Append en_US when it is absent.
///
/// Evidence must be a new directory and is retained on every outcome. Native
/// nonzero statuses are observations and do not stop later cases. Transport,
/// decoding and bookkeeping errors abort with partial evidence. The returned
/// report keeps the report owner's supported prior-document contract.
pub fn execute(
    candidates: &[u8],
    prior: Option<&[u8]>,
    variant: i64,
    requested: &[String],
    tools: &Tools,
    evidence: &Path,
) -> Result<Report> {
    DirBuilder::new()
        .mode(0o700)
        .create(evidence)
        .map_err(|error| format!("cannot create verification evidence: {error}"))?;
    match execute_in(candidates, prior, variant, requested, tools, evidence) {
        Ok(report) => Ok(report),
        Err(error) => {
            let message = error.to_string();
            let _ = fs::write(evidence.join("failure.txt"), format!("{message}\n"));
            Err(format!("{message} (verification evidence: {})", evidence.display()).into())
        }
    }
}

fn execute_in(
    candidates: &[u8],
    prior: Option<&[u8]>,
    variant: i64,
    requested: &[String],
    tools: &Tools,
    evidence: &Path,
) -> Result<Report> {
    fs::write(evidence.join("candidates.json"), candidates)?;
    if let Some(prior) = prior {
        fs::write(evidence.join("prior.json"), prior)?;
    }
    let document: Value = serde_json::from_slice(candidates)?;
    let entries = document
        .get("locales")
        .and_then(Value::as_object)
        .ok_or("candidate input requires a locales object")?;
    let mut names = if requested.is_empty() {
        let mut names: Vec<_> = entries.keys().cloned().collect();
        names.sort();
        names
    } else {
        requested.to_vec()
    };
    if !names.iter().any(|name| name == "en_US") {
        names.push("en_US".into());
    }
    let case_order: Vec<_> = ["alphabet", "digits"]
        .into_iter()
        .chain(BLOCKS.iter().map(|block| block.name))
        .collect();
    fs::write(
        evidence.join("selection.json"),
        serde_json::to_vec_pretty(&json!({
            "variant": variant, "names": names, "execution": "sequential",
            "case_order": case_order,
        }))?,
    )?;
    // Require both fields explicitly. Missing exemplars are not JSON null.
    // Only selected entries need planning data, as in the original verifier.
    let selected = names
        .iter()
        .map(|name| {
            let entry = entries
                .get(name)
                .ok_or_else(|| format!("requested locale {name:?} is not a candidate"))?;
            Ok((
                name,
                exemplars(entry, name, "alphabet")?,
                exemplars(entry, name, "auxiliary")?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let blocks = BLOCKS
        .iter()
        .map(|block| Ok((block.name, block_words(block.name)?)))
        .collect::<Result<Vec<_>>>()?;
    let digits = digit_words();
    let mut observations = Vec::new();
    for (index, (name, alphabet, auxiliary)) in selected.into_iter().enumerate() {
        let directory = evidence.join(format!("locale-{:06}", index + 1));
        DirBuilder::new().mode(0o700).create(&directory)?;
        let language = name
            .split_once('_')
            .map_or(name.as_str(), |(language, _)| language);
        fs::write(
            directory.join("locale.json"),
            serde_json::to_vec_pretty(&json!({
                "name": name, "occurrence": index + 1, "language": language,
            }))?,
        )?;
        let words = alphabet_words(alphabet.as_deref(), auxiliary.as_deref(), language);
        let alphabet = run_case(tools, name, 0, "alphabet", &words, &directory)?;
        let digits = run_case(tools, name, 1, "digits", &digits, &directory)?;
        let mut scripts = Vec::new();
        for (index, (label, words)) in blocks.iter().enumerate() {
            let outcome = run_case(tools, name, index + 2, label, words, &directory)?;
            scripts.push((
                (*label).into(),
                outcome
                    .ok()
                    .map(|difference| difference.displaced.into_iter().collect()),
            ));
        }
        observations.push(Observation {
            name: name.clone(),
            alphabet,
            digits,
            scripts,
        });
    }
    Report::update(prior, candidates, variant, observations)
}

fn exemplars(entry: &Value, name: &str, field: &str) -> Result<Option<Vec<String>>> {
    let value = entry
        .get(field)
        .ok_or_else(|| format!("candidate {name:?} has no {field} field"))?;
    match value {
        Value::Null => Ok(None),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                value.as_str().map(str::to_owned).ok_or_else(|| {
                    format!("candidate {name:?} {field} must contain strings").into()
                })
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        _ => Err(format!("candidate {name:?} {field} must be an array or null").into()),
    }
}

fn run_case(
    tools: &Tools,
    name: &str,
    index: usize,
    label: &str,
    words: &[String],
    directory: &Path,
) -> Result<CaseOutcome> {
    let pair = directory.join(format!("case-{index:02}-{label}"));
    DirBuilder::new().mode(0o700).create(&pair)?;
    let mut input = Vec::new();
    for word in words {
        input.extend_from_slice(word.as_bytes());
        input.push(b'\n');
    }
    fs::write(
        pair.join("case.json"),
        serde_json::to_vec_pretty(&json!({
            "locale_key": name, "index": index, "label": label,
            "words": words.len(), "input_bytes": input.len(),
        }))?,
    )?;
    let operation = format!("verification {label}");
    let candidate = invoke(tools, Role::Candidate, name, &input, &pair, &operation)?;
    let reference = invoke(tools, Role::Reference, name, &input, &pair, &operation)?;
    if !candidate.status.success() || !reference.status.success() {
        return Ok(Err([
            String::from_utf8_lossy(&candidate.stderr).into_owned(),
            String::from_utf8_lossy(&reference.stderr).into_owned(),
        ]));
    }
    compare_order_outputs(&candidate.stdout, &reference.stdout)
        .map(Ok)
        .map_err(|error| format!("cannot compare {label} for {name:?}: {error}").into())
}
