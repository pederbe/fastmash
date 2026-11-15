//! Execute the documented workflows in native PowerShell, checking bytes in Rust.
#![cfg(windows)]
#![forbid(unsafe_code)]
use fastmash_corpus_driver::windows_output;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::Duration,
};

const FILE_INPUT: &str = "& $env:ComSpec /d /v:off /s /c '\"\"%FASTMASH_TEST_BINARY%\" reverse < \"%FASTMASH_TEST_INPUT%\"\"' > $env:FASTMASH_TEST_OUTPUT\nexit $LASTEXITCODE\n";

struct Workflow {
    root: PathBuf,
    binary: PathBuf,
    shell: PathBuf,
}
impl Workflow {
    fn execute(
        &self,
        label: &str,
        script: &str,
        bytes: &[u8],
        locale: &str,
        delayed_parent: bool,
    ) -> (Output, Vec<u8>) {
        let input = self.root.join("quoted ' é %NAME% & ! input.tsv");
        let output = self.root.join(format!("{label} ' é %NAME% & ! output.tsv"));
        fs::write(&input, bytes).unwrap();
        let script_path = self.root.join("workflow.ps1");
        fs::write(&script_path, script).unwrap();
        let mut command = if delayed_parent {
            let mut command = Command::new(std::env::var_os("ComSpec").unwrap());
            command.args(["/d", "/v:on", "/s", "/c"]).arg(format!(
                "\"\"{}\" -NoProfile -NonInteractive -File \"{}\"\"",
                self.shell.display(),
                script_path.display()
            ));
            command
        } else {
            let mut command = Command::new(&self.shell);
            command
                .args(["-NoProfile", "-NonInteractive", "-File"])
                .arg(script_path);
            command
        };
        command
            .env("FASTMASH_TEST_BINARY", &self.binary)
            .env("FASTMASH_TEST_INPUT", &input)
            .env("FASTMASH_TEST_OUTPUT", &output)
            .env("NAME", "EXPANDED-WRONG-PATH")
            .env("LC_ALL", locale);
        let observed = windows_output(command, Duration::from_secs(30)).unwrap();
        let bytes = fs::read(output).unwrap_or_default();
        (observed, bytes)
    }
}
impl Drop for Workflow {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
fn success(observed: &Output) {
    assert!(observed.status.success(), "{observed:?}");
    assert!(observed.stderr.is_empty(), "{observed:?}");
}

#[test]
#[ignore = "requires FASTMASH_TEST_BINARY, FASTMASH_PWSH_74 and FASTMASH_PWSH_CURRENT"]
fn native_powershell_baseline_and_current_preserve_workflow_contracts() {
    let binary = PathBuf::from(std::env::var_os("FASTMASH_TEST_BINARY").unwrap())
        .canonicalize()
        .unwrap();
    for (variable, baseline) in [("FASTMASH_PWSH_74", true), ("FASTMASH_PWSH_CURRENT", false)] {
        let shell = PathBuf::from(std::env::var_os(variable).unwrap());
        let root = std::env::temp_dir().join(format!(
            "fastmash-powershell-{}-{}",
            std::process::id(),
            if baseline { "74" } else { "current" }
        ));
        fs::create_dir(&root).unwrap();
        let workflow = Workflow {
            root,
            binary: binary.clone(),
            shell,
        };
        let (version, _) = workflow.execute(
            "version",
            "$PSVersionTable.PSVersion.ToString()",
            b"",
            "C",
            false,
        );
        success(&version);
        let version_text = String::from_utf8(version.stdout).unwrap();
        if baseline {
            assert!(version_text.starts_with("7.4."), "{version_text}");
        } else {
            assert!(version_text.starts_with("7."), "{version_text}");
        }
        let bytes = b"a\0\r\x1a\xff\xc3\xa9\n";
        for delayed_parent in [false, true] {
            let (observed, actual) = workflow.execute(
                if delayed_parent { "delayed" } else { "file" },
                FILE_INPUT,
                bytes,
                "C",
                delayed_parent,
            );
            success(&observed);
            assert_eq!(actual, bytes, "file-input path or bytes changed");
        }
        let (observed, actual) = workflow.execute("native-pipe", "& $env:ComSpec /d /v:off /s /c '\"\"%FASTMASH_TEST_BINARY%\" reverse < \"%FASTMASH_TEST_INPUT%\"\"' | & $env:FASTMASH_TEST_BINARY reverse > $env:FASTMASH_TEST_OUTPUT\nexit $LASTEXITCODE", bytes, "C", false);
        success(&observed);
        assert_eq!(actual, bytes);
        let (observed, actual) = workflow.execute("text-pipe", "'1', '2' | & $env:FASTMASH_TEST_BINARY reverse > $env:FASTMASH_TEST_OUTPUT\nexit $LASTEXITCODE", b"", "C", false);
        success(&observed);
        assert_eq!(actual, b"1\r\n2\r\n");
        let (observed, actual) = workflow.execute("quoted-argument", "'a' | & $env:FASTMASH_TEST_BINARY --no-strict '--filler=x \"quote\" y' cut 2 > $env:FASTMASH_TEST_OUTPUT\nexit $LASTEXITCODE", b"", "C", false);
        success(&observed);
        assert_eq!(actual, b"x \"quote\" y\n");
        let (observed, actual) = workflow.execute("locale", "& $env:ComSpec /d /v:off /s /c '\"\"%FASTMASH_TEST_BINARY%\" sum 1 < \"%FASTMASH_TEST_INPUT%\"\"' > $env:FASTMASH_TEST_OUTPUT\nexit $LASTEXITCODE", b"1,25\n2,5\n", "de_DE.UTF-8", false);
        success(&observed);
        assert_eq!(actual, b"3,75\n");
        for (label, script, status) in [
            (
                "command-error",
                "& $env:FASTMASH_TEST_BINARY sum 0\nexit $LASTEXITCODE",
                1,
            ),
            (
                "refusal",
                "& $env:FASTMASH_TEST_BINARY unsupported-operation 1\nexit $LASTEXITCODE",
                77,
            ),
            (
                "missing-input",
                "$env:FASTMASH_TEST_INPUT += '.missing'\n& $env:ComSpec /d /v:off /s /c '\"\"%FASTMASH_TEST_BINARY%\" sum 1 < \"%FASTMASH_TEST_INPUT%\"\"' > $env:FASTMASH_TEST_OUTPUT\nexit $LASTEXITCODE",
                1,
            ),
        ] {
            let (observed, actual) = workflow.execute(label, script, b"", "C", false);
            assert_eq!(observed.status.code(), Some(status), "{observed:?}");
            assert!(!observed.stderr.is_empty());
            assert!(actual.is_empty());
        }
        if let Some(evidence) = std::env::var_os("FASTMASH_POWERSHELL_EVIDENCE") {
            let evidence = Path::new(&evidence);
            fs::create_dir_all(evidence).unwrap();
            fs::write(evidence.join(if baseline { "baseline.txt" } else { "current.txt" }), format!("PowerShell {}: raw file, native pipeline, text pipeline, literal path and argument quoting, delayed expansion, explicit locale and exit propagation passed\n", version_text.trim())).unwrap();
        }
    }
}
