#![deny(unsafe_op_in_unsafe_fn)]
#![warn(clippy::undocumented_unsafe_blocks)]
// The process entry is `main` below, not Rust's runtime start (see there).
#![cfg_attr(not(test), no_main)]
#[cfg(not(all(
    target_arch = "x86_64",
    target_os = "linux",
    target_pointer_width = "64"
)))]
compile_error!("Fastmash supports only Linux x86-64");
// Arguments and the standard-stream capture come from glibc initializers
// (see `main` below); other C libraries would run with no arguments.
#[cfg(not(target_env = "gnu"))]
compile_error!("Fastmash requires glibc (target_env = \"gnu\")");
mod normality;
use normality::{dagostino_pearson, jarque_bera};
mod moments;
use moments::{excess_kurtosis, skewness};
mod annotated;
mod base64_fields;
mod boundary_exp;
mod checksum;
mod collation_locales;
mod command_memory;
mod command_output;
mod crosstab;
mod decimal;
mod dispersion;
mod grammar;
mod guarded_log;
mod line_numeric;
mod linux;
mod locale;
mod log_cache;
mod log_table;
mod mean_math;
mod named_fields;
mod numeric_locales;
mod numerics;
mod options;
mod ordered_statistics;
mod paired;
#[cfg(test)]
mod paired_sharing_tests;
mod path_fields;
mod presentation;
mod projected_sort;
mod random;
mod records;
mod robust_statistics;
#[cfg(test)]
mod routing_tests;
mod samples;
mod scalar_text;
mod sorted_input;
mod standard_io;
mod table_checks;
mod table_modes;
mod text_order;
mod text_samples;
mod transpose;
#[cfg(test)]
use fastmash_conversion::format::{self, FormatId};
use fastmash_conversion::{field_policy, profile::Profile};
use std::{
    io::{self, BufRead, Write},
    os::unix::ffi::OsStrExt,
};

struct Failure {
    status: i32,
    message: Vec<u8>,
}
fn unsupported(reason: &str) -> Failure {
    Failure {
        status: 77,
        message: format!("{reason}\n").into_bytes(),
    }
}
fn unsupported_hint(reason: &str, hint: &str) -> Failure {
    let mut error = unsupported(reason);
    error.message.extend_from_slice(b"hint: ");
    error.message.extend_from_slice(hint.as_bytes());
    error.message.push(b'\n');
    error
}
fn failure(message: Vec<u8>) -> Failure {
    Failure { status: 1, message }
}
fn numeric_failure(error: fastmash_portable_numerics::NumericFailure) -> Failure {
    use fastmash_portable_numerics::NumericFailure;
    let (status, message) = match error {
        NumericFailure::UnsupportedDomain => (
            77,
            "numerical operation result is outside the supported domain\n",
        ),
        NumericFailure::Capacity => (77, "numerical arithmetic capacity exceeded\n"),
        NumericFailure::Allocation => (77, "numerical arithmetic memory allocation failed\n"),
        NumericFailure::Invariant => (70, "internal numerical invariant failure\n"),
        NumericFailure::TableIdentity => (70, "numerical table identity check failed\n"),
    };
    Failure {
        status,
        message: message.as_bytes().to_vec(),
    }
}
fn conversion_failure(error: fastmash_conversion::profile::Error) -> Failure {
    use fastmash_conversion::profile::Error;
    unsupported(match error {
        Error::InputCapacity => "numeric conversion input exceeds 8192-byte limit",
        Error::OutputCapacity => "numeric output exceeds 16384-byte limit",
        Error::CoefficientCapacity => "numeric conversion coefficient capacity exceeded",
        Error::IntermediateCapacity => "numeric conversion intermediate capacity exceeded",
        Error::PowerCapacity => "numeric conversion power capacity exceeded",
        Error::ShiftCapacity => "numeric conversion shift capacity exceeded",
        Error::UnsupportedProfile => "unsupported numerical conversion profile",
        Error::UnsupportedFormat => "unsupported numerical output format",
        Error::Allocation => "numeric conversion memory allocation failed",
        Error::InvalidView | Error::InternalInvariant | Error::Encoding(_) => {
            "internal numeric conversion failure"
        }
    })
}
fn os_failure(error: &io::Error, reading: bool) -> Failure {
    match (reading, error.raw_os_error()) {
        (true, Some(9)) => failure(b"read error: Bad file descriptor\n".to_vec()),
        (false, Some(9)) => failure(b"write error: Bad file descriptor\n".to_vec()),
        (true, Some(21)) => failure(b"read error: Is a directory\n".to_vec()),
        (false, Some(28)) => failure(b"write error: No space left on device\n".to_vec()),
        (true, _) => unsupported("unsupported input I/O error"),
        (false, _) => unsupported("unsupported output I/O error"),
    }
}

fn setup_sigpipe() -> Result<(), Failure> {
    setup_sigpipe_with(
        || fastmash_sort_process::linux::mask(None).map_err(|_| ()),
        || fastmash_sort_process::linux::default_sigpipe().map_err(|_| ()),
    )
}
fn setup_sigpipe_with(
    inspect: impl FnOnce() -> Result<u64, ()>,
    restore: impl FnOnce() -> Result<(), ()>,
) -> Result<(), Failure> {
    let mask = inspect().map_err(|_| unsupported("unable to inspect runtime signal mask"))?;
    if mask & (1 << 12) != 0 {
        return Err(unsupported("blocked SIGPIPE is unsupported"));
    }
    restore().map_err(|_| unsupported("unable to restore runtime SIGPIPE disposition"))
}

fn selected_field(
    record: &[u8],
    field: u64,
    line: u64,
    delimiter: records::Separator,
) -> Result<field_policy::FieldRange, Failure> {
    records::field(record, field, delimiter).map_err(|fields| {
        failure(
            format!(
                "invalid input: field {field} requested, line {line} has only {fields} fields\n"
            )
            .into_bytes(),
        )
    })
}

