//! Invocation-selected printf layouts over canonical binary80 values.
use crate::profile::{Error, Profile};
use fastmash_numeric_contract::{Value80, ValueClass};
use num_bigint::BigUint;

#[derive(Clone, Debug)]
pub struct Spec {
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    left: bool,
    plus: bool,
    space: bool,
    alternate: bool,
    zero: bool,
    group: bool,
    width: usize,
    precision: Option<usize>,
    kind: u8,
}
#[derive(Debug, PartialEq, Eq)]
pub enum Invalid {
    TooLong,
    NoDirective,
    MissingType,
    Type(u8),
    Multiple,
    Capacity,
}

fn literal(input: &[u8], at: &mut usize) -> Vec<u8> {
    let mut out = Vec::new();
    while let Some(&b) = input.get(*at) {
        if b == b'%' {
            if input.get(*at + 1) != Some(&b'%') {
                break;
            }
            *at += 1;
        }
        out.push(b);
        *at += 1;
    }
    out
}
fn number(input: &[u8], at: &mut usize) -> Result<usize, Invalid> {
    let mut n = 0usize;
    while let Some(&b) = input.get(*at).filter(|b| b.is_ascii_digit()) {
        n = n
            .checked_mul(10)
            .and_then(|n| n.checked_add((b - b'0') as usize))
            .ok_or(Invalid::Capacity)?;
        *at += 1;
    }
    Ok(n)
}
impl Spec {
    pub fn parse(input: &[u8]) -> Result<Self, Invalid> {
        if input.len() > 99 {
            return Err(Invalid::TooLong);
        }
        let mut at = 0;
        let prefix = literal(input, &mut at);
        if at == input.len() {
            return Err(Invalid::NoDirective);
        }
        at += 1;
        let mut s = Self {
            prefix,
            suffix: Vec::new(),
            left: false,
            plus: false,
            space: false,
            alternate: false,
            zero: false,
            group: false,
            width: 0,
            precision: None,
            kind: b'g',
        };
        while let Some(&b) = input.get(at) {
            match b {
                b'-' => s.left = true,
                b'+' => s.plus = true,
                b' ' => s.space = true,
                b'#' => s.alternate = true,
                b'0' => s.zero = true,
                b'\'' => s.group = true,
                _ => break,
            }
            at += 1;
        }
        s.width = number(input, &mut at)?;
        if input.get(at) == Some(&b'.') {
            at += 1;
            s.precision = Some(number(input, &mut at)?);
        }
        s.kind = *input.get(at).ok_or(Invalid::MissingType)?;
        if !b"efgaEFGA".contains(&s.kind) {
            return Err(Invalid::Type(s.kind));
        }
        at += 1;
        s.suffix = literal(input, &mut at);
        if at != input.len() {
            return Err(Invalid::Multiple);
        }
        Ok(s)
    }
    pub fn render(&self, value: Value80, profile: Profile) -> Result<Vec<u8>, Error> {
        let upper = self.kind.is_ascii_uppercase();
        let special = matches!(
            value.classify(),
            ValueClass::Infinity | ValueClass::Nan { .. }
        );
        let mut body = Output::default();
        if special {
            body.add(if value.classify() == ValueClass::Infinity {
                if upper { b"INF" } else { b"inf" }
            } else if upper {
                b"NAN"
            } else {
                b"nan"
            })?;
        } else if self.kind.eq_ignore_ascii_case(&b'a') {
            self.hex(&mut body, value, profile)?;
        } else {
            self.decimal(&mut body, value, profile)?;
        }
        let sign = if value.sign_exponent() & 0x8000 != 0 {
            Some(b'-')
        } else if self.plus {
            Some(b'+')
        } else if self.space {
            Some(b' ')
        } else {
            None
        };
        // Width counts characters, as glibc does: a multibyte thousands
        // separator (U+202F, U+2019) is one character.
        let characters = body.0.iter().filter(|&&b| b & 0xc0 != 0x80).count();
        let length = characters
            .checked_add(usize::from(sign.is_some()))
            .ok_or(Error::Allocation)?;
        let pad = self.width.saturating_sub(length);
        let mut out = Output::default();
        out.add(&self.prefix)?;
        let zeros = self.zero && !self.left && !special;
        if !self.left && !zeros {
            out.repeat(b' ', pad)?;
        }
        if let Some(sign) = sign {
            out.add(&[sign])?;
        }
        let hex = zeros && self.kind.eq_ignore_ascii_case(&b'a');
        if hex {
            out.add(&body.0[..2])?;
        }
        if zeros {
            out.repeat(b'0', pad)?;
        }
        out.add(&body.0[if hex { 2 } else { 0 }..])?;
        if self.left {
            out.repeat(b' ', pad)?;
        }
        out.add(&self.suffix)?;
        Ok(out.0)
    }
    fn decimal(&self, out: &mut Output, value: Value80, profile: Profile) -> Result<(), Error> {
        let mut d = Decimal::exact(value);
        let kind = self.kind.to_ascii_lowercase();
        let p = self.precision.unwrap_or(6);
        let p = if kind == b'g' { p.max(1) } else { p };
        let keep = if kind == b'f' {
            (p as i128) + i128::from(d.point)
        } else {
            (p as i128) + i128::from(kind == b'e')
        };
        d.round(keep);
        let exponential =
            kind == b'e' || (kind == b'g' && (d.point <= -4 || i128::from(d.point) > p as i128));
        let mut fraction = if kind == b'g' {
            if exponential {
                p - 1
            } else {
                usize::try_from((p as i128) - i128::from(d.point)).map_err(|_| Error::Allocation)?
            }
        } else {
            p
        };
        if kind == b'g' && !self.alternate {
            let significant = d
                .digits
                .iter()
                .rposition(|b| *b != b'0')
                .map_or(1, |i| i + 1);
            let actual = if exponential {
                significant.saturating_sub(1)
            } else {
                usize::try_from((significant as i128) - i128::from(d.point)).unwrap_or(0)
            };
            fraction = fraction.min(actual);
        }
        let point = if exponential { 1 } else { d.point };
        let integer = point.max(1) as usize;
        for index in 0..integer {
            if self.group && !exponential && index > 0 && profile.separates(integer - index) {
                out.add(profile.thousands())?;
            }
            out.add(&[if point <= 0 { b'0' } else { d.at(index as i32) }])?;
        }
        if fraction > 0 || self.alternate {
            out.add(&[profile.radix()])?;
        }
        // Only the exact finite expansion is scanned; requested precision adds zeros.
        let leading = if point < 0 {
            fraction.min((-point) as usize)
        } else {
            0
        };
        out.repeat(b'0', leading)?;
        let start = point.max(0) as usize;
        let available = d.digits.len().saturating_sub(start).min(fraction - leading);
        if available > 0 {
            out.add(&d.digits[start..start + available])?;
        }
        out.repeat(b'0', fraction - leading - available)?;
        if exponential {
            out.add(if self.kind.is_ascii_uppercase() {
                b"E"
            } else {
                b"e"
            })?;
            out.exponent(d.point - 1, 2)?;
        }
        Ok(())
    }
    fn hex(&self, out: &mut Output, value: Value80, profile: Profile) -> Result<(), Error> {
        let upper = self.kind.is_ascii_uppercase();
        out.add(if upper { b"0X" } else { b"0x" })?;
        let mut m = u128::from(value.significand());
        let mut exponent = if m == 0 {
            0
        } else if value.exponent() == 0 {
            -16385
        } else {
            i32::from(value.exponent()) - 16386
        };
        if let Some(p) = self.precision.filter(|p| *p < 15) {
            let shift = 4 * (15 - p);
            let quotient = m >> shift;
            let remainder = m & ((1u128 << shift) - 1);
            let half = 1u128 << (shift - 1);
            m = (quotient
                + u128::from(remainder > half || (remainder == half && quotient & 1 != 0)))
                << shift;
            if m >> 64 != 0 {
                m >>= 4;
                exponent += 4;
            }
        }
        let alphabet = if upper {
            b"0123456789ABCDEF"
        } else {
            b"0123456789abcdef"
        };
        let mut digits = [b'0'; 16];
        for (i, b) in digits.iter_mut().enumerate() {
            *b = alphabet[((m >> (4 * (15 - i))) & 15) as usize];
        }
        let p = self
            .precision
            .unwrap_or_else(|| digits.iter().rposition(|b| *b != b'0').unwrap_or(0));
        out.add(&digits[..1])?;
        if p > 0 || self.alternate {
            out.add(&[profile.radix()])?;
        }
        out.add(&digits[1..1 + p.min(15)])?;
        out.repeat(b'0', p.saturating_sub(15))?;
        out.add(if upper { b"P" } else { b"p" })?;
        out.exponent(exponent, 1)
    }
}

