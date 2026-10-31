//! Archive notices from Cargo's target-filtered normal dependency graph.
#![forbid(unsafe_code)]

use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fs,
    io::Write,
    path::Path,
};

pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

const PREFIXES: [&str; 7] = [
    "license",
    "licence",
    "copying",
    "copyright",
    "notice",
    "unicode",
    "authors",
];
const MIT: &str = include_str!("mit.txt");
// Standard dedication verified against https://unlicense.org/ and
// https://spdx.org/licenses/Unlicense.html.
const UNLICENSE: &str = include_str!("unlicense.txt");
const RUST_STD: &str = "The Rust standard library (std, core, alloc, and their dependencies) is\n\
linked into the binaries under the MIT and Apache-2.0 licenses:\n\
<https://github.com/rust-lang/rust/blob/master/COPYRIGHT>.\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Linux,
    Macos,
}

impl Target {
    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "x86_64-unknown-linux-gnu" => Ok(Self::Linux),
            "aarch64-apple-darwin" => Ok(Self::Macos),
            _ => Err(format!("unsupported target: {value}").into()),
        }
    }

    pub const fn triple(self) -> &'static str {
        match self {
            Self::Linux => "x86_64-unknown-linux-gnu",
            Self::Macos => "aarch64-apple-darwin",
        }
    }
}

fn text<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value[field]
        .as_str()
        .ok_or_else(|| format!("Cargo metadata lacks string {field}").into())
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a [Value]> {
    value[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("Cargo metadata lacks array {field}").into())
}

fn linked_packages(metadata: &Value) -> Result<Vec<&Value>> {
    let mut packages = BTreeMap::new();
    for package in array(metadata, "packages")? {
        packages.insert(text(package, "id")?, package);
    }
    let mut nodes = BTreeMap::new();
    for node in array(&metadata["resolve"], "nodes")? {
        nodes.insert(text(node, "id")?, node);
    }
    let root = packages
        .values()
        .find(|package| package["name"] == "fastmash" && package["source"].is_null())
        .ok_or("Cargo metadata has no local fastmash package")?;
    let mut pending = vec![text(root, "id")?];
    let mut linked = BTreeSet::new();
    while let Some(current) = pending.pop() {
        if !linked.insert(current) {
            continue;
        }
        let node = nodes.get(current).ok_or("missing dependency graph node")?;
        for dependency in array(node, "deps")? {
            if array(dependency, "dep_kinds")?
                .iter()
                .any(|kind| kind["kind"].is_null())
            {
                pending.push(text(dependency, "pkg")?);
            }
        }
    }
    let mut result = Vec::new();
    for id in linked {
        let package = *packages.get(id).ok_or("missing dependency package")?;
        if !package["source"].is_null() {
            result.push(package);
        }
    }
    result.sort_by(|left, right| {
        (left["name"].as_str(), left["version"].as_str())
            .cmp(&(right["name"].as_str(), right["version"].as_str()))
    });
    Ok(result)
}

/// Writes stored notices and the two supported missing-file fallbacks.
/// `metadata` must already have been filtered for `target` by Cargo.
pub fn write_notices(metadata: &Value, target: Target, out: &mut impl Write) -> Result<()> {
    write!(
        out,
        "# Third-party licenses\n\nThe fastmash executable includes the following third-party code.\n\n{RUST_STD}\n"
    )?;
    write!(out, "Target: `{}`.\n\n", target.triple())?;
    for package in linked_packages(metadata)? {
        let name = text(package, "name")?;
        let license = package["license"].as_str().unwrap_or("None");
        write!(
            out,
            "## {name} {}\n\nLicense: {license}\n\n",
            text(package, "version")?
        )?;
        let directory = Path::new(text(package, "manifest_path")?)
            .parent()
            .ok_or("package manifest has no parent directory")?;
        let mut files = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            let filename = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if path.is_file() && PREFIXES.iter().any(|prefix| filename.starts_with(prefix)) {
                files.push(path);
            }
        }
        files.sort();
        if files.is_empty() {
            if !matches!(license, "MIT" | "Unlicense") {
                return Err(
                    format!("{name}: no license file found in {}", directory.display()).into(),
                );
            }
            let repository = package["repository"].as_str().unwrap_or("None");
            write!(
                out,
                "The crate ships no license file. Source: {repository}\n\n"
            )?;
            if license == "MIT" {
                let authors = array(package, "authors")?
                    .iter()
                    .map(|author| author.as_str().ok_or("invalid package author"))
                    .collect::<std::result::Result<Vec<_>, _>>()?
                    .join(", ");
                let authors = if authors.is_empty() {
                    "the authors"
                } else {
                    &authors
                };
                write!(out, "```text\nCopyright (c) {authors}\n\n{MIT}```\n\n")?;
            } else {
                write!(
                    out,
                    "The crate declares `Unlicense` in its package metadata. Standard text: \
<https://unlicense.org/> (<https://spdx.org/licenses/Unlicense.html>).\n\n\
```text\n{UNLICENSE}```\n\n"
                )?;
            }
        }
        for path in files {
            let bytes = fs::read(&path)?;
            let content = String::from_utf8_lossy(&bytes)
                .replace("\r\n", "\n")
                .replace('\r', "\n");
            write!(
                out,
                "### {}\n\n```text\n{}\n```\n\n",
                path.file_name().unwrap().to_string_lossy(),
                content.trim()
            )?;
        }
    }
    Ok(())
}