fn field_value(
    record: &[u8],
    field: u64,
    line: u64,
    delimiter: records::Separator,
    narm: bool,
    profile: Profile,
) -> Result<Option<numerics::Value>, Failure> {
    let span = selected_field(record, field, line, delimiter)?;
    decimal::field(record, span, field, line, narm, profile)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Cut,
    Base64,
    Debase64,
    Checksum(checksum::Algorithm),
    Path(path_fields::Operation),
    Rounding(line_numeric::Rounding),
    Getnum(line_numeric::Extraction),
    Bin(fastmash_numeric_contract::Raw80),
    Strbin(std::num::NonZeroU64),
    Count,
    Countunique,
    Unique,
    Collapse,
    First,
    Last,
    Rand,
    Min,
    Max,
    Absmin,
    Absmax,
    Range,
    Sum,
    Mean,
    Geomean,
    Harmmean,
    Ms,
    Rms,
    Median,
    Mode,
    Antimode,
    Q1,
    Q3,
    Iqr,
    Percentile(u8),
    Trimmean(ordered_statistics::Trim),
    Pvar,
    Svar,
    Pstdev,
    Sstdev,
    Madraw,
    Mad,
    Pskew,
    Sskew,
    Pkurt,
    Skurt,
    Jarque,
    Dpo,
    Pcov,
    Scov,
    Ppearson,
    Spearson,
    Dotprod,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selector {
    Single(u64),
    Pair { left: u64, right: u64 },
}
impl Kind {
    /// Whether the operation depends on the numeric locale: it reads numbers,
    /// or prints a number (counts only matter with an explicit format).
    fn reads_or_prints_numbers(self, explicit_format: bool) -> bool {
        match self {
            Self::Cut
            | Self::Base64
            | Self::Debase64
            | Self::Checksum(_)
            | Self::Path(_)
            | Self::Unique
            | Self::Collapse
            | Self::First
            | Self::Last
            | Self::Rand => false,
            Self::Count | Self::Countunique => explicit_format,
            // Default output prints integers below 1e14 without a decimal point.
            Self::Strbin(buckets) => explicit_format || buckets.get() > 100_000_000_000_000,
            _ => true,
        }
    }
    fn converts_numeric_field(self) -> bool {
        match self {
            Self::Min
            | Self::Max
            | Self::Absmin
            | Self::Absmax
            | Self::Range
            | Self::Sum
            | Self::Mean
            | Self::Geomean
            | Self::Harmmean
            | Self::Ms
            | Self::Rms
            | Self::Median
            | Self::Mode
            | Self::Antimode
            | Self::Q1
            | Self::Q3
            | Self::Iqr
            | Self::Percentile(_)
            | Self::Trimmean(_)
            | Self::Pvar
            | Self::Svar
            | Self::Pstdev
            | Self::Sstdev
            | Self::Madraw
            | Self::Mad
            | Self::Pskew
            | Self::Sskew
            | Self::Pkurt
            | Self::Skurt
            | Self::Jarque
            | Self::Dpo
            | Self::Rounding(_)
            | Self::Bin(_) => true,
            Self::Cut
            | Self::Base64
            | Self::Debase64
            | Self::Checksum(_)
            | Self::Path(_)
            | Self::Getnum(_)
            | Self::Strbin(_)
            | Self::Count
            | Self::Countunique
            | Self::Unique
            | Self::Collapse
            | Self::First
            | Self::Last
            | Self::Rand
            | Self::Pcov
            | Self::Scov
            | Self::Ppearson
            | Self::Spearson
            | Self::Dotprod => false,
        }
    }
    fn is_line(self) -> bool {
        matches!(
            self,
            Self::Cut
                | Self::Base64
                | Self::Debase64
                | Self::Checksum(_)
                | Self::Path(_)
                | Self::Rounding(_)
                | Self::Getnum(_)
                | Self::Bin(_)
                | Self::Strbin(_)
        )
    }
    fn uses_borrowed_number(self) -> bool {
        self.uses_shared_sorted_samples()
            || self.is_mad()
            || self.is_dispersion()
            || matches!(
                self,
                Self::Sum
                    | Self::Rounding(_)
                    | Self::Bin(_)
                    | Self::Mean
                    | Self::Min
                    | Self::Max
                    | Self::Absmin
                    | Self::Absmax
                    | Self::Range
            )
    }
    fn is_mad(self) -> bool {
        matches!(self, Self::Madraw | Self::Mad)
    }
    fn is_dispersion(self) -> bool {
        matches!(self, Self::Pvar | Self::Svar | Self::Pstdev | Self::Sstdev)
    }
    fn uses_shared_sorted_samples(self) -> bool {
        matches!(
            self,
            Self::Median
                | Self::Q1
                | Self::Q3
                | Self::Iqr
                | Self::Percentile(_)
                | Self::Trimmean(_)
                | Self::Mode
                | Self::Antimode
        )
    }
    fn is_moment(self) -> bool {
        matches!(self, Self::Pskew | Self::Sskew | Self::Pkurt | Self::Skurt)
    }
    fn is_normality(self) -> bool {
        matches!(self, Self::Jarque | Self::Dpo)
    }
}

struct Operation {
    kind: Kind,
    selector: Selector,
    value: numerics::Value,
    maximum: numerics::Value,
    count: u64,
    text: scalar_text::ScalarText,
    samples: samples::Samples,
    text_samples: text_samples::TextSamples,
    pair_samples: Option<paired::PairSamples>,
    sum_source: Option<usize>,
    sample_source: Option<usize>,
    raw_deviation: Option<numerics::Value>,
    moments: moments::Cache,
    dispersion: dispersion::Dispersion,
    dispersion_source: Option<usize>,
    conversion_source: Option<usize>,
    /// Earlier single-field operations that already parse a pair's left and
    /// right fields in each record, whose values the pair reuses.
    pair_conversion: [Option<usize>; 2],
    parsed: Option<numerics::Value>,
}
impl Operation {
    fn reset(&mut self) {
        self.value = integer(0);
        self.maximum = integer(0);
        self.count = 0;
        self.parsed = None;
        self.text.clear();
        self.samples.clear();
        self.raw_deviation = None;
        self.moments.clear();
        self.dispersion.clear();
        self.text_samples.clear();
        if let Some(samples) = &mut self.pair_samples {
            samples.reset();
        }
    }
}
fn integer(n: u64) -> numerics::Value {
    numerics::Numerics::promote_u64(n)
}
fn operations(
    requests: Vec<grammar::Request>,
) -> Result<(Vec<Operation>, Vec<named_fields::Named>), Failure> {
    let mut names = Vec::new();
    command_memory::reserve(
        &mut names,
        requests
            .len()
            .checked_mul(2)
            .ok_or_else(|| unsupported("command memory allocation failed"))?,
    )?;
    let mut operations = Vec::new();
    command_memory::reserve(&mut operations, requests.len())?;
    for (index, request) in requests.into_iter().enumerate() {
        let mut resolve = |field, target| match field {
            grammar::Field::Number(field) => field,
            grammar::Field::Name(name) => {
                names.push(named_fields::Named {
                    operation: index,
                    target,
                    name,
                });
                0
            }
        };
        let selector = match request.selector {
            grammar::Selector::Single(field) => {
                Selector::Single(resolve(field, named_fields::Target::Single))
            }
            grammar::Selector::Pair { left, right } => Selector::Pair {
                left: resolve(left, named_fields::Target::Left),
                right: resolve(right, named_fields::Target::Right),
            },
        };
        let pair_samples = matches!(
            request.kind,
            Kind::Pcov | Kind::Scov | Kind::Ppearson | Kind::Spearson | Kind::Dotprod
        )
        .then(paired::PairSamples::default);
        operations.push(Operation {
            kind: request.kind,
            selector,
            value: integer(0),
            maximum: integer(0),
            count: 0,
            text: scalar_text::ScalarText::default(),
            samples: samples::Samples::default(),
            text_samples: text_samples::TextSamples::default(),
            pair_samples,
            sum_source: None,
            sample_source: None,
            raw_deviation: None,
            moments: moments::Cache::default(),
            dispersion: dispersion::Dispersion::default(),
            dispersion_source: None,
            conversion_source: None,
            pair_conversion: [None, None],
            parsed: None,
        });
    }
    Ok((operations, names))
}

fn apply_named(operation: &mut Operation, target: named_fields::Target, field: u64) {
    match (&mut operation.selector, target) {
        (Selector::Single(value), named_fields::Target::Single)
        | (Selector::Pair { left: value, .. }, named_fields::Target::Left)
        | (Selector::Pair { right: value, .. }, named_fields::Target::Right) => *value = field,
        _ => unreachable!("named selector target matches parsed selector"),
    }
}

fn link_shared_fields(operations: &mut [Operation]) {
    for index in 0..operations.len() {
        if let Selector::Pair { left, right } = operations[index].selector {
            operations[index].sample_source =
                (0..index).find(|&prior| operations[prior].selector == operations[index].selector);
            // The first numeric request on a field parses it in every record.
            operations[index].pair_conversion = [left, right].map(|field| {
                (0..index).find(|&prior| {
                    operations[prior].kind.converts_numeric_field()
                        && operations[prior].selector == Selector::Single(field)
                })
            });
            continue;
        }
        if operations[index].kind.converts_numeric_field() {
            operations[index].conversion_source = (0..index).find(|&prior| {
                operations[prior].kind.converts_numeric_field()
                    && operations[prior].selector == operations[index].selector
            });
        }
        if matches!(operations[index].kind, Kind::Sum | Kind::Mean) {
            operations[index].sum_source = (0..index).find(|&prior| {
                matches!(operations[prior].kind, Kind::Sum | Kind::Mean)
                    && operations[prior].selector == operations[index].selector
            });
        }
        if operations[index].kind.is_dispersion() {
            operations[index].dispersion_source = (0..index).find(|&prior| {
                operations[prior].kind.is_dispersion()
                    && operations[prior].selector == operations[index].selector
            });
        }
        if operations[index].kind.uses_shared_sorted_samples()
            || operations[index].kind.is_mad()
            || (operations[index].kind.is_moment() || operations[index].kind.is_normality())
        {
            operations[index].sample_source = (0..index).find(|&prior| {
                ((operations[prior].kind.uses_shared_sorted_samples()
                    && operations[index].kind.uses_shared_sorted_samples())
                    || (operations[prior].kind.is_mad() && operations[index].kind.is_mad())
                    || ((operations[prior].kind.is_moment()
                        || operations[prior].kind.is_normality())
                        && (operations[index].kind.is_moment()
                            || operations[index].kind.is_normality())))
                    && operations[prior].selector == operations[index].selector
            });
        }
    }
}

fn initialize_random(
    operations: &[Operation],
    explicit_seed: Option<u32>,
    source: &mut impl random::SeedSource,
) -> Result<Option<random::RandomState>, Failure> {
    operations
        .iter()
        .any(|operation| operation.kind == Kind::Rand)
        .then(|| random::RandomState::initialize(explicit_seed, source))
        .transpose()
        .map_err(|()| unsupported("unable to initialize random state"))
}

fn initialize_before_input<T>(
    sorted: bool,
    operations: &[Operation],
    explicit_seed: Option<u32>,
    source: &mut impl random::SeedSource,
    command_setup: impl FnOnce() -> Result<(), Failure>,
    startup: impl FnOnce() -> Result<T, Failure>,
    sort_admit: impl FnOnce() -> Result<(), Failure>,
) -> Result<(T, Option<random::RandomState>), Failure> {
    command_setup()?;
    let arithmetic = startup()?;
    let random = initialize_random(operations, explicit_seed, source)?;
    if sorted {
        sort_admit()?;
    }
    Ok((arithmetic, random))
}

fn run(
    reader: &mut impl BufRead,
    writer: &mut impl command_output::Transport,
    args: &[std::ffi::OsString],
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    run_with_seed_source(
        reader,
        writer,
        args,
        name,
        report,
        &mut random::OsSeedSource,
    )
}

fn run_with_seed_source(
    reader: &mut impl BufRead,
    writer: &mut impl command_output::Transport,
    args: &[std::ffi::OsString],
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
    seed_source: &mut impl random::SeedSource,
) -> Result<i32, Failure> {
    let policy = locale::Policy::from_env();
    let action = options::parse_with_policy(
        args,
        name,
        std::env::var_os("POSIXLY_CORRECT").is_some(),
        policy,
    );
    let information: Option<&[u8]> = match &action {
        Ok(options::Action::Help) => Some(include_bytes!("help.txt")),
        Ok(options::Action::Version) => {
            Some(concat!("fastmash ", env!("CARGO_PKG_VERSION"), "\n").as_bytes())
        }
        _ => None,
    };
    if let Some(output) = information {
        return records::write_output(writer, output)
            .map(|()| 0)
            .map_err(|e| os_failure(&e, false));
    }
    let profile = policy.numeric;
    let options::Action::Calculate(mut options) = action? else {
        unreachable!("informational action already handled")
    };
    options.presentation.profile = profile;
    let command = grammar::command_for_format(
        &options.operands,
        options.group.as_ref(),
        options.vnlog,
        profile,
        policy.utf8,
    )?;
    options.linewise = command.mode == grammar::Mode::Line;
    options.crosstab = command.mode == grammar::Mode::Crosstab;
    // GNU's rmdup compares keys exactly; -i only changes its sort order.
    if options.ignore_case
        && match command.mode {
            grammar::Mode::Aggregate | grammar::Mode::Crosstab => true,
            grammar::Mode::Dedup => options.sort,
            _ => false,
        }
    {
        policy.case_folding()?;
    }
    if matches!(
        command.mode,
        grammar::Mode::Reverse
            | grammar::Mode::Transpose
            | grammar::Mode::Noop
            | grammar::Mode::Check { .. }
            | grammar::Mode::Dedup
    ) {
        options.validate_annotation()?;
        setup_sigpipe()?;
        return table_modes::run(reader, writer, &options, command.mode, command.keys, report);
    }
    let (mut operations, names) = operations(command.operations)?;
    let explicit_format = options.presentation.spec.is_some();
    if operations
        .iter()
        .any(|operation| operation.kind.reads_or_prints_numbers(explicit_format))
    {
        policy.numbers()?;
    }
    let has_pairs = operations
        .iter()
        .any(|operation| operation.pair_samples.is_some());
    let mut key_names = Vec::new();
    let mut keys = Vec::new();
    command_memory::reserve(&mut keys, command.keys.len())?;
    command_memory::reserve(&mut key_names, command.keys.len())?;
    for (index, field) in command.keys.into_iter().enumerate() {
        keys.push(match field {
            grammar::Field::Number(n) => n,
            grammar::Field::Name(name) => {
                key_names.push(named_fields::Named {
                    operation: index,
                    target: named_fields::Target::Single,
                    name,
                });
                0
            }
        });
    }
    if (!names.is_empty() || !key_names.is_empty()) && !options.header_in {
        return Err(failure(
            b"-H or --header-in must be used with named columns\n".to_vec(),
        ));
    }
    let sorted = options.sort && !keys.is_empty();
    options.validate_annotation()?;
    if sorted {
        policy.sorting()?;
    }
    let projected = sorted && (policy.language() || projected_sort::supports(&operations));
    let (mut arithmetic, mut random) = initialize_before_input(
        sorted && !projected,
        &operations,
        options.seed,
        seed_source,
        setup_sigpipe,
        || {
            numerics::Numerics::with_requirements(
                false,
                operations.iter().any(|op| {
                    matches!(
                        op.kind,
                        Kind::Rms
                            | Kind::Pstdev
                            | Kind::Sstdev
                            | Kind::Ppearson
                            | Kind::Spearson
                            | Kind::Pskew
                            | Kind::Sskew
                            | Kind::Jarque
                            | Kind::Dpo
                    )
                }),
            )
            .and_then(|arithmetic| {
                arithmetic.with_mean_math(
                    operations
                        .iter()
                        .any(|op| op.kind == Kind::Geomean || op.kind.is_normality()),
                )
            })
            .map_err(numeric_failure)
        },
        sorted_input::admit,
    )?;
    if projected {
        return projected_sort::run(
            reader,
            writer,
            &options,
            CalculationFields {
                program: name,
                operations,
                names: &names,
                keys,
                key_names: &key_names,
            },
            &mut arithmetic,
            random.as_mut(),
            report,
        );
    }
    let mut prepared = sorted_input::Header::Unprepared;
    let mut session = None;
    let mut header_errno = None;
    if sorted {
        standard_io::check_input_access().map_err(|error| os_failure(&error, true))?;
        prepared = sorted_input::prepare(&options)?;
        match &prepared {
            sorted_input::Header::Record(record) => {
                for (index, target, field) in
                    named_fields::resolve_with(&names, record, options.input, options.locale.utf8)?
                {
                    apply_named(&mut operations[index], target, field);
                }
                for (index, _, field) in named_fields::resolve_with(
                    &key_names,
                    record,
                    options.input,
                    options.locale.utf8,
                )? {
                    keys[index] = field;
                }
            }
            sorted_input::Header::ReadError(error) => header_errno = error.raw_os_error(),
            _ => {}
        }
        session = Some(sorted_input::start(
            &keys,
            options.input,
            options.record_end,
            options.ignore_case,
        )?);
    }
    let arithmetic = &mut arithmetic;
    if options.full || options.header_out || !keys.is_empty() || has_pairs || options.linewise {
        let transport = writer;
        let (capacity, line_buffered) = transport.buffering();
        let buffer = headers::output::Buffered::new(transport, capacity, line_buffered)
            .map_err(|e| os_failure(&e, false))?;
        let mut output = command_output::Header::new(buffer, &options);
        let calculation_reader: &mut dyn BufRead = match &mut session {
            Some(session) => session.reader(),
            None => reader,
        };
        let result = calculate(
            calculation_reader,
            &mut output,
            &options,
            CalculationFields {
                program: name,
                operations,
                names: &names,
                keys,
                key_names: &key_names,
            },
            arithmetic,
            random.as_mut(),
            prepared,
        );
        let result = match &mut session {
            Some(session) => sorted_input::complete(session, result, header_errno),
            None => result,
        };
        Ok(command_output::complete(
            output.buffer,
            result,
            command_output::Transport::close,
            report,
        ))
    } else {
        let mut output = command_output::Plain {
            writer,
            record_end: options.record_end,
        };
        calculate(
            reader,
            &mut output,
            &options,
            CalculationFields {
                program: name,
                operations,
                names: &names,
                keys,
                key_names: &key_names,
            },
            arithmetic,
            random.as_mut(),
            prepared,
        )
        .map(|()| 0)
    }
}

struct CalculationFields<'a> {
    /// The invoked program name, for the `--full` compatibility warning.
    program: &'a [u8],
    operations: Vec<Operation>,
    names: &'a [named_fields::Named],
    keys: Vec<u64>,
    key_names: &'a [named_fields::Named],
}

