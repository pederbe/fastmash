//! The audited exception for process startup, described in ADR 0017.
//!
//! Rust repairs closed standard descriptors before ordinary application code
//! can inspect them. No safe interface observes their original state. Capture
//! it before the runtime, then occupy closed slots with the wrong access mode
//! so later opens cannot turn an invalid inherited stream into successful I/O.
//! Linux also keeps its direct glibc entry to avoid Rust's stack-guard lookup
//! on every tiny Command, with the SIGPIPE setup that runtime start supplies.
//! No allocation, Rust I/O or runtime initialization belongs in capture.

use std::sync::atomic::{AtomicU8, Ordering};

static CLOSED: AtomicU8 = AtomicU8::new(0);

// ELF executable preinitializers run before Rust's standard-fd sanitization.
// Only raw syscalls and an atomic store are safe here; no runtime is initialized.
// SAFETY: ELF calls this no-argument function before main, once. It uses only
// native syscalls and statically initialized atomics before Rust can repair fds.
#[used]
#[cfg(target_os = "linux")]
#[cfg_attr(target_env = "gnu", unsafe(link_section = ".preinit_array"))]
#[cfg_attr(target_env = "musl", unsafe(link_section = ".init_array"))]
static CAPTURE: unsafe extern "C" fn() = capture;

#[cfg(target_os = "linux")]
unsafe extern "C" fn capture() {
    let mut closed = 0;
    for fd in 0..3 {
        // F_GETFD identifies closed slots without reading or writing the stream.
        // SAFETY: fcntl F_GETFD takes no pointers.
        if unsafe { syscall(72, fd, 1, 0, 0) } == -9 {
            closed |= 1 << fd;
            // Occupy the slot so later opens cannot alias a standard stream.
            // Wrong access retains EBADF for children as well as this process.
            // SAFETY: the path is a static NUL-terminated string; the slot is free.
            let opened = unsafe {
                syscall(
                    257,
                    (-100isize) as usize,
                    c"/dev/null".as_ptr() as usize,
                    usize::from(fd == 0),
                    0,
                )
            };
            if opened != fd as isize {
                // No allocation or diagnostic is safe during failed initialization.
                // SAFETY: exit_group takes no pointers and ends the process.
                unsafe { syscall(231, 1, 0, 0, 0) };
            }
        }
    }
    CLOSED.store(closed, Ordering::Relaxed);
}

// dyld runs Mach-O constructors before Rust's runtime initializes and repairs
// closed standard descriptors. libc is available here; allocation and Rust's
// runtime are not. Native process tests verify this ordering.
// SAFETY: dyld calls this no-argument constructor once before main. Its native
// descriptor calls and static atomic store require no Rust runtime state.
#[cfg(target_os = "macos")]
#[used]
#[unsafe(link_section = "__DATA,__mod_init_func,mod_init_funcs")]
static CAPTURE: unsafe extern "C" fn() = capture;

#[cfg(target_os = "macos")]
unsafe extern "C" fn capture() {
    let mut closed = 0;
    for fd in 0..3 {
        // SAFETY: fcntl F_GETFD takes no pointers and inspects an inherited fd.
        if unsafe { libc::fcntl(fd, libc::F_GETFD) } == -1 {
            // SAFETY: __error returns this thread's live native errno slot.
            if unsafe { *libc::__error() } != libc::EBADF {
                // SAFETY: _exit ends the process without runtime teardown.
                unsafe { libc::_exit(1) };
            }
            closed |= 1 << fd;
            // Hold each closed slot with the wrong access mode, preserving an
            // EBADF result and preventing later opens from aliasing a stream.
            let access = if fd == 0 {
                libc::O_WRONLY
            } else {
                libc::O_RDONLY
            };
            // SAFETY: open reads a constant NUL-terminated path, no mode is
            // needed without O_CREAT, and the expected descriptor is free.
            let opened = unsafe { libc::open(c"/dev/null".as_ptr(), access) };
            if opened != fd {
                // SAFETY: no runtime or diagnostic is safe after failed capture.
                unsafe { libc::_exit(1) };
            }
        }
    }
    CLOSED.store(closed, Ordering::Relaxed);
}

pub(super) fn originally_closed(fd: usize) -> bool {
    CLOSED.load(Ordering::Relaxed) & (1 << fd) != 0
}

/// Raw Linux x86-64 syscall with memory effects and architectural clobbers.
///
/// # Safety
/// The arguments must satisfy the syscall's contract: pointers valid for the
/// lengths given for as long as the kernel uses them, and no effect on memory
/// or descriptors that live Rust values own.
#[cfg(target_os = "linux")]
unsafe fn syscall(number: usize, a: usize, b: usize, c: usize, d: usize) -> isize {
    let result: isize;
    // SAFETY: the syscall instruction clobbers only rax, rcx and r11, as
    // declared; the caller upholds the syscall's own contract.
    unsafe {
        core::arch::asm!("syscall", inlateout("rax") number => result,
            in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d,
            lateout("rcx") _, lateout("r11") _, options(nostack));
    }
    result
}

/// The Linux C entry supplies SIGPIPE setup without Rust's stack-guard lookup.
/// glibc has initialized std's arguments. Musl passes its native arguments here.
#[cfg(all(not(test), target_os = "linux"))]
// SAFETY: this is the unique main symbol with the Linux C entry ABI. Native
// arguments remain live throughout the call. No panic unwinds across this ABI.
#[unsafe(no_mangle)]
extern "C" fn main(_argc: libc::c_int, _argv: *const *const libc::c_char) -> libc::c_int {
    // SAFETY: the process is still single-threaded. SIGPIPE is a valid signal,
    // and SIG_IGN is the disposition Rust's runtime start would install.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    #[cfg(target_env = "gnu")]
    {
        super::malloc_arenas::cap();
        super::run_process(std::env::args_os().map(Ok));
    }
    #[cfg(target_env = "musl")]
    {
        let arguments = (0.._argc).map(|index| {
            // SAFETY: the C entry receives argc valid argument pointers. The
            // index is below argc and every argument is NUL terminated.
            let argument = unsafe { std::ffi::CStr::from_ptr(*_argv.add(index as usize)) };
            super::options::copy_argument(argument.to_bytes())
        });
        super::run_process(arguments);
    }
    0
}
