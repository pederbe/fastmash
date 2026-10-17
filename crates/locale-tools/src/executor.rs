//! Unix execution of the historical numeric-locale comparison plan.
//!
//! Stdout and stderr use regular-file captures. The old Python comparator used
//! pipes, so equivalence still needs qualification on an admitted reference.

use crate::Result;
use crate::capture::{Budget, Captured, capture};
use crate::comparison::{Case, Plan, PlannedLocale, normalize_stderr};
use serde::Serialize;
use serde_json::json;
use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, DirBuilder};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};
use std::time::Duration;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct Summary {
    pub completed_pairs: usize,
    pub mismatches: usize,
    /// At most the first 25 completed-pair mismatches, in plan order.
    pub diagnostics: Vec<String>,
}

#[derive(Debug)]
pub struct Failure {
    pub summary: Summary,
    pub evidence: PathBuf,
    pub message: String,
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} (comparison evidence: {})",
            self.message,
            self.evidence.display()
        )
    }
}

impl std::error::Error for Failure {}

/// Run candidate then reference for every case in locale-major order. The
/// evidence directory must be new. Raw successful pairs remain available.
pub fn execute(
    plan: &Plan,
    candidate: &Path,
    reference: &Path,
    locpath: &OsStr,
    evidence: &Path,
) -> std::result::Result<Summary, Failure> {
    let mut summary = Summary::default();
    if let Err(error) = DirBuilder::new().mode(0o700).create(evidence) {
        return Err(Failure {
            summary,
            evidence: evidence.into(),
            message: error.to_string(),
        });
    }
    let mut pair = 0;
    let result = compare_with(plan, &mut summary, |role, locale, index, case| {
        if role == Role::Candidate {
            pair += 1;
        }
        let directory = evidence.join(format!("pair-{pair:06}"));
        if role == Role::Candidate {
            DirBuilder::new().mode(0o700).create(&directory)?;
        }
        let program = if role == Role::Candidate {
            candidate
        } else {
            reference
        };
        invoke(program, role, locale, index, case, locpath, &directory).map_err(|error| {
            format!(
                "{} {} case {index}: {error}",
                role.directory(),
                locale.locale
            )
            .into()
        })
    })
    .and_then(|()| {
        fs::write(
            evidence.join("summary.json"),
            serde_json::to_vec_pretty(&summary)?,
        )?;
        Ok(())
    });
    match result {
        Ok(()) => Ok(summary),
        Err(error) => {
            let message = error.to_string();
            let _ = fs::write(evidence.join("failure.txt"), format!("{message}\n"));
            if let Ok(raw) = serde_json::to_vec_pretty(&summary) {
                let _ = fs::write(evidence.join("summary.json"), raw);
            }
            Err(Failure {
                summary,
                evidence: evidence.into(),
                message,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

fn invoke(
    program: &Path,
    role: Role,
    locale: &PlannedLocale,
    index: usize,
    case: &Case<'_>,
    locpath: &OsStr,
    pair: &Path,
) -> Result<Captured> {
    let directory = pair.join(role.directory());
    DirBuilder::new().mode(0o700).create(&directory)?;
    let budget = Budget::comparison(Duration::from_secs(30));
    fs::write(
        directory.join("command.json"),
        serde_json::to_vec_pretty(&json!({
            "program": program.to_string_lossy(), "program_bytes": program.as_os_str().as_bytes(),
            "argv0": role.argv0(), "arguments": &case.arguments,
            "locale_key": locale.name, "case_index": index,
            "working_directory_bytes": std::env::current_dir()?.as_os_str().as_bytes(),
            "environment_cleared": true,
            "environment": { "PATH": "/usr/bin:/bin", "LC_ALL": locale.locale,
                "LOCPATH_bytes": locpath.as_bytes(), "TZ": "UTC" },
            "stdin_transport": "pipe", "stdout_transport": "regular-file",
            "stderr_transport": "regular-file",
            "legacy_output_transport": "pipe",
            "execution_milliseconds": budget.execution.as_millis(),
            "cleanup_milliseconds": budget.cleanup.as_millis(),
        }))?,
    )?;
    let mut command = Command::new(program);
    command
        .arg0(role.argv0())
        .args(&case.arguments)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", &locale.locale)
        .env("LOCPATH", locpath)
        .env("TZ", "UTC");
    capture(&mut command, Some(case.input), &directory, budget)
}

fn compare_with(
    plan: &Plan,
    summary: &mut Summary,
    mut invoke: impl FnMut(Role, &PlannedLocale, usize, &Case<'_>) -> Result<Captured>,
) -> Result<()> {
    for locale in &plan.locales {
        for (index, case) in locale.cases().enumerate() {
            let candidate = invoke(Role::Candidate, locale, index, &case)?;
            let candidate_tuple = (
                signed_status(candidate.status)?,
                &candidate.stdout,
                normalize_stderr(&candidate.stderr),
            );
            let reference = invoke(Role::Reference, locale, index, &case)?;
            let reference_tuple = (
                signed_status(reference.status)?,
                &reference.stdout,
                normalize_stderr(&reference.stderr),
            );
            summary.completed_pairs += 1;
            if candidate_tuple != reference_tuple {
                summary.mismatches += 1;
                if summary.diagnostics.len() < 25 {
                    summary.diagnostics.push(format!("Mismatch {} case {index}: candidate {candidate_tuple:?}, reference {reference_tuple:?}", locale.locale));
                }
            }
        }
    }
    Ok(())
}

fn signed_status(status: ExitStatus) -> Result<i32> {
    status
        .code()
        .or_else(|| status.signal().map(|signal| -signal))
        .ok_or_else(|| "native invocation returned no exit code or terminal signal".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;

    fn plan() -> Plan {
        Plan::from_json(br#"{"locales":{"a":{"decimal_point":".","thousands_sep":"","grouping":[-1]},"b":{"decimal_point":",","thousands_sep":".","grouping":[3]}}}"#, true).unwrap()
    }

    fn reply(status: i32, stdout: &[u8], stderr: &[u8]) -> Captured {
        Captured {
            status: std::process::ExitStatus::from_raw(status << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    #[test]
    fn loop_keeps_role_order_counts_all_mismatches_and_limits_diagnostics() {
        let mut summary = Summary::default();
        let mut calls = Vec::new();
        compare_with(&plan(), &mut summary, |role, locale, index, _| {
            calls.push((role, locale.name.clone(), index));
            Ok(reply(
                77,
                if role == Role::Candidate { b"a" } else { b"b" },
                b"\xffwarning",
            ))
        })
        .unwrap();
        assert_eq!(summary.completed_pairs, 42);
        assert_eq!(summary.mismatches, 42);
        assert_eq!(summary.diagnostics.len(), 25);
        assert!(summary.diagnostics[0].starts_with("Mismatch a.UTF-8 case 0:"));
        assert!(summary.diagnostics[24].starts_with("Mismatch b.UTF-8 case 3:"));
        assert_eq!(
            &calls[..4],
            &[
                (Role::Candidate, "a".into(), 0),
                (Role::Reference, "a".into(), 0),
                (Role::Candidate, "a".into(), 1),
                (Role::Reference, "a".into(), 1)
            ]
        );
        assert_eq!(
            &calls[41..43],
            &[
                (Role::Reference, "a".into(), 20),
                (Role::Candidate, "b".into(), 0)
            ]
        );
    }

    #[test]
    fn abnormal_invocation_never_completes_a_partial_pair() {
        for (fail_at, expected_completed) in [(1, 0), (4, 1)] {
            let mut summary = Summary::default();
            let mut calls = 0;
            let result = compare_with(&plan(), &mut summary, |role, _, _, _| {
                calls += 1;
                if calls == fail_at {
                    return Err("fixture transport failure".into());
                }
                Ok(reply(
                    70,
                    b"same",
                    if role == Role::Candidate {
                        b"fastmash: bad"
                    } else {
                        b"datamash: bad"
                    },
                ))
            });
            assert!(result.is_err());
            assert_eq!(calls, fail_at);
            assert_eq!(summary.completed_pairs, expected_completed);
            assert_eq!(summary.mismatches, 0);
        }
    }

    #[test]
    fn signed_status_preserves_exit_codes_and_native_signal_numbers() {
        for code in [0, 1, 70, 77] {
            assert_eq!(
                signed_status(std::process::ExitStatus::from_raw(code << 8)).unwrap(),
                code
            );
        }
        assert_eq!(
            signed_status(std::process::ExitStatus::from_raw(9)).unwrap(),
            -9
        );
    }
}
