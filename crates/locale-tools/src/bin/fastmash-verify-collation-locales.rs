#![forbid(unsafe_code)]

#[cfg(unix)]
mod cli {
    use fastmash_locale_tools::Result;
    use fastmash_locale_tools::verification_executor::{self, Tools};
    use std::ffi::OsString;
    use std::fs::{self, OpenOptions};
    use std::io::{self, Write};
    use std::path::PathBuf;
    use std::process::ExitCode;

    const USAGE: &str = "Usage: fastmash-verify-collation-locales --variant N --evidence PATH [--prior PATH] [--locale NAME]... [--] CANDIDATES_JSON CANDIDATE REFERENCE SORT LOCPATH OUTPUT_JSON\nThe variant labels the report and does not configure the supplied candidate binary.";

    struct Config {
        candidates: PathBuf,
        prior: Option<PathBuf>,
        variant: i64,
        tools: Tools,
        output: PathBuf,
        evidence: PathBuf,
        locales: Vec<String>,
    }

    fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>> {
        let mut args = args.into_iter();
        let mut paths = Vec::new();
        let mut evidence = None;
        let mut prior = None;
        let mut variant = None;
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
                if arg == "--evidence" || arg == "--prior" {
                    let field = if arg == "--evidence" {
                        &mut evidence
                    } else {
                        &mut prior
                    };
                    if field.is_some() {
                        return Err(
                            format!("{} may be supplied only once", arg.to_string_lossy()).into(),
                        );
                    }
                    *field = Some(PathBuf::from(args.next().ok_or_else(|| {
                        format!("{} requires a path", arg.to_string_lossy())
                    })?));
                    continue;
                }
                if arg == "--variant" {
                    if variant.is_some() {
                        return Err("--variant may be supplied only once".into());
                    }
                    variant = Some(
                        args.next()
                            .ok_or("--variant requires an integer")?
                            .into_string()
                            .map_err(|_| "--variant requires a UTF-8 integer")?
                            .parse::<i64>()
                            .map_err(|_| "--variant requires a signed integer")?,
                    );
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
        let [candidates, candidate, reference, sort, locpath, output]: [PathBuf; 6] = paths
            .try_into()
            .map_err(|_| "expected CANDIDATES_JSON CANDIDATE REFERENCE SORT LOCPATH OUTPUT_JSON")?;
        Ok(Some(Config {
            candidates,
            prior,
            variant: variant.ok_or("--variant requires a signed integer")?,
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

    fn verify(config: &Config) -> Result<()> {
        let candidates = fs::read(&config.candidates)
            .map_err(|error| format!("cannot read candidates input: {error}"))?;
        let prior = config
            .prior
            .as_ref()
            .map(|path| {
                fs::read(path).map_err(|error| format!("cannot read prior verification: {error}"))
            })
            .transpose()?;
        match fs::symlink_metadata(&config.output) {
            Ok(_) => return Err("output already exists".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("cannot inspect output: {error}").into()),
        }
        let report = verification_executor::execute(
            &candidates,
            prior.as_deref(),
            config.variant,
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
                    "cannot create report: {error} (verification evidence: {})",
                    config.evidence.display()
                )
            })?;
        output.write_all(&raw).map_err(|error| {
            format!(
                "cannot write report: {error} (verification evidence: {})",
                config.evidence.display()
            )
        })?;
        std::io::stdout()
            .lock()
            .write_all(report.console_output().as_bytes())?;
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
                eprintln!("fastmash-verify-collation-locales: {error}\n{USAGE}");
                return ExitCode::FAILURE;
            }
        };
        match verify(&config) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("fastmash-verify-collation-locales: {error}");
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
    eprintln!("fastmash-verify-collation-locales: verification requires Unix");
    std::process::ExitCode::FAILURE
}
