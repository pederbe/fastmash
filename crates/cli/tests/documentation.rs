//! User documentation describes the current release, so it carries no
//! Fastmash release number that would go stale at the next release.
use std::{
    fs,
    path::{Path, PathBuf},
};

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Book directories that record history, measurements or project internals.
const SKIPPED: [&str; 3] = ["adr", "benchmarks", "contributing"];

/// Exact phrases that look like a release number but are not one: another
/// product's version that a reader needs, or sample data.
const ALLOWED: [&str; 1] = [
    // A grouped number in the German locale.
    "1.234.567,00",
];

fn markdown_pages(directory: &Path, pages: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_str().unwrap();
        if path.is_dir() {
            if !SKIPPED.contains(&name) {
                markdown_pages(&path, pages);
            }
        } else if name.ends_with(".md") {
            pages.push(path);
        }
    }
}

/// Whether `word` starts like `1.2.3` or `v1.2.3`.
fn is_release_number(word: &str) -> bool {
    let word = word.trim_matches('.');
    let mut parts = word.strip_prefix('v').unwrap_or(word).split('.');
    (0..3).all(|_| {
        parts
            .next()
            .is_some_and(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    })
}

fn release_numbers(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut line = line.to_owned();
        for phrase in ALLOWED {
            line = line.replace(phrase, " ");
        }
        if line.contains("@VERSION@") {
            found.push((index + 1, "@VERSION@".to_owned()));
        }
        for word in line.split(|c: char| !c.is_ascii_alphanumeric() && c != '.') {
            if is_release_number(word) {
                found.push((index + 1, word.to_owned()));
            }
        }
    }
    found
}

#[test]
fn recognizes_release_numbers() {
    for word in ["1.2.3", "v0.10.0", "0.4.0.", "10.20.30.40"] {
        assert!(is_release_number(word), "{word}");
    }
    for word in ["1.9", "20.04", "v2", "a.b.c", "1..3", "x1.2.3", ""] {
        assert!(!is_release_number(word), "{word}");
    }
    assert_eq!(
        release_numbers("ok 1.234.567,00\nfastmash_0.1.0_amd64.deb v@VERSION@"),
        [(2, "@VERSION@".to_owned()), (2, "0.1.0".to_owned())]
    );
}

#[test]
fn user_documentation_has_no_release_numbers() {
    let root = Path::new(ROOT);
    let mut pages = vec![root.join("README.md"), root.join("install.sh")];
    markdown_pages(&root.join("docs/src"), &mut pages);
    pages.sort();
    let mut failures = String::new();
    for page in pages {
        let text = fs::read_to_string(&page).unwrap();
        for (line, number) in release_numbers(&text) {
            failures.push_str(&format!(
                "{}:{line}: {number}\n",
                page.strip_prefix(root).unwrap().display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "user documentation names a release number:\n{failures}\
         Remove the number, or add the exact phrase to ALLOWED in \
         crates/cli/tests/documentation.rs if it is another product's \
         version or sample data."
    );
}
