use std::{
    ffi::OsString,
    fs,
    io::Write,
    os::unix::ffi::OsStringExt,
    os::unix::process::CommandExt,
    process::{Command, Output, Stdio},
};

#[path = "support/executable.rs"]
mod executable;

#[path = "support/temp_dir.rs"]
mod temp_dir;

fn invoke(arguments: &[&str], input: &[u8], file: bool) -> Output {
    invoke_in(arguments, input, file, "C")
}

fn invoke_in(arguments: &[&str], input: &[u8], file: bool, locale: &str) -> Output {
    let arguments: Vec<_> = arguments.iter().map(OsString::from).collect();
    invoke_os(&arguments, input, file, locale)
}

fn invoke_os(arguments: &[OsString], input: &[u8], file: bool, locale: &str) -> Output {
    let mut command = Command::new(executable::fastmash());
    command
        .arg0("fastmash")
        .args(arguments)
        .env_clear()
        .env("LC_ALL", locale)
        .env("PATH", "/usr/bin:/bin")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if file {
        let directory =
            temp_dir::TempDir::new(&format!("health-{:?}", std::thread::current().id()));
        let path = directory.0.join("input");
        fs::write(&path, input).unwrap();
        command
            .stdin(fs::File::open(path).unwrap())
            .output()
            .unwrap()
    } else {
        let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(input);
        child.wait_with_output().unwrap()
    }
}

fn report(arguments: &[&str], input: &[u8]) -> String {
    let pipe = invoke(arguments, input, false);
    let file = invoke(arguments, input, true);
    assert!(pipe.status.success(), "{pipe:?}");
    assert!(pipe.stderr.is_empty(), "{pipe:?}");
    assert_eq!(pipe.stdout, file.stdout);
    assert_eq!(pipe.status.code(), file.status.code());
    assert_eq!(pipe.stderr, file.stderr);
    String::from_utf8(pipe.stdout).unwrap()
}

const TSV_HEADER: &[u8] = b"row_kind\tcategory\tbasis\tfield_index\tfield_name\tcount\tcount_unit\trecord_index\texpected\tobserved\tsample\tsample_truncated\texamples_omitted";
type TsvRow = [Vec<u8>; 13];

// Parse the wire format independently of the product's report and renderer types.
fn parse_tsv(bytes: &[u8]) -> Vec<TsvRow> {
    assert_eq!(bytes.last(), Some(&b'\n'));
    let mut records = bytes[..bytes.len() - 1].split(|&byte| byte == b'\n');
    assert_eq!(records.next(), Some(TSV_HEADER));
    records
        .map(|record| {
            let cells: Vec<_> = record
                .split(|&byte| byte == b'\t')
                .map(decode_tsv)
                .collect();
            let row: TsvRow = cells.try_into().expect("exactly thirteen TSV columns");
            for at in [3, 5, 7, 12] {
                if !row[at].is_empty() {
                    let spelling = std::str::from_utf8(&row[at]).unwrap();
                    let value: u64 = spelling.parse().unwrap();
                    assert_eq!(value.to_string(), spelling);
                }
            }
            row
        })
        .collect()
}

fn decode_tsv(mut bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    while let Some((&byte, rest)) = bytes.split_first() {
        assert!((0x20..=0x7e).contains(&byte), "unescaped string byte");
        bytes = rest;
        if byte != b'\\' {
            result.push(byte);
            continue;
        }
        let (&escape, rest) = bytes.split_first().expect("complete escape");
        bytes = rest;
        result.push(match escape {
            b'\\' => b'\\',
            b't' => b'\t',
            b'n' => b'\n',
            b'r' => b'\r',
            b'0' => 0,
            b'x' => {
                let hex = bytes.get(..2).expect("two hexadecimal digits");
                assert!(
                    hex.iter()
                        .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(b))
                );
                bytes = &bytes[2..];
                let decoded = u8::from_str_radix(std::str::from_utf8(hex).unwrap(), 16).unwrap();
                assert!(!matches!(decoded, 0 | b'\t' | b'\n' | b'\r' | 0x20..=0x7e));
                decoded
            }
            _ => panic!("unknown escape"),
        });
    }
    result
}

fn tsv_row(kind: &str, category: &str, cells: &[(usize, &[u8])]) -> TsvRow {
    let mut row = std::array::from_fn(|_| Vec::new());
    row[0] = kind.as_bytes().to_vec();
    row[1] = category.as_bytes().to_vec();
    for &(at, bytes) in cells {
        assert!(at >= 2);
        row[at] = bytes.to_vec();
    }
    row
}

fn summary(category: &str, cells: &[(usize, &[u8])]) -> TsvRow {
    tsv_row("summary", category, cells)
}

fn field_rows(
    index: &str,
    name: &[u8],
    counts: [&str; 6],
    basis: &str,
    expected: &str,
    presence: Option<&str>,
) -> Vec<TsvRow> {
    let mut rows = Vec::new();
    for (category, count) in [
        "absent_values",
        "empty_values",
        "missing_values",
        "integer_values",
        "other_number_values",
        "text_values",
    ]
    .into_iter()
    .zip(counts)
    {
        rows.push(tsv_row(
            "field",
            category,
            &[
                (2, b"observation"),
                (3, index.as_bytes()),
                (4, name),
                (5, count.as_bytes()),
                (6, b"cells"),
            ],
        ));
    }
    rows.push(tsv_row(
        "field",
        "type_expectation",
        &[
            (2, basis.as_bytes()),
            (3, index.as_bytes()),
            (4, name),
            (8, expected.as_bytes()),
        ],
    ));
    if let Some(presence) = presence {
        rows.push(tsv_row(
            "field",
            "presence_requirement",
            &[
                (2, b"declared"),
                (3, index.as_bytes()),
                (4, name),
                (8, presence.as_bytes()),
            ],
        ));
    }
    rows
}

#[test]
fn tsv_empty_input_has_the_exact_schema_and_unavailable_widths() {
    let rows = parse_tsv(report(&["health", "tsv", "tsv"], b"").as_bytes());
    assert_eq!(
        rows,
        [
            summary("schema_version", &[(9, b"1")]),
            summary(
                "data_records",
                &[(2, b"observation"), (5, b"0"), (6, b"data_records")]
            ),
            summary(
                "header_records",
                &[(2, b"observation"), (5, b"0"), (6, b"header_records")]
            ),
            summary(
                "accepted_records",
                &[(2, b"observation"), (5, b"0"), (6, b"records")]
            ),
            summary("baseline_width", &[]),
            summary("baseline_width_source", &[(9, b"unavailable")]),
            summary(
                "maximum_observed_width",
                &[(2, b"observation"), (6, b"fields")]
            ),
            summary("separator_kind", &[(9, b"literal")]),
            summary("separator", &[(9, b"\t")]),
            summary("record_terminator", &[(9, b"lf")]),
            summary("numeric_decimal_separator", &[(9, b".")]),
            summary("input_header", &[(9, b"no")]),
            summary("comment_filter", &[(9, b"none")]),
            summary("vnlog", &[(9, b"no")]),
            summary("record_numbering", &[(9, b"accepted_records_1based")]),
            summary(
                "example_limit",
                &[(2, b"observation"), (5, b"3"), (6, b"examples")]
            ),
            summary(
                "sample_byte_limit",
                &[(2, b"observation"), (5, b"128"), (6, b"bytes")]
            ),
        ]
    );
}

#[test]
fn tsv_healthy_fields_partition_records_and_show_supplied_expectations() {
    let rows = parse_tsv(
        report(
            &[
                "--header-in",
                "health",
                "required",
                "id",
                "tsv",
                "type",
                "price",
                "number",
                "validate",
                "examples",
                "2",
                "width",
                "3",
                "tsv",
            ],
            b"id\tprice\tcode\n1\t2.5\tfoo\n2\t3\tbar",
        )
        .as_bytes(),
    );
    assert_eq!(
        rows[1],
        summary(
            "data_records",
            &[(2, b"observation"), (5, b"2"), (6, b"data_records")]
        )
    );
    assert_eq!(
        rows[2],
        summary(
            "header_records",
            &[(2, b"observation"), (5, b"1"), (6, b"header_records")]
        )
    );
    assert_eq!(
        rows[3],
        summary(
            "accepted_records",
            &[(2, b"observation"), (5, b"3"), (6, b"records")]
        )
    );
    assert_eq!(
        rows[4],
        summary(
            "baseline_width",
            &[(2, b"declared"), (5, b"3"), (6, b"fields")]
        )
    );
    assert_eq!(
        rows[5],
        summary("baseline_width_source", &[(9, b"declared")])
    );
    assert_eq!(
        rows[6],
        summary(
            "maximum_observed_width",
            &[(2, b"observation"), (5, b"3"), (6, b"fields")]
        )
    );
    assert_eq!(rows[11], summary("input_header", &[(9, b"yes")]));
    assert_eq!(
        rows[15],
        summary(
            "example_limit",
            &[(2, b"observation"), (5, b"2"), (6, b"examples")]
        )
    );
    let expected = [
        field_rows(
            "1",
            b"id",
            ["0", "0", "0", "2", "0", "0"],
            "inferred",
            "integer",
            Some("required"),
        ),
        field_rows(
            "2",
            b"price",
            ["0", "0", "0", "1", "1", "0"],
            "declared",
            "number",
            None,
        ),
        field_rows(
            "3",
            b"code",
            ["0", "0", "0", "0", "0", "2"],
            "inferred",
            "text",
            None,
        ),
    ]
    .concat();
    assert_eq!(rows[17..], expected);
}

