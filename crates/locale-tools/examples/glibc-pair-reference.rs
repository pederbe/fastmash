//! Verify retained independent contraction pairs through a selected GNU sort.
#![forbid(unsafe_code)]

#[cfg(unix)]
fn run() -> fastmash_locale_tools::Result<()> {
    use fastmash_locale_tools::glibc_collation::{Pair, Reference};
    use std::cmp::Ordering;
    use std::fs;
    use std::os::unix::fs::DirBuilderExt;
    use std::path::PathBuf;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 6 {
        return Err(
            "usage: glibc-pair-reference GNU_SORT LOCALE_TOOL LOCPATH CAPTURES RETAINED_JSON LOCALE...".into(),
        );
    }
    let source: serde_json::Value = serde_json::from_slice(&fs::read(&args[4])?)?;
    let contractions = source["contractions"]
        .as_object()
        .ok_or("reference has no contractions object")?;
    let scalar = |raw: &str| -> fastmash_locale_tools::Result<char> {
        char::from_u32(u32::from_str_radix(raw, 16)?)
            .ok_or_else(|| "invalid reference scalar".into())
    };
    let mut pairs = Vec::new();
    for (sequence, letter) in contractions {
        let left: String = sequence
            .split_ascii_whitespace()
            .map(scalar)
            .collect::<fastmash_locale_tools::Result<_>>()?;
        let right = scalar(letter.as_str().ok_or("reference letter is not hex text")?)?.to_string();
        pairs.push(Pair {
            left: left.clone(),
            right: right.clone(),
        });
        pairs.push(Pair {
            left: format!("a{left}b"),
            right: format!("a{right}b"),
        });
    }
    let captures = PathBuf::from(&args[3]);
    fs::DirBuilder::new().mode(0o700).create(&captures)?;
    for (index, name) in args[5..].iter().enumerate() {
        let name = name.to_str().ok_or("locale must be UTF-8")?;
        let mut reference = Reference::open(
            &PathBuf::from(&args[0]),
            &PathBuf::from(&args[1]),
            &PathBuf::from(&args[2]),
            &format!("{name}.UTF-8"),
            &captures.join(format!("locale-{index:04}")),
        )?;
        let actual = reference.compare(&pairs)?;
        if actual.iter().any(|order| *order != Ordering::Equal) {
            return Err(format!(
                "{name}: a contraction differs from the retained independent reference"
            )
            .into());
        }
        println!("{name}: {} exact contraction ties", actual.len());
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
