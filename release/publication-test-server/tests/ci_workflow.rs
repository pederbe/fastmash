//! Keep every automatic PR check behind the required CI result.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workflow() -> String {
    fs::read_to_string(root().join(".github/workflows/ci.yml")).unwrap()
}

#[cfg(unix)]
#[test]
fn weekly_release_does_not_retry_completed_manual_delivery() {
    let workflow = fs::read_to_string(root().join(".github/workflows/weekly-release.yml")).unwrap();
    let script = workflow.split_once("        run: |\n").unwrap().1;
    let script = script.split_once("          minimum=()\n").unwrap().0;
    let script = script
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    let script = format!(
        "git() {{ case \"$1\" in\n\
           tag) printf 'v0.4.0\\n' ;;\n\
           rev-parse) printf 'frozen-source\\n' ;;\n\
           ls-remote) printf '%s\\trefs/heads/release\\n' \"$DELIVERED_SHA\" ;;\n\
           *) return 99 ;; esac; }}\n\
         gh() {{ case \"$1 $2\" in\n\
           api*) printf 'v0.4.0\\n' ;;\n\
           'run list') printf '%s\\n' \"$RECENT_RUN\" ;;\n\
           'run rerun') printf 'retry\\n' ;;\n\
           *) return 99 ;; esac; }}\n\
         {script}\nprintf 'prepare\\n'"
    );
    let failed = r#"[{"databaseId":42,"status":"completed","conclusion":"failure"}]"#;
    let running = r#"[{"databaseId":42,"status":"in_progress","conclusion":""}]"#;
    let success = r#"[{"databaseId":42,"status":"completed","conclusion":"success"}]"#;
    for (delivered, recent, supersede, expected) in [
        ("frozen-source", failed, "false", "prepare\n"),
        ("previous-source", failed, "false", "retry\n"),
        ("", failed, "false", "retry\n"),
        ("previous-source", running, "false", ""),
        ("previous-source", success, "false", "prepare\n"),
        ("previous-source", failed, "true", "prepare\n"),
    ] {
        let output = std::process::Command::new("bash")
            .args(["-ec", &script])
            .env("DELIVERED_SHA", delivered)
            .env("RECENT_RUN", recent)
            .env("SUPERSEDE_FAILED", supersede)
            .env("GITHUB_REPOSITORY", "pederbe/fastmash")
            .env("GITHUB_STEP_SUMMARY", "/dev/null")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }
}

#[test]
fn release_dispatch_can_find_the_repository_without_a_checkout() {
    let workflow = fs::read_to_string(root().join(".github/workflows/release.yml")).unwrap();
    let publish = workflow.split_once("\n  publish:\n").unwrap().1;
    let step = publish
        .split_once("      - name: Publish verified release and start package repository update\n")
        .unwrap()
        .1
        .split_once("        run: |\n")
        .unwrap()
        .0;
    // gh run/workflow commands otherwise infer the repository from .git.
    assert!(step.contains("          GH_REPO: ${{ github.repository }}\n"));
}

#[cfg(unix)]
#[test]
fn installer_selects_the_latest_available_registry_version() {
    let workflow = fs::read_to_string(root().join(".github/workflows/cargo-binstall.yml")).unwrap();
    let step = workflow
        .split_once("      - name: Select the published stable version\n")
        .unwrap()
        .1
        .split_once("      - name:")
        .unwrap()
        .0;
    let script = step.split_once("        run: |\n").unwrap().1;
    let script = script
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    // GitHub can publish an archive before the matching crate reaches crates.io.
    let script = format!(
        "curl() {{ printf '%s\\n' \"$REGISTRY_FIXTURE\"; return \"$REGISTRY_STATUS\"; }}\n\
         gh() {{ printf 'v0.4.0\\n'; }}\n{script}"
    );
    let available = concat!(
        "{\"vers\":\"0.3.0\",\"yanked\":false}\n",
        "{\"vers\":\"0.10.0\",\"yanked\":false}\n",
        "{\"vers\":\"0.9.0\",\"yanked\":false}\n",
        "{\"vers\":\"1.0.0\",\"yanked\":true}\n",
        "{\"vers\":\"2.0.0-rc.1\",\"yanked\":false}\n"
    );
    for (requested, registry, status, expected) in [
        (
            "",
            "{\"vers\":\"0.3.0\",\"yanked\":false}",
            "0",
            Some("0.3.0"),
        ),
        ("", available, "0", Some("0.10.0")),
        ("v0.4.0", "unavailable", "22", Some("0.4.0")),
        ("0.3.0", "unavailable", "22", Some("0.3.0")),
        ("", "{\"vers\":\"1.0.0\",\"yanked\":true}", "0", None),
        ("", "invalid response", "0", None),
        ("", available, "22", None),
        ("0.4.0-rc.1", available, "0", None),
    ] {
        let file =
            std::env::temp_dir().join(format!("fastmash-installer-env-{}", std::process::id()));
        let output = std::process::Command::new("bash")
            .args(["-e", "-o", "pipefail", "-c", &script])
            .env("REQUESTED_VERSION", requested)
            .env("REGISTRY_FIXTURE", registry)
            .env("REGISTRY_STATUS", status)
            .env("GITHUB_REPOSITORY", "pederbe/fastmash")
            .env("GITHUB_ENV", &file)
            .output()
            .unwrap();
        let selected = fs::read_to_string(&file).ok();
        if selected.is_some() {
            fs::remove_file(&file).unwrap();
        }
        assert_eq!(output.status.success(), expected.is_some(), "{output:?}");
        assert_eq!(
            selected,
            expected.map(|version| format!("VERSION={version}\n"))
        );
    }
}

