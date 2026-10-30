//! Plan native sorting and optional Hash grouping before input is read.
use super::{
    Failure, binding::Binding, command_output, intake, numerics, options, projected_sort, random,
    replay, unsupported,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Route {
    pub(super) hash: Option<projected_sort::Grouping>,
    /// Retain input that cannot rewind so Hash grouping may give up to sorting.
    pub(super) hold: bool,
    /// Keep only selected fields, or preserve whole Records.
    pub(super) packed: bool,
}

pub(super) struct Facts {
    pub(super) hash_eligible: bool,
    pub(super) grouping: projected_sort::Grouping,
    pub(super) language: bool,
    pub(super) packed: bool,
    pub(super) hold: Hold,
}

/// The `FASTMASH_PIPE_GROUPING` setting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hold {
    Never,
    Language,
    Always,
}

impl Hold {
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

pub(super) fn plan(facts: Facts) -> Route {
    let hash = (facts.hash_eligible && facts.grouping != projected_sort::Grouping::Sort)
        .then_some(facts.grouping);
    Route {
        hash,
        hold: hash.is_some()
            && match facts.hold {
                Hold::Never => false,
                Hold::Language => facts.language,
                Hold::Always => true,
            },
        packed: facts.packed,
    }
}

/// Reports a Command's Sort route under `FASTMASH_SORT_TRACE`.
pub(super) fn trace(route: &str) {
    if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
        let message = format!("sort route: {route}\n");
        let _ = std::io::Write::write_all(&mut std::io::stderr(), message.as_bytes());
    }
}

#[allow(clippy::too_many_arguments)]
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn run(
    route: Route,
    reader: &mut impl replay::Rewind,
    writer: &mut impl command_output::Transport,
    options: &options::Options,
    mut binding: Binding<'_>,
    arithmetic: &mut numerics::Numerics,
    random: Option<&mut random::RandomState>,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let mut input = replay::Replay::new(reader, route.hold);
    if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
        let first = if input.holding() {
            "hash first, holding the input, then "
        } else if route.hash.is_some() {
            "hash first, then "
        } else {
            ""
        };
        let sort = if route.packed {
            "native sort of the selected fields"
        } else {
            "native sort of whole records"
        };
        trace(&format!("{first}{sort}"));
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
    let unread = reader
        .hand_over()
        .map_err(|error| intake::read_failure(&error))?;
    projected_sort::run(
        reader,
        writer,
        options,
        binding,
        arithmetic,
        random,
        route.packed,
        unread,
        report,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use projected_sort::Grouping;

    #[test]
    fn hash_grouping_and_input_retention_are_planned_independently_of_storage() {
        for grouping in [Grouping::Hash, Grouping::Sort, Grouping::RestartAfter(3)] {
            for eligible in [false, true] {
                for language in [false, true] {
                    for packed in [false, true] {
                        for hold in [Hold::Never, Hold::Language, Hold::Always] {
                            let route = plan(Facts {
                                hash_eligible: eligible,
                                grouping,
                                language,
                                packed,
                                hold,
                            });
                            let hash = eligible && grouping != Grouping::Sort;
                            assert_eq!(route.hash, hash.then_some(grouping));
                            assert_eq!(route.packed, packed);
                            assert_eq!(
                                route.hold,
                                hash && (hold == Hold::Always
                                    || (hold == Hold::Language && language))
                            );
                        }
                    }
                }
            }
        }
        assert_eq!(Hold::setting(None).ok(), Some(Hold::Language));
        assert_eq!(
            Hold::setting(Some("hash".as_ref())).ok(),
            Some(Hold::Always)
        );
        assert_eq!(Hold::setting(Some("sort".as_ref())).ok(), Some(Hold::Never));
        assert!(Hold::setting(Some("yes".as_ref())).is_err());
    }
}
