//! Whole-input table inspection, separate from calculation and arithmetic.
use super::{
    Failure,
    buffered_stdout::BufferedStdout,
    command_memory::reserve,
    command_output, conversion_failure, failure, field_policy, grammar,
    intake::{self, Intake},
    options::Options,
    os_failure, records, unsupported,
};
use fastmash_conversion::{
    bytes::{self, Lexeme},
    profile::Profile,
};
use std::{ffi::OsString, io::BufRead, os::unix::ffi::OsStrExt};

pub(super) const SAMPLE_BYTES: usize = 128;

#[cfg(test)]
thread_local! {
    pub(super) static COUNT_LIMIT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

pub(super) struct Settings {
    pub examples: usize,
    pub width: Option<usize>,
    pub validate: bool,
    pub tsv: bool,
    declarations: Vec<Declaration>,
}

struct Declaration {
    field: grammar::Field,
    rule: Rule,
}

enum Rule {
    Type(Type),
    Presence(Presence),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Presence {
    Required,
    Nonmissing,
}

impl Presence {
    pub fn id(self) -> &'static [u8] {
        match self {
            Self::Required => b"required",
            Self::Nonmissing => b"nonmissing",
        }
    }
}

impl Settings {
    pub fn parse(arguments: &[OsString], options: &Options) -> Result<Self, Failure> {
        let mut examples = None;
        let mut width = None;
        let mut validate = false;
        let mut tsv = false;
        let mut declarations = Vec::new();
        let mut arguments = arguments.iter();
        while let Some(control) = arguments.next() {
            match control.as_bytes() {
                b"examples" => {
                    let limit = decimal_count(arguments.next(), "examples")?;
                    if limit > isize::MAX as usize / std::mem::size_of::<Example>() {
                        return Err(unsupported("health example limit exceeds checked capacity"));
                    }
                    if examples.is_some_and(|previous| previous != limit) {
                        return Err(failure(b"health: conflicting example limits\n".to_vec()));
                    }
                    examples = Some(limit);
                }
                b"width" => {
                    let value = decimal_count(arguments.next(), "width")?;
                    if width.is_some_and(|previous| previous != value) {
                        return Err(failure(b"health: conflicting widths\n".to_vec()));
                    }
                    width = Some(value);
                }
                b"validate" => validate = true,
                b"tsv" => tsv = true,
                b"type" | b"required" | b"nonmissing" => {
                    let argument = arguments.next().ok_or_else(|| {
                        failure(b"health: field rule requires a field\n".to_vec())
                    })?;
                    let field = grammar::single_field(
                        argument,
                        options.vnlog,
                        options.locale.numeric,
                        options.locale.utf8,
                    )?;
                    if matches!(field, grammar::Field::Name(_)) && !options.header_in {
                        return Err(failure(
                            b"health: named fields require an Input header\n".to_vec(),
                        ));
                    }
                    let rule = match control.as_bytes() {
                        b"type" => Rule::Type(match arguments.next().map(|kind| kind.as_bytes()) {
                            Some(b"integer") => Type::Integer,
                            Some(b"number") => Type::Number,
                            Some(b"text") => Type::Text,
                            _ => {
                                return Err(failure(
                                    b"health: type requires integer, number or text\n".to_vec(),
                                ));
                            }
                        }),
                        b"required" => Rule::Presence(Presence::Required),
                        b"nonmissing" => Rule::Presence(Presence::Nonmissing),
                        _ => unreachable!(),
                    };
                    reserve(&mut declarations, 1)?;
                    declarations.push(Declaration { field, rule });
                }
                _ => {
                    return Err(failure(
                        b"health: unknown or unavailable control\n".to_vec(),
                    ));
                }
            }
        }
        if validate && width.is_none() && declarations.is_empty() {
            return Err(failure(
                b"health: validate requires at least one supplied rule\n".to_vec(),
            ));
        }
        Ok(Self {
            examples: examples.unwrap_or(3),
            width,
            validate,
            tsv,
            declarations,
        })
    }
}

fn decimal_count(argument: Option<&OsString>, control: &str) -> Result<usize, Failure> {
    let bytes = argument.map_or(&[][..], |argument| argument.as_bytes());
    if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
        return Err(failure(
            format!("health: {control} requires a nonnegative decimal integer\n").into_bytes(),
        ));
    }
    bytes
        .iter()
        .try_fold(0usize, |n, b| {
            n.checked_mul(10)?.checked_add(usize::from(b - b'0'))
        })
        .ok_or_else(|| {
            unsupported(if control == "examples" {
                "health example limit exceeds checked capacity"
            } else {
                "health width exceeds checked capacity"
            })
        })
}

