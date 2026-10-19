use std::env;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

// Raw receipts are optional for ordinary callers and retained on both outcomes.
pub(crate) struct Receipts {
    directory: Option<PathBuf>,
    next: usize,
}

impl Receipts {
    pub(crate) fn from_env() -> io::Result<Self> {
        let directory = env::var_os("INSTALLATION_TEST_EVIDENCE").map(PathBuf::from);
        if let Some(path) = &directory {
            fs::DirBuilder::new().mode(0o700).create(path)?;
        }
        Ok(Self { directory, next: 0 })
    }

    pub(crate) fn run(
        &mut self,
        label: &str,
        command: &mut Command,
        input: &[u8],
    ) -> io::Result<Output> {
        self.next += 1;
        let prefix = self
            .directory
            .as_ref()
            .map(|path| path.join(format!("{:03}-{label}", self.next)));
        if let Some(path) = &prefix {
            fs::write(path.with_extension("command"), format!("{command:?}\n"))?;
            let mut argv = Vec::new();
            for argument in std::iter::once(command.get_program()).chain(command.get_args()) {
                argv.extend_from_slice(argument.as_encoded_bytes());
                argv.push(0);
            }
            fs::write(path.with_extension("argv"), argv)?;
            fs::write(path.with_extension("stdin"), input)?;
        }
        let child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        if let (Err(error), Some(path)) = (&child, &prefix) {
            fs::write(path.with_extension("spawn-error"), format!("{error}\n"))?;
        }
        let mut child = child?;
        let delivered = child.stdin.take().unwrap().write_all(input);
        let output = child.wait_with_output()?;
        if let Some(path) = &prefix {
            fs::write(path.with_extension("stdout"), &output.stdout)?;
            fs::write(path.with_extension("stderr"), &output.stderr)?;
            fs::write(
                path.with_extension("status"),
                format!("{}\n", output.status),
            )?;
            fs::write(path.with_extension("delivery"), format!("{delivered:?}\n"))?;
        }
        // A child's own failure remains primary when it closed its input early.
        if output.status.success() {
            delivered?;
        }
        Ok(output)
    }

    pub(crate) fn container_id(&self, image: &str, id: &str) -> io::Result<()> {
        if let Some(path) = &self.directory {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path.join("containers.txt"))?;
            writeln!(file, "{image}\t{id}")?;
        }
        Ok(())
    }
}

pub(crate) fn successful(output: &Output, label: &str) -> io::Result<()> {
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{label} failed ({})\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )))
    }
}
