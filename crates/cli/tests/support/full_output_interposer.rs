//! Darwin test transports: selected output ENOSPC and selected input EOF EIO.
//! Build as a cdylib; dyld never applies an image's interposition to itself.
#![no_std]

use core::ffi::{c_char, c_int, c_void};

type Write = unsafe extern "C" fn(c_int, *const c_void, usize) -> isize;
type Read = unsafe extern "C" fn(c_int, *mut c_void, usize) -> isize;

#[link(name = "System")]
unsafe extern "C" {
    fn write(fd: c_int, bytes: *const c_void, length: usize) -> isize;
    fn read(fd: c_int, bytes: *mut c_void, length: usize) -> isize;
    fn getenv(name: *const c_char) -> *const c_char;
    fn __error() -> *mut c_int;
    fn abort() -> !;
}

unsafe extern "C" fn failed_read(fd: c_int, bytes: *mut c_void, length: usize) -> isize {
    // SAFETY: the exact read ABI delegates the original arguments. dyld
    // excludes this image from its own tuple, preventing recursion.
    let result = unsafe { read(fd, bytes, length) };
    if fd != 0 || length == 0 || result != 0 {
        return result;
    }
    // Preserve the native errno even if looking up the selector changes it.
    // SAFETY: __error returns this thread's live native errno cell; getenv
    // accepts a static terminated name and its value remains C-owned.
    let saved = unsafe { *__error() };
    let selected = unsafe { getenv(c"FASTMASH_TEST_READ_ERROR".as_ptr()) };
    // SAFETY: after a nonzero first byte, this terminated value has a second byte.
    let enabled =
        !selected.is_null() && unsafe { *selected == b'1' as c_char && *selected.add(1) == 0 };
    // SAFETY: the same errno cell is writable by this thread alone.
    unsafe { *__error() = if enabled { 5 } else { saved } }; // Darwin EIO.
    if enabled { -1 } else { result }
}

unsafe extern "C" fn full_write(fd: c_int, bytes: *const c_void, length: usize) -> isize {
    // SAFETY: getenv accepts this static terminated name. Its value remains
    // owned by the C environment, which the test child never changes.
    let selected = unsafe { getenv(c"FASTMASH_TEST_FULL_FD".as_ptr()) };
    if !selected.is_null() && (fd == 1 || fd == 2) {
        // SAFETY: getenv returned a terminated string; after its first byte,
        // a second byte exists unless the first byte is already the terminator.
        let matches =
            unsafe { *selected == b'0' as c_char + fd as c_char && *selected.add(1) == 0 };
        if matches {
            // SAFETY: __error returns this thread's writable native errno cell.
            unsafe { *__error() = 28 }; // Darwin ENOSPC.
            return -1;
        }
    }
    // SAFETY: the exact write ABI forwards the caller's arguments unchanged.
    // dyld excludes this image from its own tuple, so this calls native write.
    unsafe { write(fd, bytes, length) }
}

#[used]
#[unsafe(link_section = "__DATA,__interpose")]
static INTERPOSE: [Write; 2] = [full_write, write];

#[used]
#[unsafe(link_section = "__DATA,__interpose")]
static READ_INTERPOSE: [Read; 2] = [failed_read, read];

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    // SAFETY: abort ends an impossible internal failure without runtime setup.
    unsafe { abort() }
}
