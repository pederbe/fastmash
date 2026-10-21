//! Overlap a native Spill merge with calculation through an anonymous pipe.
//! Only selected fields need text again. Empty unselected fields preserve
//! selector positions and field-count diagnostics. Numeric crossings use the
//! serial collector instead, since their original neighbours must be retained.
#![forbid(unsafe_code)]
use super::command_output::CommandOutput;
use super::{Failure, Intake, failure, grouping, intake, records, spill, unsupported};
use std::io::Write;

const BUFFER: usize = 64 << 10;
pub(super) const MEMORY: usize = 3 * BUFFER;

fn pipe_error(error: impl std::fmt::Display) -> Failure {
    failure(format!("native sort pipe I/O error: {error}\n").into_bytes())
}

/// Read, write and kernel pipe buffers share the otherwise unused final-merge
/// writer allowance. Numeric state and output stay on the calling thread.
/// `None` leaves sort emission and calculation untouched for the serial route.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn calculate<S: spill::Codec, O: CommandOutput>(
    sorted: &mut spill::Finished<S>,
    layout: &[u64],
    context: &mut grouping::Context<'_, O>,
) -> Result<Option<()>, Failure> {
    let (options, keys) = (context.options, context.keys);
    let Ok((reader, writer)) = std::io::pipe() else {
        return Ok(None);
    };
    if !matches!(
        nix::fcntl::fcntl(&reader, nix::fcntl::FcntlArg::F_GETPIPE_SZ),
        Ok(size) if size > 0 && size as usize <= BUFFER
    ) {
        return Ok(None);
    }
    let mut reader = spill::RunReader::new(BUFFER, reader)?;
    let mut writer = spill::RunWriter::new(BUFFER, writer)?;
    let index = records::FieldIndex::selecting(layout.iter().copied());
    let mut spare = grouping::Raw::for_keys(keys.len(), index.fresh())?;
    std::thread::scope(|scope| {
        let merge = std::thread::Builder::new()
            .name("sort-merge".into())
            .spawn_scoped(scope, move || {
                // Cancellation closes our reader. Keep that pipe's SIGPIPE
                // local to this disposable thread, leaving stdout's default
                // signal behaviour unchanged on the calling thread.
                use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
                let mut blocked = SigSet::empty();
                blocked.add(Signal::SIGPIPE);
                pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&blocked), None)
                    .map_err(|_| unsupported("unable to block native sort pipe signal"))?;
                sorted.emit(|record| {
                    if let Some(bytes) = record.original() {
                        writer.write_all(bytes).map_err(pipe_error)?;
                    } else {
                        let mut selected = layout.iter().enumerate().peekable();
                        for field in 1..=record.fields {
                            if field > 1 {
                                writer.write_all(b"\t").map_err(pipe_error)?;
                            }
                            if selected.peek().is_some_and(|(_, number)| **number == field) {
                                let (at, _) = selected.next().unwrap();
                                writer
                                    .write_all(record.segment(record.key_count() + at))
                                    .map_err(pipe_error)?;
                            } else if record.fields == 1 {
                                // An empty byte string has zero fields. Keep
                                // one unselected field present for diagnostics.
                                writer.write_all(b"_").map_err(pipe_error)?;
                            }
                        }
                    }
                    writer.write_all(&[options.record_end]).map_err(pipe_error)
                })?;
                writer.into_inner().map_err(pipe_error)?;
                Ok(())
            });
        let Ok(merge) = merge else {
            return Ok(None);
        };
        if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
            let _ = crate::standard_io::Stderr.write_all(b"sort calculation: native merge pipe\n");
        }
        let mut intake = Intake::new(options, intake::Header::None)
            .with_filter(intake::Filter::None)
            .after(u64::from(options.header_in));
        let mut calculation = grouping::Calculation::new(keys, options);
        let result = (|| {
            while intake.next(&mut reader, spare.bytes_mut())?.is_some() {
                let line = intake.line();
                if line == 1 && options.header_out && !options.header_in {
                    context
                        .output
                        .first(spare.bytes(), context.operations, keys)?;
                }
                calculation.push_in_place(
                    &mut spare,
                    || grouping::Raw::for_keys(keys.len(), index.fresh()),
                    line,
                    context,
                )?;
            }
            let line = intake.line();
            intake.finish().map(|()| line)
        })();
        // A calculation error must unblock and join the merge before returning.
        drop(reader);
        let merged = merge
            .join()
            .map_err(|_| unsupported("native sort merge thread failed"))?;
        let line = result?;
        merged?;
        context.random = None;
        calculation.finish(line, context).map(Some)
    })
}