fn calculate(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut impl command_output::CommandOutput,
    options: &options::Options,
    fields: CalculationFields<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    prepared: sorted_input::Header,
) -> Result<(), Failure> {
    let CalculationFields {
        program,
        mut operations,
        names,
        mut keys,
        key_names,
    } = fields;
    let mut random = random;
    let mut line = 0u64;
    let limit = usize::MAX;
    let header_prepared = !matches!(prepared, sorted_input::Header::Unprepared);
    let mut warning_pending = options.header_in && !header_prepared;
    if !warning_pending {
        command_output::warn_full(options, program);
    }
    if let sorted_input::Header::Record(record) = prepared {
        line = 1;
        link_shared_fields(&mut operations);
        output.first(&record, &operations, &keys)?;
    }
    let mut record = Vec::new();
    let mut representative: Option<Vec<u8>> = None;
    // Key positions in `representative`, found on first use and kept until it
    // changes, so each record locates only its own keys.
    let mut representative_keys = Vec::new();
    command_memory::reserve(&mut representative_keys, keys.len())?;
    representative_keys.resize(keys.len(), None);
    let read_error = loop {
        match records::read_record_terminated(reader, &mut record, limit, options.record_end) {
            Ok(false) => break None,
            Ok(true) => {}
            Err(records::ReadError::Capacity) => {
                return Err(unsupported("record exceeds addressable size"));
            }
            Err(records::ReadError::Allocation) => {
                return Err(unsupported("record memory allocation failed"));
            }
            Err(records::ReadError::Io(error)) => break Some(error),
        }
        if options.vnlog {
            if !annotated::prepare(&mut record, line == 0 && !header_prepared)? {
                continue;
            }
        } else if options.skip_comments && records::is_comment(&record) {
            continue;
        }
        line = line
            .checked_add(1)
            .ok_or_else(|| unsupported("record count exceeds u64 limit"))?;
        if line == 1 && !header_prepared {
            for (index, target, field) in
                named_fields::resolve_with(names, &record, options.input, options.locale.utf8)?
            {
                apply_named(&mut operations[index], target, field);
            }
            for (index, _, field) in
                named_fields::resolve_with(key_names, &record, options.input, options.locale.utf8)?
            {
                keys[index] = field;
            }
            link_shared_fields(&mut operations);
            if warning_pending {
                command_output::warn_full(options, program);
                warning_pending = false;
            }
            output.first(&record, &operations, &keys)?;
        }
        if options.header_in && line == 1 && !header_prepared {
            continue;
        }
        if !keys.is_empty() || options.full || options.linewise {
            let new_group = match representative.as_deref() {
                None => true,
                Some(previous) => {
                    options.linewise
                        || different(
                            &record,
                            previous,
                            &mut representative_keys,
                            &keys,
                            line,
                            options.input,
                            options.ignore_case,
                        )?
                }
            };
            if new_group {
                if let Some(previous) = representative.as_deref() {
                    finish_group(
                        output,
                        previous,
                        &keys,
                        &mut operations,
                        line,
                        options,
                        arithmetic,
                    )?;
                }
                for operation in &mut operations {
                    operation.reset();
                }
            }
            let keep = collect(
                &record,
                &mut operations,
                line,
                options,
                arithmetic,
                random.as_deref_mut(),
            )?;
            if new_group || keep {
                match &mut representative {
                    Some(previous) => std::mem::swap(previous, &mut record),
                    None => representative = Some(std::mem::take(&mut record)),
                }
                representative_keys.fill(None);
            }
        } else {
            collect(
                &record,
                &mut operations,
                line,
                options,
                arithmetic,
                random.as_deref_mut(),
            )?;
        }
    };
    if warning_pending {
        command_output::warn_full(options, program);
    }
    if let Some(previous) = representative.as_deref() {
        finish_group(
            output,
            previous,
            &keys,
            &mut operations,
            line,
            options,
            arithmetic,
        )?;
    } else if line > u64::from(options.header_in) {
        if operations
            .iter()
            .any(|operation| operation.pair_samples.is_some())
        {
            let operation_count = operations.len();
            for at in 0..operation_count {
                let value = summarize_cached_at(
                    &mut operations,
                    at,
                    arithmetic,
                    options.collapse,
                    &options.presentation,
                    options.ignore_case,
                )?;
                output.result(
                    &value,
                    if at + 1 == operation_count {
                        options.record_end
                    } else {
                        options.output
                    },
                );
            }
        } else {
            let mut fields = Vec::new();
            command_memory::reserve(&mut fields, operations.len())?;
            for at in 0..operations.len() {
                fields.push(summarize_cached_at(
                    &mut operations,
                    at,
                    arithmetic,
                    options.collapse,
                    &options.presentation,
                    options.ignore_case,
                )?);
            }
            output.row(&fields, options.output)?;
        }
    } else {
        output.empty()?;
    }
    output.end(options)?;
    if let Some(error) = read_error {
        if (!keys.is_empty() || options.linewise) && error.raw_os_error() == Some(5) {
            return Err(failure(b"read error: Input/output error\n".to_vec()));
        }
        return Err(os_failure(&error, true));
    }
    Ok(())
}
fn collect(
    record: &[u8],
    operations: &mut [Operation],
    line: u64,
    options: &options::Options,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
) -> Result<bool, Failure> {
    let mut keep_line = false;
    let mut random = random;
    for index in 0..operations.len() {
        let (previous, remaining) = operations.split_at_mut(index);
        let operation = &mut remaining[0];
        if let Some(source) = operation.dispersion_source {
            operation.count = previous[source].count;
            continue;
        }
        if let Some(source) = operation.sample_source {
            operation.count = previous[source].count;
            continue;
        }
        if let Some(source) = operation.sum_source {
            operation.value = previous[source].value;
            operation.count = previous[source].count;
            continue;
        }
        if let Selector::Pair { left, right } = operation.selector {
            let [left_source, right_source] = operation.pair_conversion;
            let samples = operation
                .pair_samples
                .as_mut()
                .expect("paired selectors own paired samples");
            let parse = |source: Option<usize>, field| match source {
                Some(source) => Ok(previous[source].parsed),
                None => field_value(
                    record,
                    field,
                    line,
                    options.input,
                    options.narm,
                    options.presentation.profile,
                ),
            };
            if let Some(value) = parse(left_source, left)? {
                samples
                    .push_left(value)
                    .map_err(|error| sample_failure(operation.kind, error))?;
            }
            if let Some(value) = parse(right_source, right)? {
                samples
                    .push_right(value)
                    .map_err(|error| sample_failure(operation.kind, error))?;
            }
            continue;
        }
        let Selector::Single(field) = operation.selector else {
            unreachable!("paired selectors were handled first")
        };
        if let Kind::Path(kind) = operation.kind {
            let span = selected_field(record, field, line, options.input)?;
            let bytes = &record[span.start..span.start + span.length];
            if options.narm && field_policy::is_na(bytes) {
                continue;
            }
            operation
                .text
                .replace(kind.select(bytes))
                .map_err(scalar_text_failure)?;
            operation.count = 1;
            continue;
        }
        if let Kind::Checksum(algorithm) = operation.kind {
            let span = selected_field(record, field, line, options.input)?;
            let bytes = &record[span.start..span.start + span.length];
            if options.narm && field_policy::is_na(bytes) {
                continue;
            }
            algorithm.store(bytes, &mut operation.text)?;
            operation.count = 1;
            continue;
        }
        if matches!(operation.kind, Kind::Base64 | Kind::Debase64) {
            let span = selected_field(record, field, line, options.input)?;
            let bytes = &record[span.start..span.start + span.length];
            if options.narm && field_policy::is_na(bytes) {
                continue;
            }
            base64_fields::transform(
                &mut operation.text,
                bytes,
                operation.kind == Kind::Debase64,
                line,
                field,
            )?;
            operation.count = 1;
            continue;
        }
        if matches!(operation.kind, Kind::Getnum(_) | Kind::Strbin(_)) {
            let span = selected_field(record, field, line, options.input)?;
            let bytes = &record[span.start..span.start + span.length];
            if options.narm && field_policy::is_na(bytes) {
                continue;
            }
            operation.value = match operation.kind {
                Kind::Getnum(kind) => {
                    line_numeric::extract(bytes, kind, options.presentation.profile)?
                }
                Kind::Strbin(buckets) => integer(line_numeric::strbin(bytes, buckets)),
                _ => unreachable!(),
            };
            operation.count = 1;
            continue;
        }
        if matches!(
            operation.kind,
            Kind::Count
                | Kind::Countunique
                | Kind::Unique
                | Kind::Collapse
                | Kind::Cut
                | Kind::First
                | Kind::Last
                | Kind::Rand
        ) {
            let span = selected_field(record, field, line, options.input)?;
            let bytes = &record[span.start..span.start + span.length];
            if collect_text(operation, bytes, options.narm, random.as_deref_mut())? {
                keep_line = true;
            }
            continue;
        }
        // The first numeric request owns conversion, not the transformed accumulator.
        // Store None too, so a skipped NA never reuses the previous record's value.
        let parsed = match operation.conversion_source {
            Some(source) => previous[source].parsed,
            None => field_value(
                record,
                field,
                line,
                options.input,
                options.narm,
                options.presentation.profile,
            )?,
        };
        operation.parsed = parsed;
        let Some(value) = parsed else {
            continue;
        };
        if let Kind::Rounding(kind) = operation.kind {
            operation.value = line_numeric::rounding(value, kind)?;
            operation.count = 1;
            continue;
        }
        if let Kind::Bin(width) = operation.kind {
            operation.value = line_numeric::bin(value, width)?;
            operation.count = 1;
            continue;
        }
        operation.count = operation
            .count
            .checked_add(1)
            .ok_or_else(|| unsupported("operation count exceeds u64 limit"))?;
        if operation.kind.is_dispersion() {
            operation
                .dispersion
                .push(value)
                .map_err(|error| sample_failure(operation.kind, error))?;
            continue;
        }
        if operation.kind.uses_shared_sorted_samples()
            || operation.kind.is_mad()
            || operation.kind.is_moment()
            || operation.kind.is_normality()
        {
            operation
                .samples
                .push_growable(value)
                .map_err(|error| sample_failure(operation.kind, error))?;
            continue;
        }
        if operation.kind == Kind::Range {
            if operation.count == 1 {
                operation.value = value;
                operation.maximum = value;
            } else {
                if numerics::compare(value, operation.value) == numerics::Comparison::Less {
                    operation.value = value;
                }
                if numerics::compare(value, operation.maximum) == numerics::Comparison::Greater {
                    operation.maximum = value;
                }
            }
            continue;
        }
        if matches!(
            operation.kind,
            Kind::Min | Kind::Max | Kind::Absmin | Kind::Absmax
        ) {
            if operation.count == 1 {
                operation.value = value;
            }
            let relation = if matches!(operation.kind, Kind::Absmin | Kind::Absmax) {
                numerics::compare_magnitude(value, operation.value)
            } else {
                numerics::compare(value, operation.value)
            };
            if matches!(
                (operation.kind, relation),
                (Kind::Min | Kind::Absmin, numerics::Comparison::Less)
                    | (Kind::Max | Kind::Absmax, numerics::Comparison::Greater)
            ) {
                operation.value = value;
                keep_line = true;
            }
            continue;
        }
        let value = if matches!(operation.kind, Kind::Ms | Kind::Rms) {
            decimal::multiply(value, value)?
        } else if operation.kind == Kind::Geomean {
            arithmetic.mean_log(value).map_err(numeric_failure)?
        } else if operation.kind == Kind::Harmmean {
            decimal::divide(integer(1), value)?
        } else {
            value
        };
        operation.value = decimal::add(operation.value, value)?;
    }
    Ok(keep_line)
}

