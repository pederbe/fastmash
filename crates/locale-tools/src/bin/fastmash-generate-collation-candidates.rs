#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> fastmash_locale_tools::Result<()> {
    use fastmash_locale_tools::collation_candidates::{Exemplars, NumericLocales};
    use serde::Serialize;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::PathBuf;

    const USAGE: &str = "Usage: fastmash-generate-collation-candidates --evidence PATH [--] NUMERIC_JSON EXEMPLARS_JSON GNU_SORT LOCPATH OUTPUT_JSON";
    let mut args = std::env::args_os().skip(1);
    let mut evidence = None;
    let mut paths = Vec::new();
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional {
            if arg == "--help" || arg == "-h" {
                println!(
                    "Generate collation candidates from numeric locales, ICU exemplars and GNU sort probes.\n\n{USAGE}\n\n\
EXEMPLARS_JSON must identify ICU 78 and the same locales as NUMERIC_JSON. GNU_SORT\n\
and LOCPATH must belong to the identified reference environment. OUTPUT_JSON and\n\
the evidence directory must be new. Review the candidate JSON before changing tables.\n\
The summary's missing-ICU count refers to locales without ICU alphabet data.\n\n\
Example, after generating /tmp/fastmash-icu-exemplars.json with the ICU tool:\n\
  fastmash-generate-collation-candidates --evidence /tmp/fastmash-candidate-evidence -- \\\n    data/locales/glibc-lc-numeric.json /tmp/fastmash-icu-exemplars.json \\\n    /path/to/gnu-sort /path/to/compiled-locales /tmp/fastmash-candidates.json\n\n\
Guide: https://fastmash.io/contributing/testing.html#locale-tools"
                );
                return Ok(());
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
            if arg.as_encoded_bytes().starts_with(b"-") {
                return Err(format!("unknown option: {}\n{USAGE}", arg.to_string_lossy()).into());
            }
        }
        paths.push(PathBuf::from(arg));
    }
    let [numeric, exemplars, sort, locpath, output]: [PathBuf; 5] = paths
        .try_into()
        .map_err(|_| format!("expected five paths\n{USAGE}"))?;
    let evidence = evidence.ok_or("--evidence requires a path")?;
    if output
        .try_exists()
        .map_err(|error| format!("cannot inspect output {}: {error}", output.display()))?
    {
        return Err(format!(
            "output already exists: {}. Choose a new output path",
            output.display()
        )
        .into());
    }
    let numeric = NumericLocales::parse(&fs::read(&numeric).map_err(|error| {
        format!(
            "cannot read numeric reference {}: {error}",
            numeric.display()
        )
    })?)?;
    let exemplars = Exemplars::parse(
        &fs::read(&exemplars).map_err(|error| {
            format!("cannot read ICU exemplars {}: {error}", exemplars.display())
        })?,
        &numeric,
    )?;
    let candidates = fastmash_locale_tools::candidate_executor::generate(
        numeric, exemplars, &sort, &locpath, &evidence,
    )?;
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut raw = Vec::new();
    candidates.serialize(&mut serde_json::Serializer::with_formatter(
        &mut raw, formatter,
    ))?;
    raw.push(b'\n');
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(|error| {
            format!(
                "cannot create candidate report {}: {error}",
                output.display()
            )
        })?
        .write_all(&raw)
        .map_err(|error| {
            format!(
                "cannot write candidate report {}: {error} (evidence: {})",
                output.display(),
                evidence.display()
            )
        })?;
    let upper = candidates
        .locales
        .values()
        .filter(|entry| entry.tags[0].ends_with("upper"))
        .count();
    let absent = candidates
        .locales
        .values()
        .filter(|entry| entry.letters.alphabet.is_none())
        .count();
    println!(
        "{} candidates, {upper} uppercase first, {absent} without ICU data",
        candidates.locales.len()
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fastmash-generate-collation-candidates: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("fastmash-generate-collation-candidates: native reference probes require Unix");
        std::process::ExitCode::from(77)
    }
}
