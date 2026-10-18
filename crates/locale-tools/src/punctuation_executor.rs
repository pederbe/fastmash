//! Unix execution of the fixed punctuation comparison, with retained evidence.

use crate::Result;
use crate::capture::{Budget, Captured, capture_pipe};
use crate::collation::{Row, Table};
use crate::collation_plan::{compare_order_outputs, punctuation_words};
use crate::collation_punctuation::{PunctuationOutcome, Report};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Tools {
    pub candidate: PathBuf,
    pub reference: PathBuf,
    pub sort: PathBuf,
    pub locpath: PathBuf,
}

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
        let candidate = invoke(tools, Role::Candidate, row, &input, &pair)?;
        let reference = invoke(tools, Role::Reference, row, &input, &pair)?;
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

#[derive(Clone, Copy)]
enum Role {
    Candidate,
    Reference,
}

impl Role {
    fn directory(self) -> &'static str {
        match self {
            Self::Candidate => "candidate",
            Self::Reference => "reference",
        }
    }

    fn argv0(self) -> &'static str {
        match self {
            Self::Candidate => "fastmash",
            Self::Reference => "datamash",
        }
    }
}

fn invoke(tools: &Tools, role: Role, row: &Row, input: &[u8], pair: &Path) -> Result<Captured> {
    let directory = pair.join(role.directory());
    DirBuilder::new().mode(0o700).create(&directory)?;
    let (program, mut arguments) = match role {
        Role::Candidate => (&tools.candidate, Vec::new()),
        Role::Reference => (
            &tools.reference,
            vec![
                OsString::from("--sort-cmd"),
                tools.sort.as_os_str().to_owned(),
            ],
        ),
    };
    arguments.extend(["-s", "-g", "1", "count", "1"].map(OsString::from));
    let locale = format!("{}.UTF-8", row.name);
    let budget = Budget::comparison(Duration::from_secs(300));
    fs::write(
        directory.join("command.json"),
        serde_json::to_vec_pretty(&json!({
            "program": program.to_string_lossy(),
            "program_bytes": program.as_os_str().as_bytes(),
            "argv0": role.argv0(),
            "arguments": arguments.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),
            "arguments_bytes": arguments.iter().map(|arg| arg.as_bytes()).collect::<Vec<_>>(),
            "locale_key": row.name,
            "working_directory_bytes": std::env::current_dir()?.as_os_str().as_bytes(),
            "environment_cleared": true,
            "environment": { "PATH": "/usr/bin:/bin", "LC_ALL": locale,
                "LOCPATH_bytes": tools.locpath.as_os_str().as_bytes(), "TZ": "UTC" },
            "stdin_transport": "pipe", "stdout_transport": "regular-file",
            "stderr_transport": "regular-file", "legacy_output_transport": "pipe",
            "execution_milliseconds": budget.execution.as_millis(),
            "cleanup_milliseconds": budget.cleanup.as_millis(),
        }))?,
    )?;
    let mut command = Command::new(program);
    command
        .arg0(role.argv0())
        .args(&arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", &locale)
        .env("LOCPATH", &tools.locpath)
        .env("TZ", "UTC");
    capture_pipe(&mut command, Some(input), &directory, budget).map_err(|error| {
        format!(
            "{} punctuation for {:?}: {error}",
            role.directory(),
            row.name
        )
        .into()
    })
}
