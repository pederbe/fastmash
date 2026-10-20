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
            "native image transition deadline",
        ))
    } else {
        Ok(budget)
    }
}

// Native image inventories can lag successful attach/detach commands. Keep the
// shared deadline until the required native and filesystem conditions hold.
fn retry_transition(
    deadline: Instant,
    mut attempt: impl FnMut(usize) -> io::Result<bool>,
) -> io::Result<()> {
    let mut index = 0;
    loop {
        remaining(deadline)?;
        match attempt(index) {
            Ok(true) => return Ok(()),
            Ok(false) => {}
            Err(error) if error.raw_os_error() == Some(nix::errno::Errno::EBUSY as i32) => {}
            Err(error) => return Err(error),
        }
        thread::sleep(Duration::from_millis(250).min(remaining(deadline)?));
        index += 1;
    }
}

fn wait_for_attachment(
    deadline: Instant,
    mut observe: impl FnMut(usize) -> io::Result<Option<String>>,
) -> io::Result<()> {
    retry_transition(deadline, |attempt| Ok(observe(attempt)?.is_some()))
}

// Called only after a fresh inventory omits the owned image. A requested
// device path is observed for absence, never reused as detach authority.
fn detachment_proven(successful: bool, requested_device: Option<&Path>) -> io::Result<bool> {
    if successful {
        return Ok(true);
    }
    let Some(device) = requested_device else {
        return Ok(false);
    };
    Ok(!device.try_exists()?)
}

fn request_detach(
    successful: &mut bool,
    requested_device: &mut Option<PathBuf>,
    device: &str,
    request: impl FnOnce() -> io::Result<ExitStatus>,
) -> io::Result<()> {
    // A fresh association invalidates proof about an earlier attachment, even
    // when the next request fails before returning a native exit status.
    *successful = false;
    *requested_device = None;
    let status = request()?;
    if !status.success() && status.code() != Some(16) {
        return Err(io::Error::other(format!("hdiutil detach: {status}")));
    }
    *successful = status.success();
    *requested_device = Some(PathBuf::from(device));
    Ok(())
}

pub struct FullVolume {
    root: PathBuf,
    image: PathBuf,
    mount: PathBuf,
    attached: bool,
}
impl FullVolume {
    fn create(
        root: &Path,
        create: impl FnOnce(&Path, &Path) -> io::Result<()>,
    ) -> io::Result<Self> {
        fs::DirBuilder::new().mode(0o700).create(root)?;
        let root = root.canonicalize()?;
        let image = root.join("full.dmg");
        // Failed creation can leave native state that an inventory momentarily
        // omits. Preserve its image and receipts instead of claiming cleanup.
        create(&root, &image)?;
        Ok(Self {
            image,
            mount: root.join("mount"),
            root,
            attached: false,
        })
    }

    pub fn new(root: &Path) -> io::Result<Self> {
        let mut volume = Self::create(root, |root, image| {
            utility(
                root,
                "create",
                &[
                    OsStr::new("create"),
                    OsStr::new("-size"),
                    OsStr::new("8m"),
                    OsStr::new("-fs"),
                    OsStr::new("HFS+"),
                    OsStr::new("-volname"),
                    OsStr::new("FastmashFault"),
                    image.as_os_str(),
                ],
            )
        })?;
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
        let deadline = Instant::now() + Duration::from_secs(15);
        wait_for_attachment(deadline, |attempt| {
            volume.current_device(&format!("attach-info-{attempt}"), deadline)
        })?;
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
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut successful_detach = false;
        let mut requested_device = None;
        let cleaned = retry_transition(deadline, |attempt| {
            if self.attached
                && let Some(device) =
                    self.current_device(&format!("cleanup-info-{attempt}"), deadline)?
            {
                request_detach(
                    &mut successful_detach,
                    &mut requested_device,
                    &device,
                    || {
                        bounded(
                            Command::new("/usr/bin/hdiutil").args(["detach", "-force", &device]),
                            &self.root,
                            &format!("detach-{attempt}"),
                            remaining(deadline)?,
                        )
                    },
                )?;
                // Success still needs a fresh native absence observation.
                return Ok(false);
            }
            if self.attached && !detachment_proven(successful_detach, requested_device.as_deref())?
            {
                return Ok(false);
            }
            if self.mount.try_exists()? {
                fs::remove_dir(&self.mount)?;
            }
            if self.image.try_exists()? {
                fs::remove_file(&self.image)?;
            }
            Ok(true)
        });
        if let Err(error) = cleaned {
            return Err(self.cleanup_failure(error));
        }
        self.attached = false;
        fs::write(
            self.root.join("cleanup.ok"),
            b"owned image absent from native attachment list and removed\n",
        )?;
        Ok(())
    }