#[derive(Clone, Copy)]
pub(super) struct Example {
    pub record: u64,
    pub observed: Observed,
    pub bytes: [u8; SAMPLE_BYTES],
    pub length: usize,
    pub truncated: bool,
}

impl Example {
    fn new(record: u64, observed: Observed, sample: &[u8]) -> Self {
        let length = sample.len().min(SAMPLE_BYTES);
        let mut bytes = [0; SAMPLE_BYTES];
        bytes[..length].copy_from_slice(&sample[..length]);
        Self {
            record,
            observed,
            bytes,
            length,
            truncated: sample.len() > length,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Observed {
    Width(usize),
    Blank,
    Duplicate,
    Absent,
    Empty,
    Missing,
    Integer,
    OtherNumber,
    Text,
}

#[derive(Clone, Copy)]
pub(super) enum Category {
    HeaderWidthMismatch,
    WidthMismatch,
    BlankHeader,
    DuplicateHeader,
    AbsentField,
    EmptyField,
    MissingIndicator,
    MixedTypes,
    TypeMismatch,
    PresenceMismatch,
}

impl Category {
    pub fn id(self) -> &'static [u8] {
        match self {
            Self::HeaderWidthMismatch => b"header_width_mismatch",
            Self::WidthMismatch => b"width_mismatch",
            Self::BlankHeader => b"blank_header",
            Self::DuplicateHeader => b"duplicate_header",
            Self::AbsentField => b"absent_field",
            Self::EmptyField => b"empty_field",
            Self::MissingIndicator => b"missing_indicator",
            Self::MixedTypes => b"mixed_types",
            Self::TypeMismatch => b"type_mismatch",
            Self::PresenceMismatch => b"presence_mismatch",
        }
    }
    pub fn unit(self) -> &'static [u8] {
        match self {
            Self::HeaderWidthMismatch => b"header_records",
            Self::WidthMismatch => b"data_records",
            Self::BlankHeader | Self::DuplicateHeader => b"header_fields",
            Self::AbsentField
            | Self::EmptyField
            | Self::MissingIndicator
            | Self::TypeMismatch
            | Self::PresenceMismatch => b"cells",
            Self::MixedTypes => b"fields",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Basis {
    Observation,
    Inferred,
    Declared,
}

impl Basis {
    pub fn id(self) -> &'static [u8] {
        match self {
            Self::Observation => b"observation",
            Self::Inferred => b"inferred",
            Self::Declared => b"declared",
        }
    }
}

pub(super) struct Finding {
    pub category: Category,
    pub basis: Basis,
    pub count: u64,
    /// Zero-based internally; reports use one-based field positions.
    pub field: Option<usize>,
    pub expected: Option<Expected>,
    pub candidates: u64,
    pub examples: Vec<Example>,
}

impl Finding {
    pub(super) fn is_violation(&self) -> bool {
        self.basis == Basis::Declared
    }
}

pub(super) enum Expected {
    Width(usize),
    Type(Type),
    Presence(Presence),
}

struct Width {
    width: usize,
    count: u64,
    examples: Vec<Example>,
}

#[derive(Default)]
pub(super) struct Field {
    pub position: usize,
    pub absent: u64,
    pub empty: u64,
    pub missing: u64,
    pub integer: u64,
    pub other_number: u64,
    pub text: u64,
    pub expectation: Type,
    pub declared_type: Option<Type>,
    pub presence: Option<Presence>,
    absent_examples: Vec<Example>,
    empty_examples: Vec<Example>,
    missing_examples: Vec<Example>,
    numeric_examples: Vec<Example>,
    other_number_examples: Vec<Example>,
    text_examples: Vec<Example>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Type {
    Integer,
    Number,
    Text,
    #[default]
    Undetermined,
}

impl Type {
    pub fn id(self) -> &'static [u8] {
        match self {
            Self::Integer => b"integer",
            Self::Number => b"number",
            Self::Text => b"text",
            Self::Undetermined => b"undetermined",
        }
    }

    pub fn basis(self) -> &'static [u8] {
        if matches!(self, Self::Undetermined) {
            b"observation"
        } else {
            b"inferred"
        }
    }
}

impl Field {
    fn numeric_count(&self) -> Result<u64, Failure> {
        let mut count = self.integer;
        add(&mut count, self.other_number)?;
        Ok(count)
    }

    fn infer(&mut self) -> Result<(), Failure> {
        if let Some(kind) = self.declared_type {
            self.expectation = kind;
            return Ok(());
        }
        self.expectation = match self.numeric_count()?.cmp(&self.text) {
            std::cmp::Ordering::Greater if self.other_number == 0 => Type::Integer,
            std::cmp::Ordering::Greater => Type::Number,
            std::cmp::Ordering::Less => Type::Text,
            std::cmp::Ordering::Equal => Type::Undetermined,
        };
        Ok(())
    }
}

fn classify(value: &[u8], profile: Profile) -> Result<Observed, Failure> {
    let digits = &value[value.iter().take_while(|&&byte| bytes::space(byte)).count()..];
    let digits = match digits.first() {
        Some(b'+' | b'-') => &digits[1..],
        _ => digits,
    };
    if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) {
        return Ok(Observed::Integer);
    }
    let lexical = bytes::lex_slice(value, profile).map_err(conversion_failure)?;
    Ok(
        if lexical.consumed == value.len() && !matches!(lexical.lexeme, Lexeme::NoConversion) {
            Observed::OtherNumber
        } else {
            Observed::Text
        },
    )
}

#[derive(Clone, Copy)]
pub(super) enum Baseline {
    Unavailable,
    Declared(usize),
    Header(usize),
    Modal(usize),
}

impl Baseline {
    pub fn width(self) -> Option<usize> {
        match self {
            Self::Unavailable => None,
            Self::Declared(width) | Self::Header(width) | Self::Modal(width) => Some(width),
        }
    }

