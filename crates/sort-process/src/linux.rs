//! Checked Linux entropy for native storage.
use rustix::{
    io::Errno,
    rand::{GetRandomFlags, getrandom},
};
use std::io;
pub fn random(bytes: &mut [u8]) -> io::Result<()> {
    let mut at = 0;
    while at < bytes.len() {
        match getrandom(&mut bytes[at..], GetRandomFlags::empty()) {
            Ok(0) => return Err(io::Error::other("empty random read")),
            Ok(n) => at += n,
            Err(Errno::INTR) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
