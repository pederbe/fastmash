//! Sparse completed cells, independent of input grouping and dense output shape.
use super::{Failure, headers::output::Buffered, options::Options, unsupported};
use std::{collections::HashMap, io::Write};

fn allocation() -> Failure {
    unsupported("crosstab memory allocation failed")
}
#[cfg(test)]
thread_local! { static FAIL_AFTER: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) }; }
fn allocation_point() -> Result<(), Failure> {
    #[cfg(test)]
    if FAIL_AFTER.with(|budget| match budget.get() {
        Some(0) => true,
        Some(n) => {
            budget.set(Some(n - 1));
            false
        }
        None => false,
    }) {
        return Err(allocation());
    }
    Ok(())
}
fn copy(bytes: &[u8]) -> Result<Vec<u8>, Failure> {
    let mut result = Vec::new();
    allocation_point()?;
    result
        .try_reserve_exact(bytes.len())
        .map_err(|_| allocation())?;
    result.extend_from_slice(bytes);
    Ok(result)
}
fn label(bytes: &[u8]) -> &[u8] {
    // Preserve GNU's C-string label identity, but not its lossy 511-byte cap.
    &bytes[..bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len())]
}
fn intern(labels: &mut HashMap<Vec<u8>, usize>, bytes: &[u8]) -> Result<usize, Failure> {
    let bytes = label(bytes);
    if let Some(&id) = labels.get(bytes) {
        return Ok(id);
    }
    let owned = copy(bytes)?;
    allocation_point()?;
    labels.try_reserve(1).map_err(|_| allocation())?;
    let id = labels.len();
    labels.insert(owned, id);
    Ok(id)
}
fn ordered(labels: &HashMap<Vec<u8>, usize>) -> Result<Vec<(&Vec<u8>, &usize)>, Failure> {
    let mut order = Vec::new();
    allocation_point()?;
    order
        .try_reserve_exact(labels.len())
        .map_err(|_| allocation())?;
    order.extend(labels.iter());
    order.sort_unstable_by(|a, b| a.0.cmp(b.0));
    Ok(order)
}

#[derive(Default)]
pub(super) struct Table {
    rows: HashMap<Vec<u8>, usize>,
    columns: HashMap<Vec<u8>, usize>,
    cells: HashMap<(usize, usize), Vec<u8>>,
}
impl Table {
    pub fn insert(&mut self, row: &[u8], column: &[u8], value: &[u8]) -> Result<(), Failure> {
        let key = (
            intern(&mut self.rows, row)?,
            intern(&mut self.columns, column)?,
        );
        if !self.cells.contains_key(&key) {
            let value = copy(label(value))?;
            allocation_point()?;
            self.cells.try_reserve(1).map_err(|_| allocation())?;
            self.cells.insert(key, value);
        }
        Ok(())
    }
    pub fn write<W: Write>(
        &self,
        output: &mut Buffered<'_, W>,
        options: &Options,
    ) -> Result<(), Failure> {
        // Prepare both orders before emitting anything. Never allocate rows * columns.
        let rows = ordered(&self.rows)?;
        let columns = ordered(&self.columns)?;
        for (name, _) in &columns {
            output.raw(&[options.output]);
            output.formatted(name);
        }
        output.raw(&[options.record_end]);
        for (name, &row) in rows {
            output.formatted(name);
            for &(_, &column) in &columns {
                output.raw(&[options.output]);
                output.formatted(
                    self.cells
                        .get(&(row, column))
                        .map_or(options.filler.as_ref(), Vec::as_slice),
                );
            }
            output.raw(&[options.record_end]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allocation_failures_abort_collection_or_render_before_table_output() {
        for step in 0..6 {
            FAIL_AFTER.with(|b| b.set(Some(step)));
            let mut table = Table::default();
            let error = table
                .insert(b"r", b"c", b"1")
                .expect_err("injected allocation failure");
            assert_eq!(error.status, 77);
            assert!(table.cells.is_empty());
        }
        FAIL_AFTER.with(|b| b.set(None));
        let mut table = Table::default();
        assert!(table.insert(b"r", b"c", b"1").is_ok());
        assert!(table.insert(b"r", b"c", b"2").is_ok());
        assert_eq!(table.cells[&(0, 0)], b"1");
        let super::super::options::Action::Calculate(options) =
            super::super::options::parse(&["ct".into(), "1,2".into()], b"fastmash", false)
                .ok()
                .unwrap()
        else {
            panic!()
        };
        for step in 0..2 {
            FAIL_AFTER.with(|b| b.set(Some(step)));
            let mut bytes = Vec::new();
            let mut output = Buffered::new(&mut bytes, 1, false).unwrap();
            assert_eq!(table.write(&mut output, &options).err().unwrap().status, 77);
            drop(output);
            assert!(bytes.is_empty());
        }
        FAIL_AFTER.with(|b| b.set(None));
    }
}