fn collect_text(
    operation: &mut Operation,
    bytes: &[u8],
    narm: bool,
    random: Option<&mut random::RandomState>,
) -> Result<bool, Failure> {
    if narm && field_policy::is_na(bytes) {
        return Ok(false);
    }
    operation.count = operation
        .count
        .checked_add(1)
        .ok_or_else(|| unsupported("operation count exceeds u64 limit"))?;
    if matches!(
        operation.kind,
        Kind::Countunique | Kind::Unique | Kind::Collapse
    ) {
        operation
            .text_samples
            .push(bytes)
            .map_err(|error| text_sample_failure(operation.kind, error))?;
    }
    select_scalar(operation, bytes, random)
}

fn select_scalar(
    operation: &mut Operation,
    bytes: &[u8],
    random: Option<&mut random::RandomState>,
) -> Result<bool, Failure> {
    let selected = match operation.kind {
        Kind::Cut | Kind::Last => true,
        Kind::First => !operation.text.is_present(),
        Kind::Rand => {
            let draw = random
                .expect("rand operations require initialized state")
                .draw();
            operation.count == 1 || u64::from(draw) % operation.count == 0
        }
        _ => false,
    };
    if selected {
        operation.text.replace(bytes).map_err(scalar_text_failure)?;
    }
    Ok(selected)
}