fn expected_finding(
    category: &str,
    basis: &str,
    field: (&str, &[u8]),
    count: &str,
    unit: &str,
    expected: &[u8],
    omitted: &str,
) -> TsvRow {
    tsv_row(
        "finding",
        category,
        &[
            (2, basis.as_bytes()),
            (3, field.0.as_bytes()),
            (4, field.1),
            (5, count.as_bytes()),
            (6, unit.as_bytes()),
            (8, expected),
            (12, omitted.as_bytes()),
        ],
    )
}

fn with_examples(rows: &mut Vec<TsvRow>, finding: TsvRow, examples: &[(&str, &str, &[u8], bool)]) {
    rows.push(finding.clone());
    for &(record, observed, sample, truncated) in examples {
        let mut example = finding.clone();
        example[0] = b"example".to_vec();
        for at in [5, 6, 12] {
            example[at].clear();
        }
        example[7] = record.as_bytes().to_vec();
        example[9] = observed.as_bytes().to_vec();
        example[10] = sample.to_vec();
        example[11] = if truncated {
            b"yes".to_vec()
        } else {
            b"no".to_vec()
        };
        rows.push(example);
    }
}

#[test]
fn tsv_combined_findings_follow_the_complete_hand_calculated_contract() {
    // Five data records, with widths 4/4/4/0/5, and one four-field header.
    let input = b"a\tb\tb\t\n1\t2\tNA\t\n3\t4.5\tx\t4\n5\tbad\tN/A\t6\n\n7\t8\tNaN\toops\textra\n";
    let output = validated(
        &[
            "--header-in",
            "health",
            "width",
            "3",
            "type",
            "2",
            "integer",
            "type",
            "3",
            "text",
            "required",
            "1",
            "nonmissing",
            "3",
            "examples",
            "3",
            "tsv",
        ],
        input,
        1,
    );
    let rows = parse_tsv(output.as_bytes());
    assert_eq!(
        rows[1],
        summary(
            "data_records",
            &[(2, b"observation"), (5, b"5"), (6, b"data_records")]
        )
    );
    assert_eq!(
        rows[2],
        summary(
            "header_records",
            &[(2, b"observation"), (5, b"1"), (6, b"header_records")]
        )
    );
    assert_eq!(
        rows[3],
        summary(
            "accepted_records",
            &[(2, b"observation"), (5, b"6"), (6, b"records")]
        )
    );
    assert_eq!(
        rows[4],
        summary(
            "baseline_width",
            &[(2, b"declared"), (5, b"3"), (6, b"fields")]
        )
    );
    assert_eq!(
        rows[6],
        summary(
            "maximum_observed_width",
            &[(2, b"observation"), (5, b"5"), (6, b"fields")]
        )
    );
    let mut expected = [
        field_rows(
            "1",
            b"a",
            ["1", "0", "0", "4", "0", "0"],
            "inferred",
            "integer",
            Some("required"),
        ),
        field_rows(
            "2",
            b"b",
            ["1", "0", "0", "2", "1", "1"],
            "declared",
            "integer",
            None,
        ),
        field_rows(
            "3",
            b"b",
            ["1", "0", "3", "0", "0", "1"],
            "declared",
            "text",
            Some("nonmissing"),
        ),
        field_rows(
            "4",
            b"",
            ["1", "1", "0", "2", "0", "1"],
            "inferred",
            "integer",
            None,
        ),
        field_rows(
            "5",
            b"",
            ["4", "0", "0", "0", "0", "1"],
            "inferred",
            "text",
            None,
        ),
    ]
    .concat();
    with_examples(
        &mut expected,
        expected_finding(
            "header_width_mismatch",
            "declared",
            ("", b""),
            "1",
            "header_records",
            b"3",
            "0",
        ),
        &[("1", "4", b"a\tb\tb\t", false)],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "width_mismatch",
            "declared",
            ("", b""),
            "5",
            "data_records",
            b"3",
            "2",
        ),
        &[
            ("2", "4", b"1\t2\tNA\t", false),
            ("3", "4", b"3\t4.5\tx\t4", false),
            ("4", "4", b"5\tbad\tN/A\t6", false),
        ],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "blank_header",
            "observation",
            ("4", b""),
            "1",
            "header_fields",
            b"",
            "0",
        ),
        &[("1", "blank", b"", false)],
    );
    for index in ["2", "3"] {
        with_examples(
            &mut expected,
            expected_finding(
                "duplicate_header",
                "observation",
                (index, b"b"),
                "1",
                "header_fields",
                b"",
                "0",
            ),
            &[("1", "duplicate", b"b", false)],
        );
    }
    for (index, name) in [("1", b"a".as_slice()), ("2", b"b"), ("3", b"b"), ("4", b"")] {
        with_examples(
            &mut expected,
            expected_finding(
                "absent_field",
                "observation",
                (index, name),
                "1",
                "cells",
                b"",
                "0",
            ),
            &[("5", "absent", b"", false)],
        );
    }
    with_examples(
        &mut expected,
        expected_finding(
            "absent_field",
            "observation",
            ("5", b""),
            "4",
            "cells",
            b"",
            "1",
        ),
        &[
            ("2", "absent", b"", false),
            ("3", "absent", b"", false),
            ("4", "absent", b"", false),
        ],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "empty_field",
            "observation",
            ("4", b""),
            "1",
            "cells",
            b"",
            "0",
        ),
        &[("2", "empty", b"", false)],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "missing_indicator",
            "observation",
            ("3", b"b"),
            "3",
            "cells",
            b"",
            "0",
        ),
        &[
            ("2", "missing_indicator", b"NA", false),
            ("4", "missing_indicator", b"N/A", false),
            ("6", "missing_indicator", b"NaN", false),
        ],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "mixed_types",
            "observation",
            ("2", b"b"),
            "1",
            "fields",
            b"",
            "1",
        ),
        &[
            ("2", "integer", b"2", false),
            ("3", "other_number", b"4.5", false),
            ("4", "text", b"bad", false),
        ],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "mixed_types",
            "observation",
            ("4", b""),
            "1",
            "fields",
            b"",
            "0",
        ),
        &[
            ("3", "integer", b"4", false),
            ("4", "integer", b"6", false),
            ("6", "text", b"oops", false),
        ],
    );
    // Basis comes before position, so inferred field 4 precedes declared field 2.
    with_examples(
        &mut expected,
        expected_finding(
            "type_mismatch",
            "inferred",
            ("4", b""),
            "1",
            "cells",
            b"integer",
            "0",
        ),
        &[("6", "text", b"oops", false)],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "type_mismatch",
            "declared",
            ("2", b"b"),
            "2",
            "cells",
            b"integer",
            "0",
        ),
        &[
            ("3", "other_number", b"4.5", false),
            ("4", "text", b"bad", false),
        ],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "presence_mismatch",
            "declared",
            ("1", b"a"),
            "1",
            "cells",
            b"required",
            "0",
        ),
        &[("5", "absent", b"", false)],
    );
    with_examples(
        &mut expected,
        expected_finding(
            "presence_mismatch",
            "declared",
            ("3", b"b"),
            "4",
            "cells",
            b"nonmissing",
            "1",
        ),
        &[
            ("2", "missing_indicator", b"NA", false),
            ("4", "missing_indicator", b"N/A", false),
            ("5", "absent", b"", false),
        ],
    );
    assert_eq!(rows[17..], expected);
}

#[test]
fn tsv_header_only_zero_width_and_sparse_rules_do_not_invent_fields_or_records() {
    let header = parse_tsv(report(&["-H", "health", "tsv"], b"a\tb\n").as_bytes());
    assert_eq!(
        header[4],
        summary(
            "baseline_width",
            &[(2, b"inferred"), (5, b"2"), (6, b"fields")]
        )
    );
    assert_eq!(
        header[5],
        summary("baseline_width_source", &[(9, b"header")])
    );
    assert_eq!(
        header[6],
        summary(
            "maximum_observed_width",
            &[(2, b"observation"), (6, b"fields")]
        )
    );
    assert_eq!(
        header[17..],
        [
            field_rows("1", b"a", ["0"; 6], "observation", "undetermined", None),
            field_rows("2", b"b", ["0"; 6], "observation", "undetermined", None),
        ]
        .concat()
    );

    let empty = parse_tsv(validated(&["health", "tsv", "required", "5"], b"", 0).as_bytes());
    assert_eq!(empty[4], summary("baseline_width", &[]));
    assert_eq!(
        empty[17..],
        field_rows(
            "5",
            b"",
            ["0"; 6],
            "observation",
            "undetermined",
            Some("required")
        )
    );

    let zero = parse_tsv(validated(&["health", "tsv", "width", "0"], b"\n", 0).as_bytes());
    assert_eq!(zero.len(), 17);
    assert_eq!(
        zero[4],
        summary(
            "baseline_width",
            &[(2, b"declared"), (5, b"0"), (6, b"fields")]
        )
    );
    assert_eq!(
        zero[6],
        summary(
            "maximum_observed_width",
            &[(2, b"observation"), (5, b"0"), (6, b"fields")]
        )
    );

    let sparse = parse_tsv(
        validated(
            &[
                "health",
                "tsv",
                "type",
                "5",
                "number",
                "nonmissing",
                "5",
                "examples",
                "0",
            ],
            b"1\t2\n3\t4\n",
            1,
        )
        .as_bytes(),
    );
    let mut expected = [
        field_rows(
            "1",
            b"",
            ["0", "0", "0", "2", "0", "0"],
            "inferred",
            "integer",
            None,
        ),
        field_rows(
            "2",
            b"",
            ["0", "0", "0", "2", "0", "0"],
            "inferred",
            "integer",
            None,
        ),
        field_rows(
            "5",
            b"",
            ["2", "0", "0", "0", "0", "0"],
            "declared",
            "number",
            Some("nonmissing"),
        ),
    ]
    .concat();
    expected.push(expected_finding(
        "absent_field",
        "observation",
        ("5", b""),
        "2",
        "cells",
        b"",
        "2",
    ));
    expected.push(expected_finding(
        "presence_mismatch",
        "declared",
        ("5", b""),
        "2",
        "cells",
        b"nonmissing",
        "2",
    ));
    assert_eq!(sparse[17..], expected);
    assert_eq!(
        sparse[5],
        summary("baseline_width_source", &[(9, b"modal")])
    );
}

