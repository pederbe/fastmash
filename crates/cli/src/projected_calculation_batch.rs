//! Overlap Spill merging and ordered calculation without a text round trip.
#![forbid(unsafe_code)]
use super::grouping::GroupRecord;
use super::operation_set::Fields;
use super::storage::Mixed;
use super::{
    Failure, Record, Selector, Sorted, allocation, grouping, numerics, operation_set, records,
    spill, unsupported,
};
use std::io::Write;
use std::sync::{Condvar, Mutex};

const BUFFER: usize = 64 << 10;
pub(super) const MEMORY: usize = 3 * BUFFER;
const RECORDS: usize = 256;

type Numbers = [(u64, Option<numerics::Value>); 2];
struct Prepared<'a, S: spill::Codec> {
    record: Record<Mixed<'a, S>>,
    numbers: Numbers,
}
struct Batch<'a, S: spill::Codec> {
    records: Vec<Prepared<'a, S>>,
    heap: usize,
}
impl<'a, S: spill::Codec> Batch<'a, S> {
    fn new() -> Result<Self, Failure> {
        let mut records = Vec::new();
        assert!(RECORDS * std::mem::size_of::<Prepared<'a, S>>() <= BUFFER);
        records
            .try_reserve_exact(RECORDS)
            .map_err(|_| allocation())?;
        Ok(Self { records, heap: 0 })
    }
    fn full(&self, heap: usize) -> bool {
        self.records.len() == self.records.capacity()
            || (!self.records.is_empty()
                && self.heap.saturating_add(heap).saturating_add(
                    self.records.capacity() * std::mem::size_of::<Prepared<'a, S>>(),
                ) > BUFFER)
    }
}

// One waiting batch, one being filled and one being consumed. A single
// oversized record travels alone, as it does in the serial merge.
struct Exchange<'a, S: spill::Codec> {
    slot: Mutex<State<'a, S>>,
    changed: Condvar,
}
struct State<'a, S: spill::Codec> {
    batch: Option<Batch<'a, S>>,
    done: bool,
    cancelled: bool,
}
impl<'a, S: spill::Codec> Exchange<'a, S> {
    fn send(&self, batch: Batch<'a, S>) -> Result<(), Failure> {
        let mut state = self
            .slot
            .lock()
            .map_err(|_| unsupported("native sort batch lock failed"))?;
        while state.batch.is_some() && !state.cancelled {
            state = self
                .changed
                .wait(state)
                .map_err(|_| unsupported("native sort batch lock failed"))?;
        }
        if state.cancelled {
            return Err(unsupported("native sort calculation cancelled"));
        }
        state.batch = Some(batch);
        self.changed.notify_all();
        Ok(())
    }
    fn receive(&self) -> Result<Option<Batch<'a, S>>, Failure> {
        let mut state = self
            .slot
            .lock()
            .map_err(|_| unsupported("native sort batch lock failed"))?;
        while state.batch.is_none() && !state.done {
            state = self
                .changed
                .wait(state)
                .map_err(|_| unsupported("native sort batch lock failed"))?;
        }
        let batch = state.batch.take();
        self.changed.notify_all();
        Ok(batch)
    }
}
struct Completion<'a, 'b, S: spill::Codec>(&'b Exchange<'a, S>, bool);
impl<S: spill::Codec> Drop for Completion<'_, '_, S> {
    fn drop(&mut self) {
        let mut state = self
            .0
            .slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.1 {
            state.cancelled = true;
        } else {
            state.done = true;
        }
        self.0.changed.notify_all();
    }
}

