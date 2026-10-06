//! Private directories for tests that write fixtures or copy executables.
#[cfg(not(target_os = "macos"))]
use std::path::Path;
use std::{
    fs::{self, DirBuilder},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::PathBuf,
};

pub(crate) struct TempDir(pub(crate) PathBuf);

impl TempDir {
    pub(crate) fn new(name: &str) -> Self {
        // Use the shared temporary parent rather than trusting TMPDIR's
        // ancestors. Darwin's /tmp is a root-owned link to /private/tmp.
        // Validate that destination; Linux retains its direct-directory check.
        #[cfg(target_os = "macos")]
        let parent = fs::canonicalize("/tmp").unwrap();
        #[cfg(not(target_os = "macos"))]
        let parent = Path::new("/tmp").to_owned();
        let metadata = fs::symlink_metadata(&parent).unwrap();
        assert!(
            metadata.is_dir()
                && metadata.uid() == 0
                && (metadata.mode() & 0o022 == 0 || metadata.mode() & 0o1000 != 0),
            "test temporary parent must be root-owned and private or sticky"
        );
        let path = parent.join(format!("fastmash-test-{name}-{}", std::process::id()));
        Self::create(path).unwrap()
    }

    /// Exclusively creates a root under a trusted or sticky parent. Existing
    /// entries are rejected without inspecting, modifying or removing them.
    pub(crate) fn create(path: PathBuf) -> io::Result<Self> {
        DirBuilder::new().mode(0o700).create(&path)?;
        let mut root = Self(path);
        // Restore owner permissions if the umask removed any; creation never
        // grants access to other users. The guard covers both setup failures.
        fs::set_permissions(&root.0, fs::Permissions::from_mode(0o700))?;
        root.0 = fs::canonicalize(&root.0)?;
        Ok(root)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // A failed removal leaves an entry that create will reject next time.
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_roots_are_private_canonical_and_removed_on_drop() {
        let parent = TempDir::new("owned-cleanup");
        let spelled = parent.0.join(".").join("owned");
        let root = TempDir::create(spelled).unwrap();
        let path = parent.0.join("owned");
        assert_eq!(root.0, path);
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, 0o700);
        fs::create_dir(path.join("nested")).unwrap();
        fs::write(path.join("nested/fixture"), b"test").unwrap();
        drop(root);
        assert!(!path.exists());
        assert!(parent.0.is_dir());
    }

    #[test]
    fn fixture_errors_and_unwinding_remove_owned_roots() {
        fn bad_fixture(path: PathBuf) -> io::Result<()> {
            let root = TempDir::create(path)?;
            fs::write(root.0.join("missing/fixture"), b"test")
        }
        let parent = TempDir::new("failed-fixture-cleanup");
        let error_path = parent.0.join("error");
        assert_eq!(
            bad_fixture(error_path.clone()).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert!(!error_path.exists());
        let panic_path = parent.0.join("panic");
        let result = std::panic::catch_unwind(|| {
            let root = TempDir::create(panic_path.clone()).unwrap();
            fs::write(root.0.join("fixture"), b"test").unwrap();
            panic!("fixture setup failed");
        });
        assert!(result.is_err());
        assert!(!panic_path.exists());
        assert!(parent.0.is_dir());
    }
}