    pub fn source(self) -> &'static [u8] {
        match self {
            Self::Unavailable => b"unavailable",
            Self::Declared(_) => b"declared",
            Self::Header(_) => b"header",
            Self::Modal(_) => b"modal",
        }
    }

    pub fn basis(self) -> Basis {
        if matches!(self, Self::Declared(_)) {
            Basis::Declared
        } else {
            Basis::Inferred
        }
    }
}

pub(super) struct Report {
    pub data_records: u64,
    pub header_records: u64,
    pub accepted_records: u64,
    pub baseline: Baseline,
    pub maximum_width: Option<usize>,
    pub header: Vec<Vec<u8>>,
    pub fields: Vec<Field>,
    pub findings: Vec<Finding>,
}

fn add(count: &mut u64, amount: u64) -> Result<(), Failure> {
    let next = count
        .checked_add(amount)
        .ok_or_else(|| unsupported("health count exceeds u64 limit"))?;
    #[cfg(test)]
    if COUNT_LIMIT.with(|limit| limit.get().is_some_and(|limit| next > limit)) {
        return Err(unsupported("health count exceeds u64 limit"));
    }
    *count = next;
    Ok(())
}

fn retain(examples: &mut Vec<Example>, example: Example, limit: usize) -> Result<(), Failure> {
    let at = examples.partition_point(|previous| previous.record < example.record);
    if at >= limit {
        return Ok(());
    }
    if examples.len() == limit {
        examples.pop();
    }
    reserve(examples, 1)?;
    examples.insert(at, example);
    Ok(())
}

