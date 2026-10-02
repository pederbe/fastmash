//! Table-mode dispatch and streaming reversal/pass-through.
use super::{
    Failure,
    buffered_stdout::BufferedStdout,
    command_output, failure,
    grammar::Mode,
    intake::{self, Intake},
    options::Options,
    os_failure, records,
};
use fastmash_conversion::field_policy::FieldRange;
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
        BufferedStdout::new(writer, capacity, line_buffered).map_err(|e| os_failure(&e, false))?;
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
    output: &mut BufferedStdout<'_, W>,
    options: &Options,
    mode: Mode,
) -> Result<(), Failure> {
    let mut record = Vec::new();
    if mode == Mode::Noop {
        // GNU's noop reads no Input header (datamash.c noop_file).
        let mut intake = Intake::new(options, intake::Header::None);
        while let Some(row) = intake.next(reader, &mut record)? {
            if options.full {
                output.raw(row.raw());
                output.raw(&[options.record_end]);
            }
        }
        return intake.finish();
    }
    let mut intake = Intake::new(options, intake::Header::of(options));
    let mut reversal = Reversal {
        spans: Vec::new(),
        previous_width: 0,
    };
    if intake.header(reader, &mut record, |_| Ok(()))? {
        reversal.write(&record, 1, output, options)?;
    }
    while let Some(row) = intake.next(reader, &mut record)? {
        reversal.write(row.data(), intake.line(), output, options)?;
    }
    intake.finish()
}

struct Reversal {
    spans: Vec<FieldRange>,
    previous_width: usize,
}

impl Reversal {
    /// Writes Record `line` with its fields reversed (datamash.c
    /// reverse_fields_in_file); line 1 may be the Input header.
    fn write<W: std::io::Write>(
        &mut self,
        record: &[u8],
        line: u64,
        output: &mut BufferedStdout<'_, W>,
        options: &Options,
    ) -> Result<(), Failure> {
        let spans = &mut self.spans;
        spans.clear();
        for span in records::fields(record, options.input) {
            super::command_memory::reserve(spans, 1)?;
            spans.push(span);
        }
        if options.strict && line > 1 && spans.len() != self.previous_width {
            return Err(failure(format!("reverse-field input error: line {line} has {} fields (previous lines had {});\nsee --help to disable strict mode\n", spans.len(), self.previous_width).into_bytes()));
        }
        self.previous_width = spans.len();
        if line == 1 && options.header_in && !options.header_out {
            return Ok(());
        }
        if line == 1 && !options.header_in && options.header_out {
            for i in (1..=spans.len()).rev() {
                if i != spans.len() {
                    output.raw(&[options.output]);
                }
                output.formatted(super::headers::generated(i as u64).as_bytes());
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
        Ok(())
    }
}
