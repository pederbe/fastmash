#![forbid(unsafe_code)]
//! Own an isolated macOS full-output volume through normal and panic cleanup.
use nix::{
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
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
                volume.image.as_os_str(),
                OsStr::new("-mountpoint"),
                volume.mount.as_os_str(),
                OsStr::new("-nobrowse"),
                OsStr::new("-noautoopen"),
            ],
        )?;
        Ok(volume)
    }
    pub fn mount(&self) -> &Path {
        &self.mount
    }
    fn cleanup(&mut self) -> io::Result<()> {
        if self.attached {
            utility(
                &self.root,
                "detach",
                &[
                    OsStr::new("detach"),
                    // The probe has already reaped every candidate. A native
                    // detach can still report EBUSY for this private volume.
                    // Force only its owned mount, with captures kept outside.
                    OsStr::new("-force"),
                    self.mount.as_os_str(),
                ],
            )?;
            self.attached = false;
            fs::remove_dir(&self.mount)?;
        }
        if self.image.try_exists()? {
            fs::remove_file(&self.image)?;
        }
        fs::write(
            self.root.join("cleanup.ok"),
            b"owned image detached and removed\n",
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
