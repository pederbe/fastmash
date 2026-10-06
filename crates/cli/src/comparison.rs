//! Dataset comparison owns source scans, numerical changes and delayed output.
//! The Command supplies its source resolver; callers learn no calculation seam.
use super::{
    Failure, binding, buffered_stdout,
    command_memory::reserve,
    command_output::{self, CommandOutput},
    command_sources::Resolver,
    decimal, headers, numeric_failure,
    numerics::{self, Numerics, Value},
    options, os_failure, unsupported,
};
use fastmash_numeric_contract::ValueClass;
use std::{
    cmp::Ordering,
    ffi::{OsStr, OsString},
    io::BufRead,
    os::unix::ffi::OsStrExt,
};

#[path = "comparison_keys.rs"]
mod keys;

pub(super) struct Request {
    pub before: OsString,
    pub after: OsString,
    pub ranking: Option<Ranking>,
}
#[derive(Clone, Copy)]
pub(super) struct Ranking {
    pub result: usize,
    pub percent: bool,
    pub limit: Option<u64>,
}

fn source_context(mut error: Failure, role: &str, path: &OsStr, utf8: bool) -> Failure {
    let mut quoted = Vec::new();
    super::named_fields::quote_with(path.as_bytes(), &mut quoted, utf8);
    super::append(
        &mut error,
        &[b"compare: ", role.as_bytes(), b" source ", &quoted, b"\n"],
    );
    error
}

fn finite(value: Value) -> bool {
    matches!(
        Numerics::value80(value).classify(),
        ValueClass::Zero | ValueClass::Subnormal | ValueClass::Normal
    )
}
fn checked(
    result: Result<Value, Failure>,
    operands_finite: bool,
    at: usize,
    stage: &str,
) -> Result<Value, Failure> {
    let result = result.and_then(|value| {
        if operands_finite && Numerics::value80(value).classify() == ValueClass::Infinity {
            Err(unsupported("comparison finite arithmetic overflow"))
        } else {
            Ok(value)
        }
    });
    result.map_err(|mut error| {
        super::append(
            &mut error,
            &[format!("compare: result {}, {stage}\n", at + 1).as_bytes()],
        );
        error
    })
}

fn push(row: &mut Vec<Vec<u8>>, bytes: &[u8]) -> Result<(), Failure> {
    let mut cell = Vec::new();
    reserve(&mut cell, bytes.len())?;
    cell.extend_from_slice(bytes);
    reserve(row, 1)?;
    row.push(cell);
    Ok(())
}

fn parts(bytes: &[&[u8]]) -> Result<Vec<u8>, Failure> {
    let size = bytes
        .iter()
        .try_fold(0usize, |size, bytes| size.checked_add(bytes.len()))
        .ok_or_else(|| unsupported("command memory allocation failed"))?;
    let mut label = Vec::new();
    reserve(&mut label, size)?;
    for bytes in bytes {
        label.extend_from_slice(bytes);
    }
    Ok(label)
}

/// Prepare the fixed schema with the ordinary aggregate label renderer. The
/// caller retains it until both sources and all changes have completed.
fn header<'r>(
    binding: &binding::Binding<'_>,
    mut field: impl FnMut(u64) -> Result<&'r [u8], Failure>,
    options: &options::Options,
) -> Result<Vec<Vec<u8>>, Failure> {
    let mut row = Vec::new();
    for &key in &binding.keys {
        let bytes = field(key)?;
        let generated = (!options.header_in).then(|| headers::generated(key));
        let label = generated
            .as_ref()
            .map_or_else(|| headers::label(bytes), |label| label.as_bytes());
        reserve(&mut row, 1)?;
        row.push(parts(&[b"key(", label, b")"])?);
    }
    for label in [b"presence".as_slice(), b"rank", b"rank_state"] {
        push(&mut row, label)?;
    }
    let mut requests = Vec::new();
    reserve(&mut requests, binding.operations.len())?;
    requests.extend(binding.operations.requests());
    let mut prepared = Ok(());
    let rendered = headers::render_fields(
        &requests,
        field,
        (
            options.output,
            options.record_end,
            options.presentation.profile,
        ),
        options.header_in,
        &options.result_names,
        |header, _| {
            if prepared.is_err() {
                return;
            }
            header.with_fragments(|fragments| {
                prepared = (|| {
                    let label = parts(fragments)?;
                    for prefix in [
                        b"before".as_slice(),
                        b"after",
                        b"difference",
                        b"difference_state",
                        b"percentage",
                        b"percentage_state",
                    ] {
                        reserve(&mut row, 1)?;
                        row.push(parts(&[prefix, b"(", &label, b")"])?);
                    }
                    Ok(())
                })();
            });
        },
    );
    // Label preparation and field validation can each fail; retain both under
    // the same failure precedence as source and output completion.
    super::command_sources::complete(rendered, prepared)?;
    Ok(row)
}
fn number(row: &mut Vec<Vec<u8>>, value: Value, options: &options::Options) -> Result<(), Failure> {
    let bytes = options.presentation.render(Numerics::value80(value))?;
    reserve(row, 1)?;
    row.push(bytes);
    Ok(())
}

