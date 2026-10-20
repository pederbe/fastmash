#![forbid(unsafe_code)]
//! Own an isolated macOS full-output volume through normal and panic cleanup.
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use plist::Value;
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

struct Running(Child, bool);
impl Drop for Running {
    fn drop(&mut self) {
        if self.1 {
            return;
        }
        let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
        let _ = self.0.kill();
        let limit = Instant::now() + Duration::from_secs(2);
        while Instant::now() < limit {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                self.1 = true;
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        eprintln!("full-volume: child {} cleanup unconfirmed", self.0.id());
    }
}

pub fn bounded(
    command: &mut Command,
    root: &Path,
    name: &str,
    budget: Duration,
) -> io::Result<ExitStatus> {
    command
        .stdout(fresh(&root.join(format!("{name}.stdout")))?)
        .stderr(fresh(&root.join(format!("{name}.stderr")))?)
        .process_group(0);
    let mut child = Running(command.spawn()?, false);
    let limit = Instant::now() + budget;
    loop {
        if let Some(status) = child.0.try_wait()? {
            child.1 = true;
            fs::write(root.join(format!("{name}.status")), format!("{status}\n"))?;
            return Ok(status);
        }
        if Instant::now() >= limit {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("{name} deadline"),
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
}
fn fresh(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}
fn utility(root: &Path, name: &str, arguments: &[&OsStr]) -> io::Result<()> {
    let status = bounded(
        Command::new("/usr/bin/hdiutil").args(arguments),
        root,
        name,
        Duration::from_secs(15),
    )?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("hdiutil {name}: {status}")))
    }
}

fn invalid_attachment() -> io::Error {
    io::Error::other("invalid or ambiguous native image attachment list")
}

// Use the image's own native association, never a cached or inferred disk ID.
fn attached_device(info: &Value, image: &Path) -> io::Result<Option<String>> {
    let images = info
        .as_dictionary()
        .and_then(|value| value.get("images"))
        .and_then(Value::as_array)
        .ok_or_else(invalid_attachment)?;
    let mut found = None;
    for entry in images {
        let entry = entry.as_dictionary().ok_or_else(invalid_attachment)?;
        let path = entry
            .get("image-path")
            .and_then(Value::as_string)
            .ok_or_else(invalid_attachment)?;
        if Path::new(path) != image {
            continue;
        }
        if found.is_some() {
            return Err(invalid_attachment());
        }
        let entities = entry
            .get("system-entities")
            .and_then(Value::as_array)
            .ok_or_else(invalid_attachment)?;
        let devices = entities
            .iter()
            .map(|entity| {
                entity
                    .as_dictionary()
                    .and_then(|entity| entity.get("dev-entry"))
                    .and_then(Value::as_string)
                    .ok_or_else(invalid_attachment)
            })
            .collect::<io::Result<Vec<_>>>()?;
        let whole: Vec<_> = devices
            .iter()
            .copied()
            .filter(|device| {
                device.strip_prefix("/dev/disk").is_some_and(|suffix| {
                    !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
                })
            })
            .collect();
        let [whole] = whole.as_slice() else {
            return Err(invalid_attachment());
        };
        if devices.iter().any(|device| {
            *device != *whole
                && !device.strip_prefix(*whole).is_some_and(|suffix| {
                    suffix.strip_prefix('s').is_some_and(|partition| {
                        !partition.is_empty() && partition.bytes().all(|byte| byte.is_ascii_digit())
                    })
                })
        }) {
            return Err(invalid_attachment());
        }
        found = Some((*whole).to_owned());
    }
    Ok(found)
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let budget = deadline.saturating_duration_since(Instant::now());
    if budget.is_zero() {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "image cleanup deadline",
        ))
    } else {
        Ok(budget)
    }
}