fn sample_failure(kind: Kind, error: samples::Error) -> Failure {
    let reason = match error {
        #[cfg(test)]
        samples::Error::Capacity => "samples exceed 65536 values per operation per group",
        samples::Error::Allocation => "sample memory allocation failed",
        samples::Error::SortingAllocation => "sample sorting memory allocation failed",
    };
    unsupported(&format!("{} {reason}", grammar::name(kind)))
}

fn text_sample_failure(kind: Kind, error: text_samples::Error) -> Failure {
    let reason = match error {
        text_samples::Error::Allocation => "text memory allocation failed",
    };
    unsupported(&format!("{} {reason}", grammar::name(kind)))
}

fn scalar_text_failure(error: scalar_text::Error) -> Failure {
    match error {
        scalar_text::Error::Allocation => unsupported("text value allocation failed"),
    }
}

fn quartile(
    sorted: &[numerics::Value],
    which: samples::Quartile,
) -> Result<numerics::Value, Failure> {
    use fastmash_numeric_contract::Raw80;
    match which.select(sorted) {
        Some(samples::Selection::Single(value)) => Ok(value),
        Some(samples::Selection::Between {
            lower,
            upper,
            quarters,
        }) => {
            let raw = match quarters {
                0 => Raw80::new(0, 0),
                1 => Raw80::new(0x3ffd, 1 << 63),
                2 => Raw80::new(0x3ffe, 1 << 63),
                3 => Raw80::new(0x3ffe, 0xc000_0000_0000_0000),
                _ => unreachable!("quarter remainder is less than four"),
            };
            let fraction = numerics::canonical(raw);
            // Preserve GNU's arithmetic even at fraction zero (infinities/NaNs).
            let delta = decimal::subtract(upper, lower)?;
            let product = decimal::multiply(delta, fraction)?;
            decimal::add(lower, product)
        }
        None => Err(unsupported("internal quartile sample state missing")),
    }
}

fn middle_value(
    upper: numerics::Value,
    lower: Option<numerics::Value>,
) -> Result<numerics::Value, Failure> {
    if let Some(lower) = lower {
        let half = numerics::canonical(fastmash_numeric_contract::Raw80::new(0x3ffe, 1 << 63));
        decimal::multiply(decimal::add(upper, lower)?, half)
    } else {
        Ok(upper)
    }
}

fn quiet_nan() -> numerics::Value {
    numerics::canonical(fastmash_numeric_contract::Raw80::new(
        0x7fff,
        0xc000_0000_0000_0000,
    ))
}

fn format_count(count: u64, presentation: &presentation::Presentation) -> Result<Vec<u8>, Failure> {
    // Default14 renders these exact integers without rounding or an exponent.
    if presentation.spec.is_none() && count < 100_000_000_000_000 {
        let mut digits = [0u8; 14];
        let mut start = digits.len();
        let mut remaining = count;
        loop {
            start -= 1;
            digits[start] = b'0' + (remaining % 10) as u8;
            remaining /= 10;
            if remaining == 0 {
                break;
            }
        }
        let mut output = Vec::new();
        output
            .try_reserve_exact(digits.len() - start)
            .map_err(|_| conversion_failure(fastmash_conversion::profile::Error::Allocation))?;
        output.extend_from_slice(&digits[start..]);
        return Ok(output);
    }
    presentation.render(numerics::Numerics::value80(integer(count)))
}

fn moment_value(
    kind: Kind,
    values: &[numerics::Value],
    arithmetic: &mut numerics::Numerics,
    cache: &mut moments::Cache,
) -> Result<numerics::Value, Failure> {
    match kind {
        Kind::Jarque => jarque_bera(values, arithmetic, cache),
        Kind::Dpo => dagostino_pearson(values, arithmetic, cache),
        Kind::Pskew | Kind::Sskew => skewness(values, kind == Kind::Sskew, arithmetic, cache),
        Kind::Pkurt | Kind::Skurt => {
            excess_kurtosis(values, kind == Kind::Skurt, arithmetic, cache)
        }
        _ => unreachable!("moment finalization requires a moment or normality operation"),
    }
}

#[cfg(test)]
fn summarize_at(
    operations: &mut [Operation],
    at: usize,
    arithmetic: &mut numerics::Numerics,
    collapse: u8,
    presentation: &presentation::Presentation,
    ignore_case: bool,
) -> Result<Vec<u8>, Failure> {
    summarize_cached_at(
        operations,
        at,
        arithmetic,
        collapse,
        presentation,
        ignore_case,
    )
}

/// Retained text result (`N/A` or empty when none); `None` for non-text kinds.
fn text_result(operation: &Operation) -> Option<&[u8]> {
    let absent: &[u8] = match operation.kind {
        Kind::Cut | Kind::First | Kind::Last | Kind::Rand => b"N/A",
        Kind::Base64 | Kind::Debase64 | Kind::Checksum(_) | Kind::Path(_) => b"",
        _ => return None,
    };
    Some(operation.text.output().unwrap_or(absent))
}
fn summarize_cached_at(
    operations: &mut [Operation],
    at: usize,
    arithmetic: &mut numerics::Numerics,
    collapse: u8,
    presentation: &presentation::Presentation,
    ignore_case: bool,
) -> Result<Vec<u8>, Failure> {
    let operation = &operations[at];
    if let Selector::Pair { left, right } = operation.selector {
        let kind = match operation.kind {
            Kind::Pcov => paired::Kind::Covariance { sample: false },
            Kind::Scov => paired::Kind::Covariance { sample: true },
            Kind::Ppearson => paired::Kind::Pearson { sample: false },
            Kind::Spearson => paired::Kind::Pearson { sample: true },
            Kind::Dotprod => paired::Kind::DotProduct,
            _ => unreachable!("paired selector has a paired operation kind"),
        };
        let owner = &operations[operation.sample_source.unwrap_or(at)];
        let value = owner
            .pair_samples
            .as_ref()
            .expect("paired owner retains independently compacted samples")
            .summarize(
                kind,
                operation.kind,
                left,
                right,
                arithmetic,
                presentation.utf8,
            )?;
        return presentation.render(numerics::Numerics::value80(value));
    }
    if operation.kind.is_moment() || operation.kind.is_normality() {
        let kind = operation.kind;
        let owner = &mut operations[operation.sample_source.unwrap_or(at)];
        let value = moment_value(
            kind,
            owner.samples.as_slice(),
            arithmetic,
            &mut owner.moments,
        )?;
        return presentation.render(numerics::Numerics::value80(value));
    }
    if operation.kind.is_dispersion() {
        let kind = operation.kind;
        let source = operation.dispersion_source.unwrap_or(at);
        let value = operations[source]
            .dispersion
            .variance(matches!(kind, Kind::Svar | Kind::Sstdev))?;
        let value = if matches!(kind, Kind::Pstdev | Kind::Sstdev) {
            arithmetic.sqrt(value).map_err(numeric_failure)?
        } else {
            value
        };
        return presentation.render(numerics::Numerics::value80(value));
    }
    if !operation.kind.uses_shared_sorted_samples() && !operation.kind.is_mad() {
        return summarize(
            &mut operations[at],
            arithmetic,
            collapse,
            presentation,
            ignore_case,
        );
    }
    let kind = operation.kind;
    if operation.count == 0 {
        return presentation
            .render(fastmash_conversion::convert::quiet_nan(false).map_err(conversion_failure)?);
    }
    let source = operation.sample_source.unwrap_or(at);
    if kind.is_mad() {
        let owner = &mut operations[source];
        let deviation = match owner.raw_deviation {
            Some(value) => value,
            None => {
                let value = robust_statistics::deviation(&mut owner.samples, kind)?;
                owner.raw_deviation = Some(value);
                value
            }
        };
        let value = robust_statistics::scale(deviation, kind == Kind::Mad)?;
        return presentation.render(numerics::Numerics::value80(value));
    }
    let sorted = operations[source]
        .samples
        .sorted_once()
        .map_err(|error| sample_failure(kind, error))?;
    let value = match kind {
        Kind::Mode | Kind::Antimode => samples::mode(sorted, kind == Kind::Antimode)
            .ok_or_else(|| unsupported("internal mode sample state missing"))?,
        Kind::Median => {
            let upper = sorted[sorted.len() / 2];
            let lower = (sorted.len() % 2 == 0).then(|| sorted[sorted.len() / 2 - 1]);
            middle_value(upper, lower)?
        }
        Kind::Q1 => quartile(sorted, samples::Quartile::First)?,
        Kind::Q3 => quartile(sorted, samples::Quartile::Third)?,
        Kind::Iqr => {
            let upper = quartile(sorted, samples::Quartile::Third)?;
            let lower = quartile(sorted, samples::Quartile::First)?;
            decimal::subtract(upper, lower)?
        }
        Kind::Percentile(percent) => ordered_statistics::percentile(sorted, percent)?,
        Kind::Trimmean(trim) if trim.is_half() => {
            let upper = sorted[sorted.len() / 2];
            let lower = (sorted.len() % 2 == 0).then(|| sorted[sorted.len() / 2 - 1]);
            middle_value(upper, lower)?
        }
        Kind::Trimmean(trim) => ordered_statistics::trimmed(sorted, trim)?,
        _ => unreachable!("ordered-sample finalizer has a supported kind"),
    };
    presentation.render(numerics::Numerics::value80(value))
}

