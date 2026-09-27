//! Fallible text collection with GNU's NUL-delimited string extraction.

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Error {
    Allocation,
}

#[derive(Default)]
pub(super) struct TextSamples {
    bytes: Vec<u8>,
    count: usize,
}

impl TextSamples {
    pub(super) fn push(&mut self, value: &[u8]) -> Result<(), Error> {
        let count = self.count.checked_add(1).ok_or(Error::Allocation)?;
        let additional = value.len().checked_add(1).ok_or(Error::Allocation)?;
        self.bytes
            .try_reserve(additional)
            .map_err(|_| Error::Allocation)?;
        self.bytes.extend_from_slice(value);
        self.bytes.push(0);
        self.count = count;
        Ok(())
    }

    pub(super) fn clear(&mut self) {
        self.bytes.clear();
        self.count = 0;
    }

    fn sorted(&self, ignore_case: bool) -> Result<Vec<&[u8]>, Error> {
        let mut values = Vec::new();
        values
            .try_reserve_exact(self.count)
            .map_err(|_| Error::Allocation)?;
        // GNU stops after count strings, even if embedded NULs split earlier fields.
        values.extend(self.bytes.split(|&byte| byte == 0).take(self.count));
        if ignore_case {
            // All slices borrow one unchanged buffer. Their addresses encode
            // input order, including empty strings, without an extra ordinal.
            values.sort_unstable_by(|a, b| {
                super::text_order::compare(a, b, true).then(a.as_ptr().cmp(&b.as_ptr()))
            });
        } else {
            values.sort_unstable();
        }
        Ok(values)
    }

    pub(super) fn count_unique(&self, ignore_case: bool) -> Result<usize, Error> {
        let values = self.sorted(ignore_case)?;
        Ok(usize::from(!values.is_empty())
            + values
                .windows(2)
                .filter(|pair| !super::text_order::compare(pair[0], pair[1], ignore_case).is_eq())
                .count())
    }
    pub(super) fn unique(&self, delimiter: u8, ignore_case: bool) -> Result<Vec<u8>, Error> {
        let mut values = self.sorted(ignore_case)?;
        values.dedup_by(|a, b| super::text_order::compare(a, b, ignore_case).is_eq());
        // Each selected string has a retained NUL to cover its list separator.
        let length =
            values.iter().map(|value| value.len()).sum::<usize>() + values.len().saturating_sub(1);
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        for (index, value) in values.into_iter().enumerate() {
            if index != 0 {
                output.push(delimiter);
            }
            output.extend_from_slice(value);
        }
        Ok(output)
    }

    pub(super) fn collapse(&self, delimiter: u8) -> Result<Vec<u8>, Error> {
        let length = self.bytes.len().saturating_sub(1);
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| Error::Allocation)?;
        output.extend_from_slice(&self.bytes[..length]);
        for byte in &mut output {
            if *byte == 0 {
                *byte = delimiter;
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_orders_bytes_without_escaping_or_changing_collection() {
        let mut samples = TextSamples::default();
        assert_eq!(samples.unique(b'|', false).unwrap(), b"");
        for value in [b"b,c".as_slice(), b"a", b"", b"A", b"\xff", b"a"] {
            samples.push(value).unwrap();
        }
        assert_eq!(samples.unique(b',', false).unwrap(), b",A,a,b,c,\xff");
        assert_eq!(samples.unique(b'|', false).unwrap(), b"|A|a|b,c|\xff");
        assert_eq!(samples.count_unique(false), Ok(5));
        samples.clear();
        samples.push(b"z").unwrap();
        assert_eq!(samples.unique(b'\xff', false).unwrap(), b"z");
    }

    #[test]
    fn unique_preserves_gnu_nul_displacement() {
        let mut samples = TextSamples::default();
        for value in [b"b\0\0a".as_slice(), b"z", b"y"] {
            samples.push(value).unwrap();
        }
        assert_eq!(samples.unique(b':', false).unwrap(), b":a:b");
    }

    #[test]
    fn collapse_preserves_order_duplicates_and_the_complete_retained_buffer() {
        let mut samples = TextSamples::default();
        assert_eq!(samples.collapse(b'|').unwrap(), b"");
        for value in [b"b\0\0a".as_slice(), b"z", b"z", b"y\0"] {
            samples.push(value).unwrap();
        }
        assert_eq!(samples.collapse(b':').unwrap(), b"b::a:z:z:y:");
        assert_eq!(
            samples.collapse(b'\xff').unwrap(),
            b"b\xff\xffa\xffz\xffz\xffy\xff"
        );
        assert_eq!(samples.collapse(b':').unwrap(), b"b::a:z:z:y:");
    }

    #[test]
    fn identity_is_raw_text_and_finalizing_preserves_collection() {
        let mut samples = TextSamples::default();
        assert_eq!(samples.count_unique(false), Ok(0));
        for value in [
            b"1".as_slice(),
            b"01",
            b"1.0",
            b"a",
            b"A",
            b"\xff",
            b"",
            b"a",
        ] {
            samples.push(value).unwrap();
        }
        assert_eq!(samples.count_unique(false), Ok(7));
        assert_eq!(samples.count_unique(false), Ok(7));
        let capacity = samples.bytes.capacity();
        samples.clear();
        assert_eq!(samples.count_unique(false), Ok(0));
        assert_eq!(samples.bytes.capacity(), capacity);
        samples.push(b"z").unwrap();
        assert_eq!(samples.count_unique(false), Ok(1));
    }

    #[test]
    fn embedded_nuls_displace_later_fields_like_gnu() {
        let mut samples = TextSamples::default();
        samples.push(b"a\0a").unwrap();
        samples.push(b"b").unwrap();
        assert_eq!(samples.count_unique(false), Ok(1));
        samples.clear();
        for value in [b"a\0\0b".as_slice(), b"c", b"d"] {
            samples.push(value).unwrap();
        }
        assert_eq!(samples.count_unique(false), Ok(3));
    }
}
