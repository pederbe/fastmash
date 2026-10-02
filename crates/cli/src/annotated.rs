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

/// Reads a Record under the vnlog prologue rules: false for a skipped Record;
/// for the `# ` header, strips its annotation in place. Data before the header
/// is an error (text-lines.c line_record_fread). Data Records are never
/// shifted: see `data`.
pub(super) fn header(bytes: &mut Vec<u8>) -> Result<bool, Failure> {
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