pub struct FullVolume {
    root: PathBuf,
    image: PathBuf,
    mount: PathBuf,
    attached: bool,
}
impl FullVolume {
    pub fn new(root: &Path) -> io::Result<Self> {
        fs::DirBuilder::new().mode(0o700).create(root)?;
        let root = root.canonicalize()?;
        let mut volume = Self {
            image: root.join("full.dmg"),
            mount: root.join("mount"),
            root,
            attached: false,
        };
        utility(
            &volume.root,
            "create",
            &[
                OsStr::new("create"),
                OsStr::new("-size"),
                OsStr::new("8m"),
                OsStr::new("-fs"),
                OsStr::new("HFS+"),
                OsStr::new("-volname"),
                OsStr::new("FastmashFault"),
                volume.image.as_os_str(),
            ],
        )?;
        fs::create_dir(&volume.mount)?;
        // An unsuccessful attach may already have mounted the owned image.
        // Drop must attempt cleanup before leaving its partial captures.
        volume.attached = true;
        utility(
            &volume.root,
            "attach",
            &[
                OsStr::new("attach"),
                OsStr::new("-plist"),
                volume.image.as_os_str(),
                OsStr::new("-mountpoint"),
                volume.mount.as_os_str(),
                OsStr::new("-nobrowse"),
                OsStr::new("-noautoopen"),
            ],
        )?;
        if volume
            .current_device("attach-info", Instant::now() + Duration::from_secs(15))?
            .is_none()
        {
            return Err(io::Error::other(
                "attached image missing from native ownership list",
            ));
        }
        Ok(volume)
    }
    pub fn mount(&self) -> &Path {
        &self.mount
    }
    fn current_device(&self, name: &str, deadline: Instant) -> io::Result<Option<String>> {
        let status = bounded(
            Command::new("/usr/bin/hdiutil").args(["info", "-plist"]),
            &self.root,
            name,
            remaining(deadline)?,
        )?;
        if !status.success() {
            return Err(io::Error::other(format!("hdiutil {name}: {status}")));
        }
        let info = Value::from_file(self.root.join(format!("{name}.stdout")))
            .map_err(|error| io::Error::other(error.to_string()))?;
        attached_device(&info, &self.image)
    }
    fn cleanup(&mut self) -> io::Result<()> {
        if self.attached {
            // A busy detach may already have unmounted the filesystem. Resolve
            // this image's whole device afresh before each bounded attempt.
            let deadline = Instant::now() + Duration::from_secs(15);
            for attempt in 0..3 {
                let Some(device) =
                    self.current_device(&format!("cleanup-info-{attempt}"), deadline)?
                else {
                    self.attached = false;
                    break;
                };
                let status = bounded(
                    Command::new("/usr/bin/hdiutil").args(["detach", "-force", &device]),
                    &self.root,
                    &format!("detach-{attempt}"),
                    remaining(deadline)?,
                )?;
                if !status.success() && status.code() != Some(16) {
                    return Err(io::Error::other(format!("hdiutil detach: {status}")));
                }
                thread::sleep(Duration::from_millis(250).min(remaining(deadline)?));
            }
            if self.attached {
                if self
                    .current_device("cleanup-info-final", deadline)?
                    .is_some()
                {
                    // Preserve a read-only inventory of holders of this image's
                    // files. Diagnosis must neither hide the cleanup failure nor
                    // delay it indefinitely when lsof cannot inspect a holder.
                    if let Err(error) = bounded(
                        Command::new("/usr/sbin/lsof")
                            .args([OsStr::new("-nP"), OsStr::new("+D")])
                            .arg(&self.mount)
                            .arg(&self.image),
                        &self.root,
                        "cleanup-holders",
                        Duration::from_secs(2),
                    ) {
                        let _ =
                            fs::write(self.root.join("cleanup-holders.error"), error.to_string());
                    }
                    return Err(io::Error::other(
                        "owned image remains attached after bounded cleanup",
                    ));
                }
                self.attached = false;
            }
        }
        if self.mount.try_exists()? {
            fs::remove_dir(&self.mount)?;
        }
        if self.image.try_exists()? {
            fs::remove_file(&self.image)?;
        }
        fs::write(
            self.root.join("cleanup.ok"),
            b"owned image absent from native attachment list and removed\n",
        )?;
        Ok(())
    }
}
impl Drop for FullVolume {
    fn drop(&mut self) {
        if let Err(error) = self.cleanup() {
            // Keep the image, mount path and logs available on cleanup failure.
            // A failed destructor must fail qualification, including on unwind.
            panic!(
                "full-volume cleanup failed at {}: {error}",
                self.root.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(images: &str) -> Value {
        Value::from_reader_xml(format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>images</key><array>{images}</array></dict></plist>"
        ).as_bytes()).unwrap()
    }

    fn image(path: &str, devices: &[&str]) -> String {
        let entities: String = devices
            .iter()
            .map(|device| format!("<dict><key>dev-entry</key><string>{device}</string></dict>"))
            .collect();
        format!(
            "<dict><key>image-path</key><string>{path}</string><key>image-alias</key><data>AAE=</data><key>system-entities</key><array>{entities}</array></dict>"
        )
    }

    #[test]
    fn native_binding_selects_only_the_owned_image_and_whole_device() {
        let owned = "/owned/ångström.dmg";
        let value = info(&format!(
            "{}{}",
            image("/other/ångström.dmg", &["/dev/disk42", "/dev/disk42s1"]),
            image(owned, &["/dev/disk7", "/dev/disk7s1"]),
        ));
        // No mount point is needed after a partial unmount. Data fields in the
        // native plist remain valid and do not affect the image association.
        assert_eq!(
            attached_device(&value, Path::new(owned))
                .unwrap()
                .as_deref(),
            Some("/dev/disk7")
        );
        assert_eq!(
            attached_device(&value, Path::new("/absent/ångström.dmg")).unwrap(),
            None
        );
        assert_eq!(attached_device(&info(""), Path::new(owned)).unwrap(), None);
    }

    #[test]
    fn ambiguous_or_inconsistent_native_bindings_are_refused() {
        let owned = "/owned/full.dmg";
        for devices in [
            vec!["/dev/disk7s1"],
            vec!["/dev/disk7", "/dev/disk8"],
            vec!["/dev/disk7", "/dev/disk70s1"],
            vec!["/dev/disk7", "/dev/disk7s1/other"],
        ] {
            assert!(attached_device(&info(&image(owned, &devices)), Path::new(owned)).is_err());
        }
        let duplicate = image(owned, &["/dev/disk7"]);
        assert!(
            attached_device(&info(&format!("{duplicate}{duplicate}")), Path::new(owned)).is_err()
        );
        assert!(attached_device(&Value::String("unknown".into()), Path::new(owned)).is_err());
    }

    #[test]
    fn exhausted_cleanup_budget_refuses_another_utility() {
        let error = remaining(Instant::now() - Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
