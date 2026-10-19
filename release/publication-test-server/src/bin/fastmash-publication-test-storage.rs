//! Local storage for the publication fixture's existing s3 sync/cp calls.
#![forbid(unsafe_code)]

#[cfg(unix)]
mod unix {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode, Stdio};

    #[derive(Clone, Copy, PartialEq)]
    enum Operation {
        Sync,
        Copy,
    }

    struct Adapter {
        operation: Operation,
        excludes: Vec<OsString>,
        live: OsString,
        count: PathBuf,
        stop_after: OsString,
    }

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }

    fn required_env(name: &str) -> io::Result<OsString> {
        env::var_os(name).ok_or_else(|| invalid(&format!("{name} is required")))
    }

    impl Adapter {
        fn write_object(&self, source: &Path, path: &OsStr) -> io::Result<bool> {
            // The uploader uses literal paths and single trailing-asterisk prefixes.
            if self.excludes.iter().any(|pattern| {
                let bytes = pattern.as_bytes();
                bytes.strip_suffix(b"*").map_or_else(
                    || path.as_bytes() == bytes,
                    |prefix| path.as_bytes().starts_with(prefix),
                )
            }) {
                return Ok(false);
            }
            let mut destination = self.live.clone();
            destination.push("/");
            destination.push(path);
            let destination = PathBuf::from(destination);
            if self.operation == Operation::Sync {
                let bytes = fs::read(source)?;
                if fs::read(&destination).is_ok_and(|live| live == bytes) {
                    return Ok(false);
                }
            }
            fs::create_dir_all(destination.parent().unwrap())?;
            let mut pending = destination.as_os_str().to_owned();
            pending.push(".pending");
            let pending = PathBuf::from(pending);
            if let Err(error) =
                fs::copy(source, &pending).and_then(|_| fs::rename(&pending, &destination))
            {
                let _ = fs::remove_file(&pending);
                return Err(error);
            }
            // The complete object is visible before counter I/O or interruption.
            let count = fs::read_to_string(&self.count)?
                .trim()
                .parse::<u64>()
                .map_err(|_| invalid("invalid publication count"))?;
            let next = count
                .checked_add(1)
                .ok_or_else(|| invalid("publication count overflow"))?
                .to_string();
            fs::write(&self.count, format!("{next}\n"))?;
            Ok(self.stop_after == OsStr::new(&next))
        }
    }

    fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                collect_files(&entry.path(), files)?;
            } else if kind.is_file() {
                files.push(entry.path());
            }
        }
        Ok(())
    }

    fn sorted_files() -> io::Result<Vec<u8>> {
        let mut files = Vec::new();
        collect_files(Path::new("."), &mut files)?;
        // Keep the inherited locale's actual sort -z ordering and raw path bytes.
        let mut child = Command::new("sort")
            .arg("-z")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut input = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || -> io::Result<()> {
            for path in files {
                input.write_all(path.as_os_str().as_bytes())?;
                input.write_all(&[0])?;
            }
            Ok(())
        });
        let output = child.wait_with_output();
        let written = writer
            .join()
            .map_err(|_| io::Error::other("sort input writer panicked"))?;
        let output = output?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "sort failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        written?;
        Ok(output.stdout)
    }

    pub(super) fn run() -> io::Result<ExitCode> {
        let mut args = env::args_os().skip(1);
        if args.next().as_deref() != Some(OsStr::new("s3")) {
            return Err(invalid("expected s3 sync or s3 cp"));
        }
        let operation = match args.next().as_deref() {
            Some(value) if value == "sync" => Operation::Sync,
            Some(value) if value == "cp" => Operation::Copy,
            _ => return Err(invalid("expected s3 sync or s3 cp")),
        };
        let mut excludes = Vec::new();
        let mut positional = Vec::new();
        while let Some(arg) = args.next() {
            match arg.as_bytes() {
                b"--only-show-errors" => {}
                b"--cache-control" => {
                    args.next()
                        .ok_or_else(|| invalid("--cache-control needs a value"))?;
                }
                b"--exclude" => {
                    let pattern = args
                        .next()
                        .ok_or_else(|| invalid("--exclude needs a value"))?;
                    let bytes = pattern.as_bytes();
                    let literal = bytes.strip_suffix(b"*").unwrap_or(bytes);
                    if literal.iter().any(|byte| b"*?[\\".contains(byte)) {
                        return Err(invalid("unsupported exclusion pattern"));
                    }
                    excludes.push(pattern);
                }
                _ => positional.push(arg),
            }
        }
        if positional.len() < 2 {
            return Err(invalid("source and test-bucket destination are required"));
        }
        let destination = positional[1].as_bytes();
        if !destination.starts_with(b"s3://test-bucket") {
            return Err(invalid("destination must use test-bucket"));
        }
        let adapter = Adapter {
            operation,
            excludes,
            live: required_env("PUBLICATION_LIVE")?,
            count: required_env("PUBLICATION_COUNT")?.into(),
            stop_after: env::var_os("PUBLICATION_STOP_AFTER").unwrap_or_else(|| "0".into()),
        };
        if operation == Operation::Sync {
            for path in sorted_files()?
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
            {
                let relative = path.strip_prefix(b"./").unwrap();
                if adapter.write_object(
                    Path::new(OsStr::from_bytes(path)),
                    OsStr::from_bytes(relative),
                )? {
                    return Ok(ExitCode::from(75));
                }
            }
        } else {
            let relative = destination
                .strip_prefix(b"s3://test-bucket/")
                .unwrap_or(destination);
            if adapter.write_object(Path::new(&positional[0]), OsStr::from_bytes(relative))? {
                return Ok(ExitCode::from(75));
            }
        }
        Ok(ExitCode::SUCCESS)
    }
}

#[cfg(unix)]
fn main() -> std::process::ExitCode {
    match unix::run() {
        Ok(status) => status,
        Err(error) => {
            eprintln!("publication storage: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(not(unix))]
fn main() -> std::process::ExitCode {
    eprintln!("publication storage requires Unix paths and sort -z");
    std::process::ExitCode::FAILURE
}
