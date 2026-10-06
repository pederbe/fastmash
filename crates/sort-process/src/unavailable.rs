//! Explicitly unavailable external sort on native macOS.
use super::{Message, StartError};
use std::io::{self, BufReader};
use std::process::ChildStdout;

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "external sorting requires Linux",
    )
}
pub fn available(_: Option<u8>) -> bool {
    false
}
pub fn admit() -> io::Result<()> {
    Err(unsupported())
}
/// This type preserves the caller interface but cannot be constructed
/// successfully on macOS. Commands choose the built-in sorter instead.
pub struct Session {
    _private: (),
}
impl Session {
    pub fn start(_: &[u64], _: Option<u8>, _: bool, _: bool) -> Result<Self, StartError> {
        Err(StartError::Other(unsupported()))
    }
    pub fn reader(&mut self) -> &mut BufReader<ChildStdout> {
        unreachable!("external sorting is unavailable on macOS")
    }
    pub fn finish(&mut self, _: bool) -> io::Result<Message> {
        Err(unsupported())
    }
}
