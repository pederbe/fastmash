//! Native errors for the existing synthetic transport fixtures.
//!
//! Fixture numbers retain their original Unix meanings. Translate when creating
//! an error, before production code sees it. A read's EIO is a device error,
//! while a write's EIO exercises the deliberately unsupported-output path.
use std::io;
pub(super) const BAD_WRITE: &str = if cfg!(windows) {
    "write error: Invalid handle\n"
} else {
    "write error: Bad file descriptor\n"
};
pub(super) const BAD_READ: &str = if cfg!(windows) {
    "read error: Invalid handle\n"
} else {
    "read error: Bad file descriptor\n"
};

pub(super) fn write_code(reference: i32) -> i32 {
    if cfg!(windows) {
        match reference {
            4 => 995,
            5 => 123456,
            9 => 6,
            28 => 112,
            value => value,
        }
    } else {
        reference
    }
}
pub(super) fn read_code(reference: i32) -> i32 {
    if cfg!(windows) && reference == 5 {
        1117
    } else {
        write_code(reference)
    }
}
pub(super) fn write(reference: i32) -> io::Error {
    if cfg!(windows) && reference == 4 {
        io::ErrorKind::Interrupted.into()
    } else {
        io::Error::from_raw_os_error(write_code(reference))
    }
}
pub(super) fn read(reference: i32) -> io::Error {
    if cfg!(windows) && reference == 21 {
        io::ErrorKind::IsADirectory.into()
    } else {
        io::Error::from_raw_os_error(read_code(reference))
    }
}
