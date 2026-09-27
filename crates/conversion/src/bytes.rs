//! Byte-prefix grammar; field acceptance and NA filtering live in callers.

use std::ops::Range;

use crate::profile::{EXPONENT_CAP, Error, INPUT_CAP, Profile};

pub fn space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

pub fn digit(byte: u8, radix: u8) -> Option<u8> {
    let value = match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'f' => byte - b'a' + 10,
        b'A'..=b'F' => byte - b'A' + 10,
        _ => return None,
    };
    (value < radix).then_some(value)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminator {
    EmbeddedNul,
    SliceEnd,
    AppendedNul,
}

#[derive(Clone, Copy, Debug)]
pub struct ParseView<'a> {
    pub bytes: &'a [u8],
    pub record_id: &'a str,
    pub generation: u32,
    pub origin_start: usize,
    pub phase: &'static str,
    pub appended_nul: bool,
}

impl<'a> ParseView<'a> {
    pub fn effective(self) -> Result<&'a [u8], Error> {
        if self.bytes.len() > INPUT_CAP {
            return Err(Error::InputCapacity);
        }
        self.origin_start
            .checked_add(self.bytes.len())
            .ok_or(Error::InvalidView)?;
        Ok(&self.bytes[..self.nul_offset()])
    }

    pub fn nul_offset(self) -> usize {
        self.bytes
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.bytes.len())
    }

    pub fn terminator(self) -> Terminator {
        if self.nul_offset() != self.bytes.len() {
            Terminator::EmbeddedNul
        } else if self.appended_nul {
            Terminator::AppendedNul
        } else {
            Terminator::SliceEnd
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exponent {
    Finite(i32),
    Greater { negative: bool },
}

#[derive(Clone, Debug)]
pub struct FiniteToken<'a> {
    // Backing storage may extend past the token or a NUL. Only the validated
    // ranges below describe numeric input; backing bytes are not a token API.
    pub(crate) bytes: &'a [u8],
    pub(crate) negative: bool,
    pub(crate) radix: u8,
    pub(crate) integer: Range<usize>,
    pub(crate) fraction: Range<usize>,
    pub(crate) radix_offset: Option<usize>,
    pub(crate) exponent: Exponent,
    pub(crate) exponent_digits: Range<usize>,
}

impl FiniteToken<'_> {
    pub fn negative(&self) -> bool {
        self.negative
    }
    pub fn radix(&self) -> u8 {
        self.radix
    }
    pub fn exponent(&self) -> Exponent {
        self.exponent
    }
    pub fn fraction_digits(&self) -> usize {
        self.fraction.len()
    }
    pub fn digit_count(&self) -> usize {
        self.integer.len() + self.fraction.len()
    }

    pub fn digits(&self) -> impl Iterator<Item = u8> + '_ {
        self.bytes[self.integer.clone()]
            .iter()
            .chain(self.bytes[self.fraction.clone()].iter())
            .map(|&b| digit(b, self.radix).expect("lexer validated coefficient digit"))
    }

    pub fn one_digit_tiny_decimal(&self) -> bool {
        self.radix == 10
            && self.radix_offset.is_none()
            && self.integer.len() == 1
            && self.digits().next().is_some_and(|d| d != 0)
            && !self.exponent_digits.is_empty()
            && matches!(self.exponent, Exponent::Finite(-4952..=-4932))
    }
}

#[derive(Clone, Debug)]
pub enum Lexeme<'a> {
    NoConversion,
    Finite(FiniteToken<'a>),
    Infinity {
        negative: bool,
    },
    Nan {
        negative: bool,
        complete_payload: bool,
    },
}

#[derive(Clone, Debug)]
pub struct LexResult<'a> {
    pub lexeme: Lexeme<'a>,
    pub consumed: usize,
    pub leading_space: Range<usize>,
    pub sign: Option<Range<usize>>,
}

fn word(bytes: &[u8], at: usize, expected: &[u8]) -> bool {
    bytes
        .get(at..at + expected.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(expected))
}

fn exponent(bytes: &[u8], mut at: usize, marker: u8) -> (usize, Exponent, Range<usize>) {
    let rollback = at;
    if bytes.get(at).map(u8::to_ascii_lowercase) != Some(marker) {
        return (at, Exponent::Finite(0), at..at);
    }
    at += 1;
    let negative = bytes.get(at) == Some(&b'-');
    if matches!(bytes.get(at), Some(b'+' | b'-')) {
        at += 1;
    }
    let start = at;
    let mut magnitude = 0_u32;
    let mut greater = false;
    while let Some(b'0'..=b'9') = bytes.get(at) {
        if !greater {
            match magnitude
                .checked_mul(10)
                .and_then(|m| m.checked_add(u32::from(bytes[at] - b'0')))
            {
                Some(m) if m <= EXPONENT_CAP => magnitude = m,
                _ => greater = true,
            }
        }
        at += 1;
    }
    if at == start {
        return (rollback, Exponent::Finite(0), rollback..rollback);
    }
    let exp = if greater {
        Exponent::Greater { negative }
    } else {
        Exponent::Finite(if negative {
            -(magnitude as i32)
        } else {
            magnitude as i32
        })
    };
    (at, exp, start..at)
}

