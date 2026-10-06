//! Global Comparison keys retain complete fields and original contribution order.
//! Collection owns only key spelling and the Operations' required state.
use super::super::{
    Failure, binding,
    command_memory::{reserve, reserve_map},
    command_sources, csv_input, failure, intake,
    numerics::{Numerics, Value},
    operation_set::{Fields, RetainedGroup, Whole},
    options, records,
};
use std::{collections::HashMap, io::BufRead};

/// Lexicographic field ordering uses unsigned bytes and shorter prefixes first.
pub(super) type Key = Vec<Vec<u8>>;

pub(super) struct Summary {
    pub key: Key,
    pub spelling: Key,
    pub values: Vec<Value>,
}

pub(super) struct Dataset {
    pub summaries: Vec<Summary>,
    /// Only the selected label source prepares a report header; unused header
    /// positions do not constrain the other source's positional data Fields.
    pub header: Option<Vec<Vec<u8>>>,
}

struct Collected<'a> {
    spelling: Key,
    operations: RetainedGroup<'a>,
}

pub(super) fn context(mut error: Failure, key: &Key, utf8: bool) -> Failure {
    super::super::append(&mut error, &[b"compare: key ("]);
    for (at, field) in key.iter().enumerate() {
        let mut quoted = Vec::new();
        super::super::named_fields::quote_with(field, &mut quoted, utf8);
        if at != 0 {
            super::super::append(&mut error, &[b", "]);
        }
        super::super::append(&mut error, &[&quoted]);
    }
    super::super::append(&mut error, &[b")\n"]);
    error
}

fn extract<'r>(
    fields: &mut impl Fields<'r>,
    requested: &[u64],
    line: u64,
    options: &options::Options,
    normalize: bool,
) -> Result<Key, Failure> {
    let mut key = Vec::new();
    reserve(&mut key, requested.len())?;
    for &field in requested {
        let value = fields.text(field, line, options)?;
        let mut bytes = Vec::new();
        reserve(&mut bytes, value.len())?;
        bytes.extend_from_slice(value);
        if normalize && options.ignore_case {
            bytes.make_ascii_lowercase();
        }
        key.push(bytes);
    }
    Ok(key)
}

#[derive(Default)]
struct Collection<'a> {
    indices: HashMap<Key, usize>,
    groups: Vec<Collected<'a>>,
}

impl<'a> Collection<'a> {
    fn collect<'r>(
        &mut self,
        fields: &mut impl Fields<'r>,
        binding: &'a binding::Binding<'_>,
        line: u64,
        options: &options::Options,
        arithmetic: &mut Numerics,
    ) -> Result<(), Failure> {
        // A key exists even when every Operation omits its selected values.
        let key = extract(fields, &binding.keys, line, options, true).map_err(|mut error| {
            super::super::append(
                &mut error,
                &[format!("compare: extracting key at line {line}\n").as_bytes()],
            );
            error
        })?;
        let at = if let Some(&at) = self.indices.get(&key) {
            at
        } else {
            let create = (|| {
                let spelling = extract(fields, &binding.keys, line, options, false)?;
                let operations = binding.operations.retained_group()?;
                reserve(&mut self.groups, 1)?;
                reserve_map(&mut self.indices, 1)?;
                Ok(Collected {
                    spelling,
                    operations,
                })
            })();
            let group = create.map_err(|error| context(error, &key, options.locale.utf8))?;
            let at = self.groups.len();
            self.groups.push(group);
            self.indices.insert(key, at);
            at
        };
        let group = &mut self.groups[at];
        group
            .operations
            .collect_fields(fields, line, options, arithmetic)
            .map_err(|error| context(error, &group.spelling, options.locale.utf8))?;
        Ok(())
    }

    fn finish(
        mut self,
        options: &options::Options,
        arithmetic: &mut Numerics,
        header: Option<Vec<Vec<u8>>>,
    ) -> Result<Dataset, Failure> {
        let mut completed = Vec::new();
        reserve(&mut completed, self.groups.len())?;
        // Complete in encounter order so an error is independent of hash iteration.
        for group in &mut self.groups {
            let mut group_completion = group.operations.completion();
            let completion = (|| {
                let mut values = Vec::new();
                reserve(&mut values, group_completion.len())?;
                for at in 0..group_completion.len() {
                    let value = group_completion
                        .numerical_result(at, arithmetic, options)
                        .map_err(|mut error| {
                            super::super::append(
                                &mut error,
                                &[format!("compare: completing result {}\n", at + 1).as_bytes()],
                            );
                            error
                        })?;
                    values.push(value);
                }
                Ok(values)
            })();
            completed.push(
                completion.map_err(|error| context(error, &group.spelling, options.locale.utf8))?,
            );
        }
        let mut summaries = Vec::new();
        reserve(&mut summaries, self.groups.len())?;
        for (key, at) in self.indices {
            summaries.push(Summary {
                key,
                spelling: std::mem::take(&mut self.groups[at].spelling),
                values: std::mem::take(&mut completed[at]),
            });
        }
        summaries.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        Ok(Dataset { summaries, header })
    }
}

