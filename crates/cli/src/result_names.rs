//! Supplied labels for selected Operation-produced output columns.

use super::{Failure, command_memory::reserve, failure, grammar::Mode, unsupported};

#[derive(Default)]
pub(super) struct ResultNames {
    // Sorted by the zero-based output position, independently of argument order.
    entries: Vec<(usize, Vec<u8>)>,
}

impl ResultNames {
    pub(super) fn add(&mut self, argument: &[u8]) -> Result<(), Failure> {
        let invalid = || failure(b"--result-name requires a positive INDEX:NAME\n".to_vec());
        let colon = argument
            .iter()
            .position(|&byte| byte == b':')
            .ok_or_else(invalid)?;
        let (digits, name) = (&argument[..colon], &argument[colon + 1..]);
        if digits.is_empty() || name.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return Err(invalid());
        }
        let index = digits
            .iter()
            .try_fold(0usize, |index, &digit| {
                index
                    .checked_mul(10)?
                    .checked_add(usize::from(digit - b'0'))
            })
            .and_then(|index| index.checked_sub(1))
            .ok_or_else(invalid)?;
        let at = self
            .entries
            .binary_search_by_key(&index, |&(index, _)| index)
            .map_or_else(Ok, |_| {
                Err(failure(b"--result-name target is repeated\n".to_vec()))
            })?;
        let mut bytes = Vec::new();
        reserve(&mut bytes, name.len())?;
        bytes.extend_from_slice(name);
        reserve(&mut self.entries, 1)?;
        self.entries.insert(at, (index, bytes));
        Ok(())
    }

    pub(super) fn get(&self, index: usize) -> Option<&[u8]> {
        self.entries
            .binary_search_by_key(&index, |&(index, _)| index)
            .ok()
            .map(|at| self.entries[at].1.as_slice())
    }

    pub(super) fn validate(
        &self,
        mode: Mode,
        explicit_header_out: bool,
        results: usize,
        delimiter: u8,
        record_end: u8,
        csv: bool,
    ) -> Result<(), Failure> {
        if self.entries.is_empty() {
            return Ok(());
        }
        if !matches!(mode, Mode::Aggregate | Mode::Line) {
            return Err(unsupported(
                "--result-name is supported only for aggregate and per-row operations",
            ));
        }
        if !explicit_header_out {
            return Err(failure(
                b"--result-name requires --header-out or -H\n".to_vec(),
            ));
        }
        if self
            .entries
            .last()
            .is_some_and(|&(index, _)| index >= results)
        {
            return Err(failure(
                b"--result-name target exceeds the number of results\n".to_vec(),
            ));
        }
        if !csv
            && self
                .entries
                .iter()
                .any(|(_, name)| name.contains(&delimiter) || name.contains(&record_end))
        {
            return Err(failure(
                b"--result-name contains the text output delimiter or record terminator\n".to_vec(),
            ));
        }
        Ok(())
    }
}
