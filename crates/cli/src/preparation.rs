//! Ordered Command preparation with private payloads for existing runners.
//! Cases certify completed preparation; startup and Input checks can still fail.
use super::{
    Failure, Kind, binding, failure, grammar, operation_set,
    options::{Options, Setting},
    projected_sort,
    records::Separator,
    sort_route, sorted_input, unsupported,
};

#[derive(Default)]
pub(super) struct Controls {
    calculation: Option<&'static [u8]>,
    selection: Option<&'static [u8]>,
    comparison: Option<&'static [u8]>,
    csv_input: Option<&'static [u8]>,
}

impl Controls {
    pub(super) fn record(&mut self, setting: Setting, spelling: &'static [u8]) {
        let (calculation, selection, comparison) = match setting {
            Setting::Full => (true, false, true),
            Setting::TableOnly | Setting::Filler | Setting::Collapse | Setting::Seed => {
                (true, true, true)
            }
            Setting::ResultName | Setting::Format | Setting::Round => (true, true, false),
            Setting::Vnlog | Setting::Unavailable => (false, true, true),
            Setting::HeaderOut
            | Setting::CsvOut
            | Setting::CsvIn
            | Setting::Csv
            | Setting::Na
            | Setting::Sort
            | Setting::IgnoreCase
            | Setting::Group
            | Setting::Output => (true, false, false),
            Setting::ZeroTerminated
            | Setting::Input
            | Setting::Whitespace
            | Setting::HeaderIn
            | Setting::Headers
            | Setting::Comments
            | Setting::Color
            | Setting::NoColor
            | Setting::Help
            | Setting::Version => (false, false, false),
        };
        if calculation {
            self.calculation.get_or_insert(spelling);
        }
        if selection {
            self.selection.get_or_insert(spelling);
        }
        if comparison {
            self.comparison.get_or_insert(spelling);
        }
    }
    pub(super) fn record_unavailable(&mut self, spelling: &'static [u8]) {
        self.calculation.get_or_insert(spelling);
    }
    pub(super) fn record_csv_input(&mut self, spelling: &'static [u8]) {
        self.csv_input.get_or_insert(spelling);
    }
}

fn validate_comparison(options: &Options) -> Result<(), Failure> {
    if let Some(option) = options.controls.comparison {
        return Err(unsupported(&format!(
            "compare does not support --{}",
            String::from_utf8_lossy(option)
        )));
    }
    Ok(())
}
fn validate_selection(options: &Options) -> Result<(), Failure> {
    if let Some(option) = options.controls.selection {
        return Err(unsupported(&format!(
            "Top-N selection does not support --{}",
            String::from_utf8_lossy(option)
        )));
    }
    Ok(())
}

fn validate_csv(options: &Options, mode: super::grammar::Mode) -> Result<(), Failure> {
    if !options.csv_in && !options.csv_out {
        return Ok(());
    }
    if options.csv_out && options.supplied_output {
        return Err(failure(
            b"CSV output conflicts with --output-delimiter\n".to_vec(),
        ));
    }
    if options.record_end == 0 {
        return Err(failure(b"CSV conflicts with --zero-terminated\n".to_vec()));
    }
    if options.vnlog {
        return Err(failure(b"CSV conflicts with --vnlog\n".to_vec()));
    }
    if options.csv_in
        && let Some(option) = options.controls.csv_input
    {
        return Err(failure(
            format!(
                "CSV input conflicts with --{}\n",
                String::from_utf8_lossy(option)
            )
            .into_bytes(),
        ));
    }
    if !matches!(
        mode,
        super::grammar::Mode::Aggregate
            | super::grammar::Mode::Line
            | super::grammar::Mode::Select(_)
            | super::grammar::Mode::Compare
    ) {
        return Err(unsupported(
            "CSV is supported only for aggregate and per-row operations",
        ));
    }
    Ok(())
}

fn validate_health(options: &Options) -> Result<(), Failure> {
    if let Some(option) = options.controls.calculation {
        return Err(failure(
            format!(
                "health does not support calculation option '--{}'\n",
                String::from_utf8_lossy(option)
            )
            .into_bytes(),
        ));
    }
    if options.vnlog && options.input != Separator::Whitespace {
        return Err(failure(
            b"vnlog processing is whitespace-delimited\n".to_vec(),
        ));
    }
    Ok(())
}