    fn cleanup_failure(&self, error: io::Error) -> io::Error {
        // Preserve a bounded, read-only inventory without hiding the failure.
        if let Err(inventory) = bounded(
            Command::new("/usr/sbin/lsof")
                .args([OsStr::new("-nP"), OsStr::new("+D")])
                .arg(&self.mount)
                .arg(&self.image),
            &self.root,
            "cleanup-holders",
            Duration::from_secs(2),
        ) {
            let _ = fs::write(
                self.root.join("cleanup-holders.error"),
                inventory.to_string(),
            );
        }
        error
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

    #[test]
    fn failed_creation_preserves_partial_image_without_a_cleanup_claim() {
        let directory = crate::process::Scratch::new().unwrap();
        let root = directory.path.join("failed-creation");
        let result = FullVolume::create(&root, |root, image| {
            fs::write(image, b"partial native image")?;
            fs::write(root.join("create.stderr"), b"Resource busy\n")?;
            Err(io::Error::other("hdiutil create: exit status: 1"))
        });
        assert!(result.is_err());
        assert!(!root.join("cleanup.ok").exists());
        assert_eq!(
            fs::read(root.join("full.dmg")).unwrap(),
            b"partial native image"
        );
        assert_eq!(
            fs::read(root.join("create.stderr")).unwrap(),
            b"Resource busy\n"
        );
    }

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
    fn busy_detachment_uses_the_budget_until_absence_is_confirmed() {
        let mut calls = 0;
        retry_transition(Instant::now() + Duration::from_secs(3), |attempt| {
            assert_eq!(attempt, calls);
            calls += 1;
            // Replay three fast busy outcomes, then a native absence result.
            Ok(attempt == 3)
        })
        .unwrap();
        assert_eq!(calls, 4);
    }

    #[test]
    fn expired_detachment_budget_never_calls_the_native_utility() {
        let mut calls = 0;
        let error = retry_transition(Instant::now() - Duration::from_secs(1), |_| {
            calls += 1;
            Ok(true)
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(calls, 0);
    }

    #[test]
    fn unexpected_detachment_errors_stop_without_retry() {
        let mut calls = 0;
        let error = retry_transition(Instant::now() + Duration::from_secs(3), |_| {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(calls, 1);
    }

    #[test]
    fn attachment_waits_for_the_owned_image_to_appear_in_native_inventory() {
        let owned = Path::new("/owned/full.dmg");
        let snapshots = [
            info(""),
            info(&image("/owned/full.dmg", &["/dev/disk7", "/dev/disk7s1"])),
        ];
        let mut calls = 0;
        wait_for_attachment(Instant::now() + Duration::from_secs(3), |attempt| {
            calls += 1;
            attached_device(&snapshots[attempt], owned)
        })
        .unwrap();
        assert_eq!(calls, 2);
    }

    #[test]
    fn busy_mount_after_provisional_absence_preserves_cleanup_ownership() {
        let mut calls = 0;
        retry_transition(Instant::now() + Duration::from_secs(3), |attempt| {
            calls += 1;
            match attempt {
                // Native image absence alone did not release the mount.
                0 => Err(io::Error::from_raw_os_error(
                    nix::errno::Errno::EBUSY as i32,
                )),
                // A fresh observation finds the owned association again;
                // detachment is requested before another absence check.
                1 => Ok(false),
                2 => Ok(true),
                _ => panic!("cleanup retried after confirmed completion"),
            }
        })
        .unwrap();
        assert_eq!(calls, 3);
    }

    #[test]
    fn absent_image_and_empty_mount_do_not_prove_device_detachment() {
        let directory = crate::process::Scratch::new().unwrap();
        let device = directory.path.join("device-node");
        let mount = directory.path.join("unmounted-directory");
        fs::create_dir(&mount).unwrap();
        fs::write(&device, b"still present").unwrap();
        fs::remove_dir(&mount).unwrap();
        let image = directory.path.join("backing-image");
        fs::write(&image, b"still open").unwrap();
        let held = File::open(&image).unwrap();
        fs::remove_file(&image).unwrap();
        assert_eq!(held.metadata().unwrap().len(), 10);
        assert!(!detachment_proven(false, None).unwrap());
        assert!(!detachment_proven(false, Some(&device)).unwrap());
        fs::remove_file(&device).unwrap();
        assert!(detachment_proven(false, Some(&device)).unwrap());
        // A successful owned detach remains proof if that device name is reused.
        fs::write(&device, b"reused name").unwrap();
        assert!(detachment_proven(true, Some(&device)).unwrap());
    }

    #[test]
    fn a_new_association_invalidates_old_proof_before_a_busy_request() {
        let mut successful = true;
        let mut requested = Some(PathBuf::from("/dev/disk7"));
        let error = request_detach(&mut successful, &mut requested, "/dev/disk8", || {
            Err(io::Error::from_raw_os_error(
                nix::errno::Errno::EBUSY as i32,
            ))
        })
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(nix::errno::Errno::EBUSY as i32));
        assert!(!detachment_proven(successful, requested.as_deref()).unwrap());
        assert!(requested.is_none());
    }

    #[test]
    fn missing_attachment_still_refuses_when_its_budget_expires() {
        let error = wait_for_attachment(Instant::now() + Duration::from_millis(20), |_| Ok(None))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn permanently_busy_cleanup_still_refuses_at_its_deadline() {
        let error = retry_transition(Instant::now() + Duration::from_millis(20), |_| {
            Err(io::Error::from_raw_os_error(
                nix::errno::Errno::EBUSY as i32,
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn exhausted_cleanup_budget_refuses_another_utility() {
        let error = remaining(Instant::now() - Duration::from_secs(1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
