//! Run the local publication schedule with the real uploader and apt/dnf clients.
#![forbid(unsafe_code)]

#[cfg(unix)]
mod unix {
    use std::env;
    use std::ffi::{OsStr, OsString};
    use std::fs::{self, File};
    use std::io::{self, Write};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::DirBuilderExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Output};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const APT_SCRIPT: &str = r#"
set -euo pipefail
rm -f /etc/apt/sources.list /etc/apt/sources.list.d/*
printf "deb [signed-by=/repo/fastmash.gpg] %s/apt stable main\n" "$ORIGIN" > /etc/apt/sources.list.d/fastmash.list
apt-get -q -o APT::Update::Error-Mode=any update
apt-cache policy fastmash
"#;

    const DNF_SCRIPT: &str = r#"
set -euo pipefail
rm -f /etc/yum.repos.d/*
sed "s|https://packages.fastmash.io|$ORIGIN|g" /repo/fastmash.repo > /etc/yum.repos.d/fastmash.repo
dnf -qy --refresh makecache
dnf -q --showduplicates list fastmash
"#;

    fn failure(message: impl Into<String>) -> io::Error {
        io::Error::other(message.into())
    }

    fn contains(bytes: &[u8], needle: &[u8]) -> bool {
        bytes.windows(needle.len()).any(|part| part == needle)
    }

    fn apt_accepted(output: &Output) -> io::Result<()> {
        if !output.status.success() {
            return Err(failure(format!("apt failed: {}", output.status)));
        }
        if output
            .stdout
            .split(|byte| *byte == b'\n')
            .any(|line| matches!(line.trim_ascii(), b"Candidate: 0.1.0" | b"Candidate: 0.2.0"))
        {
            Ok(())
        } else {
            Err(failure("apt policy has no candidate 0.1.0 or 0.2.0"))
        }
    }

    fn corruption_rejected(output: &Output, client: &str, phrases: &[&[u8]]) -> io::Result<()> {
        if output.status.success() {
            return Err(failure(format!("{client} accepted corrupt metadata")));
        }
        if phrases
            .iter()
            .any(|phrase| contains(&output.stdout, phrase) || contains(&output.stderr, phrase))
        {
            Ok(())
        } else {
            Err(failure(format!(
                "{client} failed without the expected corruption diagnostic"
            )))
        }
    }

    fn request_evidence(log: &[u8]) -> (bool, bool) {
        let mut by_hash = false;
        let mut signature = false;
        for line in log.split(|byte| *byte == b'\n') {
            let fields: Vec<_> = line.split(|byte| byte.is_ascii_whitespace()).collect();
            if fields.len() < 4 || !matches!(fields[0], b"GET" | b"HEAD") {
                continue;
            }
            let native_line = (fields[2] == b"200" && fields[3..] == [b"OK".as_slice()])
                || (fields[2] == b"404" && fields[3..] == [b"Not".as_slice(), b"Found".as_slice()]);
            if !native_line {
                continue;
            }
            let path = fields[1];
            by_hash |= contains(path, b"/by-hash/SHA512/");
            signature |= fields[2] == b"200"
                && path.strip_prefix(b"/rpm/snapshots/").is_some_and(|suffix| {
                    suffix
                        .strip_suffix(b"/repodata/repomd.xml.asc")
                        .is_some_and(|snapshot| !snapshot.is_empty())
                });
        }
        (by_hash, signature)
    }

    struct WorkRoot(PathBuf);

    impl WorkRoot {
        fn new() -> io::Result<Self> {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_err(|error| failure(error.to_string()))?
                .as_nanos();
            let path = env::temp_dir().join(format!(
                "fastmash-package-publication-{}-{stamp}",
                std::process::id()
            ));
            fs::DirBuilder::new().mode(0o700).create(&path)?;
            let mut root = Self(path);
            root.0 = fs::canonicalize(&root.0)?;
            Ok(root)
        }
    }

    impl Drop for WorkRoot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    struct Fixture {
        root: WorkRoot,
        old: PathBuf,
        new: PathBuf,
        uploader: PathBuf,
        engine: OsString,
        apt_image: OsString,
        dnf_image: OsString,
        action_path: OsString,
        logs: PathBuf,
        ledger: File,
        server: Option<Child>,
        origin: String,
    }

    fn default_env(name: &str, default: &str) -> OsString {
        env::var_os(name)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| default.into())
    }

    fn copy_tree(source: &Path, destination: &Path) -> io::Result<()> {
        fs::create_dir(destination)?;
        for entry in fs::read_dir(source)? {
            let entry = entry?;
            let target = destination.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_tree(&entry.path(), &target)?;
            } else {
                fs::copy(entry.path(), target)?;
            }
        }
        fs::set_permissions(destination, fs::metadata(source)?.permissions())
    }

    impl Fixture {
        fn new(old: PathBuf, new: PathBuf, uploader: PathBuf) -> io::Result<Self> {
            let root = WorkRoot::new()?;
            fs::create_dir(root.0.join("bin"))?;
            let inherited_path =
                env::var_os("PATH").ok_or_else(|| failure("PATH is required for native tools"))?;
            let action_path = env::join_paths(
                std::iter::once(root.0.join("bin")).chain(env::split_paths(&inherited_path)),
            )
            .map_err(|error| failure(format!("cannot prepend fixture adapter to PATH: {error}")))?;
            let logs = if let Some(path) = env::var_os("PUBLICATION_TEST_EVIDENCE") {
                let path = PathBuf::from(path);
                fs::DirBuilder::new().mode(0o700).create(&path)?;
                fs::canonicalize(path)?
            } else {
                let path = root.0.join("logs");
                fs::create_dir(&path)?;
                path
            };
            let ledger = File::create(logs.join("completed.tsv"))?;
            let mut fixture = Self {
                root,
                old,
                new,
                uploader,
                engine: default_env("CONTAINER_ENGINE", "docker"),
                apt_image: default_env("APT_TEST_IMAGE", "debian:12-slim"),
                dnf_image: default_env("DNF_TEST_IMAGE", "almalinux:8"),
                action_path,
                logs,
                ledger,
                server: None,
                origin: String::new(),
            };
            // Raw bytes keep non-UTF-8 paths and the selected native controls exact.
            for (name, value) in [
                ("work-root", fixture.root.0.as_os_str()),
                ("old", fixture.old.as_os_str()),
                ("new", fixture.new.as_os_str()),
                ("uploader", fixture.uploader.as_os_str()),
                ("engine", fixture.engine.as_os_str()),
                ("apt-image", fixture.apt_image.as_os_str()),
                ("dnf-image", fixture.dnf_image.as_os_str()),
                ("inherited-path", inherited_path.as_os_str()),
            ] {
                fs::write(fixture.logs.join(format!("{name}.raw")), value.as_bytes())?;
            }
            let executable = env::current_exe()?;
            let siblings = executable
                .parent()
                .ok_or_else(|| failure("executable has no parent"))?;
            fs::copy(
                siblings.join("fastmash-publication-test-storage"),
                fixture.root.0.join("bin/aws"),
            )?;
            fs::write(fixture.root.0.join("action"), b"#!/bin/bash\nset -euo pipefail\nbash \"$PUBLICATION_UPLOADER\" \"$PUBLICATION_SOURCE\" test-bucket\n")?;
            let mut server = Command::new(siblings.join("fastmash-publication-test-server"));
            server
                .args([
                    fixture.root.0.join("live"),
                    fixture.root.0.join("ready"),
                    fixture.root.0.join("trigger"),
                    fixture.root.0.join("action"),
                ])
                .stdout(File::create(fixture.logs.join("server.stdout"))?)
                .stderr(File::create(fixture.logs.join("server.log"))?);
            fixture.action_environment(&mut server);
            fixture.server = Some(server.spawn()?);
            fs::write(
                fixture.logs.join("server.pid"),
                format!("{}\n", fixture.server.as_ref().unwrap().id()),
            )?;
            let port = fixture.wait_ready()?;
            fixture.origin = format!("http://127.0.0.1:{port}");
            fs::write(fixture.logs.join("origin"), format!("{}\n", fixture.origin))?;
            Ok(fixture)
        }

        fn action_environment(&self, command: &mut Command) {
            command
                .env("PATH", &self.action_path)
                .env("PUBLICATION_LIVE", self.root.0.join("live"))
                .env("PUBLICATION_COUNT", self.root.0.join("count"))
                .env("PUBLICATION_SOURCE", &self.new)
                .env("PUBLICATION_UPLOADER", &self.uploader)
                .env_remove("PUBLICATION_STOP_AFTER");
        }

        fn wait_ready(&mut self) -> io::Result<u16> {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                if let Some(status) = self.server.as_mut().unwrap().try_wait()? {
                    return Err(failure(format!(
                        "fixture server exited before readiness: {status}"
                    )));
                }
                match fs::read(self.root.0.join("ready")) {
                    Ok(bytes) if bytes.ends_with(b"\n") => {
                        let text = std::str::from_utf8(&bytes)
                            .map_err(|_| failure("fixture server wrote an invalid ready port"))?;
                        let port = text
                            .trim()
                            .parse::<u16>()
                            .map_err(|_| failure("fixture server wrote an invalid ready port"))?;
                        if port == 0 {
                            return Err(failure("fixture server wrote a zero ready port"));
                        }
                        return Ok(port);
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                if Instant::now() >= deadline {
                    return Err(failure(
                        "fixture server did not become ready within ten seconds",
                    ));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn reset(&mut self, stage: &str) -> io::Result<()> {
            let live = self.root.0.join("live");
            if live.exists() {
                fs::remove_dir_all(&live)?;
            }
            copy_tree(&self.old, &live)?;
            fs::write(self.root.0.join("count"), b"0\n")?;
            writeln!(self.ledger, "{stage}\treset")
        }

        fn record(&mut self, stage: &str, output: &Output) -> io::Result<()> {
            fs::write(self.logs.join(format!("{stage}.stdout")), &output.stdout)?;
            fs::write(self.logs.join(format!("{stage}.stderr")), &output.stderr)?;
            fs::write(
                self.logs.join(format!("{stage}.status")),
                format!("{}\n", output.status),
            )?;
            writeln!(self.ledger, "{stage}\t{}", output.status)
        }

        fn check(&self, stage: &str, output: &Output, result: io::Result<()>) -> io::Result<()> {
            if let Err(error) = result {
                eprintln!("{stage} stdout:");
                let _ = io::stderr().write_all(&output.stdout);
                eprintln!("{stage} stderr:");
                let _ = io::stderr().write_all(&output.stderr);
                return Err(failure(format!("{stage}: {error}")));
            }
            Ok(())
        }

        fn apt(&mut self, stage: &str) -> io::Result<Output> {
            let mut mount = self.old.as_os_str().to_owned();
            mount.push(":/repo:ro");
            let output = Command::new(&self.engine)
                .args(["run", "--rm", "--network=host", "-v"])
                .arg(mount)
                .arg("-e")
                .arg(format!("ORIGIN={}", self.origin))
                .arg("--cidfile")
                .arg(self.logs.join(format!("{stage}.container-id")))
                .arg(&self.apt_image)
                .args(["bash", "-c", APT_SCRIPT])
                .output()?;
            self.record(stage, &output)?;
            Ok(output)
        }

        fn dnf(&mut self, stage: &str) -> io::Result<Output> {
            let mut mount = self.new.as_os_str().to_owned();
            mount.push(":/repo:ro");
            let output = Command::new(&self.engine)
                .args(["run", "--rm", "--network=host", "-v"])
                .arg(mount)
                .arg("-e")
                .arg(format!("ORIGIN={}", self.origin))
                .arg("--cidfile")
                .arg(self.logs.join(format!("{stage}.container-id")))
                .arg(&self.dnf_image)
                .args(["bash", "-c", DNF_SCRIPT])
                .output()?;
            self.record(stage, &output)?;
            Ok(output)
        }

        fn clients(&mut self, stage: &str) -> io::Result<()> {
            let apt_stage = format!("{stage}-apt");
            let apt = self.apt(&apt_stage)?;
            self.check(&apt_stage, &apt, apt_accepted(&apt))?;
            let dnf_stage = format!("{stage}-dnf");
            let dnf = self.dnf(&dnf_stage)?;
            let accepted = if dnf.status.success() {
                Ok(())
            } else {
                Err(failure(format!("dnf failed: {}", dnf.status)))
            };
            self.check(&dnf_stage, &dnf, accepted)
        }

        fn upload(&mut self, stage: &str, stop: Option<u64>) -> io::Result<()> {
            let mut command = Command::new("bash");
            command.arg(self.root.0.join("action"));
            self.action_environment(&mut command);
            if let Some(stop) = stop {
                command.env("PUBLICATION_STOP_AFTER", stop.to_string());
            }
            let output = command.output()?;
            self.record(stage, &output)?;
            fs::write(
                self.logs.join(format!("{stage}.count")),
                fs::read(self.root.0.join("count"))?,
            )?;
            let expected = if stop.is_some() { 75 } else { 0 };
            let accepted = if output.status.code() == Some(expected) {
                Ok(())
            } else {
                Err(failure(format!(
                    "expected uploader status {expected}, got {}",
                    output.status
                )))
            };
            self.check(stage, &output, accepted)
        }

        fn exercise(&mut self) -> io::Result<()> {
            self.reset("initial")?;
            self.clients("initial-old")?;
            self.upload("initial-upload", None)?;
            let writes = fs::read_to_string(self.root.0.join("count"))?
                .trim()
                .parse::<u64>()
                .map_err(|_| failure("uploader wrote an invalid complete-run count"))?;
            if writes == 0 {
                return Err(failure("uploader completed without writing any objects"));
            }
            writeln!(self.ledger, "writes\t{writes}")?;
            self.clients("initial-new")?;
            for stop in 1..=writes {
                let stage = format!("stop-{stop}");
                self.reset(&stage)?;
                self.upload(&format!("{stage}-interrupted-upload"), Some(stop))?;
                self.clients(&format!("{stage}-interrupted"))?;
                self.upload(&format!("{stage}-resume-upload"), None)?;
                self.clients(&format!("{stage}-resumed"))?;
                writeln!(self.ledger, "{stage}\tcompleted")?;
            }
            println!(
                "apt and dnf accept and resume every interrupted upload ({writes} object boundaries)"
            );
            for (stage, path) in [
                ("trigger-inrelease", "/apt/dists/stable/InRelease"),
                ("trigger-mirrorlist", "/rpm/mirrorlist"),
            ] {
                self.reset(stage)?;
                fs::write(self.root.0.join("trigger"), format!("{path}\n"))?;
                self.clients(&format!("{stage}-spanning"))?;
                if self.root.0.join("trigger").try_exists()? {
                    return Err(failure(format!(
                        "publication trigger was not consumed: {path}"
                    )));
                }
                self.clients(&format!("{stage}-published"))?;
                fs::write(
                    self.logs.join(format!("{stage}.count")),
                    fs::read(self.root.0.join("count"))?,
                )?;
                writeln!(self.ledger, "{stage}\tcompleted\t{path}")?;
            }
            println!("apt and dnf accept clients spanning publication");
            let (by_hash, signature) = request_evidence(&fs::read(self.logs.join("server.log"))?);
            if !by_hash || !signature {
                return Err(failure(
                    "server log lacks a by-hash request or successful snapshot signature request",
                ));
            }
            writeln!(self.ledger, "request-evidence\tcompleted")?;
            self.reset("corrupt-index")?;
            let digest = Command::new("sha512sum")
                .stdin(File::open(
                    self.old
                        .join("apt/dists/stable/main/binary-amd64/Packages.gz"),
                )?)
                .output()?;
            self.record("index-sha512", &digest)?;
            if !digest.status.success() {
                return self.check("index-sha512", &digest, Err(failure("sha512sum failed")));
            }
            let hash = digest
                .stdout
                .split(|byte| byte.is_ascii_whitespace())
                .next()
                .unwrap_or_default();
            if hash.len() != 128 || !hash.iter().all(u8::is_ascii_hexdigit) {
                return Err(failure("sha512sum did not return a SHA512 digest"));
            }
            let index = self
                .root
                .0
                .join("live/apt/dists/stable/main/binary-amd64/by-hash/SHA512")
                .join(OsStr::from_bytes(hash));
            if !index.is_file() {
                return Err(failure("old SHA512 by-hash index is missing"));
            }
            fs::write(index, b"corrupt index\n")?;
            let apt = self.apt("corrupt-index-apt")?;
            self.check(
                "corrupt-index-apt",
                &apt,
                corruption_rejected(&apt, "apt", &[b"Hash Sum mismatch", b"unexpected size"]),
            )?;
            writeln!(self.ledger, "corrupt-index\tcompleted")?;
            self.reset("corrupt-signature")?;
            let mut mirrorlist = fs::read(self.old.join("rpm/mirrorlist"))?;
            while mirrorlist.last() == Some(&b'\n') {
                mirrorlist.pop();
            }
            let snapshot = mirrorlist
                .strip_prefix(b"https://packages.fastmash.io/")
                .ok_or_else(|| failure("old mirrorlist has no fixture origin"))?;
            let signature = self
                .root
                .0
                .join("live")
                .join(OsStr::from_bytes(snapshot))
                .join("repodata/repomd.xml.asc");
            if !signature.is_file() {
                return Err(failure("old snapshot signature is missing"));
            }
            fs::write(signature, b"corrupt signature\n")?;
            let dnf = self.dnf("corrupt-signature-dnf")?;
            self.check(
                "corrupt-signature-dnf",
                &dnf,
                corruption_rejected(&dnf, "dnf", &[b"GPG signature"]),
            )?;
            writeln!(self.ledger, "corrupt-signature\tcompleted")?;
            println!("apt and dnf reject corrupt signed metadata");
            Ok(())
        }

        fn server_tail(&self) {
            if let Ok(log) = fs::read(self.logs.join("server.log")) {
                let lines: Vec<_> = log.split_inclusive(|byte| *byte == b'\n').collect();
                eprintln!("fixture server log (last thirty lines):");
                for line in &lines[lines.len().saturating_sub(30)..] {
                    let _ = io::stderr().write_all(line);
                }
            }
        }

        fn stop_server(&mut self) -> io::Result<()> {
            let Some(mut server) = self.server.take() else {
                return Ok(());
            };
            let (description, result) = match server.try_wait() {
                Ok(Some(status)) => {
                    let _ = server.wait();
                    let message = format!("fixture server exited before cleanup: {status}");
                    (message.clone(), Err(failure(message)))
                }
                Ok(None) => {
                    let killed = server.kill();
                    let waited = server.wait();
                    match (killed, waited) {
                        (Ok(()), Ok(status)) => (format!("killed and reaped: {status}"), Ok(())),
                        (Err(error), _) | (_, Err(error)) => (error.to_string(), Err(error)),
                    }
                }
                Err(error) => {
                    let _ = server.kill();
                    let _ = server.wait();
                    (error.to_string(), Err(error))
                }
            };
            fs::write(
                self.logs.join("server-cleanup.status"),
                format!("{description}\n"),
            )?;
            writeln!(self.ledger, "server-cleanup\t{description}")?;
            result
        }

        fn cleanup(&mut self) -> io::Result<()> {
            let stopped = self.stop_server();
            if stopped.is_err() {
                self.server_tail();
            }
            let removed = fs::remove_dir_all(&self.root.0);
            stopped.and(removed)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if self.server.is_some() {
                self.server_tail();
            }
            if let Some(mut server) = self.server.take() {
                let _ = server.kill();
                let _ = server.wait();
            }
        }
    }

    pub(super) fn run() -> io::Result<()> {
        let args: Vec<_> = env::args_os().skip(1).collect();
        if args.len() != 3 {
            return Err(failure(
                "usage: fastmash-test-package-publication OLD_REPOSITORY NEW_REPOSITORY UPLOADER",
            ));
        }
        let old = fs::canonicalize(&args[0])?;
        let new = fs::canonicalize(&args[1])?;
        let uploader = fs::canonicalize(&args[2])?;
        if !old.is_dir() || !new.is_dir() || !uploader.is_file() {
            return Err(failure(
                "old and new repositories must be directories and uploader must be a file",
            ));
        }
        let mut fixture = Fixture::new(old, new, uploader)?;
        let result = fixture.exercise();
        if result.is_err() {
            fixture.server_tail();
        }
        let cleanup = fixture.cleanup();
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => {
                eprintln!("publication fixture cleanup failed: {cleanup}");
                Err(error)
            }
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{apt_accepted, corruption_rejected, request_evidence};
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
        fn apt_requires_a_successful_native_policy_with_a_supported_candidate() {
            assert!(apt_accepted(&output(0, b"fastmash:\n  Candidate: 0.1.0\n", b"")).is_ok());
            assert!(apt_accepted(&output(0, b"fastmash:\n\tCandidate: 0.2.0\n", b"")).is_ok());
            assert!(apt_accepted(&output(0, b"Candidate: 0.3.0\n", b"")).is_err());
            assert!(apt_accepted(&output(0, b"Candidate: 0.1.00\n", b"")).is_err());
            assert!(apt_accepted(&output(0, b"Version table:\n", b"")).is_err());
            assert!(apt_accepted(&output(1, b"  Candidate: 0.2.0\n", b"")).is_err());
        }

        #[test]
        fn apt_corruption_requires_a_failed_client_and_a_known_diagnostic() {
            let phrases: &[&[u8]] = &[b"Hash Sum mismatch", b"unexpected size"];
            assert!(
                corruption_rejected(&output(100, b"Hash Sum mismatch\n", b""), "apt", phrases)
                    .is_ok()
            );
            assert!(
                corruption_rejected(&output(100, b"", b"\xffunexpected size\n"), "apt", phrases)
                    .is_ok()
            );
            assert!(
                corruption_rejected(&output(100, b"", b"Connection refused\n"), "apt", phrases)
                    .is_err()
            );
            assert!(
                corruption_rejected(&output(0, b"Hash Sum mismatch\n", b""), "apt", phrases)
                    .is_err()
            );
        }

        #[test]
        fn dnf_corruption_requires_a_signature_failure_in_either_raw_stream() {
            let phrases: &[&[u8]] = &[b"GPG signature"];
            assert!(
                corruption_rejected(
                    &output(1, b"GPG signature verification failed\n", b""),
                    "dnf",
                    phrases
                )
                .is_ok()
            );
            assert!(
                corruption_rejected(
                    &output(1, b"", b"\xffGPG signature verification failed\n"),
                    "dnf",
                    phrases
                )
                .is_ok()
            );
            assert!(
                corruption_rejected(&output(1, b"", b"No matching packages\n"), "dnf", phrases)
                    .is_err()
            );
            assert!(
                corruption_rejected(&output(0, b"GPG signature\n", b""), "dnf", phrases).is_err()
            );
        }

        #[test]
        fn signature_evidence_requires_an_actual_successful_snapshot_request() {
            assert_eq!(request_evidence(b"GET /apt/by-hash/SHA512/123 200 OK\nGET /rpm/snapshots/abc/repodata/repomd.xml.asc 200 OK\n"), (true, true));
            assert_eq!(
                request_evidence(b"GET /rpm/snapshots/abc/repodata/repomd.xml.asc 404 Not Found\n"),
                (false, false)
            );
            assert_eq!(
                request_evidence(b"diagnostic /rpm/snapshots/abc/repodata/repomd.xml.asc 200 OK\n"),
                (false, false)
            );
            assert_eq!(
                request_evidence(b"GET /rpm/repodata/repomd.xml.asc 200 OK\n"),
                (false, false)
            );
            assert_eq!(
                request_evidence(b"GET /rpm/snapshots//repodata/repomd.xml.asc 200 OK\n"),
                (false, false)
            );
        }
    }
}

fn main() -> std::process::ExitCode {
    #[cfg(unix)]
    match unix::run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("publication checks: {error}");
            std::process::ExitCode::FAILURE
        }
    }
    #[cfg(not(unix))]
    {
        eprintln!("publication checks require Unix native package tools");
        std::process::ExitCode::FAILURE
    }
}
