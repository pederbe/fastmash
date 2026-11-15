//! Bounded full-volume calibration. The caller supplies a private test volume.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub struct FullOutput {
    file: Option<File>,
    path: PathBuf,
    length: u64,
}
fn full(error: &io::Error) -> bool {
    matches!(error.raw_os_error(), Some(39 | 112)) || error.kind() == io::ErrorKind::StorageFull
}
impl FullOutput {
    pub fn new(_binary: &Path, root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        let volume = std::env::var_os("FASTMASH_TEST_FULL_VOLUME")
            .ok_or("FASTMASH_TEST_FULL_VOLUME must name a private mounted test volume")?;
        let path = Path::new(&volume).join(format!("fastmash-full-{}.bin", std::process::id()));
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        let result = (|| {
            let bytes = [0x55; 65536];
            let mut written = 0;
            loop {
                match file.write(&bytes) {
                    Ok(0) => return Err("full-volume calibration returned zero write".into()),
                    Ok(length) => {
                        written += length;
                        if written > 128 << 20 {
                            return Err(
                                "test volume did not fill within 128 MiB; refusing further writes"
                                    .into(),
                            );
                        }
                    }
                    Err(error) if full(&error) => break,
                    Err(error) => return Err(error.into()),
                }
            }
            // A short failed write may leave space in its final cluster.
            for _ in 0..65536 {
                match file.write(b"x") {
                    Err(error) if full(&error) => return Ok(()),
                    Ok(1) => {}
                    result => {
                        return Err(format!("invalid full-volume byte control: {result:?}").into());
                    }
                }
            }
            Err("test volume still accepts single bytes".into())
        })();
        if let Err(error) = result {
            drop(file);
            fs::remove_file(&path)?;
            return Err(error);
        }
        let length = file.metadata()?.len();
        let mut owner = Self {
            file: Some(file),
            path,
            length,
        };
        owner.verify()?;
        fs::write(
            root.join("calibration.json"),
            serde_json::to_vec_pretty(&owner.identity()?)?,
        )?;
        Ok(owner)
    }
    pub fn destination(&mut self) -> Result<File> {
        self.verify()?;
        Ok(self.file.as_ref().ok_or("closed full sink")?.try_clone()?)
    }
    pub fn verify(&mut self) -> Result<()> {
        let file = self.file.as_mut().ok_or("closed full sink")?;
        match file.write(b"x") {
            Err(error) if full(&error) => {}
            result => {
                return Err(
                    format!("full sink accepted a byte or failed differently: {result:?}").into(),
                );
            }
        }
        if file.metadata()?.len() != self.length {
            return Err("full sink length changed".into());
        }
        Ok(())
    }
    pub fn identity(&self) -> Result<serde_json::Value> {
        Ok(
            serde_json::json!({"mechanism":"private Windows volume exhausted by bounded writes", "path":self.path, "length":self.length}),
        )
    }
    pub fn finish(mut self) -> Result<()> {
        drop(self.file.take());
        fs::remove_file(&self.path)?;
        Ok(())
    }
}
impl Drop for FullOutput {
    fn drop(&mut self) {
        drop(self.file.take());
        let _ = fs::remove_file(&self.path);
    }
}
