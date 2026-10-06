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
mod annotated;
mod base64_fields;
mod binding;
mod boundary_exp;
mod buffered_stdout;
mod checksum;
mod collation_glibc;
mod collation_locales;
#[cfg(test)]
mod color_tests;
mod command_memory;
mod command_output;
mod command_sources;
#[cfg(test)]
mod command_test_support;
mod comparison;
#[cfg(test)]
mod comparison_tests;
mod crosstab;
mod csv_input;
#[cfg(test)]
mod csv_input_tests;
mod decimal;
mod decimal_powers;
mod dispersion;
mod exp_table;
mod failure;
mod grammar;
mod grouping;
mod guarded_exp;
mod guarded_log;
mod health;
mod health_render;
#[cfg(test)]
mod health_tests;
mod help;
mod intake;
mod line_numeric;
mod linux;
mod locale;
mod log_cache;
mod log_table;
mod malloc_arenas;
mod mean_math;
mod moments;
mod named_fields;
mod normality;
mod numeric_locales;
mod numerics;
mod operation;
mod operation_set;
mod options;
mod ordered_statistics;
mod paired;
mod path_fields;
mod preparation;
#[cfg(test)]
mod preparation_tests;
mod presentation;
mod projected_sort;
mod random;
mod records;
mod replay;
mod robust_statistics;
#[cfg(test)]
mod routing_tests;
mod samples;
mod scalar_text;
mod selection;
#[cfg(test)]
mod selection_tests;
mod sort_route;
mod sorted_input;
mod standard_io;
mod table_checks;
mod table_modes;
mod terminal_style;
#[cfg(test)]
#[path = "../tests/support/temp_dir.rs"]
mod test_dir;
#[cfg(test)]
#[path = "../tests/support/terminal.rs"]
mod test_terminal;
mod text_order;
mod text_samples;
mod transpose;
mod weighted;
#[cfg(test)]
mod weighted_tests;

#[cfg(test)]
use failure::MESSAGE_FAILS;
use failure::{
    Failure, append, conversion_failure, failure, numeric_failure, os_failure, reported,
    unsupported, unsupported_hint,
};
use fastmash_conversion::{field_policy, profile::Profile};
use operation::Kind;
use operation_set::OperationSet;
use std::{
    io::{self, BufRead, Write},
    os::unix::ffi::OsStrExt,
};

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

/// The diagnostic for field `field` of line `line`, which has `fields` fields.
#[cold]
fn missing_field(field: u64, line: u64, fields: u64) -> Failure {
    failure(
        format!("invalid input: field {field} requested, line {line} has only {fields} fields\n")
            .into_bytes(),
    )
}

#[inline]
fn selected_field(
    record: &[u8],
    field: u64,
    line: u64,
    delimiter: records::Separator,
) -> Result<field_policy::FieldRange, Failure> {
    records::field(record, field, delimiter).map_err(|fields| missing_field(field, line, fields))
}

/// [`selected_field`] through the record's field index.
#[inline]
fn indexed_field(
    record: &[u8],
    index: &mut records::FieldIndex,
    field: u64,
    line: u64,
    delimiter: records::Separator,
) -> Result<field_policy::FieldRange, Failure> {
    index
        .field(record, field, delimiter)
        .map_err(|fields| missing_field(field, line, fields))
}

