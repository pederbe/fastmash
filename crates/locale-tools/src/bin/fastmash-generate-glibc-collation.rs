#![forbid(unsafe_code)]

#[cfg(unix)]
#[path = "../../../cli/src/collation_locales.rs"]
mod sorting;

#[cfg(unix)]
fn run() -> fastmash_locale_tools::Result<()> {
    use fastmash_locale_tools::collation::Row;
    use fastmash_locale_tools::glibc_collation::Reference;
    use fastmash_locale_tools::glibc_generation::{Common, decompositions, letters, probe};
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::unix::fs::DirBuilderExt;
    use std::path::PathBuf;

    const USAGE: &str = "Usage: fastmash-generate-glibc-collation --evidence PATH [--locale NAME]... [--] GNU_SORT LOCALE_TOOL LOCPATH COMMON OUTPUT_JSON OUTPUT_RUST";
    let mut args = std::env::args_os().skip(1);
    let mut evidence = None;
    let mut selected = BTreeSet::new();
    let mut paths = Vec::new();
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional {
            if arg == "--help" || arg == "-h" {
                println!(
                    "Generate JSON and Rust candidates for glibc collation character handling.\n\n{USAGE}\n\n\
COMMON is the reference's iso14651_t1_common source. GNU_SORT, LOCALE_TOOL and\n\
LOCPATH must match that reference. Both output files and the evidence directory\n\
must be new. By default, probe every sorting locale compiled into this tool.\n\
Repeat --locale NAME to select a subset. Keep subset output separate from a full table.\n\
The report's fourth level is a collation comparison level. Review all generated\n\
candidates before changing product tables. Raw probe captures remain in the evidence directory.\n\n\
Example, with paths to your reference tools and source:\n\
  fastmash-generate-glibc-collation --evidence /tmp/fastmash-glibc-captures \\\n    --locale en_US --locale de_DE -- /path/to/gnu-sort /path/to/reference/bin/locale \\\n    /path/to/compiled-locales /path/to/reference/iso14651_t1_common \\\n    /tmp/fastmash-glibc.json /tmp/fastmash-glibc.rs\n\n\
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
            if arg == "--locale" {
                let name = args.next().ok_or("--locale requires a name")?;
                let name = name
                    .into_string()
                    .map_err(|_| "locale name must be UTF-8")?;
                if !selected.insert(name) {
                    return Err("duplicate --locale name".into());
                }
                continue;
            }
            if arg.as_encoded_bytes().starts_with(b"-") {
                return Err(format!("unknown option: {}\n{USAGE}", arg.to_string_lossy()).into());
            }
        }
        paths.push(PathBuf::from(arg));
    }
    let [
        sort,
        locale_tool,
        locpath,
        common_path,
        output_json,
        output_rust,
    ]: [PathBuf; 6] = paths
        .try_into()
        .map_err(|_| format!("expected six paths\n{USAGE}"))?;
    let evidence = evidence.ok_or("--evidence requires a path")?;
    for output in [&output_json, &output_rust] {
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
    }
    if output_json == output_rust {
        return Err("JSON and Rust output paths must differ".into());
    }
    let all: Vec<_> = sorting::COLLATION
        .iter()
        .map(|(name, tag)| {
            Ok(Row {
                name: std::str::from_utf8(name)?.into(),
                tag: (*tag).into(),
            })
        })
        .collect::<fastmash_locale_tools::Result<_>>()?;
    if let Some(name) = selected
        .iter()
        .find(|name| !all.iter().any(|row| &row.name == *name))
    {
        return Err(format!("{name:?} is not a sorting locale").into());
    }
    let rows: Vec<_> = all
        .into_iter()
        .filter(|row| selected.is_empty() || selected.contains(&row.name))
        .collect();
    let source = fs::read(&common_path).map_err(|error| {
        format!(
            "cannot read common collation source {}: {error}",
            common_path.display()
        )
    })?;
    let common = Common::parse(std::str::from_utf8(&source)?)?;
    let source_name = common_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("common source name must be UTF-8")?;
    let decomposed = decompositions();
    let letters = letters();
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&evidence)
        .map_err(|error| {
            format!(
                "cannot create new probe evidence directory {}: {error}",
                evidence.display()
            )
        })?;
    // Keep the exact generation inputs with the subprocess captures.
    fs::write(evidence.join("common"), &source)?;
    fs::write(
        evidence.join("sorting-locales.json"),
        serde_json::to_vec_pretty(
            &rows
                .iter()
                .map(|row| (&row.name, &row.tag))
                .collect::<Vec<_>>(),
        )?,
    )?;
    let mut observations = BTreeMap::new();
    for (index, row) in rows.iter().enumerate() {
        let mut reference = Reference::open(
            &sort,
            &locale_tool,
            &locpath,
            &format!("{}.UTF-8", row.name),
            &evidence.join(format!("locale-{index:04}")),
        )?;
        let observed = probe(&common, &letters, &decomposed, |_, pairs| {
            reference.compare(pairs)
        })?;
        fs::write(
            evidence.join(format!("observed-{index:04}.json")),
            serde_json::to_vec_pretty(&observed)?,
        )?;
        eprintln!(
            "{}: {} weighed lowest, {} weighed otherwise, {} letters without a fourth level",
            row.name,
            observed.weighed.len(),
            observed.other_weighed.len(),
            observed.no_fourth.len()
        );
        observations.insert(row.name.clone(), observed);
    }
    let artifacts = fastmash_locale_tools::glibc_artifacts::emit(
        source_name,
        &source,
        &common,
        &decomposed,
        &rows,
        &observations,
    )?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_json)
        .map_err(|error| {
            format!(
                "cannot create JSON output {}: {error} (evidence: {})",
                output_json.display(),
                evidence.display()
            )
        })?
        .write_all(&artifacts.json)
        .map_err(|error| {
            format!(
                "cannot write JSON output {}: {error} (evidence: {})",
                output_json.display(),
                evidence.display()
            )
        })?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_rust)
        .map_err(|error| {
            format!(
                "cannot create Rust output {}: {error} (evidence: {})",
                output_rust.display(),
                evidence.display()
            )
        })?
        .write_all(artifacts.rust.as_bytes())
        .map_err(|error| {
            format!(
                "cannot write Rust output {}: {error} (evidence: {})",
                output_rust.display(),
                evidence.display()
            )
        })?;
    println!("{}", artifacts.summary);
    Ok(())
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fastmash-generate-glibc-collation: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("fastmash-generate-glibc-collation: native reference probes require Unix");
        std::process::ExitCode::from(77)
    }
}
