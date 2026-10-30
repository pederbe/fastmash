//! Linux SIGPIPE restoration and checked entropy for native storage.
use rustix::{
    io::Errno,
    rand::{GetRandomFlags, getrandom},
};
use std::io;
/// Execute a raw Linux x86-64 syscall.
///
/// # Safety
/// The caller must satisfy the selected syscall's ABI and requirements, including
/// pointer validity, buffer bounds, aliasing and access permissions for as long as
/// the kernel uses them. Its effects on memory, file descriptor ownership and
/// process state must preserve the invariants of any live Rust values.
pub unsafe fn syscall(n: usize, args: [usize; 6]) -> isize {
    let result: isize;
    // SAFETY: the syscall instruction clobbers only rax, rcx and r11, as
    // declared; the caller upholds the syscall's own contract.
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") n as isize => result,
            in("rdi") args[0], in("rsi") args[1], in("rdx") args[2],
            in("r10") args[3], in("r8") args[4], in("r9") args[5],
            lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    result
}
fn checked(value: isize) -> io::Result<usize> {
    if value < 0 {
        Err(io::Error::from_raw_os_error(-value as i32))
    } else {
        Ok(value as usize)
    }
}
/// Module-private: every caller passes arguments valid for its syscall
/// (pointers to live locals or borrowed slices, with their lengths).
fn call(n: usize, args: [usize; 6]) -> io::Result<usize> {
    // SAFETY: see above; each call site in this module is reviewed as such.
    checked(unsafe { syscall(n, args) })
}
pub fn default_sigpipe() -> io::Result<()> {
    sigpipe(0)
}
/// Sets SIGPIPE's disposition: `SIG_DFL` (0) or `SIG_IGN` (1).
fn sigpipe(disposition: u64) -> io::Result<()> {
    let action = [disposition, 0, 0, 0];
    call(13, [13, action.as_ptr() as usize, 0, 8, 0, 0]).map(|_| ())
}
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
