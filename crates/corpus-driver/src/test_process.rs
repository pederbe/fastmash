#![forbid(unsafe_code)]
#[cfg(unix)]
#[path = "test_process_unix.rs"]
mod native;

#[cfg(unix)]
fn main() {
    native::main();
}
#[cfg(windows)]
fn main() {
    eprintln!("fastmash-test-process: this fixture requires Unix process setup");
    std::process::exit(2);
}
