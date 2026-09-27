//! Compact ragged table storage; missing cells are emitted without storing padding.
use super::{
    Failure, annotated, failure, headers::output::Buffered, options::Options, os_failure, records,
    unsupported,
};
use std::io::{BufRead, Write};

fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<(), Failure> {
    values
        .try_reserve(additional)
        .map_err(|_| unsupported("transpose memory allocation failed"))
}

#[derive(Default)]
struct Table {
    bytes: Vec<u8>,
    field_ends: Vec<usize>,
    row_ends: Vec<usize>,
    columns: usize,
}
impl Table {
    fn push(&mut self, record: &[u8], options: &Options) -> Result<(), Failure> {
        let row_start = self.field_ends.len();
        for span in records::fields(record, options.input) {
            reserve(&mut self.bytes, span.length)?;
            reserve(&mut self.field_ends, 1)?;
            self.bytes
                .extend_from_slice(&record[span.start..span.start + span.length]);
            self.field_ends.push(self.bytes.len());
        }
        let width = self.field_ends.len() - row_start;
        if options.strict && !self.row_ends.is_empty() && width != self.columns {
            let line = self
                .row_ends
                .len()
                .checked_add(1)
                .ok_or_else(|| unsupported("transpose row count overflow"))?;
            return Err(failure(format!("transpose input error: line {line} has {width} fields (previous lines had {});\nsee --help to disable strict mode\n",self.columns).into_bytes()));
        }
        reserve(&mut self.row_ends, 1)?;
        self.row_ends.push(self.field_ends.len());
        self.columns = self.columns.max(width);
        Ok(())
    }

    fn write<W: Write>(&self, output: &mut Buffered<'_, W>, options: &Options) {
        for column in 0..self.columns {
            let mut row_start = 0;
            for (row, &row_end) in self.row_ends.iter().enumerate() {
                if row > 0 {
                    output.raw(&[options.output]);
                }
                // Check width before adding, so ragged dimensions cannot overflow.
                if column < row_end - row_start {
                    let field = row_start + column;
                    let start = if field == 0 {
                        0
                    } else {
                        self.field_ends[field - 1]
                    };
                    output.raw(&self.bytes[start..self.field_ends[field]]);
                } else {
                    output.raw(&options.filler);
                }
                row_start = row_end;
            }
            output.raw(&[options.record_end]);
        }
    }
}

pub(super) fn run<W: Write>(
    reader: &mut impl BufRead,
    output: &mut Buffered<'_, W>,
    options: &Options,
) -> Result<(), Failure> {
    let mut table = Table::default();
    let mut record = Vec::new();
    let read_error = loop {
        match records::read_record_terminated(reader, &mut record, usize::MAX, options.record_end) {
            Ok(false) => break None,
            Ok(true) => (),
            Err(records::ReadError::Io(error)) => break Some(error),
            Err(_) => return Err(unsupported("transpose memory allocation failed")),
        }
        if options.vnlog {
            if !annotated::prepare(&mut record, false)? {
                continue;
            }
        } else if options.skip_comments && records::is_comment(&record) {
            continue;
        }
        table.push(&record, options)?;
    };
    drop(record);
    table.write(output, options);
    read_error.map_or(Ok(()), |error| {
        Err(if error.raw_os_error() == Some(5) {
            failure(b"read error: Input/output error\n".to_vec())
        } else {
            os_failure(&error, true)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn impossible_growth_is_reported_without_allocating() {
        assert!(reserve(&mut vec![1u8], usize::MAX).is_err());
        assert!(reserve(&mut Vec::<usize>::new(), usize::MAX).is_err());
    }
}
