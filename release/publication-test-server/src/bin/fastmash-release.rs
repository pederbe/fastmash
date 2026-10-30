//! Weekly release versioning and changelog metadata.
#![forbid(unsafe_code)]
#[path = "../weekly.rs"]
mod weekly;
mod common {
    use std::{
        fs,
        path::{Path, PathBuf},
        process::Command,
    };
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
    pub fn repository() -> Result<PathBuf> {
        let output = Command::new("git")
            .args(["rev-parse", "--show-toplevel"])
            .output()?;
        if !output.status.success() {
            return Err("not in a repository".into());
        }
        Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
    }
    pub fn crate_version(path: &Path) -> Result<String> {
        let text = fs::read_to_string(path)?;
        Ok(text
            .lines()
            .find_map(|line| {
                line.strip_prefix("version = \"")
                    .map(|s| s.trim_end_matches('"').to_string())
            })
            .ok_or("missing package version")?)
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((command, args)) => match command.as_str() {
            "prepare-weekly" => weekly::prepare(args),
            "release-notes" => weekly::notes(args),
            _ => Err("unknown release command".into()),
        },
        None => Err("expected a release command".into()),
    };
    if let Err(error) = result {
        eprintln!("fastmash-release: {error}");
        std::process::exit(1);
    }
}

#[cfg(all(test, unix))]
#[path = "../crates_publish_tests.rs"]
mod crates_publish_tests;
