//! The external-sort companion's process roles, run by the
//! `fastmash-sort-supervisor` executable installed beside `fastmash`.
use crate::{Kind, Message, linux};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, DirBuilder},
    io,
    os::{
        fd::{AsFd, FromRawFd, OwnedFd},
        unix::{
            ffi::{OsStrExt, OsStringExt},
            fs::DirBuilderExt,
            process::{CommandExt, ExitStatusExt},
        },
    },
    path::{Component, Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// The sort configuration passed on the companion's command line; the one
/// encoder (`args`) and decoder (`parse`) of that format.
pub(crate) struct Config {
    pub(crate) mask: u64,
    pub(crate) separator: Option<u8>,
    pub(crate) keys: Vec<u64>,
    pub(crate) zero_terminated: bool,
    pub(crate) ignore_case: bool,
    /// `sort --parallel`, 1 to 8.
    pub(crate) threads: u32,
}
impl Config {
    fn parse(args: &[String]) -> io::Result<Self> {
        if !(4..=5).contains(&args.len()) {
            return Err(invalid());
        }
        let (zero_terminated, ignore_case) = match args.get(4).map(String::as_str) {
            None => (false, false),
            Some("nul") => (true, false),
            Some("fold") => (false, true),
            Some("nul-fold") => (true, true),
            _ => return Err(invalid()),
        };
        let mask = args[0].parse().map_err(|_| invalid())?;
        if mask & (linux::CANCEL | (1 << 12)) != 0 {
            return Err(invalid());
        }
        let separator = if args[1] == "whitespace" {
            None
        } else {
            let byte = args[1].parse::<u8>().map_err(|_| invalid())?;
            if byte == 0 {
                return Err(invalid());
            }
            Some(byte)
        };
        let keys = args[2]
            .split(',')
            .map(|s| s.parse::<u64>().map_err(|_| invalid()))
            .collect::<io::Result<Vec<_>>>()?;
        if keys.is_empty() {
            return Err(invalid());
        }
        let threads = args[3].parse::<u32>().map_err(|_| invalid())?;
        if !(1..=8).contains(&threads) {
            return Err(invalid());
        }
        Ok(Self {
            mask,
            separator,
            keys,
            zero_terminated,
            ignore_case,
            threads,
        })
    }
    pub(crate) fn args(&self, command: &mut Command) {
        command
            .arg(self.mask.to_string())
            .arg(
                self.separator
                    .map_or_else(|| "whitespace".into(), |b| b.to_string()),
            )
            .arg(
                self.keys
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .arg(self.threads.to_string());
        if self.zero_terminated || self.ignore_case {
            command.arg(match (self.zero_terminated, self.ignore_case) {
                (true, true) => "nul-fold",
                (true, false) => "nul",
                _ => "fold",
            });
        }
    }
}
fn invalid() -> io::Error {
    io::Error::other("invalid supervisor arguments")
}

fn command() -> io::Result<Command> {
    let mut command = Command::new(std::env::current_exe()?);
    command
        .env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("LANGUAGE", "C")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    Ok(command)
}

/// Whether `root` is a directory `TempRoot::create` could have made: an
/// absolute path in the normal form `Session::start` gives the base (no `.`,
/// repeated or trailing separators; `..` may remain), whose last component is
/// `fastmash-sort-` and 32 hexadecimal digits.
fn valid_root(root: &OsStr) -> bool {
    let path = Path::new(root);
    let normal = path.components().all(|c| {
        matches!(
            c,
            Component::RootDir | Component::Normal(_) | Component::ParentDir
        )
    });
    let rebuilt: PathBuf = path.components().collect();
    let name = path.file_name().map(OsStr::as_bytes).unwrap_or_default();
    path.is_absolute()
        && normal
        && rebuilt.as_os_str() == root
        && name.len() == 14 + 32
        && name.starts_with(b"fastmash-sort-")
        && name[14..].iter().all(u8::is_ascii_hexdigit)
}

/// Why the private directory could not be made.
enum CreateError {
    /// Creating it in the base directory failed (reported with the base).
    Directory(io::Error),
    Other(io::Error),
}

struct TempRoot(PathBuf);
impl TempRoot {
    fn create(base: &Path) -> Result<Self, CreateError> {
        for _ in 0..8 {
            let mut random = [0u8; 16];
            linux::random(&mut random).map_err(CreateError::Other)?;
            let name = random
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            let path = base.join(format!("fastmash-sort-{name}"));
            match DirBuilder::new().mode(0o700).create(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(CreateError::Directory(e)),
            }
        }
        // Eight random names all taken.
        Err(CreateError::Directory(io::Error::from_raw_os_error(17)))
    }
    fn clean(&self) -> io::Result<()> {
        // std's Unix remove_dir_all does not follow symlinks in the tree.
        match fs::remove_dir_all(&self.0) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}
impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = self.clean();
    }
}

struct Sorter {
    child: Child,
    status: Option<i32>,
    cancelled: Option<Instant>,
}
impl Sorter {
    fn check(&mut self) -> io::Result<()> {
        if self.status.is_none() {
            self.status = self.child.try_wait()?.map(ExitStatusExt::into_raw);
        }
        Ok(())
    }
    fn signal(&mut self, signal: i32) -> io::Result<()> {
        self.check()?;
        if self.status.is_none() {
            // An unreaped child PID cannot be reused.
            match linux::signal_pid(self.child.id(), signal) {
                Err(e) if e.raw_os_error() == Some(3) => {}
                result => result?,
            }
        }
        Ok(())
    }
    fn cancel(&mut self) -> io::Result<()> {
        if self.cancelled.is_none() {
            self.cancelled = Some(Instant::now());
            self.signal(15)?;
        }
        if self
            .cancelled
            .is_some_and(|at| at.elapsed() >= Duration::from_secs(1))
        {
            self.signal(9)?;
        }
        Ok(())
    }
}
impl Drop for Sorter {
    fn drop(&mut self) {
        if self.status.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

// On every early return, sorter destruction precedes directory destruction.
fn supervise(control: OwnedFd, parent: OwnedFd, base: &Path, config: Config) -> io::Result<()> {
    linux::close_on_exec_extras()?;
    if linux::mask(None)? & linux::CANCEL != linux::CANCEL {
        return Err(invalid());
    }
    let signals = linux::signal_fd()?;
    let mut root = None;
    let mut sorter = None;
    let mut temporary_failed = false;
    let setup = (|| {
        root = Some(TempRoot::create(base).map_err(|error| match error {
            CreateError::Directory(error) => {
                temporary_failed = true;
                error
            }
            CreateError::Other(error) => error,
        })?);
        let mut launch = command()?;
        launch
            .arg("--exec-sort")
            .arg(std::process::id().to_string())
            .arg(&root.as_ref().unwrap().0);
        config.args(&mut launch);
        let child = launch.spawn()?;
        sorter = Some(Sorter {
            child,
            status: None,
            cancelled: None,
        });
        Ok::<_, io::Error>(())
    })();
    // SAFETY: no Rust value in this process owns standard input or output,
    // and the supervisor never uses them again.
    unsafe { linux::close_streams() };
    let mut pending = Some(match setup {
        Ok(()) => Message::new(Kind::Ready),
        Err(e) => Message {
            kind: if temporary_failed {
                Kind::TemporaryFailed
            } else {
                Kind::Failed
            },
            status: e.raw_os_error().unwrap_or(5),
            cleanup: root
                .as_ref()
                .and_then(|r| r.clean().err())
                .map_or(0, |e| e.raw_os_error().unwrap_or(5)),
        },
    });
    let mut ready = false;
    let mut finish = false;
    let mut abort = false;
    let mut peer_alive = true;
    loop {
        let mut polls = [
            linux::Poll::new(parent.as_fd(), 1),
            linux::Poll::new(control.as_fd(), 1 | if pending.is_some() { 4 } else { 0 }),
            linux::Poll::new(signals.as_fd(), 1),
        ];
        linux::poll(&mut polls, 25)?;
        if polls[0].returned != 0 {
            peer_alive = false;
            abort = true;
        }
        while let Some(signal) = linux::read_signal(&signals)? {
            let _ = linux::pid_signal(parent.as_fd(), signal);
            if let Some(child) = &mut sorter {
                child.signal(signal)?;
            }
            abort = true;
        }
        if peer_alive && polls[1].returned & (1 | 8 | 16 | 32) != 0 {
            match Message::receive(control.as_fd()) {
                Ok(Some(message))
                    if ready
                        && !finish
                        && !abort
                        && message.status == 0
                        && message.cleanup == 0 =>
                {
                    match message.kind {
                        Kind::Finish => finish = true,
                        Kind::Abort => abort = true,
                        _ => {
                            peer_alive = false;
                            abort = true;
                        }
                    }
                }
                Ok(None) => {}
                _ => {
                    peer_alive = false;
                    abort = true;
                }
            }
        }
        if let Some(child) = &mut sorter {
            child.check()?;
            if abort {
                child.cancel()?;
            }
            if (finish || abort)
                && let Some(status) = child.status
                && pending.is_none()
            {
                let cleanup = root
                    .as_ref()
                    .and_then(|r| r.clean().err())
                    .map_or(0, |e| e.raw_os_error().unwrap_or(5));
                pending = Some(Message {
                    kind: Kind::Done,
                    status,
                    cleanup,
                });
            }
        }
        if !peer_alive {
            if sorter.as_ref().is_none_or(|s| s.status.is_some()) {
                return Ok(());
            }
            // Drop startup replies after peer death so cancellation can complete.
            pending = None;
            continue;
        }
        if let Some(message) = pending {
            match message.send(control.as_fd()) {
                Ok(()) => {
                    pending = None;
                    if message.kind == Kind::Ready {
                        ready = true;
                    } else {
                        return Ok(());
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                Err(_) => {
                    peer_alive = false;
                    abort = true;
                }
            }
        }
    }
}

fn exec_sort(parent: u32, root: &OsStr, config: Config) -> io::Result<()> {
    linux::parent_death(parent)?;
    linux::close_on_exec_extras()?;
    linux::default_sigpipe()?;
    linux::restore_mask(config.mask);
    let mut sort = Command::new(crate::SYSTEM_SORT);
    sort.env_clear()
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .env("LANGUAGE", "C")
        // As GNU datamash runs it, except that the thread count, which sort
        // would read from OMP_* in the environment cleared here, is explicit.
        // No --buffer-size: sort sizes its buffer as it does for datamash.
        .arg("-s")
        .arg(format!("--parallel={}", config.threads))
        .arg({
            let mut arg = OsString::from("--temporary-directory=");
            arg.push(root);
            arg
        });
    if let Some(byte) = config.separator {
        sort.arg("-t").arg(OsString::from_vec(vec![byte]));
    }
    if config.zero_terminated {
        sort.arg("-z");
    }
    if config.ignore_case {
        sort.arg("-f");
    }
    for key in config.keys {
        sort.arg(format!("-k{key},{key}"));
    }
    Err(sort.exec())
}
fn text(arg: &OsStr) -> io::Result<String> {
    arg.to_str().map(String::from).ok_or_else(invalid)
}
fn run() -> io::Result<()> {
    // Paths may be any bytes, so arguments are read as OsString.
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    // --supervise-PROTOCOL CONTROL PARENT BASE CONFIG... | --exec-sort PARENT ROOT CONFIG...
    let (role, fixed) = match args.first().and_then(|a| a.to_str()) {
        Some(role) if role == format!("--supervise-{}", crate::PROTOCOL) => ("--supervise", 4),
        Some(role) if role.starts_with("--supervise-") => {
            mismatch(args.get(1));
            return Err(invalid());
        }
        Some("--exec-sort") => ("--exec-sort", 3),
        _ => return Err(invalid()),
    };
    if !(fixed + 4..=fixed + 5).contains(&args.len()) {
        return Err(invalid());
    }
    let config = Config::parse(
        &args[fixed..]
            .iter()
            .map(|a| text(a))
            .collect::<io::Result<Vec<_>>>()?,
    )?;
    match role {
        "--supervise" => {
            let control: i32 = text(&args[1])?.parse().map_err(|_| invalid())?;
            let parent: i32 = text(&args[2])?.parse().map_err(|_| invalid())?;
            let base = Path::new(&args[3]);
            if !base.is_absolute() {
                return Err(invalid());
            }
            if control <= 2
                || parent <= 2
                || control == parent
                || !linux::is_open(control)
                || !linux::is_open(parent)
            {
                return Err(invalid());
            }
            // SAFETY: checked open and distinct just above; inherited from the
            // parent for this process alone, so nothing else here owns them.
            let control = unsafe { OwnedFd::from_raw_fd(control) };
            // SAFETY: as above.
            let parent = unsafe { OwnedFd::from_raw_fd(parent) };
            linux::cloexec(control.as_fd(), true)?;
            linux::cloexec(parent.as_fd(), true)?;
            supervise(control, parent, base, config)
        }
        _ => {
            let parent: u32 = text(&args[1])?.parse().map_err(|_| invalid())?;
            if parent < 2 || !valid_root(&args[2]) {
                return Err(invalid());
            }
            exec_sort(parent, &args[2], config)
        }
    }
}
/// Tells a `fastmash` from another release, through the control descriptor
/// every protocol version passes after the role, that this companion does
/// not speak its version.
fn mismatch(control: Option<&OsString>) {
    let Some(control) = control.and_then(|c| c.to_str()?.parse::<i32>().ok()) else {
        return;
    };
    if control > 2 && linux::is_open(control) {
        // SAFETY: an open descriptor inherited for this process alone; it is
        // borrowed only to send one message before the process exits.
        let control = unsafe { std::os::fd::BorrowedFd::borrow_raw(control) };
        let _ = Message::new(Kind::Mismatch).send(control);
    }
}
/// Runs the supervisor or sort-exec role named by the first argument.
pub fn main() {
    if let Err(error) = run() {
        // Cleanup and status must not wait on an inherited stderr after peer death.
        // Only the exec role emits a sorter-startup diagnostic while the original
        // is still consuming its pipes. The supervisor reports through control.
        if std::env::args_os().nth(1).as_deref() == Some(OsStr::new("--exec-sort")) {
            report(&error);
        }
        std::process::exit(77);
    }
}
/// Writes the exec role's diagnostic. A failed write is ignored, so the role
/// exits 77 whatever its standard error is (`eprintln!` would panic, and the
/// process abort). SIGPIPE, whose default the role restores for the sort just
/// before exec, is ignored again first: a pipe without a reader then fails
/// the write instead of ending the process.
fn report(error: &io::Error) {
    let _ = linux::ignore_sigpipe();
    // Formatted first, so that it goes out in one write.
    let message = format!("fastmash-sort-supervisor: {error}\n");
    let _ = io::Write::write_all(&mut io::stderr(), message.as_bytes());
}

#[cfg(test)]
mod framing_tests {
    use super::*;
    #[test]
    fn configuration_roundtrips_record_and_case_modes() {
        for tail in [None, Some("nul"), Some("fold"), Some("nul-fold")] {
            let mut args: Vec<String> = ["0", "9", "1,2", "4"]
                .into_iter()
                .map(String::from)
                .collect();
            if let Some(tail) = tail {
                args.push(tail.into());
            }
            let config = Config::parse(&args).unwrap();
            assert_eq!(
                config.zero_terminated,
                matches!(tail, Some("nul" | "nul-fold"))
            );
            assert_eq!(
                config.ignore_case,
                matches!(tail, Some("fold" | "nul-fold"))
            );
            let mut command = Command::new("unused");
            config.args(&mut command);
            let serialized: Vec<_> = command
                .get_args()
                .map(|arg| arg.to_str().unwrap().to_string())
                .collect();
            assert_eq!(serialized, args);
            let again = Config::parse(&serialized).unwrap();
            assert_eq!(again.keys, vec![1, 2]);
            assert_eq!(again.separator, Some(9));
            assert_eq!(again.zero_terminated, config.zero_terminated);
            assert_eq!(again.ignore_case, config.ignore_case);
            assert_eq!(again.threads, 4);
        }
        assert!(Config::parse(&["0", "9", "1", "4", "lf"].map(String::from)).is_err());
        assert!(Config::parse(&["0", "9", "1", "4", "nul", "extra"].map(String::from)).is_err());
    }

    #[test]
    fn configuration_rejects_blocked_cancellation_and_malformed_fields() {
        // SIGHUP, SIGINT and SIGTERM (CANCEL) and SIGPIPE's bit must be unblocked.
        for mask in [1u64, 2, 1 << 12, 1 << 14] {
            assert!(
                Config::parse(&[mask.to_string(), "9".into(), "1".into(), "1".into()]).is_err()
            );
        }
        for (separator, keys) in [
            ("0", "1"),
            ("256", "1"),
            ("tab", "1"),
            ("9", ""),
            ("9", "1,,2"),
            ("9", "x"),
        ] {
            let args = ["0", separator, keys, "1"].map(String::from);
            assert!(Config::parse(&args).is_err(), "{args:?}");
        }
        assert!(Config::parse(&["0", "whitespace", "1", "1"].map(String::from)).is_ok());
        for threads in ["0", "9", "x", ""] {
            let args = ["0", "9", "1", threads].map(String::from);
            assert!(Config::parse(&args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn only_created_temporary_roots_are_valid() {
        let hex = "0123456789abcdef0123456789abcdef";
        for root in [
            format!("/tmp/fastmash-sort-{hex}"),
            format!("/var/tmp/fastmash-sort-{hex}"),
            format!("/scratch dir/x/fastmash-sort-{hex}"),
            format!("/home/u/../scratch/fastmash-sort-{hex}"),
        ] {
            assert!(valid_root(OsStr::new(&root)), "{root}");
        }
        for root in [
            format!("tmp/fastmash-sort-{hex}"),
            format!("/tmp/./fastmash-sort-{hex}"),
            format!("/tmp/fastmash-sort-{hex}/.."),
            format!("/tmp//fastmash-sort-{hex}"),
            format!("/tmp/fastmash-sort-{hex}/"),
            format!("/tmp/fastmash-sort-{}", &hex[..31]),
            format!("/tmp/fastmash-sort-{hex}0"),
            "/tmp/fastmash-sort-0123456789abcdef0123456789abcdeg".into(),
            "/tmp".into(),
            "/".into(),
            String::new(),
        ] {
            assert!(!valid_root(OsStr::new(&root)), "{root}");
        }
        let mut bytes = b"/\xff/fastmash-sort-".to_vec();
        bytes.extend_from_slice(hex.as_bytes());
        assert!(valid_root(OsStr::from_bytes(&bytes)));
    }
}
