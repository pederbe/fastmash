//! Run the real isolated package clients while keeping the installed oracle on
//! the host. Client images need no Rust compiler or host-linked helper.
use crate::installed::{Target, verify_with_receipts};
use crate::notices::ExpectedDocuments;
use crate::receipt::{Receipts, successful};
use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const APT_SETUP: &str = r#"set -euo pipefail
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
cp /repo/fastmash.gpg /usr/share/keyrings/fastmash.gpg
echo "deb [signed-by=/usr/share/keyrings/fastmash.gpg] file:/repo/apt stable main" > /etc/apt/sources.list.d/fastmash.list
apt-get -q update
"#;

const DNF_SETUP: &str = r#"set -euo pipefail
rm -f /etc/yum.repos.d/*
sed "s|https://packages.fastmash.io/rpm|file:///repo/rpm|" /repo/rpm/mirrorlist > /tmp/fastmash-mirrorlist
sed -e "s|https://packages.fastmash.io/rpm/mirrorlist|file:///tmp/fastmash-mirrorlist|" -e "s|https://packages.fastmash.io/fastmash.asc|file:///repo/fastmash.asc|" /repo/fastmash.repo > /etc/yum.repos.d/fastmash.repo
"#;

struct Work(PathBuf);

impl Work {
    fn new() -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = env::temp_dir().join(format!(
            "fastmash-installation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Container<'a> {
    engine: &'a OsStr,
    id: String,
    removed: bool,
}

fn identity(bytes: &[u8]) -> io::Result<String> {
    let text = std::str::from_utf8(bytes).map_err(io::Error::other)?.trim();
    if text.len() != 64 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other(
            "container engine did not return a full container identity",
        ));
    }
    Ok(text.to_owned())
}

impl<'a> Container<'a> {
    fn target(&self) -> Target<'_> {
        Target::Container {
            engine: self.engine,
            id: &self.id,
        }
    }

    fn launch(
        engine: &'a OsStr,
        image: &OsStr,
        source: &Path,
        mount: &str,
        qualified: &Path,
        cidfile: &Path,
        receipts: &mut Receipts,
    ) -> io::Result<Self> {
        let mut source_mount = source.as_os_str().to_owned();
        source_mount.push(format!(":{mount}:ro"));
        let mut checksum_mount = qualified.as_os_str().to_owned();
        checksum_mount.push(":/release/qualified.sha256:ro");
        let mut command = Command::new(engine);
        command
            .args(["create", "--network=none", "--cidfile"])
            .arg(cidfile)
            .arg("-v")
            .arg(source_mount)
            .arg("-v")
            .arg(checksum_mount)
            .args(["--entrypoint", "bash"])
            .arg(image)
            .args(["-c", "exec sleep infinity"]);
        let output = match receipts.run("container-create", &mut command, b"") {
            Ok(output) => output,
            Err(error) => {
                // Receipt I/O can fail after the engine has created the client.
                // Its private cidfile still identifies only our owned client.
                if let Ok(bytes) = fs::read(cidfile)
                    && let Ok(id) = identity(&bytes)
                {
                    let mut container = Self {
                        engine,
                        id,
                        removed: false,
                    };
                    let _ = receipts.container_id(&image.to_string_lossy(), &container.id);
                    if let Err(cleanup) = container.cleanup(receipts) {
                        eprintln!("installation container cleanup failed: {cleanup}");
                    }
                }
                return Err(error);
            }
        };
        // A cidfile is useful even if creation failed after assigning an ID.
        // Missing cidfiles never establish that a container has been removed.
        let cid = fs::read(cidfile)
            .ok()
            .and_then(|bytes| identity(&bytes).ok());
        let returned = identity(&output.stdout).ok();
        let id = cid.as_ref().or(returned.as_ref());
        let mut container = id.map(|id| Self {
            engine,
            id: id.clone(),
            removed: false,
        });
        let result = (|| {
            if let Some(container) = &container {
                receipts.container_id(&image.to_string_lossy(), &container.id)?;
            }
            successful(&output, "container creation")?;
            let container = container.as_ref().ok_or_else(|| {
                io::Error::other("container creation returned no usable identity")
            })?;
            if cid.as_ref() != returned.as_ref() {
                return Err(io::Error::other(
                    "container creation output and cidfile disagree",
                ));
            }
            let mut start = Command::new(engine);
            start.arg("start").arg(&container.id);
            let output = receipts.run("container-start", &mut start, b"")?;
            successful(&output, "container start")
        })();
        if let Err(error) = result {
            if let Some(container) = &mut container
                && let Err(cleanup) = container.cleanup(receipts)
            {
                eprintln!("installation container cleanup failed: {cleanup}");
            }
            return Err(error);
        }
        Ok(container.unwrap())
    }

    fn cleanup(&mut self, receipts: &mut Receipts) -> io::Result<()> {
        let mut remove = Command::new(self.engine);
        remove.args(["rm", "-f", &self.id]);
        let output = receipts.run("container-remove", &mut remove, b"")?;
        successful(&output, "owned container removal")?;
        let mut inspect = Command::new(self.engine);
        inspect.args([
            "ps",
            "--all",
            "--no-trunc",
            "--filter",
            &format!("id={}", self.id),
            "--format",
            "{{.ID}}",
        ]);
        let output = receipts.run("container-absence", &mut inspect, b"")?;
        successful(&output, "owned container absence observation")?;
        if !output.stdout.is_empty() || !output.stderr.is_empty() {
            return Err(io::Error::other(
                "owned container absence was not established",
            ));
        }
        self.removed = true;
        Ok(())
    }
}

impl Drop for Container<'_> {
    fn drop(&mut self) {
        if !self.removed {
            let _ = Command::new(self.engine)
                .args(["rm", "-f", &self.id])
                .output();
        }
    }
}

fn native(receipts: &mut Receipts, label: &str, command: &mut Command) -> io::Result<Output> {
    let output = receipts.run(label, command, b"")?;
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    successful(&output, label)?;
    Ok(output)
}

fn previous_version(output: &Output, expected: &OsStr) -> io::Result<()> {
    successful(output, "previous installed version")?;
    let mut actual = output.stdout.as_slice();
    while let Some(value) = actual.strip_suffix(b"\n") {
        actual = value;
    }
    let mut wanted = OsString::from("fastmash ");
    wanted.push(expected);
    if actual != wanted.as_encoded_bytes() {
        return Err(io::Error::other("unexpected previous installed version"));
    }
    Ok(())
}

fn rpm_signature(output: &Output) -> io::Result<()> {
    successful(output, "installed RPM signature query")?;
    if !output
        .stdout
        .windows(b"Key ID".len())
        .any(|window| window == b"Key ID")
    {
        return Err(io::Error::other(
            "installed RPM has no native signature observation",
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Family {
    Apt,
    Dnf,
    Deb,
    Rpm,
}

// These late command-line filters preserve only this package's documentation
// when an explicit notice check runs in a client that normally excludes docs.
const DPKG_DOCUMENTS: [&str; 2] = [
    "--path-include=/usr/share/doc/fastmash",
    "--path-include=/usr/share/doc/fastmash/*",
];

fn apt_command(target: &Target<'_>, documents: bool) -> Command {
    let mut command = target.command("apt-get");
    if documents {
        for option in DPKG_DOCUMENTS {
            let mut value = OsString::from("DPkg::Options::=");
            value.push(option);
            command.arg("-o").arg(value);
        }
    }
    command
}

fn exercise(
    container: &Container<'_>,
    family: Family,
    version: &OsStr,
    previous: Option<&OsStr>,
    package: Option<&Path>,
    documents: Option<&ExpectedDocuments>,
    receipts: &mut Receipts,
) -> io::Result<()> {
    let target = container.target();
    match family {
        Family::Apt | Family::Dnf => {
            let setup = if matches!(family, Family::Apt) {
                APT_SETUP
            } else {
                DNF_SETUP
            };
            native(
                receipts,
                "repository-setup",
                target.command("bash").args(["-c", setup]),
            )?;
            let selected = previous.unwrap_or(version);
            let mut exact_package = OsString::from(if matches!(family, Family::Apt) {
                "fastmash="
            } else {
                "fastmash-"
            });
            exact_package.push(selected);
            if matches!(family, Family::Apt) {
                native(
                    receipts,
                    "package-install",
                    apt_command(&target, documents.is_some())
                        .args(["-qy", "install"])
                        .arg(exact_package),
                )?;
            } else {
                native(
                    receipts,
                    "package-install",
                    target
                        .command("dnf")
                        .args(["-qy", "install"])
                        .arg(exact_package),
                )?;
            }
            if let Some(previous) = previous {
                let output = receipts.run(
                    "previous-version",
                    target.command("fastmash").arg("--version"),
                    b"",
                )?;
                io::stderr().write_all(&output.stderr)?;
                previous_version(&output, previous)?;
                if matches!(family, Family::Apt) {
                    native(
                        receipts,
                        "package-upgrade",
                        apt_command(&target, documents.is_some()).args(["-qy", "upgrade"]),
                    )?;
                } else {
                    native(
                        receipts,
                        "package-upgrade",
                        target.command("dnf").args(["-qy", "upgrade", "fastmash"]),
                    )?;
                }
            }
            if matches!(family, Family::Dnf) {
                let output = receipts.run(
                    "rpm-signature",
                    target
                        .command("rpm")
                        .args(["-q", "--qf", "%{RSAHEADER:pgpsig}\n", "fastmash"]),
                    b"",
                )?;
                io::stderr().write_all(&output.stderr)?;
                rpm_signature(&output)?;
            }
        }
        Family::Deb => {
            let mut command = target.command("dpkg");
            if documents.is_some() {
                command.args(DPKG_DOCUMENTS);
            }
            native(
                receipts,
                "package-install",
                command.arg("-i").arg(package.unwrap()),
            )?;
        }
        Family::Rpm => {
            native(
                receipts,
                "package-install",
                target.command("rpm").arg("-i").arg(package.unwrap()),
            )?;
        }
    }
    verify_with_receipts(
        target,
        version,
        Path::new("/usr/bin"),
        Path::new("/release/qualified.sha256"),
        receipts,
    )?;
    if let Some(documents) = documents {
        documents.verify(
            container.target(),
            matches!(family, Family::Apt | Family::Deb),
            receipts,
        )?;
    }
    let target = container.target();
    match family {
        Family::Apt => {
            native(
                receipts,
                "package-remove",
                target
                    .command("apt-get")
                    .args(["-qy", "remove", "fastmash"]),
            )?;
        }
        Family::Dnf => {
            native(
                receipts,
                "package-remove",
                target.command("dnf").args(["-qy", "remove", "fastmash"]),
            )?;
        }
        Family::Deb => {
            native(
                receipts,
                "package-remove",
                target.command("dpkg").args(["-r", "fastmash"]),
            )?;
        }
        Family::Rpm => {
            native(
                receipts,
                "package-remove",
                target.command("rpm").args(["-e", "fastmash"]),
            )?;
        }
    }
    for program in ["fastmash", "fastmash-sort-supervisor"] {
        let output = native(
            receipts,
            "executable-absence",
            target
                .command("test")
                .args(["!", "-e"])
                .arg(Path::new("/usr/bin").join(program)),
        )?;
        if !output.stdout.is_empty() || !output.stderr.is_empty() {
            return Err(io::Error::other(
                "executable absence returned unexpected diagnostics",
            ));
        }
    }
    Ok(())
}

struct Installation<'a> {
    engine: &'a OsStr,
    source: &'a Path,
    qualified: &'a Path,
    version: &'a OsStr,
    previous: Option<&'a OsStr>,
    documents: Option<&'a ExpectedDocuments>,
}

fn configured(value: Option<OsString>, fallback: &str) -> OsString {
    value
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| fallback.into())
}

impl Installation<'_> {
    fn one(
        &self,
        image: &OsStr,
        family: Family,
        package: Option<&Path>,
        cidfile: &Path,
        receipts: &mut Receipts,
    ) -> io::Result<()> {
        let mount = if package.is_some() {
            "/packages"
        } else {
            "/repo"
        };
        let mut container = Container::launch(
            self.engine,
            image,
            self.source,
            mount,
            self.qualified,
            cidfile,
            receipts,
        )?;
        let result = exercise(
            &container,
            family,
            self.version,
            self.previous,
            package,
            self.documents,
            receipts,
        );
        let cleanup = container.cleanup(receipts);
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => {
                eprintln!("installation container cleanup failed: {cleanup}");
                Err(error)
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

fn invocation(args: &[OsString]) -> io::Result<(&[OsString], Option<&Path>)> {
    let positional = |args: &[OsString]| {
        matches!(
            args.first().and_then(|arg| arg.to_str()),
            Some("repository" | "packages")
        ) && (args.len() == 4
            || (args.first().is_some_and(|arg| arg == "repository") && args.len() == 5))
    };
    // Existing positional values, including empty PREVIOUS, keep their meaning.
    if positional(args) {
        return Ok((args, None));
    }
    if args.len() >= 2
        && args[args.len() - 2] == "--expected-docs"
        && positional(&args[..args.len() - 2])
    {
        return Ok((
            &args[..args.len() - 2],
            Some(Path::new(&args[args.len() - 1])),
        ));
    }
    Err(io::Error::other(
        "usage: fastmash-test-package-installation repository REPOSITORY VERSION QUALIFIED [PREVIOUS] [--expected-docs DOC_DIR]\n       fastmash-test-package-installation packages PACKAGE_DIRECTORY VERSION QUALIFIED [--expected-docs DOC_DIR]",
    ))
}

pub fn run(args: &[OsString]) -> io::Result<()> {
    let (args, expected) = invocation(args)?;
    let source = fs::canonicalize(&args[1])?;
    let qualified = fs::canonicalize(&args[3])?;
    if !source.is_dir() || !qualified.is_file() {
        return Err(io::Error::other(
            "package input must be a directory and qualified checksums must be a file",
        ));
    }
    let documents = expected.map(ExpectedDocuments::load).transpose()?;
    let engine = configured(env::var_os("CONTAINER_ENGINE"), "docker");
    let work = Work::new()?;
    let mut receipts = Receipts::from_env()?;
    let previous = args
        .get(4)
        .filter(|value| !value.is_empty())
        .map(OsString::as_os_str);
    let installation = Installation {
        engine: &engine,
        source: &source,
        qualified: &qualified,
        version: &args[2],
        previous,
        documents: documents.as_ref(),
    };
    let result = if args[0] == "repository" {
        let dnf_image = configured(env::var_os("DNF_TEST_IMAGE"), "almalinux:8");
        let mut result = Ok(());
        for (index, (image, family)) in [
            (OsStr::new("debian:12-slim"), Family::Apt),
            (OsStr::new("ubuntu:24.04"), Family::Apt),
            (dnf_image.as_os_str(), Family::Dnf),
        ]
        .into_iter()
        .enumerate()
        {
            result = installation.one(
                image,
                family,
                None,
                &work.0.join(format!("{index}.cid")),
                &mut receipts,
            );
            if result.is_err() {
                break;
            }
        }
        if result.is_ok() {
            println!(
                "repository installs of {}{} passed",
                args[2].to_string_lossy(),
                previous
                    .map(|value| format!(" (upgraded from {})", value.to_string_lossy()))
                    .unwrap_or_default()
            );
        }
        result
    } else {
        // Version-derived names make ambiguous wildcard matches impossible and
        // retain the two release package formats and native install commands.
        let mut deb = OsString::from("fastmash_");
        deb.push(&args[2]);
        deb.push("_amd64.deb");
        let mut rpm = OsString::from("fastmash-");
        rpm.push(&args[2]);
        rpm.push("-1.x86_64.rpm");
        let mut result = Ok(());
        for (index, (image, family, name)) in [
            ("debian:12-slim", Family::Deb, deb),
            ("almalinux:8-minimal", Family::Rpm, rpm),
        ]
        .into_iter()
        .enumerate()
        {
            if !source.join(&name).is_file() {
                result = Err(io::Error::other(format!(
                    "release package is missing: {}",
                    name.to_string_lossy()
                )));
                break;
            }
            result = installation.one(
                OsStr::new(image),
                family,
                Some(&Path::new("/packages").join(name)),
                &work.0.join(format!("{index}.cid")),
                &mut receipts,
            );
            if result.is_err() {
                break;
            }
        }
        result
    };
    let cleanup = fs::remove_dir_all(&work.0);
    match (result, cleanup) {
        (Err(error), Err(cleanup)) => {
            eprintln!("installation temporary cleanup failed: {cleanup}");
            Err(error)
        }
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::{configured, identity, invocation, previous_version, rpm_signature};
    use std::ffi::{OsStr, OsString};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::process::ExitStatusExt;
    use std::process::{ExitStatus, Output};

    fn output(code: i32, stdout: &[u8], stderr: &[u8]) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8),
            stdout: stdout.to_vec(),
            stderr: stderr.to_vec(),
        }
    }

    #[test]
    fn expected_documents_extend_both_forms_without_reinterpreting_previous() {
        let native_directory = OsString::from_vec(b"/expected-\xff".to_vec());
        for mode in ["packages", "repository"] {
            let positional: Vec<OsString> = [mode, "source", "0.2.0", "qualified"]
                .map(Into::into)
                .to_vec();
            assert_eq!(
                invocation(&positional).unwrap(),
                (positional.as_slice(), None)
            );
            let mut requested = positional.clone();
            requested.extend(["--expected-docs".into(), native_directory.clone()]);
            let (retained, expected) = invocation(&requested).unwrap();
            assert_eq!(retained, positional);
            assert_eq!(expected.unwrap().as_os_str(), native_directory);
        }
        for previous in [
            OsString::new(),
            "0.1.0".into(),
            "--expected-docs".into(),
            OsString::from_vec(b"version-\xff".to_vec()),
        ] {
            let mut positional: Vec<OsString> = ["repository", "source", "0.2.0", "qualified"]
                .map(Into::into)
                .to_vec();
            positional.push(previous);
            assert_eq!(
                invocation(&positional).unwrap(),
                (positional.as_slice(), None)
            );
            let mut requested = positional.clone();
            requested.extend(["--expected-docs".into(), native_directory.clone()]);
            assert_eq!(invocation(&requested).unwrap().0, positional);
        }
    }

    #[test]
    fn expected_documents_require_one_trailing_option_directory_pair() {
        for values in [
            vec![
                "packages",
                "source",
                "0.2.0",
                "qualified",
                "--expected-docs",
            ],
            vec![
                "packages",
                "source",
                "0.2.0",
                "qualified",
                "--other",
                "docs",
            ],
            vec![
                "repository",
                "source",
                "0.2.0",
                "qualified",
                "0.1.0",
                "--expected-docs",
            ],
            vec![
                "packages",
                "--expected-docs",
                "docs",
                "source",
                "0.2.0",
                "qualified",
            ],
            vec![
                "repository",
                "source",
                "0.2.0",
                "qualified",
                "--expected-docs",
                "docs",
                "--expected-docs",
                "docs",
            ],
        ] {
            let values: Vec<OsString> = values.into_iter().map(Into::into).collect();
            assert!(invocation(&values).is_err());
        }
    }

    #[test]
    fn native_controls_keep_unset_empty_and_nonempty_values() {
        for fallback in ["docker", "almalinux:8"] {
            assert_eq!(configured(None, fallback), OsStr::new(fallback));
            assert_eq!(
                configured(Some(OsString::new()), fallback),
                OsStr::new(fallback)
            );
            assert_eq!(
                configured(Some("custom-client".into()), fallback),
                "custom-client"
            );
            let native = OsString::from_vec(b"custom-\xff".to_vec());
            assert_eq!(
                configured(Some(native), fallback).into_vec(),
                b"custom-\xff"
            );
        }
    }

    #[test]
    fn prior_version_is_observed_from_a_successful_native_command() {
        assert!(
            previous_version(&output(0, b"fastmash 0.1.0\n", b""), OsStr::new("0.1.0")).is_ok()
        );
        assert!(
            previous_version(&output(0, b"fastmash 0.2.0\n", b""), OsStr::new("0.1.0")).is_err()
        );
        assert!(
            previous_version(
                &output(125, b"fastmash 0.1.0\n", b"engine failed"),
                OsStr::new("0.1.0")
            )
            .is_err()
        );
    }

    #[test]
    fn rpm_signature_requires_successful_native_key_evidence() {
        assert!(rpm_signature(&output(0, b"RSA/SHA256, Key ID 12345678\n", b"")).is_ok());
        assert!(rpm_signature(&output(0, b"(none)\n", b"")).is_err());
        assert!(rpm_signature(&output(125, b"Key ID 12345678\n", b"engine failed")).is_err());
    }

    #[test]
    fn cleanup_keeps_full_returned_container_identities() {
        let full = "a1".repeat(32);
        assert_eq!(identity(format!("{full}\n").as_bytes()).unwrap(), full);
        assert!(identity(b"a12345678901\n").is_err());
        assert!(identity(b"engine failed\n").is_err());
    }
}
