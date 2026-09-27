//! Fallible storage for command parsing and request setup.
use super::{Failure, unsupported};

#[cfg(test)]
thread_local! {
    pub(super) static FAIL_RESERVATION: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

pub(super) fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<(), Failure> {
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
    values
        .try_reserve(additional)
        .map_err(|_| unsupported("command memory allocation failed"))
}
