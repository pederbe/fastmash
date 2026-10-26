//! Exercise registry failures and partial-upload recovery without network publication.
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
#[test]
fn resumes_only_missing_packages_and_refuses_ambiguous_registry_state() {
    let root = std::path::PathBuf::from("/var/tmp").join(format!(
        "fastmash-registry-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("bin")).unwrap();
    let trace = root.join("trace");
    executable(
        &root.join("bin/cargo"),
        r#"#!/bin/bash
set -euo pipefail
printf '%s\n' "$*" >> "$FIXTURE_TRACE"
if [ "$1" = package ]; then
  while [ "$1" != -p ]; do shift; done
  shift
  mkdir -p "$CARGO_TARGET_DIR/package"
  printf existing > "$CARGO_TARGET_DIR/package/$1-0.3.0.crate"
fi
"#,
    );
    executable(
        &root.join("bin/curl"),
        r#"#!/bin/bash
set -euo pipefail
output=
for ((i=1; i<=$#; i++)); do
  if [ "${!i}" = --output ]; then j=$((i+1)); output=${!j}; fi
done
if [ "$FIXTURE_HTTP" = 403 ]; then printf 403; exit; fi
if [[ "${!#}" = */fastmash-numeric-contract/* ]]; then
  hash=$(printf existing | sha256sum | cut -d' ' -f1)
  printf '{"version":{"num":"0.3.0","yanked":false,"checksum":"%s"}}' "$hash" > "$output"
  printf 200
else
  printf 404
fi
"#,
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../publish-crates.sh");
    let run = |status: &str| {
        Command::new("bash")
            .arg(&script)
            .args(["0.3.0", "dry-run"])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("FIXTURE_TRACE", &trace)
            .env("FIXTURE_HTTP", status)
            .env("CARGO_TARGET_DIR", root.join("target"))
            .output()
            .unwrap()
    };
    let output = run("partial");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(&trace).unwrap();
    let publication = calls
        .lines()
        .find(|line| line.starts_with("publish "))
        .unwrap();
    assert!(!publication.contains("-p fastmash-numeric-contract"));
    assert!(publication.contains("-p fastmash-conversion"));
    assert!(publication.contains("-p fastmash-portable-numerics"));
    assert!(publication.contains("-p fastmash-sort-process"));
    assert!(publication.ends_with("-p fastmash --dry-run"));
    fs::write(&trace, "").unwrap();
    let output = run("403");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("HTTP 403"));
    assert_eq!(fs::read_to_string(&trace).unwrap(), "");
    fs::remove_dir_all(root).unwrap();
}
