//! Byte-preserving readable and schema-version-1 TSV table-health reports.
use super::{
    buffered_stdout::BufferedStdout,
    health::{Expected, Observed, Report, SAMPLE_BYTES, Settings},
    intake::Filter,
    options::Options,
    records::Separator,
    terminal_style::{Role, Style},
};
use std::io::Write;

fn number<W: Write>(output: &mut BufferedStdout<'_, W>, mut value: u64) {
    let mut bytes = [0u8; 20];
    let mut at = bytes.len();
    loop {
        at -= 1;
        bytes[at] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            break;
        }
    }
    output.raw(&bytes[at..]);
}

fn escaped<W: Write>(output: &mut BufferedStdout<'_, W>, bytes: &[u8]) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    for &byte in bytes {
        match byte {
            b'\\' => output.raw(b"\\\\"),
            b'\t' => output.raw(b"\\t"),
            b'\n' => output.raw(b"\\n"),
            b'\r' => output.raw(b"\\r"),
            0 => output.raw(b"\\0"),
            0x20..=0x7e => output.raw(&[byte]),
            _ => output.raw(&[
                b'\\',
                b'x',
                HEX[usize::from(byte >> 4)],
                HEX[usize::from(byte & 15)],
            ]),
        }
    }
}

#[derive(Clone, Copy, Default)]
enum Cell<'a> {
    #[default]
    Empty,
    Bytes(&'a [u8]),
    Number(u64),
}

impl<'a> Cell<'a> {
    fn count(value: Option<u64>) -> Self {
        value.map_or(Self::Empty, Self::Number)
    }
}

#[derive(Clone, Copy, Default)]
struct TsvRow<'a> {
    kind: &'a [u8],
    category: &'a [u8],
    basis: &'a [u8],
    field: Option<usize>,
    name: &'a [u8],
    count: Option<u64>,
    unit: &'a [u8],
    record: Option<u64>,
    expected: Cell<'a>,
    observed: Cell<'a>,
    sample: &'a [u8],
    truncated: Option<bool>,
    omitted: Option<u64>,
}

impl TsvRow<'_> {
    fn write<W: Write>(&self, output: &mut BufferedStdout<'_, W>) {
        let cells = [
            Cell::Bytes(self.kind),
            Cell::Bytes(self.category),
            Cell::Bytes(self.basis),
            Cell::count(self.field.map(|field| field as u64 + 1)),
            Cell::Bytes(self.name),
            Cell::count(self.count),
            Cell::Bytes(self.unit),
            Cell::count(self.record),
            self.expected,
            self.observed,
            Cell::Bytes(self.sample),
            self.truncated.map_or(Cell::Empty, |truncated| {
                Cell::Bytes(if truncated { b"yes" } else { b"no" })
            }),
            Cell::count(self.omitted),
        ];
        for (at, cell) in cells.into_iter().enumerate() {
            if at != 0 {
                output.raw(b"\t");
            }
            match cell {
                Cell::Empty => {}
                Cell::Bytes(bytes) => escaped(output, bytes),
                Cell::Number(value) => number(output, value),
            }
        }
        output.raw(b"\n");
    }
}

fn summary_value<W: Write>(output: &mut BufferedStdout<'_, W>, category: &[u8], value: &[u8]) {
    TsvRow {
        kind: b"summary",
        category,
        observed: Cell::Bytes(value),
        ..Default::default()
    }
    .write(output);
}

fn summary_count<W: Write>(
    output: &mut BufferedStdout<'_, W>,
    category: &[u8],
    basis: &[u8],
    count: Option<u64>,
    unit: &[u8],
) {
    TsvRow {
        kind: b"summary",
        category,
        basis,
        count,
        unit,
        ..Default::default()
    }
    .write(output);
}

fn expected_cell(expected: Option<&Expected>) -> Cell<'_> {
    match expected {
        None => Cell::Empty,
        Some(Expected::Width(width)) => Cell::Number(*width as u64),
        Some(Expected::Type(kind)) => Cell::Bytes(kind.id()),
        Some(Expected::Presence(presence)) => Cell::Bytes(presence.id()),
    }
}

