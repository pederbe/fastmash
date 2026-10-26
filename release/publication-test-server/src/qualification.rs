//! Bind accepted measurements to runtime source and build inputs, excluding release versions.
use crate::common::{Result, repository};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
const PACKAGES: &[(&str, &str)] = &[
    ("fastmash", "cli"),
    ("fastmash-conversion", "conversion"),
    ("fastmash-numeric-contract", "numeric-contract"),
    ("fastmash-portable-numerics", "portable-numerics"),
    ("fastmash-sort-process", "sort-process"),
    ("fastmash-corpus-driver", "corpus-driver"),
];
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(output.stdout)
}
fn runtime(path: &str) -> bool {
    PACKAGES[..5]
        .iter()
        .any(|(_, dir)| path.starts_with(&format!("crates/{dir}/")))
        || matches!(
            path,
            "Cargo.toml"
                | "Cargo.lock"
                | "release/Containerfile"
                | "release/build.sh"
                | "rust-toolchain"
                | "rust-toolchain.toml"
        )
        || path.starts_with(".cargo/")
}
fn normalize(path: &str, bytes: Vec<u8>) -> Result<Vec<u8>> {
    if !path.ends_with("Cargo.toml") && path != "Cargo.lock" {
        return Ok(bytes);
    }
    let text = String::from_utf8(bytes)?;
    let mut own_package = false;
    let mut output = String::new();
    for line in text.lines() {
        if path != "Cargo.lock" && line.starts_with('[') {
            own_package = line == "[package]";
        }
        if path == "Cargo.lock" && line.starts_with("name = \"") {
            own_package = PACKAGES
                .iter()
                .any(|(name, _)| line == format!("name = \"{name}\""));
        }
        if path == "Cargo.lock" && line == "[[package]]" {
            own_package = false;
        }
        let line = if own_package && line.starts_with("version = \"") {
            "version = \"RELEASE_VERSION\"".to_string()
        } else if path.ends_with("Cargo.toml")
            && PACKAGES
                .iter()
                .any(|(name, _)| line.starts_with(&format!("{name} = ")))
        {
            // The dependency's path and every other option remain in the digest.
            if let Some((prefix, rest)) = line.split_once("version = \"=") {
                let (_, suffix) = rest.split_once('"').ok_or("invalid internal version pin")?;
                format!("{prefix}version = \"=RELEASE_VERSION\"{suffix}")
            } else {
                line.to_string()
            }
        } else {
            line.to_string()
        };
        output.push_str(&line);
        output.push('\n');
    }
    Ok(output.into_bytes())
}
fn digest(root: &Path, source: &str) -> Result<String> {
    let reference = if source == "WORKTREE" { "HEAD" } else { source };
    let listing = String::from_utf8(git(root, &["ls-tree", "-r", "--full-tree", reference])?)?;
    let mut data = Vec::new();
    for row in listing.lines() {
        let (entry, path) = row.split_once('\t').ok_or("invalid tree entry")?;
        if !runtime(path) {
            continue;
        }
        let mode = entry.split_whitespace().next().ok_or("missing tree mode")?;
        let bytes = if source == "WORKTREE" {
            fs::read(root.join(path))?
        } else {
            git(root, &["show", &format!("{source}:{path}")])?
        };
        let bytes = normalize(path, bytes)?;
        data.extend_from_slice(format!("{mode} {path}\0{}\0", bytes.len()).as_bytes());
        data.extend_from_slice(&bytes);
    }
    if data.is_empty() {
        return Err("no runtime inputs".into());
    }
    let mut hash = Command::new("sha256sum")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;
    hash.stdin
        .take()
        .ok_or("missing hash input")?
        .write_all(&data)?;
    let output = hash.wait_with_output()?;
    if !output.status.success() {
        return Err("sha256sum failed".into());
    }
    Ok(String::from_utf8(output.stdout)?
        .split_whitespace()
        .next()
        .ok_or("missing digest")?
        .to_string())
}
pub fn inputs(args: &[String]) -> Result<()> {
    let [source] = args else {
        return Err("usage: qualification-inputs REF|WORKTREE".into());
    };
    println!("{}", digest(&repository()?, source)?);
    Ok(())
}
pub fn check(args: &[String]) -> Result<()> {
    let [source] = args else {
        return Err("usage: check-qualification REF|WORKTREE".into());
    };
    let root = repository()?;
    let text = fs::read_to_string(root.join("release/qualification.tsv")).map_err(|_| "no accepted runtime qualification record; record the two-host evidence before releasing")?;
    let fields: std::collections::BTreeMap<_, _> =
        text.lines().filter_map(|s| s.split_once('\t')).collect();
    for key in [
        "inputs_sha256",
        "source_sha",
        "amd_evidence",
        "intel_evidence",
    ] {
        if fields.get(key).is_none_or(|v| v.is_empty()) {
            return Err(format!("qualification lacks {key}").into());
        }
    }
    if fields["amd_evidence"] == fields["intel_evidence"] {
        return Err("qualification requires both host evidence bindings".into());
    }
    if fields["inputs_sha256"] != digest(&root, fields["source_sha"])? {
        return Err("qualification source binding is invalid".into());
    }
    if fields["inputs_sha256"] != digest(&root, source)? {
        return Err("runtime inputs changed since accepted two-host qualification; qualify the new source and record its evidence before releasing".into());
    }
    Ok(())
}
pub fn record(args: &[String]) -> Result<()> {
    let [source, amd, intel] = args else {
        return Err("usage: record-qualification REF AMD_EVIDENCE_URL INTEL_EVIDENCE_URL".into());
    };
    if !amd.starts_with("https://")
        || !intel.starts_with("https://")
        || amd == intel
        || args.iter().any(|s| s.contains(['\t', '\n']))
    {
        return Err(
            "provide distinct HTTPS URLs for the accepted AMD and Intel measurement evidence"
                .into(),
        );
    }
    let root = repository()?;
    let sha = String::from_utf8(git(&root, &["rev-parse", &format!("{source}^{{commit}}")])?)?;
    let hash = digest(&root, source)?;
    fs::write(
        root.join("release/qualification.tsv"),
        format!(
            "inputs_sha256\t{hash}\nsource_sha\t{}\namd_evidence\t{amd}\nintel_evidence\t{intel}\n",
            sha.trim()
        ),
    )?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalization_keeps_external_versions_and_build_options() {
        let original = b"[package]\nname = \"fastmash\"\nversion = \"0.2.0\"\n[dependencies]\nmemchr = \"0.2.0\"\nfastmash-conversion = { path = \"../conversion\", version = \"=0.2.0\", features = [\"x\"] }\n";
        let normalized =
            String::from_utf8(normalize("crates/cli/Cargo.toml", original.to_vec()).unwrap())
                .unwrap();
        assert!(normalized.contains("memchr = \"0.2.0\""));
        assert!(normalized.contains("features = [\"x\"]"));
        assert!(!normalized.contains("=0.2.0"));
        let bumped = String::from_utf8(original.to_vec())
            .unwrap()
            .replace("version = \"0.2.0\"", "version = \"0.3.0\"")
            .replace("=0.2.0", "=0.3.0");
        assert_eq!(
            normalize("crates/cli/Cargo.toml", original.to_vec()).unwrap(),
            normalize("crates/cli/Cargo.toml", bumped.into_bytes()).unwrap()
        );
        assert!(runtime("crates/cli/build.rs"));
        assert!(runtime("release/Containerfile"));
        assert!(!runtime("CHANGELOG.md"));
    }
    #[test]
    fn lock_normalization_retains_third_party_versions() {
        let text = "[[package]]\nname = \"fastmash\"\nversion = \"0.2.0\"\n[[package]]\nname = \"other\"\nversion = \"0.2.0\"\n";
        let text =
            String::from_utf8(normalize("Cargo.lock", text.as_bytes().to_vec()).unwrap()).unwrap();
        assert!(text.contains("name = \"other\"\nversion = \"0.2.0\""));
        assert!(text.contains("name = \"fastmash\"\nversion = \"RELEASE_VERSION\""));
    }
    #[test]
    fn committed_input_digest_accepts_only_version_metadata_changes() {
        let root = std::path::PathBuf::from("/var/tmp").join(format!(
            "fastmash-qualification-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("crates/cli/src")).unwrap();
        git(&root, &["init"]).unwrap();
        git(&root, &["config", "user.email", "test@example.com"]).unwrap();
        git(&root, &["config", "user.name", "Test"]).unwrap();
        let manifest = "[package]\nname = \"fastmash\"\nversion = \"0.2.0\"\n";
        let lock = "[[package]]\nname = \"fastmash\"\nversion = \"0.2.0\"\n";
        fs::write(root.join("crates/cli/Cargo.toml"), manifest).unwrap();
        fs::write(root.join("Cargo.lock"), lock).unwrap();
        fs::write(root.join("crates/cli/src/main.rs"), "fn main() {}\n").unwrap();
        git(&root, &["add", "."]).unwrap();
        git(&root, &["commit", "-m", "Qualified runtime"]).unwrap();
        let accepted = digest(&root, "HEAD").unwrap();
        fs::write(
            root.join("crates/cli/Cargo.toml"),
            manifest.replace("0.2.0", "0.3.0"),
        )
        .unwrap();
        fs::write(root.join("Cargo.lock"), lock.replace("0.2.0", "0.3.0")).unwrap();
        assert_eq!(accepted, digest(&root, "WORKTREE").unwrap());
        fs::write(
            root.join("crates/cli/src/main.rs"),
            "fn main() { println!(\"changed\"); }\n",
        )
        .unwrap();
        assert_ne!(accepted, digest(&root, "WORKTREE").unwrap());
        // Recorded source is immutable even after working-tree inputs change.
        assert_eq!(accepted, digest(&root, "HEAD").unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
