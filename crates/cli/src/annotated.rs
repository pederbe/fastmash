//! Vnlog record interpretation, separate from raw sorter keys and numeric policy.
use super::{Failure, command_memory::reserve, failure};

fn first(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|b| !matches!(b, b' ' | b'\t'))
        .unwrap_or(bytes.len())
}
fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|b| !matches!(b, b' ' | b'\t'))
        .map_or(0, |i| i + 1);
    &bytes[..end]
}

pub(super) fn skip_data(bytes: &[u8]) -> bool {
    matches!(bytes.get(first(bytes)), None | Some(0 | b'#'))
}

pub(super) fn data(bytes: &[u8]) -> &[u8] {
    let end = bytes.iter().position(|b| *b == b'#').unwrap_or(bytes.len());
    trim_end(&bytes[..end])
}

/// Return false for ignored records. Only headers are shifted; data fields keep
/// their original offsets, including nonleading NUL bytes.
pub(super) fn prepare(bytes: &mut Vec<u8>, header: bool) -> Result<bool, Failure> {
    if !header {
        if skip_data(bytes) {
            return Ok(false);
        }
        bytes.truncate(data(bytes).len());
        return Ok(true);
    }
    let start = first(bytes);
    match bytes.get(start) {
        None | Some(0) => return Ok(false),
        Some(b'#') => {
            if matches!(bytes.get(start + 1), Some(b'#' | b'!')) {
                return Ok(false);
            }
            let content = start + 1 + first(&bytes[start + 1..]);
            if matches!(bytes.get(content), None | Some(0)) {
                return Ok(false);
            }
            let end = trim_end(bytes).len();
            bytes.copy_within(content..end, 0);
            bytes.truncate(end - content);
        }
        _ => {
            let visible = bytes.split(|b| *b == 0).next().unwrap();
            let prefix = b"invalid vnlog data: received record before header: '";
            let mut message = Vec::new();
            reserve(
                &mut message,
                prefix.len().saturating_add(visible.len()).saturating_add(2),
            )?;
            message.extend_from_slice(prefix);
            message.extend_from_slice(visible);
            message.extend_from_slice(b"'\n");
            return Err(failure(message));
        }
    }
    Ok(true)
}
