//! The Sort route of a `-s` Command with Grouping keys: how its equal keys
//! are brought together. [`plan`] decides it before input is read, from the
//! Command and a few facts about where it runs; [`run`] takes it.

use super::{
    Failure, binding::Binding, buffered_stdout, calculate, command_output, intake, numerics,
    options, os_failure, projected_sort, random, replay, sorted_input, unsupported,
};

/// The sort that brings equal keys together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sort {
    /// In process, spilling sorted runs to temporary files where memory
    /// requires, keeping only the selected fields of each record (`packed`)
    /// or whole records.
    Native { packed: bool },
    /// The system `sort`, run by the sort supervisor.
    External,
}

/// A Command's Sort route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Route {
    /// Hash grouping first, with this setting, where standard input is a
    /// file; it gives up to `sort` where it cannot give the sorted Groups
    /// exactly.
    pub(super) hash: Option<projected_sort::Grouping>,
    /// Whether hash grouping holds input that cannot rewind, such as a pipe,
    /// so that the native sort can read it again ([`replay::Replay`]). The
    /// system sort reads standard input itself, so only a native sort can.
    pub(super) hold: bool,
    /// The sort, when hash grouping is not tried or gives up.
    pub(super) sort: Sort,
}

/// What the Sort route of a Command depends on.
pub(super) struct Facts {
    /// Whether hash grouping gives the Command's sorted Groups
    /// (`projected_sort::hash_eligible`).
    pub(super) hash_eligible: bool,
    /// The `FASTMASH_GROUPING` setting.
    pub(super) grouping: projected_sort::Grouping,
    /// Whether the locale is a language locale, whose ordering is built in.
    pub(super) language: bool,
    /// Whether the native sorter covers the Command's Operations.
    pub(super) covered: bool,
    /// Whether a native sort keeps only the selected fields
    /// (`projected_sort::packed`).
    pub(super) packed: bool,
    /// Where hash grouping holds piped input (`FASTMASH_PIPE_GROUPING`).
    pub(super) hold: Hold,
}

/// Where hash grouping holds input that cannot rewind, such as a pipe, so
/// that it can give up to a native sort: the `FASTMASH_PIPE_GROUPING`
/// setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hold {
    /// Never: piped input sorts (`sort`).
    Never,
    /// Before a language-locale sort (`language`, the default), which takes
    /// about as much memory as the held input and gains the most time.
    Language,
    /// Before any native sort (`hash`).
    Always,
}

impl Hold {
    /// The setting `value` gives.
    pub(super) fn setting(value: Option<&std::ffi::OsStr>) -> Result<Self, Failure> {
        match value.map(std::ffi::OsStr::to_str) {
            Some(Some("sort")) => Ok(Self::Never),
            None | Some(Some("language")) => Ok(Self::Language),
            Some(Some("hash")) => Ok(Self::Always),
            Some(_) => Err(unsupported(
                "FASTMASH_PIPE_GROUPING must be sort, language or hash",
            )),
        }
    }
}

/// The Sort route of a Command; `available` is asked as for [`native`].
pub(super) fn plan(facts: Facts, available: impl FnOnce() -> bool) -> Route {
    let grouping = facts.grouping;
    let hash =
        (facts.hash_eligible && grouping != projected_sort::Grouping::Sort).then_some(grouping);
    let sort = if native(facts.language, facts.covered, available) {
        Sort::Native {
            packed: facts.packed,
        }
    } else {
        Sort::External
    };
    Route {
        hash,
        hold: match facts.hold {
            Hold::Never => false,
            Hold::Language => facts.language,
            Hold::Always => true,
        } && hash.is_some()
            && sort != Sort::External,
        sort,
    }
}

/// Whether a Command sorts in process: in a language locale, whose ordering
/// is built in, and for the Operations the native sorter covers (`covered`);
/// otherwise where the system `sort` cannot run safely or work
/// (`available`, asked only then).
pub(super) fn native(language: bool, covered: bool, available: impl FnOnce() -> bool) -> bool {
    language || covered || !available()
}

