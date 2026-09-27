//! Raw Linux x86-64 syscalls.

/// Raw Linux x86-64 syscall with memory effects and architectural clobbers.
///
/// # Safety
/// The arguments must satisfy the syscall's contract: pointers valid for the
/// lengths given for as long as the kernel uses them, and no effect on memory
/// or descriptors that live Rust values own.
pub unsafe fn syscall(number: usize, a: usize, b: usize, c: usize, d: usize) -> isize {
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
