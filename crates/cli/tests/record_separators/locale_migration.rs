//! Decision evidence: record reference and current CLI behavior without choosing a policy.
use super::*;

#[test]
#[ignore = "requires named GNU reference, generated locales and fresh evidence directory"]
fn locale_migration_inventory() {
    let reference = std::env::var_os("FASTMASH_RECORD_REFERENCE").unwrap();
    let evidence = std::path::PathBuf::from(std::env::var_os("FASTMASH_RECORD_EVIDENCE").unwrap());
    let locales = std::env::var_os("FASTMASH_RECORD_LOCALE_PATH").unwrap();
    fs::create_dir(&evidence).unwrap();
    let environments = [
        vec![("LC_ALL", "C")],
        vec![("LC_ALL", "POSIX")],
        vec![("LANG", "C.UTF-8")],
        vec![("LANG", "en_US.UTF-8")],
        vec![("LANG", "de_DE.UTF-8")],
        vec![("LANG", "en_US.UTF-8"), ("LC_COLLATE", "C")],
        vec![
            ("LANG", "de_DE.UTF-8"),
            ("LC_CTYPE", "C"),
            ("LC_MESSAGES", "C"),
            ("LC_COLLATE", "C"),
        ],
        vec![("LANG", "de_DE.UTF-8"), ("LC_ALL", "C")],
        vec![
            ("LANG", "en_US.UTF-8"),
            ("LC_CTYPE", "C"),
            ("LC_MESSAGES", "C"),
            ("LC_NUMERIC", "C"),
        ],
    ];
    let jobs = [
        ("decimal-sum", vec!["sum", "1"], "1.25\n2.5\n"),
        ("comma-sum", vec!["sum", "1"], "1,25\n2,5\n"),
        (
            "grouped-number-output",
            vec!["--format=%'.2f", "sum", "1"],
            "1234\n",
        ),
        (
            "ascii-identifiers",
            vec!["-sg1", "count", "2"],
            "a_1\t1\na-1\t1\nA1\t1\na1\t1\nZ\t1\nz\t1\n",
        ),
        (
            "unicode-identifiers",
            vec!["-sg1", "count", "2"],
            "z\t1\nä\t1\na\t1\nÄ\t1\né\t1\nE\t1\n",
        ),
        (
            "unicode-fold-sort",
            vec!["-isg1", "count", "2"],
            "ä\t1\nÄ\t1\nz\t1\nZ\t1\nß\t1\nSS\t1\n",
        ),
        (
            "unicode-adjacent",
            vec!["-ig1", "count", "2"],
            "ä\t1\nÄ\t1\na\t1\nA\t1\n",
        ),
        (
            "unicode-unique",
            vec!["-i", "unique", "1"],
            "ä\nÄ\nz\nZ\nß\nSS\n",
        ),
        (
            "external-sort-means",
            vec!["-sg1", "harmmean", "2"],
            "z\t2\nä\t2\na\t2\nÄ\t2\n",
        ),
        ("unicode-whitespace", vec!["-W", "count", "2"], "a\u{a0}2\n"),
        ("invalid-number", vec!["sum", "1"], "bad\n"),
        ("invalid-format", vec!["--format=%Q", "sum", "1"], "1\n"),
    ];
    let mut summary =
        String::from("environment\tjob\treference_status\tcandidate_status\texact_match\n");
    for (env_index, environment) in environments.iter().enumerate() {
        let mut check = Command::new("locale");
        check
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LOCPATH", &locales)
            .envs(environment.iter().copied());
        let output = check.arg("charmap").output().unwrap();
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "locale unavailable: {environment:?}: {output:?}"
        );
        fs::write(
            evidence.join(format!("{env_index:02}.environment")),
            format!(
                "{environment:?}\nLOCPATH={locales:?}\ncharmap={:?}\n",
                output.stdout
            ),
        )
        .unwrap();
        for (job_index, (name, arguments, input)) in jobs.iter().enumerate() {
            let prefix = format!("{env_index:02}-{job_index:02}");
            let input_path = evidence.join(format!("{prefix}.input"));
            fs::write(&input_path, input).unwrap();
            fs::write(
                evidence.join(format!("{prefix}.args")),
                format!("{arguments:?}"),
            )
            .unwrap();
            let run = |binary: &OsStr, label: &str| {
                let output = Command::new(binary)
                    .arg0("fastmash")
                    .args(arguments)
                    .env_clear()
                    .env("PATH", "/usr/bin:/bin")
                    .env("LOCPATH", &locales)
                    .envs(environment.iter().copied())
                    .stdin(fs::File::open(&input_path).unwrap())
                    .output()
                    .unwrap();
                fs::write(
                    evidence.join(format!("{prefix}.{label}.stdout")),
                    &output.stdout,
                )
                .unwrap();
                fs::write(
                    evidence.join(format!("{prefix}.{label}.stderr")),
                    &output.stderr,
                )
                .unwrap();
                fs::write(
                    evidence.join(format!("{prefix}.{label}.status")),
                    output.status.to_string(),
                )
                .unwrap();
                output
            };
            let expected = run(&reference, "gnu");
            let actual = run(&candidate(), "candidate");
            summary.push_str(&format!(
                "{env_index}\t{name}\t{}\t{}\t{}\n",
                expected.status,
                actual.status,
                expected.status == actual.status
                    && expected.stdout == actual.stdout
                    && expected.stderr == actual.stderr
            ));
        }
    }
    fs::write(evidence.join("summary.tsv"), summary).unwrap();
}
