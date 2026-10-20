//! Exact pair comparisons through the independently identified GNU sort.

use crate::Result;
use crate::capture::{Budget, capture_pipe};
use serde_json::json;
use std::cmp::Ordering;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct Pair {
    pub left: String,
    pub right: String,
}

/// Every pair is sent twice with opposite input order. A stable sort leaves
/// ties in those opposite orders, whereas unequal pairs have the same order.
/// Numeric group prefixes keep independent comparisons together. NUL is both
/// the record delimiter and the field separator, so embedded tabs and newlines
/// remain part of the sole field. Character positions skip only ASCII digits.
pub struct Batch<'a> {
    pairs: &'a [Pair],
    width: usize,
    pub input: Vec<u8>,
}

impl<'a> Batch<'a> {
    pub fn new(pairs: &'a [Pair]) -> Result<Self> {
        let groups = pairs
            .len()
            .checked_mul(2)
            .ok_or("too many comparison pairs")?;
        let width = groups.saturating_sub(1).to_string().len();
        let mut input = Vec::new();
        for (index, pair) in pairs.iter().enumerate() {
            if pair.left.contains('\0') || pair.right.contains('\0') {
                return Err("glibc comparison strings must contain no NUL".into());
            }
            for (id, values) in [
                (index * 2, [&pair.left, &pair.right]),
                (index * 2 + 1, [&pair.right, &pair.left]),
            ] {
                let prefix = format!("{id:0width$}");
                for value in values {
                    let bytes = prefix
                        .len()
                        .checked_add(value.len())
                        .and_then(|n| n.checked_add(1))
                        .ok_or("comparison input size overflow")?;
                    input.try_reserve(bytes)?;
                    input.extend_from_slice(prefix.as_bytes());
                    input.extend_from_slice(value.as_bytes());
                    input.push(0);
                }
            }
        }
        Ok(Self {
            pairs,
            width,
            input,
        })
    }

    pub fn arguments(&self) -> [String; 5] {
        [
            "--stable".into(),
            "--zero-terminated".into(),
            r"--field-separator=\0".into(),
            format!("--key=1.1,1.{}n", self.width),
            format!("--key=1.{}", self.width + 1),
        ]
    }

    pub fn decode(&self, output: &[u8]) -> Result<Vec<Ordering>> {
        if self.pairs.is_empty() {
            return if output.is_empty() {
                Ok(Vec::new())
            } else {
                Err("unexpected comparison output".into())
            };
        }
        if output.last() != Some(&0) {
            return Err("comparison output lacks its final NUL".into());
        }
        let mut records = output[..output.len() - 1].split(|byte| *byte == 0);
        let mut orders = Vec::new();
        orders.try_reserve(self.pairs.len())?;
        for (index, pair) in self.pairs.iter().enumerate() {
            let mut orientation = |id| -> Result<bool> {
                let prefix = format!("{id:0width$}", width = self.width);
                let mut next = || -> Result<&[u8]> {
                    records
                        .next()
                        .ok_or("truncated comparison output")?
                        .strip_prefix(prefix.as_bytes())
                        .ok_or_else(|| "comparison groups changed".into())
                };
                let first = next()?;
                let second = next()?;
                if first == pair.left.as_bytes() && second == pair.right.as_bytes() {
                    Ok(true)
                } else if first == pair.right.as_bytes() && second == pair.left.as_bytes() {
                    Ok(false)
                } else {
                    Err("comparison payload changed".into())
                }
            };
            let forward = orientation(index * 2)?;
            let reverse = orientation(index * 2 + 1)?;
            orders.push(if pair.left == pair.right {
                Ordering::Equal
            } else {
                match (forward, reverse) {
                    (true, true) => Ordering::Less,
                    (false, false) => Ordering::Greater,
                    (true, false) => Ordering::Equal,
                    (false, true) => return Err("reference sort is not stable".into()),
                }
            });
        }
        if records.next().is_some() {
            return Err("extra comparison records".into());
        }
        Ok(orders)
    }
}