#[test]
fn tsv_mixed_witness_limits_use_cell_candidates_and_preserve_advisory_validation() {
    let input = b"1\n2\n3\n4\nword\n5\nNA\n";
    for (limit, records) in [
        ("0", vec![]),
        ("1", vec!["1"]),
        ("2", vec!["1", "5"]),
        ("3", vec!["1", "2", "5"]),
    ] {
        let rows = parse_tsv(
            validated(
                &["health", "tsv", "width", "1", "examples", limit],
                input,
                0,
            )
            .as_bytes(),
        );
        assert_eq!(
            rows[17..24],
            field_rows(
                "1",
                b"",
                ["0", "0", "1", "5", "0", "1"],
                "inferred",
                "integer",
                None
            )
        );
        let mixed: Vec<_> = rows
            .iter()
            .filter(|row| row[1] == b"mixed_types")
            .cloned()
            .collect();
        let omitted = (6 - records.len()).to_string();
        let mut expected = Vec::new();
        let examples: Vec<_> = records
            .iter()
            .map(|&record| {
                if record == "5" {
                    (record, "text", b"word".as_slice(), false)
                } else {
                    (record, "integer", record.as_bytes(), false)
                }
            })
            .collect();
        with_examples(
            &mut expected,
            expected_finding(
                "mixed_types",
                "observation",
                ("1", b""),
                "1",
                "fields",
                b"",
                &omitted,
            ),
            &examples,
        );
        assert_eq!(mixed, expected);
        assert!(
            !rows
                .iter()
                .any(|row| row[0] == b"finding" && row[2] == b"declared")
        );
    }
}

#[test]
fn tsv_full_binary_labels_and_bounded_samples_decode_without_ambiguity() {
    for terminator in [b'\n', 0] {
        let mut name = b"\\x41\\t\\0\t\r\x1f\x7f\xff".to_vec();
        name.push(if terminator == 0 { b'\n' } else { 0 });
        name.extend_from_slice(&[b'q'; 180]);
        let mut header = name.clone();
        header.push(b',');
        header.extend_from_slice(&name);
        let mut value = b"text\\n\\xFF\t\r\x80".to_vec();
        value.push(if terminator == 0 { b'\n' } else { 0 });
        value.extend_from_slice(&[b'v'; 180]);
        let mut data = value.clone();
        data.extend_from_slice(b",NA");
        let mut input = header.clone();
        input.push(terminator);
        input.extend_from_slice(&data);
        input.push(terminator);
        let mut arguments = vec![
            "-H", "-t", ",", "health", "tsv", "type", "1", "integer", "width", "1",
        ];
        if terminator == 0 {
            arguments.insert(0, "-z");
        }
        let output = validated(&arguments, &input, 1);
        assert!(!output.as_bytes().contains(&0));
        let rows = parse_tsv(output.as_bytes());
        for row in &rows {
            if !row[3].is_empty() {
                assert_eq!(row[4], name, "full field metadata");
            }
            if row[0] == b"example" {
                let sample = match row[1].as_slice() {
                    b"header_width_mismatch" => &header[..128],
                    b"width_mismatch" => &data[..128],
                    b"duplicate_header" => &name[..128],
                    b"type_mismatch" => &value[..128],
                    b"missing_indicator" => b"NA",
                    _ => panic!("unexpected finding"),
                };
                assert_eq!(row[10], sample);
                assert_eq!(
                    row[11],
                    if sample == b"NA" {
                        b"no".as_slice()
                    } else {
                        b"yes"
                    }
                );
            }
        }
        assert_eq!(
            rows[9],
            summary(
                "record_terminator",
                &[(9, if terminator == 0 { b"nul" } else { b"lf" })]
            )
        );
        assert_eq!(rows.iter().filter(|row| row[0] == b"example").count(), 6);
    }
}

#[test]
fn tsv_input_settings_and_accepted_locations_follow_existing_filters() {
    let args = ["-W", "-C", "-z", "-H", "health", "tsv", "width", "2"];
    let input = b"# skip\0id value\0# more\0one 1,5\0two nope\0";
    let pipe = invoke_in(&args, input, false, "de_DE.UTF-8");
    let file = invoke_in(&args, input, true, "de_DE.UTF-8");
    assert!(pipe.status.success() && pipe.stderr.is_empty(), "{pipe:?}");
    assert_eq!(pipe.stdout, file.stdout);
    assert_eq!(pipe.status.code(), file.status.code());
    assert_eq!(pipe.stderr, file.stderr);
    let rows = parse_tsv(&pipe.stdout);
    assert_eq!(
        rows[1],
        summary(
            "data_records",
            &[(2, b"observation"), (5, b"2"), (6, b"data_records")]
        )
    );
    assert_eq!(
        rows[3],
        summary(
            "accepted_records",
            &[(2, b"observation"), (5, b"3"), (6, b"records")]
        )
    );
    assert_eq!(rows[7], summary("separator_kind", &[(9, b"whitespace")]));
    assert_eq!(rows[8], summary("separator", &[]));
    assert_eq!(rows[9], summary("record_terminator", &[(9, b"nul")]));
    assert_eq!(rows[10], summary("numeric_decimal_separator", &[(9, b",")]));
    assert_eq!(rows[12], summary("comment_filter", &[(9, b"comments")]));
    assert_eq!(rows[13], summary("vnlog", &[(9, b"no")]));
    assert_eq!(
        rows[24..31],
        field_rows(
            "2",
            b"value",
            ["0", "0", "0", "0", "1", "1"],
            "observation",
            "undetermined",
            None
        )
    );
    let examples: Vec<_> = rows.iter().filter(|row| row[0] == b"example").collect();
    assert_eq!(
        examples
            .iter()
            .map(|row| row[7].as_slice())
            .collect::<Vec<_>>(),
        [b"2", b"3"]
    );

    let vnlog = parse_tsv(
        validated(
            &["--vnlog", "health", "tsv", "width", "1"],
            b"## skip\n#! metadata\n # a b \t \n1 2 # note\n",
            1,
        )
        .as_bytes(),
    );
    assert_eq!(vnlog[7], summary("separator_kind", &[(9, b"whitespace")]));
    assert_eq!(vnlog[11], summary("input_header", &[(9, b"yes")]));
    assert_eq!(vnlog[12], summary("comment_filter", &[(9, b"vnlog")]));
    assert_eq!(vnlog[13], summary("vnlog", &[(9, b"yes")]));
    let samples: Vec<_> = vnlog
        .iter()
        .filter(|row| row[0] == b"example")
        .map(|row| (&row[7], &row[10]))
        .collect();
    assert_eq!(
        samples,
        [
            (&b"1".to_vec(), &b" # a b \t ".to_vec()),
            (&b"2".to_vec(), &b"1 2 # note".to_vec())
        ]
    );
}

#[test]
fn supplied_width_overrides_the_header_and_reports_both_record_kinds() {
    let output = report(&["-H", "health", "width", "1"], b"a\tb\n1\n2\t3\n");
    assert!(output.contains("Baseline width: 1 fields (declared, declared)\n"));
    assert!(output.contains("header_width_mismatch [declared, violation]: 1 header_records; expected 1 fields; examples retained 1, omitted 0\n"));
    assert!(output.contains("width_mismatch [declared, violation]: 1 data_records; expected 1 fields; examples retained 1, omitted 0\n"));
    assert!(
        output.contains("accepted record 1: observed 2 fields; sample \"a\\tb\"; truncated no\n")
    );
    assert!(
        output.find("header_width_mismatch").unwrap() < output.find("width_mismatch [").unwrap()
    );
    assert!(!output.contains("[inferred, advisory]:"));
}

