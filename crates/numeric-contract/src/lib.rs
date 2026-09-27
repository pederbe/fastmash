//! Checked binary80 value storage shared by the Fastmash numerical crates.
//!
//! Values preserve encoding identity, including signed zero and NaN payloads.
//! This crate performs no floating-point evaluation or numeric comparison.

#![no_std]
#![forbid(unsafe_code)]

/// A failed fixture-text or canonical-encoding import.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingError {
    HighBits,
    HexLength { expected: usize, actual: usize },
    HexDigit { index: usize, byte: u8 },
    PseudoDenormal,
    MissingIntegerBit,
}

/// Meaningful binary80 bits, including noncanonical diagnostic encodings.
///
/// The fields do not specify Rust layout or a native ABI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Raw80 {
    sign_exponent: u16,
    significand: u64,
}

impl Raw80 {
    pub const fn new(sign_exponent: u16, significand: u64) -> Self {
        Self {
            sign_exponent,
            significand,
        }
    }

    pub const fn sign_exponent(self) -> u16 {
        self.sign_exponent
    }

    pub const fn significand(self) -> u64 {
        self.significand
    }

    pub const fn exponent(self) -> u16 {
        self.sign_exponent & 0x7fff
    }

    pub const fn is_negative(self) -> bool {
        self.sign_exponent & 0x8000 != 0
    }

    pub const fn try_from_bits(bits: u128) -> Result<Self, EncodingError> {
        if bits >> 80 != 0 {
            return Err(EncodingError::HighBits);
        }
        Ok(Self::new((bits >> 64) as u16, bits as u64))
    }

    pub const fn to_bits(self) -> u128 {
        ((self.sign_exponent as u128) << 64) | self.significand as u128
    }

    pub const fn from_le_bytes(bytes: [u8; 10]) -> Self {
        Self::new(
            u16::from_le_bytes([bytes[8], bytes[9]]),
            u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
        )
    }

    pub const fn to_le_bytes(self) -> [u8; 10] {
        let fraction = self.significand.to_le_bytes();
        let se = self.sign_exponent.to_le_bytes();
        [
            fraction[0],
            fraction[1],
            fraction[2],
            fraction[3],
            fraction[4],
            fraction[5],
            fraction[6],
            fraction[7],
            se[0],
            se[1],
        ]
    }

    pub fn from_hex(bytes: &[u8]) -> Result<Self, EncodingError> {
        Self::try_from_bits(parse_hex(bytes, 20)?)
    }

    pub fn to_hex(self) -> [u8; 20] {
        hex_bytes(self.to_bits())
    }

    pub const fn same_bits(self, other: Self) -> bool {
        self.sign_exponent == other.sign_exponent && self.significand == other.significand
    }
}

/// A canonical binary80 value. Import never quiets or normalizes it.
///
/// Use [`Self::same_bits`] for encoding identity. Numeric equality and ordering
/// belong to the operation's semantic profile and are not implemented here.
///
/// ```compile_fail
/// use fastmash_numeric_contract::{Raw80, Value80};
/// let bypass = Value80(Raw80::new(0, 1 << 63));
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Value80(Raw80);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueClass {
    Zero,
    Subnormal,
    Normal,
    Infinity,
    Nan { quiet: bool, payload: u64 },
}

impl Value80 {
    /// Canonical signed zero used by checked numerical implementations.
    pub const fn signed_zero(negative: bool) -> Self {
        Self(Raw80::new(if negative { 0x8000 } else { 0 }, 0))
    }

    /// Canonical signed infinity used by checked numerical implementations.
    pub const fn infinity(negative: bool) -> Self {
        Self(Raw80::new(if negative { 0xffff } else { 0x7fff }, 1 << 63))
    }

    /// The profile-independent positive canonical quiet NaN encoding.
    pub const fn canonical_quiet_nan() -> Self {
        Self(Raw80::new(0x7fff, 0xc000_0000_0000_0000))
    }

    /// Preserve a canonical NaN's sign and payload while setting its quiet bit.
    pub const fn quiet_nan(self) -> Self {
        Self(Raw80::new(
            self.sign_exponent(),
            self.significand() | 0x4000_0000_0000_0000,
        ))
    }

    /// Encode an exact `u64` as canonical binary80 without floating arithmetic.
    pub const fn exact_u64(value: u64) -> Self {
        if value == 0 {
            return Self::signed_zero(false);
        }
        let bit = 63 - value.leading_zeros();
        Self(Raw80::new(16_383 + bit as u16, value << (63 - bit)))
    }

    pub const fn from_raw(raw: Raw80) -> Result<Self, EncodingError> {
        let integer_bit = raw.significand >> 63 != 0;
        if raw.exponent() == 0 {
            if integer_bit {
                return Err(EncodingError::PseudoDenormal);
            }
        } else if !integer_bit {
            return Err(EncodingError::MissingIntegerBit);
        }
        Ok(Self(raw))
    }

    pub const fn try_from_bits(bits: u128) -> Result<Self, EncodingError> {
        match Raw80::try_from_bits(bits) {
            Ok(raw) => Self::from_raw(raw),
            Err(error) => Err(error),
        }
    }