struct Report {
    key: keys::Key,
    cells: Vec<Vec<u8>>,
    score: Option<Value>,
}

fn report(
    before: Option<&[Value]>,
    after: Option<&[Value]>,
    options: &options::Options,
    ranking: Option<Ranking>,
) -> Result<(Vec<Vec<u8>>, Option<Value>), Failure> {
    let mut row = Vec::new();
    let mut score = None;
    if before.is_none() && after.is_none() {
        return Ok((row, score));
    }
    push(
        &mut row,
        if before.is_some() && after.is_some() {
            b"matched"
        } else if before.is_some() {
            b"removed"
        } else {
            b"added"
        },
    )?;
    push(&mut row, b"")?;
    push(
        &mut row,
        if ranking.is_none() {
            b"not_requested"
        } else if before.is_none() || after.is_none() {
            b"one_sided"
        } else {
            b"unavailable"
        },
    )?;
    let count = before.or(after).expect("one side present").len();
    for at in 0..count {
        if let Some(before) = before {
            number(&mut row, before[at], options)?;
        } else {
            push(&mut row, b"")?;
        }
        if let Some(after) = after {
            number(&mut row, after[at], options)?;
        } else {
            push(&mut row, b"")?;
        }
        let (Some(before), Some(after)) = (before, after) else {
            push(&mut row, b"")?;
            push(&mut row, b"not_matched")?;
            push(&mut row, b"")?;
            push(&mut row, b"not_matched")?;
            continue;
        };
        let (before, after) = (before[at], after[at]);
        let difference = checked(
            decimal::subtract(after, before),
            finite(before) && finite(after),
            at,
            "subtraction",
        )?;
        if matches!(
            Numerics::value80(difference).classify(),
            ValueClass::Nan { .. }
        ) {
            push(&mut row, b"")?;
            push(&mut row, b"unordered")?;
        } else {
            number(&mut row, difference, options)?;
            push(&mut row, b"available")?;
            if ranking.is_some_and(|rank| rank.result == at && !rank.percent) {
                score = Some(difference);
            }
        }
        let unavailable = if Numerics::value80(before).classify() == ValueClass::Zero {
            Some(b"zero_baseline".as_slice())
        } else if !finite(before) {
            Some(b"nonfinite_baseline".as_slice())
        } else if !finite(after) {
            Some(b"nonfinite_after".as_slice())
        } else {
            None
        };
        if let Some(state) = unavailable {
            push(&mut row, b"")?;
            push(&mut row, state)?;
        } else {
            let quotient = checked(
                decimal::divide(difference, before),
                true,
                at,
                "percentage division",
            )?;
            let percentage = checked(
                decimal::multiply(quotient, Numerics::promote_u64(100)),
                true,
                at,
                "percentage multiplication",
            )?;
            number(&mut row, percentage, options)?;
            push(&mut row, b"available")?;
            if ranking.is_some_and(|rank| rank.result == at && rank.percent) {
                score = Some(percentage);
            }
        }
    }
    if score.is_some() {
        row[2].clear();
        reserve(&mut row[2], b"ranked".len())?;
        row[2].extend_from_slice(b"ranked");
    }
    Ok((row, score))
}

