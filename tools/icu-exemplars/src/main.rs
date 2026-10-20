#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "Usage: fastmash-icu-exemplars NUMERIC_JSON OUTPUT_JSON";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if args.len() != 2 {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    }
    if !fastmash_icu_exemplars::supported() {
        eprintln!(
            "ICU 78 required, built with ICU {}",
            fastmash_icu_exemplars::ICU_VERSION
        );
        return ExitCode::from(77);
    }
    let result = (|| -> fastmash_icu_exemplars::Result<()> {
        let source: serde_json::Value = serde_json::from_slice(&fs::read(&args[0])?)?;
        let names = source
            .get("locales")
            .and_then(serde_json::Value::as_object)
            .ok_or("numeric reference requires a locales object")?
            .keys()
            .cloned();
        let exemplars = fastmash_icu_exemplars::collect(names)?;
        let mut raw = serde_json::to_vec_pretty(&exemplars)?;
        raw.push(b'\n');
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&args[1])?
            .write_all(&raw)?;
        println!(
            "{} locales from ICU {}",
            exemplars.locales.len(),
            fastmash_icu_exemplars::ICU_VERSION
        );
        Ok(())
    })();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fastmash-icu-exemplars: {error}");
            if error
                .downcast_ref::<fastmash_icu_exemplars::Refusal>()
                .is_some()
            {
                ExitCode::from(77)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