fn summarize(
    operation: &mut Operation,
    arithmetic: &mut numerics::Numerics,
    collapse: u8,
    presentation: &presentation::Presentation,
    ignore_case: bool,
) -> Result<Vec<u8>, Failure> {
    if operation.kind == Kind::Unique {
        return operation
            .text_samples
            .unique(collapse, ignore_case)
            .map_err(|error| text_sample_failure(operation.kind, error));
    }
    if operation.kind == Kind::Collapse {
        return operation
            .text_samples
            .collapse(collapse)
            .map_err(|error| text_sample_failure(operation.kind, error));
    }
    if let Some(bytes) = text_result(operation) {
        let mut result = Vec::new();
        result
            .try_reserve_exact(bytes.len())
            .map_err(|_| scalar_text_failure(scalar_text::Error::Allocation))?;
        result.extend_from_slice(bytes);
        return Ok(result);
    }
    if operation.count == 0 {
        return presentation.render(match operation.kind {
            Kind::Sum | Kind::Count | Kind::Countunique => {
                fastmash_numeric_contract::Value80::signed_zero(false)
            }
            Kind::Min | Kind::Absmin => {
                fastmash_conversion::convert::infinity(true).map_err(conversion_failure)?
            }
            Kind::Max | Kind::Absmax => {
                fastmash_conversion::convert::infinity(false).map_err(conversion_failure)?
            }
            _ => fastmash_conversion::convert::quiet_nan(false).map_err(conversion_failure)?,
        });
    }
    if operation.kind == Kind::Count {
        return format_count(operation.count, presentation);
    }
    if operation.kind == Kind::Countunique {
        return format_count(
            operation
                .text_samples
                .count_unique(ignore_case)
                .map_err(|error| text_sample_failure(operation.kind, error))? as u64,
            presentation,
        );
    }
    let value = if operation.kind.is_moment() || operation.kind.is_normality() {
        moment_value(
            operation.kind,
            operation.samples.as_slice(),
            arithmetic,
            &mut moments::Cache::default(),
        )?
    } else if matches!(
        operation.kind,
        Kind::Pvar | Kind::Svar | Kind::Pstdev | Kind::Sstdev
    ) {
        operation
            .dispersion
            .variance(matches!(operation.kind, Kind::Svar | Kind::Sstdev))?
    } else if operation.kind == Kind::Range {
        decimal::subtract(operation.maximum, operation.value)?
    } else if matches!(
        operation.kind,
        Kind::Mean | Kind::Geomean | Kind::Ms | Kind::Rms
    ) {
        decimal::mean(operation.value, operation.count)?
    } else if operation.kind == Kind::Harmmean {
        decimal::divide(integer(operation.count), operation.value)?
    } else {
        operation.value
    };
    let value = if operation.kind == Kind::Geomean {
        arithmetic.mean_exp(value).map_err(numeric_failure)?
    } else if matches!(operation.kind, Kind::Rms | Kind::Pstdev | Kind::Sstdev) {
        arithmetic.sqrt(value).map_err(numeric_failure)?
    } else {
        value
    };
    presentation.render(numerics::Numerics::value80(value))
}

