//! Number rules of locales and conversion limits. These do not inspect the host locale.

use fastmash_numeric_contract::EncodingError;

pub const INPUT_CAP: usize = 8192;
pub const COEFFICIENT_BITS: u64 = 32768;
pub const INTERMEDIATE_BITS: u64 = 131072;
pub const POWER_CAP: u32 = 20000;
pub const SHIFT_CAP: u32 = 65536;
pub const OUTPUT_CAP: usize = 16384;
pub const EXPONENT_CAP: u32 = 1_000_000;

/// Number rules of a locale: the decimal point, and the thousands separator
/// and grouping used by the `'` format flag. These are data compiled into the
/// program, never read from the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    decimal: u8,
    thousands: &'static [u8],
    grouping: &'static [u8],
}

impl Profile {
    /// The C and POSIX locales: a decimal point and no grouping.
    pub const C: Self = Self::new(b'.', b"", b"");
    /// en_US rules, also the numeric reference of the en admission evidence.
    pub const EN_NUMERIC: Self = Self::new(b'.', b",", b"\x03");
    /// de_DE rules, also the numeric reference of the de admission evidence.
    pub const DE_NUMERIC: Self = Self::new(b',', b".", b"\x03");

    /// `grouping` uses C's `localeconv` encoding: group sizes from the decimal
    /// point outward, the last repeating, and CHAR_MAX (127) or a size of 0
    /// ending grouping.
    pub const fn new(decimal: u8, thousands: &'static [u8], grouping: &'static [u8]) -> Self {
        Self {
            decimal,
            thousands,
            grouping,
        }
    }

    /// Stable identities of the admission evidence profiles.
    pub fn from_name(name: &str) -> Result<Self, Error> {
        match name {
            "C" => Ok(Self::C),
            "de-numeric" => Ok(Self::DE_NUMERIC),
            _ => Err(Error::UnsupportedProfile),
        }
    }

    pub const fn radix(self) -> u8 {
        self.decimal
    }

    pub const fn erange(self) -> i32 {
        34
    }

    pub const fn grouping(self) -> &'static [u8] {
        self.grouping
    }

    pub const fn thousands(self) -> &'static [u8] {
        self.thousands
    }

    /// Whether a separator precedes the integer digit that has `right` digits
    /// after it (1 for the last-but-one digit), by glibc's rules: sizes count
    /// from the decimal point outward, a 0 repeats the previous size, as does
    /// the end of the list, and CHAR_MAX (or more) ends grouping. There is no
    /// grouping without a separator.
    pub fn separates(self, right: usize) -> bool {
        if self.thousands.is_empty() || right == 0 {
            return false;
        }
        let mut boundary = 0usize;
        let mut size = 0usize;
        for &next in self.grouping {
            if next == 0 {
                break;
            }
            if next >= 127 {
                return false;
            }
            size = usize::from(next);
            boundary += size;
            if boundary >= right {
                return boundary == right;
            }
        }
        size != 0 && (right - boundary).is_multiple_of(size)
    }
}

#[cfg(test)]
mod tests {
    use super::Profile;

    fn marks(profile: Profile, digits: usize) -> String {
        (1..digits)
            .rev()
            .map(|right| if profile.separates(right) { ',' } else { '-' })
            .collect()
    }

    #[test]
    fn grouping_follows_the_c_library_rules() {
        // Marks for the gaps of a 10-digit integer, from the left.
        let group = |grouping| marks(Profile::new(b'.', b",", grouping), 10);
        assert_eq!(group(b"\x03"), ",--,--,--");
        assert_eq!(group(b"\x03\x02"), ",-,-,-,--");
        // A 0 repeats the previous size, like the end of the list.
        assert_eq!(group(b"\x03\x00"), group(b"\x03"));
        assert_eq!(group(b"\x03\x02\x00"), group(b"\x03\x02"));
        // CHAR_MAX ends grouping; a leading 0 or CHAR_MAX means none.
        assert_eq!(group(b"\x03\x7f"), "------,--");
        assert_eq!(group(b"\x00"), "---------");
        assert_eq!(group(b"\x7f"), "---------");
        assert_eq!(marks(Profile::new(b'.', b"", b"\x03"), 10), "---------");
    }

    #[test]
    fn grouping_a_long_integer_is_linear() {
        // 5,000 digits, as a binary80 maximum printed with %'Lf.
        let profile = Profile::new(b'.', b",", b"\x02\x02\x02\x03");
        assert_eq!(marks(profile, 5_000).matches(',').count(), 1_667);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    UnsupportedProfile,
    UnsupportedFormat,
    InputCapacity,
    OutputCapacity,
    CoefficientCapacity,
    IntermediateCapacity,
    PowerCapacity,
    ShiftCapacity,
    InvalidView,
    Allocation,
    InternalInvariant,
    Encoding(EncodingError),
}

impl From<EncodingError> for Error {
    fn from(error: EncodingError) -> Self {
        Self::Encoding(error)
    }
}

pub fn copy_bytes(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(bytes.len())
        .map_err(|_| Error::Allocation)?;
    result.extend_from_slice(bytes);
    Ok(result)
}
