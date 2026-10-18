#![forbid(unsafe_code)]

#[cfg(unix)]
mod cli {
    use fastmash_locale_tools::Result;
    use fastmash_locale_tools::collation::Table;
    use fastmash_locale_tools::punctuation_executor::{self, Tools};
    use std::ffi::OsString;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::process::ExitCode;

    const USAGE: &str = "Usage: fastmash-compare-collation-punctuation --evidence PATH [--locale NAME]... [--] VERIFICATION_JSON CANDIDATE REFERENCE SORT LOCPATH OUTPUT_JSON";

    struct Config {
        verification: PathBuf,
        tools: Tools,
        output: PathBuf,
        evidence: PathBuf,
        locales: Vec<String>,
    }

    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>> {
        let mut args = args.into_iter();
        let mut paths = Vec::new();
        let mut evidence = None;
        let mut locales = Vec::new();
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
                if arg == "--evidence" {
                    if evidence.is_some() {
                        return Err("--evidence may be supplied only once".into());
                    }
                    evidence = Some(PathBuf::from(
                        args.next().ok_or("--evidence requires a path")?,
                    ));
                    continue;
                }
                if arg == "--locale" {
                    locales.push(
                        args.next()
                            .ok_or("--locale requires a name")?
                            .into_string()
                            .map_err(|_| "--locale requires a UTF-8 name")?,
                    );
                    continue;
                }
                if arg.as_encoded_bytes().starts_with(b"-") {
                    return Err("unknown option".into());
                }
            }
            paths.push(PathBuf::from(arg));
        }
        let [verification, candidate, reference, sort, locpath, output]: [PathBuf; 6] =
            paths.try_into().map_err(
                |_| "expected VERIFICATION_JSON CANDIDATE REFERENCE SORT LOCPATH OUTPUT_JSON",
            )?;
        Ok(Some(Config {
            verification,
            tools: Tools {
                candidate,
                reference,
                sort,
                locpath,
            },
            output,
            evidence: evidence.ok_or("--evidence requires a new directory path")?,
            locales,
        }))
    }

    fn compare(config: &Config) -> Result<()> {
        let raw = fs::read(&config.verification)
            .map_err(|error| format!("cannot read verification input: {error}"))?;
        let table = Table::from_verification(&raw)?;
        match fs::symlink_metadata(&config.output) {
            Ok(_) => return Err("output already exists".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect output: {error}").into()),
        }
        let report = punctuation_executor::execute(
            &table,
            &config.locales,
            &config.tools,
            &config.evidence,
        )?;
        let raw = report.json()?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&config.output)
            .map_err(|error| {
                format!(
                    "cannot create report: {error} (comparison evidence: {})",
                    config.evidence.display()
                )
            })?;
        output.write_all(&raw).map_err(|error| {
            format!(
                "cannot write report: {error} (comparison evidence: {})",
                config.evidence.display()
            )
        })?;
        writeln!(std::io::stdout().lock(), "{}", report.summary_line())?;
        Ok(())
    }

    pub(super) fn main() -> ExitCode {
        let config = match parse(std::env::args_os().skip(1)) {
            Ok(Some(config)) => config,
            Ok(None) => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            Err(error) => {
                eprintln!("fastmash-compare-collation-punctuation: {error}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        };
        match compare(&config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("fastmash-compare-collation-punctuation: {error}");
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
    eprintln!("fastmash-compare-collation-punctuation: punctuation comparisons require Unix");
    std::process::ExitCode::FAILURE
}
