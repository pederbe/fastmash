//! Prepare versioned release metadata without changing the development branch.
use crate::common::{Result, repository};
use std::{fs, path::Path, process::Command};

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn version(text: &str) -> Result<[u64; 3]> {
    let parts: Vec<_> = text.trim_start_matches('v').split('.').collect();
    let [a, b, c] = parts.as_slice() else {
        return Err("expected a stable x.y.z version".into());
    };
    Ok([a.parse()?, b.parse()?, c.parse()?])
}

fn meaningful(path: &str) -> bool {
    !path.ends_with(".md")
        && !path.starts_with("docs/")
        && !path.starts_with("site/")
        && !path.starts_with("tools/site/")
        && !path.starts_with(".github/ISSUE_TEMPLATE/")
        && !matches!(
            path,
            "book.toml"
                | ".github/FUNDING.yml"
                | ".github/workflows/docs.yml"
                | ".github/workflows/website-deploy.yml"
        )
}

fn bump(mut old: [u64; 3], breaking: bool) -> [u64; 3] {
    if breaking {
        if old[0] == 0 {
            old[1] += 1;
        } else {
            old[0] += 1;
            old[1] = 0;
        }
        old[2] = 0;
    } else {
        old[2] += 1;
    }
    old
}

/// PREVIOUS_TAG DATE (auto|minor|major) OUTPUT. The output uses Actions key=value syntax.
pub fn prepare(args: &[String]) -> Result<()> {
    let (required, minimum) = args.split_at(args.len().min(4));
    let [previous, date, mode, output] = required else {
        return Err("usage: fastmash-release prepare-weekly PREVIOUS_TAG YYYY-MM-DD (auto|minor|major) OUTPUT".into());
    };
    let root = repository()?;
    if minimum.len() > 1 {
        return Err("expected at most one minimum tag".into());
    }
    prepare_at(
        &root,
        previous,
        date,
        mode,
        Path::new(output),
        minimum.first().map(String::as_str),
    )
}