#[test]
fn installed_evidence_upload_recovers_once_without_hiding_failure() {
    let workflow = fs::read_to_string(root().join(".github/workflows/macos-install.yml")).unwrap();
    let (_, uploads) = workflow
        .split_once("      - name: Retain installation and hosted comparison evidence\n")
        .unwrap();
    let (first, retry) = uploads
        .split_once("      - name: Retry installation evidence upload\n")
        .expect("an artifact service failure must get one retry");
    assert!(first.contains("        id: installation-evidence\n"));
    assert!(first.contains("        if: always()\n"));
    assert!(first.contains("        continue-on-error: true\n"));
    assert!(retry.contains(
        "        if: ${{ !cancelled() && steps.installation-evidence.outcome == 'failure' }}\n"
    ));
    assert!(
        !retry.contains("continue-on-error:"),
        "a second failure must fail CI"
    );
    assert!(retry.contains("          overwrite: true\n"));
    // Both attempts must retain the same evidence with the same pinned action.
    let action = first.lines().find(|line| line.contains("uses:")).unwrap();
    assert!(retry.lines().any(|line| line == action));
    for line in first
        .split_once("        with:\n")
        .unwrap()
        .1
        .lines()
        .take_while(|line| line.starts_with("          "))
    {
        assert!(
            retry.lines().any(|other| other == line),
            "missing retry input: {line}"
        );
    }
    assert_eq!(retry.matches("uses: actions/upload-artifact@").count(), 1);
}

#[test]
fn required_ci_waits_for_every_job_and_every_pr_workflow() {
    let workflow = workflow();
    let jobs = workflow.split_once("\njobs:\n").unwrap().1;
    let job_names: BTreeSet<_> = jobs
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter(|line| !line.starts_with(' ') && line.ends_with(':'))
        .map(|line| line.trim_end_matches(':'))
        .filter(|name| *name != "ci")
        .collect();
    let gate = jobs.split_once("  ci:\n").unwrap().1;
    let dependencies: BTreeSet<_> = gate
        .lines()
        .find_map(|line| line.strip_prefix("    needs: ["))
        .unwrap()
        .trim_end_matches(']')
        .split(", ")
        .collect();
    assert_eq!(dependencies, job_names, "CI must wait for every job");
    assert!(gate.contains("    if: always()\n"));
    assert!(gate.contains("RESULTS: ${{ join(needs.*.result, ' ') }}"));

    for entry in fs::read_dir(root().join(".github/workflows")).unwrap() {
        let path = entry.unwrap().path();
        let content = fs::read_to_string(&path).unwrap();
        if content.contains("\n  pull_request:") {
            assert_eq!(path.file_name().unwrap(), "ci.yml");
        }
    }
}

#[cfg(unix)]
#[test]
fn actual_ci_step_rejects_failed_cancelled_or_missing_results() {
    let workflow = workflow();
    let gate = workflow.split_once("\n  ci:\n").unwrap().1;
    let script = gate.split_once("        run: |\n").unwrap().1;
    let script = script
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    for (results, expected) in [
        ("success success", true),
        ("success skipped success", true),
        ("success failure skipped", false),
        ("success cancelled skipped", false),
        ("success timed_out", false),
        ("", false),
    ] {
        let output = std::process::Command::new("bash")
            .args(["-ec", &script])
            .env("RESULTS", results)
            .env("CHANGES_RESULT", "success")
            .env("DOCUMENTATION_RESULT", "success")
            .output()
            .unwrap();
        assert_eq!(output.status.success(), expected, "results: {results:?}");
    }
}

#[cfg(unix)]
#[test]
fn required_website_check_cannot_be_skipped() {
    let workflow = workflow();
    let gate = workflow.split_once("\n  ci:\n").unwrap().1;
    let script = gate.split_once("        run: |\n").unwrap().1;
    let script = script
        .lines()
        .map(|line| line.strip_prefix("          ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n");
    for (changes, documentation, expected) in [
        ("success", "success", true),
        ("success", "skipped", false),
        ("success", "failure", false),
        ("success", "cancelled", false),
        ("skipped", "success", false),
    ] {
        let result = std::process::Command::new("bash")
            .args(["-ec", &script])
            .env("RESULTS", "success skipped")
            .env("CHANGES_RESULT", changes)
            .env("DOCUMENTATION_RESULT", documentation)
            .status()
            .unwrap();
        assert_eq!(result.success(), expected, "{changes}/{documentation}");
    }
}
