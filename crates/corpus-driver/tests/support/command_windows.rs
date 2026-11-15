use std::{
    io::{Read, Write},
    process::{Command, Stdio},
    time::Duration,
};
#[allow(clippy::zombie_processes)] // Deliberately outlive the leader to test Job cancellation.
pub(super) fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref().unwrap_or("echo") {
        "echo" => {
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).unwrap();
            std::io::stdout().write_all(&bytes).unwrap();
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
            Command::new(std::env::current_exe().unwrap())
                .arg("mark-later")
                .arg(args.next().unwrap())
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
        }
        "mark-later" => {
            std::thread::sleep(Duration::from_secs(2));
            std::fs::write(args.next().unwrap(), b"escaped").unwrap();
        }
        other => panic!("unknown fixture action {other}"),
    }
}
