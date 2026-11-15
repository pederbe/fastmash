//! Platform meanings used by diagnostics and output completion together.
#![forbid(unsafe_code)]
use std::io;

pub(super) fn bad_descriptor(code: Option<i32>) -> bool {
    #[cfg(unix)]
    return code == Some(9);
    #[cfg(windows)]
    return code == Some(6);
}

pub(super) fn invalid_descriptor() -> io::Error {
    #[cfg(unix)]
    return io::Error::from_raw_os_error(9);
    #[cfg(windows)]
    return io::Error::from_raw_os_error(6);
}

pub(super) fn supported_output(code: Option<i32>, kind: io::ErrorKind) -> bool {
    #[cfg(unix)]
    {
        let _ = kind;
        matches!(code, Some(9 | 28))
    }
    #[cfg(windows)]
    {
        bad_descriptor(code)
            || matches!(
                kind,
                io::ErrorKind::PermissionDenied
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::StorageFull
                    | io::ErrorKind::WriteZero
            )
            || matches!(code, Some(39 | 112 | 1117))
    }
}

pub(super) fn device_read(code: Option<i32>) -> bool {
    #[cfg(unix)]
    return code == Some(5);
    #[cfg(windows)]
    return code == Some(1117);
}
