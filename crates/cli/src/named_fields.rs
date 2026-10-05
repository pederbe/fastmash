//! Resolve requested names before output headers or numerical field access.
use super::{Failure, failure, records};

pub(super) struct Named {
    pub operation: usize,
    pub target: Target,
    pub name: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Target {
    Single,
    Left,
    Right,
}

/// Return all matches before the caller changes any operation or emits output.
#[cfg(test)]
pub(super) fn resolve(
    requests: &[Named],
    record: &[u8],
    delimiter: records::Separator,
) -> Result<Vec<(usize, Target, u64)>, Failure> {
    resolve_with(requests, record, delimiter, false)
}

pub(super) fn resolve_decoded(
    requests: &[Named],
    record: &super::csv_input::Record,
    utf8: bool,
) -> Result<Vec<(usize, Target, u64)>, Failure> {
    resolve_matching(requests, utf8, |name| {
        record
            .fields()
            .position(|field| super::headers::label(field) == name)
    })
}

fn resolve_matching(
    requests: &[Named],
    utf8: bool,
    mut find: impl FnMut(&[u8]) -> Option<usize>,
) -> Result<Vec<(usize, Target, u64)>, Failure> {
    let mut resolved = Vec::new();
    super::command_memory::reserve(&mut resolved, requests.len())?;
    for request in requests {
        let field = find(&request.name);
        let Some(field) = field else {
            let mut message = b"column name ".to_vec();
            quote_with(&request.name, &mut message, utf8);
            message.extend_from_slice(b" not found in input file\n");
            return Err(failure(message));
        };
        resolved.push((request.operation, request.target, field as u64 + 1));
    }
    Ok(resolved)
}

pub(super) fn resolve_with(
    requests: &[Named],
    record: &[u8],
    delimiter: records::Separator,
    utf8: bool,
) -> Result<Vec<(usize, Target, u64)>, Failure> {
    resolve_matching(requests, utf8, |name| {
        records::fields(record, delimiter).position(|span| {
            super::headers::label(&record[span.start..span.start + span.length]) == name
        })
    })
}

/// GNU's locale quotation in the admitted C/POSIX locale, over raw bytes.
pub(super) fn quote(name: &[u8], output: &mut Vec<u8>) {
    output.push(b'\'');
    for &byte in name {
        let escaped = match byte {
            b'\'' | b'\\' => Some(byte),
            7 => Some(b'a'),
            8 => Some(b'b'),
            9 => Some(b't'),
            10 => Some(b'n'),
            11 => Some(b'v'),
            12 => Some(b'f'),
            13 => Some(b'r'),
            _ => None,
        };
        if let Some(escaped) = escaped {
            output.extend_from_slice(&[b'\\', escaped]);
        } else if (32..=126).contains(&byte) {
            output.push(byte);
        } else {
            output.extend_from_slice(&[
                b'\\',
                b'0' + (byte >> 6),
                b'0' + ((byte >> 3) & 7),
                b'0' + (byte & 7),
            ]);
        }
    }
    output.push(b'\'');
}

pub(super) fn quote_with(name: &[u8], output: &mut Vec<u8>, utf8: bool) {
    if !utf8 {
        return quote(name, output);
    }
    output.extend_from_slice("‘".as_bytes());
    for chunk in name.utf8_chunks() {
        for ch in chunk.valid().chars() {
            if ch.is_control() || ch == '\\' {
                let mut encoded = [0; 4];
                let mut escaped = Vec::new();
                quote(ch.encode_utf8(&mut encoded).as_bytes(), &mut escaped);
                output.extend_from_slice(&escaped[1..escaped.len() - 1]);
            } else {
                if ch == '’' {
                    output.push(b'\\');
                }
                let mut encoded = [0; 4];
                output.extend_from_slice(ch.encode_utf8(&mut encoded).as_bytes());
            }
        }
        for byte in chunk.invalid() {
            output.extend_from_slice(&[
                b'\\',
                b'0' + (byte >> 6),
                b'0' + ((byte >> 3) & 7),
                b'0' + (byte & 7),
            ]);
        }
    }
    output.extend_from_slice("’".as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requests(names: &[&[u8]]) -> Vec<Named> {
        names
            .iter()
            .enumerate()
            .map(|(operation, name)| Named {
                operation,
                target: Target::Single,
                name: name.to_vec(),
            })
            .collect()
    }

    #[test]
    fn whitespace_lookup_uses_the_same_spans_as_numeric_selection() {
        let record = b" \ta\0tail \t b  a ";
        let matched = resolve(
            &requests(&[b"b", b"a"]),
            record,
            records::Separator::Whitespace,
        )
        .ok()
        .unwrap();
        assert_eq!(matched, [(0, Target::Single, 2), (1, Target::Single, 1)]);
        assert_eq!(
            records::field(record, 4, records::Separator::Whitespace)
                .unwrap()
                .length,
            0
        );
        assert!(resolve(&requests(&[b"a b"]), record, records::Separator::Whitespace).is_err());
    }

    #[test]
    fn exact_first_matches_keep_request_order_and_c_string_labels() {
        let names = requests(&[b"a", b"A", b"raw\xff", b"a"]);
        assert_eq!(
            resolve(
                &names,
                b"A|a\0tail|raw\xff|a",
                records::Separator::Literal(b'|')
            )
            .ok()
            .unwrap(),
            [
                (0, Target::Single, 2),
                (1, Target::Single, 1),
                (2, Target::Single, 3),
                (3, Target::Single, 2)
            ]
        );
        assert_eq!(
            resolve(
                &requests(&[b"a"]),
                b"|a|",
                records::Separator::Literal(b'|')
            )
            .ok()
            .unwrap(),
            [(0, Target::Single, 2)]
        );
    }

    #[test]
    fn missing_names_return_only_the_first_failure() {
        for record in [b"".as_slice(), b"other", b"\0a"] {
            let error = resolve(
                &requests(&[b"a", b"b"]),
                record,
                records::Separator::Literal(b'\t'),
            )
            .err()
            .unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, b"column name 'a' not found in input file\n");
        }
        assert!(
            resolve(
                &requests(&[b"a", b"missing"]),
                b"a",
                records::Separator::Literal(b'\t')
            )
            .is_err()
        );
        assert!(
            resolve(&[], b"", records::Separator::Literal(b'\t'))
                .ok()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn diagnostic_quoting_matches_c_locale_without_changing_lookup_bytes() {
        let name = b"a'\\\"\t\r\n\x07\x08\x0b\x0c\x01\x7f\xff";
        let mut actual = Vec::new();
        quote(name, &mut actual);
        assert_eq!(actual, b"'a\\'\\\\\"\\t\\r\\n\\a\\b\\v\\f\\001\\177\\377'");
    }
}