/// One admitted glibc collation locale and its retained native captures.
pub struct Reference {
    sort: std::path::PathBuf,
    locpath: std::path::PathBuf,
    locale: String,
    root: std::path::PathBuf,
    next: usize,
}

impl Reference {
    pub fn open(
        sort: &Path,
        locale_tool: &Path,
        locpath: &Path,
        locale: &str,
        evidence: &Path,
    ) -> Result<Self> {
        let sort = fs::canonicalize(sort)?;
        let locale_tool = fs::canonicalize(locale_tool)?;
        let locpath = fs::canonicalize(locpath)?;
        fs::DirBuilder::new().mode(0o700).create(evidence)?;
        let reference = Self {
            sort,
            locpath,
            locale: locale.into(),
            root: evidence.into(),
            next: 0,
        };
        // GNU sort can silently use C when setlocale fails. The matching glibc
        // locale command diagnoses failure even when it exits successfully.
        let directory = evidence.join("locale-admission");
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let output = reference.capture(
            &locale_tool,
            &["-k".into(), "LC_COLLATE".into()],
            None,
            &directory,
            Budget::query(Duration::from_secs(5)),
        )?;
        if !output.status.success() || !output.stderr.is_empty() || output.stdout.is_empty() {
            return Err(format!(
                "cannot admit collation locale {locale:?} (captures: {})",
                directory.display()
            )
            .into());
        }
        if locale != "C"
            && locale != "POSIX"
            && !output
                .stdout
                .split(|byte| *byte == b'\n')
                .any(|line| line == b"collate-codeset=\"UTF-8\"")
        {
            return Err(format!("collation locale {locale:?} is not identified as UTF-8").into());
        }
        Ok(reference)
    }

    fn capture(
        &self,
        program: &Path,
        arguments: &[String],
        input: Option<&[u8]>,
        directory: &Path,
        budget: Budget,
    ) -> Result<crate::capture::Captured> {
        fs::write(
            directory.join("command.json"),
            serde_json::to_vec_pretty(&json!({
                "program_bytes": program.as_os_str().as_bytes(), "arguments": arguments,
                "environment_cleared": true,
                "environment": {"PATH":"/usr/bin:/bin", "LANG":"C", "LC_COLLATE":self.locale,
                    "LOCPATH_bytes":self.locpath.as_os_str().as_bytes(), "TZ":"UTC"},
                "execution_milliseconds":budget.execution.as_millis(),
                "cleanup_milliseconds":budget.cleanup.as_millis(),
                "stdin_transport":if input.is_some() {"pipe"} else {"null"},
                "stdout_transport":"regular-file", "stderr_transport":"regular-file",
            }))?,
        )?;
        let mut command = Command::new(program);
        // The old newlocale handle selected LC_COLLATE only. All other
        // categories use C, independently of the caller's environment.
        command
            .args(arguments)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .env("LC_COLLATE", &self.locale)
            .env("LOCPATH", &self.locpath)
            .env("TZ", "UTC");
        capture_pipe(&mut command, input, directory, budget)
    }

    pub fn compare(&mut self, pairs: &[Pair]) -> Result<Vec<Ordering>> {
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        let batch = Batch::new(pairs)?;
        let directory = self.root.join(format!("batch-{:06}", self.next));
        self.next = self
            .next
            .checked_add(1)
            .ok_or("comparison batch count overflow")?;
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let output = self.capture(
            &self.sort,
            &batch.arguments(),
            Some(&batch.input),
            &directory,
            Budget::comparison(Duration::from_secs(300)),
        )?;
        if !output.status.success() || !output.stderr.is_empty() {
            return Err(format!(
                "GNU comparison failed: {} (captures: {})",
                output.status,
                directory.display()
            )
            .into());
        }
        batch.decode(&output.stdout)
    }
}