#[test]
fn nonmissing_is_one_effective_presence_rule_and_validation_preserves_the_report() {
    let input = b"1\tNA\n2\t\n3\n4\tcode\n";
    let args = [
        "health",
        "required",
        "2",
        "nonmissing",
        "2",
        "examples",
        "2",
    ];
    let ordinary = report(&args, input);
    assert!(ordinary.contains("presence_requirement nonmissing (declared)\n"));
    assert!(ordinary.contains("presence_mismatch [declared, violation]: 3 cells; expected nonmissing; field 2, name \"\"; examples retained 2, omitted 1\n"));
    assert_eq!(ordinary.matches("presence_mismatch [").count(), 1);
    let presence = ordinary.split("presence_mismatch [").nth(1).unwrap();
    assert!(
        presence.contains("accepted record 1, field 2: observed missing_indicator; sample \"NA\"")
    );
    assert!(presence.contains("accepted record 2, field 2: observed empty; sample \"\""));
    assert!(!presence.contains("accepted record 3, field 2:"));
    for file in [false, true] {
        let validation = invoke(
            &[
                "health",
                "validate",
                "required",
                "2",
                "nonmissing",
                "2",
                "examples",
                "2",
            ],
            input,
            file,
        );
        assert_eq!(validation.status.code(), Some(1));
        assert!(validation.stderr.is_empty());
        assert_eq!(validation.stdout, ordinary.as_bytes());
    }
    let required = report(&["health", "required", "2"], input);
    assert!(required.contains("presence_requirement required (declared)\n"));
    assert!(
        required.contains(
            "presence_mismatch [declared, violation]: 2 cells; expected required; field 2"
        )
    );
}

#[test]
fn supplied_integer_overrides_inference_and_retains_late_fraction_examples() {
    let input = b"1\n2\n3\n4\n1.5\nword\n2e0\nNA\n\n";
    for limit in [0, 1, 2, 3] {
        let output = report(
            &[
                "health",
                "type",
                "1",
                "integer",
                "examples",
                &limit.to_string(),
            ],
            input,
        );
        assert!(output.contains("type_expectation integer (declared)\n"));
        assert!(output.contains("mixed_types [observation, advisory]: 1 fields; field 1"));
        assert!(output.contains(&format!("type_mismatch [declared, violation]: 3 cells; expected integer; field 1, name \"\"; examples retained {limit}, omitted {}\n", 3 - limit)));
        assert!(!output.contains("type_mismatch [inferred"));
        let mismatch = output.split("type_mismatch [").nth(1).unwrap();
        for (n, record, kind, sample) in [
            (1, 5, "other_number", "1.5"),
            (2, 6, "text", "word"),
            (3, 7, "other_number", "2e0"),
        ] {
            assert_eq!(
                mismatch.contains(&format!(
                    "accepted record {record}, field 1: observed {kind}; sample \"{sample}\""
                )),
                n <= limit
            );
        }
    }
    let text = report(&["health", "type", "1", "text"], input);
    assert!(text.contains("type_expectation text (declared)\n"));
    assert!(!text.contains("type_mismatch"));
    let number = report(&["health", "type", "1", "number"], input);
    assert!(
        number.contains("type_mismatch [declared, violation]: 1 cells; expected number; field 1")
    );
}

fn validated(arguments: &[&str], input: &[u8], expected_status: i32) -> String {
    let ordinary = report(arguments, input);
    let mut arguments = arguments.to_vec();
    arguments.push("validate");
    for file in [false, true] {
        let validation = invoke(&arguments, input, file);
        assert_eq!(
            validation.status.code(),
            Some(expected_status),
            "{validation:?}"
        );
        assert!(validation.stderr.is_empty(), "{validation:?}");
        assert_eq!(validation.stdout, ordinary.as_bytes());
    }
    ordinary
}

#[test]
fn sparse_declarations_track_only_observed_header_and_explicit_positions() {
    let output = validated(
        &["health", "required", "5", "width", "100", "examples", "2"],
        b"a\tb\nc\nd\te\tf\tg\n",
        1,
    );
    for (field, absent) in [(1, 0), (2, 1), (3, 2), (4, 2), (5, 3)] {
        assert!(output.contains(&format!(
            "field {field}, name \"\": absent_values {absent};"
        )));
    }
    assert_eq!(output.matches("\n  field ").count(), 5);
    assert!(!output.contains("field 6,"));
    assert!(
        output.contains(
            "presence_mismatch [declared, violation]: 3 cells; expected required; field 5"
        )
    );
    let short = validated(&["health", "required", "5"], b"a\tb\nc\td\n", 1);
    assert_eq!(short.matches("\n  field ").count(), 3);
    assert!(!short.contains("field 3,"));
    assert!(!short.contains("field 4,"));
    for field in [1, 2, 5] {
        assert!(short.contains(&format!("\n  field {field},")));
    }
    let extreme = validated(&["health", "required", "9223372036854775807"], b"a\tb\n", 1);
    assert_eq!(extreme.matches("\n  field ").count(), 3);
    assert!(extreme.contains("field 9223372036854775807, name \"\": absent_values 1;"));
    let empty = validated(&["health", "required", "5"], b"", 0);
    assert_eq!(empty.matches("\n  field ").count(), 1);
    assert!(empty.contains("field 5, name \"\": absent_values 0;"));
    assert!(!empty.contains("presence_mismatch"));
}

#[test]
fn named_declarations_use_unique_full_byte_labels_and_normalize_aliases() {
    let output = validated(
        &[
            "-H",
            "health",
            "type",
            "2",
            "integer",
            "type",
            "b",
            "integer",
            "nonmissing",
            "b",
            "required",
            "2",
        ],
        b"a\tb\ntext\t1.5\nx\tNA\n",
        1,
    );
    assert_eq!(output.matches("type_mismatch [").count(), 1);
    assert_eq!(output.matches("presence_mismatch [").count(), 1);
    assert!(output.contains("field 2, name \"b\""));
    for declarations in [
        ["type", "2", "integer", "type", "b", "number"],
        ["type", "b", "number", "type", "2", "integer"],
    ] {
        let mut args = vec!["-H", "health"];
        args.extend(declarations);
        let error = invoke(&args, b"a\tb\n1\t2\n", false);
        assert_eq!(error.status.code(), Some(1));
        assert!(error.stdout.is_empty());
        assert_eq!(
            error.stderr,
            b"fastmash: health: conflicting types for field 2\n"
        );
    }
    let full_bytes = validated(
        &["-H", "health", "type", "a", "integer"],
        b"a\0tail\ta\ntext\t1\n",
        0,
    );
    assert!(full_bytes.contains("field 1, name \"a\\0tail\""));
    assert!(full_bytes.contains("field 2, name \"a\": absent_values 0;"));
    assert!(full_bytes.contains("type_expectation integer (declared)"));
    assert!(!full_bytes.contains("type_mismatch"));
    let binary = validated(
        &["-H", "health", "required", "a\\-b", "type", "\\01", "text"],
        b"a-b\t01\nvalue\tNA\n",
        0,
    );
    assert!(!binary.contains("presence_mismatch"));
    let args: Vec<_> = [
        b"-H".as_slice(),
        b"health",
        b"required",
        b"raw\\\xff",
        b"validate",
    ]
    .into_iter()
    .map(|arg| OsString::from_vec(arg.to_vec()))
    .collect();
    for file in [false, true] {
        let binary = invoke_os(&args, b"raw\xff\nvalue\n", file, "C");
        assert_eq!(binary.status.code(), Some(0), "{binary:?}");
        assert!(binary.stderr.is_empty());
        assert!(
            String::from_utf8(binary.stdout)
                .unwrap()
                .contains("field 1, name \"raw\\xFF\"")
        );
    }
}

#[test]
fn missing_or_ambiguous_names_are_command_errors_but_positions_remain_available() {
    for (args, input, expected) in [
        (
            vec!["health", "required", "name"],
            b"name\n1\n".as_slice(),
            "named fields require an Input header",
        ),
        (
            vec!["-H", "health", "required", "name"],
            b"other\n1\n",
            "not found in Input header",
        ),
        (
            vec!["-H", "health", "required", "a"],
            b"a\0tail\n1\n",
            "not found in Input header",
        ),
        (
            vec!["-H", "health", "required", "name"],
            b"name\tname\n1\t2\n",
            "is ambiguous in Input header",
        ),
        (
            vec!["-H", "health", "required", "name"],
            b"",
            "not found in Input header",
        ),
    ] {
        for file in [false, true] {
            let error = invoke(&args, input, file);
            assert_eq!(error.status.code(), Some(1), "{error:?}");
            assert!(error.stdout.is_empty());
            assert!(String::from_utf8(error.stderr).unwrap().contains(expected));
        }
    }
    let positions = validated(&["-H", "health", "required", "2"], b"name\tname\n1\t2\n", 0);
    assert!(positions.contains("duplicate_header"));
    let names = validated(
        &[
            "--vnlog", "health", "type", "01", "integer", "required", "0",
        ],
        b"# other 01 0\ntext 2 data\n",
        0,
    );
    assert!(names.contains("field 2, name \"01\""));
    let error = invoke(
        &["--vnlog", "health", "required", "1"],
        b"# name value\na b\n",
        false,
    );
    assert_eq!(error.status.code(), Some(1));
    assert!(error.stdout.is_empty());
    assert!(
        String::from_utf8(error.stderr)
            .unwrap()
            .contains("not found in Input header")
    );
}

