//! Bind accepted measurements to exact release source and compiler inputs.
use crate::common::{Result, repository};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};
const RUNTIME_CRATES: &[&str] = &[
    "cli",
    "conversion",
    "numeric-contract",
    "portable-numerics",
    "sort-process",
];
fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(output.stdout)
}
fn runtime(path: &str) -> bool {
    RUNTIME_CRATES
        .iter()
        .any(|dir| path.starts_with(&format!("crates/{dir}/")))
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
    let text = fs::read_to_string(root.join("release/qualification.tsv")).map_err(|_| "no accepted exact-version qualification record; qualify the planned release version on both hosts before releasing")?;
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
        return Err("release build inputs or package versions changed since accepted two-host qualification; qualify this exact version and commit its accepted executable hashes before releasing".into());
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
    fn committed_input_digest_rejects_version_and_runtime_changes() {
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
        let manifest = "[package]\nname = \"fastmash\"\nversion = \"0.2.0\"\n[dependencies]\nfastmash-conversion = { version = \"=0.2.0\", path = \"../conversion\" }\n";
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
        assert_ne!(accepted, digest(&root, "WORKTREE").unwrap());
        fs::write(root.join("Cargo.lock"), lock).unwrap();
        assert_ne!(accepted, digest(&root, "WORKTREE").unwrap());
        fs::write(root.join("crates/cli/Cargo.toml"), manifest).unwrap();
        fs::write(root.join("Cargo.lock"), lock.replace("0.2.0", "0.3.0")).unwrap();
        assert_ne!(accepted, digest(&root, "WORKTREE").unwrap());
        fs::write(root.join("Cargo.lock"), lock).unwrap();
        assert_eq!(accepted, digest(&root, "WORKTREE").unwrap());
        fs::write(
            root.join("crates/cli/Cargo.toml"),
            manifest.replace("=0.2.0", "=0.3.0"),
        )
        .unwrap();
        assert_ne!(accepted, digest(&root, "WORKTREE").unwrap());
        fs::write(root.join("crates/cli/Cargo.toml"), manifest).unwrap();
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
