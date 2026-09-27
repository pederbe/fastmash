//! Fallible reusable storage for one selected text value.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Allocation,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WriteError<E> {
    Allocation,
    Transform(E),
    InvalidLength,
}

#[derive(Default)]
pub(super) struct ScalarText {
    bytes: Vec<u8>,
    present: bool,
    #[cfg(test)]
    fail_next_growth: bool,
}

impl ScalarText {
    /// Publish only a complete transformation; failed growth preserves the old value.
    pub(super) fn write_with<E>(
        &mut self,
        length: usize,
        write: impl FnOnce(&mut [u8]) -> Result<usize, E>,
    ) -> Result<(), WriteError<E>> {
        if length > self.bytes.capacity() {
            #[cfg(test)]
            if std::mem::take(&mut self.fail_next_growth) {
                return Err(WriteError::Allocation);
            }
            self.bytes
                .try_reserve_exact(length - self.bytes.len())
                .map_err(|_| WriteError::Allocation)?;
        }
        self.present = false;
        self.bytes.resize(length, 0);
        let written = write(&mut self.bytes).map_err(WriteError::Transform)?;
        if written > length {
            return Err(WriteError::InvalidLength);
        }
        self.bytes.truncate(written);
        self.present = true;
        Ok(())
    }

    pub(super) fn is_present(&self) -> bool {
        self.present
    }

    pub(super) fn replace(&mut self, value: &[u8]) -> Result<(), Error> {
        if value.len() > self.bytes.capacity() {
            #[cfg(test)]
            if std::mem::take(&mut self.fail_next_growth) {
                return Err(Error::Allocation);
            }
            self.bytes
                .try_reserve_exact(value.len() - self.bytes.len())
                .map_err(|_| Error::Allocation)?;
        }
        self.bytes.clear();
        self.bytes.extend_from_slice(value);
        self.present = true;
        Ok(())
    }

    pub(super) fn output(&self) -> Option<&[u8]> {
        self.present.then(|| {
            let end = memchr::memchr(0, &self.bytes).unwrap_or(self.bytes.len());
            &self.bytes[..end]
        })
    }

    pub(super) fn clear(&mut self) {
        self.bytes.clear();
        self.present = false;
    }

    #[cfg(test)]
    pub(super) fn fail_next_growth(&mut self) {
        self.fail_next_growth = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_and_oversized_writes_are_not_published() {
        let mut text = ScalarText::default();
        text.replace(b"old").unwrap();
        assert_eq!(
            text.write_with(3, |out| {
                out[0] = b'x';
                Err(())
            }),
            Err(WriteError::Transform(()))
        );
        assert_eq!(text.output(), None);
        assert_eq!(
            text.write_with(3, |_| Ok::<_, ()>(4)),
            Err(WriteError::InvalidLength)
        );
        assert_eq!(text.output(), None);
        text.write_with(3, |out| {
            out[0] = b'f';
            Ok::<_, ()>(1)
        })
        .unwrap();
        assert_eq!(text.output(), Some(b"f".as_slice()));
        text.clear();
        text.fail_next_growth();
        text.write_with(3, |_| Ok::<_, ()>(0)).unwrap();
        assert_eq!(text.output(), Some(b"".as_slice()));
    }

    #[test]
    fn empty_and_nul_truncated_values_remain_present() {
        let mut text = ScalarText::default();
        assert_eq!(text.output(), None);
        text.replace(b"").unwrap();
        assert_eq!(text.output(), Some(b"".as_slice()));
        text.replace(b"a\0tail").unwrap();
        assert_eq!(text.output(), Some(b"a".as_slice()));
        text.clear();
        assert_eq!(text.output(), None);
    }

    #[test]
    fn failed_growth_is_transactional_and_reset_retains_capacity() {
        let mut text = ScalarText::default();
        text.replace(b"old").unwrap();
        let capacity = text.bytes.capacity();
        text.fail_next_growth();
        assert_eq!(
            text.replace(&vec![b'x'; capacity + 1]),
            Err(Error::Allocation)
        );
        assert_eq!(text.output(), Some(b"old".as_slice()));
        text.clear();
        assert!(text.bytes.capacity() >= capacity);
        assert_eq!(text.output(), None);
    }
}