/// Runs a Command by its Sort route: hash grouping, then the sort where it
/// gives up. `cleared` is the external route's startup check
/// (`sorted_input::admit`), which the startup order gives before input is
/// read.
#[allow(clippy::too_many_arguments)]
pub(super) fn run(
    route: Route,
    reader: &mut impl replay::Rewind,
    writer: &mut impl command_output::Transport,
    options: &options::Options,
    mut binding: Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    cleared: Option<sorted_input::Cleared>,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    // Where hash grouping gives up, standard input is back where it started.
    let mut input = replay::Replay::new(reader, route.hold);
    if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
        let first = if input.holding() {
            "hash first, holding the input, then "
        } else if route.hash.is_some() {
            "hash first, then "
        } else {
            ""
        };
        let sort = match route.sort {
            Sort::Native { packed: true } => "native sort of the selected fields",
            Sort::Native { packed: false } => "native sort of whole records",
            Sort::External => "system sort",
        };
        let message = format!("sort route: {first}{sort}\n");
        let _ = std::io::Write::write_all(&mut std::io::stderr(), message.as_bytes());
    }
    let reader = &mut input;
    if let Some(setting) = route.hash
        && let Some(status) = projected_sort::group_by_hash(
            reader,
            writer,
            options,
            &mut binding,
            arithmetic,
            setting,
            report,
        )?
    {
        return Ok(status);
    }
    if let Sort::Native { packed } = route.sort {
        let unread = reader
            .hand_over()
            .map_err(|error| intake::read_failure(&error))?;
        return projected_sort::run(
            reader, writer, options, binding, arithmetic, random, packed, unread, report,
        );
    }
    let cleared = cleared.ok_or_else(|| unsupported("internal sort startup check missing"))?;
    let sorting = sorted_input::Sorting::prepare(cleared, options)?;
    if let Some(record) = sorting.header() {
        binding.header(record, options)?;
    }
    // A weighted Command with no Input header at clean EOF has no calculation
    // domain. Reuse calculation and output finalization without asking sort to
    // interpret its unresolved named keys. Present headers and read errors keep
    // the ordinary external-sort path.
    if binding.operations.has_weighted_mean()
        && binding.unresolved_keys()
        && sorting.clean_header_eof(options)
    {
        let (capacity, line_buffered) = writer.buffering();
        let buffer = buffered_stdout::BufferedStdout::new(writer, capacity, line_buffered)
            .map_err(|e| os_failure(&e, false))?;
        let mut output = command_output::Results::new(buffer, options);
        let result = calculate(
            &mut std::io::empty(),
            &mut output,
            options,
            binding,
            arithmetic,
            random,
            None,
        );
        return Ok(command_output::complete(
            output.buffer,
            result,
            command_output::Transport::close,
            report,
        ));
    }
    let sorted = sorting.start(&binding.keys, options)?;
    let (capacity, line_buffered) = writer.buffering();
    let buffer = buffered_stdout::BufferedStdout::new(writer, capacity, line_buffered)
        .map_err(|e| os_failure(&e, false))?;
    let mut output = command_output::Results::new(buffer, options);
    let result = sorted.run(|reader, header| {
        calculate(
            reader,
            &mut output,
            options,
            binding,
            arithmetic,
            random,
            header,
        )
    });
    Ok(command_output::complete(
        output.buffer,
        result,
        command_output::Transport::close,
        report,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use projected_sort::Grouping;

    fn facts(hash_eligible: bool, grouping: Grouping, language: bool, covered: bool) -> Facts {
        Facts {
            hash_eligible,
            grouping,
            language,
            covered,
            packed: true,
            hold: Hold::Never,
        }
    }

    #[test]
    fn the_system_sort_is_probed_only_for_what_the_native_sorter_does_not_cover() {
        let unasked = || -> bool { panic!("probed") };
        assert!(native(true, false, unasked));
        assert!(native(false, true, unasked));
        assert!(!native(false, false, || true));
        assert!(native(false, false, || false));
        let native_sort = Sort::Native { packed: true };
        assert_eq!(
            plan(facts(false, Grouping::Hash, false, true), unasked).sort,
            native_sort
        );
        assert_eq!(
            plan(facts(false, Grouping::Hash, false, false), || true).sort,
            Sort::External
        );
    }

    #[test]
    fn hash_grouping_comes_first_where_eligible_and_not_turned_off() {
        for (eligible, grouping, hash) in [
            (true, Grouping::Hash, Some(Grouping::Hash)),
            (
                true,
                Grouping::RestartAfter(3),
                Some(Grouping::RestartAfter(3)),
            ),
            (true, Grouping::Sort, None),
            (false, Grouping::Hash, None),
        ] {
            let route = plan(facts(eligible, grouping, false, false), || true);
            assert_eq!(route.hash, hash, "{eligible} {grouping:?}");
            assert_eq!(route.sort, Sort::External);
        }
        // Hash grouping and its fallback are planned independently.
        assert_eq!(
            plan(facts(true, Grouping::Hash, true, false), || true),
            Route {
                hash: Some(Grouping::Hash),
                hold: false,
                sort: Sort::Native { packed: true },
            }
        );
    }

    #[test]
    fn piped_input_is_held_only_for_hash_grouping_before_a_native_sort() {
        let held = |hold, eligible, grouping, language, covered, available: bool| {
            let facts = Facts {
                hold,
                ..facts(eligible, grouping, language, covered)
            };
            plan(facts, || available).hold
        };
        // Before a language-locale sort, where set to.
        assert!(held(
            Hold::Language,
            true,
            Grouping::Hash,
            true,
            false,
            true
        ));
        assert!(!held(
            Hold::Language,
            true,
            Grouping::Hash,
            false,
            true,
            true
        ));
        // Always or never; the system sort never.
        assert!(held(Hold::Always, true, Grouping::Hash, false, true, true));
        assert!(held(
            Hold::Always,
            true,
            Grouping::Hash,
            false,
            false,
            false
        ));
        assert!(!held(
            Hold::Always,
            true,
            Grouping::Hash,
            false,
            false,
            true
        ));
        assert!(!held(Hold::Never, true, Grouping::Hash, true, false, true));
        // Only where hash grouping comes first.
        assert!(!held(Hold::Always, true, Grouping::Sort, false, true, true));
        assert!(!held(
            Hold::Always,
            false,
            Grouping::Hash,
            false,
            true,
            true
        ));
        assert_eq!(Hold::setting(None).ok(), Some(Hold::Language));
        assert_eq!(
            Hold::setting(Some("language".as_ref())).ok(),
            Some(Hold::Language)
        );
        assert!(Hold::setting(Some("yes".as_ref())).is_err());
    }
}
