//! Test the public shell installer through real downloads from a local mirror.
//! Target-selection fixtures do not establish native macOS support. The ignored
//! archive test instead installs actual CI-built bytes on the real native host.
#[path = "support/temp_dir.rs"]
mod temp_dir;

use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    net::{TcpListener, TcpStream},
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
use temp_dir::TempDir;

const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn documented_instructions() -> &'static str {
    include_str!("../../../docs/src/install.md")
        .split("```bash\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
}

struct Mirror {
    address: std::net::SocketAddr,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<()>>,
}

impl Mirror {
    fn new(version: &str, files: Vec<(String, Vec<u8>)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let version = version.to_owned();
        let worker = thread::spawn(move || {
            for connection in listener.incoming() {
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                let mut stream = connection.unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut first = String::new();
                reader.read_line(&mut first).unwrap();
                let mut header = String::new();
                loop {
                    header.clear();
                    if reader.read_line(&mut header).unwrap() == 0 || header == "\r\n" {
                        break;
                    }
                }
                drop(reader);
                let mut request = first.split_whitespace();
                let method = request.next().unwrap_or("");
                let path = request.next().unwrap_or("");
                if path == "/latest" {
                    write!(stream, "HTTP/1.1 302 Found\r\nLocation: /tag/v{version}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                } else if path == format!("/tag/v{version}") {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                } else if let Some((_, contents)) = files
                    .iter()
                    .find(|(name, _)| path == format!("/download/v{version}/{name}"))
                {
                    write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        contents.len()
                    )
                    .unwrap();
                    if method != "HEAD" {
                        stream.write_all(contents).unwrap();
                    }
                } else {
                    write!(
                        stream,
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                }
            }
        });
        Self {
            address,
            stop,
            worker: Some(worker),
        }
    }

    fn url(&self) -> String {
        format!("http://{}", self.address)
    }
}

impl Drop for Mirror {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(self.address);
        self.worker.take().unwrap().join().unwrap();
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn find_tool(tool: &str) -> Option<PathBuf> {
    let output = Command::new("/bin/sh")
        .args(["-c", "command -v \"$1\"", "find-tool", tool])
        .output()
        .unwrap();
    output
        .status
        .success()
        .then(|| PathBuf::from(String::from_utf8(output.stdout).unwrap().trim()))
}

fn real_tool(tool: &str) -> PathBuf {
    find_tool(tool).unwrap_or_else(|| panic!("missing installer-test tool {tool}"))
}

fn script(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn fixture_path(root: &Path, os: &str, arch: &str, version: &str) -> PathBuf {
    let bin = root.join("tools");
    fs::create_dir(&bin).unwrap();
    // Exclude GNU checksum tools from the emulated macOS PATH. The installer
    // must use the same shasum interface as the system tool on a native Mac.
    for tool in [
        "curl", "tar", "gzip", "mktemp", "rm", "mkdir", "cp", "chmod", "mv", "sed", "tail", "tr",
        "grep",
    ] {
        symlink(real_tool(tool), bin.join(tool)).unwrap();
    }
    let checksum = if os == "Darwin" {
        "shasum"
    } else {
        "sha256sum"
    };
    if let Some(tool) = find_tool(checksum) {
        symlink(tool, bin.join(checksum)).unwrap();
    } else if checksum == "sha256sum" {
        // A native Mac need not install GNU coreutils to exercise the Linux
        // target fixture. The shared -c interface is provided by system shasum.
        let shasum = real_tool("shasum");
        script(
            &bin.join(checksum),
            &format!("#!/bin/sh\nexec '{}' -a 256 \"$@\"\n", shasum.display()),
        );
    } else {
        panic!("missing installer-test checksum tool {checksum}");
    }
    script(
        &bin.join("uname"),
        &format!("#!/bin/sh\ncase $1 in -s) echo {os};; -m) echo {arch};; esac\n"),
    );
    script(
        &bin.join("sw_vers"),
        &format!("#!/bin/sh\necho {version}\n"),
    );
    bin
}

fn fixture_archive(
    root: &Path,
    version: &str,
    target: &str,
    supervisor: bool,
) -> (String, Vec<u8>) {
    let name = format!("fastmash-v{version}-{target}");
    let stage = root.join(&name);
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("fastmash"), b"replacement fastmash").unwrap();
    if supervisor {
        fs::write(
            stage.join("fastmash-sort-supervisor"),
            b"replacement supervisor",
        )
        .unwrap();
    }
    let archive = root.join(format!("{name}.tar.gz"));
    let status = Command::new("tar")
        .args(["-czf"])
        .arg(&archive)
        .args(["-C"])
        .arg(root)
        .arg(&name)
        .status()
        .unwrap();
    assert!(status.success());
    (format!("{name}.tar.gz"), fs::read(archive).unwrap())
}

fn mirrored_files(archive: (String, Vec<u8>), corrupt: bool) -> Vec<(String, Vec<u8>)> {
    let (name, bytes) = archive;
    let checksum = if corrupt {
        "0".repeat(64)
    } else {
        digest(&bytes)
    };
    vec![
        (
            format!("{name}.sha256"),
            format!("{checksum}  {name}\n").into_bytes(),
        ),
        (name, bytes),
    ]
}

fn install(
    mirror: &Mirror,
    version: Option<&str>,
    dir: &Path,
    path: Option<&Path>,
    cwd: &Path,
) -> Output {
    let mut command = Command::new("/bin/sh");
    command
        .arg(Path::new(ROOT).join("install.sh"))
        .current_dir(cwd)
        .env_clear()
        .env(
            "PATH",
            path.map_or_else(
                || "/usr/bin:/bin:/usr/sbin:/sbin".into(),
                |p| p.as_os_str().to_owned(),
            ),
        )
        .env("HOME", cwd)
        .env("TMPDIR", cwd)
        .env("FASTMASH_RELEASES", mirror.url())
        .env("FASTMASH_INSTALL_DIR", dir);
    if let Some(version) = version {
        command.env("FASTMASH_VERSION", version);
    }
    command.output().unwrap()
}

fn previous_installation(dir: &Path) {
    fs::create_dir(dir).unwrap();
    fs::write(dir.join("fastmash"), b"previous fastmash").unwrap();
    fs::write(dir.join("fastmash-sort-supervisor"), b"previous supervisor").unwrap();
}

fn assert_previous(dir: &Path) {
    assert_eq!(
        fs::read(dir.join("fastmash")).unwrap(),
        b"previous fastmash"
    );
    assert_eq!(
        fs::read(dir.join("fastmash-sort-supervisor")).unwrap(),
        b"previous supervisor"
    );
    assert_eq!(fs::read_dir(dir).unwrap().count(), 2);
}

#[test]
fn linux_installs_the_matched_pair_and_latest_redirect() {
    let root = TempDir::new("archive-linux-pair");
    let path = fixture_path(&root.0, "Linux", "x86_64", "");
    let archive = fixture_archive(&root.0, "0.1.0", "x86_64-unknown-linux-gnu", true);
    let mirror = Mirror::new("0.1.0", mirrored_files(archive, false));
    let dir = root.0.join("installed");
    previous_installation(&dir);
    let output = install(&mirror, None, &dir, Some(&path), &root.0);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(dir.join("fastmash")).unwrap(),
        b"replacement fastmash"
    );
    assert_eq!(
        fs::read(dir.join("fastmash-sort-supervisor")).unwrap(),
        b"replacement supervisor"
    );
    assert_eq!(
        fs::metadata(dir.join("fastmash"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(fs::read_dir(dir).unwrap().count(), 2);
}

#[test]
fn emulated_macos_uses_shasum_and_installs_only_the_cli() {
    let root = TempDir::new("archive-macos-selection");
    let path = fixture_path(&root.0, "Darwin", "arm64", "15.7");
    let archive = fixture_archive(&root.0, "0.2.0-macos-test", "aarch64-apple-darwin", false);
    let mirror = Mirror::new("0.2.0-macos-test", mirrored_files(archive, false));
    let dir = root.0.join("installed");
    let output = install(
        &mirror,
        Some("0.2.0-macos-test"),
        &dir,
        Some(&path),
        &root.0,
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(dir.join("fastmash")).unwrap(),
        b"replacement fastmash"
    );
    assert!(!dir.join("fastmash-sort-supervisor").exists());
    assert_eq!(fs::read_dir(dir).unwrap().count(), 1);
}

#[test]
fn failed_download_or_checksum_preserves_the_existing_installation() {
    for (label, files) in [
        ("missing", Vec::new()),
        ("checksum", {
            let root = TempDir::new("archive-bad-checksum-fixture");
            mirrored_files(
                fixture_archive(&root.0, "0.1.0", "x86_64-unknown-linux-gnu", true),
                true,
            )
        }),
        ("missing-checksum", {
            let root = TempDir::new("archive-missing-checksum-fixture");
            vec![fixture_archive(
                &root.0,
                "0.1.0",
                "x86_64-unknown-linux-gnu",
                true,
            )]
        }),
    ] {
        let root = TempDir::new(&format!("archive-{label}"));
        let path = fixture_path(&root.0, "Linux", "x86_64", "");
        let mirror = Mirror::new("0.1.0", files);
        let dir = root.0.join("installed");
        previous_installation(&dir);
        let output = install(&mirror, Some("0.1.0"), &dir, Some(&path), &root.0);
        assert!(!output.status.success(), "{output:?}");
        assert_previous(&dir);
    }
}

#[test]
fn incomplete_linux_archive_preserves_both_programs() {
    let root = TempDir::new("archive-missing-supervisor");
    let path = fixture_path(&root.0, "Linux", "x86_64", "");
    let archive = fixture_archive(&root.0, "0.1.0", "x86_64-unknown-linux-gnu", false);
    let mirror = Mirror::new("0.1.0", mirrored_files(archive, false));
    let dir = root.0.join("installed");
    previous_installation(&dir);
    let output = install(&mirror, Some("0.1.0"), &dir, Some(&path), &root.0);
    assert!(!output.status.success(), "{output:?}");
    assert_previous(&dir);
}

#[test]
fn unsupported_native_target_or_old_macos_preserves_the_installation() {
    for (label, os, arch, version) in [
        ("intel", "Darwin", "x86_64", "26.0"),
        ("old", "Darwin", "arm64", "14.7"),
        ("arm-linux", "Linux", "aarch64", ""),
    ] {
        let root = TempDir::new(&format!("archive-{label}"));
        let path = fixture_path(&root.0, os, arch, version);
        let mirror = Mirror::new("0.1.0", Vec::new());
        let dir = root.0.join("installed");
        previous_installation(&dir);
        let output = install(&mirror, Some("0.1.0"), &dir, Some(&path), &root.0);
        assert!(!output.status.success(), "{output:?}");
        assert_previous(&dir);
    }
}

#[test]
fn documented_archive_or_binary_checksum_failure_preserves_the_installation() {
    let root = TempDir::new("documented-macos-checksum");
    let version = "0.2.0-macos-test";
    let (name, archive) = fixture_archive(&root.0, version, "aarch64-apple-darwin", false);
    fs::write(root.0.join("archive-version.txt"), format!("{version}\n")).unwrap();
    let dir = root.0.join(".local/bin");
    fs::create_dir(root.0.join(".local")).unwrap();
    previous_installation(&dir);
    // The second attempt has an authentic archive but no binary checksum;
    // both checks must stop the documented method before replacing a program.
    for checksum in ["0".repeat(64), digest(&archive)] {
        fs::write(
            root.0.join(format!("{name}.sha256")),
            format!("{checksum}  {name}\n"),
        )
        .unwrap();
        let output = Command::new("/bin/bash")
            .args(["-c", documented_instructions()])
            .current_dir(&root.0)
            .env_clear()
            .env("HOME", &root.0)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .output()
            .unwrap();
        assert!(!output.status.success(), "{output:?}");
        assert_previous(&dir);
    }
}

fn installed_command(
    binary: &Path,
    cwd: &Path,
    args: &[&str],
    input: &[u8],
    file_input: bool,
) -> Output {
    let mut command = Command::new(binary);
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TMPDIR", cwd)
        .env("FASTMASH_SORT_MEMORY_BYTES", "1024")
        .env("FASTMASH_GROUPING", "sort")
        .env("FASTMASH_PIPE_GROUPING", "sort")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if file_input {
        let file = cwd.join("input.tsv");
        fs::write(&file, input).unwrap();
        command
            .stdin(fs::File::open(file).unwrap())
            .output()
            .unwrap()
    } else {
        let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
}

#[test]
#[ignore = "requires FASTMASH_ARCHIVE_DIR containing the one native CI-built macOS archive"]
fn native_archive_installs_and_runs_outside_the_checkout() {
    assert_eq!(std::env::consts::OS, "macos");
    assert_eq!(std::env::consts::ARCH, "aarch64");
    let archives =
        PathBuf::from(std::env::var_os("FASTMASH_ARCHIVE_DIR").expect("archive directory"));
    let version = fs::read_to_string(archives.join("archive-version.txt")).unwrap();
    let version = version.trim();
    let name = format!("fastmash-v{version}-aarch64-apple-darwin.tar.gz");
    let bytes = fs::read(archives.join(&name)).unwrap();
    println!("archive_sha256={}", digest(&bytes));
    let files = vec![
        (name.clone(), bytes),
        (
            format!("{name}.sha256"),
            fs::read(archives.join(format!("{name}.sha256"))).unwrap(),
        ),
    ];
    let root = TempDir::new("native-archive-install");
    // Execute the exact manual installation block published in the guide.
    // Its HOME and working directory are private and outside the checkout.
    let instructions = documented_instructions();
    fs::write(root.0.join("archive-version.txt"), format!("{version}\n")).unwrap();
    for (file, contents) in &files {
        fs::write(root.0.join(file), contents).unwrap();
    }
    let documented_install = || {
        Command::new("/bin/bash")
            .args(["-c", instructions])
            .current_dir(&root.0)
            .env_clear()
            .env("HOME", &root.0)
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .env("LC_ALL", "C")
            .output()
            .unwrap()
    };
    let manual = documented_install();
    assert!(manual.status.success(), "documented method: {manual:?}");
    assert!(manual.stdout.ends_with(b"3\n"), "{manual:?}");
    let manual_binary = root.0.join(".local/bin/fastmash");
    let original_manual = fs::read(&manual_binary).unwrap();
    assert_eq!(
        digest(&original_manual),
        fs::read_to_string(archives.join("binary.sha256"))
            .unwrap()
            .split_whitespace()
            .next()
            .unwrap()
    );
    fs::write(
        root.0.join(format!("{name}.sha256")),
        format!("{}  {name}\n", "0".repeat(64)),
    )
    .unwrap();
    let failed_manual = documented_install();
    assert!(!failed_manual.status.success(), "{failed_manual:?}");
    assert_eq!(fs::read(&manual_binary).unwrap(), original_manual);
    fs::write(root.0.join(format!("{name}.sha256")), &files[1].1).unwrap();
    let unpacked = root.0.join(name.strip_suffix(".tar.gz").unwrap());
    let provenance = fs::read_to_string(unpacked.join("build-provenance.txt")).unwrap();
    let source = Command::new("git")
        .args(["-C", ROOT, "rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(source.status.success());
    assert!(provenance.contains(&format!(
        "source_revision={}\n",
        String::from_utf8(source.stdout).unwrap().trim()
    )));
    assert!(provenance.contains("MACOSX_DEPLOYMENT_TARGET=15.0\n"));
    assert!(provenance.contains("artifact_kind=development-test\n"));
    assert!(provenance.contains("target=aarch64-apple-darwin\n"));
    assert!(!unpacked.join("fastmash-sort-supervisor").exists());
    for notice in ["LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md"] {
        assert!(unpacked.join(notice).is_file(), "missing {notice}");
    }
    assert!(
        fs::read_to_string(unpacked.join("THIRD-PARTY-LICENSES.md"))
            .unwrap()
            .contains("Target: `aarch64-apple-darwin`")
    );
    println!(
        "documented_method_installed_sha256={}",
        digest(&original_manual)
    );
    let dir = std::env::var_os("FASTMASH_ARCHIVE_INSTALL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.0.join("bin"));
    let mirror = Mirror::new(version, files.clone());
    let output = install(&mirror, Some(version), &dir, None, &root.0);
    println!(
        "installer_stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.status.success(), "{output:?}");
    let binary = fs::canonicalize(dir.join("fastmash")).unwrap();
    assert!(!binary.starts_with(fs::canonicalize(ROOT).unwrap()));
    assert!(!dir.join("fastmash-sort-supervisor").exists());
    let expected = fs::read_to_string(archives.join("binary.sha256")).unwrap();
    assert_eq!(
        digest(&fs::read(&binary).unwrap()),
        expected.split_whitespace().next().unwrap()
    );
    println!("installed_binary={}", binary.display());
    println!(
        "installed_binary_sha256={}",
        digest(&fs::read(&binary).unwrap())
    );
    for args in [["--version"], ["--help"]] {
        let output = installed_command(&binary, &root.0, &args, b"", false);
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty());
        if args[0] == "--version" {
            assert_eq!(
                output.stdout,
                concat!("fastmash ", env!("CARGO_PKG_VERSION"), "\n").as_bytes()
            );
        }
    }
    let spill_input: Vec<u8> = (0..5000)
        .flat_map(|i| {
            format!("{}\t{}\n", if i % 2 == 0 { "b" } else { "a" }, i % 4 + 1).into_bytes()
        })
        .collect();
    let cases: Vec<(&[&str], &[u8], &[u8])> = vec![
        (&["sum", "1", "mean", "2"], b"1\t2\n3\t4\n", b"4\t3\n"),
        (&["-g1", "sum", "2"], b"a\t1\na\t3\nb\t2\n", b"a\t4\nb\t2\n"),
        (&["-sg1", "median", "2"], &spill_input, b"a\t3\nb\t2\n"),
        (
            &["--csv", "-sg1", "sum", "2"],
            b"\"b,b\",2\n\"a,a\",1\n\"a,a\",3\n",
            b"\"a,a\",4\n\"b,b\",2\n",
        ),
        (&["top:2", "1"], b"1\ta\n3\tb\n2\tc\n", b"3\tb\n2\tc\n"),
        (&["wmean", "1:2"], b"2\t1\n4\t3\n", b"3.5\n"),
        (
            &["health", "type", "1", "number", "validate"],
            b"1\n2\n",
            b"",
        ),
    ];
    for (args, input, expected) in cases {
        for file_input in [false, true] {
            let output = installed_command(&binary, &root.0, args, input, file_input);
            assert!(
                output.status.success(),
                "{args:?} file={file_input}: {output:?}"
            );
            if args[0] != "health" {
                assert_eq!(output.stdout, expected, "{args:?} file={file_input}");
            }
            assert!(output.stderr.is_empty(), "{args:?}: {output:?}");
        }
    }
    fs::write(root.0.join("before.tsv"), b"2\n").unwrap();
    for file_input in [false, true] {
        let output = installed_command(
            &binary,
            &root.0,
            &["compare", "before.tsv", "-", "sum", "1"],
            b"5\n",
            file_input,
        );
        assert!(output.status.success(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"matched\t\tnot_requested\t2\t5\t3\tavailable\t150\tavailable\n"
        );
        assert!(output.stderr.is_empty(), "{output:?}");
    }
    // Observe the documented terminal download's security metadata without
    // removing quarantine, bypassing assessment or changing a macOS policy.
    let download = root.0.join(&name);
    let status = Command::new("/usr/bin/curl")
        .args(["-fsSL", "-o"])
        .arg(&download)
        .arg(format!("{}/download/v{version}/{name}", mirror.url()))
        .status()
        .unwrap();
    assert!(status.success());
    for file in [&download, &binary, &manual_binary] {
        let observation = Command::new("/usr/bin/xattr")
            .arg("-l")
            .arg(file)
            .output()
            .unwrap();
        assert!(observation.status.success(), "{observation:?}");
        println!(
            "xattrs {}: {}",
            file.display(),
            String::from_utf8_lossy(&observation.stdout)
        );
    }
    for (tool, args) in [
        ("/usr/bin/codesign", vec!["--display", "--verbose=4"]),
        (
            "/usr/sbin/spctl",
            vec!["--assess", "--type", "execute", "--verbose=4"],
        ),
    ] {
        let observation = Command::new(tool).args(args).arg(&binary).output().unwrap();
        println!(
            "{tool} status={} stdout={} stderr={}",
            observation.status,
            String::from_utf8_lossy(&observation.stdout),
            String::from_utf8_lossy(&observation.stderr)
        );
    }
    for files in [Vec::new(), {
        let mut corrupt = files;
        corrupt[1].1 = format!("{}  {name}\n", "0".repeat(64)).into_bytes();
        corrupt
    }] {
        let failed_mirror = Mirror::new(version, files);
        let before = fs::read(&binary).unwrap();
        let output = install(&failed_mirror, Some(version), &dir, None, &root.0);
        assert!(!output.status.success(), "{output:?}");
        assert_eq!(fs::read(&binary).unwrap(), before);
    }
}
