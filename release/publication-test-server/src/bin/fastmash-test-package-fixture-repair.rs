//! Replay the repository builder against a partial, fixed-generation fixture.
#![forbid(unsafe_code)]

#[cfg(unix)]
mod unix {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs;
    use std::io;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use std::os::unix::process::ExitStatusExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, ExitCode, ExitStatus};

    fn invalid(message: &str) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message)
    }

    fn copy_contents(source: &Path, destination: &Path) -> io::Result<()> {
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            let kind = entry.file_type()?;
            if kind.is_dir() {
                fs::create_dir(&target)?;
                copy_contents(&entry.path(), &target)?;
                fs::set_permissions(&target, entry.metadata()?.permissions())?;
            } else if kind.is_symlink() {
                symlink(fs::read_link(entry.path())?, target)?;
            } else if kind.is_file() {
                fs::copy(entry.path(), target)?;
            } else {
                return Err(invalid("unsupported file in publication fixture"));
            }
        }
        Ok(())
    }

    fn reference_snapshot(repository: &Path) -> io::Result<PathBuf> {
        let mirrorlist = fs::read(repository.join("rpm/mirrorlist"))?;
        let generation = mirrorlist
            .trim_ascii_end()
            .strip_prefix(b"https://packages.fastmash.io/rpm/snapshots/")
            .and_then(|value| value.strip_suffix(b"/"))
            .filter(|value| value.len() == 64 && value.iter().all(u8::is_ascii_hexdigit))
            .ok_or_else(|| invalid("fixture mirrorlist has no metadata generation"))?;
        Ok(repository
            .join("rpm/snapshots")
            .join(OsStr::from_bytes(generation)))
    }

    fn find_primary(directory: &Path) -> io::Result<Option<PathBuf>> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let is_primary = |name: &OsStr| name.as_bytes().ends_with(b"-primary.xml");
            // Native indexers choose their compression format. The repair fixture
            // copies these bytes unchanged, so it need not assume or decode gzip.
            if is_primary(&name) || Path::new(&name).file_stem().is_some_and(is_primary) {
                return Ok(Some(entry.path()));
            }
            if entry.file_type()?.is_dir()
                && let Some(path) = find_primary(&entry.path())?
            {
                return Ok(Some(path));
            }
        }
        Ok(None)
    }

    struct PartialSnapshot {
        reference: PathBuf,
        primary: PathBuf,
        expected_primary: PathBuf,
    }

    impl PartialSnapshot {
        fn prepare(repository: &Path, partial: &Path) -> io::Result<Self> {
            let reference = reference_snapshot(repository)?;
            copy_contents(repository, partial)?;
            let snapshot = partial
                .join("rpm/snapshots")
                .join(reference.file_name().unwrap());
            let primary = find_primary(&snapshot.join("repodata"))?.ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "fixture snapshot has no primary metadata",
                )
            })?;
            let expected_primary = reference
                .join("repodata")
                .join(primary.file_name().unwrap());
            fs::remove_file(&primary)?;
            fs::remove_file(snapshot.join("repodata/repomd.xml.asc"))?;
            Ok(Self {
                reference,
                primary,
                expected_primary,
            })
        }

        fn restored(&self) -> io::Result<()> {
            if fs::read(&self.primary)? != fs::read(&self.expected_primary)? {
                return Err(io::Error::other(
                    "regenerated primary metadata differs from its frozen reference",
                ));
            }
            Ok(())
        }
    }

    fn frozen_indexer(reference: &Path, arguments: &[OsString]) -> io::Result<()> {
        let output = arguments
            .windows(2)
            .find(|pair| pair[0] == "--outputdir")
            .map(|pair| PathBuf::from(&pair[1]))
            .ok_or_else(|| invalid("frozen indexer requires --outputdir DIRECTORY"))?;
        fs::create_dir_all(&output)?;
        copy_contents(reference, &output)?;
        fs::remove_file(output.join("repodata/repomd.xml.asc"))
    }

    fn native_exit(status: ExitStatus) -> ExitCode {
        let code = status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0));
        ExitCode::from(u8::try_from(code).unwrap_or(1))
    }

    fn repair(work: &Path, repository_script: &Path) -> io::Result<ExitCode> {
        let partial = work.join("partial");
        // This tool may remove only the partial directory that it created.
        fs::create_dir(&partial)?;
        let result = (|| {
            let snapshot = PartialSnapshot::prepare(&work.join("new"), &partial)?;
            let bin = work.join("build/bin");
            fs::create_dir_all(&bin)?;
            symlink(env::current_exe()?, bin.join("createrepo_c"))?;
            let mut paths = vec![bin];
            paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
            let path = env::join_paths(paths).map_err(|error| invalid(&error.to_string()))?;
            let status = Command::new("bash")
                .arg(repository_script)
                .arg(&partial)
                .arg(work.join("test-key.asc"))
                .arg(work.join("build/fastmash_0.2.0_amd64.deb"))
                .env("PUBLICATION_REFERENCE", &snapshot.reference)
                .env("PATH", path)
                .status()?;
            if !status.success() {
                return Ok(native_exit(status));
            }
            snapshot.restored()?;
            Ok(ExitCode::SUCCESS)
        })();
        let cleanup = fs::remove_dir_all(&partial);
        match result {
            Ok(code) if code == ExitCode::SUCCESS => {
                cleanup?;
                println!("regeneration completes a partially uploaded snapshot");
                Ok(code)
            }
            other => {
                if let Err(error) = cleanup {
                    eprintln!("fixture repair cleanup: {error}");
                }
                other
            }
        }
    }

    pub(super) fn run() -> io::Result<ExitCode> {
        let mut args = env::args_os();
        let executable = PathBuf::from(args.next().unwrap_or_default());
        let arguments: Vec<_> = args.collect();
        if executable.file_name() == Some(OsStr::new("createrepo_c")) {
            let reference = env::var_os("PUBLICATION_REFERENCE")
                .ok_or_else(|| invalid("PUBLICATION_REFERENCE is required"))?;
            frozen_indexer(Path::new(&reference), &arguments)?;
            return Ok(ExitCode::SUCCESS);
        }
        if arguments.len() != 2 {
            return Err(invalid(
                "usage: fastmash-test-package-fixture-repair WORK REPOSITORY_SCRIPT",
            ));
        }
        let work = fs::canonicalize(&arguments[0])?;
        let script = fs::canonicalize(&arguments[1])?;
        repair(&work, &script)
    }

    #[cfg(test)]
    mod tests {
        use super::{PartialSnapshot, find_primary};
        use std::fs;
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::{DirBuilderExt, PermissionsExt, symlink};
        use std::path::PathBuf;
        use std::sync::atomic::{AtomicU64, Ordering};

        struct Fixture(PathBuf);

        impl Fixture {
            fn new() -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "fastmash-fixture-repair-unit-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
                Self(path)
            }
        }

        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        #[test]
        fn primary_metadata_follows_native_indexer_compression() {
            for name in [
                "frozen-primary.xml.gz",
                "frozen-primary.xml.zst",
                "frozen-primary.xml.xz",
                "frozen-primary.xml.bz2",
                "frozen-primary.xml",
            ] {
                let fixture = Fixture::new();
                let metadata = fixture.0.join(name);
                fs::write(&metadata, b"opaque frozen metadata").unwrap();
                fs::write(
                    fixture.0.join("frozen-filelists.xml.zst"),
                    b"other metadata",
                )
                .unwrap();
                assert_eq!(find_primary(&fixture.0).unwrap(), Some(metadata), "{name}");
            }
        }

        #[test]
        fn partial_snapshot_preserves_reference_and_requires_exact_restored_bytes() {
            let fixture = Fixture::new();
            let repository = fixture.0.join("new");
            let generation = "a".repeat(64);
            let snapshot = repository.join("rpm/snapshots").join(&generation);
            fs::create_dir_all(snapshot.join("repodata")).unwrap();
            fs::write(
                repository.join("rpm/mirrorlist"),
                format!("https://packages.fastmash.io/rpm/snapshots/{generation}/\n"),
            )
            .unwrap();
            let primary = snapshot.join("repodata/frozen-primary.xml.gz");
            fs::write(&primary, b"\0\xfforiginal primary\n").unwrap();
            fs::write(snapshot.join("repodata/repomd.xml"), b"index bytes").unwrap();
            fs::write(snapshot.join("repodata/repomd.xml.asc"), b"signature bytes").unwrap();
            let unusual = std::ffi::OsStr::from_bytes(b"package-\xff.rpm");
            fs::write(repository.join("rpm").join(unusual), b"retained RPM bytes").unwrap();
            fs::set_permissions(&primary, fs::Permissions::from_mode(0o640)).unwrap();
            symlink("repomd.xml", snapshot.join("repodata/index-link")).unwrap();
            let partial = fixture.0.join("partial");
            fs::create_dir(&partial).unwrap();

            let prepared = PartialSnapshot::prepare(&repository, &partial).unwrap();
            assert_eq!(fs::read(&primary).unwrap(), b"\0\xfforiginal primary\n");
            assert_eq!(
                fs::read(snapshot.join("repodata/repomd.xml.asc")).unwrap(),
                b"signature bytes"
            );
            assert!(!prepared.primary.exists());
            let partial_metadata = prepared.primary.parent().unwrap();
            assert!(!partial_metadata.join("repomd.xml.asc").exists());
            assert_eq!(
                fs::read(partial_metadata.join("repomd.xml")).unwrap(),
                b"index bytes"
            );
            assert_eq!(
                fs::read(partial.join("rpm").join(unusual)).unwrap(),
                b"retained RPM bytes"
            );
            assert_eq!(
                fs::read_link(partial_metadata.join("index-link")).unwrap(),
                PathBuf::from("repomd.xml")
            );
            assert!(prepared.restored().is_err());

            fs::copy(&primary, &prepared.primary).unwrap();
            assert_eq!(
                fs::metadata(&prepared.primary)
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o640
            );
            prepared.restored().unwrap();
            fs::write(&prepared.primary, b"\0\xffchanged primary\n").unwrap();
            assert!(
                prepared
                    .restored()
                    .unwrap_err()
                    .to_string()
                    .contains("differs")
            );
        }
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match unix::run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("fixture repair checks: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("fixture repair checks require native Unix repository tools");
        std::process::ExitCode::FAILURE
    }
}
