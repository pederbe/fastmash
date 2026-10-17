#![forbid(unsafe_code)]

#[cfg(unix)]
mod cli {
    use fastmash_locale_tools::{Result, comparison::Plan, executor};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::time::{SystemTime, UNIX_EPOCH};

    const USAGE: &str = "Usage: fastmash-compare-numeric-locales [--all] [--evidence PATH] [--] CANDIDATE REFERENCE LOCPATH TABLE_JSON";

    struct Config {
        candidate: PathBuf,
        reference: PathBuf,
        locpath: PathBuf,
        table: PathBuf,
        evidence: Option<PathBuf>,
        all: bool,
    }

    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>> {
        let mut args = args.into_iter();
        let mut paths = Vec::new();
        let mut all = false;
        let mut evidence = None;
        let mut positional = false;
        while let Some(arg) = args.next() {
            if !positional {
                if arg == "--help" || arg == "-h" {
                    return Ok(None);
                }
                if arg == "--" {
                    positional = true;
                    continue;
                }
                if arg == "--all" {
                    all = true;
                    continue;
                }
                if arg == "--evidence" {
                    if evidence.is_some() {
                        return Err("--evidence may be supplied only once".into());
                    }
                    evidence = Some(PathBuf::from(
                        args.next().ok_or("--evidence requires a path")?,
                    ));
                    continue;
                }
                if arg.as_bytes().starts_with(b"-") {
                    return Err(format!("unknown option: {}", arg.to_string_lossy()).into());
                }
            }
            paths.push(PathBuf::from(arg));
        }
        let [candidate, reference, locpath, table]: [PathBuf; 4] = paths
            .try_into()
            .map_err(|_| "expected CANDIDATE REFERENCE LOCPATH TABLE_JSON")?;
        Ok(Some(Config {
            candidate,
            reference,
            locpath,
            table,
            evidence,
            all,
        }))
    }

    fn compare(config: Config) -> Result<bool> {
        let data = fs::read(&config.table)
            .map_err(|error| format!("cannot read {}: {error}", config.table.display()))?;
        let plan = Plan::from_json(&data, config.all)?;
        let evidence = match config.evidence {
            Some(path) => path,
            None => {
                let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
                std::env::temp_dir().join(format!(
                    "fastmash-locale-comparison-{}-{stamp}",
                    std::process::id()
                ))
            }
        };
        // execute creates this new private directory and retains its raw receipts.
        eprintln!("Comparison evidence: {}", evidence.display());
        match executor::execute(
            &plan,
            &config.candidate,
            &config.reference,
            config.locpath.as_os_str(),
            &evidence,
        ) {
            Ok(summary) => {
                for diagnostic in &summary.diagnostics {
                    println!("{diagnostic}");
                }
                println!(
                    "{} locales, {} cases, {} mismatches",
                    plan.locales.len(),
                    summary.completed_pairs,
                    summary.mismatches
                );
                Ok(summary.mismatches == 0)
            }
            Err(failure) => {
                for diagnostic in &failure.summary.diagnostics {
                    println!("{diagnostic}");
                }
                eprintln!(
                    "fastmash-compare-numeric-locales: stopped after {} completed pairs: {} (evidence: {})",
                    failure.summary.completed_pairs,
                    failure.message,
                    failure.evidence.display()
                );
                Ok(false)
            }
        }
    }

    pub(super) fn main() -> ExitCode {
        let config = match parse(std::env::args_os().skip(1)) {
            Ok(Some(config)) => config,
            Ok(None) => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("fastmash-compare-numeric-locales: {error}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        };
        match compare(config) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::FAILURE,
            Err(error) => {
                eprintln!("fastmash-compare-numeric-locales: {error}");
                ExitCode::FAILURE
            }
        }
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    cli::main()
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("fastmash-compare-numeric-locales: numeric locale comparisons require Unix");
    std::process::ExitCode::FAILURE
}
