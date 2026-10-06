//! Calibrate native write and read faults, including disabled controls.
use std::{
    ffi::{c_int, c_void},
    os::fd::AsRawFd,
};

unsafe extern "C" {
    fn write(fd: c_int, bytes: *const c_void, length: usize) -> isize;
    fn read(fd: c_int, bytes: *mut c_void, length: usize) -> isize;
    fn __error() -> *mut c_int;
}

fn main() {
    let selected = std::env::args().nth(1).expect("selected descriptor");
    if selected == "read" {
        read_probe(std::env::args().nth(2).as_deref() == Some("error"));
        return;
    }
    if selected == "read-access" {
        let mut byte = 0u8;
        // SAFETY: the probe caller deliberately installed write-only stdin;
        // read receives one live byte and must preserve its native EBADF.
        unsafe {
            assert_eq!(read(0, (&mut byte as *mut u8).cast(), 1), -1);
            assert_eq!(*__error(), 9);
        }
        return;
    }
    let selected: c_int = selected.parse().expect("integer descriptor");
    assert!((0..=2).contains(&selected));
    let mut valid = true;
    for (fd, bytes) in [(1, b"stdout\n".as_slice()), (2, b"stderr\n".as_slice())] {
        // SAFETY: the byte slice is live for the synchronous native write.
        let result = unsafe { write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if fd == selected {
            // SAFETY: __error returns this thread's native errno cell.
            valid &= result == -1 && unsafe { *__error() } == 28;
        } else {
            valid &= result == bytes.len() as isize;
        }
    }
    std::process::exit(if valid { 0 } else { 1 });
}

fn read_probe(expected_error: bool) {
    let mut bytes = [0u8; 3];
    // SAFETY: a zero-length request uses live storage and must remain ordinary
    // EOF; the native success preserves this thread's errno sentinel.
    unsafe {
        *__error() = 22;
        assert_eq!(read(0, bytes.as_mut_ptr().cast(), 0), 0);
        assert_eq!(*__error(), 22);
    }
    let other = std::fs::File::open("/dev/null").unwrap();
    assert!(other.as_raw_fd() > 2);
    // SAFETY: reads use owned/live descriptors or the deliberately invalid -1;
    // every output pointer covers the three-byte buffer passed to read.
    unsafe {
        *__error() = 22;
        assert_eq!(
            read(other.as_raw_fd(), bytes.as_mut_ptr().cast(), bytes.len()),
            0
        );
        assert_eq!(*__error(), 22);
        assert_eq!(read(-1, bytes.as_mut_ptr().cast(), bytes.len()), -1);
        assert_eq!(*__error(), 9);
    }
    let mut collected = Vec::new();
    let mut short = false;
    loop {
        // SAFETY: read writes at most three bytes into this live buffer.
        let result = unsafe {
            *__error() = 22;
            read(0, bytes.as_mut_ptr().cast(), bytes.len())
        };
        if result > 0 {
            assert!(result as usize <= bytes.len());
            // SAFETY: __error returns this thread's initialized errno cell.
            assert_eq!(unsafe { *__error() }, 22);
            short |= (result as usize) < bytes.len();
            collected.extend_from_slice(&bytes[..result as usize]);
        } else {
            assert_eq!(result, if expected_error { -1 } else { 0 });
            // SAFETY: as above; EIO replaces EOF only under the explicit selector.
            assert_eq!(unsafe { *__error() }, if expected_error { 5 } else { 22 });
            break;
        }
    }
    assert!(
        short,
        "probe input must include a short final positive read"
    );
    assert_eq!(collected, b"complete\npartial");
    // SAFETY: output owns the complete collected bytes until write returns.
    assert_eq!(
        unsafe { write(1, collected.as_ptr().cast(), collected.len()) },
        collected.len() as isize
    );
}
