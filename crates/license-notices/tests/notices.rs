use fastmash_license_notices::{Target, write_notices};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

struct Directory(PathBuf);

impl Directory {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "fastmash-license-notices-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Directory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn package(id: &str, name: &str, license: &str, external: bool, directory: &Path) -> Value {
    json!({
        "id": id,
        "name": name,
        "version": "1.0.0",
        "source": external.then_some("registry+fixture"),
        "license": license,
        "manifest_path": directory.join("Cargo.toml"),
        "authors": ["Alice", "Bob"],
        "repository": "https://example.invalid/source"
    })
}

fn dependency(id: &str, kinds: &[Option<&str>]) -> Value {
    json!({
        "pkg": id,
        "dep_kinds": kinds.iter().map(|kind| json!({"kind": kind})).collect::<Vec<_>>()
    })
}

fn node(id: &str, dependencies: Vec<Value>) -> Value {
    json!({"id": id, "deps": dependencies})
}

fn metadata(packages: Vec<Value>, nodes: Vec<Value>) -> Value {
    json!({"packages": packages, "resolve": {"nodes": nodes}})
}

fn one_dependency(directory: &Path, license: &str) -> Value {
    metadata(
        vec![
            package("root", "fastmash", "MIT", false, directory),
            package("fixture", "fixture", license, true, directory),
        ],
        vec![
            node("root", vec![dependency("fixture", &[None])]),
            node("fixture", vec![]),
        ],
    )
}

fn render(metadata: &Value, target: Target) -> String {
    let mut output = Vec::new();
    write_notices(metadata, target, &mut output).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn normal_graph_excludes_development_build_and_unrelated_packages() {
    let directory = Directory::new();
    let packages = [
        ("root", "fastmash", false),
        ("local", "local-library", false),
        ("zeta", "zeta", true),
        ("beta", "beta", true),
        ("dev", "development-only", true),
        ("build", "build-only", true),
        ("other", "unrelated-tool", true),
    ]
    .map(|(id, name, external)| package(id, name, "MIT", external, &directory.0));
    let graph = metadata(
        packages.into(),
        vec![
            node(
                "root",
                vec![
                    dependency("local", &[None]),
                    dependency("beta", &[Some("build"), None]),
                    dependency("dev", &[Some("dev")]),
                    dependency("build", &[Some("build")]),
                ],
            ),
            node("local", vec![dependency("zeta", &[None])]),
            node("zeta", vec![]),
            node("beta", vec![dependency("zeta", &[None])]),
            node("dev", vec![]),
            node("build", vec![]),
            node("other", vec![]),
        ],
    );
    let output = render(&graph, Target::Linux);
    let headings: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("## "))
        .collect();
    assert_eq!(headings, ["## beta 1.0.0", "## zeta 1.0.0"]);
}

#[test]
fn missing_file_fallbacks_keep_attribution_and_only_known_terms() {
    let directory = Directory::new();
    let mit = render(&one_dependency(&directory.0, "MIT"), Target::Linux);
    assert!(mit.contains("Source: https://example.invalid/source\n\n```text\nCopyright (c) Alice, Bob\n\nPermission is hereby granted"));
    let mut anonymous = one_dependency(&directory.0, "MIT");
    anonymous["packages"][1]["authors"] = json!([]);
    assert!(render(&anonymous, Target::Linux).contains("Copyright (c) the authors\n"));

    let unlicense = render(&one_dependency(&directory.0, "Unlicense"), Target::Linux);
    assert!(unlicense.contains("License: Unlicense\n"));
    assert!(unlicense.contains("The crate declares `Unlicense` in its package metadata."));
    assert!(
        unlicense
            .contains("<https://unlicense.org/> (<https://spdx.org/licenses/Unlicense.html>).")
    );
    assert!(
        unlicense
            .contains("This is free and unencumbered software released into the public domain.")
    );
    assert!(!unlicense.contains("Copyright (c)"));
    for license in ["Apache-2.0", "MIT OR Apache-2.0", "Unknown"] {
        let error = write_notices(
            &one_dependency(&directory.0, license),
            Target::Linux,
            &mut Vec::new(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .starts_with("fixture: no license file found in ")
        );
    }
}

#[test]
fn stored_license_files_keep_text_and_order_without_fallbacks() {
    let directory = Directory::new();
    fs::write(directory.0.join("NOTICE"), b" \r\nnotice\rtext\r\n ").unwrap();
    fs::write(directory.0.join("LICENSE"), b" \r\ncrate license \xff\r\n ").unwrap();
    fs::write(directory.0.join("authors.txt"), b"Alice\n").unwrap();
    fs::write(directory.0.join("README.md"), b"not a notice").unwrap();
    fs::create_dir(directory.0.join("COPYING")).unwrap();
    let output = render(&one_dependency(&directory.0, "Apache-2.0"), Target::Linux);
    let expected = "### LICENSE\n\n```text\ncrate license \u{fffd}\n```\n\n\
### NOTICE\n\n```text\nnotice\ntext\n```\n\n\
### authors.txt\n\n```text\nAlice\n```\n\n";
    assert!(output.ends_with(expected), "{output}");
    assert!(!output.contains("not a notice"));
    assert!(!output.contains("ships no license file"));
}

#[test]
fn targets_preserve_archive_description_and_standard_library_preface() {
    let directory = Directory::new();
    let graph = one_dependency(&directory.0, "MIT");
    let linux = render(&graph, Target::Linux);
    let macos = render(&graph, Target::Macos);
    assert!(linux.starts_with("# Third-party licenses\n\nThe fastmash executable includes the following third-party code.\n\n"));
    assert!(macos.starts_with("# Third-party licenses\n\nThe fastmash executable includes the following third-party code.\n\n"));
    for output in [&linux, &macos] {
        assert!(output.contains("The Rust standard library (std, core, alloc, and their dependencies) is\nlinked into the binaries under the MIT and Apache-2.0 licenses:\n<https://github.com/rust-lang/rust/blob/master/COPYRIGHT>.\n\n"));
    }
    assert!(linux.contains("Target: `x86_64-unknown-linux-gnu`.\n\n"));
    assert!(macos.contains("Target: `aarch64-apple-darwin`.\n\n"));
}
