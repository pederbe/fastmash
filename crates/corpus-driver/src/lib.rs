#![forbid(unsafe_code)]

//! Frozen Command checks, including directory input and closed output pipes.
//! Remaining transports stay in the full corpus gate until migration completes.

pub mod fixture;
mod process;

use fixture::{Fixture, digest};
use process::{Scratch, invoke, require_unblocked_signals};
use serde::Serialize;
use serde_json::json;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Error(Box<dyn std::error::Error + Send + Sync>);

impl Error {
    pub fn message(message: impl Into<String>) -> Self {
        Self(std::io::Error::other(message.into()).into())
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self(error.into())
    }
}
impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self(error.into())
    }
}
impl From<nix::errno::Errno> for Error {
    fn from(error: nix::errno::Errno) -> Self {
        Self(error.into())
    }
}

#[derive(Debug)]
pub struct Options {
    pub binary: PathBuf,
    pub fixture: PathBuf,
    pub root: PathBuf,
    pub pattern: String,
    pub keep_going: bool,
    pub expected_version: Option<String>,
    pub stdin_file: bool,
    pub evidence_dir: Option<PathBuf>,
}

impl Options {
    pub fn new(binary: PathBuf) -> Self {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        Self {
            binary,
            fixture: root.join("tests/cli/cases-v3.jsonl.gz"),
            root,
            pattern: String::new(),
            keep_going: false,
            expected_version: None,
            stdin_file: false,
            evidence_dir: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RunResult {
    pub attempted: usize,
    pub passed: usize,
    pub failed: Vec<String>,
    pub selected_cases: usize,
    pub deferred_cases: usize,
}

pub fn run(options: &Options, output: &mut impl Write) -> Result<RunResult> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        return Err(Error::message("unsupported regression test platform"));
    }
    require_unblocked_signals()?;
    let binary = fs::canonicalize(&options.binary)?;
    if !binary.is_file() {
        return Err(Error::message(format!("{} is required", binary.display())));
    }
    let supervisor = binary.with_file_name("fastmash-sort-supervisor");
    let supervisor_sha256 = if cfg!(target_os = "linux") {
        if !supervisor.is_file() {
            return Err(Error::message(format!(
                "{} requires fastmash-sort-supervisor beside it on Linux",
                binary.display()
            )));
        }
        Some(digest(&fs::read(&supervisor)?))
    } else {
        None
    };
    let binary_sha256 = digest(&fs::read(&binary)?);
    if let Some(directory) = &options.evidence_dir {
        prepare_evidence(directory)?;
    }
    let fixture = Fixture::load(&options.fixture)?;
    let matched: Vec<_> = fixture
        .cases
        .iter()
        .filter(|case| case.id.contains(&options.pattern))
        .collect();
    let cases: Vec<_> = matched
        .iter()
        .copied()
        .filter(|case| case.supported(options.stdin_file))
        .collect();
    let excluded: Vec<_> = matched
        .iter()
        .filter(|case| !case.supported(options.stdin_file))
        .map(|case| json!({"id":case.id, "reason":case.deferred_reason(options.stdin_file)}))
        .collect();
    let evidence = json!({
        "platform": if cfg!(target_os="macos") { "darwin" } else { "linux" },
        "binary": binary, "binary_sha256":binary_sha256,
        "supervisor_sha256":supervisor_sha256,
        "fixture":fixture.path, "fixture_sha256":fixture.fixture_sha256,
        "content_sha256":fixture.content_sha256,
        "provenance":fixture.provenance_path, "provenance_sha256":fixture.provenance_sha256,
        "expected_version":options.expected_version, "stdin_file":options.stdin_file,
        "full_output_calibration":null,
        "selected_cases":cases.len(), "matched_cases":matched.len(),
        "transport_dispositions":excluded,
        "native_dispositions":cases.iter().filter(|case| !case.native_dispositions().is_empty())
            .map(|case| json!({"id":case.id, "reasons":case.native_dispositions()})).collect::<Vec<_>>(),
    });
    if let Some(directory) = &options.evidence_dir {
        write_json(&directory.join("identity.json"), &evidence)?;
        let mut dispositions = File::create(directory.join("dispositions.jsonl"))?;
        for case in matched {
            write_line(
                &mut dispositions,
                &json!({"id":case.id,
                    "disposition":case.disposition(options.stdin_file),
                    "reason":case.deferred_reason(options.stdin_file),
                    "native_dispositions":case.native_dispositions(),
                }),
            )?;
        }
    }
    let directory = Scratch::new()?;
    let launcher = std::env::current_exe()?;
    let mut result = RunResult {
        attempted: 0,
        passed: 0,
        failed: Vec::new(),
        selected_cases: cases.len(),
        deferred_cases: excluded.len(),
    };
    for case in cases {
        let expected = case.expected(&options.root, options.expected_version.as_deref())?;
        result.attempted += 1;
        let observed = invoke(
            case,
            &binary,
            &launcher,
            &directory.path,
            options.stdin_file,
        )?;
        let passed = observed.matches(&expected);
        if let Some(directory) = &options.evidence_dir {
            let mut outcomes = OpenOptions::new()
                .append(true)
                .create(true)
                .open(directory.join("cases.jsonl"))?;
            write_line(&mut outcomes, &json!({"id":case.id, "passed":passed}))?;
            if !passed || !case.native_dispositions().is_empty() {
                let mut observations = OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(directory.join("observations.jsonl"))?;
                write_line(
                    &mut observations,
                    &json!({"id":case.id, "passed":passed,
                    "observed":observed, "expected":expected}),
                )?;
            }
        }
        if passed {
            result.passed += 1;
            continue;
        }
        result.failed.push(case.id.clone());
        writeln!(output, "FAIL {}: {}", case.id, observed.argv[1..].join(" "))?;
        if observed.timed_out {
            writeln!(output, "  execution exceeded 8 seconds")?;
        }
        if observed.overflow {
            writeln!(output, "  output exceeded the {}-byte guard", process::CAP)?;
        }
        let expected = serde_json::to_value(&expected)?;
        let observed = serde_json::to_value(&observed)?;
        for (key, value) in expected
            .as_object()
            .expect("serialized expectation is an object")
        {
            if observed.get(key) != Some(value) {
                writeln!(
                    output,
                    "  {key}: expected {value}, observed {}",
                    observed.get(key).unwrap_or(&serde_json::Value::Null)
                )?;
            }
        }
        if !options.keep_going {
            break;
        }
    }
    directory.remove()?;
    // A run's identity must still describe the executables and frozen inputs
    // used by its final case. A shared build replacing them invalidates evidence.
    if digest(&fs::read(&binary)?) != binary_sha256
        || digest(&fs::read(&fixture.path)?) != fixture.fixture_sha256
        || digest(&fs::read(&fixture.provenance_path)?) != fixture.provenance_sha256
        || supervisor_sha256.is_some_and(|hash| {
            fs::read(&supervisor)
                .map(|bytes| digest(&bytes) != hash)
                .unwrap_or(true)
        })
    {
        return Err(Error::message(
            "executable or fixture identity changed during the run",
        ));
    }
    write!(
        output,
        "{} of {} cases passed",
        result.passed, result.selected_cases
    )?;
    if result.attempted != result.selected_cases {
        write!(output, "; stopped after {}", result.attempted)?;
    }
    writeln!(output)?;
    if result.deferred_cases != 0 {
        writeln!(
            output,
            "{} transport cases deferred in this input mode",
            result.deferred_cases
        )?;
    }
    if let Some(directory) = &options.evidence_dir {
        write_json(&directory.join("result.json"), &result)?;
    }
    Ok(result)
}

fn prepare_evidence(directory: &Path) -> Result<()> {
    if directory.exists() && fs::read_dir(directory)?.next().is_some() {
        return Err(Error::message(format!(
            "{} must be empty so observations retain one run identity",
            directory.display()
        )));
    }
    fs::create_dir_all(directory)?;
    Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = File::create(path)?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    Ok(())
}
fn write_line(output: &mut impl Write, value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(&mut *output, value)?;
    writeln!(output)?;
    Ok(())
}

pub fn main_entry(mut args: impl Iterator<Item = OsString>) -> Result<i32> {
    let first = args.next();
    if first.as_deref() == Some(std::ffi::OsStr::new(process::EXEC_MODE)) {
        process::exec_target(args)?;
        return Err(Error::message("internal exec unexpectedly returned"));
    }
    let mut args = first.into_iter().chain(args);
    let mut options = Options::new(PathBuf::new());
    while let Some(arg) = args.next() {
        let value = |args: &mut dyn Iterator<Item = OsString>| {
            args.next().ok_or_else(|| {
                Error::message(format!("{} requires a value", arg.to_string_lossy()))
            })
        };
        match arg.to_str() {
            Some("--help" | "-h") => {
                println!(
                    "Run frozen Fastmash commands and supported transports.\n\nUsage: fastmash-corpus --binary PATH [OPTIONS]\n\n  --fixture PATH          compressed corpus and sibling provenance\n  --match TEXT            select case IDs containing TEXT\n  --expected-version V    successful version-action expectation\n  --stdin-file            ordinary input through a regular file\n  --keep-going            continue after mismatches\n  --evidence-dir PATH     retain one run in an empty directory\n\nDirectory input and closed output pipes run by default. Remaining transports stay in the full Python corpus run."
                );
                return Ok(0);
            }
            Some("--binary") => options.binary = value(&mut args)?.into(),
            Some("--fixture") => options.fixture = value(&mut args)?.into(),
            Some("--evidence-dir") => options.evidence_dir = Some(value(&mut args)?.into()),
            Some("--match") => {
                options.pattern = value(&mut args)?
                    .into_string()
                    .map_err(|_| Error::message("--match requires UTF-8 text"))?
            }
            Some("--expected-version") => {
                options.expected_version = Some(
                    value(&mut args)?
                        .into_string()
                        .map_err(|_| Error::message("--expected-version requires UTF-8 text"))?,
                )
            }
            Some("--stdin-file") => options.stdin_file = true,
            Some("--keep-going") => options.keep_going = true,
            _ => {
                return Err(Error::message(format!(
                    "unknown argument {}",
                    arg.to_string_lossy()
                )));
            }
        }
    }
    if options.binary.as_os_str().is_empty() {
        return Err(Error::message("--binary is required"));
    }
    let result = run(&options, &mut std::io::stdout().lock())?;
    Ok(i32::from(!result.failed.is_empty()))
}
