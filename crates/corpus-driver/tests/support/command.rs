#![forbid(unsafe_code)]
#[cfg(unix)]
#[path = "command_unix.rs"]
mod native;
#[cfg(windows)]
#[path = "command_windows.rs"]
mod native;
fn main() {
    native::main();
}
