//! Field boundaries and GNU's missing-value tokens.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FieldRange {
    pub start: usize,
    pub length: usize,
}

/// Whether a whole field is one of GNU's case-insensitive missing-value tokens.
pub(crate) fn is_na(part: &[u8]) -> bool {
    [b"NA".as_slice(), b"N/A".as_slice(), b"NAN".as_slice()]
        .iter()
        .any(|word| part.eq_ignore_ascii_case(word))
}
