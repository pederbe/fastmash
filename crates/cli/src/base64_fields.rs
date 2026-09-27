//! Strict GNU-compatible base64 over raw selected field bytes.

use super::{
    Failure, failure,
    scalar_text::{ScalarText, WriteError},
    unsupported,
};
use base64::{Engine, engine::general_purpose::STANDARD};

enum CodecError {
    Invalid,
    Capacity,
}

pub(super) fn transform(
    text: &mut ScalarText,
    input: &[u8],
    decode: bool,
    line: u64,
    field: u64,
) -> Result<(), Failure> {
    // A decoded result cannot exceed its input length. Encoding needs checked expansion.
    let length = if decode {
        Some(input.len())
    } else {
        base64::encoded_len(input.len(), true)
    }
    .ok_or_else(|| unsupported("base64 output size overflow"))?;
    text.write_with(length, |output| {
        if decode {
            STANDARD
                .decode_slice(input, output)
                .map_err(|error| match error {
                    base64::DecodeSliceError::DecodeError(_) => CodecError::Invalid,
                    base64::DecodeSliceError::OutputSliceTooSmall => CodecError::Capacity,
                })
        } else {
            STANDARD
                .encode_slice(input, output)
                .map_err(|_| CodecError::Capacity)
        }
    })
    .map_err(|error| match error {
        WriteError::Allocation => unsupported("base64 output allocation failed"),
        WriteError::InvalidLength | WriteError::Transform(CodecError::Capacity) => {
            unsupported("base64 output size invariant failed")
        }
        WriteError::Transform(CodecError::Invalid) => {
            let mut message =
                format!("invalid base64 value in line {line} field {field}: '").into_bytes();
            let end = input
                .iter()
                .position(|&byte| byte == 0)
                .unwrap_or(input.len());
            message.extend_from_slice(&input[..end]);
            message.extend_from_slice(b"'\n");
            failure(message)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_transform_is_absent_and_failed_growth_preserves_old_value() {
        let mut text = ScalarText::default();
        text.replace(b"old").unwrap();
        text.fail_next_growth();
        assert_eq!(
            transform(&mut text, &[b'x'; 1024], false, 1, 1)
                .err()
                .unwrap()
                .status,
            77
        );
        assert_eq!(text.output(), Some(b"old".as_slice()));
        assert_eq!(
            transform(&mut text, b"!", true, 1, 1).err().unwrap().status,
            1
        );
        assert_eq!(text.output(), None);
        transform(&mut text, b"Zg==", true, 1, 1).ok().unwrap();
        assert_eq!(text.output(), Some(b"f".as_slice()));
    }

    #[test]
    fn strict_decode_and_complete_input_validation() {
        let mut text = ScalarText::default();
        for input in [b"Zg".as_slice(), b"Zh==", b"Zg==\n", b"Zg==\0", b"Zg==AAAA"] {
            assert_eq!(
                transform(&mut text, input, true, 1, 1)
                    .err()
                    .unwrap()
                    .status,
                1
            );
            assert_eq!(text.output(), None);
        }
        transform(&mut text, b"", true, 1, 1).ok().unwrap();
        assert_eq!(text.output(), Some(b"".as_slice()));
        transform(&mut text, b"a\0b", false, 1, 1).ok().unwrap();
        assert_eq!(text.output(), Some(b"YQBi".as_slice()));
    }
}
