//! Compare installed package documents with independently supplied host bytes.
use crate::installed::Target;
use crate::receipt::Receipts;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Output;

const NAMES: [&str; 5] = [
    "README.md",
    "CHANGELOG.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "THIRD-PARTY-LICENSES.md",
];

pub(crate) struct ExpectedDocuments(Vec<(&'static str, Vec<u8>)>);

impl ExpectedDocuments {
    pub(crate) fn load(directory: &Path) -> io::Result<Self> {
        let directory = fs::canonicalize(directory).map_err(|error| {
            io::Error::other(format!(
                "cannot open expected package documents {}: {error}",
                directory.display()
            ))
        })?;
        let mut documents = Vec::new();
        for name in NAMES {
            let requested = directory.join(name);
            let path = fs::canonicalize(&requested).map_err(|error| {
                io::Error::other(format!(
                    "cannot open expected package document {name}: {error}"
                ))
            })?;
            let bytes = fs::read(path).map_err(|error| {
                io::Error::other(format!(
                    "cannot read expected package document {name}: {error}"
                ))
            })?;
            documents.push((name, bytes));
        }
        Ok(Self(documents))
    }

    pub(crate) fn verify(
        &self,
        target: Target<'_>,
        debian: bool,
        receipts: &mut Receipts,
    ) -> io::Result<()> {
        let mut command = if debian {
            let mut command = target.command("dpkg-query");
            command.args(["-L", "fastmash"]);
            command
        } else {
            let mut command = target.command("rpm");
            command.args(["-ql", "fastmash"]);
            command
        };
        let listed = receipts.run("notice-paths", &mut command, b"")?;
        if !listed.status.success() {
            return Err(io::Error::other(format!(
                "cannot list installed package documents ({})",
                listed.status
            )));
        }
        let paths = document_paths(&listed.stdout)?;
        for ((name, expected), path) in self.0.iter().zip(paths) {
            let output = receipts.run(
                "notice-bytes",
                target.command("cat").arg("--").arg(path),
                b"",
            )?;
            exact_document(name, &output, expected)?;
        }
        Ok(())
    }
}

fn document_paths(listing: &[u8]) -> io::Result<Vec<PathBuf>> {
    let listing = std::str::from_utf8(listing)
        .map_err(|_| io::Error::other("installed package document paths are not UTF-8"))?;
    let mut paths = Vec::new();
    for name in NAMES {
        let mut selected = None;
        for path in listing.lines() {
            if Path::new(path).file_name() == Some(OsStr::new(name)) {
                if !Path::new(path).is_absolute() || path.split('/').any(|part| part == "..") {
                    return Err(io::Error::other(format!(
                        "invalid installed package document path for {name}"
                    )));
                }
                if selected.replace(PathBuf::from(path)).is_some() {
                    return Err(io::Error::other(format!(
                        "ambiguous installed package document path for {name}"
                    )));
                }
            }
        }
        paths.push(selected.ok_or_else(|| {
            io::Error::other(format!("installed package document is not listed: {name}"))
        })?);
    }
    Ok(paths)
}

fn exact_document(name: &str, output: &Output, expected: &[u8]) -> io::Result<()> {
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "cannot read installed package document {name} ({})",
            output.status
        )));
    }
    if output.stdout != expected || !output.stderr.is_empty() {
        return Err(io::Error::other(format!(
            "installed package document differs from expected bytes: {name}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ExpectedDocuments, NAMES, document_paths, exact_document};
    use std::fs;
    use std::os::unix::process::ExitStatusExt;
    use std::path::PathBuf;
    use std::process::{ExitStatus, Output};
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Directory(PathBuf);

    impl Directory {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "fastmash-expected-documents-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Directory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn package_document_paths_require_every_unique_absolute_name() {
        let listing = NAMES
            .map(|name| format!("/observed/legal/{name}\n"))
            .concat();
        let paths = document_paths(listing.as_bytes()).unwrap();
        assert_eq!(paths[2], PathBuf::from("/observed/legal/LICENSE-MIT"));
        assert!(
            document_paths(
                listing
                    .replace("/observed/legal/README.md\n", "")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(document_paths(format!("{listing}/other/README.md\n").as_bytes()).is_err());
        assert!(
            document_paths(
                listing
                    .replace("/observed/legal/README.md", "relative/README.md")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(
            document_paths(
                listing
                    .replace("/observed/legal/README.md", "/observed/../README.md")
                    .as_bytes()
            )
            .is_err()
        );
        assert!(document_paths(b"\xff").is_err());
    }

    #[test]
    fn package_notice_bytes_require_success_and_exact_streams() {
        let mut output = Output {
            status: ExitStatus::from_raw(0),
            stdout: b"notice\0\xff\n".to_vec(),
            stderr: Vec::new(),
        };
        assert!(exact_document("LICENSE-MIT", &output, b"notice\0\xff\n").is_ok());
        assert!(exact_document("LICENSE-MIT", &output, b"notice\0\xff").is_err());
        output.stderr.push(b'x');
        assert!(exact_document("LICENSE-MIT", &output, b"notice\0\xff\n").is_err());
        output.stderr.clear();
        output.status = ExitStatus::from_raw(1 << 8);
        assert!(exact_document("LICENSE-MIT", &output, b"notice\0\xff\n").is_err());
    }

    #[test]
    fn expected_package_documents_are_read_eagerly_with_scoped_input_errors() {
        let directory = Directory::new();
        for name in NAMES {
            fs::write(directory.0.join(name), name.as_bytes()).unwrap();
        }
        let loaded = ExpectedDocuments::load(&directory.0).unwrap();
        assert_eq!(loaded.0[4].1, b"THIRD-PARTY-LICENSES.md");
        fs::remove_file(directory.0.join(NAMES[4])).unwrap();
        let error = ExpectedDocuments::load(&directory.0).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("expected package document THIRD-PARTY-LICENSES.md")
        );
        fs::create_dir(directory.0.join(NAMES[4])).unwrap();
        let error = ExpectedDocuments::load(&directory.0).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("expected package document THIRD-PARTY-LICENSES.md")
        );
    }
}