struct Cached<'a, F> {
    fields: F,
    numbers: &'a Numbers,
}
impl<'r, F: Fields<'r>> Fields<'r> for Cached<'_, F> {
    fn text(
        &mut self,
        field: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<&'r [u8], Failure> {
        self.fields.text(field, line, options)
    }
    fn number(
        &mut self,
        field: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<Option<numerics::Value>, Failure> {
        if let Some(value) = self
            .numbers
            .iter()
            .find(|(number, _)| *number == field)
            .and_then(|(_, value)| *value)
        {
            return Ok(Some(value));
        }
        self.fields.number(field, line, options)
    }
}
struct Calculated<'a, S: super::Storage> {
    sorted: Sorted<'a, S>,
    numbers: Numbers,
}
impl<S: super::Storage> GroupRecord for Calculated<'_, S> {
    fn field(
        &mut self,
        field: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<&[u8], Failure> {
        self.sorted.field(field, line, options)
    }
    fn field_pair(
        &mut self,
        a: u64,
        b: u64,
        line: u64,
        options: &super::options::Options,
    ) -> Result<(&[u8], &[u8]), Failure> {
        self.sorted.field_pair(a, b, line, options)
    }
    fn same_group(
        &mut self,
        previous: &mut Self,
        keys: &[u64],
        line: u64,
        options: &super::options::Options,
    ) -> Result<bool, Failure> {
        self.sorted
            .same_group(&mut previous.sorted, keys, line, options)
    }
    fn original(&self) -> Option<&[u8]> {
        self.sorted.original()
    }
    fn collect(
        &mut self,
        operations: &mut super::OperationSet,
        line: u64,
        options: &super::options::Options,
        arithmetic: &mut numerics::Numerics,
        random: Option<&mut super::random::RandomState>,
    ) -> Result<bool, Failure> {
        if let Some(record) = self.sorted.record.original() {
            operations.collect_fields(
                &mut Cached {
                    fields: operation_set::Whole {
                        record,
                        index: &mut self.sorted.fields,
                    },
                    numbers: &self.numbers,
                },
                false,
                line,
                options,
                arithmetic,
                random,
            )
        } else {
            operations.collect_fields(
                &mut Cached {
                    fields: super::Projected {
                        record: &self.sorted.record,
                        layout: self.sorted.layout,
                        crossings: self.sorted.crossings,
                        separator: Some(b'\t'),
                    },
                    numbers: &self.numbers,
                },
                self.sorted.texts,
                line,
                options,
                arithmetic,
                random,
            )
        }
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub(super) fn calculate<S: spill::Codec, W: Write>(
    sorted: &mut spill::Finished<S>,
    layout: &[u64],
    context: &mut grouping::Context<'_, super::command_output::Results<'_, W>>,
) -> Result<Option<()>, Failure> {
    let (options, keys) = (context.options, context.keys);
    let width = context
        .operations
        .requests()
        .filter(|(kind, selector)| {
            kind.converts_numeric_field() || matches!(selector, Selector::Pair { .. })
        })
        .take(2)
        .count()
        .max(1);
    let mut fields = [0; 2];
    let mut count = 0;
    'fields: for (kind, selector) in context.operations.requests() {
        if !kind.converts_numeric_field() && !matches!(selector, Selector::Pair { .. }) {
            continue;
        }
        let selected = match selector {
            Selector::Single(field) => [Some(field), None],
            Selector::Pair { left, right } if width == 1 => [Some(left.max(right)), None],
            Selector::Pair { left, right } => [Some(left), Some(right)],
        };
        for field in selected.into_iter().flatten() {
            if !fields[..count].contains(&field) {
                fields[count] = field;
                count += 1;
                if count == width {
                    break 'fields;
                }
            }
        }
    }
    let exchange = Exchange {
        slot: Mutex::new(State {
            batch: None,
            done: false,
            cancelled: false,
        }),
        changed: Condvar::new(),
    };
    let mut batch = Batch::new()?;
    let crossings = super::Crossings::default();
    let texts = context.operations.native_sort() == operation_set::NativeSort::Packed;
    std::thread::scope(|scope| {
        let shared = &exchange;
        let crossings = &crossings;
        let merge = std::thread::Builder::new()
            .name("sort-merge".into())
            .spawn_scoped(scope, move || {
                let _complete = Completion(shared, false);
                sorted.emit(|record| {
                    let heap = match &record.data {
                        Mixed::Owned(storage) => storage.segments().heap_bytes(),
                        Mixed::View(_) => 0,
                    };
                    if batch.full(heap) {
                        shared.send(std::mem::replace(
                            &mut batch,
                            Batch {
                                records: Vec::new(),
                                heap: 0,
                            },
                        ))?;
                        batch = Batch::new()?;
                    }
                    let mut whole_index = records::FieldIndex::default();
                    let mut projected = super::Projected {
                        record: &record,
                        layout,
                        crossings,
                        separator: Some(b'\t'),
                    };
                    let numbers = fields.map(|field| {
                        (
                            field,
                            (field != 0)
                                .then(|| {
                                    if let Some(bytes) = record.original() {
                                        operation_set::Whole {
                                            record: bytes,
                                            index: &mut whole_index,
                                        }
                                        .number(field, 0, options)
                                    } else {
                                        projected.number(field, 0, options)
                                    }
                                    .ok()
                                    .flatten()
                                })
                                .flatten(),
                        )
                    });
                    batch.heap = batch.heap.saturating_add(heap);
                    batch.records.push(Prepared { record, numbers });
                    Ok(())
                })?;
                if !batch.records.is_empty() {
                    shared.send(batch)?;
                }
                Ok(())
            });
        let Ok(merge) = merge else {
            return Ok(None);
        };
        if std::env::var_os("FASTMASH_SORT_TRACE").is_some() {
            let _ =
                crate::standard_io::Stderr.write_all(b"sort calculation: native merge batches\n");
        }
        let cancel = Completion(&exchange, true);
        let mut calculation = grouping::Calculation::new(keys, options);
        let mut line = u64::from(options.header_in);
        let mut spare = None;
        let index = records::FieldIndex::selecting(layout.iter().copied());
        let result = (|| {
            while let Some(batch) = exchange.receive()? {
                for prepared in batch.records {
                    line = line.checked_add(1).ok_or_else(allocation)?;
                    if line == 1 && options.header_out && !options.header_in {
                        super::generated_header(
                            &prepared.record,
                            layout,
                            keys,
                            context.operations,
                            options,
                            context.output,
                        )?;
                    }
                    let mut fields = spare.take().unwrap_or_else(|| index.fresh());
                    fields.clear();
                    let record = Calculated {
                        sorted: Sorted {
                            record: prepared.record,
                            layout,
                            fields,
                            crossings,
                            texts,
                        },
                        numbers: prepared.numbers,
                    };
                    if let Some(unused) = calculation.push(record, line, context)? {
                        spare = Some(unused.sorted.fields);
                    }
                }
            }
            Ok(())
        })();
        drop(cancel);
        let merged = merge
            .join()
            .map_err(|_| unsupported("native sort merge thread failed"))?;
        result?;
        merged?;
        context.random = None;
        calculation.finish(line, context).map(Some)
    })
}