fn bind(report: &mut Report, settings: &Settings, utf8: bool) -> Result<(), Failure> {
    reserve(&mut report.fields, report.header.len())?;
    report
        .fields
        .extend((0..report.header.len()).map(|position| Field {
            position,
            ..Field::default()
        }));
    for declaration in &settings.declarations {
        let position = match &declaration.field {
            grammar::Field::Number(position) => usize::try_from(position - 1)
                .map_err(|_| unsupported("health field position exceeds checked capacity"))?,
            grammar::Field::Name(name) => {
                let mut matches = report
                    .header
                    .iter()
                    .enumerate()
                    .filter(|(_, label)| *label == name);
                let first = matches.next();
                let reason = if first.is_none() {
                    Some(b" not found in Input header\n".as_slice())
                } else if matches.next().is_some() {
                    Some(b" is ambiguous in Input header\n".as_slice())
                } else {
                    None
                };
                if let Some(reason) = reason {
                    let mut message = b"health: field name ".to_vec();
                    super::named_fields::quote_with(name, &mut message, utf8);
                    message.extend_from_slice(reason);
                    return Err(failure(message));
                }
                first.unwrap().0
            }
        };
        let at = match report
            .fields
            .binary_search_by_key(&position, |field| field.position)
        {
            Ok(at) => at,
            Err(at) => {
                reserve(&mut report.fields, 1)?;
                report.fields.insert(
                    at,
                    Field {
                        position,
                        ..Field::default()
                    },
                );
                at
            }
        };
        let state = &mut report.fields[at];
        match declaration.rule {
            Rule::Presence(presence) if state.presence != Some(Presence::Nonmissing) => {
                state.presence = Some(presence);
            }
            Rule::Type(kind) => {
                if state.declared_type.is_some_and(|previous| previous != kind) {
                    return Err(failure(
                        format!("health: conflicting types for field {}\n", position + 1)
                            .into_bytes(),
                    ));
                }
                state.declared_type = Some(kind);
            }
            _ => {}
        }
    }
    Ok(())
}

