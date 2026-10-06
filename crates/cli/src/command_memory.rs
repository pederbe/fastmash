//! Fallible storage for command parsing and request setup.
use super::{Failure, unsupported};

#[cfg(test)]
thread_local! {
    pub(super) static FAIL_RESERVATION: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

fn admit() -> Result<(), Failure> {
    #[cfg(test)]
    if FAIL_RESERVATION.with(|remaining| match remaining.get() {
        Some(0) => {
            remaining.set(None);
            true
        }
        Some(n) => {
            remaining.set(Some(n - 1));
            false
        }
        None => false,
    }) {
        return Err(unsupported("command memory allocation failed"));
    }
    Ok(())
}

pub(super) fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<(), Failure> {
    admit()?;
    values
        .try_reserve(additional)
        .map_err(|_| unsupported("command memory allocation failed"))
}

/// Checked growth for keyed Command state, under the same refusal policy.
pub(super) fn reserve_map<K: std::hash::Hash + Eq, V>(
    values: &mut std::collections::HashMap<K, V>,
    additional: usize,
) -> Result<(), Failure> {
    admit()?;
    values
        .try_reserve(additional)
        .map_err(|_| unsupported("command memory allocation failed"))
}

/// Exact requested growth where a retention budget also counts spare capacity.
pub(super) fn reserve_exact<T>(values: &mut Vec<T>, additional: usize) -> Result<(), Failure> {
    admit()?;
    values
        .try_reserve_exact(additional)
        .map_err(|_| unsupported("command memory allocation failed"))
}