/// Whether `current` starts a new group after `previous`. `previous_keys`
/// caches each key's position in `previous`; it is located at the same point
/// as before caching, so diagnostics and their order are unchanged.
fn different(
    current: &[u8],
    previous: &[u8],
    previous_keys: &mut [Option<field_policy::FieldRange>],
    keys: &[u64],
    line: u64,
    input: records::Separator,
    ignore_case: bool,
) -> Result<bool, Failure> {
    for (&key, cached) in keys.iter().zip(previous_keys.iter_mut()) {
        let a = selected_field(current, key, line, input)?;
        let b = match *cached {
            Some(span) => span,
            None => *cached.insert(selected_field(previous, key, line, input)?),
        };
        if !text_order::same_group(
            &current[a.start..a.start + a.length],
            &previous[b.start..b.start + b.length],
            ignore_case,
        ) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn finish_group(
    output: &mut impl command_output::CommandOutput,
    record: &[u8],
    keys: &[u64],
    operations: &mut [Operation],
    line: u64,
    options: &options::Options,
    arithmetic: &mut numerics::Numerics,
) -> Result<(), Failure> {
    if options.crosstab {
        let row = selected_field(record, keys[0], line, options.input)?;
        let column = selected_field(record, keys[1], line, options.input)?;
        let value = summarize_cached_at(
            operations,
            0,
            arithmetic,
            options.collapse,
            &options.presentation,
            options.ignore_case,
        )?;
        return output.cell(
            &record[row.start..row.start + row.length],
            &record[column.start..column.start + column.length],
            &value,
        );
    }
    if options.full {
        command_output::full_prefix(output, record, options);
    }
    for &key in keys.iter().filter(|_| !options.full) {
        let span = selected_field(record, key, line, options.input)?;
        output.key(
            &record[span.start..span.start + span.length],
            options.output,
        );
    }
    write_results(output, operations, arithmetic, options)
}

/// Writes one group's results in request order. Text results go straight to the
/// output, without copying each result.
fn write_results(
    output: &mut impl command_output::CommandOutput,
    operations: &mut [Operation],
    arithmetic: &mut numerics::Numerics,
    options: &options::Options,
) -> Result<(), Failure> {
    let operation_count = operations.len();
    for at in 0..operation_count {
        let separator = if at + 1 == operation_count {
            options.record_end
        } else {
            options.output
        };
        if let Some(bytes) = text_result(&operations[at]) {
            output.result(bytes, separator);
            continue;
        }
        let value = summarize_cached_at(
            operations,
            at,
            arithmetic,
            options.collapse,
            &options.presentation,
            options.ignore_case,
        )?;
        output.result(&value, separator);
    }
    Ok(())
}

/// Process entry, without Rust's runtime start. That start installs a
/// stack-overflow handler whose guard lookup reads /proc/self/maps on every
/// run, a large share of a tiny command's time. Closed standard streams are
/// already held open by `standard_io`'s preinitializer, and arguments come from
/// std's own glibc initializer, so only SIGPIPE needs the runtime's setting:
/// ignored, so failed writes are reported as errors.
#[cfg(not(test))]
#[unsafe(no_mangle)]
extern "C" fn main(_argc: libc::c_int, _argv: *const *const libc::c_char) -> libc::c_int {
    // SAFETY: the process is still single-threaded; this is the disposition
    // Rust's runtime start sets.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    run_process();
    0
}

#[cfg_attr(test, allow(dead_code))]
fn run_process() {
    let args = match options::collect_args(std::env::args_os()) {
        Ok(args) => args,
        Err(error) => {
            let mut stderr = standard_io::Stderr;
            let _ = stderr.write_all(b"fastmash: ");
            let _ = stderr.write_all(&error.message);
            std::process::exit(error.status);
        }
    };
    let name = args
        .first()
        .map_or(b"fastmash".as_slice(), |v| v.as_bytes());
    let mut stdin = io::BufReader::new(standard_io::Stdin);
    let mut stdout = command_output::Stdout::new();
    let mut report = |error: &Failure| {
        let mut stderr = standard_io::Stderr;
        let wrote = stderr
            .write_all(name)
            .and_then(|_| stderr.write_all(b": "))
            .and_then(|_| stderr.write_all(&error.message))
            .and_then(|_| stderr.flush());
        wrote.is_ok()
    };
    // argc can be 0 (execve with an empty argv before Linux 5.18).
    let operands = args.get(1..).unwrap_or_default();
    let result = run(&mut stdin, &mut stdout, operands, name, &mut report);
    let mut status = match result {
        Ok(status) => status,
        Err(error) => {
            if report(&error) {
                error.status
            } else {
                1
            }
        }
    };
    if let Err(error) = stdout.close()
        && error.raw_os_error() != Some(9)
    {
        let _ = report(&os_failure(&error, false));
        status = 1;
    }
    if !standard_io::finish_stderr() {
        status = 1;
    }
    if status != 0 {
        std::process::exit(status);
    }
}

#[cfg(test)]
mod field_value_tests {
    use super::*;

    #[test]
    fn group_changes_reuse_cached_previous_key_positions() {
        let tab = records::Separator::Literal(b'\t');
        let mut cache = [None, None];
        let previous = b"a\tx\t1".as_slice();
        assert!(
            !different(b"a\tx\t2", previous, &mut cache, &[1, 2], 2, tab, false)
                .ok()
                .unwrap()
        );
        assert!(cache.iter().all(Option::is_some));
        // A differing first key returns before the second key is compared.
        let mut fresh = [None, None];
        assert!(
            different(b"b\tx", previous, &mut fresh, &[1, 2], 3, tab, false)
                .ok()
                .unwrap()
        );
        assert!(fresh[0].is_some() && fresh[1].is_none());
        // A previous record without the key fails at first use, as before.
        let mut short = [None];
        let error = different(b"a\tx", b"a", &mut short, &[2], 4, tab, false)
            .err()
            .unwrap();
        let uncached = selected_field(b"a", 2, 4, tab).err().unwrap();
        assert_eq!(error.message, uncached.message);
        assert!(short[0].is_none());
    }

    #[test]
    fn grouped_calculation_allocations_refuse_or_complete_exactly() {
        let args: Vec<std::ffi::OsString> = ["-g", "1", "sum", "2"]
            .into_iter()
            .map(Into::into)
            .collect();
        let (mut refusals, mut completed) = (0, false);
        for at in 0..200 {
            let mut input: &[u8] = b"a\t1\na\t2\nb\t4\n";
            let mut output = Vec::new();
            let mut reported = Vec::new();
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let result = run(&mut input, &mut output, &args, b"fastmash", &mut |error| {
                reported.push((error.status, error.message.clone()));
                true
            });
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            match result {
                Ok(0) => {
                    assert_eq!(output, b"a\t3\nb\t4\n");
                    completed = true;
                    break;
                }
                Ok(status) => {
                    assert_eq!(status, 77);
                    assert_eq!(
                        reported,
                        [(77, b"command memory allocation failed\n".to_vec())]
                    );
                    refusals += 1;
                }
                Err(error) => {
                    assert_eq!(error.status, 77);
                    assert_eq!(error.message, b"command memory allocation failed\n");
                    refusals += 1;
                }
            }
        }
        assert!(completed && refusals > 0, "refusals={refusals}");
    }

    #[test]
    fn command_allocations_fail_before_publishing_partial_request_state() {
        let args: Vec<std::ffi::OsString> = ["-H", "-g", "a", "count", "a,a,a"]
            .into_iter()
            .map(Into::into)
            .collect();
        let mut failures = 0;
        let mut completed = false;
        for at in 0..100 {
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(Some(at)));
            let result = (|| -> Result<(), Failure> {
                let options::Action::Calculate(options) =
                    options::parse(&args, b"fastmash", false)?
                else {
                    unreachable!()
                };
                let command = grammar::command(&options.operands, options.group.as_ref())?;
                let (_, names) = operations(command.operations)?;
                named_fields::resolve_with(&names, b"a", options.input, options.locale.utf8)?;
                Ok(())
            })();
            command_memory::FAIL_RESERVATION.with(|slot| slot.set(None));
            match result {
                Err(error) => {
                    assert_eq!(error.status, 77);
                    assert_eq!(error.message, b"command memory allocation failed\n");
                    failures += 1;
                }
                Ok(()) => {
                    completed = true;
                    break;
                }
            }
        }
        assert!(
            completed && failures > 10,
            "failures={failures}, completed={completed}"
        );
    }

    #[test]
    fn selected_spans_distinguish_empty_fields_from_absent_columns() {
        let delimiter = records::Separator::Literal(b'\t');
        let span = selected_field(b"a\t", 2, 3, delimiter).ok().unwrap();
        assert_eq!((span.start, span.length), (2, 0));
        let error = selected_field(b"", 1, 3, delimiter).err().unwrap();
        assert_eq!(
            error.message,
            b"invalid input: field 1 requested, line 3 has only 0 fields\n"
        );
        assert!(!field_policy::is_na(b""));
        assert!(!field_policy::is_na(b"NA\0"));
        assert!(field_policy::is_na(b"n/A"));
    }

    #[test]
    fn missing_values_require_exact_whole_field_matches() {
        for word in [b"NA".as_slice(), b"na", b"N/A", b"n/a", b"NaN", b"nan"] {
            assert!(matches!(
                field_value(
                    word,
                    1,
                    1,
                    records::Separator::Literal(b'\t'),
                    true,
                    Profile::C
                ),
                Ok(None)
            ));
        }
        for word in [
            b" NA".as_slice(),
            b"NA ",
            b"NA\0",
            b"NAN\0tail",
            b"N/Ax",
            b"NANx",
        ] {
            assert_eq!(
                field_value(
                    word,
                    1,
                    1,
                    records::Separator::Literal(b'\t'),
                    true,
                    Profile::C
                )
                .err()
                .unwrap()
                .status,
                1
            );
        }
    }

    #[test]
    fn missing_values_preserve_field_errors_and_default_policy() {
        for (word, field, narm) in [
            (b"".as_slice(), 1, true),
            (b"NA".as_slice(), 2, true),
            (b"NA".as_slice(), 1, false),
        ] {
            assert_eq!(
                field_value(
                    word,
                    field,
                    1,
                    records::Separator::Literal(b'\t'),
                    narm,
                    Profile::C
                )
                .err()
                .unwrap()
                .status,
                1
            );
        }
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn count_fast_format_matches_general_default14() {
        let mut counts = vec![0, 12_345_678_901_234, 99_999_999_999_999, u64::MAX];
        for power in 0..=19 {
            let n = 10u64.pow(power);
            counts.extend([n - 1, n, n + 1]);
        }
        for count in counts {
            let expected = format::format(
                numerics::Numerics::value80(integer(count)),
                Profile::C,
                FormatId::Default14,
                16384,
            )
            .unwrap()
            .bytes;
            assert_eq!(
                format_count(count, &Default::default()).ok().unwrap(),
                expected,
                "count {count}"
            );
        }
    }

    #[test]
    fn numeric_failures_preserve_exact_status_and_diagnostic_bytes() {
        use fastmash_portable_numerics::NumericFailure;
        for (cause, status, diagnostic) in [
            (
                NumericFailure::UnsupportedDomain,
                77,
                b"fastmash: numerical operation result is outside the supported domain\n"
                    .as_slice(),
            ),
            (
                NumericFailure::Capacity,
                77,
                b"fastmash: numerical arithmetic capacity exceeded\n",
            ),
            (
                NumericFailure::Allocation,
                77,
                b"fastmash: numerical arithmetic memory allocation failed\n",
            ),
            (
                NumericFailure::Invariant,
                70,
                b"fastmash: internal numerical invariant failure\n",
            ),
            (
                NumericFailure::TableIdentity,
                70,
                b"fastmash: numerical table identity check failed\n",
            ),
        ] {
            let error = numeric_failure(cause);
            assert_eq!(error.status, status, "{cause:?}");
            let prefix = b"fastmash: ";
            assert_eq!(error.message, &diagnostic[prefix.len()..], "{cause:?}");
            assert_eq!([prefix.as_slice(), &error.message].concat(), diagnostic);
        }
    }

    struct PanicSource;
    impl random::SeedSource for PanicSource {
        fn seed(&mut self) -> Result<u32, ()> {
            panic!("entropy source must not be used")
        }
    }

    #[derive(Default)]
    struct FailingSource {
        calls: usize,
    }
    impl random::SeedSource for FailingSource {
        fn seed(&mut self) -> Result<u32, ()> {
            self.calls += 1;
            Err(())
        }
    }

    #[test]
    fn conversion_failures_distinguish_capacity_allocation_and_internal_causes() {
        use fastmash_conversion::profile::Error;
        for (cause, message) in [
            (
                Error::InputCapacity,
                "numeric conversion input exceeds 8192-byte limit\n",
            ),
            (
                Error::OutputCapacity,
                "numeric output exceeds 16384-byte limit\n",
            ),
            (
                Error::CoefficientCapacity,
                "numeric conversion coefficient capacity exceeded\n",
            ),
            (
                Error::IntermediateCapacity,
                "numeric conversion intermediate capacity exceeded\n",
            ),
            (
                Error::PowerCapacity,
                "numeric conversion power capacity exceeded\n",
            ),
            (
                Error::ShiftCapacity,
                "numeric conversion shift capacity exceeded\n",
            ),
            (
                Error::UnsupportedProfile,
                "unsupported numerical conversion profile\n",
            ),
            (
                Error::UnsupportedFormat,
                "unsupported numerical output format\n",
            ),
            (
                Error::Allocation,
                "numeric conversion memory allocation failed\n",
            ),
            (Error::InvalidView, "internal numeric conversion failure\n"),
            (
                Error::InternalInvariant,
                "internal numeric conversion failure\n",
            ),
        ] {
            let error = conversion_failure(cause);
            assert_eq!(error.status, 77);
            assert_eq!(error.message, message.as_bytes());
        }
    }

    #[test]
    fn invocation_random_state_obeys_entropy_use_and_failure_contracts() {
        let count = operations(grammar::parse(&args(&["count", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0;
        assert!(
            initialize_random(&count, Some(7), &mut PanicSource)
                .ok()
                .unwrap()
                .is_none()
        );

        let rand = operations(grammar::parse(&args(&["rand", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0;
        assert!(
            initialize_random(&rand, Some(7), &mut PanicSource)
                .ok()
                .unwrap()
                .is_some()
        );
        let mut source = FailingSource::default();
        let error = initialize_random(&rand, None, &mut source).err().unwrap();
        assert_eq!(source.calls, 1);
        assert_eq!(error.status, 77);
        assert_eq!(error.message, b"unable to initialize random state\n");
    }

    #[test]
    fn lifecycle_orders_runtime_entropy_and_sort_admission() {
        let rand = operations(grammar::parse(&args(&["rand", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0;
        let runtime = unsupported("runtime first");
        let error = initialize_before_input(
            true,
            &rand,
            None,
            &mut PanicSource,
            || Ok(()),
            || Err::<(), _>(runtime),
            || panic!("sort admission must not precede runtime admission"),
        )
        .err()
        .unwrap();
        assert_eq!(error.message, b"runtime first\n");

        let mut source = FailingSource::default();
        let mut sort_called = false;
        let error = initialize_before_input(
            true,
            &rand,
            None,
            &mut source,
            || Ok(()),
            || Ok(()),
            || {
                sort_called = true;
                Ok(())
            },
        )
        .err()
        .unwrap();
        assert_eq!(error.message, b"unable to initialize random state\n");
        assert_eq!(source.calls, 1);
        assert!(!sort_called);
    }

    #[test]
    fn scalar_replacement_failure_is_transactional_and_status_77() {
        let mut operations = operations(grammar::parse(&args(&["rand", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0;
        operations[0].text.replace(b"old").unwrap();
        operations[0].text.fail_next_growth();
        operations[0].count = 1;
        let mut random = initialize_random(&operations, Some(0), &mut PanicSource)
            .ok()
            .unwrap()
            .unwrap();
        let error = select_scalar(&mut operations[0], b"replacement", Some(&mut random))
            .err()
            .unwrap();
        assert_eq!(error.status, 77);
        assert_eq!(error.message, b"text value allocation failed\n");
        assert_eq!(operations[0].text.output(), Some(b"old".as_slice()));
    }

    #[test]
    fn random_selection_count_boundaries_preserve_gnu_modulo_rule() {
        // A fixed independently observed first draw avoids billions of rows.
        // Counts beyond the 31-bit draw range retain the old value for any
        // nonzero draw; this intentionally does not claim unbiased sampling.
        for (count, selected) in [
            (1, true),
            (1_804_289_383, true),
            ((1u64 << 31) - 1, false),
            (1u64 << 31, false),
            ((1u64 << 31) + 1, false),
            (u64::MAX, false),
        ] {
            let mut ops = operations(grammar::parse(&args(&["rand", "1"])).ok().unwrap())
                .ok()
                .unwrap()
                .0;
            ops[0].text.replace(b"old").unwrap();
            ops[0].count = count;
            let mut random = random::RandomState::initialize(Some(0), &mut PanicSource).unwrap();
            select_scalar(&mut ops[0], b"new", Some(&mut random))
                .ok()
                .unwrap();
            assert_eq!(
                ops[0].text.output(),
                Some(if selected {
                    b"new".as_slice()
                } else {
                    b"old".as_slice()
                })
            );
        }
    }

    fn args(items: &[&str]) -> Vec<std::ffi::OsString> {
        items.iter().map(std::ffi::OsString::from).collect()
    }
    #[test]
    fn request_grammar_preserves_order_and_duplicates() {
        let parse = |items: &[&str]| {
            grammar::command(&args(items), None).and_then(|command| operations(command.operations))
        };
        let parsed = parse(&["mean", "2", "sum", "1", "mean", "2"]).ok().unwrap();
        let actual: Vec<_> = parsed
            .0
            .iter()
            .map(|op| (op.kind, op.selector, op.count))
            .collect();
        assert_eq!(
            actual,
            [
                (Kind::Mean, Selector::Single(2), 0),
                (Kind::Sum, Selector::Single(1), 0),
                (Kind::Mean, Selector::Single(2), 0)
            ]
        );
        for input in [
            vec![],
            vec!["sum"],
            vec!["sum", "0"],
            vec!["sum", "18446744073709551616"],
            vec!["sum", "+1"],
            vec!["sum", "1", "unknown", "1"],
        ] {
            assert!(parse(&input).is_err());
        }
        assert!(parse(&["sum", "18446744073709551615"]).is_err());
        assert!(parse(&["sum", "01"].repeat(16)).is_ok());
        assert!(parse(&["sum", "1"].repeat(1200)).is_ok());
    }
    #[test]
    fn selected_field_diagnostics_keep_length_and_raw_bytes() {
        for (record, field, expected) in [
            (
                b"1\tx\t9".as_slice(),
                2,
                b"invalid numeric value in line 7 field 2: 'x'\n".as_slice(),
            ),
            (
                b"1\t\xff\0tail",
                2,
                b"invalid numeric value in line 7 field 2: '\xff'\n",
            ),
            (
                b"x",
                2,
                b"invalid input: field 2 requested, line 7 has only 1 fields\n",
            ),
        ] {
            let error = field_value(
                record,
                field,
                7,
                records::Separator::Literal(b'\t'),
                false,
                Profile::C,
            )
            .err()
            .unwrap();
            assert_eq!(error.status, 1);
            assert_eq!(error.message, expected);
        }
    }
    #[test]
    fn multi_column_short_zero_and_flush_writes() {
        struct Writer {
            data: Vec<u8>,
            zero: bool,
            fail_flush: bool,
        }
        impl Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.zero {
                    return Ok(0);
                }
                let n = bytes.len().min(2);
                self.data.extend_from_slice(&bytes[..n]);
                Ok(n)
            }
            fn flush(&mut self) -> io::Result<()> {
                if self.fail_flush {
                    Err(io::Error::from_raw_os_error(28))
                } else {
                    Ok(())
                }
            }
        }
        let row = b"12\t3\t4.5\n";
        let mut writer = Writer {
            data: vec![],
            zero: false,
            fail_flush: false,
        };
        records::write_output(&mut writer, row).unwrap();
        assert_eq!(writer.data, row);
        writer.zero = true;
        assert_eq!(
            records::write_output(&mut writer, row).unwrap_err().kind(),
            io::ErrorKind::WriteZero
        );
        writer.zero = false;
        writer.fail_flush = true;
        let error = records::write_output(&mut writer, row).unwrap_err();
        assert_eq!(
            os_failure(&error, false).message,
            b"write error: No space left on device\n"
        );
    }
}

mod headers;

#[cfg(test)]
mod portable_routing_tests {
    use super::*;
    #[test]
    fn sorted_startup_retains_the_single_constructed_value() {
        let operations = operations(grammar::parse(&["sum".into(), "1".into()]).ok().unwrap())
            .ok()
            .unwrap()
            .0;
        struct NoEntropy;
        impl random::SeedSource for NoEntropy {
            fn seed(&mut self) -> Result<u32, ()> {
                panic!("no random request")
            }
        }
        let (session, _) = initialize_before_input(
            true,
            &operations,
            None,
            &mut NoEntropy,
            || Ok(()),
            || Ok(42u32),
            || Ok(()),
        )
        .ok()
        .unwrap();
        assert_eq!(session, 42);
    }
}
