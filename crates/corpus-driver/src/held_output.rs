//! Owned nonblocking destinations for observations made before input EOF.
use crate::{Error, Result};
use nix::errno::Errno;
use nix::fcntl::{FcntlArg, FdFlag, OFlag, fcntl};
use nix::pty::openpty;
use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::fs::{FileExt, FileTypeExt, MetadataExt};
use std::process::Stdio;

#[derive(Clone, Debug, Serialize)]
pub struct Destination {
    pub isatty: bool,
    pub block_size: u64,
}

pub struct Streaming {
    reader: File,
    terminal: bool,
    ended: bool,
    pub bytes: Vec<u8>,
}

impl Streaming {
    pub fn new(terminal: bool) -> Result<(Self, Stdio, Destination)> {
        let (reader, writer) = if terminal {
            let pair = openpty(None, None)?;
            let mut attributes = tcgetattr(&pair.slave)?;
            cfmakeraw(&mut attributes);
            tcsetattr(&pair.slave, SetArg::TCSANOW, &attributes)?;
            // Both native openpty descriptors must close across exec except
            // for the slave that Command deliberately installs as stdout.
            for fd in [&pair.master, &pair.slave] {
                fcntl(fd, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC))?;
            }
            (File::from(pair.master), File::from(pair.slave))
        } else {
            let (reader, writer) = std::io::pipe()?;
            (
                File::from(OwnedFd::from(reader)),
                File::from(OwnedFd::from(writer)),
            )
        };
        let metadata = writer.metadata()?;
        let facts = destination(&writer)?;
        if facts.isatty != terminal
            || (terminal && !metadata.file_type().is_char_device())
            || (!terminal && !metadata.file_type().is_fifo())
            || facts.block_size == 0
            || (cfg!(target_os = "linux") && facts.block_size != if terminal { 1024 } else { 4096 })
        {
            return Err(Error::message("held-output destination facts differ"));
        }
        let flags = OFlag::from_bits_truncate(fcntl(&reader, FcntlArg::F_GETFL)?);
        fcntl(&reader, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
        Ok((
            Self {
                reader,
                terminal,
                ended: false,
                bytes: Vec::new(),
            },
            Stdio::from(writer),
            facts,
        ))
    }

    pub fn drain(&mut self, cap: usize) -> Result<()> {
        while !self.ended && self.bytes.len() <= cap {
            let mut buffer = [0; 65536];
            let remaining = (cap + 1 - self.bytes.len()).min(buffer.len());
            match self.reader.read(&mut buffer[..remaining]) {
                Ok(0) => self.ended = true,
                Ok(length) => self.bytes.extend_from_slice(&buffer[..length]),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
                // A PTY master reports its final slave close as EIO on Linux.
                Err(error) if self.terminal && error.raw_os_error() == Some(Errno::EIO as i32) => {
                    self.ended = true;
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub fn ended(&self) -> bool {
        self.ended
    }
}

pub fn destination(file: &File) -> Result<Destination> {
    let isatty = match nix::unistd::isatty(file) {
        Ok(value) => value,
        Err(Errno::ENOTTY) => false,
        Err(error) => return Err(error.into()),
    };
    Ok(Destination {
        isatty,
        block_size: file.metadata()?.blksize(),
    })
}

pub fn snapshot(file: &File, cap: usize) -> Result<Vec<u8>> {
    let mut bytes = vec![0; cap + 1];
    // Reading through a cloned file's shared offset would reposition the
    // candidate's live stdout. A positional read leaves that offset intact.
    let length = file.read_at(&mut bytes, 0)?;
    bytes.truncate(length);
    Ok(bytes)
}
