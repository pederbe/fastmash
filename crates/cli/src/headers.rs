//! Pure output-header rendering over logical output calls.

use super::{Failure, Kind, Selector, failure, records};

#[path = "header_output.rs"]
pub(super) mod output;

/// Logical output calls stay separate so transport can preserve failure behavior.
pub(super) enum Part<'a> {
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

pub(super) fn render_selectors(
    record: &[u8],
    requests: &[(Kind, Selector)],
    input: records::Separator,
    output: (u8, u8, fastmash_conversion::profile::Profile),
    named: bool,
    line: u64,
    mut emit: impl FnMut(Part<'_>),
) -> Result<(), Failure> {
    for (at, &(kind, selector)) in requests.iter().enumerate() {
        let field_span = |field| {
            records::field(record, field, input).map_err(|fields| {
                failure(
                    format!(
                        "invalid input: field {field} requested, line {line} has only {fields} fields\n"
                    )
                    .into_bytes(),
                )
            })
        };
        let (left, right) = match selector {
            Selector::Single(field) => (field_span(field)?, None),
            Selector::Pair { left, right } => (field_span(left)?, Some(field_span(right)?)),
        };
        emit(Part::Operation(match kind {
            Kind::Cut => b"cut",
            Kind::Rounding(kind) => kind.name().as_bytes(),
            Kind::Getnum(_) => b"getnum",
            Kind::Bin(_) => b"bin",
            Kind::Strbin(_) => b"strbin",
            Kind::Base64 => b"base64",
            Kind::Debase64 => b"debase64",
            Kind::Checksum(algorithm) => algorithm.name().as_bytes(),
            Kind::Path(kind) => kind.name().as_bytes(),
            Kind::Count => b"count",
            Kind::Countunique => b"countunique",
            Kind::Unique => b"unique",
            Kind::Collapse => b"collapse",
            Kind::First => b"first",
            Kind::Last => b"last",
            Kind::Rand => b"rand",
            Kind::Min => b"min",
            Kind::Max => b"max",
            Kind::Absmin => b"absmin",
            Kind::Absmax => b"absmax",
            Kind::Range => b"range",
            Kind::Sum => b"sum",
            Kind::Mean => b"mean",
            Kind::Geomean => b"geomean",
            Kind::Harmmean => b"harmmean",
            Kind::Ms => b"ms",
            Kind::Rms => b"rms",
            Kind::Median => b"median",
            Kind::Mode => b"mode",
            Kind::Antimode => b"antimode",
            Kind::Q1 => b"q1",
            Kind::Q3 => b"q3",
            Kind::Iqr => b"iqr",
            Kind::Percentile(_) => b"perc",
            Kind::Trimmean(_) => b"trimmean",
            Kind::Pvar => b"pvar",
            Kind::Svar => b"svar",
            Kind::Pstdev => b"pstdev",
            Kind::Sstdev => b"sstdev",
            Kind::Madraw => b"madraw",
            Kind::Mad => b"mad",
            Kind::Pskew => b"pskew",
            Kind::Sskew => b"sskew",
            Kind::Pkurt => b"pkurt",
            Kind::Skurt => b"skurt",
            Kind::Jarque => b"jarque",
            Kind::Dpo => b"dpo",
            Kind::Pcov => b"pcov",
            Kind::Scov => b"scov",
            Kind::Ppearson => b"ppearson",
            Kind::Spearson => b"spearson",
            Kind::Dotprod => b"dotprod",
        }));
        parameter(kind, output.2, &mut emit)?;
        let field_name = |span: fastmash_conversion::field_policy::FieldRange| {
            let name = &record[span.start..span.start + span.length];
            &name[..name.iter().position(|b| *b == 0).unwrap_or(name.len())]
        };
        let left_field = match selector {
            Selector::Single(field) | Selector::Pair { left: field, .. } => field,
        };
        if named {
            emit(Part::Name(field_name(left)));
            if let Some(right) = right {
                emit(Part::PairName(field_name(right)));
            }
        } else {
            let left = format!("field-{left_field}");
            emit(Part::Name(left.as_bytes()));
            if let Selector::Pair { right, .. } = selector {
                let right = format!("field-{right}");
                emit(Part::PairName(right.as_bytes()));
            }
        }
        emit(Part::Close);
        emit(Part::Separator(if at + 1 == requests.len() {
            output.1
        } else {
            output.0
        }));
    }
    Ok(())
}

pub(super) fn parameter(
    kind: Kind,
    profile: fastmash_conversion::profile::Profile,
    mut emit: impl FnMut(Part<'_>),
) -> Result<(), Failure> {
    match kind {
        Kind::Percentile(percent) => emit(Part::Parameter(percent.to_string().as_bytes())),
        Kind::Trimmean(trim) => emit(Part::Parameter(&trim.display(profile)?)),
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(output: &mut Vec<u8>, part: Part<'_>) {
        match part {
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