fn align(
    before: Vec<keys::Summary>,
    after: Vec<keys::Summary>,
    options: &options::Options,
    ranking: Option<Ranking>,
) -> Result<Vec<Report>, Failure> {
    let capacity = before
        .len()
        .checked_add(after.len())
        .ok_or_else(|| unsupported("command memory allocation failed"))?;
    let mut rows = Vec::new();
    reserve(&mut rows, capacity)?;
    let mut before = before.into_iter().peekable();
    let mut after = after.into_iter().peekable();
    while before.peek().is_some() || after.peek().is_some() {
        let order = match (before.peek(), after.peek()) {
            (Some(left), Some(right)) => left.key.cmp(&right.key),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => unreachable!(),
        };
        let left = (order != std::cmp::Ordering::Greater)
            .then(|| before.next())
            .flatten();
        let right = (order != std::cmp::Ordering::Less)
            .then(|| after.next())
            .flatten();
        let representative = left.as_ref().or(right.as_ref()).expect("one key present");
        let (mut row, score) = report(
            left.as_ref().map(|summary| summary.values.as_slice()),
            right.as_ref().map(|summary| summary.values.as_slice()),
            options,
            ranking,
        )
        .map_err(|error| keys::context(error, &representative.spelling, options.locale.utf8))?;
        let keys::Summary {
            key,
            spelling: mut prefix,
            ..
        } = left.or(right).expect("one key present");
        reserve(&mut prefix, row.len())
            .map_err(|error| keys::context(error, &prefix, options.locale.utf8))?;
        prefix.append(&mut row);
        rows.push(Report {
            key,
            cells: prefix,
            score,
        });
    }
    Ok(rows)
}

/// All input, summary and change calculations have completed before selection.
/// Only available typed changes enter magnitude ordering; display never does.
fn rank(rows: &mut Vec<Report>, ranking: Ranking) -> Result<(), Failure> {
    rows.sort_unstable_by(|left, right| {
        let order = match (left.score, right.score) {
            (Some(left), Some(right)) => match numerics::compare_magnitude(left, right) {
                numerics::Comparison::Less => Ordering::Greater,
                numerics::Comparison::Equal => Ordering::Equal,
                numerics::Comparison::Greater => Ordering::Less,
                numerics::Comparison::Unordered => unreachable!("ranking scores exclude NaNs"),
            },
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        order.then_with(|| left.key.cmp(&right.key))
    });
    if let Some(limit) = ranking.limit {
        let mut retained = 0u64;
        rows.retain(|row| {
            if row.score.is_none() {
                true
            } else if retained < limit {
                retained += 1;
                true
            } else {
                false
            }
        });
    }
    for (ordinal, row) in (1u64..).zip(rows.iter_mut().take_while(|row| row.score.is_some())) {
        let cell = &mut row.cells[row.key.len() + 1];
        reserve(cell, 20)?;
        let mut remaining = ordinal;
        while remaining != 0 {
            cell.push(b'0' + (remaining % 10) as u8);
            remaining /= 10;
        }
        cell.reverse();
    }
    Ok(())
}

pub(super) fn run(
    reader: &mut impl BufRead,
    writer: &mut impl command_output::Transport,
    sources: &mut impl Resolver,
    prepared: super::preparation::Comparison<'_>,
    name: &[u8],
    diagnostics: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let (options, request, operations, keys) = prepared.into_parts();
    let mut binding = binding::Binding::new(name, operations, keys)?;
    let needs = binding
        .operations
        .needs(options.presentation.spec.is_some());
    let mut arithmetic = Numerics::with(needs.numerics).map_err(numeric_failure)?;
    let (capacity, line_buffered) = writer.buffering();
    let buffer = buffered_stdout::BufferedStdout::new(writer, capacity, line_buffered)
        .map_err(|error| os_failure(&error, false))?;
    let mut output = command_output::Results::new(buffer, options);
    let result = (|| {
        let before = sources
            .with_source(reader, &request.before, |input| {
                keys::scan(
                    input,
                    &mut binding,
                    options,
                    &mut arithmetic,
                    options.header_out,
                )
            })
            .map_err(|error| {
                source_context(error, "before", &request.before, options.locale.utf8)
            })?;
        let after = sources
            .with_source(reader, &request.after, |input| {
                keys::scan(input, &mut binding, options, &mut arithmetic, false)
            })
            .map_err(|error| source_context(error, "after", &request.after, options.locale.utf8))?;
        let mut rows = align(before.summaries, after.summaries, options, request.ranking).map_err(
            |error| {
                let error = source_context(error, "before", &request.before, options.locale.utf8);
                source_context(error, "after", &request.after, options.locale.utf8)
            },
        )?;
        if let Some(ranking) = request.ranking {
            rank(&mut rows, ranking)?;
        }
        if let Some(header) = before.header.or(after.header) {
            output.row(&header, options.output)?;
        }
        for row in rows {
            output.row(&row.cells, options.output)?;
        }
        Ok(())
    })();
    Ok(command_output::complete(
        output.buffer,
        result,
        command_output::Transport::close,
        diagnostics,
    ))
}