    pub const fn from_le_bytes(bytes: [u8; 10]) -> Result<Self, EncodingError> {
        Self::from_raw(Raw80::from_le_bytes(bytes))
    }

    pub fn from_hex(bytes: &[u8]) -> Result<Self, EncodingError> {
        Self::from_raw(Raw80::from_hex(bytes)?)
    }

    pub const fn raw(self) -> Raw80 {
        self.0
    }

    pub const fn sign_exponent(self) -> u16 {
        self.0.sign_exponent()
    }

    pub const fn exponent(self) -> u16 {
        self.0.exponent()
    }

    pub const fn significand(self) -> u64 {
        self.0.significand()
    }

    pub const fn is_negative(self) -> bool {
        self.0.is_negative()
    }

    /// Change only the sign bit of an already canonical value.
    pub const fn with_sign(self, negative: bool) -> Self {
        Self(Raw80::new(
            (self.sign_exponent() & 0x7fff) | if negative { 0x8000 } else { 0 },
            self.significand(),
        ))
    }

    /// Construct a canonical normal value from normalized fields.
    ///
    /// The exponent is bounded to the normal range and the explicit integer
    /// bit is set, so every input produces a canonical normal encoding.
    pub const fn canonical_normal(negative: bool, exponent: u16, significand: u64) -> Self {
        let exponent = if exponent == 0 {
            1
        } else if exponent >= 0x7fff {
            0x7ffe
        } else {
            exponent
        };
        Self(Raw80::new(
            exponent | if negative { 0x8000 } else { 0 },
            significand | (1 << 63),
        ))
    }

    pub const fn to_bits(self) -> u128 {
        self.0.to_bits()
    }

    pub const fn to_le_bytes(self) -> [u8; 10] {
        self.0.to_le_bytes()
    }

    pub fn to_hex(self) -> [u8; 20] {
        self.0.to_hex()
    }

    pub const fn same_bits(self, other: Self) -> bool {
        self.0.same_bits(other.0)
    }

    pub const fn classify(self) -> ValueClass {
        match self.exponent() {
            0 if self.significand() == 0 => ValueClass::Zero,
            0 => ValueClass::Subnormal,
            0x7fff => {
                let fraction = self.significand() & 0x7fff_ffff_ffff_ffff;
                if fraction == 0 {
                    ValueClass::Infinity
                } else {
                    ValueClass::Nan {
                        quiet: fraction & 0x4000_0000_0000_0000 != 0,
                        payload: fraction & 0x3fff_ffff_ffff_ffff,
                    }
                }
            }
            _ => ValueClass::Normal,
        }
    }
}

impl TryFrom<Raw80> for Value80 {
    type Error = EncodingError;

    fn try_from(raw: Raw80) -> Result<Self, Self::Error> {
        Self::from_raw(raw)
    }
}

/// Exact source bits. Nonfinite admission is a consumer policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binary64Bits(u64);

impl Binary64Bits {
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    pub const fn bits(self) -> u64 {
        self.0
    }

    pub fn from_hex(bytes: &[u8]) -> Result<Self, EncodingError> {
        Ok(Self(parse_hex(bytes, 16)? as u64))
    }

    pub fn to_hex(self) -> [u8; 16] {
        hex_bytes(self.0 as u128)
    }

    pub const fn same_bits(self, other: Self) -> bool {
        self.0 == other.0
    }
}

/// The source's declared width and signedness, before numerical conversion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerOperand {
    I32(i32),
    U32(u32),
    I64(i64),
    U64(u64),
}

/// Evidence origin; a declaration or prediction is not an observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BasisKind {
    Observed,
    ExactDerivation,
    SourcePrediction,
    DeclaredConfiguration,
}

/// A fact with consumer-owned basis and distinct typed absence reasons.
///
/// The consumer's basis includes its subject, stage and source reference.
/// This enum supplies no default and does not decide comparison eligibility.
#[derive(Clone, Copy, Debug)]
pub enum Fact<T, B, U, N> {
    Known { value: T, basis: B },
    Unavailable(U),
    NotApplicable(N),
}

fn parse_hex(bytes: &[u8], expected: usize) -> Result<u128, EncodingError> {
    if bytes.len() != expected {
        return Err(EncodingError::HexLength {
            expected,
            actual: bytes.len(),
        });
    }
    let mut bits = 0_u128;
    for (index, &byte) in bytes.iter().enumerate() {
        let digit = match byte {
            b'0'..=b'9' => byte - b'0',
            b'a'..=b'f' => byte - b'a' + 10,
            b'A'..=b'F' => byte - b'A' + 10,
            _ => return Err(EncodingError::HexDigit { index, byte }),
        };
        bits = (bits << 4) | u128::from(digit);
    }
    Ok(bits)
}

fn hex_bytes<const N: usize>(mut bits: u128) -> [u8; N] {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = [0; N];
    for byte in output.iter_mut().rev() {
        *byte = DIGITS[(bits & 15) as usize];
        bits >>= 4;
    }
    output
}
