//! Ordered field grammar and fallible request expansion.

use super::{Failure, Kind, conversion_failure, failure, unsupported};
use fastmash_conversion::{
    caller_policy::{self, ScannerRejection, TokenValue},
    profile::Profile,
};
use fastmash_numeric_contract::IntegerOperand;
use std::{ffi::OsString, os::unix::ffi::OsStrExt};

use super::command_memory::reserve;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Field {
    Number(u64),
    Name(Vec<u8>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Selector {
    Single(Field),
    Pair { left: Field, right: Field },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Request {
    pub kind: Kind,
    pub selector: Selector,
}

#[cfg(test)]
impl PartialEq<(Kind, Field)> for Request {
    fn eq(&self, other: &(Kind, Field)) -> bool {
        self.kind == other.0 && self.selector == Selector::Single(other.1.clone())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenKind {
    End,
    Space,
    Comma,
    Dash,
    Colon,
    Identifier,
    Integer(i64),
    Float(fastmash_numeric_contract::Raw80),
}

#[derive(Clone, Copy)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}

struct Scanner {
    script: Vec<u8>,
    position: usize,
    lookahead: Option<Token>,
    keep_space: bool,
    named_numbers: bool,
    profile: Profile,
    utf8: bool,
}

fn quoted_error(prefix: &[u8], value: &[u8], utf8: bool) -> Failure {
    let mut message = prefix.to_vec();
    super::named_fields::quote_with(value, &mut message, utf8);
    message.push(b'\n');
    failure(message)
}

fn decode_identifier(bytes: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut result = Vec::new();
    reserve(&mut result, bytes.len())?;
    let mut bytes = bytes.iter();
    while let Some(&byte) = bytes.next() {
        result.push(if byte == b'\\' {
            *bytes.next().expect("scanner validated identifier escape")
        } else {
            byte
        });
    }
    Ok(result)
}
fn space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

impl Scanner {
    fn new(args: &[OsString]) -> Result<Self, Failure> {
        Self::from_args(args, false)
    }

    fn from_args(args: &[OsString], grouped: bool) -> Result<Self, Failure> {
        if args.is_empty() {
            return Err(unsupported("unsupported command: no operation specified"));
        }
        let length = args
            .iter()
            .try_fold(args.len() - 1, |n, arg| n.checked_add(arg.as_bytes().len()));
        let length = length.ok_or_else(|| unsupported("command memory allocation failed"))?;
        let mut script = Vec::new();
        reserve(&mut script, length)?;
        for (index, arg) in args.iter().enumerate() {
            if index != 0 {
                script.push(b' ');
            }
            script.extend_from_slice(arg.as_bytes());
        }
        if !grouped && script.is_empty() {
            return Err(failure(b"missing operation specifiers\n".to_vec()));
        }
        if script.contains(&0) {
            return Err(unsupported("NUL bytes in command syntax are unsupported"));
        }
        Ok(Self {
            script,
            position: 0,
            lookahead: None,
            keep_space: false,
            named_numbers: false,
            profile: Profile::C,
            utf8: false,
        })
    }

    fn spelling(&self, token: Token) -> &[u8] {
        &self.script[token.start..token.end]
    }

    fn peek(&mut self) -> Result<Token, Failure> {
        if let Some(token) = self.lookahead {
            return Ok(token);
        }
        let token = self.next()?;
        self.lookahead = Some(token);
        Ok(token)
    }

    fn next(&mut self) -> Result<Token, Failure> {
        if let Some(token) = self.lookahead.take() {
            return Ok(token);
        }
        let mut start = self.position;
        let token = |kind, start, end| Token { kind, start, end };
        if start == self.script.len() {
            return Ok(token(TokenKind::End, start, start));
        }
        if space(self.script[start]) {
            while self.script.get(self.position).is_some_and(|b| space(*b)) {
                self.position += 1;
            }
            if self.keep_space {
                let kind = if self.position == self.script.len() {
                    TokenKind::End
                } else {
                    TokenKind::Space
                };
                return Ok(token(kind, start, self.position));
            }
            start = self.position;
        }
        // GNU checks for EOF before skipping whitespace, not afterwards.
        let Some(&first) = self.script.get(start) else {
            return Err(failure(b"invalid operand ''\n".to_vec()));
        };
        let punctuation = match first {
            b',' => Some(TokenKind::Comma),
            b'-' => Some(TokenKind::Dash),
            b':' => Some(TokenKind::Colon),
            _ => None,
        };
        if let Some(kind) = punctuation {
            self.position += 1;
            return Ok(token(kind, start, self.position));
        }
        if first.is_ascii_digit() {
            // Field integers need only strtol's nonnegative decimal domain.
            // Scanning the whole remaining command through the conversion
            // evidence API imposed its unrelated input-size bound on argv.
            let digits = self.script[start..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count();
            let end = start + digits;
            let integer = self.script[start..end].iter().try_fold(0i64, |n, b| {
                n.checked_mul(10)?.checked_add(i64::from(b - b'0'))
            });
            if self.script.get(end) != Some(&b'.') {
                let invalid_tail = self
                    .script
                    .get(end)
                    .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_');
                let value = integer.filter(|_| !invalid_tail).ok_or_else(|| {
                    let mut message = b"invalid numeric value '".to_vec();
                    message.extend_from_slice(&self.script[start..]);
                    message.extend_from_slice(b"'\n");
                    failure(message)
                })?;
                self.position = end;
                return Ok(token(TokenKind::Integer(value), start, end));
            }
            // Whitespace terminates numeric conversion; subsequent operations
            // cannot affect this token's value or errno. Preserve the original
            // suffix separately for GNU's invalid-numeric diagnostic below.
            let suffix = &self.script[start..];
            let numeric_end = suffix
                .iter()
                .position(|b| space(*b))
                .unwrap_or(suffix.len());
            let result =
                caller_policy::scanner("command-argument", &suffix[..numeric_end], 0, self.profile)
                    .map_err(|error| conversion_failure(error.error))?;
            // A finite command token converted to infinity has a known range
            // failure even when the historical errno classifier has no route.
            let finite_overflow = result.phases.iter().any(|phase| {
                matches!(phase, caller_policy::PrimitivePhase::Floating(parsed)
                    if matches!(parsed.category, fastmash_conversion::records::Category::Decimal | fastmash_conversion::records::Category::Hexadecimal)
                    && parsed.value.is_some_and(|v| v.classify() == fastmash_numeric_contract::ValueClass::Infinity))
            });
            let rejection = if result.rejection == Some(ScannerRejection::RequiredFactUnavailable)
                && finite_overflow
            {
                Some(ScannerRejection::Range)
            } else {
                result.rejection
            };
            match rejection {
                Some(ScannerRejection::AlphabeticTail | ScannerRejection::Range) => {
                    let mut message = b"invalid numeric value '".to_vec();
                    message.extend_from_slice(&self.script[start..]);
                    message.extend_from_slice(b"'\n");
                    return Err(failure(message));
                }
                Some(ScannerRejection::RequiredFactUnavailable) => {
                    return Err(unsupported(
                        "numeric command argument is outside the supported range",
                    ));
                }
                Some(ScannerRejection::NonDigitStart) => {
                    return Err(unsupported("internal numeric argument scanner failure"));
                }
                None => {}
            }
            self.position = start
                + result
                    .consumed
                    .ok_or_else(|| unsupported("internal numeric argument scanner failure"))?;
            let kind = match result.value {
                Some(TokenValue::Integer(IntegerOperand::I64(n))) => TokenKind::Integer(n),
                Some(TokenValue::Float(value)) => TokenKind::Float(value.raw()),
                _ => return Err(unsupported("internal numeric argument scanner failure")),
            };
            return Ok(token(kind, start, self.position));
        }
        if first.is_ascii_alphabetic() || first == b'_' || first == b'\\' {
            let mut length = 0;
            while let Some(&byte) = self.script.get(self.position) {
                if byte == b'\\' {
                    self.position += 1;
                    if self.position == self.script.len() {
                        return Err(failure(b"backslash at end of identifier\n".to_vec()));
                    }
                } else if !byte.is_ascii_alphanumeric() && byte != b'_' {
                    break;
                }
                length += 1;
                if length >= 512 {
                    return Err(failure(b"identifier name too long\n".to_vec()));
                }
                self.position += 1;
            }
            return Ok(token(TokenKind::Identifier, start, self.position));
        }
        Err(quoted_error(
            b"invalid operand ",
            &self.script[start..],
            self.utf8,
        ))
    }

    fn field(&mut self, kind: Kind, range_end: bool) -> Result<Field, Failure> {
        let token = self.next();
        let token = token?;
        match token.kind {
            TokenKind::Integer(_) if self.named_numbers => {
                Ok(Field::Name(decode_identifier(self.spelling(token))?))
            }
            TokenKind::Integer(n) if n > 0 => Ok(Field::Number(n as u64)),
            TokenKind::Identifier => Ok(Field::Name(decode_identifier(self.spelling(token))?)),
            TokenKind::Integer(_) | TokenKind::Float(_) => {
                let mut message = b"invalid field '".to_vec();
                message.extend_from_slice(self.spelling(token));
                message.extend_from_slice(
                    format!("' for operation {}\n", quoted(name(kind), self.utf8)).as_bytes(),
                );
                Err(failure(message))
            }
            TokenKind::Dash => Err(operation_error("invalid field range", kind, self.utf8)),
            TokenKind::End if range_end => {
                Err(operation_error("invalid field range", kind, self.utf8))
            }
            TokenKind::Comma | TokenKind::End => {
                Err(operation_error("missing field", kind, self.utf8))
            }
            TokenKind::Colon => Err(operation_error("invalid field pair", kind, self.utf8)),
            _ => Err(operation_error("missing field", kind, self.utf8)),
        }
    }
}

pub(super) fn name(kind: Kind) -> &'static str {
    match kind {
        Kind::Cut => "cut",
        Kind::Rounding(kind) => kind.name(),
        Kind::Getnum(_) => "getnum",
        Kind::Bin(_) => "bin",
        Kind::Strbin(_) => "strbin",
        Kind::Base64 => "base64",
        Kind::Debase64 => "debase64",
        Kind::Checksum(algorithm) => algorithm.name(),
        Kind::Path(kind) => kind.name(),
        Kind::Count => "count",
        Kind::Countunique => "countunique",
        Kind::Unique => "unique",
        Kind::Collapse => "collapse",
        Kind::First => "first",
        Kind::Last => "last",
        Kind::Rand => "rand",
        Kind::Min => "min",
        Kind::Max => "max",
        Kind::Absmin => "absmin",
        Kind::Absmax => "absmax",
        Kind::Range => "range",
        Kind::Sum => "sum",
        Kind::Mean => "mean",
        Kind::Geomean => "geomean",
        Kind::Harmmean => "harmmean",
        Kind::Ms => "ms",
        Kind::Rms => "rms",
        Kind::Median => "median",
        Kind::Mode => "mode",
        Kind::Antimode => "antimode",
        Kind::Q1 => "q1",
        Kind::Q3 => "q3",
        Kind::Iqr => "iqr",
        Kind::Percentile(_) => "perc",
        Kind::Trimmean(_) => "trimmean",
        Kind::Pvar => "pvar",
        Kind::Svar => "svar",
        Kind::Pstdev => "pstdev",
        Kind::Sstdev => "sstdev",
        Kind::Madraw => "madraw",
        Kind::Mad => "mad",
        Kind::Pskew => "pskew",
        Kind::Sskew => "sskew",
        Kind::Pkurt => "pkurt",
        Kind::Skurt => "skurt",
        Kind::Jarque => "jarque",
        Kind::Dpo => "dpo",
        Kind::Pcov => "pcov",
        Kind::Scov => "scov",
        Kind::Ppearson => "ppearson",
        Kind::Spearson => "spearson",
        Kind::Dotprod => "dotprod",
    }
}

/// `name` quoted like GNU's `quote()`: curly quotes under a UTF-8 `LC_CTYPE`.
pub(super) fn quoted(name: &str, utf8: bool) -> String {
    let mut quoted = Vec::new();
    super::named_fields::quote_with(name.as_bytes(), &mut quoted, utf8);
    String::from_utf8(quoted).expect("quoting keeps UTF-8")
}

fn operation_error(reason: &str, kind: Kind, utf8: bool) -> Failure {
    failure(format!("{reason} for operation {}\n", quoted(name(kind), utf8)).into_bytes())
}

fn pair_required(kind: Kind, utf8: bool) -> Failure {
    failure(
        format!(
            "operation {} requires field pairs\n",
            quoted(name(kind), utf8)
        )
        .into_bytes(),
    )
}

fn pair_forbidden(kind: Kind, utf8: bool) -> Failure {
    failure(
        format!(
            "operation {} cannot use pair of fields\n",
            quoted(name(kind), utf8)
        )
        .into_bytes(),
    )
}

fn paired(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Pcov | Kind::Scov | Kind::Ppearson | Kind::Spearson | Kind::Dotprod
    )
}

#[cfg(test)]
pub(super) fn parse(args: &[OsString]) -> Result<Vec<Request>, Failure> {
    let mut scanner = Scanner::new(args)?;
    parse_operations(&mut scanner, false)
}

fn parse_operations(scanner: &mut Scanner, grouped: bool) -> Result<Vec<Request>, Failure> {
    parse_operations_for_mode(scanner, grouped, "groupby")
}

/// Operations without a parameter, by name; `operation_kind` adds the rest.
const PLAIN_OPERATIONS: &[(&[u8], Kind)] = &[
    (b"cut", Kind::Cut),
    (b"echo", Kind::Cut),
    (b"base64", Kind::Base64),
    (b"debase64", Kind::Debase64),
    (b"count", Kind::Count),
    (b"first", Kind::First),
    (b"countunique", Kind::Countunique),
    (b"unique", Kind::Unique),
    (b"uniq", Kind::Unique),
    (b"collapse", Kind::Collapse),
    (b"last", Kind::Last),
    (b"rand", Kind::Rand),
    (b"min", Kind::Min),
    (b"max", Kind::Max),
    (b"absmin", Kind::Absmin),
    (b"absmax", Kind::Absmax),
    (b"range", Kind::Range),
    (b"sum", Kind::Sum),
    (b"mean", Kind::Mean),
    (b"geomean", Kind::Geomean),
    (b"harmmean", Kind::Harmmean),
    (b"ms", Kind::Ms),
    (b"rms", Kind::Rms),
    (b"median", Kind::Median),
    (b"mode", Kind::Mode),
    (b"antimode", Kind::Antimode),
    (b"q1", Kind::Q1),
    (b"q3", Kind::Q3),
    (b"iqr", Kind::Iqr),
    (b"pvar", Kind::Pvar),
    (b"svar", Kind::Svar),
    (b"pstdev", Kind::Pstdev),
    (b"sstdev", Kind::Sstdev),
    (b"madraw", Kind::Madraw),
    (b"mad", Kind::Mad),
    (b"pskew", Kind::Pskew),
    (b"sskew", Kind::Sskew),
    (b"pkurt", Kind::Pkurt),
    (b"skurt", Kind::Skurt),
    (b"jarque", Kind::Jarque),
    (b"dpo", Kind::Dpo),
    (b"pcov", Kind::Pcov),
    (b"scov", Kind::Scov),
    (b"ppearson", Kind::Ppearson),
    (b"spearson", Kind::Spearson),
    (b"dotprod", Kind::Dotprod),
];

/// The operation named `spelling` (ASCII case-insensitive), with its default
/// parameter; `None` for names that are not operations.
fn operation_kind(spelling: &[u8]) -> Option<Kind> {
    use super::line_numeric::Rounding;
    if let Some((_, kind)) = PLAIN_OPERATIONS
        .iter()
        .find(|(name, _)| spelling.eq_ignore_ascii_case(name))
    {
        return Some(*kind);
    }
    if let Some(kind) = [
        Rounding::Round,
        Rounding::Floor,
        Rounding::Ceil,
        Rounding::Trunc,
        Rounding::Frac,
    ]
    .into_iter()
    .find(|kind| spelling.eq_ignore_ascii_case(kind.name().as_bytes()))
    {
        return Some(Kind::Rounding(kind));
    }
    if let Some(kind) = super::path_fields::Operation::from_name(spelling) {
        return Some(Kind::Path(kind));
    }
    if let Some(algorithm) = super::checksum::Algorithm::from_name(spelling) {
        return Some(Kind::Checksum(algorithm));
    }
    let is = |name: &[u8]| spelling.eq_ignore_ascii_case(name);
    Some(if is(b"getnum") {
        Kind::Getnum(super::line_numeric::Extraction::Positive)
    } else if is(b"bin") {
        Kind::Bin(super::numerics::Numerics::value80(super::integer(100)).raw())
    } else if is(b"strbin") {
        Kind::Strbin(std::num::NonZeroU64::new(10).unwrap())
    } else if is(b"trimmean") {
        Kind::Trimmean(super::ordered_statistics::Trim::zero())
    } else if is(b"perc") {
        Kind::Percentile(95)
    } else {
        return None;
    })
}

fn invalid_parameter(scanner: &Scanner, token: Token, kind: Kind) -> Failure {
    let mut message = b"invalid parameter ".to_vec();
    message.extend_from_slice(scanner.spelling(token));
    message.extend_from_slice(
        format!(" for operation {}\n", quoted(name(kind), scanner.utf8)).as_bytes(),
    );
    failure(message)
}

/// `:PARAMETER`s of the per-row operations that take a character (getnum's
/// type); `None` records a numeric parameter, refused later.
fn line_parameters(scanner: &mut Scanner, kind: Kind) -> Result<Vec<Option<u8>>, Failure> {
    let mut parameters = Vec::new();
    if !matches!(
        kind,
        Kind::Rounding(_)
            | Kind::Getnum(_)
            | Kind::Base64
            | Kind::Debase64
            | Kind::Checksum(_)
            | Kind::Path(_)
    ) {
        return Ok(parameters);
    }
    while scanner.peek()?.kind == TokenKind::Colon {
        scanner.next()?;
        let token = scanner.next()?;
        let parameter = match token.kind {
            TokenKind::Integer(_) | TokenKind::Float(_) => None,
            TokenKind::Identifier if matches!(kind, Kind::Getnum(_)) => {
                decode_identifier(scanner.spelling(token))?.first().copied()
            }
            TokenKind::Space | TokenKind::End => {
                return Err(operation_error("missing parameter", kind, scanner.utf8));
            }
            _ => return Err(invalid_parameter(scanner, token, kind)),
        };
        reserve(&mut parameters, 1)?;
        parameters.push(parameter);
    }
    Ok(parameters)
}

/// Numeric `:PARAMETER`s of perc, trimmean, bin and strbin.
fn numeric_parameters(scanner: &mut Scanner, kind: Kind) -> Result<Vec<TokenKind>, Failure> {
    let mut parameters = Vec::new();
    if !matches!(
        kind,
        Kind::Percentile(_) | Kind::Trimmean(_) | Kind::Bin(_) | Kind::Strbin(_)
    ) {
        return Ok(parameters);
    }
    while scanner.peek()?.kind == TokenKind::Colon {
        scanner.next()?;
        let token = scanner.next()?;
        match token.kind {
            TokenKind::Integer(n) => {
                reserve(&mut parameters, 1)?;
                parameters.push(TokenKind::Integer(n));
            }
            TokenKind::Float(raw)
                if matches!(kind, Kind::Trimmean(_) | Kind::Bin(_) | Kind::Strbin(_)) =>
            {
                reserve(&mut parameters, 1)?;
                parameters.push(TokenKind::Float(raw));
            }
            TokenKind::Float(_) => {
                return Err(unsupported(
                    "non-integer percentile parameters are outside the supported range",
                ));
            }
            TokenKind::Space | TokenKind::End => {
                return Err(operation_error("missing parameter", kind, scanner.utf8));
            }
            _ => return Err(invalid_parameter(scanner, token, kind)),
        }
    }
    Ok(parameters)
}

/// A selector and, for a numeric range, its inclusive end. Ranges stay
/// compact until the whole list has passed its syntax checks.
type SelectorSpan = (Selector, Option<u64>);

fn selector_list(scanner: &mut Scanner, kind: Kind) -> Result<Vec<SelectorSpan>, Failure> {
    let mut selectors = Vec::new();
    loop {
        if paired(kind) && scanner.peek()?.kind == TokenKind::Colon {
            return Err(operation_error("invalid field pair", kind, scanner.utf8));
        }
        let start = scanner.field(kind, false)?;
        reserve(&mut selectors, 1)?;
        if scanner.peek()?.kind == TokenKind::Colon {
            scanner.next()?;
            if matches!(
                scanner.peek()?.kind,
                TokenKind::Colon | TokenKind::Comma | TokenKind::End | TokenKind::Dash
            ) {
                return Err(operation_error("invalid field pair", kind, scanner.utf8));
            }
            let right = scanner.field(kind, false)?;
            if scanner.peek()?.kind == TokenKind::Colon {
                return Err(operation_error("invalid field pair", kind, scanner.utf8));
            }
            if scanner.peek()?.kind == TokenKind::Dash {
                return Err(failure(b"paired field ranges are invalid\n".to_vec()));
            }
            selectors.push((Selector::Pair { left: start, right }, None));
        } else {
            let end = if scanner.peek()?.kind == TokenKind::Dash {
                scanner.next()?;
                let end = scanner.field(kind, true)?;
                let (Field::Number(start), Field::Number(end)) = (&start, end) else {
                    return Err(failure(
                        format!(
                            "field range for {} must be numeric\n",
                            quoted(name(kind), scanner.utf8)
                        )
                        .into_bytes(),
                    ));
                };
                if *start >= end {
                    return Err(operation_error("invalid field range", kind, scanner.utf8));
                }
                Some(end)
            } else {
                None
            };
            selectors.push((Selector::Single(start), end));
        }
        if scanner.peek()?.kind != TokenKind::Comma {
            return Ok(selectors);
        }
        scanner.next()?;
    }
}

/// `kind` with its parsed parameters applied, or the parameter's error.
fn with_parameters(
    scanner: &Scanner,
    mut kind: Kind,
    parameters: &[TokenKind],
    line_parameters: &[Option<u8>],
) -> Result<Kind, Failure> {
    if matches!(kind, Kind::Percentile(_) | Kind::Trimmean(_)) {
        if parameters.len() > 1 {
            return Err(operation_error("too many parameters", kind, scanner.utf8));
        }
        if matches!(kind, Kind::Percentile(_)) {
            let percent = match parameters.first() {
                Some(TokenKind::Integer(n)) => *n,
                None => 95,
                _ => unreachable!("percentile floats refused above"),
            };
            if !(1..=100).contains(&percent) {
                return Err(failure(
                    format!("invalid percentile value {percent}\n").into_bytes(),
                ));
            }
            kind = Kind::Percentile(percent as u8);
        } else {
            let value = match parameters.first() {
                Some(TokenKind::Integer(n)) => {
                    super::numerics::Numerics::value80(super::integer(*n as u64))
                }
                Some(TokenKind::Float(raw)) => {
                    fastmash_numeric_contract::Value80::from_raw(*raw).expect("scanner value")
                }
                None => fastmash_numeric_contract::Value80::signed_zero(false),
                _ => unreachable!("numeric trim parameter"),
            };
            kind = Kind::Trimmean(super::ordered_statistics::Trim::new(
                value,
                scanner.profile,
            )?);
        }
    }
    if matches!(kind, Kind::Bin(_) | Kind::Strbin(_)) {
        if parameters.len() > 1 {
            return Err(operation_error("too many parameters", kind, scanner.utf8));
        }
        if let Some(parameter) = parameters.first() {
            kind = match (kind, parameter) {
                (Kind::Bin(_), TokenKind::Integer(n)) => {
                    Kind::Bin(super::numerics::Numerics::value80(super::integer(*n as u64)).raw())
                }
                (Kind::Bin(_), TokenKind::Float(raw)) => Kind::Bin(*raw),
                (Kind::Strbin(_), TokenKind::Integer(n)) => {
                    Kind::Strbin(std::num::NonZeroU64::new(*n as u64).ok_or_else(|| {
                        failure(b"strbin bucket size must not be zero\n".to_vec())
                    })?)
                }
                (Kind::Strbin(_), TokenKind::Float(_)) => {
                    return Err(failure(
                        b"strbin requires an integer bucket count\n".to_vec(),
                    ));
                }
                _ => unreachable!("numeric binning parameter"),
            };
        }
    }
    if !line_parameters.is_empty() {
        if line_parameters.len() > 1 || !matches!(kind, Kind::Getnum(_)) {
            return Err(operation_error("too many parameters", kind, scanner.utf8));
        }
        let Some(byte) = line_parameters[0] else {
            return Err(failure(
                b"getnum requires a character type parameter\n".to_vec(),
            ));
        };
        let Some(extraction) = super::line_numeric::Extraction::from_byte(byte) else {
            let mut message = b"invalid getnum type '".to_vec();
            message.push(byte);
            message.extend_from_slice(b"'\n");
            return Err(failure(message));
        };
        kind = Kind::Getnum(extraction);
    }
    Ok(kind)
}

/// Check pairing, then append one request per selected field.
fn expand_requests(
    requests: &mut Vec<Request>,
    kind: Kind,
    selectors: Vec<SelectorSpan>,
    utf8: bool,
) -> Result<(), Failure> {
    for (selector, _) in &selectors {
        match selector {
            Selector::Single(_) if paired(kind) => return Err(pair_required(kind, utf8)),
            Selector::Pair { .. } if !paired(kind) => return Err(pair_forbidden(kind, utf8)),
            _ => {}
        }
    }
    let total = selectors
        .iter()
        .try_fold(requests.len() as u64, |count, (selector, end)| {
            let added = match (selector, end) {
                (Selector::Single(Field::Number(start)), Some(end)) => {
                    end.checked_sub(*start)?.checked_add(1)?
                }
                (_, None) => 1,
                _ => unreachable!("range endpoints were checked"),
            };
            count.checked_add(added)
        })
        .ok_or_else(|| unsupported("command memory allocation failed"))?;
    let total =
        usize::try_from(total).map_err(|_| unsupported("command memory allocation failed"))?;
    let additional = total - requests.len();
    reserve(requests, additional)?;
    for (selector, end) in selectors {
        match (selector, end) {
            (Selector::Single(Field::Number(start)), Some(end)) => {
                requests.extend((start..=end).map(|field| Request {
                    kind,
                    selector: Selector::Single(Field::Number(field)),
                }));
            }
            (selector, None) => requests.push(Request { kind, selector }),
            _ => unreachable!("range endpoints were checked"),
        }
    }
    Ok(())
}

fn parse_operations_for_mode(
    scanner: &mut Scanner,
    grouped: bool,
    mode: &str,
) -> Result<Vec<Request>, Failure> {
    let mut requests: Vec<Request> = Vec::new();
    while scanner.peek()?.kind != TokenKind::End {
        let token = scanner.next()?;
        let decoded = decode_identifier(scanner.spelling(token))?;
        let spelling = decoded.as_slice();
        if (grouped && is_group(spelling))
            || [
                b"reverse".as_slice(),
                b"nop",
                b"noop",
                b"check",
                b"rmdup",
                b"dedup",
                b"crosstab",
                b"ct",
            ]
            .iter()
            .any(|m| spelling.eq_ignore_ascii_case(m))
        {
            return Err(quoted_error(
                b"conflicting operation ",
                spelling,
                scanner.utf8,
            ));
        }
        if token.kind != TokenKind::Identifier {
            return Err(quoted_error(b"invalid operation ", spelling, scanner.utf8));
        }
        let Some(kind) = operation_kind(spelling) else {
            if spelling.eq_ignore_ascii_case(b"transpose") {
                return Err(quoted_error(
                    b"conflicting operation ",
                    spelling,
                    scanner.utf8,
                ));
            }
            if unavailable_operation(spelling) {
                return Err(unsupported(&format!(
                    "unsupported operation '{}'",
                    String::from_utf8_lossy(spelling)
                )));
            }
            return Err(quoted_error(b"invalid operation ", spelling, scanner.utf8));
        };
        let expected_line = !grouped
            && requests
                .first()
                .map_or(kind.is_line(), |r| r.kind.is_line());
        if (kind.is_line()) != expected_line || (grouped && kind.is_line()) {
            let expected = if expected_line && !grouped {
                "line"
            } else {
                mode
            };
            let found = if kind.is_line() { "line" } else { "groupby" };
            let prefix = format!(
                "conflicting operation found: expecting {expected} operations, but found {found} operation "
            );
            return Err(quoted_error(prefix.as_bytes(), spelling, scanner.utf8));
        }
        scanner.keep_space = true;
        let line_parameters = line_parameters(scanner, kind)?;
        let parameters = numeric_parameters(scanner, kind)?;
        match scanner.peek()?.kind {
            TokenKind::Colon => return Err(unsupported("operation parameters are unsupported")),
            TokenKind::Space => {
                scanner.next()?;
            }
            _ => {}
        }
        scanner.keep_space = false;
        let selectors = selector_list(scanner, kind)?;
        let kind = with_parameters(scanner, kind, &parameters, &line_parameters)?;
        expand_requests(&mut requests, kind, selectors, scanner.utf8)?;
    }
    Ok(requests)
}

fn unavailable_operation(value: &[u8]) -> bool {
    [
        "bin",
        "strbin",
        "floor",
        "ceil",
        "round",
        "trunc",
        "frac",
        "getnum",
        "cut",
        "echo",
        "transpose",
        "reverse",
        "line",
        "dedup",
        "rmdup",
        "nop",
        "noop",
        "crosstab",
        "ct",
        "check",
    ]
    .iter()
    .any(|name| value.eq_ignore_ascii_case(name.as_bytes()))
}
fn is_group(bytes: &[u8]) -> bool {
    [b"groupby".as_slice(), b"grouping", b"gb"]
        .iter()
        .any(|name| bytes.eq_ignore_ascii_case(name))
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Aggregate,
    Crosstab,
    Line,
    Reverse,
    Transpose,
    Noop,
    Check { lines: u64, fields: u64 },
    Dedup,
}

pub(super) struct Command {
    pub mode: Mode,
    pub keys: Vec<Field>,
    pub operations: Vec<Request>,
}

#[cfg(test)]
pub(super) fn command(args: &[OsString], group: Option<&OsString>) -> Result<Command, Failure> {
    command_for_format(args, group, false, Profile::C, false)
}

pub(super) fn command_for_format(
    args: &[OsString],
    group: Option<&OsString>,
    named_numbers: bool,
    profile: Profile,
    utf8: bool,
) -> Result<Command, Failure> {
    if let Some(group) = group {
        let mut scanner = Scanner::from_args(std::slice::from_ref(group), true)?;
        scanner.named_numbers = named_numbers;
        scanner.profile = profile;
        scanner.utf8 = utf8;
        let keys = group_fields(&mut scanner, "groupby")?;
        let mut scanner = Scanner::from_args(args, true)?;
        scanner.named_numbers = named_numbers;
        scanner.profile = profile;
        scanner.utf8 = utf8;
        let operations = parse_operations(&mut scanner, true)?;
        if operations.is_empty() {
            return Err(unsupported("grouping requires at least one operation"));
        }
        return Ok(Command {
            mode: Mode::Aggregate,
            keys,
            operations,
        });
    }
    let mut scanner = Scanner::new(args)?;
    scanner.named_numbers = named_numbers;
    scanner.profile = profile;
    scanner.utf8 = utf8;
    let token = scanner.peek()?;
    let decoded = decode_identifier(scanner.spelling(token))?;
    if decoded.eq_ignore_ascii_case(b"crosstab") || decoded.eq_ignore_ascii_case(b"ct") {
        scanner.next()?;
        let keys = group_fields(&mut scanner, "crosstab")?;
        if keys.len() != 2 {
            return Err(failure(
                format!("crosstab requires exactly 2 fields, found {}\n", keys.len()).into_bytes(),
            ));
        }
        let mut operations = parse_operations_for_mode(&mut scanner, true, "crosstab")?;
        if operations.is_empty() {
            reserve(&mut operations, 1)?;
            let field = match &keys[0] {
                Field::Number(n) => Field::Number(*n),
                Field::Name(name) => {
                    let mut bytes = Vec::new();
                    reserve(&mut bytes, name.len())?;
                    bytes.extend_from_slice(name);
                    Field::Name(bytes)
                }
            };
            operations.push(Request {
                kind: Kind::Count,
                selector: Selector::Single(field),
            });
        } else if operations.len() > 1 || matches!(operations[0].selector, Selector::Pair { .. }) {
            // GNU represents a paired calculation as two operations for this check.
            let count = operations
                .iter()
                .try_fold(0usize, |n, request| {
                    n.checked_add(if matches!(request.selector, Selector::Pair { .. }) {
                        2
                    } else {
                        1
                    })
                })
                .ok_or_else(|| unsupported("command memory allocation failed"))?;
            return Err(failure(
                format!("crosstab supports one operation, found {}\n", count).into_bytes(),
            ));
        }
        return Ok(Command {
            mode: Mode::Crosstab,
            keys,
            operations,
        });
    }
    if decoded.eq_ignore_ascii_case(b"check") {
        scanner.next()?;
        return Ok(Command {
            mode: check_options(&mut scanner)?,
            keys: Vec::new(),
            operations: Vec::new(),
        });
    }
    if decoded.eq_ignore_ascii_case(b"rmdup") || decoded.eq_ignore_ascii_case(b"dedup") {
        scanner.next()?;
        let specs = group_specs(&mut scanner, "dedup")?;
        let extra = scanner.next()?;
        if extra.kind != TokenKind::End {
            return Err(quoted_error(
                b"extra operand ",
                scanner.spelling(extra),
                scanner.utf8,
            ));
        }
        if specs.len() != 1
            || matches!(&specs[0], (Field::Number(first), Some(last)) if first != last)
        {
            return Err(unsupported("rmdup supports exactly one key"));
        }
        let keys = expand_groups(specs)?;
        return Ok(Command {
            mode: Mode::Dedup,
            keys,
            operations: Vec::new(),
        });
    }
    let table_mode = if decoded.eq_ignore_ascii_case(b"reverse") {
        Some(Mode::Reverse)
    } else if decoded.eq_ignore_ascii_case(b"transpose") {
        Some(Mode::Transpose)
    } else if decoded.eq_ignore_ascii_case(b"nop") || decoded.eq_ignore_ascii_case(b"noop") {
        Some(Mode::Noop)
    } else {
        None
    };
    if let Some(mode) = table_mode {
        scanner.next()?;
        let extra = scanner.next()?;
        if extra.kind != TokenKind::End {
            return Err(quoted_error(
                b"extra operand ",
                scanner.spelling(extra),
                scanner.utf8,
            ));
        }
        return Ok(Command {
            mode,
            keys: Vec::new(),
            operations: Vec::new(),
        });
    }
    if !is_group(&decoded) {
        let operations = parse_operations(&mut scanner, false)?;
        return Ok(Command {
            mode: if operations.first().is_some_and(|r| r.kind.is_line()) {
                Mode::Line
            } else {
                Mode::Aggregate
            },
            keys: Vec::new(),
            operations,
        });
    }
    scanner.next()?;
    let keys = group_fields(&mut scanner, "groupby")?;
    let operations = parse_operations(&mut scanner, true)?;
    if operations.is_empty() {
        return Err(failure(b"missing operation\n".to_vec()));
    }
    Ok(Command {
        mode: Mode::Aggregate,
        keys,
        operations,
    })
}

fn group_error(reason: &str, mode: &str, utf8: bool) -> Failure {
    failure(format!("{reason} for operation {}\n", quoted(mode, utf8)).into_bytes())
}

fn group_fields(scanner: &mut Scanner, mode: &str) -> Result<Vec<Field>, Failure> {
    expand_groups(group_specs(scanner, mode)?)
}

type GroupSpecs = Vec<(Field, Option<u64>)>;

fn group_specs(scanner: &mut Scanner, mode: &str) -> Result<GroupSpecs, Failure> {
    let mut spans = Vec::new();
    loop {
        let token = scanner.next()?;
        let (field, end) = match token.kind {
            TokenKind::Integer(_) if scanner.named_numbers => (
                Field::Name(decode_identifier(scanner.spelling(token))?),
                None,
            ),
            TokenKind::Identifier => (
                Field::Name(decode_identifier(scanner.spelling(token))?),
                None,
            ),
            TokenKind::Integer(n) if n > 0 => {
                let peek = scanner.peek()?;
                if peek.kind == TokenKind::Dash {
                    scanner.next()?;
                    let end = scanner.next()?;
                    let TokenKind::Integer(end) = end.kind else {
                        return Err(group_error("invalid field range", mode, scanner.utf8));
                    };
                    if end < n {
                        return Err(group_error("invalid field range", mode, scanner.utf8));
                    }
                    if n > 2147483646 || end > 2147483646 {
                        return Err(unsupported("grouping range is outside the supported range"));
                    }
                    (Field::Number(n as u64), Some(end as u64))
                } else {
                    // GNU's numeric lookahead overwrites scan_val_int, including
                    // the decimal prefix stored before scanning a floating token.
                    let n = if matches!(peek.kind, TokenKind::Integer(_) | TokenKind::Float(_)) {
                        scanner
                            .spelling(peek)
                            .iter()
                            .take_while(|b| b.is_ascii_digit())
                            .try_fold(0u64, |n, b| {
                                n.checked_mul(10)?.checked_add((b - b'0') as u64)
                            })
                            .filter(|n| *n > 0 && *n <= i64::MAX as u64)
                            .ok_or_else(|| {
                                unsupported("grouping key is outside the supported range")
                            })?
                    } else {
                        n as u64
                    };
                    (Field::Number(n), None)
                }
            }
            TokenKind::Comma | TokenKind::End => {
                return Err(group_error("missing field", mode, scanner.utf8));
            }
            _ => {
                let mut message = b"invalid field '".to_vec();
                message.extend_from_slice(scanner.spelling(token));
                message.extend_from_slice(
                    format!("' for operation {}\n", quoted(mode, scanner.utf8)).as_bytes(),
                );
                return Err(failure(message));
            }
        };
        reserve(&mut spans, 1)?;
        spans.push((field, end));
        match scanner.peek()?.kind {
            TokenKind::Comma => {
                scanner.next()?;
            }
            TokenKind::Dash => return Err(group_error("invalid field range", mode, scanner.utf8)),
            TokenKind::Colon => return Err(group_error("invalid field pair", mode, scanner.utf8)),
            _ => break,
        }
    }
    Ok(spans)
}

fn expand_groups(spans: GroupSpecs) -> Result<Vec<Field>, Failure> {
    let mut keys = Vec::new();
    for (field, end) in spans {
        let count = match (&field, end) {
            (Field::Number(start), Some(end)) => end - start + 1,
            _ => 1,
        };
        keys.try_reserve(
            usize::try_from(count).map_err(|_| unsupported("grouping key allocation failed"))?,
        )
        .map_err(|_| unsupported("grouping key allocation failed"))?;
        match (field, end) {
            (Field::Number(start), Some(end)) => keys.extend((start..=end).map(Field::Number)),
            (field, _) => keys.push(field),
        }
    }
    Ok(keys)
}

fn check_option(scanner: &Scanner, token: Token) -> Result<bool, Failure> {
    let decoded = decode_identifier(scanner.spelling(token))?;
    match decoded.as_slice() {
        b"line" | b"lines" | b"row" | b"rows" => Ok(true),
        b"field" | b"fields" | b"column" | b"columns" | b"col" => Ok(false),
        value => {
            let mut message = b"invalid option ".to_vec();
            super::named_fields::quote_with(value, &mut message, scanner.utf8);
            message.extend_from_slice(b" for operation check\n");
            Err(failure(message))
        }
    }
}

fn check_options(scanner: &mut Scanner) -> Result<Mode, Failure> {
    let mut lines = 0;
    let mut fields = 0;
    while scanner.peek()?.kind != TokenKind::End {
        let first = scanner.next()?;
        let (is_line, value) = if let TokenKind::Integer(n) = first.kind {
            let option = scanner.next()?;
            (
                check_option(
                    scanner,
                    if option.kind == TokenKind::End {
                        first
                    } else {
                        option
                    },
                )?,
                n as u64,
            )
        } else {
            let is_line = check_option(scanner, first)?;
            let TokenKind::Integer(value) = scanner.next()?.kind else {
                return Err(failure(
                    b"number expected after option in operation 'check'\n".to_vec(),
                ));
            };
            (is_line, value as u64)
        };
        if value == 0 {
            return Err(failure(
                b"invalid value zero for lines/fields in operation 'check'\n".to_vec(),
            ));
        }
        let target = if is_line { &mut lines } else { &mut fields };
        if *target != 0 {
            let what = if is_line {
                "lines/rows"
            } else {
                "fields/columns"
            };
            return Err(failure(
                format!("number of {what} already set in operation 'check'\n").into_bytes(),
            ));
        }
        *target = value;
    }
    Ok(Mode::Check { lines, fields })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }
    fn error(parts: &[&str], status: i32, message: &[u8]) {
        let got = parse(&args(parts)).err().unwrap();
        assert_eq!(got.status, status);
        assert_eq!(got.message, message);
    }

    /// GNU quotes operation names with `quote()`: curly quotes under a UTF-8
    /// `LC_CTYPE` (op-parser.c), while a rejected field keeps literal '%s'.
    #[test]
    fn utf8_diagnostics_quote_operation_names_like_gnu() {
        let utf8_error = |parts: &[&str], group: Option<&str>| {
            let group = group.map(OsString::from);
            command_for_format(&args(parts), group.as_ref(), false, Profile::C, true)
                .err()
                .unwrap()
                .message
        };
        for (parts, group, message) in [
            (&["sum"][..], None, "missing field for operation ‘sum’\n"),
            (
                &["sum", "0"],
                None,
                "invalid field '0' for operation ‘sum’\n",
            ),
            (
                &["sum", "3-1"],
                None,
                "invalid field range for operation ‘sum’\n",
            ),
            (
                &["sum", "a-b"],
                None,
                "field range for ‘sum’ must be numeric\n",
            ),
            (
                &["round:x", "1"],
                None,
                "invalid parameter x for operation ‘round’\n",
            ),
            (
                &["pcov", "1"],
                None,
                "operation ‘pcov’ requires field pairs\n",
            ),
            (
                &["mean", "1:2"],
                None,
                "operation ‘mean’ cannot use pair of fields\n",
            ),
            (
                &["sum", "1", "md5", "1"],
                None,
                "conflicting operation found: expecting groupby operations, but found line operation ‘md5’\n",
            ),
            (
                &["sum", "1", "check"],
                None,
                "conflicting operation ‘check’\n",
            ),
            (
                &["sum", "1"],
                Some("0"),
                "invalid field '0' for operation ‘groupby’\n",
            ),
            (
                &["sum", "1"],
                Some("2-1"),
                "invalid field range for operation ‘groupby’\n",
            ),
        ] {
            assert_eq!(
                String::from_utf8(utf8_error(parts, group)).unwrap(),
                message,
                "{parts:?}"
            );
        }
        error(
            &["pcov", "1"],
            1,
            b"operation 'pcov' requires field pairs\n",
        );
    }

    #[test]
    fn count_keeps_selector_expansion_and_diagnostics() {
        assert_eq!(
            parse(&args(&["CoUnT", "2,1-2,item"])).ok().unwrap(),
            vec![
                (Kind::Count, Field::Number(2)),
                (Kind::Count, Field::Number(1)),
                (Kind::Count, Field::Number(2)),
                (Kind::Count, Field::Name(b"item".to_vec()))
            ]
        );
        let escaped = parse(&args(&["pcov", r"x\:left:y\:right"]));
        if let Err(error) = &escaped {
            panic!("{}", String::from_utf8_lossy(&error.message));
        }
        assert_eq!(
            escaped.ok().unwrap(),
            [Request {
                kind: Kind::Pcov,
                selector: Selector::Pair {
                    left: Field::Name(b"x:left".to_vec()),
                    right: Field::Name(b"y:right".to_vec()),
                },
            }]
        );
        error(
            &["count", "0"],
            1,
            b"invalid field '0' for operation 'count'\n",
        );
        error(&["count"], 1, b"missing field for operation 'count'\n");
    }

    #[test]
    fn percentile_endpoint_keeps_selectors_and_parameter_errors() {
        assert_eq!(
            parse(&args(&["perc:100", "2,1-2,value"])).ok().unwrap(),
            vec![
                (Kind::Percentile(100), Field::Number(2)),
                (Kind::Percentile(100), Field::Number(1)),
                (Kind::Percentile(100), Field::Number(2)),
                (Kind::Percentile(100), Field::Name(b"value".to_vec())),
            ]
        );
        error(&["perc:0", "1"], 1, b"invalid percentile value 0\n");
        error(&["perc:101", "1"], 1, b"invalid percentile value 101\n");
        error(
            &["perc:100.0", "1"],
            77,
            b"non-integer percentile parameters are outside the supported range\n",
        );
    }

    #[test]
    fn every_operation_name_parses_back_to_its_kind() {
        let parameterised = ["round", "floor", "ceil", "trunc", "frac", "getnum", "bin"]
            .into_iter()
            .chain([
                "strbin", "trimmean", "perc", "md5", "sha1", "sha256", "dirname",
            ]);
        let names = PLAIN_OPERATIONS
            .iter()
            .map(|(name, _)| std::str::from_utf8(name).unwrap())
            .chain(parameterised);
        for spelling in names {
            let kind = operation_kind(spelling.as_bytes()).expect(spelling);
            let canonical = name(kind);
            let again = operation_kind(canonical.as_bytes()).unwrap();
            assert_eq!(name(again), canonical, "{spelling}");
            let upper = spelling.to_ascii_uppercase();
            assert_eq!(operation_kind(upper.as_bytes()), Some(kind), "{spelling}");
        }
        for alias in ["echo", "uniq"] {
            assert_ne!(name(operation_kind(alias.as_bytes()).unwrap()), alias);
        }
        for other in ["transpose", "groupby", "", "sums", "mea"] {
            assert_eq!(operation_kind(other.as_bytes()), None, "{other}");
        }
    }

    #[test]
    fn collapse_uses_existing_selector_expansion_and_canonical_name() {
        assert_eq!(
            parse(&args(&["CoLlApSe", "2,1-2,name"])).ok().unwrap(),
            vec![
                (Kind::Collapse, Field::Number(2)),
                (Kind::Collapse, Field::Number(1)),
                (Kind::Collapse, Field::Number(2)),
                (Kind::Collapse, Field::Name(b"name".to_vec())),
            ]
        );
        error(
            &["collapse", "0"],
            1,
            b"invalid field '0' for operation 'collapse'\n",
        );
        error(
            &["collapse"],
            1,
            b"missing field for operation 'collapse'\n",
        );
    }

    #[test]
    fn expansion_order_and_argument_joining() {
        let expected = [
            (Kind::Sum, 3),
            (Kind::Sum, 1),
            (Kind::Sum, 2),
            (Kind::Sum, 1),
            (Kind::Mean, 2),
        ];
        for parts in [
            vec!["sum", "3,1-2,1", "mean", "2"],
            vec!["SUM 3,1-2,1 MeAn 2"],
            vec!["\t\u{b}sum", "", "3", ",", "1", "-", "2,01", "mean", "02"],
        ] {
            assert_eq!(
                parse(&args(&parts)).ok().unwrap(),
                expected.map(|(kind, field)| (kind, Field::Number(field)))
            );
        }
    }

    #[test]
    fn whitespace_depends_on_parser_phase() {
        for parts in [vec!["sum", "1 "], vec!["sum", "1", ""]] {
            error(&parts, 1, b"invalid operand ''\n");
        }
        for parts in [vec!["sum"], vec!["sum "]] {
            error(&parts, 1, b"missing field for operation 'sum'\n");
        }
    }

    #[test]
    fn numeric_spans_and_signed_limit() {
        assert_eq!(
            parse(&args(&["sum", "9223372036854775807"])).ok().unwrap(),
            [(Kind::Sum, Field::Number(i64::MAX as u64))]
        );
        error(
            &["sum", "9223372036854775808 mean 1"],
            1,
            b"invalid numeric value '9223372036854775808 mean 1'\n",
        );
        error(
            &["sum", "1e3 mean 2"],
            1,
            b"invalid numeric value '1e3 mean 2'\n",
        );
        error(
            &["MeAn", "01.5"],
            1,
            b"invalid field '01.5' for operation 'mean'\n",
        );
        error(
            &["sum", "000"],
            1,
            b"invalid field '000' for operation 'sum'\n",
        );
    }

    #[test]
    fn list_errors_precede_expansion_and_unvisited_forms() {
        assert_eq!(parse(&args(&["sum", "1-16"])).ok().unwrap().len(), 16);
        error(
            &["sum", "1-17,0"],
            1,
            b"invalid field '0' for operation 'sum'\n",
        );
        error(
            &["sum", "0", "unknown"],
            1,
            b"invalid field '0' for operation 'sum'\n",
        );
        error(
            &["sum", "2-1:2"],
            1,
            b"invalid field range for operation 'sum'\n",
        );
        for script in ["sum 1-17", "sum 1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1,1"] {
            assert_eq!(parse(&args(&[script])).ok().unwrap().len(), 17);
        }
        error(
            &["sum 1-9223372036854775807"],
            77,
            b"command memory allocation failed\n",
        );
    }

    #[test]
    fn comma_and_range_context_diagnostics() {
        for field in [",1", "1,", "1,,2", "1-,"] {
            error(&["sum", field], 1, b"missing field for operation 'sum'\n");
        }
        for field in ["1-", "2-2", "3-1", "1--2"] {
            error(
                &["sum", field],
                1,
                b"invalid field range for operation 'sum'\n",
            );
        }
    }

    #[test]
    fn long_scripts_preserve_grammar_and_existing_numerical_limits() {
        for parts in [
            vec!["sum", "1:2"],
            vec!["sum", "1e-50000"],
            vec!["sum", "+1"],
            vec![" "],
        ] {
            assert_eq!(parse(&args(&parts)).err().unwrap().status, 1);
        }
        assert_eq!(parse(&args(&["sum:1", "2"])).err().unwrap().status, 77);
        let long = format!("{}sum 1", " ".repeat(20_000));
        assert!(parse(&args(&[&long])).is_ok());
        let selector = format!("{}1", "0".repeat(20_000));
        assert_eq!(
            parse(&args(&["count", &selector])).ok().unwrap(),
            [(Kind::Count, Field::Number(1))]
        );
    }
    #[test]
    fn paired_selector_grammar_and_precedence_are_explicit() {
        assert_eq!(
            parse(&args(&[
                "pcov",
                "1:2,Left:Right",
                "spearson",
                "3:4",
                "dotprod",
                "5:6",
            ]))
            .ok()
            .unwrap(),
            [
                Request {
                    kind: Kind::Pcov,
                    selector: Selector::Pair {
                        left: Field::Number(1),
                        right: Field::Number(2),
                    },
                },
                Request {
                    kind: Kind::Pcov,
                    selector: Selector::Pair {
                        left: Field::Name(b"Left".to_vec()),
                        right: Field::Name(b"Right".to_vec()),
                    },
                },
                Request {
                    kind: Kind::Spearson,
                    selector: Selector::Pair {
                        left: Field::Number(3),
                        right: Field::Number(4),
                    },
                },
                Request {
                    kind: Kind::Dotprod,
                    selector: Selector::Pair {
                        left: Field::Number(5),
                        right: Field::Number(6),
                    },
                },
            ]
        );
        for form in [":2", "1:", "1::2"] {
            error(
                &["pcov", form],
                1,
                b"invalid field pair for operation 'pcov'\n",
            );
        }
        error(
            &["pcov", "1"],
            1,
            b"operation 'pcov' requires field pairs\n",
        );
        error(
            &["mean", "1:2"],
            1,
            b"operation 'mean' cannot use pair of fields\n",
        );
        error(
            &["pcov", "1:2,3"],
            1,
            b"operation 'pcov' requires field pairs\n",
        );
        error(
            &["pcov", "1-2:3"],
            1,
            b"operation 'pcov' requires field pairs\n",
        );
        error(&["pcov", "1:2-3"], 1, b"paired field ranges are invalid\n");
    }

    #[test]
    fn names_mix_with_numeric_lists_without_changing_order() {
        assert_eq!(
            parse(&args(&["sum", "Cost,1-2,Revenue", "mean", "Cost"]))
                .ok()
                .unwrap(),
            [
                (Kind::Sum, Field::Name(b"Cost".to_vec())),
                (Kind::Sum, Field::Number(1)),
                (Kind::Sum, Field::Number(2)),
                (Kind::Sum, Field::Name(b"Revenue".to_vec())),
                (Kind::Mean, Field::Name(b"Cost".to_vec())),
            ]
        );
        assert_eq!(
            parse(&args(&["sum", "sum"])).ok().unwrap(),
            [(Kind::Sum, Field::Name(b"sum".to_vec()))]
        );
    }

    #[test]
    fn field_and_operation_escapes_decode_bytes() {
        use std::os::unix::ffi::OsStringExt;
        for (raw, name) in [
            (b"a\\ b".as_slice(), b"a b".as_slice()),
            (b"\\123", b"123"),
            (b"\\\xff", b"\xff"),
            (b"a\\-b\\,c\\:d", b"a-b,c:d"),
            (b"a\\\\b", b"a\\b"),
        ] {
            assert_eq!(
                parse(&["sum".into(), OsString::from_vec(raw.to_vec())])
                    .ok()
                    .unwrap(),
                [(Kind::Sum, Field::Name(name.to_vec()))]
            );
        }
        error(&["sum", "a\\"], 1, b"backslash at end of identifier\n");
        assert_eq!(
            parse(&args(&[r"s\um", "a"])).ok().unwrap(),
            [(Kind::Sum, Field::Name(b"a".to_vec()))]
        );
        assert_eq!(
            parse(&args(&["sum", "a", r"m\ean", "b"]))
                .ok()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn named_ranges_fail_after_endpoint_syntax_before_expansion() {
        for field in ["a-b", "1-b", "a-2"] {
            error(
                &["sum", field],
                1,
                b"field range for 'sum' must be numeric\n",
            );
        }
        error(
            &["mean", "a-0"],
            1,
            b"invalid field '0' for operation 'mean'\n",
        );
        error(
            &["sum", "a-"],
            1,
            b"invalid field range for operation 'sum'\n",
        );
        error(
            &["sum", "1-17,a-b"],
            1,
            b"field range for 'sum' must be numeric\n",
        );
        assert_eq!(parse(&args(&["sum", "1-16,a"])).ok().unwrap().len(), 17);
    }

    #[test]
    fn identifier_limit_counts_decoded_bytes() {
        for count in [511, 512] {
            let raw = r"\a".repeat(count);
            if count == 511 {
                assert_eq!(
                    parse(&args(&["sum", &raw])).ok().unwrap(),
                    [(Kind::Sum, Field::Name(vec![b'a'; count]))]
                );
            } else {
                error(&["sum", &raw], 1, b"identifier name too long\n");
            }
        }
    }
}
