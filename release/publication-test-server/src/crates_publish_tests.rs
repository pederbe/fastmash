//! Exercise registry recovery and frozen delivery hashes without network publication.
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

#[test]
fn reproducible_build_freezes_delivery_hashes_and_rejects_mismatch() {
    let root = std::path::PathBuf::from("/var/tmp").join(format!(
        "fastmash-delivery-build-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::create_dir(root.join("release")).unwrap();
    fs::write(root.join("release/Containerfile"), "fixture\n").unwrap();
    let historical = "historical qualification is not a delivery manifest\n";
    fs::write(root.join("release/qualified.sha256"), historical).unwrap();
    executable(&root.join("bin/curl"), "#!/bin/sh\nexit 0\n");
    executable(
        &root.join("bin/docker"),
        r#"#!/bin/bash
set -euo pipefail
if [ "$1" != run ]; then exit 0; fi
build=
for arg in "$@"; do
  case "$arg" in CARGO_TARGET_DIR=/out/*) build=${arg##*/};; esac
done
test -n "$build"
out="$RUNNER_TEMP/weekly-linux/out/$build/release"
mkdir -p "$out"
for binary in fastmash; do
  if [ "$FIXTURE_MODE" = mismatch ] && [ "$build" = second ]; then
    printf different > "$out/$binary"
  else
    printf candidate > "$out/$binary"
  fi
done
"#,
    );
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../prepare-linux.sh");
    let run = |name: &str, mode: &str| {
        Command::new("bash")
            .arg(&script)
            .current_dir(&root)
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("RUNNER_TEMP", root.join(name))
            .env("FIXTURE_MODE", mode)
            .output()
            .unwrap()
    };
    let reproducible = run("reproducible", "same");
    assert!(
        reproducible.status.success(),
        "{}",
        String::from_utf8_lossy(&reproducible.stderr)
    );
    let manifest = root.join("release/delivery.sha256");
    let text = fs::read_to_string(&manifest).unwrap();
    assert_eq!(text.lines().count(), 1);
    assert!(text.contains("  fastmash\n"));
    assert!(!text.contains("fastmash-sort-supervisor"));
    let verified = Command::new("sha256sum")
        .arg("-c")
        .arg(&manifest)
        .current_dir(root.join("reproducible/weekly-linux/out/first/release"))
        .output()
        .unwrap();
    assert!(
        verified.status.success(),
        "{}",
        String::from_utf8_lossy(&verified.stderr)
    );
    assert_eq!(
        fs::read_to_string(root.join("release/qualified.sha256")).unwrap(),
        historical
    );
    fs::remove_file(&manifest).unwrap();
    let different = run("nonreproducible", "mismatch");
    assert!(!different.status.success());
    assert!(!manifest.exists());
    assert_eq!(
        fs::read_to_string(root.join("release/qualified.sha256")).unwrap(),
        historical
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn website_refresh_skips_stale_releases_and_reports_its_own_failure() {
    let root = std::path::PathBuf::from("/var/tmp").join(format!(
        "fastmash-website-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(root.join("bin")).unwrap();
    executable(&root.join("bin/gh"), "#!/bin/sh\nprintf delivered\n");
    executable(
        &root.join("bin/curl"),
        r#"#!/bin/bash
set -euo pipefail
printf 'requested\n' >> "$FIXTURE_TRACE"
if [ "$FIXTURE_MODE" = failure ]; then exit 22; fi
printf '{"success":true,"result":{"build_uuid":"fixture-build"}}' > "${!#}"
"#,
    );
    let workflow = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/website-deploy.yml"),
    )
    .unwrap();
    let script = workflow
        .split_once("        run: |\n")
        .unwrap()
        .1
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    let trace = root.join("trace");
    for (sha, mode, success, requested) in [
        ("stale", "success", true, false),
        ("delivered", "success", true, true),
        ("", "success", true, true),
        ("delivered", "failure", false, true),
    ] {
        fs::write(&trace, "").unwrap();
        let output = Command::new("bash")
            .args(["-c", &script])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    root.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .env("RUNNER_TEMP", &root)
            .env("GITHUB_STEP_SUMMARY", root.join("summary"))
            .env("GITHUB_REPOSITORY", "fixture/website")
            .env("RELEASE_SHA", sha)
            .env("CLOUDFLARE_DEPLOY_HOOK", "https://example.invalid/hook")
            .env("FIXTURE_TRACE", &trace)
            .env("FIXTURE_MODE", mode)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(!fs::read_to_string(&trace).unwrap().is_empty(), requested);
    }
    fs::remove_dir_all(root).unwrap();
}
