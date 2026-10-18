//! The fixed native collation command protocol shared by development runners.

use crate::Result;
use crate::capture::{Budget, Captured, capture_pipe};
use serde_json::json;
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

#[derive(Clone, Copy)]
pub(crate) enum Role {
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

pub(crate) fn invoke(
    tools: &Tools,
    role: Role,
    name: &str,
    input: &[u8],
    pair: &Path,
    operation: &str,
) -> Result<Captured> {
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
    let locale = format!("{name}.UTF-8");
    let budget = Budget::comparison(Duration::from_secs(300));
    fs::write(
        directory.join("command.json"),
        serde_json::to_vec_pretty(&json!({
            "program": program.to_string_lossy(),
            "program_bytes": program.as_os_str().as_bytes(),
            "argv0": role.argv0(),
            "arguments": arguments.iter().map(|arg| arg.to_string_lossy()).collect::<Vec<_>>(),
            "arguments_bytes": arguments.iter().map(|arg| arg.as_bytes()).collect::<Vec<_>>(),
            "locale_key": name,
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
    capture_pipe(&mut command, Some(input), &directory, budget)
        .map_err(|error| format!("{} {operation} for {name:?}: {error}", role.directory()).into())
}
