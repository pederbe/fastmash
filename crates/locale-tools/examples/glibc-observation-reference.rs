//! Compare complete native observations with the retained independent data.
#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> fastmash_locale_tools::Result<()> {
    use fastmash_locale_tools::glibc_collation::Reference;
    use fastmash_locale_tools::glibc_generation::{Common, decompositions, letters, probe};
    use std::collections::BTreeSet;
    use std::fs;
    use std::os::unix::fs::DirBuilderExt;
    use std::path::PathBuf;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 7 {
        return Err("usage: glibc-observation-reference GNU_SORT LOCALE_TOOL LOCPATH COMMON CAPTURES RETAINED_JSON LOCALE...".into());
    }
    let common = Common::parse(&fs::read_to_string(&args[3])?)?;
    let source: serde_json::Value = serde_json::from_slice(&fs::read(&args[5])?)?;
    if source["unicode_decompositions"] != "16.0.0" {
        return Err("retained reference does not identify Unicode 16".into());
    }
    if source["ignorable"].as_u64() != Some(common.ignorable.len() as u64) {
        return Err("ignorable input count differs from retained source".into());
    }
    let contractions: serde_json::Map<_, _> = common
        .contractions
        .iter()
        .map(|(parts, letter)| {
            (
                parts
                    .chars()
                    .map(|ch| format!("{:04X}", ch as u32))
                    .collect::<Vec<_>>()
                    .join(" "),
                serde_json::Value::String(format!("{:04X}", *letter as u32)),
            )
        })
        .collect();
    if serde_json::Value::Object(contractions) != source["contractions"] {
        return Err("contraction inputs differ from retained reference".into());
    }
    let expand = |value: &serde_json::Value| -> fastmash_locale_tools::Result<BTreeSet<u32>> {
        let mut points = BTreeSet::new();
        for range in value
            .as_array()
            .ok_or("reference ranges are not an array")?
        {
            let start = u32::try_from(range[0].as_u64().ok_or("invalid reference range")?)?;
            let end = u32::try_from(range[1].as_u64().ok_or("invalid reference range")?)?;
            if start > end || end > 0x10FFFF {
                return Err("invalid reference range".into());
            }
            points.extend(start..=end);
        }
        Ok(points)
    };
    let everywhere = expand(&source["no_fourth_everywhere"])?;
    let letters = letters();
    let decompositions = decompositions();
    let root = PathBuf::from(&args[4]);
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    for (index, name) in args[6..].iter().enumerate() {
        let name = name.to_str().ok_or("locale name must be UTF-8")?;
        let expected = source["locales"]
            .get(name)
            .ok_or("locale is not in the retained reference")?;
        let mut reference = Reference::open(
            &PathBuf::from(&args[0]),
            &PathBuf::from(&args[1]),
            &PathBuf::from(&args[2]),
            &format!("{name}.UTF-8"),
            &root.join(format!("locale-{index:04}")),
        )?;
        let observed = probe(&common, &letters, &decompositions, |_, pairs| {
            reference.compare(pairs)
        })?;
        fs::write(
            root.join(format!("observed-{index:04}.json")),
            serde_json::to_vec_pretty(&observed)?,
        )?;
        let mut no_fourth = expand(&expected["no_fourth"])?;
        no_fourth.extend(&everywhere);
        let reordered: Vec<_> = observed
            .reordered
            .keys()
            .map(|point| format!("{point:04X}"))
            .collect();
        if serde_json::to_value(&observed.weighed)? != expected["weighed"]
            || serde_json::to_value(&observed.other_weighed)? != expected["other_weighed"]
            || observed.no_fourth.iter().copied().collect::<BTreeSet<_>>() != no_fourth
            || serde_json::to_value(reordered)? != source["reordered_everywhere"]
        {
            return Err(
                format!("{name}: observations differ from retained independent reference").into(),
            );
        }
        println!("{name}: complete native observations match");
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    std::process::ExitCode::from(77)
}