#[test]
fn declared_rules_exempt_optional_values_and_only_supplied_violations_fail_validation() {
    let optional = validated(&["health", "type", "2", "integer"], b"a\tNA\nb\t\nc\n", 0);
    assert!(!optional.contains("type_mismatch"));
    assert!(optional.contains("absent_field"));
    assert!(optional.contains("empty_field"));
    assert!(optional.contains("missing_indicator"));
    let codes = validated(
        &["health", "type", "2", "text", "required", "2"],
        b"a\tNA\nb\tN/A\nc\tNaN\n",
        0,
    );
    assert!(!codes.contains("presence_mismatch"));
    let advisory = validated(
        &["health", "type", "2", "text"],
        b"1\ta\n2\tb\n3\tc\nword\td\nshort\n",
        0,
    );
    assert!(advisory.contains("type_mismatch [inferred, advisory]"));
    assert!(advisory.contains("width_mismatch [inferred, advisory]"));
    let whole = validated(
        &["health", "type", "1", "number"],
        b"+1\n-2\n1.5\n0x1p0\ninf\nNaN(payload)\n12kg\n",
        1,
    );
    assert!(whole.contains("type_mismatch [declared, violation]: 1 cells"));
    let magnitude = validated(
        &["health", "type", "1", "number"],
        b"1e999999999999999999999999999\n",
        0,
    );
    assert!(!magnitude.contains("type_mismatch"));
}

#[test]
fn zero_width_and_header_only_input_do_not_invent_data_violations() {
    for input in [b"".as_slice(), b"\n\n"] {
        let output = validated(
            &["health", "width", "0", "required", "5"],
            input,
            if input.is_empty() { 0 } else { 1 },
        );
        assert!(output.contains("Baseline width: 0 fields (declared, declared)"));
        assert!(!output.contains("width_mismatch"));
    }
    let empty_header = validated(&["-H", "health", "width", "0"], b"\n", 0);
    assert!(empty_header.contains("Data records: 0\nHeader records: 1\n"));
    let header = validated(
        &[
            "-H", "health", "width", "0", "required", "2", "type", "1", "integer",
        ],
        b"a\tb\n",
        1,
    );
    assert!(header.contains("header_width_mismatch [declared, violation]: 1 header_records"));
    assert!(!header.contains("\n  width_mismatch"));
    assert!(!header.contains("presence_mismatch"));
    assert!(!header.contains("type_mismatch"));
    let width_only = validated(&["health", "width", "100"], b"", 0);
    assert_eq!(width_only.matches("\n  field ").count(), 0);
}

#[test]
fn declared_header_width_examples_keep_bounded_raw_accepted_bytes() {
    let vnlog = validated(
        &["--vnlog", "health", "width", "1"],
        b"## skip\n#! metadata\n # a b \t \n1 2 # note\n",
        1,
    );
    assert!(
        vnlog.contains(
            "accepted record 1: observed 2 fields; sample \" # a b \\t \"; truncated no\n"
        )
    );
    let mut input = b"## ignored\n# ".to_vec();
    input.extend_from_slice(&[b'a'; 150]);
    input.extend_from_slice(b" b \t \n");
    let long = validated(&["--vnlog", "health", "width", "0"], &input, 1);
    assert!(long.contains(&format!(
        "accepted record 1: observed 2 fields; sample \"# {}\"; truncated yes\n",
        "a".repeat(126)
    )));
    let binary = validated(
        &["-C", "-H", "-t", ",", "-z", "health", "width", "1"],
        b"#skip\0a,b\n\xff\0",
        1,
    );
    assert!(
        binary.contains(
            "accepted record 1: observed 2 fields; sample \"a,b\\n\\xFF\"; truncated no\n"
        )
    );
    let none = validated(
        &["-H", "health", "width", "0", "examples", "0"],
        b"a\tb\n",
        1,
    );
    assert!(none.contains("examples retained 0, omitted 1"));
    assert!(!none.contains("    accepted record"));
}

#[test]
fn health_declarations_reject_invalid_counts_types_and_non_single_selectors() {
    for declarations in [
        vec!["type"],
        vec!["type", "1"],
        vec!["type", "1", "float"],
        vec!["required"],
        vec!["nonmissing"],
        vec!["width"],
        vec!["width", "-1"],
        vec!["width", "1.0"],
        vec!["width", ""],
        vec!["validate"],
        vec!["examples", "2", "validate"],
        vec!["width", "1", "width", "2"],
        vec!["examples", "1", "examples", "2"],
        vec!["required", "0"],
        vec!["required", "1-1"],
        vec!["required", "1-2"],
        vec!["required", "1:2"],
        vec!["required", "1,2"],
        vec!["required", "1 2"],
        vec!["required", "1.5"],
        vec!["required", "9223372036854775808"],
        vec!["required", "a\\"],
        vec!["required", "a-b"],
        vec!["json"],
    ] {
        let mut args = vec!["-H", "health"];
        args.extend(declarations);
        let error = invoke(&args, b"a\tb\n1\t2\n", false);
        assert_eq!(error.status.code(), Some(1), "{args:?}: {error:?}");
        assert!(error.stdout.is_empty(), "{args:?}: {error:?}");
        assert!(!error.stderr.is_empty());
    }
    for args in [
        [
            "health",
            "width",
            "9999999999999999999999999999999999999999",
        ],
        [
            "health",
            "examples",
            "9999999999999999999999999999999999999999",
        ],
    ] {
        let refusal = invoke(&args, b"", false);
        assert_eq!(refusal.status.code(), Some(77));
        assert!(refusal.stdout.is_empty());
    }
    let output = validated(
        &[
            "health", "validate", "width", "1", "examples", "1", "width", "01", "validate", "type",
            "1", "integer", "type", "1", "integer", "required", "1", "required", "1", "examples",
            "1",
        ],
        b"1\n",
        0,
    );
    assert!(!output.contains("violation"));
}

#[test]
fn simultaneous_findings_keep_category_basis_and_field_order() {
    let output = validated(
        &[
            "-H",
            "health",
            "type",
            "1",
            "integer",
            "nonmissing",
            "1",
            "width",
            "1",
        ],
        b"a\tb\n1.5\t1\n2\t2\nword\tx\nNA\t3\n\n",
        1,
    );
    let findings: Vec<_> = output
        .lines()
        .filter(|line| line.starts_with("  ") && line.contains(" ["))
        .collect();
    let categories: Vec<_> = findings
        .iter()
        .map(|line| line.split_whitespace().next().unwrap())
        .collect();
    assert_eq!(
        categories,
        [
            "header_width_mismatch",
            "width_mismatch",
            "absent_field",
            "absent_field",
            "missing_indicator",
            "mixed_types",
            "mixed_types",
            "type_mismatch",
            "type_mismatch",
            "presence_mismatch"
        ]
    );
    assert!(findings[0].contains("1 header_records"));
    assert!(findings[1].contains("5 data_records"));
    assert!(findings[7].contains("[inferred, advisory]: 1 cells; expected integer; field 2"));
    assert!(findings[8].contains("[declared, violation]: 2 cells; expected integer; field 1"));
    assert!(findings[9].contains("[declared, violation]: 2 cells; expected nonmissing; field 1"));
}

#[test]
fn health_single_selectors_preserve_identifier_length_and_escaping_limits() {
    let name = "a".repeat(511);
    let input = format!("{name}\nvalue\n");
    validated(&["-H", "health", "required", &name], input.as_bytes(), 0);
    let too_long = "a".repeat(512);
    let error = invoke(&["-H", "health", "required", &too_long], b"", false);
    assert_eq!(error.status.code(), Some(1));
    assert_eq!(error.stderr, b"fastmash: identifier name too long\n");
    assert!(error.stdout.is_empty());
    validated(
        &[
            "-H",
            "health",
            "required",
            "a\\:b",
            "nonmissing",
            "a\\,b",
            "type",
            "a\\ b",
            "text",
            "required",
            "\\1\\-1",
        ],
        b"a:b\ta,b\ta b\t1-1\n1\t2\t3\t4\n",
        0,
    );
}

#[test]
fn lexical_counts_partition_missingness_and_fields_appearing_late() {
    let output = report(&["health"], b"+12\n-3\n1.0\n2e0\nword\nNA\n\t9\n\n");
    assert!(output.contains("Data records: 8\n"));
    assert!(output.contains("field 1, name \"\": absent_values 1; empty_values 1; missing_values 1; integer_values 2; other_number_values 2; text_values 1\n"));
    assert!(output.contains("field 2, name \"\": absent_values 7; empty_values 0; missing_values 0; integer_values 1; other_number_values 0; text_values 0\n"));
    assert!(output.contains("mixed_types [observation, advisory]: 1 fields; field 1, name \"\"; examples retained 3, omitted 2\n"));
}

