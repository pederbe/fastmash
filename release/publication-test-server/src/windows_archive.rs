//! Admission of the Windows archive against independently retained build files.
use crate::bytes;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{self, Read},
    path::Path,
};

pub const FILES: &[&str] = &[
    "fastmash.exe",
    "README.md",
    "CHANGELOG.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "THIRD-PARTY-LICENSES.md",
    "binary.sha256",
    "build-provenance.txt",
];

pub fn sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn outside(source: &Path, installed: &Path) -> io::Result<()> {
    if fs::canonicalize(installed)?.starts_with(fs::canonicalize(source)?) {
        return Err(io::Error::other("install outside the source checkout"));
    }
    Ok(())
}

pub fn verify(
    stage: &Path,
    qualified: &Path,
    source: &str,
    label: &str,
    kind: &str,
) -> io::Result<()> {
    let names: BTreeSet<_> = fs::read_dir(stage)?
        .map(|entry| entry.map(|e| e.file_name()))
        .collect::<io::Result<_>>()?;
    let expected: BTreeSet<_> = FILES.iter().map(std::ffi::OsString::from).collect();
    if names != expected {
        return Err(io::Error::other("Windows archive file inventory differs"));
    }
    for name in FILES {
        if !fs::symlink_metadata(stage.join(name))?.is_file() {
            return Err(io::Error::other(format!(
                "archive member is not a regular file: {name}"
            )));
        }
        if *name != "fastmash.exe" && !bytes::same(&stage.join(name), &qualified.join(name))? {
            return Err(io::Error::other(format!(
                "qualified archive member differs: {name}"
            )));
        }
    }
    let manifest = fs::read_to_string(qualified.join("binary.sha256"))?;
    let expected_hash = manifest
        .trim()
        .strip_suffix("  fastmash.exe")
        .ok_or_else(|| io::Error::other("invalid qualified executable manifest"))?;
    if expected_hash.len() != 64
        || !expected_hash.bytes().all(|b| b.is_ascii_hexdigit())
        || sha256(&stage.join("fastmash.exe"))? != expected_hash
    {
        return Err(io::Error::other(
            "installed Windows executable differs from qualified build",
        ));
    }
    let builds = match kind {
        "release-candidate" => "2",
        "development-test" => "1",
        _ => return Err(io::Error::other("unknown archive kind")),
    };
    let provenance = fs::read_to_string(stage.join("build-provenance.txt"))?;
    for (key, value) in [
        ("source_revision", source),
        ("archive_label", label),
        ("artifact_kind", kind),
        ("target", "x86_64-pc-windows-msvc"),
        ("clean_builds", builds),
    ] {
        let values: Vec<_> = provenance
            .lines()
            .filter_map(|line| line.strip_prefix(&format!("{key}=")))
            .collect();
        if values != [value] {
            return Err(io::Error::other(format!(
                "archive provenance differs: {key}"
            )));
        }
    }
    Ok(())
}

/// dumpbin /IMPORTS includes the normal and delay-load dependency lists.
/// The allowlist is adapted from Ripscope's Windows release inspector.
pub fn pe(headers: &str, imports: &str) -> io::Result<()> {
    let headers = headers.to_ascii_lowercase();
    if !headers.contains("8664 machine (x64)") || !headers.contains("20b magic # (pe32+)") {
        return Err(io::Error::other("expected an x64 PE32+ executable"));
    }
    let allowed = [
        "kernel32.dll",
        "ntdll.dll",
        "bcrypt.dll",
        "bcryptprimitives.dll",
        "advapi32.dll",
        "userenv.dll",
        "ws2_32.dll",
        "ole32.dll",
        "shell32.dll",
        "user32.dll",
        "gdi32.dll",
        "secur32.dll",
        "rpcrt4.dll",
        "crypt32.dll",
        "api-ms-win-core-synch-l1-2-0.dll",
    ];
    let libraries: BTreeSet<_> = imports
        .lines()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|line| line.ends_with(".dll"))
        .collect();
    if libraries.is_empty()
        || libraries
            .iter()
            .any(|name| !allowed.contains(&name.as_str()))
    {
        return Err(io::Error::other(format!(
            "unexpected native or dynamic CRT imports: {libraries:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    struct Files(std::path::PathBuf);
    impl Files {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "fastmash-windows-archive-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Files {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
    #[test]
    fn hash_uses_the_known_sha256_vector() {
        let files = Files::new();
        fs::write(files.0.join("abc"), b"abc").unwrap();
        assert_eq!(
            sha256(&files.0.join("abc")).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
    #[test]
    fn pe_rejects_wrong_machine_empty_and_delayed_runtime_imports() {
        let header = "8664 machine (x64)\n20B magic # (PE32+)";
        assert!(
            pe(
                header,
                "Image has the following dependencies:\nKERNEL32.dll\n bcrypt.dll\n"
            )
            .is_ok()
        );
        for imports in [
            "",
            "kernel32.dll\nImage has the following delay load dependencies:\nVCRUNTIME140.dll",
            "KERNEL32.dll\napi-ms-win-crt-runtime-l1-1-0.dll",
            "custom.dll",
        ] {
            assert!(pe(header, imports).is_err(), "{imports}");
        }
        assert!(pe("14C machine (x86)\n10B magic # (PE32)", "KERNEL32.dll").is_err());
    }
    #[test]
    fn windows_admission_checks_independent_identity_documents_and_provenance() {
        let files = Files::new();
        let stage = files.0.join("installed");
        let qualified = files.0.join("qualified");
        fs::create_dir(&stage).unwrap();
        fs::create_dir(&qualified).unwrap();
        for name in FILES {
            fs::write(stage.join(name), name).unwrap();
        }
        fs::write(
            stage.join("binary.sha256"),
            format!(
                "{}  fastmash.exe\n",
                sha256(&stage.join("fastmash.exe")).unwrap()
            ),
        )
        .unwrap();
        let source = "0123456789abcdef0123456789abcdef01234567";
        fs::write(stage.join("build-provenance.txt"),format!("source_revision={source}\narchive_label=1.0.0\nartifact_kind=release-candidate\ntarget=x86_64-pc-windows-msvc\nclean_builds=2\n")).unwrap();
        for name in FILES.iter().filter(|name| **name != "fastmash.exe") {
            fs::copy(stage.join(name), qualified.join(name)).unwrap();
        }
        let check = || verify(&stage, &qualified, source, "1.0.0", "release-candidate");
        check().unwrap();
        for name in [
            "fastmash.exe",
            "README.md",
            "THIRD-PARTY-LICENSES.md",
            "binary.sha256",
            "build-provenance.txt",
        ] {
            let bytes = fs::read(stage.join(name)).unwrap();
            fs::write(stage.join(name), b"changed").unwrap();
            assert!(check().is_err(), "{name}");
            fs::write(stage.join(name), bytes).unwrap();
        }
        fs::write(stage.join("extra.dll"), b"extra").unwrap();
        assert!(check().is_err());
        fs::remove_file(stage.join("extra.dll")).unwrap();
        assert!(
            verify(
                &stage,
                &qualified,
                "different",
                "1.0.0",
                "release-candidate"
            )
            .is_err()
        );
        assert!(verify(&stage, &qualified, source, "2.0.0", "release-candidate").is_err());
        assert!(verify(&stage, &qualified, source, "1.0.0", "development-test").is_err());
        assert!(outside(&files.0, &stage).is_err());
        outside(&qualified, &stage).unwrap();
    }
}
