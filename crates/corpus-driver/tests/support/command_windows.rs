use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::Duration,
};
#[allow(clippy::zombie_processes)] // Deliberately outlive the leader to test Job cancellation.
pub(super) fn main() {
    let mut args = std::env::args().skip(1);
    let action = args.next().unwrap_or_else(|| "echo".into());
    let trace = if matches!(action.as_str(), "echo" | "file-echo" | "descendant") {
        std::env::args().nth(3)
    } else {
        None
    };
    let record = |phase: &str| {
        if let Some(path) = &trace {
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .unwrap();
            writeln!(
                file,
                "{} {phase}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            )
            .unwrap();
        }
    };
    record("main");
    match action.as_str() {
        action @ ("echo" | "file-echo") => {
            if action == "file-echo" {
                assert!(
                    winapi_util::file::typ(std::io::stdout()).unwrap().is_disk(),
                    "expected native disk output"
                );
            }
            let mut bytes = Vec::new();
            record("read-start");
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            record("read-eof");
            std::io::stdout().write_all(&bytes).unwrap();
        }
        "console-held" => {
            use process_wrap::std::{CommandWrap, JobObject};
            let binary = args.next().unwrap();
            let ready = args.next().unwrap();
            let release = args.next().unwrap();
            let mut command = Command::new(binary);
            command
                .args(["--csv-out", "-H", "sum", "1"])
                .stdin(Stdio::piped());
            let mut wrapped = CommandWrap::from(command);
            wrapped.wrap(JobObject);
            let mut limits = win32job::ExtendedLimitInfo::new();
            limits.limit_kill_on_job_close();
            let proof = win32job::Job::create_with_limit_info(&limits).unwrap();
            let mut child = wrapped
                .spawn_with(|command| {
                    use std::os::windows::io::AsRawHandle;
                    let mut child = command.spawn()?;
                    if let Err(error) = proof.assign_process(child.as_raw_handle() as isize) {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(std::io::Error::other(error));
                    }
                    Ok(child)
                })
                .unwrap();
            drop(wrapped);
            let mut input = child.stdin().take().unwrap();
            input.write_all(b"value\n1\n2\n").unwrap();
            std::fs::write(ready, b"input delivered").unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while !std::path::Path::new(&release).exists() {
                if std::time::Instant::now() >= deadline {
                    child.start_kill().unwrap();
                    child.wait().unwrap();
                    panic!("console observation was not released");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(input);
            loop {
                if let Some(status) = child.try_wait().unwrap() {
                    assert!(status.success(), "held console child failed");
                    break;
                }
                if std::time::Instant::now() >= deadline {
                    child.start_kill().unwrap();
                    child.wait().unwrap();
                    panic!("held console child did not complete");
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        "wrong-output" => println!("incorrect"),
        "wrong-error" => eprintln!("incorrect"),
        "wrong-status" => std::process::exit(17),
        "overflow" => {
            let bytes = [b'x'; 65536];
            loop {
                if std::io::stdout().write_all(&bytes).is_err() {
                    break;
                }
            }
        }
        "stall" => std::thread::sleep(Duration::from_secs(30)),
        "descendant" => {
            record("descendant-spawn-start");
            Command::new(std::env::current_exe().unwrap())
                .arg("mark-later")
                .arg(args.next().unwrap())
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            record("descendant-spawn-complete");
        }
        "mark-later" => {
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(args.next().unwrap(), b"escaped").unwrap();
        }
        other => panic!("unknown fixture action {other}"),
    }
}