fn scan(
    reader: &mut impl BufRead,
    options: &Options,
    settings: &Settings,
) -> Result<Report, Failure> {
    let mut intake = Intake::new(options, intake::Header::of(options));
    let mut bytes = Vec::new();
    let mut widths: Vec<Width> = Vec::new();
    let mut report = Report {
        data_records: 0,
        header_records: 0,
        accepted_records: 0,
        baseline: settings
            .width
            .map_or(Baseline::Unavailable, Baseline::Declared),
        maximum_width: None,
        header: Vec::new(),
        fields: Vec::new(),
        findings: Vec::new(),
    };
    if intake.header_captured(
        reader,
        &mut bytes,
        |raw| Example::new(1, Observed::Width(0), raw),
        |bytes, mut example| {
            for span in records::fields(bytes, options.input) {
                let mut name = Vec::new();
                reserve(&mut name, span.length)?;
                name.extend_from_slice(&bytes[span.start..span.start + span.length]);
                reserve(&mut report.header, 1)?;
                report.header.push(name);
            }
            if settings
                .width
                .is_some_and(|width| width != report.header.len())
            {
                let mut examples = Vec::new();
                if settings.examples != 0 {
                    example.observed = Observed::Width(report.header.len());
                    reserve(&mut examples, 1)?;
                    examples.push(example);
                }
                reserve(&mut report.findings, 1)?;
                report.findings.push(Finding {
                    category: Category::HeaderWidthMismatch,
                    basis: Basis::Declared,
                    count: 1,
                    field: None,
                    expected: settings.width.map(Expected::Width),
                    candidates: 1,
                    examples,
                });
            }
            Ok(())
        },
    )? {
        report.header_records = 1;
        if settings.width.is_none() {
            report.baseline = Baseline::Header(report.header.len());
        }
    }
    if let Some(error) = intake.read_error() {
        return Err(intake::read_failure(error));
    }
    bind(&mut report, settings, options.locale.utf8)?;
    while let Some(record) = intake.next(reader, &mut bytes)? {
        add(&mut report.data_records, 1)?;
        let mut width = 0usize;
        for span in records::fields(record.data(), options.input) {
            if report
                .fields
                .get(width)
                .is_none_or(|field| field.position != width)
            {
                reserve(&mut report.fields, 1)?;
                report.fields.insert(
                    width,
                    Field {
                        position: width,
                        ..Field::default()
                    },
                );
            }
            let field = &mut report.fields[width];
            let value = &record.data()[span.start..span.start + span.length];
            let observed = if value.is_empty() {
                Observed::Empty
            } else if field_policy::is_na(value) {
                Observed::Missing
            } else {
                classify(value, options.locale.numeric)?
            };
            let (count, examples) = match observed {
                Observed::Empty => (&mut field.empty, &mut field.empty_examples),
                Observed::Missing => (&mut field.missing, &mut field.missing_examples),
                Observed::Integer => (&mut field.integer, &mut field.numeric_examples),
                Observed::OtherNumber => (&mut field.other_number, &mut field.numeric_examples),
                Observed::Text => (&mut field.text, &mut field.text_examples),
                _ => unreachable!(),
            };
            add(count, 1)?;
            if examples.len() < settings.examples {
                reserve(examples, 1)?;
                examples.push(Example::new(intake.line(), observed, value));
            }
            if matches!(observed, Observed::OtherNumber)
                && field.declared_type == Some(Type::Integer)
                && field.other_number_examples.len() < settings.examples
            {
                reserve(&mut field.other_number_examples, 1)?;
                field
                    .other_number_examples
                    .push(Example::new(intake.line(), observed, value));
            }
            width = width
                .checked_add(1)
                .ok_or_else(|| unsupported("health field count exceeds checked capacity"))?;
        }
        report.maximum_width = Some(
            report
                .maximum_width
                .map_or(width, |maximum| maximum.max(width)),
        );
        let at = match widths.binary_search_by_key(&width, |bin| bin.width) {
            Ok(at) => at,
            Err(at) => {
                reserve(&mut widths, 1)?;
                widths.insert(
                    at,
                    Width {
                        width,
                        count: 0,
                        examples: Vec::new(),
                    },
                );
                at
            }
        };
        let bin = &mut widths[at];
        add(&mut bin.count, 1)?;
        if bin.examples.len() < settings.examples {
            reserve(&mut bin.examples, 1)?;
            bin.examples.push(Example::new(
                intake.line(),
                Observed::Width(width),
                record.raw(),
            ));
        }
    }
    report.accepted_records = intake.line();
    intake.finish()?;
    // Widths are sorted ascending, so equal-frequency ties retain the smaller width.
    if matches!(report.baseline, Baseline::Unavailable) {
        let mut frequency = 0;
        for bin in &widths {
            if bin.count > frequency {
                report.baseline = Baseline::Modal(bin.width);
                frequency = bin.count;
            }
        }
    }
    inspect_absences(&mut report.fields, &widths, settings)?;
    for state in &mut report.fields {
        state.infer()?;
    }
    let mut mismatch = Finding {
        category: Category::WidthMismatch,
        basis: report.baseline.basis(),
        count: 0,
        field: None,
        expected: report.baseline.width().map(Expected::Width),
        candidates: 0,
        examples: Vec::new(),
    };
    for bin in widths {
        if Some(bin.width) != report.baseline.width() {
            add(&mut mismatch.count, bin.count)?;
            for example in bin.examples {
                retain(&mut mismatch.examples, example, settings.examples)?;
            }
        }
    }
    if mismatch.count != 0 {
        mismatch.candidates = mismatch.count;
        reserve(&mut report.findings, 1)?;
        report.findings.push(mismatch);
    }
    inspect_header(&mut report, settings)?;
    for category in [
        Category::AbsentField,
        Category::EmptyField,
        Category::MissingIndicator,
    ] {
        for state in &mut report.fields {
            let (count, examples) = match category {
                Category::AbsentField => (state.absent, &mut state.absent_examples),
                Category::EmptyField => (state.empty, &mut state.empty_examples),
                Category::MissingIndicator => (state.missing, &mut state.missing_examples),
                _ => unreachable!(),
            };
            if count != 0 {
                reserve(&mut report.findings, 1)?;
                report.findings.push(Finding {
                    category,
                    basis: Basis::Observation,
                    count,
                    field: Some(state.position),
                    expected: None,
                    candidates: count,
                    examples: {
                        if state.presence.is_none() {
                            std::mem::take(examples)
                        } else {
                            let mut retained = Vec::new();
                            reserve(&mut retained, examples.len())?;
                            retained.extend_from_slice(examples);
                            retained
                        }
                    },
                });
            }
        }
    }
    inspect_types(&mut report, settings)?;
    inspect_presence(&mut report, settings)?;
    Ok(report)
}

