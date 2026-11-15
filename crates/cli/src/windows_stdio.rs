//! Checked owned standard-handle copies. Originals stay with process cleanup.
#![forbid(unsafe_code)]
use super::{console_utf8::Utf8, io_errors};
use io_close::Close;
use std::{
    fs::File,
    io::{self, IsTerminal, Read, Seek, Write},
    os::windows::io::{AsHandle, AsRawHandle, RawHandle},
    sync::{Mutex, OnceLock},
};

enum Stream {
    File(File),
    Console(File, Utf8),
    Failed(Option<i32>, io::ErrorKind),
    Closed,
}

static INPUT: OnceLock<Mutex<Stream>> = OnceLock::new();
static OUTPUT: OnceLock<Mutex<Stream>> = OnceLock::new();
static ERROR: OnceLock<Mutex<Stream>> = OnceLock::new();

fn absent(handle: RawHandle) -> bool {
    // -1 is invalid in the standard-stream table, not in every Windows API.
    handle.is_null() || handle as isize == -1
}

fn copy(stream: &(impl AsHandle + AsRawHandle + IsTerminal)) -> io::Result<Stream> {
    if absent(stream.as_raw_handle()) {
        return Err(io_errors::invalid_descriptor());
    }
    let owned = stream.as_handle().try_clone_to_owned()?;
    if absent(owned.as_raw_handle()) {
        return Err(io_errors::invalid_descriptor());
    }
    let file = File::from(owned);
    Ok(if stream.is_terminal() {
        Stream::Console(file, Utf8::default())
    } else {
        Stream::File(file)
    })
}

fn slot(fd: usize) -> &'static OnceLock<Mutex<Stream>> {
    match fd {
        0 => &INPUT,
        1 => &OUTPUT,
        2 => &ERROR,
        _ => unreachable!("standard stream slot"),
    }
}

fn with<T>(fd: usize, action: impl FnOnce(&mut Stream) -> io::Result<T>) -> io::Result<T> {
    let state = slot(fd).get_or_init(|| {
        let stream = match fd {
            0 => copy(&io::stdin()),
            1 => copy(&io::stdout()),
            _ => copy(&io::stderr()),
        };
        Mutex::new(stream.unwrap_or_else(|e| Stream::Failed(e.raw_os_error(), e.kind())))
    });
    let mut state = state
        .lock()
        .map_err(|_| io::Error::other("standard stream lock poisoned"))?;
    match &*state {
        Stream::Failed(code, kind) => {
            Err(code.map_or_else(|| io::Error::from(*kind), io::Error::from_raw_os_error))
        }
        Stream::Closed => Err(io_errors::invalid_descriptor()),
        _ => action(&mut state),
    }
}

pub(super) fn originally_closed(fd: usize) -> bool {
    match fd {
        0 => absent(io::stdin().as_raw_handle()),
        1 => absent(io::stdout().as_raw_handle()),
        _ => absent(io::stderr().as_raw_handle()),
    }
}

pub(super) fn read(fd: usize, bytes: &mut [u8]) -> io::Result<usize> {
    with(fd, |stream| match stream {
        Stream::File(file) => file.read(bytes),
        Stream::Console(..) => io::stdin().read(bytes),
        _ => unreachable!(),
    })
}

pub(super) fn write(fd: usize, bytes: &[u8]) -> io::Result<usize> {
    with(fd, |stream| match stream {
        Stream::File(file) => file.write(bytes),
        Stream::Console(_, text) => {
            if fd == 1 {
                text.write(bytes, &mut io::stdout().lock())
            } else {
                text.write(bytes, &mut io::stderr().lock())
            }
        }
        _ => unreachable!(),
    })
}

pub(super) fn seek(fd: usize, position: io::SeekFrom) -> io::Result<u64> {
    with(fd, |stream| match stream {
        Stream::File(file) if winapi_util::file::typ(&*file)?.is_disk() => file.seek(position),
        _ => Err(io::ErrorKind::Unsupported.into()),
    })
}

pub(super) fn regular_input_length() -> Option<u64> {
    with(0, |stream| {
        let Stream::File(file) = stream else {
            return Err(io::ErrorKind::Unsupported.into());
        };
        // Windows pipes can report is_file(), nonzero length and successful
        // seek without supporting replay. Native disk type must come first.
        if !winapi_util::file::typ(&*file)?.is_disk() {
            return Err(io::ErrorKind::Unsupported.into());
        }
        let metadata = file.metadata()?;
        if metadata.is_file() && metadata.len() != 0 {
            Ok(metadata.len())
        } else {
            Err(io::ErrorKind::Unsupported.into())
        }
    })
    .ok()
}

pub(super) fn close(fd: usize) -> io::Result<()> {
    // Early commands need not open or inspect unused input/output streams.
    let Some(state) = slot(fd).get() else {
        return Ok(());
    };
    let mut state = state
        .lock()
        .map_err(|_| io::Error::other("standard stream lock poisoned"))?;
    match std::mem::replace(&mut *state, Stream::Closed) {
        Stream::File(file) => file.close(),
        Stream::Console(file, text) => {
            let complete = text.finish().and_then(|()| match fd {
                1 => io::stdout().flush(),
                2 => io::stderr().flush(),
                _ => Ok(()),
            });
            let closed = file.close();
            complete.and(closed)
        }
        Stream::Failed(code, kind) => {
            Err(code.map_or_else(|| io::Error::from(kind), io::Error::from_raw_os_error))
        }
        Stream::Closed => Ok(()),
    }
}
