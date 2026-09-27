//! Table-mode dispatch and streaming reversal/pass-through.
use super::{
    Failure, annotated, command_output, failure, grammar::Mode, headers::output::Buffered,
    options::Options, os_failure, records, unsupported,
};
use std::io::BufRead;

pub(super) fn run(
    reader: &mut impl BufRead,
    writer: &mut impl command_output::Transport,
    options: &Options,
    mode: Mode,
    keys: Vec<super::grammar::Field>,
    report: &mut impl FnMut(&Failure) -> bool,
) -> Result<i32, Failure> {
    let (capacity, line_buffered) = writer.buffering();
    let mut output =
        Buffered::new(writer, capacity, line_buffered).map_err(|e| os_failure(&e, false))?;
    let result = match mode {
        Mode::Transpose => super::transpose::run(reader, &mut output, options),
        Mode::Check { lines, fields } => {
            super::table_checks::check(reader, &mut output, options, lines, fields)
        }
        Mode::Dedup => super::table_checks::dedup(reader, &mut output, options, keys),
        _ => process(reader, &mut output, options, mode),
    };
    Ok(command_output::complete(
        output,
        result,
        command_output::Transport::close,
        report,
    ))
}

fn process<W: std::io::Write>(
    reader: &mut impl BufRead,
    output: &mut Buffered<'_, W>,
    options: &Options,
    mode: Mode,
) -> Result<(), Failure> {
    let mut record = Vec::new();
    let mut spans = Vec::new();
    let mut line = 0u64;
    let mut previous_width = 0;
    loop {
        match records::read_record_terminated(reader, &mut record, usize::MAX, options.record_end) {
            Ok(false) => return Ok(()),
            Ok(true) => (),
            Err(records::ReadError::Io(e)) => return Err(os_failure(&e, true)),
            Err(_) => return Err(unsupported("record memory allocation failed")),
        }
        if mode == Mode::Noop {
            if (options.vnlog && annotated::skip_data(&record))
                || (!options.vnlog && options.skip_comments && records::is_comment(&record))
            {
                continue;
            }
            if options.full {
                output.raw(&record);
                output.raw(&[options.record_end]);
            }
            continue;
        }
        if options.vnlog {
            if !annotated::prepare(&mut record, line == 0)? {
                continue;
            }
        } else if options.skip_comments && records::is_comment(&record) {
            continue;
        }
        line = line
            .checked_add(1)
            .ok_or_else(|| unsupported("record count exceeds u64 limit"))?;
        spans.clear();
        for span in records::fields(&record, options.input) {
            super::command_memory::reserve(&mut spans, 1)?;
            spans.push(span);
        }
        if options.strict && line > 1 && spans.len() != previous_width {
            return Err(failure(format!("reverse-field input error: line {line} has {} fields (previous lines had {previous_width});\nsee --help to disable strict mode\n", spans.len()).into_bytes()));
        }
        previous_width = spans.len();
        if line == 1 && options.header_in && !options.header_out {
            continue;
        }
        if line == 1 && !options.header_in && options.header_out {
            for i in (1..=spans.len()).rev() {
                if i != spans.len() {
                    output.raw(&[options.output]);
                }
                output.formatted(format!("field-{i}").as_bytes());
            }
            output.raw(&[options.record_end]);
        }
        if line == 1 && options.vnlog {
            output.raw(b"# ");
        }
        for (at, span) in spans.iter().rev().enumerate() {
            if at != 0 {
                output.raw(&[options.output]);
            }
            output.raw(&record[span.start..span.start + span.length]);
        }
        output.raw(&[options.record_end]);
    }
}
