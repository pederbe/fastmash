//! The external sort route's companion: argument validation and cancellation.
use std::{
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const SUPERVISOR: &str = env!("CARGO_BIN_EXE_fastmash-sort-supervisor");

fn wait(child: &mut Child, limit: Duration) -> ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if start.elapsed() > limit {
            let _ = child.kill();
            let _ = child.wait();
            panic!("process did not exit within {limit:?}");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn exec_sort(parent: &str, root: &str) -> std::process::Output {
    Command::new(SUPERVISOR)
        .args(["--exec-sort", parent, root, "0", "9", "1", "1"])
        .env_clear()
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn exec_sort_accepts_only_its_own_temporary_root() {
    let hex = "0123456789abcdef0123456789abcdef";
    let parent = std::process::id().to_string();
    for root in [
        format!("tmp/fastmash-sort-{hex}"),
        format!("/tmp//fastmash-sort-{hex}"),
        format!("/tmp/fastmash-sort-{}", &hex[..31]),
        format!("/tmp/fastmash-sort-{hex}0"),
        format!("/tmp/fastmash-sort-{}/..", &hex[..29]),
        "/tmp/fastmash-sort-0123456789abcdef0123456789abcdeg".to_string(),
        "/tmp".to_string(),
    ] {
        let output = exec_sort(&parent, &root);
        assert_eq!(output.status.code(), Some(77), "{root}");
        assert_eq!(
            output.stderr, b"fastmash-sort-supervisor: invalid supervisor arguments\n",
            "{root}"
        );
    }
    let output = exec_sort("1", &format!("/tmp/fastmash-sort-{hex}"));
    assert_eq!(output.status.code(), Some(77));
    assert_eq!(
        output.stderr,
        b"fastmash-sort-supervisor: invalid supervisor arguments\n"
    );
    // Controls: a well-formed root under any directory (TMPDIR) reaches sort,
    // which sorts empty input.
    for base in ["/tmp", "/var/tmp"] {
        let output = exec_sort(&parent, &format!("{base}/fastmash-sort-{hex}"));
        assert!(output.status.success(), "{output:?}");
        assert!(output.stdout.is_empty());
    }
}

/// Start an external-route job with `TMPDIR` set (or unset for `None`), in
/// `cwd`, keeping its input open so the sort is still running.
fn start_external(tmpdir: Option<&std::ffi::OsStr>, cwd: &std::path::Path) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash"));
    // `geomean` in the C locale takes the external sort route.
    command
        .args(["-s", "-g", "1", "geomean", "2"])
        .env_clear()
        .env("LC_ALL", "C")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(tmpdir) = tmpdir {
        command.env("TMPDIR", tmpdir);
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"b\t2\na\t1\n")
        .unwrap();
    child
}

fn running_root(fastmash: &mut Child) -> PathBuf {
    let start = Instant::now();
    loop {
        if let Some(root) = sort_root(fastmash.id()).filter(|root| root.exists()) {
            return root;
        }
        if fastmash.try_wait().unwrap().is_some() {
            panic!("fastmash exited early");
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the external sort did not start"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// A fresh directory, canonical (fastmash resolves relative paths against the
/// physical working directory) and removed even when a test fails.
struct Scratch(PathBuf);
impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("fastmash-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(fs::canonicalize(dir).unwrap())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn external_sort_uses_tmpdir_and_cleans_it() {
    let base = Scratch::new("tmpdir");
    let cwd = Scratch::new("cwd");
    let (base, cwd) = (&base.0, &cwd.0);
    fs::create_dir(cwd.join("relative")).unwrap();
    let parent_name = cwd.file_name().unwrap().to_str().unwrap();
    let spelled = |s: String| std::ffi::OsString::from(s);
    for (tmpdir, expected) in [
        (Some(base.clone().into_os_string()), base.clone()),
        (Some(spelled("relative".into())), cwd.join("relative")),
        // Spellings GNU sort accepts: normalised, never resolved.
        (Some(spelled("./relative/".into())), cwd.join("relative")),
        (Some(spelled(format!("{}//", base.display()))), base.clone()),
        (Some(spelled(format!("{}/.", base.display()))), base.clone()),
        (
            Some(spelled(format!("../{parent_name}/relative"))),
            cwd.join(format!("../{parent_name}/relative")),
        ),
        (None, PathBuf::from("/tmp")),
    ] {
        let mut fastmash = start_external(tmpdir.as_deref(), cwd);
        let root = running_root(&mut fastmash);
        assert_eq!(root.parent(), Some(expected.as_path()), "{tmpdir:?}");
        drop(fastmash.stdin.take());
        let status = wait(&mut fastmash, Duration::from_secs(10));
        assert!(status.success(), "{tmpdir:?}: {status:?}");
        let mut output = String::new();
        std::io::Read::read_to_string(fastmash.stdout.as_mut().unwrap(), &mut output).unwrap();
        assert_eq!(output, "a\t1\nb\t2\n", "{tmpdir:?}");
        assert!(!root.exists(), "{root:?} remains");
    }
    assert_eq!(fs::read_dir(base).unwrap().count(), 0);
    assert_eq!(fs::read_dir(cwd.join("relative")).unwrap().count(), 0);
}

/// The running sort's argument that starts with `prefix`, once it runs.
fn sort_argument(fastmash: &mut Child, prefix: &str) -> String {
    let start = Instant::now();
    loop {
        for supervisor in children(fastmash.id()) {
            for sort in children(supervisor) {
                let cmdline = fs::read(format!("/proc/{sort}/cmdline")).unwrap_or_default();
                for arg in cmdline.split(|b| *b == 0) {
                    let arg = String::from_utf8_lossy(arg);
                    if arg.starts_with(prefix) {
                        return arg.into_owned();
                    }
                }
            }
        }
        assert!(
            fastmash.try_wait().unwrap().is_none(),
            "fastmash exited early"
        );
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "the external sort did not start"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn external_sort_threads_follow_gnu_sort_defaults() {
    let available = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8);
    for (omp, expected) in [
        (vec![], available),
        (vec![("OMP_NUM_THREADS", "3")], 3),
        (vec![("OMP_NUM_THREADS", "5"), ("OMP_THREAD_LIMIT", "2")], 2),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash"));
        command
            .args(["-s", "-g", "1", "geomean", "2"])
            .env_clear()
            .env("LC_ALL", "C")
            .envs(omp.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut fastmash = command.spawn().unwrap();
        fastmash
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"b\t2\na\t1\n")
            .unwrap();
        let parallel = sort_argument(&mut fastmash, "--parallel=");
        assert_eq!(parallel, format!("--parallel={expected}"), "{omp:?}");
        drop(fastmash.stdin.take());
        assert!(wait(&mut fastmash, Duration::from_secs(10)).success());
    }
}

#[test]
fn unusable_tmpdir_is_a_clear_error() {
    let cwd = Scratch::new("unusable");
    for (tmpdir, shown) in [("/nonexistent/fastmash", "/nonexistent/fastmash"), ("", "")] {
        let output = Command::new(env!("CARGO_BIN_EXE_fastmash"))
            .args(["-s", "-g", "1", "geomean", "2"])
            .env_clear()
            .env("LC_ALL", "C")
            .env("TMPDIR", tmpdir)
            .current_dir(&cwd.0)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{tmpdir:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!(
                "{}: sort temporary I/O error: cannot create a directory in '{shown}': \
                 No such file or directory (os error 2)\n",
                env!("CARGO_BIN_EXE_fastmash")
            ),
            "{tmpdir:?}"
        );
    }
}

/// Direct children of `pid`, from each process's parent in /proc/*/stat.
fn children(pid: u32) -> Vec<u32> {
    let mut found = Vec::new();
    for entry in fs::read_dir("/proc").unwrap().flatten() {
        let Some(child) = entry
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(stat) = fs::read_to_string(format!("/proc/{child}/stat")) else {
            continue;
        };
        // "pid (comm) state ppid ...": comm may contain spaces or parentheses.
        let after = &stat[stat.rfind(')').unwrap() + 2..];
        if after.split(' ').nth(1).and_then(|p| p.parse().ok()) == Some(pid) {
            found.push(child);
        }
    }
    found
}

/// The temporary root of the sort under `fastmash`'s supervisor, once running.
fn sort_root(fastmash: u32) -> Option<PathBuf> {
    for supervisor in children(fastmash) {
        for sort in children(supervisor) {
            let cmdline = fs::read(format!("/proc/{sort}/cmdline")).ok()?;
            for arg in cmdline.split(|b| *b == 0) {
                if let Some(root) = arg.strip_prefix(b"--temporary-directory=") {
                    return Some(PathBuf::from(String::from_utf8_lossy(root).into_owned()));
                }
            }
        }
    }
    None
}

#[test]
fn interrupted_external_sort_leaves_no_temporary_directory() {
    let mut fastmash = Command::new(env!("CARGO_BIN_EXE_fastmash"))
        // `geomean` in the C locale takes the external sort route.
        .args(["-s", "-g", "1", "geomean", "2"])
        .env_clear()
        .env("LC_ALL", "C")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // Input stays open, so the sort is still running when interrupted.
    let mut input = fastmash.stdin.take().unwrap();
    input.write_all(b"b\t2\na\t1\n").unwrap();
    let start = Instant::now();
    let root = loop {
        if let Some(root) = sort_root(fastmash.id()).filter(|root| root.exists()) {
            break root;
        }
        if fastmash.try_wait().unwrap().is_some() {
            let output = fastmash.wait_with_output().unwrap();
            panic!("fastmash exited early: {output:?}");
        }
        if start.elapsed() > Duration::from_secs(10) {
            let _ = fastmash.kill();
            panic!("the external sort did not start");
        }
        thread::sleep(Duration::from_millis(5));
    };
    // SAFETY: kill takes no pointers; the child is unreaped, so its PID is ours.
    assert_eq!(unsafe { libc::kill(fastmash.id() as i32, libc::SIGINT) }, 0);
    let status = wait(&mut fastmash, Duration::from_secs(10));
    assert_eq!(status.signal(), Some(libc::SIGINT), "{status:?}");
    // TMPDIR is unset here, so the root is in /tmp.
    assert_eq!(root.parent(), Some(std::path::Path::new("/tmp")));
    drop(input);
    let start = Instant::now();
    while root.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "temporary directory remains: {root:?}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}
