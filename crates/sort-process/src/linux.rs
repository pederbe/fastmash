//! Linux x86-64 process primitives. No application code runs after fork.
//! Internal to Fastmash's executables; not a stable API. Functions that act
//! on a descriptor borrow it, so they cannot outlive its owner.
use std::io;
use std::marker::PhantomData;
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd};

pub const CANCEL: u64 = (1 << 0) | (1 << 1) | (1 << 14);

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
pub fn mask(new: Option<u64>) -> io::Result<u64> {
    let mut old = 0u64;
    call(
        14,
        [
            2,
            new.as_ref().map_or(0, |p| p as *const u64 as usize),
            &mut old as *mut u64 as usize,
            8,
            0,
            0,
        ],
    )?;
    Ok(old)
}
pub fn restore_mask(old: u64) {
    if mask(Some(old)).is_err() {
        terminal(77);
    }
}
pub fn terminal(code: usize) -> ! {
    // SAFETY: exit_group takes no pointers and ends the process.
    unsafe {
        syscall(231, [code, 0, 0, 0, 0, 0]);
    }
    // exit_group cannot return for a valid process; never resume unsafe state.
    std::process::abort()
}
pub fn waitable_children() -> io::Result<()> {
    let mut action = [0u64; 4];
    call(13, [17, 0, action.as_mut_ptr() as usize, 8, 0, 0])?;
    if action[0] == 1 || action[1] & 2 != 0 {
        return Err(io::Error::other("SIGCHLD is not waitable"));
    }
    Ok(())
}
pub fn cancellation_dispositions() -> io::Result<()> {
    for signal in [1, 2, 15] {
        let mut action = [0u64; 4];
        call(13, [signal, 0, action.as_mut_ptr() as usize, 8, 0, 0])?;
        if action[0] != 0 {
            return Err(io::Error::other(
                "sorting requires default cancellation dispositions",
            ));
        }
    }
    Ok(())
}
pub fn default_sigpipe() -> io::Result<()> {
    let action = [0u64; 4];
    call(13, [13, action.as_ptr() as usize, 0, 8, 0, 0]).map(|_| ())
}
pub fn pidfd(pid: u32) -> io::Result<OwnedFd> {
    let fd = call(434, [pid as usize, 0, 0, 0, 0, 0])?;
    // SAFETY: pidfd_open returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}
pub fn pid_signal(fd: BorrowedFd<'_>, signal: i32) -> io::Result<()> {
    call(424, [fd.as_raw_fd() as usize, signal as usize, 0, 0, 0, 0]).map(|_| ())
}
pub fn parent_death(expected: u32) -> io::Result<()> {
    call(157, [1, 9, 0, 0, 0, 0])?;
    if call(110, [0; 6])? != expected as usize {
        return Err(io::Error::other("supervisor exited before sort startup"));
    }
    Ok(())
}
pub fn signal_pid(pid: u32, signal: i32) -> io::Result<()> {
    call(62, [pid as usize, signal as usize, 0, 0, 0, 0]).map(|_| ())
}
/// Close standard input and output.
///
/// # Safety
/// No live Rust value may own or use descriptors 0 or 1 afterward, and the
/// caller must not read standard input or write standard output again.
pub unsafe fn close_streams() {
    // SAFETY: close takes no pointers; the caller guarantees no owner remains.
    unsafe {
        syscall(3, [0, 0, 0, 0, 0, 0]);
        syscall(3, [1, 0, 0, 0, 0, 0]);
    }
}
pub fn close_on_exec_extras() -> io::Result<()> {
    // CLOSE_RANGE_CLOEXEC preserves runtime-owned descriptors until exec.
    call(436, [3, u32::MAX as usize, 4, 0, 0, 0]).map(|_| ())
}
/// Whether `fd` names an open descriptor (F_GETFD), without owning it.
pub fn is_open(fd: RawFd) -> bool {
    fd >= 0 && call(72, [fd as usize, 1, 0, 0, 0, 0]).is_ok()
}
pub fn cloexec(fd: BorrowedFd<'_>, enabled: bool) -> io::Result<()> {
    let fd = fd.as_raw_fd() as usize;
    let flags = call(72, [fd, 1, 0, 0, 0, 0])?;
    call(
        72,
        [fd, 2, if enabled { flags | 1 } else { flags & !1 }, 0, 0, 0],
    )
    .map(|_| ())
}
pub fn duplicate(fd: BorrowedFd<'_>) -> io::Result<OwnedFd> {
    let new = call(72, [fd.as_raw_fd() as usize, 1030, 3, 0, 0, 0])?;
    // SAFETY: F_DUPFD_CLOEXEC returned a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(new as RawFd) })
}
pub fn socket_pair() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [-1i32; 2];
    call(
        53,
        [1, 5 | 0x80000 | 0x800, 0, fds.as_mut_ptr() as usize, 0, 0],
    )?;
    // SAFETY: socketpair returned two new descriptors that nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}
pub fn send(fd: BorrowedFd<'_>, bytes: &[u8]) -> io::Result<()> {
    let n = call(
        44,
        [
            fd.as_raw_fd() as usize,
            bytes.as_ptr() as usize,
            bytes.len(),
            0x4000 | 0x40,
            0,
            0,
        ],
    )?;
    if n != bytes.len() {
        return Err(io::Error::other("short control record"));
    }
    Ok(())
}
pub fn receive(fd: BorrowedFd<'_>, bytes: &mut [u8]) -> io::Result<usize> {
    call(
        45,
        [
            fd.as_raw_fd() as usize,
            bytes.as_mut_ptr() as usize,
            bytes.len(),
            0x40,
            0,
            0,
        ],
    )
}
/// A `struct pollfd` borrowing its descriptor for the poll call.
#[repr(C)]
pub struct Poll<'fd> {
    fd: RawFd,
    events: i16,
    pub returned: i16,
    descriptor: PhantomData<BorrowedFd<'fd>>,
}
impl<'fd> Poll<'fd> {
    pub fn new(fd: BorrowedFd<'fd>, events: i16) -> Self {
        Self {
            fd: fd.as_raw_fd(),
            events,
            returned: 0,
            descriptor: PhantomData,
        }
    }
}
pub fn poll(fds: &mut [Poll<'_>], millis: i32) -> io::Result<()> {
    match call(
        7,
        [
            fds.as_mut_ptr() as usize,
            fds.len(),
            millis as usize,
            0,
            0,
            0,
        ],
    ) {
        Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(()),
        result => result.map(|_| ()),
    }
}
pub fn signal_fd() -> io::Result<OwnedFd> {
    let bits = CANCEL;
    let fd = call(
        289,
        [
            usize::MAX,
            &bits as *const u64 as usize,
            8,
            0x80000 | 0x800,
            0,
            0,
        ],
    )?;
    // SAFETY: signalfd4 with fd -1 returned a new descriptor nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(fd as RawFd) })
}
pub fn read_signal(fd: &OwnedFd) -> io::Result<Option<i32>> {
    let mut record = [0u8; 128];
    match call(
        0,
        [
            fd.as_raw_fd() as usize,
            record.as_mut_ptr() as usize,
            record.len(),
            0,
            0,
            0,
        ],
    ) {
        Ok(128) => Ok(Some(
            u32::from_ne_bytes(record[..4].try_into().unwrap()) as i32
        )),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(e),
        _ => Err(io::Error::other("invalid signal record")),
    }
}
pub fn random(bytes: &mut [u8]) -> io::Result<()> {
    let mut at = 0;
    while at < bytes.len() {
        match call(
            318,
            [
                bytes[at..].as_mut_ptr() as usize,
                bytes.len() - at,
                0,
                0,
                0,
                0,
            ],
        ) {
            Ok(0) => return Err(io::Error::other("empty random read")),
            Ok(n) => at += n,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