fn mixed_examples(state: &Field, limit: usize) -> Result<Vec<Example>, Failure> {
    let mut numeric = state.numeric_examples.iter().peekable();
    let mut text = state.text_examples.iter().peekable();
    let mut examples = Vec::new();
    if limit >= 2 {
        reserve(&mut examples, 2)?;
        examples.push(*numeric.next().unwrap());
        examples.push(*text.next().unwrap());
    }
    while examples.len() < limit {
        let next = match (numeric.peek(), text.peek()) {
            (Some(a), Some(b)) if a.record < b.record => numeric.next(),
            (Some(_), Some(_)) | (None, Some(_)) => text.next(),
            (Some(_), None) => numeric.next(),
            (None, None) => break,
        };
        reserve(&mut examples, 1)?;
        examples.push(*next.unwrap());
    }
    examples.sort_unstable_by_key(|example| example.record);
    Ok(examples)
}

fn inspect_types(report: &mut Report, settings: &Settings) -> Result<(), Failure> {
    for category in [Category::MixedTypes, Category::TypeMismatch] {
        for basis in [Basis::Observation, Basis::Inferred, Basis::Declared] {
            for state in &mut report.fields {
                let actual_basis = if matches!(category, Category::MixedTypes) {
                    Basis::Observation
                } else if state.declared_type.is_some() {
                    Basis::Declared
                } else {
                    Basis::Inferred
                };
                if basis != actual_basis {
                    continue;
                }
                let numeric = state.numeric_count()?;
                let (count, expected, candidates, examples) = match category {
                    Category::MixedTypes if numeric != 0 && state.text != 0 => {
                        let mut candidates = numeric;
                        add(&mut candidates, state.text)?;
                        (
                            1,
                            None,
                            candidates,
                            mixed_examples(state, settings.examples)?,
                        )
                    }
                    Category::TypeMismatch
                        if matches!(state.expectation, Type::Integer | Type::Number) =>
                    {
                        let mut count = state.text;
                        let mut examples = std::mem::take(&mut state.text_examples);
                        if state.expectation == Type::Integer && state.declared_type.is_some() {
                            add(&mut count, state.other_number)?;
                            for example in &state.other_number_examples {
                                retain(&mut examples, *example, settings.examples)?;
                            }
                        }
                        if count == 0 {
                            continue;
                        }
                        (
                            count,
                            Some(Expected::Type(state.expectation)),
                            count,
                            examples,
                        )
                    }
                    _ => continue,
                };
                reserve(&mut report.findings, 1)?;
                report.findings.push(Finding {
                    category,
                    basis,
                    count,
                    field: Some(state.position),
                    expected,
                    candidates,
                    examples,
                });
            }
        }
    }
    Ok(())
}

