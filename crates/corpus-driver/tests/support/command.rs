#![forbid(unsafe_code)]

// Only the unpublished corpus driver's tests run this executable.
use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::ffi::OsStrExt;
use std::process::{Command, Stdio};
use std::time::Duration;

fn main() {
    let mut args = std::env::args_os();
    let name = args.next().unwrap();
    let mode = args.next().unwrap();
    match mode.to_str().unwrap() {
        "ignored-file-size-signal" => {
            use nix::sys::{
                resource::{Resource, getrlimit},
                signal::{SigSet, Signal, raise},
            };
            let blocked = SigSet::thread_get_mask().unwrap().contains(Signal::SIGXFSZ);
            assert!(!blocked, "file-size signal must be unblocked");
            raise(Signal::SIGXFSZ).unwrap();
            println!(
                "{}",
                serde_json::json!({
                    "pid":std::process::id(),
                    "file_size":getrlimit(Resource::RLIMIT_FSIZE).unwrap(),
                    "file_size_signal_blocked":blocked,
                    "private_environment_present":std::env::vars_os().any(|(k,_)|k.as_bytes().starts_with(b"FASTMASH_TEST_PROCESS_")),
                })
            );
        }
        "resource-facts" => {
            use nix::sys::resource::{Resource, getrlimit};
            let facts = serde_json::json!({
                "file_size":getrlimit(Resource::RLIMIT_FSIZE).unwrap(),
                "private_environment_present":std::env::vars_os().any(|(k,_)|k.as_bytes().starts_with(b"FASTMASH_TEST_PROCESS_")),
            });
            #[cfg(target_os = "linux")]
            let facts = {
                let mut facts = facts;
                facts["address_space"] = serde_json::json!(getrlimit(Resource::RLIMIT_AS).unwrap());
                facts
            };
            println!("{facts}");
        }
        "alarm-remaining" => println!("{}", nix::unistd::alarm::cancel().unwrap_or(0)),
        "process-facts" => {
            use nix::sys::signal::{SigSet, Signal};
            let mask = SigSet::thread_get_mask().unwrap();
            println!(
                "{}",
                serde_json::json!({
                    "pid":std::process::id(),
                    "argv0":name.as_bytes(),
                    "arguments":args.map(|s| s.as_bytes().to_vec()).collect::<Vec<_>>(),
                    "sigterm_blocked":mask.contains(Signal::SIGTERM),
                    "selected_environment":std::env::var_os("SELECTED_ENVIRONMENT").map(|s|s.as_bytes().to_vec()),
                    "private_environment_present":std::env::vars_os().any(|(k,_)|k.as_bytes().starts_with(b"FASTMASH_TEST_PROCESS_")),
                })
            );
        }
        "echo" => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            std::io::stdout().write_all(&bytes).unwrap();
        }
        "wrong-output" => println!("incorrect"),
        "wrong-error" => eprintln!("incorrect"),
        "wrong-status" => std::process::exit(17),
        // Deliberately reject the ordinary full-output calibration control.
        "count" => std::process::exit(17),
        "version" => println!("fastmash 0.2.0"),
        "early-close" => println!("early"),
        "late-input" | "late-input-stall" | "buffered-input" | "buffered-input-stall" => {
            let mut input = File::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
            let mut bytes = [0; 6];
            input.read_exact(&mut bytes).unwrap();
            assert_eq!(&bytes, b"input\n");
            if !mode.to_str().unwrap().starts_with("buffered-") {
                println!("before");
            }
            let failed = loop {
                match input.read(&mut [0]) {
                    Ok(0) => break false,
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(error) if error.raw_os_error() == Some(nix::errno::Errno::EIO as i32) => {
                        break true;
                    }
                    result => panic!("unexpected late-input read: {result:?}"),
                }
            };
            if failed
                && matches!(
                    mode.to_str().unwrap(),
                    "late-input-stall" | "buffered-input-stall"
                )
            {
                #[allow(clippy::zombie_processes)]
                let child = Command::new(std::env::current_exe().unwrap())
                    .arg("sleep")
                    .spawn()
                    .unwrap();
                let pidfile = std::path::PathBuf::from(args.next().unwrap());
                fs::write(&pidfile, child.id().to_string()).unwrap();
                fs::write(pidfile.with_extension("ready"), b"ready\n").unwrap();
                std::thread::sleep(Duration::from_secs(60));
            }
            println!("after");
            if failed {
                eprintln!("actual late EIO confirmed");
                std::process::exit(1);
            }
        }
        "held-output" | "held-buffered" => {
            if mode == "held-output" {
                std::io::stdout().write_all(b"before\n").unwrap();
                std::io::stdout().flush().unwrap();
            }
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            assert_eq!(bytes, b"input\n");
            if mode == "held-buffered" {
                std::io::stdout().write_all(b"before\n").unwrap();
            }
            std::io::stdout().write_all(b"after\n").unwrap();
        }
        "held-input" => {
            use nix::fcntl::{FcntlArg, OFlag, fcntl};
            use std::os::unix::fs::FileTypeExt;
            let mut input = File::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
            assert!(input.metadata().unwrap().file_type().is_fifo());
            let flags = OFlag::from_bits_truncate(fcntl(&input, FcntlArg::F_GETFL).unwrap());
            fcntl(&input, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).unwrap();
            // An empty open pipe would block. A prematurely closed writer
            // instead returns EOF, which must fail this control.
            match input.read(&mut [0]) {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    println!("stdin remains open");
                }
                _ => std::process::exit(17),
            }
        }
        "directory-input" => {
            let mut input = File::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
            if !input.metadata().unwrap().is_dir() {
                std::process::exit(17);
            }
            let error = input.read(&mut [0]).unwrap_err();
            assert_eq!(error.raw_os_error(), Some(nix::errno::Errno::EISDIR as i32));
            println!("directory read confirmed");
        }
        "broken-output" => {
            let Err(error) = std::io::stdout().write_all(b"selected output\n") else {
                std::process::exit(17);
            };
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
            eprintln!("broken output confirmed");
        }
        "overflow" => {
            let bytes = vec![b'x'; 524_289];
            let _ = std::io::stdout().write_all(&bytes);
        }
        "overflow-error" => {
            let bytes = vec![b'x'; 524_289];
            let _ = std::io::stderr().write_all(&bytes);
        }
        "sleep" => std::thread::sleep(Duration::from_secs(60)),
        "descendant" | "orphan" | "orphan-output" => {
            // The orphan branch must leave a real descendant after the leader
            // exits so the driver, rather than this fixture, proves cleanup.
            #[allow(clippy::zombie_processes)]
            let mut child = Command::new(std::env::current_exe().unwrap())
                .arg("sleep")
                .stdout(if mode == "orphan-output" {
                    Stdio::inherit()
                } else {
                    Stdio::null()
                })
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            fs::write(args.next().unwrap(), child.id().to_string()).unwrap();
            if mode == "descendant" {
                let _ = child.wait();
            }
        }
        "facts" => {
            assert_eq!(name.as_bytes(), b"fastmash");
            assert_eq!(std::env::vars_os().count(), 4);
            assert_eq!(std::env::var("PATH").unwrap(), "/usr/bin:/bin");
            assert_eq!(std::env::var("LC_ALL").unwrap(), "C");
            assert_eq!(std::env::var("TZ").unwrap(), "UTC");
            assert_eq!(std::env::var("POSIXLY_CORRECT").unwrap(), "");
            let input = File::from(std::io::stdin().as_fd().try_clone_to_owned().unwrap());
            let expected_file = args.next().unwrap() == "file";
            assert_eq!(input.metadata().unwrap().is_file(), expected_file);
            let raw = args.next().unwrap();
            assert_eq!(raw.as_bytes(), b"raw\xff");
            assert!(
                std::env::current_dir()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .as_bytes()
                    .starts_with(b"fastmash-corpus-")
            );
        }
        "sort-error" => eprintln!("/usr/bin/sort: sample"),
        "mutate-file" => fs::write(args.next().unwrap(), b"changed").unwrap(),
        "header-error" => {
            eprintln!("fastmash: missing input header for named grouping key");
            std::process::exit(1);
        }
        _ => panic!("unknown test command"),
    }
}
