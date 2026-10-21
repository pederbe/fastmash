//! A bounded handoff between final run merging and ordered calculation.
#![forbid(unsafe_code)]

use super::{Failure, allocation};
use std::sync::{Condvar, Mutex};

struct Batch<T> {
    records: Vec<T>,
    heap: usize,
}
impl<T> Batch<T> {
    fn new(count: usize) -> Result<Self, Failure> {
        let mut records = Vec::new();
        records.try_reserve_exact(count).map_err(|_| allocation())?;
        Ok(Self { records, heap: 0 })
    }
    fn metadata(&self) -> usize {
        self.records.capacity() * std::mem::size_of::<T>()
    }
}

struct State<T> {
    ready: Option<Batch<T>>,
    spare: Option<Batch<T>>,
    busy: bool,
    closed: bool,
    cancelled: bool,
}
struct Shared<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
}
impl<T> Shared<T> {
    fn send(&self, batch: &mut Batch<T>) -> bool {
        let mut state = self.state.lock().expect("handoff state");
        while state.ready.is_some() && !state.cancelled {
            state = self.changed.wait(state).expect("handoff state");
        }
        if state.cancelled {
            return false;
        }
        let spare = state.spare.take().expect("handoff spare");
        state.ready = Some(std::mem::replace(batch, spare));
        self.changed.notify_all();
        true
    }
    fn receive(&self, batch: &mut Batch<T>) -> bool {
        let mut state = self.state.lock().expect("handoff state");
        while state.ready.is_none() && !state.closed {
            state = self.changed.wait(state).expect("handoff state");
        }
        let Some(ready) = state.ready.take() else {
            return false;
        };
        debug_assert!(state.spare.is_none());
        state.spare = Some(std::mem::replace(batch, ready));
        state.busy = true;
        self.changed.notify_all();
        true
    }
    fn complete(&self) {
        let mut state = self.state.lock().expect("handoff state");
        state.busy = false;
        self.changed.notify_all();
    }
    fn idle(&self) -> bool {
        let mut state = self.state.lock().expect("handoff state");
        while (state.ready.is_some() || state.busy) && !state.cancelled {
            state = self.changed.wait(state).expect("handoff state");
        }
        !state.cancelled
    }
    fn close(&self, cancelled: bool) {
        let mut state = self.state.lock().expect("handoff state");
        state.closed = true;
        state.cancelled |= cancelled;
        self.changed.notify_all();
    }
}

pub(super) struct Sender<'a, T> {
    shared: &'a Shared<T>,
    batch: Batch<T>,
    budget: usize,
    count: usize,
}
impl<T> Sender<'_, T> {
    fn flush(&mut self) -> bool {
        self.batch.records.is_empty() || self.shared.send(&mut self.batch)
    }
    /// `heap` counts the item's allocation beyond its value in the batch.
    pub(super) fn push(&mut self, item: T, heap: usize) -> Result<(), Failure> {
        let spare = self
            .budget
            .saturating_sub(self.batch.metadata())
            .saturating_sub(self.batch.heap);
        if !self.batch.records.is_empty()
            && (self.batch.records.len() == self.count || heap > spare)
            && !self.flush()
        {
            return Err(cancelled());
        }
        self.batch.heap = self.batch.heap.checked_add(heap).ok_or_else(allocation)?;
        self.batch.records.push(item);
        // An oversized record remains permitted, as in the serial merge, but
        // cannot accumulate with another oversized queued record. Wait for
        // calculation to finish with it before decoding the next record.
        if self.batch.metadata().saturating_add(self.batch.heap) > self.budget
            && (!self.flush() || !self.shared.idle())
        {
            return Err(cancelled());
        }
        Ok(())
    }
}

// Only the caller's failed consumer can cancel. Its actual failure takes
// precedence over this internal marker, which allocates no diagnostic.
fn cancelled() -> Failure {
    Failure {
        status: 77,
        message: Vec::new(),
    }
}

