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

    const USAGE: &str = "Usage: fastmash-generate-glibc-collation --evidence PATH [--locale NAME ...] [--] GNU_SORT LOCALE_TOOL LOCPATH COMMON OUTPUT_JSON OUTPUT_RUST\n\nGenerates candidates for the sorting locales compiled into this tool. --locale limits qualification to named sorting locales. Output and capture paths must be new.";
    let mut args = std::env::args_os().skip(1);
    let mut evidence = None;
    let mut selected = BTreeSet::new();
    let mut paths = Vec::new();
    let mut positional = false;
    while let Some(arg) = args.next() {
        if !positional {
            if arg == "--help" || arg == "-h" {
                println!("{USAGE}");
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
                return Err(format!("unknown option\n{USAGE}").into());
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
        if output.try_exists()? {
            return Err(format!("output already exists: {}", output.display()).into());
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
    let source = fs::read(&common_path)?;
    let common = Common::parse(std::str::from_utf8(&source)?)?;
    let source_name = common_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("common source name must be UTF-8")?;
    let decomposed = decompositions();
    let letters = letters();
    fs::DirBuilder::new().mode(0o700).create(&evidence)?;
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
        .open(output_json)?
        .write_all(&artifacts.json)?;
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output_rust)?
        .write_all(artifacts.rust.as_bytes())?;
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