#[derive(Default)]
struct Output(Vec<u8>);
impl Output {
    fn add(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.0
            .try_reserve(bytes.len())
            .map_err(|_| Error::Allocation)?;
        self.0.extend_from_slice(bytes);
        Ok(())
    }
    fn repeat(&mut self, b: u8, n: usize) -> Result<(), Error> {
        let length = self.0.len().checked_add(n).ok_or(Error::Allocation)?;
        self.0.try_reserve(n).map_err(|_| Error::Allocation)?;
        self.0.resize(length, b);
        Ok(())
    }
    fn exponent(&mut self, e: i32, min: usize) -> Result<(), Error> {
        self.add(if e < 0 { b"-" } else { b"+" })?;
        let digits = e.unsigned_abs().to_string();
        self.repeat(b'0', min.saturating_sub(digits.len()))?;
        self.add(digits.as_bytes())
    }
}
struct Decimal {
    digits: Vec<u8>,
    point: i32,
}

impl Decimal {
    fn exact(value: Value80) -> Self {
        if value.significand() == 0 {
            return Self {
                digits: vec![b'0'],
                point: 1,
            };
        }
        let q = if value.exponent() == 0 {
            -16445
        } else {
            i32::from(value.exponent()) - 16446
        };
        let m = BigUint::from(value.significand());
        let (coefficient, scale) = if q >= 0 {
            (m << (q as usize), 0)
        } else {
            (m * BigUint::from(5u8).pow((-q) as u32), -q)
        };
        let digits = coefficient.to_str_radix(10).into_bytes();
        let point = digits.len() as i32 - scale;
        Self { digits, point }
    }
    fn at(&self, i: i32) -> u8 {
        if i < 0 {
            b'0'
        } else {
            self.digits.get(i as usize).copied().unwrap_or(b'0')
        }
    }
    fn round(&mut self, keep: i128) {
        if keep >= self.digits.len() as i128 {
            return;
        }
        let up = keep >= 0 && {
            let k = keep as usize;
            self.digits[k] > b'5'
                || (self.digits[k] == b'5'
                    && (self.digits[k + 1..].iter().any(|b| *b != b'0')
                        || (k > 0 && self.digits[k - 1] % 2 == 1)))
        };
        if keep <= 0 {
            if up {
                self.digits = vec![b'1'];
                self.point += 1;
            } else {
                self.digits = vec![b'0'];
                self.point = 1;
            }
            return;
        }
        self.digits.truncate(keep as usize);
        if up {
            for b in self.digits.iter_mut().rev() {
                if *b != b'9' {
                    *b += 1;
                    return;
                }
                *b = b'0';
            }
            self.digits[0] = b'1';
            self.point += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastmash_numeric_contract::Raw80;

    #[test]
    fn allocation_refusal_and_unchanged_bits() {
        let value = Value80::from_raw(Raw80::new(0x3fff, 0xa000_0000_0000_0000)).unwrap();
        let before = value.raw();
        for text in [
            format!("%{}.2f", usize::MAX),
            format!("%.{}f", usize::MAX),
            format!("%.{}a", usize::MAX),
        ] {
            assert_eq!(
                Spec::parse(text.as_bytes())
                    .unwrap()
                    .render(value, Profile::C),
                Err(Error::Allocation)
            );
        }
        assert_eq!(value.raw(), before);
        assert_eq!(
            Spec::parse(b"%.2f")
                .unwrap()
                .render(value, Profile::C)
                .unwrap(),
            b"1.25"
        );
    }

    /// Expected text is glibc 2.43 `printf("%'.2f")` in the named locales
    /// (compiled with localedef); `%'22.2f` pads the same text to 22
    /// characters.
    #[test]
    fn grouping_follows_glibc_locale_rules() {
        // Positive normal binary64 values widen exactly to binary80.
        let widen = |value: f64| {
            let bits = value.to_bits();
            let exponent = ((bits >> 52) & 0x7ff) as u16 - 1023 + 16383;
            let significand = (1 << 63) | ((bits & ((1 << 52) - 1)) << 11);
            Value80::from_raw(Raw80::new(exponent, significand)).unwrap()
        };
        let render = |format: &[u8], value: f64, profile| {
            let text = Spec::parse(format)
                .unwrap()
                .render(widen(value), profile)
                .unwrap();
            String::from_utf8(text).unwrap()
        };
        let nnbsp = "\u{202f}";
        let unm_large = ["12", "345", "678", "90", "12", "34.50"].join(nnbsp);
        let unm_small = ["1", "23", "45", "67.25"].join(nnbsp);
        for (locale, profile, large, small) in [
            (
                "en_US",
                Profile::EN_NUMERIC,
                "12,345,678,901,234.50",
                "1,234,567.25",
            ),
            // 3;2
            (
                "as_IN",
                Profile::new(b'.', b",", b"\x03\x02"),
                "1,23,45,67,89,01,234.50",
                "12,34,567.25",
            ),
            // 4
            (
                "cmn_TW",
                Profile::new(b'.', b",", b"\x04"),
                "12,3456,7890,1234.50",
                "123,4567.25",
            ),
            // 3;3 is the same as 3.
            (
                "eo_US",
                Profile::new(b'.', b",", b"\x03\x03"),
                "12,345,678,901,234.50",
                "1,234,567.25",
            ),
            // CHAR_MAX: no grouping, even with a separator.
            (
                "el_GR",
                Profile::new(b',', b".", b"\x7f"),
                "12345678901234,50",
                "1234567,25",
            ),
            // An empty separator: no grouping.
            (
                "bg_BG",
                Profile::new(b',', b"", b"\x03"),
                "12345678901234,50",
                "1234567,25",
            ),
            // 2;2;2;3 with U+202F, which pads as one character.
            (
                "unm_US",
                Profile::new(b'.', nnbsp.as_bytes(), b"\x02\x02\x02\x03"),
                &unm_large,
                &unm_small,
            ),
        ] {
            assert_eq!(
                render(b"%'.2f", 12345678901234.5, profile),
                large,
                "{locale}"
            );
            assert_eq!(
                render(b"%'22.2f", 1234567.25, profile),
                format!("{small:>22}"),
                "{locale}"
            );
        }
    }
}
