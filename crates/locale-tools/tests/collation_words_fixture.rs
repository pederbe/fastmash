#![forbid(unsafe_code)]
#![cfg(unix)]

use serde_json::{Value, json};
use std::fs::{self, DirBuilder};
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

struct Run {
    root: PathBuf,
    observations: PathBuf,
}

impl Run {
    fn new(label: &str, replies: Option<Value>) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "collation-words-{label}-{}-{stamp}",
            std::process::id()
        ));
        DirBuilder::new().mode(0o700).create(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let observations = root.join("observations");
        fs::create_dir(&observations).unwrap();
        let mut config = json!({"observations": observations.as_os_str().as_bytes()});
        if let Some(replies) = replies {
            config["replies"] = replies;
        }
        fs::write(
            root.join("observer-config.json"),
            serde_json::to_vec(&config).unwrap(),
        )
        .unwrap();
        Self { root, observations }
    }

    fn invoke(&self, role: &str, index: usize, input: &[u8]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_collation-words-fixture"));
        command.arg0(if role == "candidate" {
            "fastmash"
        } else {
            "datamash"
        });
        if role == "reference" {
            command.args(["--sort-cmd", "selected sort ' \n"]);
        }
        command
            .args(["-s", "-g", "1", "count", "1"])
            .current_dir(&self.root)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LC_ALL", "en_US.UTF-8")
            .env("LOCPATH", self.root.join("locales"))
            .env("TZ", "UTC")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // These short fixed inputs use the caller's outer process-tree bound,
        // as the other locale fixture tests do.
        let mut child = command.spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        let output = child.wait_with_output().unwrap();
        let prefix = self.root.join(format!("run-{role}-{index:02}"));
        fs::write(prefix.with_extension("stdout"), &output.stdout).unwrap();
        fs::write(prefix.with_extension("stderr"), &output.stderr).unwrap();
        fs::write(
            prefix.with_extension("status"),
            format!("{}\n", output.status),
        )
        .unwrap();
        output
    }

    fn assert_record(
        &self,
        role: &str,
        index: usize,
        input: &[u8],
        stdout: &[u8],
        reply: Option<(&[u8], u8)>,
    ) {
        let prefix = self.observations.join(format!("{role}-{index:02}"));
        assert_eq!(fs::read(prefix.with_extension("stdin")).unwrap(), input);
        assert_eq!(fs::read(prefix.with_extension("stdout")).unwrap(), stdout);
        let receipt = read_json(&prefix.with_extension("json"));
        assert_eq!(receipt["role"], role);
        assert_eq!(receipt["index"], index);
        let mut arguments = if role == "candidate" {
            vec![b"fastmash".to_vec()]
        } else {
            vec![
                b"datamash".to_vec(),
                b"--sort-cmd".to_vec(),
                b"selected sort ' \n".to_vec(),
            ]
        };
        arguments.extend([b"-s".as_slice(), b"-g", b"1", b"count", b"1"].map(<[u8]>::to_vec));
        assert_eq!(receipt["arguments"], json!(arguments));
        assert_eq!(
            receipt["working_directory"],
            json!(self.root.as_os_str().as_bytes())
        );
        assert_eq!(receipt["input_bytes"], input.len());
        assert_eq!(receipt["stdout_bytes"], stdout.len());
        let locpath = self.root.join("locales");
        assert_eq!(
            receipt["environment"],
            json!([
                [b"LC_ALL".as_slice(), b"en_US.UTF-8".as_slice()],
                [b"LOCPATH".as_slice(), locpath.as_os_str().as_bytes()],
                [b"PATH".as_slice(), b"/usr/bin:/bin".as_slice()],
                [b"TZ".as_slice(), b"UTC".as_slice()],
            ])
        );
        if let Some((stderr, status)) = reply {
            assert_eq!(fs::read(prefix.with_extension("stderr")).unwrap(), stderr);
            assert_eq!(
                receipt["configured_reply"],
                json!({"stderr_bytes": stderr.len(), "status": status})
            );
            assert_eq!(receipt.as_object().unwrap().len(), 8);
        } else {
            assert!(!prefix.with_extension("stderr").exists());
            assert!(receipt.get("configured_reply").is_none());
            assert_eq!(receipt.as_object().unwrap().len(), 7);
        }
        assert_eq!(
            fs::read_to_string(self.observations.join(format!("{role}.count"))).unwrap(),
            format!("{}\n", index + 1)
        );
    }

    fn finish(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}

fn read_json(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn observations_only_config_keeps_the_original_success_streams_and_receipts() {
    let run = Run::new("original", None);
    let input = "alpha\né\n".as_bytes();
    let stdout = "alpha\t1\né\t1\n".as_bytes();
    for role in ["candidate", "reference"] {
        let output = run.invoke(role, 0, input);
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(output.stdout, stdout);
        assert!(output.stderr.is_empty());
        run.assert_record(role, 0, input, stdout, None);
    }
    run.finish();
}

#[test]
fn role_specific_native_failures_advance_to_all_29_default_successes() {
    let candidate = b"\xffcan't \"quote\"\\\t\r\n\0";
    let reference_alphabet = "reference é\n".as_bytes();
    let reference_digits = b"\x80digits\x7f\n";
    let run = Run::new(
        "native-failures",
        Some(json!({
            "candidate": [{"stderr": candidate.as_slice(), "status": 77}, null],
            "reference": [
                {"stderr": reference_alphabet, "status": 0},
                {"stderr": reference_digits.as_slice(), "status": 23},
            ],
        })),
    );
    for index in 0..31 {
        let input = format!("word-{index:02}\né\n");
        let stdout = format!("word-{index:02}\t1\né\t1\n");
        for role in ["candidate", "reference"] {
            let reply: Option<(&[u8], u8)> = match (role, index) {
                ("candidate", 0) => Some((candidate, 77)),
                ("reference", 0) => Some((reference_alphabet, 0)),
                ("reference", 1) => Some((reference_digits, 23)),
                _ => None,
            };
            let output = run.invoke(role, index, input.as_bytes());
            assert_eq!(
                output.status.code(),
                Some(i32::from(reply.map_or(0, |r| r.1)))
            );
            assert_eq!(output.stdout, stdout.as_bytes());
            assert_eq!(output.stderr, reply.map_or(b"".as_slice(), |r| r.0));
            run.assert_record(role, index, input.as_bytes(), stdout.as_bytes(), reply);
        }
    }
    run.finish();
}
