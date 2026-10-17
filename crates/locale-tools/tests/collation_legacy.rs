#![forbid(unsafe_code)]
#![cfg(unix)]

use fastmash_locale_tools::collation::Table;
use serde_json::json;
use std::ffi::OsString;
use std::fs::{self, DirBuilder, File};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CANDIDATES: &[u8] = include_bytes!("../../../data/locales/collation-candidates.json");
const VERIFICATION: &[u8] = include_bytes!("../../../data/locales/collation-verification.json");
const LEGACY: &[u8] = include_bytes!("../../../scripts/verify_collation_locales.py");
const CHECK: &str = r#"#![forbid(unsafe_code)]
#[path = "crates/cli/src/collation_locales.rs"]
mod old;
#[path = "new_generated.rs"]
mod new;
fn main() {
    assert_eq!(old::COLLATION, new::COLLATION);
    println!("{} rows match", old::COLLATION.len());
}
"#;

fn selected_path(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name)
            .unwrap_or_else(|| panic!("the opt-in qualification requires {name}")),
    );
    assert!(path.is_absolute(), "{name} must be an absolute path");
    path
}

// The opt-in caller supplies the whole process-tree deadline. These fixed
// legacy/compiler checks run inside it, without a second process framework.
fn run(directory: &Path, name: &str, program: &Path, arguments: &[OsString]) {
    fs::write(
        directory.join(format!("{name}.command.json")),
        serde_json::to_vec_pretty(&json!({
            "program_bytes": program.as_os_str().as_bytes(),
            "arguments_bytes": arguments.iter().map(|arg| arg.as_bytes()).collect::<Vec<_>>(),
            "working_directory_bytes": directory.as_os_str().as_bytes(),
            "environment": {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "TZ": "UTC"},
            "stdin": "null",
            "requires_outer_process_tree_deadline": true,
        }))
        .unwrap(),
    )
    .unwrap();
    let status = Command::new(program)
        .args(arguments)
        .current_dir(directory)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            File::create(directory.join(format!("{name}.stdout"))).unwrap(),
        ))
        .stderr(Stdio::from(
            File::create(directory.join(format!("{name}.stderr"))).unwrap(),
        ))
        .status()
        .unwrap_or_else(|error| {
            fs::write(
                directory.join(format!("{name}.spawn-error")),
                error.to_string(),
            )
            .unwrap();
            panic!(
                "cannot run {name}: {error}. Evidence: {}",
                directory.display()
            );
        });
    fs::write(
        directory.join(format!("{name}.wait.json")),
        serde_json::to_vec_pretty(&json!({
            "raw_status": status.into_raw(), "exit_code": status.code(), "signal": status.signal(),
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(
        status.success(),
        "{name} failed. Evidence: {}",
        directory.display()
    );
}

#[test]
#[ignore = "requires explicit Python/Rust compiler paths, new evidence directory and an outer process-tree deadline"]
fn retained_table_modes_match_legacy_compiled_values() {
    let python = selected_path("FASTMASH_COLLATION_LEGACY_PYTHON");
    let rustc = selected_path("FASTMASH_COLLATION_LEGACY_RUSTC");
    let evidence = selected_path("FASTMASH_COLLATION_LEGACY_EVIDENCE");
    DirBuilder::new().mode(0o700).create(&evidence).unwrap();
    let script = evidence.join("legacy.py");
    fs::write(&script, LEGACY).unwrap();

    for (name, variant, expected) in [
        ("final", None, 194),
        ("candidate-0", Some(0), 319),
        ("candidate-1", Some(1), 319),
        ("candidate-minus-1", Some(-1), 319),
        ("candidate-minus-2", Some(-2), 319),
    ] {
        let directory = evidence.join(name);
        DirBuilder::new().mode(0o700).create(&directory).unwrap();
        fs::create_dir_all(directory.join("data/locales")).unwrap();
        fs::create_dir_all(directory.join("crates/cli/src")).unwrap();
        fs::write(
            directory.join("data/locales/collation-candidates.json"),
            CANDIDATES,
        )
        .unwrap();
        fs::write(
            directory.join("data/locales/collation-verification.json"),
            VERIFICATION,
        )
        .unwrap();
        let table = match variant {
            Some(variant) => Table::from_candidates(CANDIDATES, variant),
            None => Table::from_verification(VERIFICATION),
        }
        .unwrap();
        assert_eq!(table.rows().len(), expected);
        fs::write(directory.join("new_generated.rs"), table.rust_source()).unwrap();
        fs::write(directory.join("old_new_check.rs"), CHECK).unwrap();

        let mut arguments = vec!["-I".into(), script.as_os_str().into(), "table".into()];
        if let Some(variant) = variant {
            arguments.extend(["--variant".into(), variant.to_string().into()]);
        }
        run(&directory, "legacy", &python, &arguments);
        assert_eq!(
            fs::read(directory.join("legacy.stdout")).unwrap(),
            format!("{expected} locales in crates/cli/src/collation_locales.rs\n").as_bytes(),
        );
        assert!(
            fs::read(directory.join("legacy.stderr"))
                .unwrap()
                .is_empty()
        );

        let executable = directory.join("compiled-values");
        run(
            &directory,
            "compile",
            &rustc,
            &[
                "--crate-name".into(),
                "collation_legacy_check".into(),
                "--edition".into(),
                "2024".into(),
                "--forbid".into(),
                "unsafe_code".into(),
                directory.join("old_new_check.rs").into_os_string(),
                "-o".into(),
                executable.as_os_str().into(),
            ],
        );
        assert!(
            fs::read(directory.join("compile.stderr"))
                .unwrap()
                .is_empty()
        );
        run(&directory, "compare", &executable, &[]);
        assert_eq!(
            fs::read(directory.join("compare.stdout")).unwrap(),
            format!("{expected} rows match\n").as_bytes(),
        );
        assert!(
            fs::read(directory.join("compare.stderr"))
                .unwrap()
                .is_empty()
        );
    }
    fs::write(evidence.join("passed.txt"), b"Five retained table projections match legacy compiled values. No native collation verification or Python retirement is established.\n").unwrap();
    println!(
        "Retained collation table parity evidence: {}",
        evidence.display()
    );
}