pub(super) fn scan(
    reader: &mut dyn BufRead,
    binding: &mut binding::Binding<'_>,
    options: &options::Options,
    arithmetic: &mut Numerics,
    report_header: bool,
) -> Result<Dataset, Failure> {
    binding.operations.reset();
    if options.csv_in {
        return scan_decoded(reader, binding, options, arithmetic, report_header);
    }
    let mut input = intake::Intake::new(options, intake::Header::of(options));
    let mut buffer = Vec::new();
    // Bind and prepare the source before any retained Group borrows its plans.
    let prepared = (|| {
        if options.header_in {
            if !input.header(reader, &mut buffer, |record| {
                binding.header(record, options)
            })? {
                return Err(failure(
                    b"compare requires an input header on each source\n".to_vec(),
                ));
            }
        } else {
            binding.numbered()?;
        }
        let mut index = records::FieldIndex::selecting(
            binding
                .operations
                .fields()
                .chain(binding.keys.iter().copied()),
        );
        let header = if report_header {
            Some(super::header(
                binding,
                |field| {
                    if options.header_in {
                        Whole {
                            record: &buffer,
                            index: &mut index,
                        }
                        .text(field, 1, options)
                    } else {
                        Ok(b"".as_slice())
                    }
                },
                options,
            )?)
        } else {
            None
        };
        Ok((index, header))
    })();
    let (mut index, header) = match prepared {
        Ok(prepared) => prepared,
        Err(error) => return command_sources::complete(Err(error), input.finish()),
    };
    let binding = &*binding;
    let mut collection = Collection::default();
    let collected = (|| {
        while let Some(record) = input.next(reader, &mut buffer)? {
            index.clear();
            collection.collect(
                &mut Whole {
                    record: record.data(),
                    index: &mut index,
                },
                binding,
                input.line(),
                options,
                arithmetic,
            )?;
        }
        Ok(())
    })();
    command_sources::complete(collected, input.finish())?;
    collection.finish(options, arithmetic, header)
}

fn scan_decoded(
    reader: &mut dyn BufRead,
    binding: &mut binding::Binding<'_>,
    options: &options::Options,
    arithmetic: &mut Numerics,
    report_header: bool,
) -> Result<Dataset, Failure> {
    let mut input = csv_input::Intake::new();
    let mut record = csv_input::Record::default();
    let prepared = (|| {
        if options.header_in {
            if !input.next(reader, &mut record)? {
                return Err(failure(
                    b"compare requires an input header on each source\n".to_vec(),
                ));
            }
            binding.decoded_header(&record, options)?;
        } else {
            binding.numbered()?;
        }
        if report_header {
            Ok(Some(
                super::header(
                    binding,
                    |field| {
                        if options.header_in {
                            record.field(field)
                        } else {
                            Ok(b"".as_slice())
                        }
                    },
                    options,
                )
                .map_err(|error| record.location.annotate(error))?,
            ))
        } else {
            Ok(None)
        }
    })();
    let header = match prepared {
        Ok(header) => header,
        Err(error) => return command_sources::complete(Err(error), input.finish()),
    };
    let binding = &*binding;
    let mut collection = Collection::default();
    let collected = (|| {
        while input.next(reader, &mut record)? {
            collection
                .collect(
                    &mut csv_input::Fields(record.view()),
                    binding,
                    record.location.record,
                    options,
                    arithmetic,
                )
                .map_err(|error| record.location.annotate(error))?;
        }
        Ok(())
    })();
    command_sources::complete(collected, input.finish())?;
    collection.finish(options, arithmetic, header)
}