enum ObservedValue {
    Width(usize),
    Label(&'static [u8]),
}

impl ObservedValue {
    fn cell(self) -> Cell<'static> {
        match self {
            Self::Width(width) => Cell::Number(width as u64),
            Self::Label(label) => Cell::Bytes(label),
        }
    }
}

fn observed_value(observed: Observed) -> ObservedValue {
    match observed {
        Observed::Width(width) => ObservedValue::Width(width),
        Observed::Blank => ObservedValue::Label(b"blank"),
        Observed::Duplicate => ObservedValue::Label(b"duplicate"),
        Observed::Absent => ObservedValue::Label(b"absent"),
        Observed::Empty => ObservedValue::Label(b"empty"),
        Observed::Missing => ObservedValue::Label(b"missing_indicator"),
        Observed::Integer => ObservedValue::Label(b"integer"),
        Observed::OtherNumber => ObservedValue::Label(b"other_number"),
        Observed::Text => ObservedValue::Label(b"text"),
    }
}

pub(super) fn tsv<W: Write>(
    output: &mut BufferedStdout<'_, W>,
    report: &Report,
    options: &Options,
    settings: &Settings,
) {
    output.raw(b"row_kind\tcategory\tbasis\tfield_index\tfield_name\tcount\tcount_unit\trecord_index\texpected\tobserved\tsample\tsample_truncated\texamples_omitted\n");
    summary_value(output, b"schema_version", b"1");
    for (category, count, unit) in [
        (
            &b"data_records"[..],
            report.data_records,
            &b"data_records"[..],
        ),
        (b"header_records", report.header_records, b"header_records"),
        (b"accepted_records", report.accepted_records, b"records"),
    ] {
        summary_count(output, category, b"observation", Some(count), unit);
    }
    if let Some(width) = report.baseline.width() {
        summary_count(
            output,
            b"baseline_width",
            report.baseline.basis().id(),
            Some(width as u64),
            b"fields",
        );
    } else {
        summary_count(output, b"baseline_width", b"", None, b"");
    }
    summary_value(output, b"baseline_width_source", report.baseline.source());
    summary_count(
        output,
        b"maximum_observed_width",
        b"observation",
        report.maximum_width.map(|width| width as u64),
        b"fields",
    );
    match options.input {
        Separator::Literal(byte) => {
            summary_value(output, b"separator_kind", b"literal");
            summary_value(output, b"separator", &[byte]);
        }
        Separator::Whitespace => {
            summary_value(output, b"separator_kind", b"whitespace");
            summary_value(output, b"separator", b"");
        }
    }
    summary_value(
        output,
        b"record_terminator",
        if options.record_end == 0 {
            b"nul"
        } else {
            b"lf"
        },
    );
    summary_value(
        output,
        b"numeric_decimal_separator",
        &[options.locale.numeric.radix()],
    );
    summary_value(
        output,
        b"input_header",
        if options.header_in { b"yes" } else { b"no" },
    );
    summary_value(
        output,
        b"comment_filter",
        match Filter::of(options) {
            Filter::None => b"none",
            Filter::Comments => b"comments",
            Filter::Vnlog => b"vnlog",
        },
    );
    summary_value(output, b"vnlog", if options.vnlog { b"yes" } else { b"no" });
    summary_value(output, b"record_numbering", b"accepted_records_1based");
    summary_count(
        output,
        b"example_limit",
        b"observation",
        Some(settings.examples as u64),
        b"examples",
    );
    summary_count(
        output,
        b"sample_byte_limit",
        b"observation",
        Some(SAMPLE_BYTES as u64),
        b"bytes",
    );
    for state in &report.fields {
        let name: &[u8] = report.header.get(state.position).map_or(&[], Vec::as_slice);
        for (category, count) in [
            (&b"absent_values"[..], state.absent),
            (b"empty_values", state.empty),
            (b"missing_values", state.missing),
            (b"integer_values", state.integer),
            (b"other_number_values", state.other_number),
            (b"text_values", state.text),
        ] {
            TsvRow {
                kind: b"field",
                category,
                basis: b"observation",
                field: Some(state.position),
                name,
                count: Some(count),
                unit: b"cells",
                ..Default::default()
            }
            .write(output);
        }
        TsvRow {
            kind: b"field",
            category: b"type_expectation",
            basis: if state.declared_type.is_some() {
                b"declared"
            } else {
                state.expectation.basis()
            },
            field: Some(state.position),
            name,
            expected: Cell::Bytes(state.expectation.id()),
            ..Default::default()
        }
        .write(output);
        if let Some(presence) = state.presence {
            TsvRow {
                kind: b"field",
                category: b"presence_requirement",
                basis: b"declared",
                field: Some(state.position),
                name,
                expected: Cell::Bytes(presence.id()),
                ..Default::default()
            }
            .write(output);
        }
    }
    for finding in &report.findings {
        let row = TsvRow {
            kind: b"finding",
            category: finding.category.id(),
            basis: finding.basis.id(),
            field: finding.field,
            name: finding
                .field
                .and_then(|field| report.header.get(field))
                .map_or(&[], Vec::as_slice),
            count: Some(finding.count),
            unit: finding.category.unit(),
            expected: expected_cell(finding.expected.as_ref()),
            omitted: Some(finding.candidates - finding.examples.len() as u64),
            ..Default::default()
        };
        row.write(output);
        for example in &finding.examples {
            TsvRow {
                kind: b"example",
                count: None,
                unit: b"",
                record: Some(example.record),
                observed: observed_value(example.observed).cell(),
                sample: &example.bytes[..example.length],
                truncated: Some(example.truncated),
                omitted: None,
                ..row
            }
            .write(output);
        }
    }
}

