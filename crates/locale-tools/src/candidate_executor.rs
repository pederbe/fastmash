//! Bounded GNU sort probes for the retained candidate-selection protocol.

use crate::Result;
use crate::capture::{Budget, capture_pipe};
use crate::collation_candidates::{Candidates, Exemplars, NumericLocales};
use serde_json::json;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub fn generate(
    numeric: NumericLocales,
    exemplars: Exemplars,
    sort: &Path,
    locpath: &Path,
    evidence: &Path,
) -> Result<Candidates> {
    let sort = fs::canonicalize(sort)?;
    let locpath = fs::canonicalize(locpath)?;
    fs::DirBuilder::new().mode(0o700).create(evidence)?;
    let mut next = 0;
    exemplars.project(numeric, |name| {
        next += 1;
        let directory = evidence.join(format!("locale-{next:06}"));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let locale = format!("{name}.UTF-8");
        fs::write(
            directory.join("command.json"),
            serde_json::to_vec_pretty(&json!({
                "program_bytes": sort.as_os_str().as_bytes(), "arguments": [],
                "environment_cleared": true,
                "environment": {"PATH": "/usr/bin:/bin", "LC_ALL": locale,
                    "LOCPATH_bytes": locpath.as_os_str().as_bytes(), "TZ": "UTC"},
                "execution_milliseconds": 300_000, "stdin_transport": "pipe",
                "stdout_transport": "regular-file", "stderr_transport": "regular-file",
                "legacy_output_transport": "pipe",
            }))?,
        )?;
        let mut command = Command::new(&sort);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", locale)
            .env("LOCPATH", &locpath)
            .env("TZ", "UTC");
        let output = capture_pipe(
            &mut command,
            Some(b"a\nA\n"),
            &directory,
            Budget::comparison(Duration::from_secs(300)),
        )?;
        if !output.status.success() || !output.stderr.is_empty() {
            return Err(format!(
                "case-order probe for {name:?} failed: {} (evidence: {})",
                output.status,
                directory.display()
            )
            .into());
        }
        match output.stdout.as_slice() {
            b"A\na\n" => Ok(true),
            b"a\nA\n" => Ok(false),
            _ => Err(format!(
                "case-order probe for {name:?} returned invalid output (evidence: {})",
                directory.display()
            )
            .into()),
        }
    })
}
