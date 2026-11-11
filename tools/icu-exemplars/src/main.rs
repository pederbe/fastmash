#![forbid(unsafe_code)]

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::process::ExitCode;

const USAGE: &str = "Usage: fastmash-icu-exemplars NUMERIC_JSON OUTPUT_JSON";

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!(
            "Read installed ICU 78 exemplar sets for the locales in a numeric reference JSON file.\n\n{USAGE}\n\n\
OUTPUT_JSON must be new. The JSON records standard and auxiliary exemplar sets\n\
and null when ICU falls back to a different language. Status 77 means the compiled\n\
ICU version or reference data is unsupported. Invalid input and file errors exit 1.\n\n\
Example, from the repository root with a new output path:\n\
  fastmash-icu-exemplars data/locales/glibc-lc-numeric.json /tmp/fastmash-icu-exemplars.json\n\n\
Build and reference setup: tools/icu-exemplars/README.md"
        );
        return ExitCode::SUCCESS;
    }
    if args.len() != 2 {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    }
    if !fastmash_icu_exemplars::supported() {
        eprintln!(
            "ICU 78 required, built with ICU {}. Build this reference tool with ICU 78 development files",
            fastmash_icu_exemplars::ICU_VERSION
        );
        return ExitCode::from(77);
    }
    let result = (|| -> fastmash_icu_exemplars::Result<()> {
        let input = std::path::Path::new(&args[0]);
        let output = std::path::Path::new(&args[1]);
        let source: serde_json::Value =
            serde_json::from_slice(&fs::read(input).map_err(|error| {
                format!("cannot read numeric reference {}: {error}", input.display())
            })?)?;
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
            .open(output)
            .map_err(|error| format!("cannot create new exemplar output {}: {error}. Choose a new output path if it already exists", output.display()))?
            .write_all(&raw)
            .map_err(|error| format!("cannot write exemplar output {}: {error}", output.display()))?;
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