#[inline]
fn field_value(
    record: &[u8],
    index: &mut records::FieldIndex,
    field: u64,
    line: u64,
    delimiter: records::Separator,
    narm: bool,
    profile: Profile,
) -> Result<Option<numerics::Value>, Failure> {
    let span = indexed_field(record, index, field, line, delimiter)?;
    decimal::field(record, span, field, line, narm, profile)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Selector {
    Single(u64),
    Pair { left: u64, right: u64 },
}

fn integer(n: u64) -> numerics::Value {
    numerics::Numerics::promote_u64(n)
}

fn initialize_random(
    needs_random: bool,
    explicit_seed: Option<u32>,
    source: &mut impl random::SeedSource,
) -> Result<Option<random::RandomState>, Failure> {
    needs_random
        .then(|| random::RandomState::initialize(explicit_seed, source))
        .transpose()
        .map_err(|()| unsupported("unable to initialize random state"))
}

fn initialize_before_input<T>(
    sorted: bool,
    needs_random: bool,
    explicit_seed: Option<u32>,
    source: &mut impl random::SeedSource,
    command_setup: impl FnOnce() -> Result<(), Failure>,
    startup: impl FnOnce() -> Result<T, Failure>,
    sort_admit: impl FnOnce() -> Result<(), Failure>,
) -> Result<(T, Option<random::RandomState>), Failure> {
    command_setup()?;
    let arithmetic = startup()?;
    let random = initialize_random(needs_random, explicit_seed, source)?;
    if sorted {
        sort_admit()?;
    }
    Ok((arithmetic, random))
}

/// What a Command reads from the process environment, read once at its
/// start: its locale and the settings that choose how it runs. The sort
/// trace (`FASTMASH_SORT_TRACE`), a diagnostic, is read where it reports.
struct Environment {
    locale: locale::Policy,
    posixly_correct: bool,
    /// `FASTMASH_GROUPING`, which only sorted Commands with Grouping keys
    /// read (and refuse when it is invalid).
    grouping: Option<std::ffi::OsString>,
    /// `FASTMASH_SORT_MEMORY_BYTES`, which only native sorts read.
    sort_memory: Option<std::ffi::OsString>,
    /// `FASTMASH_PIPE_GROUPING`, which only sorted Commands with Grouping
    /// keys read (and refuse when it is invalid).
    pipe_grouping: Option<std::ffi::OsString>,
    terminal: terminal_style::Detection,
}

impl Environment {
    fn from_env(terminal: terminal_style::Detection) -> Self {
        Self {
            locale: locale::Policy::from_env(),
            posixly_correct: std::env::var_os("POSIXLY_CORRECT").is_some(),
            grouping: std::env::var_os("FASTMASH_GROUPING"),
            sort_memory: std::env::var_os("FASTMASH_SORT_MEMORY_BYTES"),
            pipe_grouping: std::env::var_os("FASTMASH_PIPE_GROUPING"),
            terminal,
        }
    }
}

#[cfg(test)]
fn run(
    reader: &mut impl replay::Rewind,
    writer: &mut impl command_output::Transport,
    args: &[std::ffi::OsString],
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    run_in(
        reader,
        writer,
        args,
        name,
        report,
        Environment::from_env(terminal_style::Detection::from_env()),
    )
}

/// [`run`] in `environment`.
#[cfg(test)]
fn run_in(
    reader: &mut impl replay::Rewind,
    writer: &mut impl command_output::Transport,
    args: &[std::ffi::OsString],
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
    environment: Environment,
) -> Result<i32, Failure> {
    run_with_seed_source(
        reader,
        writer,
        args,
        name,
        report,
        &mut random::OsSeedSource,
        environment,
    )
}

#[cfg(test)]
fn run_with_seed_source(
    reader: &mut impl replay::Rewind,
    writer: &mut impl command_output::Transport,
    args: &[std::ffi::OsString],
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
    seed_source: &mut impl random::SeedSource,
    environment: Environment,
) -> Result<i32, Failure> {
    run_with_sources(
        (reader, &mut command_sources::Files),
        writer,
        args,
        name,
        report,
        seed_source,
        environment,
    )
}

#[cfg(test)]
fn run_with_sources(
    input: (
        &mut impl replay::Rewind,
        &mut impl command_sources::Resolver,
    ),
    writer: &mut impl command_output::Transport,
    args: &[std::ffi::OsString],
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
    seed_source: &mut impl random::SeedSource,
    environment: Environment,
) -> Result<i32, Failure> {
    let scan =
        options::scan_with_policy(args, name, environment.posixly_correct, environment.locale);
    run_scanned_with_sources(
        input,
        writer,
        (scan, environment),
        name,
        report,
        seed_source,
    )
}

/// Dispatches the same scanned Command for process and controlled transports.
/// The process resolves diagnostic styling before consuming a failed action.
fn run_scanned_with_sources(
    input: (
        &mut impl replay::Rewind,
        &mut impl command_sources::Resolver,
    ),
    writer: &mut impl command_output::Transport,
    scanned: (options::Scan, Environment),
    name: &[u8],
    report: &mut impl FnMut(&Failure) -> bool,
    seed_source: &mut impl random::SeedSource,
) -> Result<i32, Failure> {
    let (reader, sources) = input;
    let (scan, mut environment) = scanned;
    let policy = environment.locale;
    let style = environment
        .terminal
        .style(scan.color, terminal_style::Destination::Stdout);
    let action = scan.action;
    match &action {
        Ok(options::Action::Help) => {
            return help::write(writer, style)
                .map(|()| 0)
                .map_err(|e| os_failure(&e, false));
        }
        Ok(options::Action::Version) => {
            return records::write_output(
                writer,
                concat!("fastmash ", env!("CARGO_PKG_VERSION"), "\n").as_bytes(),
            )
            .map(|()| 0)
            .map_err(|e| os_failure(&e, false));
        }
        _ => {}
    }
    let profile = policy.numeric;
    let options::Action::Calculate(mut options) = action? else {
        unreachable!("informational action already handled")
    };
    options.presentation.profile = profile;
    options.sort_memory = environment.sort_memory.take();
    let command = grammar::command_for_format(
        &options.operands,
        options.group.as_ref(),
        options.vnlog,
        profile,
        policy.utf8,
    )?;
    let (options, binding, needs, route) =
        match preparation::prepare(&mut options, command, &environment, name)? {
            preparation::Prepared::Comparison(prepared) => {
                setup_sigpipe()?;
                return comparison::run(reader, writer, sources, prepared, name, report);
            }
            preparation::Prepared::Health(prepared) => {
                setup_sigpipe()?;
                return health::run(reader, writer, prepared.into_options(), style, report);
            }
            preparation::Prepared::Table(prepared) => {
                let (options, mode, keys) = prepared.into_parts();
                setup_sigpipe()?;
                return table_modes::run(reader, writer, options, mode, keys, report);
            }
            preparation::Prepared::Selection(prepared) => {
                let (options, request, binding) = prepared.into_parts();
                setup_sigpipe()?;
                return selection::run(reader, writer, options, request, binding, report);
            }
            preparation::Prepared::Ordinary(prepared) => prepared.into_parts(),
        };
    let mut cleared = None;
    let (mut arithmetic, mut random) = initialize_before_input(
        route.is_some_and(|route| route.sort == sort_route::Sort::External),
        needs.random,
        options.seed,
        seed_source,
        setup_sigpipe,
        || numerics::Numerics::with(needs.numerics).map_err(numeric_failure),
        || {
            cleared = Some(sorted_input::admit(&policy)?);
            Ok(())
        },
    )?;
    if let Some(route) = route {
        return sort_route::run(
            route,
            reader,
            writer,
            options,
            binding,
            &mut arithmetic,
            random.as_mut(),
            cleared,
            report,
        );
    }
    let (capacity, line_buffered) = writer.buffering();
    let buffer = buffered_stdout::BufferedStdout::new(writer, capacity, line_buffered)
        .map_err(|e| os_failure(&e, false))?;
    let mut output = command_output::Results::new(buffer, options);
    let result = calculate(
        reader,
        &mut output,
        options,
        binding,
        &mut arithmetic,
        random.as_mut(),
        None,
    );
    Ok(command_output::complete(
        output.buffer,
        result,
        command_output::Transport::close,
        report,
    ))
}

fn calculate(
    reader: &mut (impl BufRead + ?Sized),
    output: &mut impl command_output::CommandOutput,
    options: &options::Options,
    mut binding: binding::Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    // The Input header, where the external route read it before sorting.
    prepared: Option<Vec<u8>>,
) -> Result<(), Failure> {
    if options.csv_in {
        return csv_input::calculate(reader, output, options, binding, arithmetic, random);
    }
    let mut random = random;
    // When the Input header was not found before sorting, GNU reads it again
    // from the sorted stream, and warns about --full only then (datamash.c
    // process_file), after the sort's own diagnostics.
    let (header, lines) = match &prepared {
        Some(_) => (intake::Header::None, 1),
        None => (intake::Header::of(options), 0),
    };
    let mut intake = intake::Intake::new(options, header)
        .after(lines)
        .warn_full(options, binding.program);
    if let Some(record) = prepared {
        output.first(&record, &binding.operations, &binding.keys)?;
    }
    let mut calculation = grouping::Calculation::new(&binding.keys, options);
    let mut spare = grouping::Raw::for_keys(binding.keys.len(), records::FieldIndex::default())?;
    // The records' field index, set up once the fields are resolved.
    let mut index = None;
    if intake.header(reader, spare.bytes_mut(), |record| {
        binding.header(record, options)
    })? {
        output.first(spare.bytes(), &binding.operations, &binding.keys)?;
    }
    while let Some(record) = intake.next(reader, spare.bytes_mut())? {
        let data = record.data().len();
        // Grouping keeps the data view only.
        spare.bytes_mut().truncate(data);
        let line = intake.line();
        if line == 1 {
            // Without an Input header, the first data Record binds the
            // Operations and gives a generated output header its width.
            binding.header(spare.bytes(), options)?;
            output.first(spare.bytes(), &binding.operations, &binding.keys)?;
        }
        let index = index.get_or_insert_with(|| {
            let index = records::FieldIndex::selecting(
                binding
                    .keys
                    .iter()
                    .copied()
                    .chain(binding.operations.fields()),
            );
            spare.index_fields(index.fresh());
            index
        });
        let mut context = grouping::Context::new(
            &mut binding.operations,
            &binding.keys,
            options,
            arithmetic,
            random.as_deref_mut(),
            output,
        );
        calculation.push_in_place(
            &mut spare,
            || grouping::Raw::for_keys(binding.keys.len(), index.fresh()),
            line,
            &mut context,
        )?;
    }
    let line = intake.line();
    let mut context = grouping::Context::new(
        &mut binding.operations,
        &binding.keys,
        options,
        arithmetic,
        None,
        output,
    );
    calculation.finish(line, &mut context)?;
    intake.finish()
}

fn sample_failure(kind: Kind, error: samples::Error) -> Failure {
    let reason = match error {
        #[cfg(test)]
        samples::Error::Capacity => "samples exceed 65536 values per operation per group",
        samples::Error::Allocation => "sample memory allocation failed",
        samples::Error::SortingAllocation => "sample sorting memory allocation failed",
    };
    unsupported(&format!("{} {reason}", kind.name()))
}

fn scalar_text_failure(error: scalar_text::Error) -> Failure {
    match error {
        scalar_text::Error::Allocation => unsupported("text value allocation failed"),
    }
}

fn quiet_nan() -> numerics::Value {
    numerics::canonical(fastmash_numeric_contract::Raw80::new(
        0x7fff,
        0xc000_0000_0000_0000,
    ))
}

/// Process entry, without Rust's runtime start. That start installs a
/// stack-overflow handler whose guard lookup reads /proc/self/maps on every
/// run, a large share of a tiny command's time. Closed standard streams are
/// already held open by `standard_io`'s preinitializer, and arguments come from
/// std's own glibc initializer, so only SIGPIPE needs the runtime's setting:
/// ignored, so failed writes are reported as errors. Before any thread
/// starts, an address-space limit caps the malloc arenas (`malloc_arenas`).
#[cfg(not(test))]
#[unsafe(no_mangle)]
extern "C" fn main(_argc: libc::c_int, _argv: *const *const libc::c_char) -> libc::c_int {
    // SAFETY: the process is still single-threaded; this is the disposition
    // Rust's runtime start sets.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };
    malloc_arenas::cap();
    run_process();
    0
}

/// The buffer standard input is read through: large reads keep system calls
/// off the per-record path, as GNU's stdio does with its own buffer.
const INPUT_BUFFER: usize = 128 << 10;

#[cfg_attr(test, allow(dead_code))]
fn run_process() {
    let terminal = terminal_style::Detection::from_env();
    let args = match options::collect_args(std::env::args_os()) {
        Ok(args) => args,
        Err(error) => {
            let mut stderr = standard_io::Stderr;
            let style = terminal.style(
                terminal_style::ColorMode::Auto,
                terminal_style::Destination::Stderr,
            );
            let _ = failure::write_prefix(&mut stderr, b"fastmash", style);
            let _ = stderr.write_all(reported(&error));
            std::process::exit(error.status);
        }
    };
    let name = args
        .first()
        .map_or(b"fastmash".as_slice(), |v| v.as_bytes());
    let mut stdin = io::BufReader::with_capacity(INPUT_BUFFER, standard_io::Stdin);
    let mut stdout = command_output::Stdout::new();
    let environment = Environment::from_env(terminal);
    // argc can be 0 (execve with an empty argv before Linux 5.18).
    let operands = args.get(1..).unwrap_or_default();
    let scan = options::scan_with_policy(
        operands,
        name,
        environment.posixly_correct,
        environment.locale,
    );
    let style = environment
        .terminal
        .style(scan.color, terminal_style::Destination::Stderr);
    let mut report = |error: &Failure| {
        let mut stderr = standard_io::Stderr;
        failure::write(&mut stderr, name, error, style).is_ok()
    };
    let result = run_scanned_with_sources(
        (&mut stdin, &mut command_sources::Files),
        &mut stdout,
        (scan, environment),
        name,
        &mut report,
        &mut random::OsSeedSource,
    );
    // The Command has completed its output, reporting any failure; a status of 1
    // may already stand for a diagnostic it could not write.
    let (mut status, mut reported_all) = match result {
        Ok(status) => (status, status != 1),
        Err(error) => (error.status, report(&error)),
    };
    // Standard output is still open where the Command stopped before any output.
    if let Err(error) = stdout.close()
        && error.raw_os_error() != Some(9)
    {
        let error = os_failure(&error, false);
        status = failure::combine(status, error.status);
        reported_all &= report(&error);
    }
    if !reported_all || !standard_io::finish_stderr() {
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
    fn a_refusal_without_memory_for_its_message_still_names_its_cause() {
        let refusal = unsupported("sort memory allocation failed");
        assert_eq!(reported(&refusal), b"sort memory allocation failed\n");
        let bare = Failure {
            status: 77,
            message: Vec::new(),
        };
        assert_eq!(reported(&bare), b"memory allocation failed\n");
    }

    #[test]
    fn a_hint_follows_the_cause_of_a_refusal_without_memory_for_its_message() {
        let hint = "install fastmash-sort-supervisor in the same directory as fastmash";
        MESSAGE_FAILS.with(|fails| fails.set(true));
        let error = unsupported_hint("unable to start sort supervisor", hint);
        assert_eq!(error.status, 77);
        assert_eq!(
            reported(&error),
            format!("memory allocation failed\nhint: {hint}\n").as_bytes()
        );
        let error = unsupported_hint("unable to start sort supervisor", hint);
        assert_eq!(
            reported(&error),
            format!("unable to start sort supervisor\nhint: {hint}\n").as_bytes()
        );
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
                let (_, names) = OperationSet::new(command.operations)?;
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
                    &mut records::FieldIndex::default(),
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
                    &mut records::FieldIndex::default(),
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
                    &mut records::FieldIndex::default(),
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
        let count = OperationSet::new(grammar::parse(&args(&["count", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0
            .needs(false)
            .random;
        assert!(
            initialize_random(count, Some(7), &mut PanicSource)
                .ok()
                .unwrap()
                .is_none()
        );

        let rand = OperationSet::new(grammar::parse(&args(&["rand", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0
            .needs(false)
            .random;
        assert!(
            initialize_random(rand, Some(7), &mut PanicSource)
                .ok()
                .unwrap()
                .is_some()
        );
        let mut source = FailingSource::default();
        let error = initialize_random(rand, None, &mut source).err().unwrap();
        assert_eq!(source.calls, 1);
        assert_eq!(error.status, 77);
        assert_eq!(error.message, b"unable to initialize random state\n");
    }

    #[test]
    fn lifecycle_orders_runtime_entropy_and_sort_admission() {
        let rand = OperationSet::new(grammar::parse(&args(&["rand", "1"])).ok().unwrap())
            .ok()
            .unwrap()
            .0
            .needs(false)
            .random;
        let runtime = unsupported("runtime first");
        let error = initialize_before_input(
            true,
            rand,
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
            rand,
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

    fn args(items: &[&str]) -> Vec<std::ffi::OsString> {
        items.iter().map(std::ffi::OsString::from).collect()
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
                &mut records::FieldIndex::default(),
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
mod result_names;
#[cfg(test)]
mod result_names_tests;

#[cfg(test)]
mod portable_routing_tests {
    use super::*;
    #[test]
    fn sorted_startup_retains_the_single_constructed_value() {
        let needs_random =
            OperationSet::new(grammar::parse(&["sum".into(), "1".into()]).ok().unwrap())
                .ok()
                .unwrap()
                .0
                .needs(false)
                .random;
        struct NoEntropy;
        impl random::SeedSource for NoEntropy {
            fn seed(&mut self) -> Result<u32, ()> {
                panic!("no random request")
            }
        }
        let (session, _) = initialize_before_input(
            true,
            needs_random,
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

#[cfg(test)]
mod environment_tests {
    use super::*;

    /// A command's status and output in an environment of `variables`.
    fn run_with(variables: &[(&str, &str)], args: &[&str], input: &[u8]) -> (i32, Vec<u8>) {
        let get = |key: &str| {
            variables
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| std::ffi::OsString::from(value))
        };
        let environment = Environment {
            locale: locale::Policy::resolve(get),
            posixly_correct: false,
            grouping: None,
            sort_memory: None,
            pipe_grouping: None,
            terminal: Default::default(),
        };
        let args: Vec<std::ffi::OsString> = args.iter().map(Into::into).collect();
        let mut reader = io::Cursor::new(input);
        let mut output = Vec::new();
        let status = run_in(
            &mut reader,
            &mut output,
            &args,
            b"fastmash",
            &mut |_| true,
            environment,
        )
        .unwrap_or_else(|error| error.status);
        (status, output)
    }

    #[test]
    fn the_locale_comes_from_the_environment_given() {
        // German numbers: a decimal comma in and out.
        let german = [("LANG", "de_AT.UTF-8")];
        assert_eq!(
            run_with(&german, &["sum", "1"], b"1,5\n3\n"),
            (0, b"4,5\n".to_vec())
        );
        assert_eq!(run_with(&[], &["sum", "1"], b"1,5\n3\n").0, 1);
        // Language order: Spanish sorts ñ as a letter after n.
        let spanish = [("LANG", "es_ES.UTF-8")];
        let args = ["-s", "-g", "1", "count", "1"];
        assert_eq!(
            run_with(&spanish, &args, "o\nñ\nn\nnz\n".as_bytes()),
            (0, "n\t1\nnz\t1\nñ\t1\no\t1\n".as_bytes().to_vec())
        );
        // A locale whose numbers Fastmash cannot write is refused.
        assert_eq!(
            run_with(&[("LC_ALL", "fr_FR")], &["sum", "1"], b"1\n").0,
            77
        );
    }
}
