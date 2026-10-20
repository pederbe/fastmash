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
