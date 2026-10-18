//! Unix execution of the fixed punctuation comparison, with retained evidence.

use crate::Result;
use crate::collation::{Row, Table};
pub use crate::collation_command::Tools;
use crate::collation_command::{Role, invoke};
use crate::collation_plan::{compare_order_outputs, punctuation_words};
use crate::collation_punctuation::{PunctuationOutcome, Report};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, DirBuilder};
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

/// Run each selected locale's candidate then reference and preserve the original
/// punctuation report. Supply `Table::from_verification` to retain its final-tag
/// selection. Empty `requested` selects all supplied table rows. Other
/// requests must be a subset, and duplicates do not change the table's order.
/// The evidence directory must be new and is retained on success or failure.
/// Native nonzero statuses are report observations. Transport and output-order
/// decoding errors abort with partial evidence and no completed report.
pub fn execute(
    table: &Table,
    requested: &[String],
    tools: &Tools,
    evidence: &Path,
) -> Result<Report> {
    let requested: BTreeSet<_> = requested.iter().map(String::as_str).collect();
    for name in &requested {
        if !table.rows().iter().any(|row| row.name == *name) {
            return Err(format!("locale {name:?} has no nonempty verification tag").into());
        }
    }
    let rows: Vec<_> = table
        .rows()
        .iter()
        .filter(|row| requested.is_empty() || requested.contains(row.name.as_str()))
        .collect();
    DirBuilder::new()
        .mode(0o700)
        .create(evidence)
        .map_err(|error| format!("cannot create comparison evidence: {error}"))?;
    match execute_in(&rows, tools, evidence) {
        Ok(report) => Ok(report),
        Err(error) => {
            let message = error.to_string();
            let _ = fs::write(evidence.join("failure.txt"), format!("{message}\n"));
            Err(format!("{message} (comparison evidence: {})", evidence.display()).into())
        }
    }
}

fn execute_in(rows: &[&Row], tools: &Tools, evidence: &Path) -> Result<Report> {
    let words = punctuation_words();
    let mut input = words.join("\n").into_bytes();
    input.push(b'\n');
    let mut outcomes = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let pair = evidence.join(format!("locale-{:06}", index + 1));
        DirBuilder::new().mode(0o700).create(&pair)?;
        let candidate = invoke(
            tools,
            Role::Candidate,
            &row.name,
            &input,
            &pair,
            "punctuation",
        )?;
        let reference = invoke(
            tools,
            Role::Reference,
            &row.name,
            &input,
            &pair,
            "punctuation",
        )?;
        let outcome = if !candidate.status.success() || !reference.status.success() {
            PunctuationOutcome::Failed([
                String::from_utf8_lossy(&candidate.stderr).into_owned(),
                String::from_utf8_lossy(&reference.stderr).into_owned(),
            ])
        } else {
            PunctuationOutcome::Compared(
                compare_order_outputs(&candidate.stdout, &reference.stdout).map_err(|error| {
                    format!("cannot compare punctuation for {:?}: {error}", row.name)
                })?,
            )
        };
        outcomes.insert(row.name.clone(), outcome);
    }
    Ok(Report::new(words.len(), outcomes))
}
