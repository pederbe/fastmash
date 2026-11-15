//! Safe Job ownership and bounded pipe pumps for Windows command checks.
use crate::fixture::{Case, Held, decode, encode};
use crate::held_output::Destination;
pub use crate::observation::Observed;
use crate::{Error, Result};
use process_wrap::std::{ChildWrapper, CommandWrap, JobObject};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use win32job::{ExtendedLimitInfo, Job};

pub const CAP: u64 = 524_288;
const DEADLINE: Duration = Duration::from_secs(8);

pub fn require_unblocked_signals() -> Result<()> {
    Ok(())
}

pub(crate) fn command_output(
    mut command: Command,
    timeout: Duration,
) -> Result<std::process::Output> {
    let (out, stdout) = io::pipe()?;
    let (err, stderr) = io::pipe()?;
    let out = Capture::new(out);
    let err = Capture::new(err);
    command.stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    let started = Instant::now();
    let mut running = Running::spawn(command)?;
    let status = loop {
        if let Some(status) = running.status()? {
            break status;
        }
        if started.elapsed() >= timeout || out.overflow()? || err.overflow()? {
            running.stop()?;
            return Err(Error::message(
                "Windows workflow exceeded its time or output guard",
            ));
        }
        thread::sleep(Duration::from_millis(2));
    };
    running.stop()?;
    let stdout = out.finish()?;
    let stderr = err.finish()?;
    if stdout.len() as u64 > CAP || stderr.len() as u64 > CAP {
        return Err(Error::message("Windows workflow output exceeded its guard"));
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// The second Job supplies an independent membership proof. Assignment occurs
/// while process-wrap holds the child suspended, before target code executes.
struct Running {
    child: Box<dyn ChildWrapper>,
    proof: Job,
    stopped: bool,
}
impl Running {
    fn spawn(command: Command) -> io::Result<Self> {
        let mut limits = ExtendedLimitInfo::new();
        limits.limit_kill_on_job_close();
        let proof = Job::create_with_limit_info(&limits).map_err(io::Error::other)?;
        let mut wrapped = CommandWrap::from(command);
        wrapped.wrap(JobObject);
        let child = wrapped.spawn_with(|command| {
            let mut child = command.spawn()?;
            if let Err(error) = proof.assign_process(child.as_raw_handle() as isize) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::other(error));
            }
            Ok(child)
        })?;
        // Command retains its copies of explicit pipe handles after spawn.
        drop(wrapped);
        Ok(Self {
            child,
            proof,
            stopped: false,
        })
    }
    fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.inner_mut().try_wait()
    }
    fn stop(&mut self) -> io::Result<()> {
        self.child.start_kill()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if self
                .proof
                .query_process_id_list()
                .map_err(io::Error::other)?
                .is_empty()
            {
                self.child.inner_mut().wait()?;
                self.stopped = true;
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::other("Windows Job cleanup deadline"));
            }
            thread::sleep(Duration::from_millis(2));
        }
    }
}
impl Drop for Running {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = self.stop();
        }
    }
}

struct Capture {
    bytes: Arc<Mutex<Vec<u8>>>,
    result: mpsc::Receiver<io::Result<()>>,
}
impl Capture {
    fn new(mut reader: impl Read + Send + 'static) -> Self {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let copy = bytes.clone();
        let (sent, result) = mpsc::channel();
        thread::spawn(move || {
            let result = (|| {
                loop {
                    let length = copy
                        .lock()
                        .map_err(|_| io::Error::other("capture lock poisoned"))?
                        .len();
                    if length > CAP as usize {
                        return Ok(());
                    }
                    let mut buffer = [0; 65536];
                    let capacity = (CAP as usize + 1 - length).min(buffer.len());
                    match reader.read(&mut buffer[..capacity]) {
                        Ok(0) => return Ok(()),
                        Ok(length) => copy
                            .lock()
                            .map_err(|_| io::Error::other("capture lock poisoned"))?
                            .extend_from_slice(&buffer[..length]),
                        Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                        Err(error) => return Err(error),
                    }
                }
            })();
            let _ = sent.send(result);
        });
        Self { bytes, result }
    }
    fn snapshot(&self) -> Result<Vec<u8>> {
        self.bytes
            .lock()
            .map(|bytes| bytes.clone())
            .map_err(|_| Error::message("capture lock poisoned"))
    }
    fn overflow(&self) -> Result<bool> {
        Ok(self
            .bytes
            .lock()
            .map_err(|_| Error::message("capture lock poisoned"))?
            .len()
            > CAP as usize)
    }
    fn finish(self) -> Result<Vec<u8>> {
        self.result
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| Error::message("capture did not finish after Job cleanup"))??;
        self.snapshot()
    }
}

