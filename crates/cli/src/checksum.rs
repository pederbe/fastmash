//! Compatibility checksums over complete selected field bytes.
use super::{Failure, scalar_text::ScalarText, scalar_text_failure};
use md5::Digest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Algorithm {
    Md5,
    Sha1,
    Sha224,
    Sha256,
    Sha384,
    Sha512,
}

impl Algorithm {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Md5 => "md5",
            Self::Sha1 => "sha1",
            Self::Sha224 => "sha224",
            Self::Sha256 => "sha256",
            Self::Sha384 => "sha384",
            Self::Sha512 => "sha512",
        }
    }

    pub(super) fn from_name(name: &[u8]) -> Option<Self> {
        [
            Self::Md5,
            Self::Sha1,
            Self::Sha224,
            Self::Sha256,
            Self::Sha384,
            Self::Sha512,
        ]
        .into_iter()
        .find(|kind| name.eq_ignore_ascii_case(kind.name().as_bytes()))
    }

    pub(super) fn store(self, input: &[u8], text: &mut ScalarText) -> Result<(), Failure> {
        match self {
            Self::Md5 => store::<md5::Md5>(input, text),
            Self::Sha1 => store::<sha1::Sha1>(input, text),
            Self::Sha224 => store::<sha2::Sha224>(input, text),
            Self::Sha256 => store::<sha2::Sha256>(input, text),
            Self::Sha384 => store::<sha2::Sha384>(input, text),
            Self::Sha512 => store::<sha2::Sha512>(input, text),
        }
    }
}

fn store<D: Digest>(input: &[u8], text: &mut ScalarText) -> Result<(), Failure> {
    let digest = D::digest(input);
    // The largest supported digest is SHA-512: 64 bytes, two hex digits each.
    let mut hex = [0u8; 128];
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (byte, pair) in digest.iter().zip(hex.as_chunks_mut::<2>().0.iter_mut()) {
        pair[0] = DIGITS[(byte >> 4) as usize];
        pair[1] = DIGITS[(byte & 15) as usize];
    }
    text.replace(&hex[..digest.len() * 2])
        .map_err(scalar_text_failure)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocation_failure_preserves_old_output_and_next_row_replaces_it() {
        let mut text = ScalarText::default();
        text.replace(b"old").unwrap();
        text.fail_next_growth();
        assert_eq!(
            Algorithm::Sha512
                .store(b"abc", &mut text)
                .err()
                .unwrap()
                .status,
            77
        );
        assert_eq!(text.output(), Some(b"old".as_slice()));
        Algorithm::Md5.store(b"abc", &mut text).ok().unwrap();
        assert_eq!(
            text.output(),
            Some(b"900150983cd24fb0d6963f7d28e17f72".as_slice())
        );
        Algorithm::Md5.store(b"", &mut text).ok().unwrap();
        assert_eq!(
            text.output(),
            Some(b"d41d8cd98f00b204e9800998ecf8427e".as_slice())
        );
    }
}
