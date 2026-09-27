//! Bounded invocation-local reuse of exact successful logarithms.
use super::numerics::Value;
use fastmash_numeric_contract::Value80;

// Measurements compared 16/64/256/1024 entries. 1024 occupies 32 KiB and removes
// nearly all repeated-wine log work; unique inputs retain a constant-time miss.
// This is a replaceable cache policy, never an input limit.
const ENTRIES: usize = 1024;
#[derive(Clone, Copy)]
struct Entry {
    input: u128,
    output: Value,
}

pub(super) struct LogCache {
    entries: Vec<Entry>,
}
impl LogCache {
    pub fn new() -> Self {
        let mut entries = Vec::new();
        // This is an optional optimization. Allocation failure keeps computation
        // on the original path instead of adding a new command failure.
        if entries.try_reserve_exact(ENTRIES).is_ok() {
            entries.resize(
                ENTRIES,
                Entry {
                    input: 0,
                    output: Value::try_from(Value80::signed_zero(false)).expect("canonical zero"),
                },
            );
        }
        Self { entries }
    }
    fn index(bits: u128) -> usize {
        let mut hash = (bits as u64) ^ ((bits >> 64) as u64).rotate_left(17);
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(0xbf58476d1ce4e5b9);
        hash ^= hash >> 27;
        hash = hash.wrapping_mul(0x94d049bb133111eb);
        hash ^= hash >> 31;
        (hash as usize) & (ENTRIES - 1)
    }
    pub fn get(&self, bits: u128) -> Option<Value> {
        let entry = self.entries.get(Self::index(bits))?;
        (entry.input == bits).then_some(entry.output)
    }
    pub fn insert(&mut self, bits: u128, output: Value) {
        if let Some(entry) = self.entries.get_mut(Self::index(bits)) {
            *entry = Entry {
                input: bits,
                output,
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn collisions_replace_only_their_exact_key_and_disabled_cache_is_optional() {
        let mut cache = LogCache::new();
        let first = (16383u128 << 64) | (1 << 63);
        let second = (first + 1..)
            .find(|&b| LogCache::index(b) == LogCache::index(first))
            .unwrap();
        let value = Value::try_from(Value80::exact_u64(7)).unwrap();
        cache.insert(first, value);
        assert!(cache.get(first).is_some());
        assert!(cache.get(second).is_none());
        cache.insert(second, value);
        assert!(cache.get(first).is_none());
        assert_eq!(
            Value80::from(cache.get(second).unwrap()).raw(),
            Value80::exact_u64(7).raw()
        );
        let mut disabled = LogCache {
            entries: Vec::new(),
        };
        disabled.insert(first, value);
        assert!(disabled.get(first).is_none());
        assert_eq!(std::mem::size_of::<Entry>() * ENTRIES, 32768);
    }
}