enum OutputCapture {
    Pipe(Capture),
    File(PathBuf),
}
impl OutputCapture {
    fn snapshot(&self) -> Result<Vec<u8>> {
        match self {
            Self::Pipe(capture) => capture.snapshot(),
            Self::File(path) => {
                let mut bytes = Vec::new();
                File::open(path)?.take(CAP + 1).read_to_end(&mut bytes)?;
                Ok(bytes)
            }
        }
    }
    fn overflow(&self) -> Result<bool> {
        match self {
            Self::Pipe(capture) => capture.overflow(),
            Self::File(path) => Ok(fs::metadata(path)?.len() > CAP),
        }
    }
    fn finish(self) -> Result<Vec<u8>> {
        match self {
            Self::Pipe(capture) => capture.finish(),
            Self::File(ref path) => {
                let result = self.snapshot();
                fs::remove_file(path)?;
                result
            }
        }
    }
}

pub(crate) fn invoke(
    case: &Case,
    binary: &Path,
    _launcher: &Path,
    directory: &Path,
    stdin_file: bool,
    mut full: Option<&mut crate::full_output::FullOutput>,
) -> Result<Observed> {
    let args = case.arguments()?;
    let data = decode(&case.input_hex)?;
    let (out_reader, out_writer) = io::pipe()?;
    let (err_reader, err_writer) = io::pipe()?;
    let regular_output = case.io == "normal" && case.observes_held();
    let (captured, output) = if regular_output {
        drop(out_reader);
        drop(out_writer);
        let path = directory.join("stdout");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        (Some(OutputCapture::File(path)), Stdio::from(file))
    } else if case.io == "full" {
        drop(out_reader);
        drop(out_writer);
        (
            None,
            Stdio::from(
                full.as_mut()
                    .ok_or_else(|| Error::message("missing full output"))?
                    .destination()?,
            ),
        )
    } else if case.io == "closed-pipe" {
        drop(out_reader);
        (None, Stdio::from(out_writer))
    } else {
        (
            Some(OutputCapture::Pipe(Capture::new(out_reader))),
            Stdio::from(out_writer),
        )
    };
    let error_capture = Capture::new(err_reader);
    let mut pump = None;
    let mut release: Option<mpsc::Sender<()>> = None;
    let input = if case.io == "directory" {
        // BACKUP_SEMANTICS admits a genuine directory handle; ReadFile rejects it.
        Stdio::from(
            OpenOptions::new()
                .read(true)
                .custom_flags(0x0200_0000)
                .open(directory)?,
        )
    } else if stdin_file {
        let path = directory.join("stdin");
        fs::write(&path, &data)?;
        Stdio::from(File::open(path)?)
    } else {
        let (reader, mut writer) = io::pipe()?;
        let (sent, complete) = mpsc::channel();
        let (finish, released) = mpsc::channel();
        let hold = case.hold_stdin || case.observes_held();
        thread::spawn(move || {
            let result = writer.write_all(&data);
            let _ = sent.send(result);
            if hold {
                let _ = released.recv();
            }
            drop(writer);
        });
        pump = Some(complete);
        release = Some(finish);
        Stdio::from(reader)
    };
    let mut env = BTreeMap::from([
        ("LC_ALL".to_owned(), case.locale.clone()),
        ("TZ".to_owned(), "UTC".to_owned()),
        ("NO_COLOR".to_owned(), "1".to_owned()),
    ]);
    // Preserve the Windows loader environment while retaining deterministic locale.
    for key in ["SystemRoot", "WINDIR", "PATH", "TEMP", "TMP"] {
        if let Ok(value) = std::env::var(key) {
            env.insert(key.to_owned(), value);
        }
    }
    if let Some(value) = &case.posixly_correct {
        env.insert("POSIXLY_CORRECT".to_owned(), value.clone());
    }
    let mut command = Command::new(binary);
    command
        .args(&args)
        .current_dir(directory)
        .env_clear()
        .envs(&env)
        .stdin(input)
        .stdout(output)
        .stderr(err_writer);
    let started = Instant::now();
    let mut running = Running::spawn(command)?;
    let process_group = running.child.id();
    let mut held = None;
    let mut observation_end = None;
    let mut written = pump.is_none();
    let mut timed_out = false;
    let mut overflow;
    let status = loop {
        if !written && let Some(pump) = &pump {
            match pump.try_recv() {
                Ok(Ok(())) => written = true,
                Ok(Err(error)) if error.kind() == io::ErrorKind::BrokenPipe => written = true,
                Ok(Err(error)) => return Err(error.into()),
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err(Error::message("input pump lost its outcome"));
                }
            }
        }
        if written && case.observes_held() {
            observation_end.get_or_insert_with(|| Instant::now() + Duration::from_secs(2));
        }
        let status = running.status()?;
        overflow = error_capture.overflow()?
            || captured
                .as_ref()
                .map(OutputCapture::overflow)
                .transpose()?
                .unwrap_or(false);
        if held.is_none() && observation_end.is_some_and(|end| Instant::now() >= end) {
            let bytes = captured
                .as_ref()
                .map(OutputCapture::snapshot)
                .transpose()?
                .unwrap_or_default();
            held = Some(Held {
                bytes: bytes.len(),
                alive: status.is_none(),
                stdout_hex: (case.io == "pipe").then(|| encode(&bytes)),
            });
            release.take();
        }
        if status.is_some() {
            break status;
        }
        if started.elapsed() >= DEADLINE {
            timed_out = true;
            break None;
        }
        if overflow {
            break None;
        }
        thread::sleep(Duration::from_millis(2));
    };
    let stdin_held_until_exit = case.hold_stdin && status.is_some() && release.is_some();
    running.stop()?;
    release.take();
    if !written && let Some(pump) = pump {
        let _ = pump
            .recv_timeout(Duration::from_secs(2))
            .map_err(|_| Error::message("input pump did not stop after Job cleanup"))?;
    }
    let stdout = captured
        .map(OutputCapture::finish)
        .transpose()?
        .unwrap_or_default();
    let stderr = error_capture.finish()?;
    overflow |= stdout.len() as u64 > CAP || stderr.len() as u64 > CAP;
    if let Some(full) = full.as_mut() {
        full.verify()?;
    }
    let mut argv = vec![binary.as_os_str().to_os_string()];
    argv.extend(args);
    Ok(Observed {
        argv: argv
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect(),
        argv_hex: argv
            .iter()
            .map(|arg| encode(arg.as_encoded_bytes()))
            .collect(),
        executable: binary.to_owned(),
        env,
        process_group,
        returncode: status.and_then(|status| status.code()).unwrap_or(1),
        timed_out,
        overflow,
        cleanup_confirmed: true,
        stdin_held_until_exit,
        held,
        destination: case.observes_held().then_some(Destination {
            isatty: false,
            block_size: 8192,
            kind: if regular_output {
                "regular-file"
            } else {
                "pipe"
            },
        }),
        acknowledged: None,
        input_error_mechanism: None,
        stdout_hex: encode(&stdout),
        stderr_hex: encode(&stderr),
    })
}

pub(crate) struct Scratch {
    pub path: PathBuf,
}
impl Scratch {
    pub fn new() -> Result<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = std::env::temp_dir().join(format!(
                "fastmash-corpus-{}-{}",
                std::process::id(),
                SERIAL.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    pub fn remove(self) -> Result<()> {
        fs::remove_dir_all(&self.path)?;
        Ok(())
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