pub(super) fn readable<W: Write>(
    output: &mut BufferedStdout<'_, W>,
    report: &Report,
    options: &Options,
    settings: &Settings,
    style: Style,
) {
    output.raw(style.start(Role::Heading));
    output.raw(b"Table health report");
    output.raw(style.reset());
    output.raw(b"\nData records: ");
    number(output, report.data_records);
    output.raw(b"\nHeader records: ");
    number(output, report.header_records);
    output.raw(b"\nAccepted records: ");
    number(output, report.accepted_records);
    output.raw(b"\nInput separator: ");
    match options.input {
        Separator::Literal(byte) => {
            output.raw(b"literal \"");
            escaped(output, &[byte]);
            output.raw(b"\"");
        }
        Separator::Whitespace => output.raw(b"whitespace"),
    }
    output.raw(b"\nRecord terminator: ");
    output.raw(if options.record_end == 0 {
        b"nul"
    } else {
        b"lf"
    });
    output.raw(b"\nNumeric decimal separator: \"");
    escaped(output, &[options.locale.numeric.radix()]);
    output.raw(b"\"");
    output.raw(b"\nInput header: ");
    output.raw(if options.header_in { b"yes" } else { b"no" });
    output.raw(b"\nComment filter: ");
    output.raw(match Filter::of(options) {
        Filter::None => b"none",
        Filter::Comments => b"comments",
        Filter::Vnlog => b"vnlog",
    });
    output.raw(b"\nVnlog: ");
    output.raw(if options.vnlog { b"yes" } else { b"no" });
    output.raw(b"\nRecord numbering: accepted_records_1based (including the Input header; filtered records excluded)\nExample limit: ");
    number(output, settings.examples as u64);
    output.raw(b" per finding\nSample byte limit: ");
    number(output, SAMPLE_BYTES as u64);
    output.raw(b" raw bytes\nBaseline width: ");
    if let Some(width) = report.baseline.width() {
        number(output, width as u64);
        output.raw(b" fields (");
        output.raw(report.baseline.basis().id());
        output.raw(b", ");
        output.raw(report.baseline.source());
        output.raw(b")");
    } else {
        output.raw(b"unavailable");
    }
    output.raw(b"\nMaximum data-record width: ");
    if let Some(width) = report.maximum_width {
        number(output, width as u64);
        output.raw(b" fields");
    } else {
        output.raw(b"unavailable");
    }
    output.raw(b"\n");
    output.raw(style.start(Role::Heading));
    output.raw(b"Field observations (data-record cells):");
    output.raw(style.reset());
    output.raw(b"\n");
    if report.fields.is_empty() {
        output.raw(b"  none\n");
    }
    for state in &report.fields {
        let field = state.position;
        output.raw(b"  field ");
        number(output, field as u64 + 1);
        output.raw(b", name \"");
        escaped(output, report.header.get(field).map_or(&[], Vec::as_slice));
        output.raw(b"\": absent_values ");
        number(output, state.absent);
        output.raw(b"; empty_values ");
        number(output, state.empty);
        output.raw(b"; missing_values ");
        number(output, state.missing);
        output.raw(b"; integer_values ");
        number(output, state.integer);
        output.raw(b"; other_number_values ");
        number(output, state.other_number);
        output.raw(b"; text_values ");
        number(output, state.text);
        output.raw(b"\n    type_expectation ");
        output.raw(state.expectation.id());
        output.raw(b" (");
        output.raw(if state.declared_type.is_some() {
            b"declared"
        } else {
            state.expectation.basis()
        });
        output.raw(b")");
        output.raw(b"\n");
        if let Some(presence) = state.presence {
            output.raw(b"    presence_requirement ");
            output.raw(presence.id());
            output.raw(b" (declared)\n");
        }
    }
    output.raw(style.start(Role::Heading));
    output.raw(b"Findings (counts may overlap):");
    output.raw(style.reset());
    output.raw(b"\n");
    if report.findings.is_empty() {
        output.raw(b"  none\n");
        return;
    }
    for finding in &report.findings {
        output.raw(b"  ");
        output.raw(finding.category.id());
        output.raw(b" [");
        output.raw(finding.basis.id());
        output.raw(b", ");
        let (role, marker): (_, &[u8]) = if finding.is_violation() {
            (Role::Failure, b"violation")
        } else {
            (Role::Advisory, b"advisory")
        };
        output.raw(style.start(role));
        output.raw(marker);
        output.raw(style.reset());
        output.raw(b"]: ");
        number(output, finding.count);
        output.raw(b" ");
        output.raw(finding.category.unit());
        if let Some(expected) = &finding.expected {
            output.raw(b"; expected ");
            match expected {
                Expected::Width(width) => {
                    number(output, *width as u64);
                    output.raw(b" fields");
                }
                Expected::Type(kind) => output.raw(kind.id()),
                Expected::Presence(presence) => output.raw(presence.id()),
            }
        }
        if let Some(field) = finding.field {
            output.raw(b"; field ");
            number(output, field as u64 + 1);
            output.raw(b", name \"");
            escaped(output, report.header.get(field).map_or(&[], Vec::as_slice));
            output.raw(b"\"");
        }
        output.raw(b"; examples retained ");
        number(output, finding.examples.len() as u64);
        output.raw(b", omitted ");
        number(output, finding.candidates - finding.examples.len() as u64);
        output.raw(b"\n");
        for example in &finding.examples {
            output.raw(b"    accepted record ");
            number(output, example.record);
            if let Some(field) = finding.field {
                output.raw(b", field ");
                number(output, field as u64 + 1);
            }
            output.raw(b": observed ");
            match observed_value(example.observed) {
                ObservedValue::Width(width) => {
                    number(output, width as u64);
                    output.raw(b" fields");
                }
                ObservedValue::Label(label) => output.raw(label),
            }
            output.raw(b"; sample \"");
            escaped(output, &example.bytes[..example.length]);
            output.raw(b"\"; truncated ");
            output.raw(if example.truncated { b"yes\n" } else { b"no\n" });
        }
    }
}