fn finite<'a>(
    bytes: &'a [u8],
    start: usize,
    negative: bool,
    radix: u8,
    point: u8,
) -> Option<(FiniteToken<'a>, usize)> {
    let mut at = start;
    while bytes.get(at).is_some_and(|&b| digit(b, radix).is_some()) {
        at += 1;
    }
    let integer = start..at;
    let radix_offset = (bytes.get(at) == Some(&point)).then_some(at);
    if radix_offset.is_some() {
        at += 1;
    }
    let fraction_start = at;
    if radix_offset.is_some() {
        while bytes.get(at).is_some_and(|&b| digit(b, radix).is_some()) {
            at += 1;
        }
    }
    let fraction = fraction_start..at;
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    let (end, exponent, exponent_digits) =
        exponent(bytes, at, if radix == 16 { b'p' } else { b'e' });
    Some((
        FiniteToken {
            bytes,
            negative,
            radix,
            integer,
            fraction,
            radix_offset,
            exponent,
            exponent_digits,
        },
        end,
    ))
}

pub fn lex(view: ParseView<'_>, profile: Profile) -> Result<LexResult<'_>, Error> {
    let bytes = view.effective()?;
    lex_slice(bytes, profile)
}

/// Borrowed grammar for command input, without diagnostic evidence storage limits.
pub fn lex_slice(bytes: &[u8], profile: Profile) -> Result<LexResult<'_>, Error> {
    // NUL belongs to none of the grammar classes below, so parsing stops there
    // naturally without scanning the whole remaining record first.
    let mut at = 0;
    while bytes.get(at).is_some_and(|&b| space(b)) {
        at += 1;
    }
    let leading_space = 0..at;
    let negative = bytes.get(at) == Some(&b'-');
    let sign = if matches!(bytes.get(at), Some(b'+' | b'-')) {
        let range = at..at + 1;
        at += 1;
        Some(range)
    } else {
        None
    };
    let result = |lexeme, consumed| LexResult {
        lexeme,
        consumed,
        leading_space: leading_space.clone(),
        sign: sign.clone(),
    };
    if word(bytes, at, b"inf") {
        return Ok(result(
            Lexeme::Infinity { negative },
            at + if word(bytes, at, b"infinity") { 8 } else { 3 },
        ));
    }
    if word(bytes, at, b"nan") {
        let bare_end = at + 3;
        let mut end = bare_end;
        let mut complete_payload = false;
        if bytes.get(end) == Some(&b'(') {
            end += 1;
            while bytes
                .get(end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                end += 1;
            }
            complete_payload = bytes.get(end) == Some(&b')');
            end = if complete_payload { end + 1 } else { bare_end };
        }
        return Ok(result(
            Lexeme::Nan {
                negative,
                complete_payload,
            },
            end,
        ));
    }
    if bytes.get(at) == Some(&b'0')
        && matches!(bytes.get(at + 1), Some(b'x' | b'X'))
        && let Some((token, end)) = finite(bytes, at + 2, negative, 16, profile.radix())
    {
        return Ok(result(Lexeme::Finite(token), end));
    }
    Ok(match finite(bytes, at, negative, 10, profile.radix()) {
        Some((token, end)) => result(Lexeme::Finite(token), end),
        None => result(Lexeme::NoConversion, 0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same_prefix(input: &[u8], profile: Profile) {
        let end = input.iter().position(|&b| b == 0).unwrap_or(input.len());
        let actual = lex_slice(input, profile).unwrap();
        let expected = lex_slice(&input[..end], profile).unwrap();
        assert_eq!(actual.consumed, expected.consumed);
        assert_eq!(actual.leading_space, expected.leading_space);
        assert_eq!(actual.sign, expected.sign);
        match (actual.lexeme, expected.lexeme) {
            (Lexeme::Finite(a), Lexeme::Finite(b)) => {
                assert_eq!(a.negative(), b.negative());
                assert_eq!(a.radix(), b.radix());
                assert_eq!(a.exponent(), b.exponent());
                assert_eq!(a.integer, b.integer);
                assert_eq!(a.fraction, b.fraction);
                assert_eq!(a.radix_offset, b.radix_offset);
                assert_eq!(a.exponent_digits, b.exponent_digits);
                assert_eq!(a.digit_count(), b.digit_count());
                assert_eq!(a.fraction_digits(), b.fraction_digits());
                assert_eq!(a.one_digit_tiny_decimal(), b.one_digit_tiny_decimal());
                assert_eq!(
                    a.digits().collect::<Vec<_>>(),
                    b.digits().collect::<Vec<_>>()
                );
            }
            (a, b) => assert_eq!(format!("{a:?}"), format!("{b:?}")),
        }
    }

    #[test]
    fn nul_terminates_every_numeric_grammar_path() {
        for profile in [Profile::C, Profile::DE_NUMERIC] {
            for text in [
                "",
                " \t\n\r\u{b}\u{c}",
                "+",
                "-",
                ".",
                ",",
                "-0",
                "12.345",
                "12,345",
                "+1.25e-003tail",
                "-1,25E+2",
                "1e+",
                "1e-99999999999",
                "1e+99999999999",
                "0x",
                "-0X1.Ap+4",
                "0x1,ap-16445",
                "0x1p-",
                "inf",
                "INFINITY",
                "-infinite",
                "nan",
                "NaN()",
                "-nan(0x123)",
                "nan(abc_123)",
                "nan(incomplete",
                "nan(bad-value)",
                "1\t2\t3",
            ] {
                for at in 0..=text.len() {
                    let mut input = text.as_bytes().to_vec();
                    input.insert(at, 0);
                    input.extend_from_slice(b"123e99)\xff\0nan(42)");
                    same_prefix(&input, profile);
                }
            }
            // Exercise arbitrary bytes after a valid prefix and after NUL.
            for byte in 0..=u8::MAX {
                for prefix in [b"1".as_slice(), b"0x1", b"1e", b"nan(", b" "] {
                    let mut input = prefix.to_vec();
                    input.extend_from_slice(&[byte, 0, byte, b'9', b')']);
                    same_prefix(&input, profile);
                }
            }
        }
    }
}