/// Runs the producer on one scoped thread and consumes its records here.
/// Returns false without invoking either callback if the budget is too small
/// or the thread cannot start, so the caller can run its serial path.
/// Three checked batches share `budget`; an individual oversized item keeps
/// the serial path's exception and is acknowledged before production resumes.
pub(super) fn run<T: Send>(
    budget: usize,
    produce: impl FnOnce(&mut Sender<'_, T>) -> Result<(), Failure> + Send,
    mut consume: impl FnMut(T) -> Result<(), Failure>,
) -> Result<bool, Failure> {
    let per_batch = budget / 3;
    let count = per_batch / 2 / std::mem::size_of::<T>().max(1);
    if count == 0 {
        return Ok(false);
    }
    let first = Batch::new(count)?;
    let second = Batch::new(count)?;
    let mut consumed = Batch::new(count)?;
    let shared = Shared {
        state: Mutex::new(State {
            ready: None,
            spare: Some(second),
            busy: false,
            closed: false,
            cancelled: false,
        }),
        changed: Condvar::new(),
    };
    std::thread::scope(|scope| {
        let shared = &shared;
        let worker = std::thread::Builder::new().spawn_scoped(scope, move || {
            let mut sender = Sender {
                shared,
                batch: first,
                budget: per_batch,
                count,
            };
            let result = produce(&mut sender);
            // Deliver every preceding record before reporting a producer
            // failure, including a partially filled final batch.
            sender.flush();
            shared.close(false);
            result
        });
        let Ok(worker) = worker else {
            return Ok(false);
        };
        while shared.receive(&mut consumed) {
            for item in consumed.records.drain(..) {
                if let Err(error) = consume(item) {
                    shared.close(true);
                    let _ = worker.join().expect("sort merge thread");
                    return Err(error);
                }
            }
            consumed.heap = 0;
            shared.complete();
        }
        worker.join().expect("sort merge thread")?;
        Ok(true)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn error(message: &[u8]) -> Failure {
        Failure {
            status: 1,
            message: message.to_vec(),
        }
    }

    #[test]
    fn handoff_preserves_every_record_and_its_order() {
        let mut values = Vec::new();
        assert!(
            run(
                4096,
                |sender| {
                    for value in 0..10000u64 {
                        sender.push(value, 0)?;
                    }
                    Ok(())
                },
                |value| {
                    values.push(value);
                    Ok(())
                }
            )
            .ok()
            .unwrap()
        );
        assert!(values.into_iter().eq(0..10000));
    }

    #[test]
    fn handoff_drains_a_partial_batch_before_producer_failure() {
        let mut values = Vec::new();
        let result = run(
            4096,
            |sender| {
                for value in 0..37u64 {
                    sender.push(value, 0)?;
                }
                Err(error(b"producer"))
            },
            |value| {
                values.push(value);
                Ok(())
            },
        );
        assert_eq!(result.err().unwrap().message, b"producer");
        assert!(values.into_iter().eq(0..37));
    }

    #[test]
    fn handoff_consumer_failure_stops_bounded_production_and_wins() {
        let produced = AtomicUsize::new(0);
        let result = run(
            4096,
            |sender| {
                for value in 0..10000u64 {
                    produced.fetch_add(1, Ordering::Relaxed);
                    sender.push(value, 0)?;
                }
                Err(error(b"producer"))
            },
            |_| Err(error(b"consumer")),
        );
        assert_eq!(result.err().unwrap().message, b"consumer");
        let batch = 4096 / 3 / 2 / std::mem::size_of::<u64>();
        assert!(produced.load(Ordering::Relaxed) <= 3 * batch + 1);
        // A producer that already failed must still report the earlier
        // record's calculation failure first.
        let result = run(
            4096,
            |sender| {
                sender.push(1u64, 0)?;
                Err(error(b"producer"))
            },
            |_| Err(error(b"consumer")),
        );
        assert_eq!(result.err().unwrap().message, b"consumer");
    }

    #[test]
    fn handoff_acknowledges_each_oversized_record_before_resuming() {
        let acknowledged = AtomicUsize::new(0);
        assert!(
            run(
                4096,
                |sender| {
                    for index in 0..12usize {
                        let record = vec![index as u8; 4096];
                        let heap = record.capacity();
                        sender.push(record, heap)?;
                        assert_eq!(acknowledged.load(Ordering::Acquire), index + 1);
                    }
                    Ok(())
                },
                |record: Vec<u8>| {
                    let index = acknowledged.load(Ordering::Relaxed);
                    assert_eq!(record, vec![index as u8; 4096]);
                    acknowledged.store(index + 1, Ordering::Release);
                    Ok(())
                }
            )
            .ok()
            .unwrap()
        );
        assert_eq!(acknowledged.load(Ordering::Relaxed), 12);
    }

    #[test]
    fn handoff_with_no_batch_room_leaves_both_callbacks_untouched() {
        assert!(
            !run::<u64>(1, |_| panic!("producer"), |_| panic!("consumer"))
                .ok()
                .unwrap()
        );
    }
}