#[test]
fn inferred_expectations_use_numeric_text_majorities_and_exclude_missing_values() {
    let output = report(
        &["-H", "health"],
        b"whole\tamount\tlabel\ttie\tmissing\n1\t1\ta\t1\tNA\n+2\t1.5\tb\tword\tN/A\nword\tword\t9\tNaN\t\n3\t2e0\tc\t\t\n",
    );
    for (field, name, counters, expectation) in [
        (
            1,
            "whole",
            "0; empty_values 0; missing_values 0; integer_values 3; other_number_values 0; text_values 1",
            "integer (inferred)",
        ),
        (
            2,
            "amount",
            "0; empty_values 0; missing_values 0; integer_values 1; other_number_values 2; text_values 1",
            "number (inferred)",
        ),
        (
            3,
            "label",
            "0; empty_values 0; missing_values 0; integer_values 1; other_number_values 0; text_values 3",
            "text (inferred)",
        ),
        (
            4,
            "tie",
            "0; empty_values 1; missing_values 1; integer_values 1; other_number_values 0; text_values 1",
            "undetermined (observation)",
        ),
        (
            5,
            "missing",
            "0; empty_values 2; missing_values 2; integer_values 0; other_number_values 0; text_values 0",
            "undetermined (observation)",
        ),
    ] {
        assert!(output.contains(&format!("field {field}, name \"{name}\": absent_values {counters}\n    type_expectation {expectation}\n")), "{output}");
    }
    let header_only = report(&["-H", "health"], b"value\n");
    assert!(header_only.contains("integer_values 0; other_number_values 0; text_values 0\n    type_expectation undetermined (observation)\n"));
    assert_eq!(
        output
            .matches("mixed_types [observation, advisory]")
            .count(),
        4
    );
    assert_eq!(
        output.matches("type_mismatch [inferred, advisory]").count(),
        2
    );
    assert!(output.contains(
        "type_mismatch [inferred, advisory]: 1 cells; expected integer; field 1, name \"whole\";"
    ));
    assert!(output.contains(
        "type_mismatch [inferred, advisory]: 1 cells; expected number; field 2, name \"amount\";"
    ));
    for field in [1, 2] {
        assert!(output.contains(&format!(
            "accepted record 4, field {field}: observed text; sample \"word\"; truncated no\n"
        )));
    }
    assert!(output.rfind("mixed_types [").unwrap() < output.find("type_mismatch [").unwrap());
}

#[test]
fn mixed_types_reserve_both_witnesses_and_count_omitted_eligible_cells() {
    for (limit, retained, omitted, records) in [
        (0, 0, 6, vec![]),
        (1, 1, 5, vec![1]),
        (2, 2, 4, vec![1, 5]),
        (3, 3, 3, vec![1, 2, 5]),
    ] {
        let output = report(
            &["health", "examples", &limit.to_string()],
            b"1\n2\n3\n4\nlate\nlast\n",
        );
        assert!(output.contains(&format!("mixed_types [observation, advisory]: 1 fields; field 1, name \"\"; examples retained {retained}, omitted {omitted}\n")), "{output}");
        let mixed = output
            .split_once("  mixed_types ")
            .unwrap()
            .1
            .split("\n  type_mismatch ")
            .next()
            .unwrap();
        assert_eq!(mixed.matches("    accepted record ").count(), retained);
        let locations: Vec<_> = mixed
            .lines()
            .filter_map(|line| line.strip_prefix("    accepted record "))
            .map(|line| line.split(',').next().unwrap().parse::<u64>().unwrap())
            .collect();
        assert_eq!(locations, records);
        if limit >= 2 {
            assert!(mixed.contains(
                "accepted record 5, field 1: observed text; sample \"late\"; truncated no"
            ));
        }
        assert!(output.contains(&format!("type_mismatch [inferred, advisory]: 2 cells; expected integer; field 1, name \"\"; examples retained {}, omitted {}\n", limit.min(2), 2 - limit.min(2))));
        let mismatch = output.split_once("  type_mismatch ").unwrap().1;
        if limit >= 1 {
            assert!(mismatch.contains(
                "accepted record 5, field 1: observed text; sample \"late\"; truncated no"
            ));
        }
        if limit >= 2 {
            assert!(mismatch.contains(
                "accepted record 6, field 1: observed text; sample \"last\"; truncated no"
            ));
        }
    }
}

#[test]
fn numeric_encodings_use_the_whole_field_grammar_without_converting_values() {
    let groups: &[(&[&[u8]], &str)] = &[
        (
            &[b"1", b"-2", b"+03", b" \t\n\r\x0b\x0c-42"],
            "integer_values 4; other_number_values 0; text_values 0",
        ),
        (
            &[
                b"1.",
                b".5",
                b"1e0",
                b"-2E+3",
                b"0xA",
                b"0x1.8p+2",
                b"+inf",
                b"-INFINITY",
                b"+NaN",
                b"nan(payload_2)",
                b"nan()",
                b" NaN",
            ],
            "integer_values 0; other_number_values 12; text_values 0",
        ),
        (
            &[
                b"12kg",
                b"1 ",
                b"1e",
                b"0x",
                b"nan(payload!)",
                b"nan(payload",
                b"+",
                b"-",
                b" ",
                b"\xff",
            ],
            "integer_values 0; other_number_values 0; text_values 10",
        ),
    ];
    for &(values, counts) in groups {
        let mut input = Vec::new();
        for value in values {
            input.extend_from_slice(value);
            input.push(0);
        }
        let output = report(&["-z", "-t", "|", "health"], &input);
        assert!(output.contains(counts), "{output}");
        assert!(!output.contains("mixed_types"));
        assert!(!output.contains("type_mismatch"));
    }
    let embedded_nul = report(&["health"], b"1\0\n2\0extra\ninf\0\nnan(payload)\0\n");
    assert!(embedded_nul.contains("integer_values 0; other_number_values 0; text_values 4\n"));
}

#[test]
fn numeric_syntax_larger_than_conversion_capacity_stays_numeric_and_samples_are_bounded() {
    let digits = "9".repeat(20_000);
    let hex = "a".repeat(20_000);
    let input = format!("{digits}\n1e{digits}\n{digits}.0\n0x{hex}p+0\ntext\n");
    let output = report(&["health"], input.as_bytes());
    assert!(output.contains("integer_values 1; other_number_values 3; text_values 1\n    type_expectation number (inferred)\n"));
    assert!(output.contains("type_mismatch [inferred, advisory]: 1 cells; expected number;"));
    assert!(output.contains(&format!(
        "accepted record 1, field 1: observed integer; sample \"{}\"; truncated yes\n",
        "9".repeat(128)
    )));
    assert!(!output.contains(&"9".repeat(129)));
}

#[test]
fn record_permutations_preserve_counts_and_inference_but_examples_follow_input_order() {
    for (input, first_sample) in [
        (
            b"1\n2.5\nword\n3\n4\n".as_slice(),
            "observed integer; sample \"1\"",
        ),
        (
            b"word\n4\n3\n2.5\n1\n".as_slice(),
            "observed text; sample \"word\"",
        ),
    ] {
        let output = report(&["health"], input);
        assert!(output.contains("integer_values 3; other_number_values 1; text_values 1\n    type_expectation number (inferred)\n"));
        assert!(output.contains("mixed_types [observation, advisory]: 1 fields; field 1, name \"\"; examples retained 3, omitted 2\n"));
        assert!(output.contains("type_mismatch [inferred, advisory]: 1 cells; expected number;"));
        let mixed = output
            .split_once("  mixed_types ")
            .unwrap()
            .1
            .split("\n  type_mismatch ")
            .next()
            .unwrap();
        assert!(mixed.contains(&format!("accepted record 1, field 1: {first_sample}")));
        for record in [1, 2, 3] {
            assert!(mixed.contains(&format!("accepted record {record}, field 1:")));
        }
    }
}

#[test]
fn text_majorities_keep_numeric_witnesses_without_inferred_type_mismatches() {
    for (limit, records) in [
        (0, vec![]),
        (1, vec![1]),
        (2, vec![1, 5]),
        (3, vec![1, 2, 5]),
    ] {
        let output = report(
            &["health", "examples", &limit.to_string()],
            b"first\nsecond\nthird\nfourth\n9.5\n10\n",
        );
        assert!(output.contains("integer_values 1; other_number_values 1; text_values 4\n    type_expectation text (inferred)\n"));
        assert!(!output.contains("type_mismatch"));
        let locations: Vec<_> = output
            .lines()
            .filter_map(|line| line.strip_prefix("    accepted record "))
            .map(|line| line.split(',').next().unwrap().parse::<u64>().unwrap())
            .collect();
        assert_eq!(locations, records);
        if limit >= 2 {
            assert!(output.contains(
                "accepted record 5, field 1: observed other_number; sample \"9.5\"; truncated no\n"
            ));
        }
    }
    let tie = report(&["health"], b"1\nword\n");
    assert!(tie.contains("type_expectation undetermined (observation)\n"));
    assert!(tie.contains("mixed_types [observation, advisory]: 1 fields"));
    assert!(!tie.contains("type_mismatch"));
}

fn width_examples(output: &str) -> impl Iterator<Item = &str> {
    output
        .lines()
        .filter(|line| line.starts_with("    accepted record ") && line.contains(" fields; sample"))
}

#[test]
fn field_observations_distinguish_empty_and_exact_candidate_missing_values() {
    let output = report(&["health"], b"a\t\tnA\nb\tN/A\tNaN\nc\t \tNA extra\n");
    assert!(
        output.contains("field 1, name \"\": absent_values 0; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 3\n")
    );
    assert!(
        output.contains("field 2, name \"\": absent_values 0; empty_values 1; missing_values 1; integer_values 0; other_number_values 0; text_values 1\n")
    );
    assert!(
        output.contains("field 3, name \"\": absent_values 0; empty_values 0; missing_values 2; integer_values 0; other_number_values 0; text_values 1\n")
    );
    assert!(output.contains("empty_field [observation, advisory]: 1 cells; field 2, name \"\"; examples retained 1, omitted 0\n"));
    assert!(output.contains("missing_indicator [observation, advisory]: 2 cells; field 3, name \"\"; examples retained 2, omitted 0\n"));
    assert!(
        output.contains("accepted record 1, field 2: observed empty; sample \"\"; truncated no\n")
    );
    assert!(output.contains(
        "accepted record 1, field 3: observed missing_indicator; sample \"nA\"; truncated no\n"
    ));
    assert!(output.find("empty_field").unwrap() < output.find("missing_indicator").unwrap());
}

