//! Keep UTF-8 scalars intact across application-buffer writes to a console.
#![forbid(unsafe_code)]
use std::io::{self, Write};

#[derive(Default)]
pub(super) struct Utf8 {
    tail: [u8; 4],
    length: usize,
    error: Option<(Option<i32>, io::ErrorKind)>,
}

impl Utf8 {
    pub(super) fn write(&mut self, bytes: &[u8], writer: &mut impl Write) -> io::Result<usize> {
        if let Some((code, kind)) = self.error {
            return Err(code.map_or_else(|| io::Error::from(kind), io::Error::from_raw_os_error));
        }
        let result = self.write_inner(bytes, writer);
        if let Err(error) = &result {
            self.error = Some((error.raw_os_error(), error.kind()));
        }
        result
    }

    fn write_inner(&mut self, mut bytes: &[u8], writer: &mut impl Write) -> io::Result<usize> {
        let consumed = bytes.len();
        while self.length != 0 && !bytes.is_empty() {
            self.tail[self.length] = bytes[0];
            self.length += 1;
            bytes = &bytes[1..];
            match std::str::from_utf8(&self.tail[..self.length]) {
                Ok(_) => {
                    writer.write_all(&self.tail[..self.length])?;
                    self.length = 0;
                }
                Err(error) if error.error_len().is_none() => {}
                Err(_) => return Err(io::ErrorKind::InvalidData.into()),
            }
        }
        match std::str::from_utf8(bytes) {
            Ok(_) => writer.write_all(bytes)?,
            Err(error) if error.error_len().is_none() => {
                let end = error.valid_up_to();
                writer.write_all(&bytes[..end])?;
                let tail = &bytes[end..];
                self.tail[..tail.len()].copy_from_slice(tail);
                self.length = tail.len();
            }
            Err(_) => return Err(io::ErrorKind::InvalidData.into()),
        }
        Ok(consumed)
    }

    pub(super) fn finish(&self) -> io::Result<()> {
        if let Some((code, kind)) = self.error {
            Err(code.map_or_else(|| io::Error::from(kind), io::Error::from_raw_os_error))
        } else if self.length == 0 {
            Ok(())
        } else {
            Err(io::ErrorKind::InvalidData.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_split_keeps_unicode_scalars_complete() {
        let input = "aé€🦀\n".as_bytes();
        for chunk in 1..=input.len() {
            let mut utf8 = Utf8::default();
            let mut output = Vec::new();
            for bytes in input.chunks(chunk) {
                assert_eq!(utf8.write(bytes, &mut output).unwrap(), bytes.len());
                assert!(std::str::from_utf8(&output).is_ok());
            }
            utf8.finish().unwrap();
            assert_eq!(output, input);
        }
    }

    #[test]
    fn incomplete_or_invalid_text_is_not_silently_replaced() {
        let mut utf8 = Utf8::default();
        utf8.write(&[0xe2], &mut Vec::new()).unwrap();
        assert_eq!(
            utf8.finish().unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            utf8.write(b"x", &mut Vec::new()).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert!(utf8.write(b"more", &mut Vec::new()).is_err());
    }
}
