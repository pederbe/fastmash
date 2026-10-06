//! Pure output-header rendering over logical output calls.

use super::{Failure, Kind, Selector, result_names::ResultNames};
#[cfg(test)]
use super::{missing_field, records};

/// Logical output calls stay separate so transport can preserve failure behavior.
#[derive(Clone, Copy)]
pub(super) enum Part<'a> {
    ResultName(&'a [u8]),
    Operation(&'static [u8]),
    /// One colon/percentage printf call, separate from the operation name.
    Parameter(&'a [u8]),
    /// One opening-parenthesis/name call, including when the displayed name is empty.
    Name(&'a [u8]),
    /// One comma/name call for the second side of a paired selector.
    PairName(&'a [u8]),
    Close,
    Separator(u8),
}

/// One complete logical label, before the output format encodes it.
pub(super) struct Header<'a> {
    parts: [Part<'a>; 5],
    length: usize,
}

impl<'a> Header<'a> {
    fn supplied(name: &'a [u8]) -> Self {
        Self {
            parts: [Part::ResultName(name); 5],
            length: 1,
        }
    }

    fn generated(
        operation: &'static [u8],
        parameter: Option<&'a [u8]>,
        left: &'a [u8],
        right: Option<&'a [u8]>,
    ) -> Self {
        let mut header = Self {
            parts: [Part::Close; 5],
            length: 0,
        };
        let mut push = |part| {
            header.parts[header.length] = part;
            header.length += 1;
        };
        push(Part::Operation(operation));
        if let Some(parameter) = parameter {
            push(Part::Parameter(parameter));
        }
        push(Part::Name(left));
        if let Some(right) = right {
            push(Part::PairName(right));
        }
        push(Part::Close);
        header
    }

    /// Preserve semantic calls, including empty names. Separators belong to callers.
    pub(super) fn emit(&self, emit: impl FnMut(Part<'a>)) {
        self.parts[..self.length].iter().copied().for_each(emit);
    }

    /// Borrow the complete Field as raw fragments for one synchronous callback.
    pub(super) fn with_fragments(&self, consume: impl FnOnce(&[&[u8]])) {
        let mut fragments: [&[u8]; 8] = [b""; 8];
        let mut length = 0;
        let mut push = |bytes| {
            fragments[length] = bytes;
            length += 1;
        };
        self.emit(|part| match part {
            Part::ResultName(bytes) | Part::Operation(bytes) => push(bytes),
            Part::Parameter(bytes) => {
                push(b":");
                push(bytes);
            }
            Part::Name(bytes) => {
                push(b"(");
                push(bytes);
            }
            Part::PairName(bytes) => {
                push(b",");
                push(bytes);
            }
            Part::Close => push(b")"),
            Part::Separator(_) => unreachable!("header separators belong to callers"),
        });
        consume(&fragments[..length]);
    }
}

/// The caller supplies a record and ordered requests.
/// Parts borrow their bytes only during the callback. Transport retains its own
/// errors, allowing later field checks to produce the primary input diagnostic.
#[cfg(test)]
pub(super) fn render(
    record: &[u8],
    requests: &[(Kind, u64)],
    input: records::Separator,
    output: u8,
    named: bool,
    line: u64,
    emit: impl FnMut(Part<'_>),
) -> Result<(), Failure> {
    let requests: Vec<_> = requests
        .iter()
        .map(|&(kind, field)| (kind, Selector::Single(field)))
        .collect();
    render_selectors(
        record,
        &requests,
        input,
        (output, b'\n', fastmash_conversion::profile::Profile::C),
        named,
        line,
        emit,
    )
}

#[cfg(test)]
pub(super) fn render_selectors(
    record: &[u8],
    requests: &[(Kind, Selector)],
    input: records::Separator,
    output: (u8, u8, fastmash_conversion::profile::Profile),
    named: bool,
    line: u64,
    emit: impl FnMut(Part<'_>),
) -> Result<(), Failure> {
    // One pass over the record serves every request.
    let mut index = records::FieldIndex::selecting(
        requests
            .iter()
            .flat_map(|&(_, selector)| match selector {
                Selector::Single(field) => [Some(field), None],
                Selector::Pair { left, right } => [Some(left), Some(right)],
            })
            .flatten(),
    );
    let field = |field| {
        index
            .field(record, field, input)
            .map(|span| &record[span.start..span.start + span.length])
            .map_err(|fields| missing_field(field, line, fields))
    };
    render_requests(
        requests,
        field,
        output,
        named,
        &ResultNames::default(),
        emit,
    )
}

/// The Output header's parts for `requests`, each field looked up through
/// `field` (which fails for a missing one) before any of its request's parts
/// are emitted: named from the fields' labels, or generated.
pub(super) fn render_requests<'r>(
    requests: &[(Kind, Selector)],
    field: impl FnMut(u64) -> Result<&'r [u8], Failure>,
    output: (u8, u8, fastmash_conversion::profile::Profile),
    named: bool,
    result_names: &ResultNames,
    mut emit: impl FnMut(Part<'_>),
) -> Result<(), Failure> {
    render_fields(
        requests,
        field,
        output,
        named,
        result_names,
        |header, separator| {
            header.emit(&mut emit);
            emit(Part::Separator(separator));
        },
    )
}

pub(super) fn render_fields<'r>(
    requests: &[(Kind, Selector)],
    mut field: impl FnMut(u64) -> Result<&'r [u8], Failure>,
    output: (u8, u8, fastmash_conversion::profile::Profile),
    named: bool,
    result_names: &ResultNames,
    mut emit: impl FnMut(Header<'_>, u8),
) -> Result<(), Failure> {
    for (at, &(kind, selector)) in requests.iter().enumerate() {
        let (left_field, right_field) = match selector {
            Selector::Single(field) => (field, None),
            Selector::Pair { left, right } => (left, Some(right)),
        };
        let left = field(left_field)?;
        let right = right_field.map(&mut field).transpose()?;
        let separator = if at + 1 == requests.len() {
            output.1
        } else {
            output.0
        };
        if let Some(name) = result_names.get(at) {
            emit(Header::supplied(name), separator);
        } else {
            let left_generated = (!named).then(|| generated(left_field));
            let right_generated = right_field.filter(|_| !named).map(generated);
            let parameter = match kind {
                Kind::Percentile(percent) => Some(percent.to_string().into_bytes()),
                Kind::Trimmean(trim) => Some(trim.display(output.2)?),
                _ => None,
            };
            emit(
                Header::generated(
                    kind.name().as_bytes(),
                    parameter.as_deref(),
                    left_generated
                        .as_ref()
                        .map_or_else(|| label(left), |label| label.as_bytes()),
                    right_generated
                        .as_ref()
                        .map(|label| label.as_bytes())
                        .or_else(|| right.map(label)),
                ),
                separator,
            );
        }
    }
    Ok(())
}

/// A field's label in an Output header: its bytes up to the first NUL, as
/// GNU prints its C strings.
pub(super) fn label(bytes: &[u8]) -> &[u8] {
    &bytes[..bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len())]
}

/// The label a generated Output header gives field `field`.
pub(super) fn generated(field: u64) -> String {
    format!("field-{field}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(output: &mut Vec<u8>, part: Part<'_>) {
        match part {
            Part::ResultName(bytes) => output.extend_from_slice(bytes),
            Part::Operation(bytes) => output.extend_from_slice(bytes),
            Part::Parameter(bytes) => {
                output.push(b':');
                output.extend_from_slice(bytes);
            }
            Part::Name(bytes) => {
                output.push(b'(');
                output.extend_from_slice(bytes);
            }
            Part::PairName(bytes) => {
                output.push(b',');
                output.extend_from_slice(bytes);
            }
            Part::Close => output.push(b')'),
            Part::Separator(byte) => output.push(byte),
        }
    }

    #[test]
    fn semantic_calls_and_fragments_preserve_complete_labels() {
        let requests = [
            (Kind::Sum, Selector::Single(1)),
            (Kind::Percentile(95), Selector::Single(2)),
            (Kind::Dotprod, Selector::Pair { left: 1, right: 2 }),
            (Kind::Sum, Selector::Single(3)),
        ];
        let mut result_names = ResultNames::default();
        result_names.add(b"4:daily,\"total\"\r\n\xff").ok().unwrap();
        for named in [false, true] {
            let mut semantic = Vec::new();
            let mut fragmented = Vec::new();
            let fields: [&[u8]; 3] = [b"a\0hidden", b"", b"value"];
            render_fields(
                &requests,
                |field| Ok(fields[field as usize - 1]),
                (b'|', b'\n', fastmash_conversion::profile::Profile::C),
                named,
                &result_names,
                |header, separator| {
                    header.emit(|part| collect(&mut semantic, part));
                    header.with_fragments(|fragments| {
                        for fragment in fragments {
                            fragmented.extend_from_slice(fragment);
                        }
                    });
                    semantic.push(separator);
                    fragmented.push(separator);
                },
            )
            .ok()
            .unwrap();
            let expected = if named {
                b"sum(a)|perc:95()|dotprod(a,)|daily,\"total\"\r\n\xff\n".as_slice()
            } else {
                b"sum(field-1)|perc:95(field-2)|dotprod(field-1,field-2)|daily,\"total\"\r\n\xff\n"
            };
            assert_eq!(semantic, expected);
            assert_eq!(fragmented, expected);
        }
    }

    #[test]
    fn empty_paired_names_keep_each_semantic_call() {
        let mut calls = Vec::new();
        render_selectors(
            b"\t",
            &[(Kind::Dotprod, Selector::Pair { left: 1, right: 2 })],
            records::Separator::Literal(b'\t'),
            (b'|', b'\n', fastmash_conversion::profile::Profile::C),
            true,
            1,
            |part| {
                calls.push(match part {
                    Part::Operation(bytes) => ("operation", bytes.to_vec()),
                    Part::Name(bytes) => ("name", bytes.to_vec()),
                    Part::PairName(bytes) => ("pair-name", bytes.to_vec()),
                    Part::Close => ("close", vec![]),
                    Part::Separator(byte) => ("separator", vec![byte]),
                    _ => panic!("unexpected semantic call"),
                });
            },
        )
        .ok()
        .unwrap();
        assert_eq!(
            calls,
            vec![
                ("operation", b"dotprod".to_vec()),
                ("name", vec![]),
                ("pair-name", vec![]),
                ("close", vec![]),
                ("separator", vec![b'\n']),
            ]
        );
    }

    #[test]
    fn count_uses_the_selected_header_name() {
        let mut out = Vec::new();
        assert!(
            render(
                b"item",
                &[(Kind::Count, 1)],
                records::Separator::Literal(b'\t'),
                b'\t',
                true,
                1,
                |p| collect(&mut out, p)
            )
            .is_ok()
        );
        assert_eq!(out, b"count(item)\n");
    }

    #[test]
    fn paired_headers_validate_both_sides_before_emitting_the_pair() {
        let requests = [
            (Kind::Sum, Selector::Single(1)),
            (Kind::Pcov, Selector::Pair { left: 2, right: 3 }),
            (Kind::Dotprod, Selector::Pair { left: 2, right: 3 }),
        ];
        let mut out = Vec::new();
        render_selectors(
            b"a\tb\tc",
            &requests,
            records::Separator::Literal(b'\t'),
            (b'\t', b'\n', fastmash_conversion::profile::Profile::C),
            true,
            1,
            |part| collect(&mut out, part),
        )
        .ok()
        .unwrap();
        assert_eq!(out, b"sum(a)\tpcov(b,c)\tdotprod(b,c)\n");

        let mut out = Vec::new();
        let error = render_selectors(
            b"a\tb",
            &requests,
            records::Separator::Literal(b'\t'),
            (b'\t', b'\n', fastmash_conversion::profile::Profile::C),
            false,
            1,
            |part| collect(&mut out, part),
        )
        .unwrap_err();
        assert_eq!(out, b"sum(field-1)\t");
        assert_eq!(
            error.message,
            b"invalid input: field 3 requested, line 1 has only 2 fields\n"
        );
    }

    #[test]
    fn percentile_keeps_percentage_as_a_separate_call() {
        let mut calls = Vec::new();
        let result = render(
            b"value",
            &[(Kind::Percentile(95), 1)],
            records::Separator::Literal(b'\t'),
            b'\t',
            true,
            1,
            |part| {
                calls.push(match part {
                    Part::ResultName(bytes) => ("result-name", bytes.to_vec()),
                    Part::Operation(bytes) => ("operation", bytes.to_vec()),
                    Part::Parameter(bytes) => ("parameter", bytes.to_vec()),
                    Part::Name(bytes) => ("name", bytes.to_vec()),
                    Part::PairName(bytes) => ("pair-name", bytes.to_vec()),
                    Part::Close => ("close", vec![]),
                    Part::Separator(byte) => ("separator", vec![byte]),
                })
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            calls,
            vec![
                ("operation", b"perc".to_vec()),
                ("parameter", b"95".to_vec()),
                ("name", b"value".to_vec()),
                ("close", vec![]),
                ("separator", vec![b'\n']),
            ]
        );
    }

    #[test]
    fn generated_names_keep_kind_order_and_duplicates() {
        let mut out = Vec::new();
        let result = render(
            b"1\t2\t3",
            &[
                (Kind::Geomean, 3),
                (Kind::Sum, 1),
                (Kind::Mean, 2),
                (Kind::Sum, 1),
            ],
            records::Separator::Literal(b'\t'),
            b'|',
            false,
            1,
            |part| collect(&mut out, part),
        );
        assert!(result.is_ok());
        assert_eq!(
            out,
            b"geomean(field-3)|sum(field-1)|mean(field-2)|sum(field-1)\n"
        );
    }

    #[test]
    fn named_fields_keep_spans_and_truncate_only_selected_display() {
        let mut out = Vec::new();
        let result = render(
            b"ignored,a\0hidden,\xff\r(),",
            &[(Kind::Mean, 2), (Kind::Sum, 3), (Kind::Geomean, 4)],
            records::Separator::Literal(b','),
            b'|',
            true,
            1,
            |part| collect(&mut out, part),
        );
        assert!(result.is_ok());
        assert_eq!(out, b"mean(a)|sum(\xff\r())|geomean()\n");
    }

    #[test]
    fn missing_later_field_preserves_prefix_and_physical_line() {
        for named in [false, true] {
            let mut out = Vec::new();
            let error = render(
                b"a\tb",
                &[(Kind::Sum, 1), (Kind::Mean, 3)],
                records::Separator::Literal(b'\t'),
                b'|',
                named,
                7,
                |part| collect(&mut out, part),
            )
            .err()
            .unwrap();
            assert_eq!(
                out,
                if named {
                    b"sum(a)|".as_slice()
                } else {
                    b"sum(field-1)|"
                }
            );
            assert_eq!(error.status, 1);
            assert_eq!(
                error.message,
                b"invalid input: field 3 requested, line 7 has only 2 fields\n"
            );
        }
    }

    #[test]
    fn blank_record_and_first_missing_selector_emit_nothing() {
        for (record, field, count) in [(b"".as_slice(), 1, 0), (b"a", 2, 1)] {
            for named in [false, true] {
                let mut out = Vec::new();
                let error = render(
                    record,
                    &[(Kind::Sum, field)],
                    records::Separator::Literal(b'\t'),
                    b',',
                    named,
                    9,
                    |part| collect(&mut out, part),
                )
                .err()
                .unwrap();
                assert!(out.is_empty());
                assert_eq!(error.status, 1);
                assert_eq!(
                    error.message,
                    format!(
                        "invalid input: field {field} requested, line 9 has only {count} fields\n"
                    )
                    .as_bytes()
                );
            }
        }
    }

    #[test]
    fn empty_name_keeps_all_four_logical_calls() {
        let mut calls = Vec::new();
        let result = render(
            b"\t",
            &[(Kind::Sum, 2)],
            records::Separator::Literal(b'\t'),
            b'|',
            true,
            1,
            |part| {
                calls.push(match part {
                    Part::ResultName(bytes) => ("result-name", bytes.to_vec()),
                    Part::Operation(bytes) => ("operation", bytes.to_vec()),
                    Part::Parameter(bytes) => ("parameter", bytes.to_vec()),
                    Part::Name(bytes) => ("name", bytes.to_vec()),
                    Part::PairName(bytes) => ("pair-name", bytes.to_vec()),
                    Part::Close => ("close", Vec::new()),
                    Part::Separator(byte) => ("separator", vec![byte]),
                });
            },
        );
        assert!(result.is_ok());
        assert_eq!(
            calls,
            vec![
                ("operation", b"sum".to_vec()),
                ("name", vec![]),
                ("close", vec![]),
                ("separator", vec![b'\n']),
            ]
        );
    }

    #[test]
    fn duplicate_names_and_literal_delimiters_are_not_escaped() {
        let mut out = Vec::new();
        let result = render(
            b"a|b\x80a|b",
            &[(Kind::Sum, 2), (Kind::Sum, 1)],
            records::Separator::Literal(0x80),
            b'|',
            true,
            1,
            |part| collect(&mut out, part),
        );
        assert!(result.is_ok());
        assert_eq!(out, b"sum(a|b)|sum(a|b)\n");
    }

    #[test]
    fn retained_transport_failure_does_not_skip_later_field_validation() {
        let mut failed = false;
        let mut calls = 0;
        let error = render(
            b"a",
            &[(Kind::Sum, 1), (Kind::Mean, 1), (Kind::Sum, 2)],
            records::Separator::Literal(b'\t'),
            b'\t',
            true,
            1,
            |_| {
                failed = true;
                calls += 1;
            },
        )
        .err()
        .unwrap();
        assert!(failed);
        assert_eq!(calls, 8);
        assert_eq!(error.status, 1);
        assert_eq!(
            error.message,
            b"invalid input: field 2 requested, line 1 has only 1 fields\n"
        );
    }

    #[test]
    fn maximum_repeated_label_matches_observed_transport_bound() {
        let mut out = Vec::new();
        let result = render(
            &vec![b'a'; 8192],
            &[(Kind::Geomean, 1); 16],
            records::Separator::Literal(b'\t'),
            b'\t',
            true,
            1,
            |part| collect(&mut out, part),
        );
        assert!(result.is_ok());
        assert_eq!(out.len(), 131232);
        let label = format!("geomean({})", "a".repeat(8192));
        let expected = vec![label; 16].join("\t") + "\n";
        assert_eq!(out, expected.as_bytes());
    }
}
