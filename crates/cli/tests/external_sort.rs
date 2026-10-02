//! The external sort route's companion: argument validation and cancellation.
//! Every command sets FASTMASH_GROUPING=sort: hash grouping would otherwise
//! group input from a file without the system sort.
use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::process::ExitStatusExt,
    path::{Path, PathBuf},
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

/// The exec role exits 77 after a failure whatever its standard error is: a
/// full device (`ENOSPC`) or a pipe without a reader (`EPIPE`). It fails
/// before exec, on arguments it rejects, while SIGPIPE is still ignored, or at
/// exec, after restoring SIGPIPE's default for the sort: under a 512 KiB stack
/// limit, which leaves 128 KiB for arguments, its own fit but the sort's
/// 20000 key options do not (`E2BIG`).
#[test]
fn the_exec_role_exits_77_whatever_its_standard_error() {
    use std::os::unix::process::CommandExt;
    let parent = std::process::id().to_string();
    let root = "/tmp/fastmash-sort-0123456789abcdef0123456789abcdef";
    let keys = format!("1{}", ",1".repeat(19999));
    let rejected = ["--exec-sort", &parent, "/tmp", "0", "9", "1", "1"];
    let unexecutable = ["--exec-sort", &parent, root, "0", "9", &keys, "1"];
    let run = |args: &[&str], stderr: Stdio| {
        let mut command = Command::new(SUPERVISOR);
        command
            .args(args)
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(stderr);
        // SAFETY: the hook runs in the forked child before exec and makes
        // only getrlimit and setrlimit calls, which are async-signal-safe.
        unsafe { command.pre_exec(small_stack) };
        command.output().unwrap()
    };
    let full = || {
        let device = fs::OpenOptions::new().write(true).open("/dev/full");
        Stdio::from(device.unwrap())
    };
    let unread = || {
        let (reader, writer) = std::io::pipe().unwrap();
        drop(reader);
        Stdio::from(writer)
    };
    for (args, reason) in [
        (&rejected[..], "invalid supervisor arguments"),
        (&unexecutable, "Argument list too long (os error 7)"),
    ] {
        let output = run(args, Stdio::piped());
        assert_eq!(output.status.code(), Some(77), "{reason}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!("fastmash-sort-supervisor: {reason}\n")
        );
        for (sink, stderr) in [("full", full()), ("unread", unread())] {
            let status = run(args, stderr).status;
            assert_eq!(status.code(), Some(77), "{reason}, {sink}: {status:?}");
        }
    }
}