#[test]
fn fields_appearing_late_backfill_absences_and_earliest_example_locations() {
    let output = report(&["health"], b"x\n\na\tb\nc\td\te\nf\ng\th\ti\tj\n");
    assert!(
        output.contains("field 1, name \"\": absent_values 1; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 5\n")
    );
    assert!(
        output.contains("field 2, name \"\": absent_values 3; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 3\n")
    );
    assert!(
        output.contains("field 3, name \"\": absent_values 4; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 2\n")
    );
    assert!(
        output.contains("field 4, name \"\": absent_values 5; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 1\n")
    );
    assert!(output.contains("absent_field [observation, advisory]: 5 cells; field 4, name \"\"; examples retained 3, omitted 2\n"));
    for record in [1, 2, 3] {
        assert!(output.contains(&format!(
            "accepted record {record}, field 4: observed absent; sample \"\"; truncated no\n"
        )));
    }
    assert!(!output.contains("accepted record 4, field 4:"));
    assert!(output.find("width_mismatch").unwrap() < output.find("absent_field").unwrap());
}

#[test]
fn missing_candidates_are_exact_without_trimming_or_coercion() {
    let input = b"a\tNA\nb\tna\nc\tNa\nd\tnA\ne\tN/A\nf\tn/a\ng\tN/a\nh\tn/A\ni\tNaN\nj\tnan\nk\tNAN\nl\tnAn\nm\t NA\nn\tNA \no\tN/Ax\np\txNaN\nq\t \nr\tNA\0\ns\tN/A\xff\nt\tNaN\\\nu\t";
    for limit in [0, 1, 2, 3] {
        let output = report(&["health", "examples", &limit.to_string()], input);
        assert!(output.contains("Data records: 21\n"));
        assert!(
            output.contains(
                "field 2, name \"\": absent_values 0; empty_values 1; missing_values 12; integer_values 0; other_number_values 0; text_values 8\n"
            )
        );
        assert!(output.contains(&format!("missing_indicator [observation, advisory]: 12 cells; field 2, name \"\"; examples retained {limit}, omitted {}\n", 12 - limit)));
        assert!(output.contains("empty_field [observation, advisory]: 1 cells; field 2"));
        assert_eq!(output.matches("observed missing_indicator;").count(), limit);
        for (record, sample) in [(1, "NA"), (2, "na"), (3, "Na")] {
            let expected = format!(
                "accepted record {record}, field 2: observed missing_indicator; sample \"{sample}\"; truncated no\n"
            );
            assert_eq!(output.contains(&expected), record <= limit);
        }
    }
}

#[test]
fn headers_track_zero_observations_and_positions_missing_from_all_data() {
    let header = report(&["--header-in", "health"], b"a\t\ta");
    for field in [1, 2, 3] {
        let name = if field == 2 { "" } else { "a" };
        assert!(header.contains(&format!(
            "field {field}, name \"{name}\": absent_values 0; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n"
        )));
    }
    assert!(!header.contains("absent_field"));
    assert!(!header.contains("empty_field"));
    assert!(!header.contains("missing_indicator"));

    let output = report(&["-H", "health"], b"one\ttwo\tthree\nNA\na\t\nb\n");
    assert!(
        output
            .contains("field 1, name \"one\": absent_values 0; empty_values 0; missing_values 1; integer_values 0; other_number_values 0; text_values 2\n")
    );
    assert!(
        output
            .contains("field 2, name \"two\": absent_values 2; empty_values 1; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(
        output.contains(
            "field 3, name \"three\": absent_values 3; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n"
        )
    );
    assert!(
        output.contains("accepted record 2, field 3: observed absent; sample \"\"; truncated no\n")
    );
    assert!(output.contains(
        "accepted record 2, field 1: observed missing_indicator; sample \"NA\"; truncated no\n"
    ));
    assert!(output.find("absent_field").unwrap() < output.find("empty_field").unwrap());
    assert!(output.find("empty_field").unwrap() < output.find("missing_indicator").unwrap());
}

#[test]
fn late_field_absence_examples_merge_widths_in_input_order_for_each_limit() {
    let input = b"a\tb\n\nc\nd\te\nf\ng\th\ni\tj\tk\tl\n";
    for limit in [0, 1, 2, 3] {
        let output = report(&["health", "examples", &limit.to_string()], input);
        assert!(
            output.contains(
                "field 4, name \"\": absent_values 6; empty_values 0; missing_values 0; integer_values 0; other_number_values 0; text_values 1\n"
            )
        );
        assert!(output.contains(&format!("absent_field [observation, advisory]: 6 cells; field 4, name \"\"; examples retained {limit}, omitted {}\n", 6 - limit)));
        for record in 1..=6 {
            let expected = format!(
                "accepted record {record}, field 4: observed absent; sample \"\"; truncated no\n"
            );
            assert_eq!(output.contains(&expected), record <= limit);
        }
    }
}

#[test]
fn whole_input_modal_width_reports_first_and_late_anomalies() {
    let output = report(&["health"], b"x\ty\tz\na\tb\nc\td\nlate");
    assert!(output.contains("Data records: 4\nHeader records: 0\nAccepted records: 4\n"));
    assert!(output.contains("Baseline width: 2 fields (inferred, modal)\n"));
    assert!(output.contains("Maximum data-record width: 3 fields\n"));
    assert!(output.contains("width_mismatch [inferred, advisory]: 2 data_records; expected 2 fields; examples retained 2, omitted 0\n"));
    assert!(
        output
            .contains("accepted record 1: observed 3 fields; sample \"x\\ty\\tz\"; truncated no\n")
    );
    assert!(
        output.contains("accepted record 4: observed 1 fields; sample \"late\"; truncated no\n")
    );
}

#[test]
fn header_shape_and_every_blank_or_duplicate_position_are_observations() {
    let output = report(&["-H", "health"], b"same\t\tsame\tother\na\tb\nc\td\n");
    assert!(output.contains("Data records: 2\nHeader records: 1\nAccepted records: 3\n"));
    assert!(output.contains("Baseline width: 4 fields (inferred, header)\n"));
    assert!(output.contains("width_mismatch [inferred, advisory]: 2 data_records"));
    assert!(output.contains("blank_header [observation, advisory]: 1 header_fields; field 2, name \"\"; examples retained 1, omitted 0\n"));
    for field in [1, 3] {
        assert!(output.contains(&format!("duplicate_header [observation, advisory]: 1 header_fields; field {field}, name \"same\"; examples retained 1, omitted 0\n")));
    }
    assert!(
        output.contains("accepted record 1, field 2: observed blank; sample \"\"; truncated no\n")
    );
    assert!(output.contains("accepted record 2: observed 2 fields"));
    assert!(output.find("width_mismatch").unwrap() < output.find("blank_header").unwrap());
    assert!(output.find("blank_header").unwrap() < output.find("duplicate_header").unwrap());
}

#[test]
fn calculation_only_options_are_rejected_even_when_their_values_are_defaults() {
    for arguments in [
        vec!["-s", "health"],
        vec!["-g", "1", "health"],
        vec!["-f", "health"],
        vec!["-i", "health"],
        vec!["--narm", "health"],
        vec!["--no-strict", "health"],
        vec!["--header-out", "health"],
        vec!["-H", "--header-out", "health"],
        vec!["--format=%.14g", "health"],
        vec!["--round=1", "health"],
        vec!["--seed=0", "health"],
        vec!["--filler=N/A", "health"],
        vec!["--output-delimiter=\t", "health"],
        vec!["--collapse-delimiter=,", "health"],
        vec!["-H", "--result-name=1:value", "health"],
        vec!["--result-name=1:value", "health"],
        vec!["--sort-cmd=/bin/sort", "health"],
        vec!["--sort-cmd", "/bin/sort", "health"],
    ] {
        let output = invoke(&arguments, b"a\n", false);
        assert_eq!(output.status.code(), Some(1), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr.clone())
                .unwrap()
                .contains("health does not support calculation option"),
            "{arguments:?}: {output:?}"
        );
    }
}

#[test]
fn tied_modal_width_chooses_the_smaller_width_regardless_of_record_order() {
    for input in [
        b"a\tb\tc\nd\te\nf\tg\nh\ti\tj\n".as_slice(),
        b"d\te\na\tb\tc\nh\ti\tj\nf\tg\n",
    ] {
        let output = report(&["health"], input);
        assert!(output.contains("Baseline width: 2 fields (inferred, modal)\n"));
        assert!(output.contains("width_mismatch [inferred, advisory]: 2 data_records"));
    }
}

