#![forbid(unsafe_code)]

use fastmash_locale_tools::{Result, collation::Table};
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "Usage: fastmash-generate-collation-table [--variant N] [--] INPUT_JSON OUTPUT_RS";

struct Config {
    input: PathBuf,
    output: PathBuf,
    variant: Option<i64>,
}

fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Option<Config>> {
    let mut args = args.into_iter();
    let mut paths = Vec::new();
    let mut variant = None;
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
            if arg == "--variant" {
                if variant.is_some() {
                    return Err("--variant may be supplied only once".into());
                }
                let value = args
                    .next()
                    .ok_or("--variant requires a signed 64-bit integer")?;
                variant = Some(
                    value
                        .to_str()
                        .ok_or("--variant requires a signed 64-bit integer")?
                        .parse::<i64>()
                        .map_err(|_| "--variant requires a signed 64-bit integer")?,
                );
                continue;
            }
            if arg.as_encoded_bytes().starts_with(b"-") {
                return Err("unknown option".into());
            }
        }
        paths.push(PathBuf::from(arg));
    }
    let [input, output]: [PathBuf; 2] = paths
        .try_into()
        .map_err(|_| "expected INPUT_JSON OUTPUT_RS")?;
    Ok(Some(Config {
        input,
        output,
        variant,
    }))
}

fn generate(config: &Config) -> Result<()> {
    let raw = fs::read(&config.input).map_err(|error| format!("cannot read input: {error}"))?;
    let table = match config.variant {
        Some(variant) => Table::from_candidates(&raw, variant)?,
        None => Table::from_verification(&raw)?,
    };
    let source = table.rust_source();
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&config.output)
        .map_err(|error| format!("cannot create output: {error}"))?;
    output
        .write_all(source.as_bytes())
        .map_err(|error| format!("cannot write output: {error}"))?;

    let mut stdout = std::io::stdout().lock();
    write!(stdout, "{} locales in ", table.rows().len())?;
    // Preserve the supplied path's OS encoding, including Unix non-UTF-8 bytes.
    stdout.write_all(config.output.as_os_str().as_encoded_bytes())?;
    stdout.write_all(b"\n")?;
    Ok(())
}

fn main() -> ExitCode {
    let config = match parse(std::env::args_os().skip(1)) {
        Ok(Some(config)) => config,
        Ok(None) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            eprintln!("fastmash-generate-collation-table: {error}\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match generate(&config) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fastmash-generate-collation-table: {error}");
            ExitCode::FAILURE
        }
    }
}
