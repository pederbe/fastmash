#![forbid(unsafe_code)]
//! Native operations on the three runtime-owned standard descriptors.
use std::io;
use std::os::fd::BorrowedFd;

fn descriptor(fd: usize) -> io::Result<BorrowedFd<'static>> {
    match fd {
        0 => Ok(rustix::stdio::stdin()),
        1 => Ok(rustix::stdio::stdout()),
        2 => Ok(rustix::stdio::stderr()),
        _ => Err(io::Error::from_raw_os_error(libc::EBADF)),
    }
}

pub(super) fn read(fd: usize, bytes: &mut [u8]) -> io::Result<usize> {
    rustix::io::read(descriptor(fd)?, bytes).map_err(io::Error::from)
}

pub(super) fn write(fd: usize, bytes: &[u8]) -> io::Result<usize> {
    rustix::io::write(descriptor(fd)?, bytes).map_err(io::Error::from)
}

pub(super) fn seek(fd: usize, position: io::SeekFrom) -> io::Result<u64> {
    let position = match position {
        io::SeekFrom::Start(offset) => rustix::fs::SeekFrom::Start(offset),
        io::SeekFrom::Current(offset) => rustix::fs::SeekFrom::Current(offset),
        io::SeekFrom::End(offset) => rustix::fs::SeekFrom::End(offset),
    };
    rustix::fs::seek(descriptor(fd)?, position).map_err(io::Error::from)
}