#[test]
fn empty_blank_header_only_trailing_fields_and_unterminated_input_keep_intake_meanings() {
    let empty = report(&["health"], b"");
    assert!(empty.contains("Data records: 0\nHeader records: 0\nAccepted records: 0\n"));
    assert!(
        empty.contains("Baseline width: unavailable\nMaximum data-record width: unavailable\n")
    );
    let blanks = report(&["health"], b"\n\n");
    assert!(blanks.contains("Data records: 2\n"));
    assert!(blanks.contains("Baseline width: 0 fields (inferred, modal)\n"));
    let header = report(&["--header-in", "health"], b"a\t\ta");
    assert!(header.contains("Data records: 0\nHeader records: 1\nAccepted records: 1\n"));
    assert!(header.contains(
        "Baseline width: 3 fields (inferred, header)\nMaximum data-record width: unavailable\n"
    ));
    assert!(header.contains("blank_header"));
    let blank_header = report(&["-H", "health"], b"\n\n");
    assert!(blank_header.contains("Baseline width: 0 fields (inferred, header)\n"));
    assert!(!blank_header.contains("blank_header"));
    let trailing = report(&["health"], b"a\t\nb\t");
    assert!(trailing.contains("Data records: 2\n"));
    assert!(trailing.contains("Baseline width: 2 fields (inferred, modal)\n"));
    assert!(
        trailing
            .contains("field 2, name \"\": absent_values 0; empty_values 2; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(trailing.contains("empty_field [observation, advisory]: 2 cells; field 2"));
    assert!(!trailing.contains("width_mismatch"));
}

#[test]
fn separators_filters_nul_and_vnlog_preserve_accepted_record_locations() {
    let filtered = report(
        &["-C", "-H", "-t", ",", "-z", "health"],
        b"# skip\0a,b\0 ;skip\0NA,\0short\0",
    );
    assert!(filtered.contains("Data records: 2\nHeader records: 1\nAccepted records: 3\n"));
    assert!(filtered.contains("Input separator: literal \",\"\nRecord terminator: nul\n"));
    assert!(filtered.contains("Comment filter: comments\n"));
    assert!(filtered.contains("accepted record 3: observed 1 fields; sample \"short\""));
    assert!(
        filtered
            .contains("field 2, name \"b\": absent_values 1; empty_values 1; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(
        filtered.contains("accepted record 2, field 1: observed missing_indicator; sample \"NA\"")
    );
    assert!(filtered.contains("accepted record 2, field 2: observed empty; sample \"\""));
    assert!(filtered.contains("accepted record 3, field 2: observed absent; sample \"\""));
    let whitespace = report(&["-W", "health"], b"a NA \nb\tN/A\nc  NaN\nshort");
    assert!(whitespace.contains("Input separator: whitespace\n"));
    assert!(whitespace.contains("Baseline width: 2 fields (inferred, modal)\n"));
    assert!(
        whitespace
            .contains("field 2, name \"\": absent_values 1; empty_values 0; missing_values 3; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(
        whitespace
            .contains("field 3, name \"\": absent_values 3; empty_values 1; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(whitespace.contains("accepted record 1, field 3: observed empty; sample \"\""));
    assert!(whitespace.contains("accepted record 4, field 2: observed absent; sample \"\""));
    let vnlog = report(
        &["--vnlog", "health"],
        b"## metadata\n\n#! annotation\n# a b\n1 NA # note\n# skipped\n3 # raw\n",
    );
    assert!(vnlog.contains("Data records: 2\nHeader records: 1\nAccepted records: 3\n"));
    assert!(vnlog.contains("Comment filter: vnlog\nVnlog: yes\n"));
    assert!(vnlog.contains("accepted record 3: observed 1 fields; sample \"3 # raw\""));
    assert!(
        vnlog.contains("field 2, name \"b\": absent_values 1; empty_values 0; missing_values 1; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(
        vnlog.contains("accepted record 2, field 2: observed missing_indicator; sample \"NA\"")
    );
    assert!(vnlog.contains("accepted record 3, field 2: observed absent; sample \"\""));
}

#[test]
fn observations_preserve_full_binary_header_labels_without_inventing_late_labels() {
    let mut label = vec![b'q'; 150];
    label.extend_from_slice(b"\\\r\0\xff");
    let mut input = b"first\t".to_vec();
    input.extend_from_slice(&label);
    input.extend_from_slice(b"\na\nb\tNA\t\nc\n");
    let output = report(&["--header-in", "health"], &input);
    let escaped = format!("{}\\\\\\r\\0\\xFF", "q".repeat(150));
    assert!(output.contains(&format!(
        "field 2, name \"{escaped}\": absent_values 2; empty_values 0; missing_values 1; integer_values 0; other_number_values 0; text_values 0\n"
    )));
    assert!(output.contains(&format!(
        "absent_field [observation, advisory]: 2 cells; field 2, name \"{escaped}\""
    )));
    assert!(output.contains(&format!(
        "missing_indicator [observation, advisory]: 1 cells; field 2, name \"{escaped}\""
    )));
    assert!(
        output.contains("field 3, name \"\": absent_values 2; empty_values 1; missing_values 0; integer_values 0; other_number_values 0; text_values 0\n")
    );
    assert!(
        output.contains("accepted record 2, field 3: observed absent; sample \"\"; truncated no\n")
    );
    assert!(
        output.contains("accepted record 3, field 3: observed empty; sample \"\"; truncated no\n")
    );
}

#[test]
fn bounded_examples_are_earliest_across_widths_and_disclose_omissions() {
    let input = b"a\tb\tc\none\nx\ty\tz\ntwo\na\tb\na\tb\na\tb\na\tb\na\tb\n";
    for (limit, retained, omitted) in [(0, 0, 4), (1, 1, 3), (2, 2, 2), (3, 3, 1)] {
        let output = report(&["health", "examples", &limit.to_string()], input);
        assert!(output.contains(&format!(
            "examples retained {retained}, omitted {omitted}\n"
        )));
        assert_eq!(width_examples(&output).count(), retained);
        if limit >= 1 {
            assert!(output.contains("accepted record 1:"));
        }
        if limit >= 2 {
            assert!(output.contains("accepted record 2:"));
        }
        if limit >= 3 {
            assert!(output.contains("accepted record 3:"));
        }
    }
    let default = report(&["health"], input);
    assert!(default.contains("Example limit: 3 per finding\n"));
    assert_eq!(width_examples(&default).count(), 3);
    assert_eq!(
        report(&["health", "examples", "2"], input),
        report(&["health", "examples", "02", "examples", "2"], input)
    );
}

#[test]
fn labels_and_samples_escape_exact_bytes_and_truncate_only_samples() {
    let mut input = b"a\\\r\0\xff\t\ta\\\r\0\xff\n".to_vec();
    input.extend_from_slice(&[b'x'; 150]);
    input.extend_from_slice(b"\n");
    let output = report(&["-H", "health"], &input);
    assert!(output.contains("name \"a\\\\\\r\\0\\xFF\""));
    assert!(output.contains(&format!("sample \"{}\"; truncated yes", "x".repeat(128))));
    assert!(!output.contains(&"x".repeat(129)));
    assert!(output.contains("Sample byte limit: 128 raw bytes\n"));
    let label = "q".repeat(150);
    let duplicate_header = format!("{label}\t{label}\n");
    let output = report(&["-H", "health"], duplicate_header.as_bytes());
    assert!(output.contains(&format!("name \"{label}\"")));
    assert!(output.contains(&format!("sample \"{}\"; truncated yes", "q".repeat(128))));
}

#[test]
fn unknown_incomplete_conflicting_controls_and_invalid_limits_emit_no_report() {
    for arguments in [
        vec!["health", "examples"],
        vec!["health", "examples", ""],
        vec!["health", "examples", "+1"],
        vec!["health", "examples", "1x"],
        vec!["health", "examples", "1", "examples", "2"],
        vec!["health", "json"],
        vec!["health", "validate"],
        vec!["--health", "health"],
    ] {
        let output = invoke(&arguments, b"x\n", false);
        assert_eq!(output.status.code(), Some(1), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let output = invoke(&["health", "examples", "18446744073709551616"], b"", false);
    assert_eq!(output.status.code(), Some(77));
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"fastmash: health example limit exceeds checked capacity\n"
    );
}

#[test]
fn report_uses_the_effective_numeric_decimal_separator_without_arithmetic() {
    let output = invoke_in(
        &["health"],
        b"1,5\n0x1,8p+1\n2\n1.5\n",
        false,
        "de_DE.UTF-8",
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let output = String::from_utf8(output.stdout).unwrap();
    assert!(output.contains("Numeric decimal separator: \",\"\n"));
    assert!(output.contains("integer_values 1; other_number_values 2; text_values 1\n    type_expectation number (inferred)\n"));
    assert!(output.contains("type_mismatch [inferred, advisory]: 1 cells; expected number;"));
    let c = report(&["health"], b"1,5\n0x1,8p+1\n2\n1.5\n");
    assert!(c.contains("integer_values 1; other_number_values 1; text_values 2\n    type_expectation undetermined (observation)\n"));
    assert!(!c.contains("type_mismatch"));
    let unsupported = invoke_in(&["health"], b"", false, "ps_AF.UTF-8");
    assert_eq!(unsupported.status.code(), Some(77), "{unsupported:?}");
    assert!(unsupported.stdout.is_empty());
    assert!(
        String::from_utf8(unsupported.stderr)
            .unwrap()
            .contains("unsupported numeric locale")
    );
}

#[test]
fn unavailable_calculation_options_keep_their_ordinary_refusal_behavior() {
    for arguments in [
        vec!["--sort-cmd=/bin/sort", "count", "1"],
        vec!["--sort-cmd=/bin/sort", "--help"],
        vec!["--sort-cmd=health", "count", "1"],
    ] {
        let output = invoke(&arguments, b"a\n", false);
        assert_eq!(output.status.code(), Some(77), "{arguments:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("unsupported option '--sort-cmd=")
        );
    }
}