fn inspect_absences(
    fields: &mut [Field],
    widths: &[Width],
    settings: &Settings,
) -> Result<(), Failure> {
    let mut widths = widths.iter().peekable();
    let mut count = 0;
    let mut examples = Vec::new();
    for state in fields {
        // A width at or below this zero-based position leaves its field absent.
        while widths.peek().is_some_and(|bin| bin.width <= state.position) {
            let bin = widths.next().unwrap();
            add(&mut count, bin.count)?;
            for example in &bin.examples {
                retain(
                    &mut examples,
                    Example::new(example.record, Observed::Absent, &[]),
                    settings.examples,
                )?;
            }
        }
        state.absent = count;
        if count != 0 {
            reserve(&mut state.absent_examples, examples.len())?;
            state.absent_examples.extend(
                examples
                    .iter()
                    .map(|example| Example::new(example.record, Observed::Absent, &[])),
            );
        }
    }
    Ok(())
}

fn inspect_presence(report: &mut Report, settings: &Settings) -> Result<(), Failure> {
    for state in &report.fields {
        let Some(presence) = state.presence else {
            continue;
        };
        let mut count = state.absent;
        add(&mut count, state.empty)?;
        if presence == Presence::Nonmissing {
            add(&mut count, state.missing)?;
        }
        if count == 0 {
            continue;
        }
        let mut examples = Vec::new();
        let missing = if presence == Presence::Nonmissing {
            state.missing_examples.as_slice()
        } else {
            &[]
        };
        for sample in state
            .absent_examples
            .iter()
            .chain(&state.empty_examples)
            .chain(missing)
        {
            retain(&mut examples, *sample, settings.examples)?;
        }
        reserve(&mut report.findings, 1)?;
        report.findings.push(Finding {
            category: Category::PresenceMismatch,
            basis: Basis::Declared,
            count,
            field: Some(state.position),
            expected: Some(Expected::Presence(presence)),
            candidates: count,
            examples,
        });
    }
    Ok(())
}

fn inspect_header(report: &mut Report, settings: &Settings) -> Result<(), Failure> {
    let mut positions = Vec::new();
    reserve(&mut positions, report.header.len())?;
    positions.extend(0..report.header.len());
    positions.sort_unstable_by(|&a, &b| report.header[a].cmp(&report.header[b]));
    let mut duplicate = Vec::new();
    reserve(&mut duplicate, report.header.len())?;
    duplicate.resize(report.header.len(), false);
    for pair in positions.windows(2) {
        if !report.header[pair[0]].is_empty() && report.header[pair[0]] == report.header[pair[1]] {
            duplicate[pair[0]] = true;
            duplicate[pair[1]] = true;
        }
    }
    for category in [Category::BlankHeader, Category::DuplicateHeader] {
        for (field, name) in report.header.iter().enumerate() {
            let observed = match category {
                Category::BlankHeader if name.is_empty() => Observed::Blank,
                Category::DuplicateHeader if duplicate[field] => Observed::Duplicate,
                _ => continue,
            };
            let mut examples = Vec::new();
            if settings.examples != 0 {
                reserve(&mut examples, 1)?;
                examples.push(Example::new(1, observed, name));
            }
            reserve(&mut report.findings, 1)?;
            report.findings.push(Finding {
                category,
                basis: Basis::Observation,
                count: 1,
                field: Some(field),
                expected: None,
                candidates: 1,
                examples,
            });
        }
    }
    Ok(())
}

pub(super) fn run(
    reader: &mut impl BufRead,
    writer: &mut impl command_output::Transport,
    options: &Options,
    style: super::terminal_style::Style,
    report_failure: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let settings = Settings::parse(&options.operands[1..], options)?;
    let (capacity, line_buffered) = writer.buffering();
    let mut output =
        BufferedStdout::new(writer, capacity, line_buffered).map_err(|e| os_failure(&e, false))?;
    let mut validation_status = 0;
    let result = scan(reader, options, &settings).map(|report| {
        if settings.validate && report.findings.iter().any(Finding::is_violation) {
            validation_status = 1;
        }
        if settings.tsv {
            super::health_render::tsv(&mut output, &report, options, &settings);
        } else {
            super::health_render::readable(&mut output, &report, options, &settings, style);
        }
    });
    let status = command_output::complete(
        output,
        result,
        command_output::Transport::close,
        report_failure,
    );
    Ok(super::failure::combine(status, validation_status))
}