fn validate_annotation(options: &Options) -> Result<(), Failure> {
    if !options.vnlog {
        return Ok(());
    }
    let message: Option<&[u8]> = if options.filler.as_ref() != b"-" {
        Some(b"vnlog processing always uses '-' for empty fields\n")
    } else if options.input != Separator::Whitespace {
        Some(b"vnlog processing is whitespace-delimited\n")
    } else if options.output != b' ' {
        Some(b"vnlog processing always uses ' ' to separate output fields\n")
    } else if options.explicit_output {
        Some(b"vnlog processing always uses the default output delimiter\n")
    } else if options.record_end != b'\n' {
        Some(b"vnlog processing always uses '\\n' to terminate output lines\n")
    } else {
        None
    };
    message.map_or(Ok(()), |message| Err(failure(message.to_vec())))
}

fn require_header(options: &Options, named: bool) -> Result<(), Failure> {
    if named && !options.header_in {
        return Err(failure(
            b"-H or --header-in must be used with named columns\n".to_vec(),
        ));
    }
    Ok(())
}

fn validate_comparison_request(
    command: &grammar::Command,
    options: &Options,
) -> Result<(), Failure> {
    if command.operations.iter().any(|request| {
        request.kind.is_line()
            || matches!(
                request.kind,
                Kind::First | Kind::Last | Kind::Rand | Kind::Unique | Kind::Collapse
            )
    }) {
        return Err(unsupported(
            "compare requires numerical aggregate operations",
        ));
    }
    require_header(
        options,
        !options.header_in
            && (command
                .keys
                .iter()
                .any(|field| matches!(field, grammar::Field::Name(_)))
                || command
                    .operations
                    .iter()
                    .any(|request| match &request.selector {
                        grammar::Selector::Single(field) => {
                            matches!(field, grammar::Field::Name(_))
                        }
                        grammar::Selector::Pair { left, right } => {
                            matches!(left, grammar::Field::Name(_))
                                || matches!(right, grammar::Field::Name(_))
                        }
                    })),
    )?;
    options.result_names.validate(
        grammar::Mode::Aggregate,
        options.explicit_header_out,
        command.operations.len(),
        options.output,
        options.record_end,
        options.csv_out,
    )?;
    Ok(())
}

pub(super) enum Prepared<'o, 'p> {
    Ordinary(Ordinary<'o, 'p>),
    Selection(Selection<'o>),
    Comparison(Comparison<'o>),
    Health(Health<'o>),
    Table(Table<'o>),
}

pub(super) struct Ordinary<'o, 'p> {
    options: &'o Options,
    binding: super::binding::Binding<'p>,
    needs: super::operation_set::Needs,
    route: Option<super::sort_route::Route>,
}
impl<'o, 'p> Ordinary<'o, 'p> {
    pub(super) fn into_parts(
        self,
    ) -> (
        &'o Options,
        super::binding::Binding<'p>,
        super::operation_set::Needs,
        Option<super::sort_route::Route>,
    ) {
        (self.options, self.binding, self.needs, self.route)
    }
}

pub(super) struct Selection<'o> {
    options: &'o Options,
    request: super::selection::Selection,
    binding: super::binding::Selection,
}
impl<'o> Selection<'o> {
    pub(super) fn into_parts(
        self,
    ) -> (
        &'o Options,
        super::selection::Selection,
        super::binding::Selection,
    ) {
        (self.options, self.request, self.binding)
    }
}

pub(super) struct Comparison<'o> {
    options: &'o Options,
    request: super::comparison::Request,
    operations: Vec<grammar::Request>,
    keys: Vec<grammar::Field>,
}
impl<'o> Comparison<'o> {
    pub(super) fn into_parts(
        self,
    ) -> (
        &'o Options,
        super::comparison::Request,
        Vec<grammar::Request>,
        Vec<grammar::Field>,
    ) {
        (self.options, self.request, self.operations, self.keys)
    }
}

pub(super) struct Health<'o> {
    options: &'o Options,
}
impl<'o> Health<'o> {
    pub(super) fn into_options(self) -> &'o Options {
        self.options
    }
}

