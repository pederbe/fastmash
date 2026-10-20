#![forbid(unsafe_code)]

//! Exercise real ENOSPC against the actual executable, without an interposer.
use fastmash_corpus_driver::fixture::digest;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use serde_json::json;
use std::error::Error;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::thread;
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const LIMIT: Duration = Duration::from_secs(10);
const MAX_VOLUME: u64 = 16 * 1024 * 1024;
const INPUT: &[u8] = b"alpha\t2\nomega\t9\n";
const DIAGNOSTIC: &[u8] = b"fastmash: write error: No space left on device\n";

fn fresh(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

struct Running(Child, bool);
impl Running {
    fn wait(&mut self, deadline: Instant) -> Result<ExitStatus> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait()? {
                self.1 = true;
                return Ok(status);
            }
            if started.elapsed() >= LIMIT || Instant::now() >= deadline {
                return Err("candidate execution deadline".into());
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if self.1 {
            return;
        }
        let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
        let _ = self.0.kill();
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(1) {
            if matches!(self.0.try_wait(), Ok(Some(_))) {
                self.1 = true;
                return;
            }
            thread::sleep(Duration::from_millis(2));
        }
        eprintln!("native-full-output: child {} did not reap", self.0.id());
    }
}

enum Destination {
    Stdout(File),
    Stderr(File),
}

fn invoke(
    binary: &Path,
    input: &Path,
    root: &Path,
    name: &str,
    args: &[&str],
    destination: Destination,
    deadline: Instant,
) -> Result<(ExitStatus, Vec<u8>)> {
    let directory = root.join(name);
    fs::create_dir(&directory)?;
    let mut command = Command::new(binary);
    command
        .arg0("fastmash")
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .stdin(File::open(input)?)
        .process_group(0);
    let (destination_fd, captured_name) = match destination {
        Destination::Stdout(file) => {
            command
                .stdout(file)
                .stderr(fresh(&directory.join("stderr"))?);
            (1, "stderr")
        }
        Destination::Stderr(file) => {
            command
                .stderr(file)
                .stdout(fresh(&directory.join("stdout"))?);
            (2, "stdout")
        }
    };
    require_time(deadline)?;
    let mut running = Running(command.spawn()?, false);
    drop(command);
    let status = running.wait(deadline)?;
    let captured = fs::read(directory.join(captured_name))?;
    fs::write(
        directory.join("observed.json"),
        serde_json::to_vec_pretty(&json!({
            "pid": running.0.id(), "reaped": running.1, "exit_code": status.code(),
            "argv":args, "destination_fd":destination_fd,
            "captured_fd":3 - destination_fd, "captured_sha256":digest(&captured),
            "stderr_sha256":(destination_fd == 1).then(|| digest(&captured)),
        }))?,
    )?;
    Ok((status, captured))
}

fn require_full(file: &mut File) -> Result<i32> {
    match file.write(b"x") {
        Err(error) if error.raw_os_error() == Some(nix::errno::Errno::ENOSPC as i32) => {
            Ok(nix::errno::Errno::ENOSPC as i32)
        }
        Err(error) => Err(format!("full sink reports a different error: {error}").into()),
        Ok(count) => Err(format!("full sink accepted {count} bytes").into()),
    }
}

fn require_empty_full(file: &mut File) -> Result<()> {
    require_full(file)?;
    let metadata = file.metadata()?;
    if metadata.is_file() && metadata.len() != 0 {
        return Err("full sink unexpectedly contains output bytes".into());
    }
    Ok(())
}

fn fill(volume: &Path, root: &Path, deadline: Instant) -> Result<File> {
    let stats = nix::sys::statvfs::statvfs(volume)?;
    // Native block-count widths differ between Linux and Darwin. Widen
    // before multiplication and refuse a capacity that cannot fit in u64.
    let capacity = u64::try_from(u128::from(stats.blocks()) * u128::from(stats.fragment_size()))?;
    if !(1024 * 1024..=MAX_VOLUME).contains(&capacity) {
        return Err(format!(
            "require a private mounted volume of 1 to 16 MiB, observed {capacity} bytes"
        )
        .into());
    }
    let directory = volume.join(format!("fastmash-full-output-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&directory)?;
    let mut sink = fresh(&directory.join("sink"))?;
    // Demonstrate this very file accepts ordinary bytes before filling its volume.
    sink.write_all(b"ordinary writable sink\n")?;
    sink.sync_all()?;
    sink.set_len(0)?;
    let mut filler = fresh(&directory.join("filler"))?;
    let mut total = 0_u64;
    let block = [0_u8; 65536];
    for bytes in [block.as_slice(), b"x".as_slice()] {
        loop {
            require_time(deadline)?;
            if total >= MAX_VOLUME {
                return Err("fill exceeded private-volume bound".into());
            }
            match filler.write(bytes) {
                Ok(0) => return Err("filler made no progress".into()),
                Ok(count) => total += count as u64,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => (),
                Err(error) if error.raw_os_error() == Some(nix::errno::Errno::ENOSPC as i32) => {
                    break;
                }
                Err(error) => return Err(error.into()),
            }
        }
    }
    let error = require_full(&mut sink)?;
    fs::write(
        root.join("volume.json"),
        serde_json::to_vec_pretty(&json!({
            "mount":volume,"capacity_bytes":capacity,"filler_bytes":total,
            "sink":directory.join("sink"),"calibration_errno":error,
            "ordinary_sink_write":true,
        }))?,
    )?;
    Ok(sink)
}

fn require_time(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err("full-output probe deadline".into());
    }
    Ok(())
}

fn run() -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut args = std::env::args_os().skip(1);
    let binary = PathBuf::from(args.next().ok_or("require executable")?).canonicalize()?;
    let root = PathBuf::from(args.next().ok_or("require new capture directory")?);
    let volume = args.next().map(PathBuf::from);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    fs::DirBuilder::new().mode(0o700).create(&root)?;
    let input = root.join("input");
    fs::write(&input, INPUT)?;
    type Case = (&'static str, &'static [&'static str], Option<&'static [u8]>);
    let cases: &[Case] = &[
        ("count", &["count", "1"], Some(b"2\n")),
        ("noop", &["--full", "noop"], Some(INPUT)),
        ("help", &["--help"], None),
    ];
    for (name, arguments, expected) in cases {
        let output = root.join(format!("ordinary-{name}.stdout"));
        let (status, stderr) = invoke(
            &binary,
            &input,
            &root,
            &format!("ordinary-{name}"),
            arguments,
            Destination::Stdout(fresh(&output)?),
            deadline,
        )?;
        let stdout = fs::read(output)?;
        if !status.success()
            || !stderr.is_empty()
            || stdout.is_empty()
            || expected.is_some_and(|bytes| bytes != stdout)
        {
            return Err(format!("ordinary candidate control differs: {name}").into());
        }
    }
    let mut full = if let Some(volume) = volume {
        fill(&volume.canonicalize()?, &root, deadline)?
    } else {
        #[cfg(target_os = "linux")]
        {
            File::options().write(true).open("/dev/full")?
        }
        #[cfg(not(target_os = "linux"))]
        {
            return Err("require private mounted volume on this platform".into());
        }
    };
    require_empty_full(&mut full)?;
    for (name, arguments, _) in cases {
        let (status, stderr) = invoke(
            &binary,
            &input,
            &root,
            &format!("full-{name}"),
            arguments,
            Destination::Stdout(full.try_clone()?),
            deadline,
        )?;
        if status.code() != Some(1) || stderr != DIAGNOSTIC {
            return Err(format!(
                "actual full-output result differs: {name}, {status}, {}",
                String::from_utf8_lossy(&stderr)
            )
            .into());
        }
        require_empty_full(&mut full)?;
    }
    for mode in ["never", "always"] {
        let arguments = [format!("--color={mode}"), "--not-an-option".to_owned()];
        let arguments = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let baseline = root.join(format!("ordinary-error-{mode}.stdout"));
        let (status, expected) = invoke(
            &binary,
            &input,
            &root,
            &format!("ordinary-error-{mode}"),
            &arguments,
            Destination::Stdout(fresh(&baseline)?),
            deadline,
        )?;
        if status.code() != Some(1) || expected.is_empty() || fs::metadata(baseline)?.len() != 0 {
            return Err(format!("ordinary diagnostic control differs: {mode}").into());
        }
        require_empty_full(&mut full)?;
        let (status, stderr) = invoke(
            &binary,
            &input,
            &root,
            &format!("unselected-stderr-{mode}"),
            &arguments,
            Destination::Stdout(full.try_clone()?),
            deadline,
        )?;
        if status.code() != Some(1) || stderr != expected {
            return Err(format!("unselected diagnostic differs: {mode}").into());
        }
        require_empty_full(&mut full)?;
        let (status, stdout) = invoke(
            &binary,
            &input,
            &root,
            &format!("full-stderr-{mode}"),
            &arguments,
            Destination::Stderr(full.try_clone()?),
            deadline,
        )?;
        if status.code() != Some(1) || !stdout.is_empty() {
            return Err(format!("actual full-stderr result differs: {mode}").into());
        }
        require_empty_full(&mut full)?;
    }
    let baseline = root.join("ordinary-version.stdout");
    let (status, stderr) = invoke(
        &binary,
        &input,
        &root,
        "ordinary-version",
        &["--version"],
        Destination::Stdout(fresh(&baseline)?),
        deadline,
    )?;
    let expected = fs::read(baseline)?;
    if !status.success() || !stderr.is_empty() || !expected.starts_with(b"fastmash ") {
        return Err("ordinary version control differs".into());
    }
    require_empty_full(&mut full)?;
    let (status, stdout) = invoke(
        &binary,
        &input,
        &root,
        "unselected-stdout",
        &["--version"],
        Destination::Stderr(full.try_clone()?),
        deadline,
    )?;
    if !status.success() || stdout != expected {
        return Err("unselected stdout differs".into());
    }
    require_empty_full(&mut full)?;
    fs::write(
        root.join("matched.json"),
        serde_json::to_vec_pretty(&json!({
            "executable":binary,"executable_sha256":digest(&fs::read(&binary)?),
            "cases":cases.iter().map(|case|case.0).collect::<Vec<_>>(),
            "stream_controls":["unselected-stderr-never","full-stderr-never",
                "unselected-stderr-always","full-stderr-always","unselected-stdout"],
            "actual_enospc":true,"matched":true,
        }))?,
    )?;
    println!("ordinary and actual full-output cases matched");
    Ok(())
}
fn main() {
    if let Err(error) = run() {
        eprintln!("native-full-output: {error}");
        std::process::exit(1);
    }
}