fn prepare_at(
    root: &Path,
    previous: &str,
    date: &str,
    mode: &str,
    output: &Path,
    minimum: Option<&str>,
) -> Result<()> {
    let old = version(previous)?;
    if date.len() != 10
        || !date.chars().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                c == '-'
            } else {
                c.is_ascii_digit()
            }
        })
    {
        return Err("expected release date YYYY-MM-DD".into());
    }
    if !["auto", "minor", "major"].contains(&mode) {
        return Err("invalid version increment".into());
    }
    if !git(root, &["status", "--porcelain"])?.is_empty() {
        return Err("release preparation needs a clean checkout".into());
    }
    // Generated tags are siblings of later main revisions. Their parent is the exact
    // previous development snapshot, so ancestry and change detection use that parent.
    let subject = git(root, &["show", "-s", "--format=%s", previous])?;
    let base = if subject.starts_with("Prepare automated release v") {
        format!("{previous}^")
    } else {
        previous.to_owned()
    };
    git(root, &["merge-base", "--is-ancestor", &base, "HEAD"])?;
    let range = format!("{base}..HEAD");
    let paths = git(root, &["diff", "--name-only", &base, "HEAD"])?;
    if !paths.lines().any(meaningful) {
        fs::write(output, "changed=false\n")?;
        return Ok(());
    }
    let log = git(
        root,
        &["log", "--no-merges", "--format=%s%x00%b%x00%h%x00", &range],
    )?;
    let records: Vec<_> = log.split('\0').collect();
    let mut groups = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
    let mut breaking = false;
    let mut feature = false;
    for record in records.chunks(3) {
        let [subject, body, hash] = record else {
            continue;
        };
        let subject = subject.trim();
        if subject.is_empty() {
            continue;
        }
        let is_breaking = subject
            .split_once(':')
            .is_some_and(|(prefix, _)| prefix.ends_with('!'))
            || body.contains("BREAKING CHANGE:")
            || body.contains("BREAKING-CHANGE:");
        breaking |= is_breaking;
        feature |= subject.starts_with("feat");
        let group = if is_breaking {
            0
        } else if subject.starts_with("feat") {
            1
        } else if subject.starts_with("fix") {
            2
        } else {
            3
        };
        groups[group].push(format!(
            "- {subject} ([{}](https://github.com/pederbe/fastmash/commit/{}))\n",
            hash.trim(),
            hash.trim()
        ));
    }
    let current_changelog = fs::read_to_string(root.join("CHANGELOG.md"))?;
    let previous_source_changelog = git(root, &["show", &format!("{base}:CHANGELOG.md")])?;
    let previous_authored = previous_source_changelog
        .split_once("## [Unreleased]\n")
        .ok_or("previous source lacks Unreleased heading")?
        .1
        .split("\n## [")
        .next()
        .unwrap_or("");
    let upcoming = current_changelog
        .split_once("## [Unreleased]\n")
        .ok_or("missing Unreleased heading")?
        .1
        .split("\n## [")
        .next()
        .unwrap_or("");
    // Existing authored behavior and numerical-profile changes require a minor
    // release before 1.0, even when historical commits lack conventional markers.
    let authored_changes = authored_delta(upcoming, previous_authored);
    let authored_behavior = authored_changes.contains("### Changed\n")
        || authored_changes.contains("### Added\n")
        || authored_changes.contains("### Removed\n");
    breaking |= old[0] == 0 && authored_behavior;
    let mut next = bump(old, breaking);
    if mode == "minor" || (feature && old[0] > 0) {
        next = next.max([old[0], old[1] + 1, 0]);
    }
    if mode == "major" {
        next = [old[0] + 1, 0, 0];
    }
    let current = crate::common::crate_version(&root.join("crates/cli/Cargo.toml"))?;
    // A deliberately prepared newer version on main takes precedence.
    next = next.max(version(&current)?);
    if let Some(minimum) = minimum {
        next = next.max(bump(version(minimum)?, false));
    }
    let next = format!("{}.{}.{}", next[0], next[1], next[2]);
    let mut manifests = Vec::new();
    let mut versions = std::collections::BTreeMap::new();
    for entry in fs::read_dir(root.join("crates"))? {
        let path = entry?.path().join("Cargo.toml");
        let text = fs::read_to_string(&path)?;
        let name = text
            .lines()
            .find_map(|line| line.strip_prefix("name = \""))
            .ok_or("crate has no package name")?
            .trim_end_matches('"')
            .to_string();
        let old = crate::common::crate_version(&path)?;
        // The published packages and corpus driver already share a version.
        if old == current {
            versions.insert(name, old);
        }
        manifests.push((path, text));
    }
    let lock = fs::read_to_string(root.join("Cargo.lock"))?;
    let mut lock = lock;
    for (name, old) in &versions {
        lock = lock.replace(
            &format!("name = \"{name}\"\nversion = \"{old}\""),
            &format!("name = \"{name}\"\nversion = \"{next}\""),
        );
    }
    for (path, text) in &mut manifests {
        let own_name = text
            .lines()
            .find_map(|line| line.strip_prefix("name = \""))
            .ok_or("crate has no package name")?
            .trim_end_matches('"')
            .to_string();
        if versions.contains_key(&own_name) {
            *text = text.replacen(
                &format!("version = \"{current}\""),
                &format!("version = \"{next}\""),
                1,
            );
        }
        for (name, old) in &versions {
            *text = text
                .lines()
                .map(|line| {
                    if line.starts_with(&format!("{name} = ")) {
                        line.replace(
                            &format!("version = \"={old}\""),
                            &format!("version = \"={next}\""),
                        )
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
                + "\n";
        }
        let _ = path;
    }
    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))?;
    let (intro, rest) = changelog
        .split_once("## [Unreleased]\n")
        .ok_or("missing Unreleased changelog heading")?;
    let (authored, _) = rest
        .split_once("\n## [")
        .ok_or("missing previous changelog section")?;
    let previous_changelog = git(root, &["show", &format!("{previous}:CHANGELOG.md")])?;
    let previous_rest = previous_changelog
        .split_once("## [Unreleased]\n")
        .ok_or("previous release lacks Unreleased heading")?
        .1;
    let history = previous_rest
        .split_once("\n## [")
        .ok_or("previous release lacks version history")?
        .1;
    let mut notes = authored_delta(authored, previous_authored);
    notes.push_str("\n\n");
    for (heading, entries) in ["Breaking changes", "Added", "Fixed", "Other changes"]
        .into_iter()
        .zip(groups)
    {
        if !entries.is_empty() {
            notes.push_str(&format!("### {heading}\n\n{}\n", entries.concat()));
        }
    }
    notes.push_str(&format!(
        "[Full comparison](https://github.com/pederbe/fastmash/compare/{previous}...v{next})\n"
    ));
    let changelog = format!(
        "{intro}## [Unreleased]\n\n## [{next}] - {date}\n\n{}\n\n## [{history}",
        notes.trim()
    );
    for (path, text) in manifests {
        fs::write(path, text)?;
    }
    fs::write(root.join("Cargo.lock"), lock)?;
    let changelog = changelog
        .lines()
        .map(|line| {
            if line.starts_with("[Unreleased]:") {
                format!("[Unreleased]: https://github.com/pederbe/fastmash/compare/v{next}...HEAD")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(root.join("CHANGELOG.md"), changelog)?;
    if root.join("site/release-version.txt").exists() {
        fs::write(root.join("site/release-version.txt"), format!("{next}\n"))?;
    }
    fs::write(output, format!("changed=true\ntag=v{next}\n"))?;
    Ok(())
}

// Keep each bullet and its continuation lines together. Headings belong to the
// records below them, so adding one entry does not repeat earlier prose.
fn authored_records(text: &str) -> Vec<(String, String)> {
    let mut records = Vec::new();
    let mut heading = String::new();
    let mut entry = String::new();
    for line in text.lines().chain(std::iter::once("### ")) {
        if line.starts_with("### ") || line.starts_with("- ") || line.is_empty() {
            if !entry.trim().is_empty() {
                records.push((heading.clone(), entry.trim().to_string()));
                entry.clear();
            }
            if line.starts_with("### ") {
                heading = line.to_string();
                continue;
            }
        }
        if !line.is_empty() {
            entry.push_str(line);
            entry.push('\n');
        }
    }
    records
}

fn authored_delta(current: &str, previous: &str) -> String {
    let old = authored_records(previous);
    let mut output = String::new();
    let mut last_heading = String::new();
    for (heading, entry) in authored_records(current) {
        if old.contains(&(heading.clone(), entry.clone())) {
            continue;
        }
        if heading != last_heading && !heading.is_empty() {
            output.push_str(&format!("{heading}\n\n"));
            last_heading = heading;
        }
        output.push_str(&entry);
        output.push_str("\n\n");
    }
    output.trim().to_string()
}

pub fn notes(args: &[String]) -> Result<()> {
    let [tag, output] = args else {
        return Err("usage: fastmash-release release-notes TAG OUTPUT".into());
    };
    let root = repository()?;
    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))?;
    let version = tag.strip_prefix('v').ok_or("expected version tag")?;
    let heading = format!("## [{version}] - ");
    let section = changelog
        .split_once(&heading)
        .ok_or("missing dated release changelog section")?
        .1;
    let (_, section) = section.split_once('\n').ok_or("empty changelog section")?;
    let notes = section.split("\n## [").next().unwrap_or("").trim();
    if notes.is_empty() {
        return Err("empty release notes".into());
    }
    fs::write(output, format!("{notes}\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Directory(std::path::PathBuf);
    impl Directory {
        fn new() -> Self {
            let name = format!(
                "fastmash-weekly-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let path = std::path::PathBuf::from("/var/tmp").join(name);
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn weekly_metadata_keeps_internal_versions_and_does_not_repeat_releases() {
        let dir = Directory::new();
        let root = &dir.0;
        git(root, &["init"]).unwrap();
        git(root, &["config", "user.email", "test@example.com"]).unwrap();
        git(root, &["config", "user.name", "Test"]).unwrap();
        for (name, directory) in [("fastmash", "cli"), ("fastmash-conversion", "conversion")] {
            fs::create_dir_all(root.join(format!("crates/{directory}"))).unwrap();
            fs::write(root.join(format!("crates/{directory}/Cargo.toml")), format!("[package]\nname = \"{name}\"\nversion = \"0.2.0\"\n[dependencies]\nfastmash-conversion = {{ path = \"../conversion\", version = \"=0.2.0\" }}\n")).unwrap();
        }
        fs::write(root.join("Cargo.toml"), "[workspace]\n").unwrap();
        fs::write(root.join("Cargo.lock"), "[[package]]\nname = \"fastmash\"\nversion = \"0.2.0\"\n\n[[package]]\nname = \"fastmash-conversion\"\nversion = \"0.2.0\"\n").unwrap();
        fs::write(
            root.join("CHANGELOG.md"),
            "# Changelog\n\n## [Unreleased]\n\n## [0.2.0] - 2026-10-12\n\nOld\n",
        )
        .unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-m", "Initial"]).unwrap();
        git(root, &["tag", "v0.2.0"]).unwrap();
        fs::write(root.join("README.md"), "Only docs\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-m", "Document installation"]).unwrap();
        let output = root
            .parent()
            .unwrap()
            .join(format!("weekly-output-{}", std::process::id()));
        prepare_at(root, "v0.2.0", "2026-10-26", "auto", &output, None).unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "changed=false\n");
        fs::write(root.join("CHANGELOG.md"), "# Changelog\n\n## [Unreleased]\n\n### Changed\n\n- Behavior A.\n\n## [0.2.0] - 2026-10-12\n\nOld\n").unwrap();
        fs::write(root.join("install.sh"), "true\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-m", "fix!: Change installer behavior"]).unwrap();
        let source = git(root, &["rev-parse", "HEAD"]).unwrap();
        prepare_at(root, "v0.2.0", "2026-10-26", "auto", &output, None).unwrap();
        assert!(fs::read_to_string(&output).unwrap().contains("tag=v0.3.0"));
        for directory in ["cli", "conversion"] {
            let manifest =
                fs::read_to_string(root.join(format!("crates/{directory}/Cargo.toml"))).unwrap();
            assert!(manifest.contains("version = \"0.3.0\""));
            assert!(manifest.contains("version = \"=0.3.0\""));
        }
        assert_eq!(
            fs::read_to_string(root.join("Cargo.lock"))
                .unwrap()
                .matches("version = \"0.3.0\"")
                .count(),
            2
        );
        assert!(
            fs::read_to_string(root.join("CHANGELOG.md"))
                .unwrap()
                .contains("### Breaking changes")
        );
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-m", "Prepare automated release v0.3.0"]).unwrap();
        git(root, &["tag", "v0.3.0"]).unwrap();
        git(root, &["checkout", "--detach", source.trim()]).unwrap();
        prepare_at(root, "v0.3.0", "2026-11-02", "auto", &output, None).unwrap();
        assert_eq!(fs::read_to_string(&output).unwrap(), "changed=false\n");
        let changelog = fs::read_to_string(root.join("CHANGELOG.md"))
            .unwrap()
            .replace("\n## [0.2.0]", "\n### Fixed\n\n- Fix B.\n\n## [0.2.0]");
        fs::write(root.join("CHANGELOG.md"), changelog).unwrap();
        fs::write(root.join("install.sh"), "true # fixed\n").unwrap();
        git(root, &["add", "."]).unwrap();
        git(root, &["commit", "-m", "Fix installer"]).unwrap();
        prepare_at(root, "v0.3.0", "2026-11-02", "auto", &output, None).unwrap();
        assert!(fs::read_to_string(&output).unwrap().contains("tag=v0.3.1"));
        let text = fs::read_to_string(root.join("CHANGELOG.md")).unwrap();
        let new_section = text
            .split_once("## [0.3.1]")
            .unwrap()
            .1
            .split("\n## [")
            .next()
            .unwrap();
        assert!(new_section.contains("Fix B."));
        assert!(!new_section.contains("Behavior A."));
        git(root, &["restore", "."]).unwrap();
        prepare_at(
            root,
            "v0.3.0",
            "2026-11-02",
            "auto",
            &output,
            Some("v0.4.0"),
        )
        .unwrap();
        assert!(fs::read_to_string(&output).unwrap().contains("tag=v0.4.1"));
        fs::remove_file(output).unwrap();
    }
}
