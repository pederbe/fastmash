//! Independent source lifetimes inside the Command's input seam.
use super::{Failure, command_memory, failure, intake, platform};
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, BufRead, Read},
    os::{fd::IntoRawFd, unix::ffi::OsStrExt},
};

pub(super) trait Resolver {
    fn with_source<T>(
        &mut self,
        stdin: &mut dyn BufRead,
        path: &OsStr,
        scan: impl FnOnce(&mut dyn BufRead) -> Result<T, Failure>,
    ) -> Result<T, Failure>;
}

pub(super) struct Files;
impl Resolver for Files {
    fn with_source<T>(
        &mut self,
        stdin: &mut dyn BufRead,
        path: &OsStr,
        scan: impl FnOnce(&mut dyn BufRead) -> Result<T, Failure>,
    ) -> Result<T, Failure> {
        if path.as_bytes() == b"-" {
            return scan(stdin);
        }
        let mut bytes = Vec::new();
        command_memory::reserve_exact(&mut bytes, 128 * 1024)?;
        bytes.resize(128 * 1024, 0);
        let file = File::open(path)
            .map_err(|error| failure(format!("open error: {error}\n").into_bytes()))?;
        let mut input = FileInput {
            file,
            bytes,
            start: 0,
            end: 0,
        };
        let result = scan(&mut input);
        let fd = input.file.into_raw_fd();
        // Ownership was taken from the File; close errors remain part of the
        // source's completion rather than being lost through File::drop.
        let completion = platform::close(fd as usize).map_err(|error| close_failure(&error));
        complete(result, completion)
    }
}

pub(super) fn close_failure(error: &io::Error) -> Failure {
    let mut failure = intake::read_failure(error);
    failure.message = format!("input close error: {error}\n").into_bytes();
    failure
}

/// Retains both causes while following the Command's failure precedence.
pub(super) fn complete<T>(
    result: Result<T, Failure>,
    completion: Result<(), Failure>,
) -> Result<T, Failure> {
    match (result, completion) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error),
        (Err(mut first), Err(later)) => {
            first.status = super::failure::combine(first.status, later.status);
            super::append(&mut first, &[super::reported(&later)]);
            Err(first)
        }
    }
}

struct FileInput {
    file: File,
    bytes: Vec<u8>,
    start: usize,
    end: usize,
}
impl Read for FileInput {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let available = self.fill_buf()?;
        let count = output.len().min(available.len());
        output[..count].copy_from_slice(&available[..count]);
        self.consume(count);
        Ok(count)
    }
}
impl BufRead for FileInput {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.start == self.end {
            self.end = self.file.read(&mut self.bytes)?;
            self.start = 0;
        }
        Ok(&self.bytes[self.start..self.end])
    }
    fn consume(&mut self, count: usize) {
        self.start += count;
    }
}