/// Lowers the stack limit to 512 KiB (or the hard limit, if lower).
fn small_stack() -> std::io::Result<()> {
    let mut limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit and setrlimit only access `limit`, live for both calls.
    let set = unsafe {
        libc::getrlimit(libc::RLIMIT_STACK, &mut limit) == 0 && {
            limit.rlim_cur = limit.rlim_max.min(512 << 10);
            libc::setrlimit(libc::RLIMIT_STACK, &limit) == 0
        }
    };
    if set {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
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
        .env("FASTMASH_GROUPING", "sort")
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
            .env("FASTMASH_GROUPING", "sort")
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

/// A job that would use the system `sort` sorts in process where `TMPDIR` is
/// unusable, so it fails only once it has to spill: here with a one-byte
/// chunk target.
#[test]
fn unusable_tmpdir_is_a_clear_error() {
    let cwd = Scratch::new("unusable");
    for tmpdir in ["/nonexistent/fastmash", ""] {
        let output = run_in(
            Path::new(env!("CARGO_BIN_EXE_fastmash")),
            &cwd.0,
            ["-s", "-g", "1", "geomean", "2"],
            &[("TMPDIR", tmpdir), ("FASTMASH_SORT_MEMORY_BYTES", "1")],
            b"b\t2\na\t1\n",
        );
        assert_eq!(output.status.code(), Some(1), "{tmpdir:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!(
                "{}: sort temporary I/O error: No such file or directory (os error 2)\n",
                env!("CARGO_BIN_EXE_fastmash")
            ),
            "{tmpdir:?}"
        );
    }
}

/// Where the temporary directory the system `sort` would use is not a
/// writable, searchable directory, small jobs sort in process and succeed,
/// as GNU datamash's do while `sort` needs no temporary files.
#[test]
fn unusable_tmpdir_sorts_small_input_in_process() {
    use std::os::unix::fs::PermissionsExt;
    let cwd = Scratch::new("small-unusable");
    let read_only = Scratch::new("read-only-tmpdir");
    fs::set_permissions(&read_only.0, fs::Permissions::from_mode(0o555)).unwrap();
    let file = cwd.0.join("file");
    fs::write(&file, b"").unwrap();
    for tmpdir in [
        read_only.0.to_str().unwrap(),
        "/nonexistent/fastmash",
        file.to_str().unwrap(),
        "",
    ] {
        let output = run_in(
            Path::new(env!("CARGO_BIN_EXE_fastmash")),
            &cwd.0,
            ["-s", "-g", "1", "geomean", "2"],
            &[("TMPDIR", tmpdir)],
            b"b\t2\na\t1\nb\t8\n",
        );
        assert!(output.status.success(), "{tmpdir:?}: {output:?}");
        assert_eq!(output.stdout, b"a\t1\nb\t4\n", "{tmpdir:?}");
        assert!(output.stderr.is_empty(), "{tmpdir:?}: {output:?}");
    }
    assert_eq!(fs::read_dir(&read_only.0).unwrap().count(), 0);
}

/// uutils `sort` rejects a field separator that is not valid UTF-8, so jobs
/// with a separator byte from 0x80 sort in process on every host.
#[test]
fn a_separator_byte_from_0x80_sorts_in_process() {
    use std::os::unix::ffi::OsStringExt;
    let cwd = Scratch::new("high-separator");
    for (job, expected) in [
        (&["-s", "-g", "1", "rms", "2"][..], &b"a\xa72\nb\xa75\n"[..]),
        (&["-s", "rmdup", "1"], b"a\xa72\nb\xa71\n"),
    ] {
        let mut args = vec![OsString::from("-t"), OsString::from_vec(vec![0xa7])];
        args.extend(job.iter().map(OsString::from));
        let output = run_in(
            Path::new(env!("CARGO_BIN_EXE_fastmash")),
            &cwd.0,
            &args,
            &[],
            b"b\xa71\na\xa72\nb\xa77\n",
        );
        assert!(output.status.success(), "{job:?}: {output:?}");
        assert_eq!(output.stdout, expected, "{job:?}");
        assert!(output.stderr.is_empty(), "{job:?}: {output:?}");
    }
}

/// Without `fastmash-sort-supervisor` beside it, fastmash sorts the jobs
/// that would use the system `sort` in process, with the same output.
#[test]
fn a_missing_supervisor_sorts_in_process() {
    let dir = Scratch::new("no-supervisor");
    let fastmash = dir.0.join("fastmash");
    fs::copy(env!("CARGO_BIN_EXE_fastmash"), &fastmash).unwrap();
    for (job, expected) in [
        (&["-s", "-g", "1", "rms", "2"][..], "a\t2\nb\t5\n"),
        (&["-s", "rmdup", "1"], "a\t2\nb\t1\n"),
    ] {
        let output = run_in(&fastmash, &dir.0, job, &[], b"b\t1\na\t2\nb\t7\n");
        assert!(output.status.success(), "{job:?}: {output:?}");
        assert_eq!(output.stdout, expected.as_bytes(), "{job:?}");
        assert!(output.stderr.is_empty(), "{job:?}: {output:?}");
    }
}

/// The supervisor marks inherited descriptors close-on-exec with
/// `close_range(CLOSE_RANGE_CLOEXEC)`, which Linux 5.9 and 5.10 reject with
/// `EINVAL` and older kernels lack (`ENOSYS`). Given those answers by a
/// seccomp filter, the jobs that would use the system `sort` sort in process.
#[test]
fn without_close_range_cloexec_jobs_sort_in_process() {
    use std::os::unix::process::CommandExt;
    let cwd = Scratch::new("no-close-range");
    let input = cwd.0.join("input");
    fs::write(&input, b"b\t1\na\t2\nb\t7\n").unwrap();
    for errno in [libc::EINVAL, libc::ENOSYS] {
        for (job, expected) in [
            (&["-s", "-g", "1", "rms", "2"][..], "a\t2\nb\t5\n"),
            (&["-s", "rmdup", "1"], "a\t2\nb\t1\n"),
        ] {
            let mut command = Command::new(env!("CARGO_BIN_EXE_fastmash"));
            command
                .args(job)
                .env_clear()
                .env("LC_ALL", "C")
                .env("FASTMASH_GROUPING", "sort")
                .current_dir(&cwd.0)
                .stdin(fs::File::open(&input).unwrap());
            // SAFETY: the hook runs in the forked child before exec and makes
            // only prctl calls, which are async-signal-safe.
            unsafe { command.pre_exec(move || deny_close_range(errno)) };
            let output = command.output().unwrap();
            assert!(output.status.success(), "{errno} {job:?}: {output:?}");
            assert_eq!(output.stdout, expected.as_bytes(), "{errno} {job:?}");
            assert!(output.stderr.is_empty(), "{errno} {job:?}: {output:?}");
        }
    }
}

/// Makes `close_range` fail with `errno` in this process and every process
/// it starts, through a seccomp filter.
fn deny_close_range(errno: i32) -> std::io::Result<()> {
    const LOAD: u16 = 0x20; // BPF_LD | BPF_W | BPF_ABS
    const EQUAL: u16 = 0x15; // BPF_JMP | BPF_JEQ | BPF_K
    const RETURN: u16 = 0x06; // BPF_RET | BPF_K
    const X86_64: u32 = 0xc000_003e; // AUDIT_ARCH_X86_64
    let step = |code, jt, jf, k| libc::sock_filter { code, jt, jf, k };
    let filter = [
        // seccomp_data: nr at offset 0, arch at offset 4.
        step(LOAD, 0, 0, 4),
        step(EQUAL, 0, 3, X86_64),
        step(LOAD, 0, 0, 0),
        step(EQUAL, 0, 1, libc::SYS_close_range as u32),
        step(RETURN, 0, 0, libc::SECCOMP_RET_ERRNO | errno as u32),
        step(RETURN, 0, 0, libc::SECCOMP_RET_ALLOW),
    ];
    let program = libc::sock_fprog {
        len: filter.len() as u16,
        filter: filter.as_ptr().cast_mut(),
    };
    // prctl reads its variadic arguments as unsigned long.
    let (one, zero): (libc::c_ulong, libc::c_ulong) = (1, 0);
    let mode = libc::c_ulong::from(libc::SECCOMP_MODE_FILTER);
    // SAFETY: prctl reads `program` and the filter it points to, both live
    // for the call; the kernel copies the filter.
    let installed = unsafe {
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, one, zero, zero, zero) == 0
            && libc::prctl(
                libc::PR_SET_SECCOMP,
                mode,
                &program as *const libc::sock_fprog,
            ) == 0
    };
    if installed {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
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
        .env("FASTMASH_GROUPING", "sort")
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

/// The system sort under `fastmash`'s supervisor, once the exec role has
/// become it.
fn running_sort(fastmash: &mut Child) -> u32 {
    let start = Instant::now();
    loop {
        for supervisor in children(fastmash.id()) {
            for sort in children(supervisor) {
                let cmdline = fs::read(format!("/proc/{sort}/cmdline")).unwrap_or_default();
                if cmdline.starts_with(b"/usr/bin/sort\0") {
                    return sort;
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

/// A system sort that a signal ends (the out-of-memory killer's, say) is
/// named before "read error (on close)", as GNU datamash's `/bin/sh` names
/// it where that is dash.
#[test]
fn a_killed_sort_is_named_before_read_error_on_close() {
    use std::io::Read;
    let cwd = Scratch::new("killed-sort");
    for (signal, named) in [(libc::SIGKILL, "Killed"), (libc::SIGTERM, "Terminated")] {
        let mut fastmash = start_external(None, &cwd.0);
        let sort = running_sort(&mut fastmash);
        // SAFETY: kill takes no pointers. The sort waits for input that stays
        // open, and its supervisor reaps it only once it has ended, so its PID
        // is not reused before this.
        assert_eq!(unsafe { libc::kill(sort as i32, signal) }, 0);
        drop(fastmash.stdin.take());
        let status = wait(&mut fastmash, Duration::from_secs(10));
        let mut stdout = Vec::new();
        let mut stderr = String::new();
        let mut child_stdout = fastmash.stdout.take().unwrap();
        child_stdout.read_to_end(&mut stdout).unwrap();
        let mut child_stderr = fastmash.stderr.take().unwrap();
        child_stderr.read_to_string(&mut stderr).unwrap();
        assert_eq!(status.code(), Some(1), "{named}: {stderr}");
        assert!(stdout.is_empty(), "{named}");
        assert_eq!(
            stderr,
            format!(
                "{named}\n{}: read error (on close)\n",
                env!("CARGO_BIN_EXE_fastmash")
            )
        );
    }
}

/// A long Input header in a regular file is read a chunk at a time, not a
/// byte a read, and every byte after it still reaches the sort. The read
/// calls are counted (`syscr` in /proc/PID/io) once the supervisor has
/// started, which follows the header read, while output larger than a pipe
/// holds, unread, keeps fastmash running.
#[test]
fn a_long_header_in_a_file_is_read_in_chunks() {
    use std::io::Read;
    let dir = Scratch::new("long-header");
    let path = dir.0.join("input");
    let length = 200_000;
    let rows = 50_000;
    let mut input = vec![b'h'; length];
    input.extend_from_slice(b"\tv\n");
    for n in 0..rows {
        input.extend_from_slice(format!("k{:05}\t1\n", n * 7919 % rows).as_bytes());
    }
    fs::write(&path, &input).unwrap();
    let mut fastmash = Command::new(env!("CARGO_BIN_EXE_fastmash"))
        // `geomean` in the C locale takes the external sort route.
        .args(["-s", "--header-in", "-g", "1", "geomean", "2"])
        .env_clear()
        .env("LC_ALL", "C")
        .env("FASTMASH_GROUPING", "sort")
        .stdin(fs::File::open(&path).unwrap())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while children(fastmash.id()).is_empty() {
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
    let io = fs::read_to_string(format!("/proc/{}/io", fastmash.id())).unwrap();
    let reads: usize = io
        .lines()
        .find_map(|line| line.strip_prefix("syscr: "))
        .unwrap()
        .parse()
        .unwrap();
    let mut stdout = Vec::new();
    let mut output = fastmash.stdout.take().unwrap();
    output.read_to_end(&mut stdout).unwrap();
    assert!(wait(&mut fastmash, Duration::from_secs(20)).success());
    let expected: String = (0..rows).map(|n| format!("k{n:05}\t1\n")).collect();
    assert!(stdout == expected.as_bytes());
    assert!(reads < length / 100, "{reads} read calls");
}

/// Where the system `sort` cannot run safely (here: `HUP` or `INT` ignored, as
/// under `nohup` or for `command &` in a script), the jobs that would use it
/// sort in process instead, with the same output as GNU datamash.
#[test]
fn ignored_cancellation_signals_fall_back_to_native_sorting() {
    let input = b"b\t2\t9\na\t1\t3\nb\t4\t1\na\t3\t7\n";
    for args in [
        &["-s", "-g", "1", "rms", "2", "pcov", "2:3"][..],
        &["-s", "-g", "1", "geomean", "2"],
        &["-s", "rmdup", "1"],
    ] {
        let run = |trap: &str| {
            let script = format!("{trap}exec \"$0\" \"$@\"");
            let mut child = Command::new("/bin/sh")
                .arg("-c")
                .arg(script)
                .arg(env!("CARGO_BIN_EXE_fastmash"))
                .args(args)
                .env_clear()
                .env("PATH", "/usr/bin:/bin")
                .env("LC_ALL", "C")
                .env("FASTMASH_GROUPING", "sort")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            child.wait_with_output().unwrap()
        };
        let normal = run("");
        assert!(normal.status.success(), "{args:?}: {normal:?}");
        for trap in ["trap '' HUP; ", "trap '' INT; ", "trap '' HUP INT TERM; "] {
            let fallback = run(trap);
            assert_eq!(
                fallback.status.code(),
                Some(0),
                "{args:?} {trap}: {fallback:?}"
            );
            assert_eq!(fallback.stdout, normal.stdout, "{args:?} {trap}");
            assert!(fallback.stderr.is_empty(), "{args:?} {trap}: {fallback:?}");
        }
    }
}

/// Runs `fastmash` in `dir` in the C locale with `args` and `env`, reading
/// `input` from a file: a file, not a pipe, since fastmash may refuse before
/// it reads its input.
fn run_in<I, S>(
    fastmash: &Path,
    dir: &Path,
    args: I,
    env: &[(&str, &str)],
    input: &[u8],
) -> std::process::Output
where
    I: IntoIterator<Item = S> + Clone,
    S: AsRef<std::ffi::OsStr>,
{
    let path = dir.join("input");
    fs::write(&path, input).unwrap();
    // Another test thread that forks while a fresh copy of fastmash is still
    // open for writing makes exec fail with ETXTBSY until that child execs.
    for _ in 0..50 {
        match Command::new(fastmash)
            .args(args.clone())
            .env_clear()
            .env("LC_ALL", "C")
            .env("FASTMASH_GROUPING", "sort")
            .env("PATH", "/usr/bin:/bin")
            .envs(env.iter().copied())
            .current_dir(dir)
            .stdin(fs::File::open(&path).unwrap())
            .output()
        {
            Err(error) if error.raw_os_error() == Some(26) => {
                thread::sleep(Duration::from_millis(20));
            }
            output => return output.unwrap(),
        }
    }
    panic!("{} stayed busy", fastmash.display())
}

/// Runs a copy of fastmash beside a companion script (`$REAL` is the real
/// companion) on a job that needs the system sort.
fn with_companion(name: &str, script: &str) -> (PathBuf, std::process::Output) {
    use std::os::unix::fs::PermissionsExt;
    let dir = Scratch::new(name);
    let fastmash = dir.0.join("fastmash");
    fs::copy(env!("CARGO_BIN_EXE_fastmash"), &fastmash).unwrap();
    let companion = dir.0.join("fastmash-sort-supervisor");
    fs::write(
        &companion,
        format!("#!/bin/sh\nREAL='{SUPERVISOR}'\n{script}\n"),
    )
    .unwrap();
    fs::set_permissions(&companion, fs::Permissions::from_mode(0o755)).unwrap();
    let output = run_in(
        &fastmash,
        &dir.0,
        ["-s", "-g", "1", "geomean", "2"],
        &[],
        b"b\t2\na\t1\n",
    );
    (fastmash, output)
}

#[test]
fn a_companion_from_another_release_is_named() {
    // A companion that does not speak this protocol version says so through
    // the control channel. One that exits without replying may have failed
    // for any reason, so it gets the plain refusal.
    for (name, script, hint) in [
        (
            "other-release",
            "shift\nexec \"$REAL\" --supervise-999 \"$@\"",
            "hint: install fastmash and fastmash-sort-supervisor from the same release\n",
        ),
        ("no-reply", "exit 77", ""),
    ] {
        let (fastmash, output) = with_companion(name, script);
        assert_eq!(output.status.code(), Some(77), "{name}: {output:?}");
        assert!(output.stdout.is_empty(), "{name}");
        assert_eq!(
            String::from_utf8_lossy(&output.stderr),
            format!(
                "{}: unable to start sort supervisor\n{hint}",
                fastmash.display()
            ),
            "{name}"
        );
    }
    let (_, output) = with_companion("same-companion", "exec \"$REAL\" \"$@\"");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"a\t1\nb\t2\n");
}