pub(super) struct Table<'o> {
    options: &'o Options,
    mode: grammar::Mode,
    keys: Vec<grammar::Field>,
}
impl<'o> Table<'o> {
    pub(super) fn into_parts(self) -> (&'o Options, grammar::Mode, Vec<grammar::Field>) {
        (self.options, self.mode, self.keys)
    }
}

/// Completes the existing pre-startup checks without acquiring any transport.
pub(super) fn prepare<'o, 'p>(
    options: &'o mut Options,
    command: super::grammar::Command,
    environment: &super::Environment,
    name: &'p [u8],
) -> Result<Prepared<'o, 'p>, Failure> {
    let policy = environment.locale;
    match command.mode {
        grammar::Mode::Compare => {
            validate_csv(options, command.mode)?;
            validate_comparison(options)?;
            validate_comparison_request(&command, options)?;
            policy.numbers()?;
            if options.ignore_case {
                policy.case_folding()?;
            }
            return Ok(Prepared::Comparison(Comparison {
                options,
                request: command.comparison.expect("comparison request"),
                operations: command.operations,
                keys: command.keys,
            }));
        }
        grammar::Mode::Health => {
            validate_health(options)?;
            policy.numbers()?;
            return Ok(Prepared::Health(Health { options }));
        }
        grammar::Mode::Reverse
        | grammar::Mode::Transpose
        | grammar::Mode::Noop
        | grammar::Mode::Check { .. }
        | grammar::Mode::Dedup => {
            prepare_formats_and_mode(options, &command, policy)?;
            validate_annotation(options)?;
            return Ok(Prepared::Table(Table {
                options,
                mode: command.mode,
                keys: command.keys,
            }));
        }
        grammar::Mode::Select(request) => {
            let field = command.ranking.expect("selection has one ranking Field");
            require_header(
                options,
                matches!(field, grammar::Field::Name(_))
                    || command
                        .keys
                        .iter()
                        .any(|key| matches!(key, grammar::Field::Name(_))),
            )?;
            validate_csv(options, command.mode)?;
            validate_selection(options)?;
            if options.ignore_case && !command.keys.is_empty() {
                policy.case_folding()?;
            }
            if options.sort && !command.keys.is_empty() {
                policy.sorting()?;
                projected_sort::admit_memory(options.sort_memory.as_deref())?;
            }
            let binding = binding::Selection::new(field, command.keys)?;
            policy.numbers()?;

            return Ok(Prepared::Selection(Selection {
                options,
                request,
                binding,
            }));
        }
        grammar::Mode::Aggregate | grammar::Mode::Line | grammar::Mode::Crosstab => {}
    }
    prepare_formats_and_mode(options, &command, policy)?;
    let binding = binding::Binding::new(name, command.operations, command.keys)?;
    let needs = binding
        .operations
        .needs(options.presentation.spec.is_some());
    if needs.numeric_locale {
        policy.numbers()?;
    }
    require_header(options, binding.names_fields())?;
    validate_annotation(options)?;
    // The Sort route of a sorted Command with Grouping keys.
    let route = if options.sort && !binding.keys.is_empty() {
        policy.sorting()?;
        if options.csv_in {
            // Decoded Records use their own lossless retention route, covering
            // every admitted Operation without a text-sort fallback.
            None
        } else {
            let native = binding.operations.native_sort();
            let facts = sort_route::Facts {
                hash_eligible: projected_sort::hash_eligible(options, &binding.operations),
                grouping: projected_sort::grouping(environment.grouping.as_deref())?,
                language: policy.language(),
                covered: native != operation_set::NativeSort::Unsupported,
                packed: projected_sort::packed(options, native),
                hold: sort_route::Hold::setting(environment.pipe_grouping.as_deref())?,
            };
            Some(sort_route::plan(facts, || {
                sorted_input::available(options.input)
            }))
        }
    } else {
        None
    };

    Ok(Prepared::Ordinary(Ordinary {
        options,
        binding,
        needs,
        route,
    }))
}

/// Common format and Mode prefix, before family-specific fallible preparation.
fn prepare_formats_and_mode(
    options: &mut Options,
    command: &super::grammar::Command,
    policy: super::locale::Policy,
) -> Result<(), Failure> {
    validate_csv(options, command.mode)?;
    options.result_names.validate(
        command.mode,
        options.explicit_header_out,
        command.operations.len(),
        options.output,
        options.record_end,
        options.csv_out,
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

    Ok(())
}
