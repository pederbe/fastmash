//! Invocation-owned glibc-compatible random state.

pub(super) trait SeedSource {
    fn seed(&mut self) -> Result<u32, ()>;
}

pub(super) struct OsSeedSource;

const GETRANDOM: usize = 318;
const EINTR: isize = -4;

impl SeedSource for OsSeedSource {
    /// Four bytes from the kernel's `getrandom`, blocking like GNU until the
    /// pool is ready. Any failure, including a seccomp-denied syscall, is an
    /// error: there is no weaker fallback source.
    fn seed(&mut self) -> Result<u32, ()> {
        let mut bytes = [0u8; 4];
        let mut filled = 0;
        while filled < bytes.len() {
            let rest = &mut bytes[filled..];
            // SAFETY: the kernel writes at most `rest.len()` bytes to `rest`.
            let result = unsafe {
                crate::linux::syscall(GETRANDOM, rest.as_mut_ptr() as usize, rest.len(), 0, 0)
            };
            match result {
                EINTR => {}
                read @ 1.. => filled += read as usize,
                _ => return Err(()),
            }
        }
        Ok(u32::from_ne_bytes(bytes))
    }
}

pub(super) struct RandomState {
    words: [u32; 31],
    front: usize,
    rear: usize,
}

impl RandomState {
    pub(super) fn initialize(
        explicit: Option<u32>,
        source: &mut impl SeedSource,
    ) -> Result<Self, ()> {
        let seed = match explicit {
            Some(seed) => seed,
            None => source.seed()?,
        };
        Ok(Self::seeded(seed))
    }

    fn seeded(seed: u32) -> Self {
        let seed = if seed == 0 { 1 } else { seed };
        let mut words = [0; 31];
        words[0] = seed;
        let mut word = i64::from(seed as i32);
        for slot in &mut words[1..] {
            let high = word / 127_773;
            let low = word % 127_773;
            word = 16_807 * low - 2_836 * high;
            if word < 0 {
                word += 2_147_483_647;
            }
            *slot = word as u32;
        }
        let mut state = Self {
            words,
            front: 3,
            rear: 0,
        };
        for _ in 0..310 {
            state.draw();
        }
        state
    }

    pub(super) fn draw(&mut self) -> u32 {
        let value = self.words[self.front].wrapping_add(self.words[self.rear]);
        self.words[self.front] = value;
        self.front += 1;
        if self.front == self.words.len() {
            self.front = 0;
        }
        self.rear += 1;
        if self.rear == self.words.len() {
            self.rear = 0;
        }
        value >> 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_seed_source_reads_four_bytes() {
        let seeds: Vec<u32> = (0..4).map(|_| OsSeedSource.seed().unwrap()).collect();
        // Four equal 32-bit seeds would mean the bytes were never written.
        assert!(seeds.windows(2).any(|pair| pair[0] != pair[1]));
    }

    struct Fixed(u32);
    impl SeedSource for Fixed {
        fn seed(&mut self) -> Result<u32, ()> {
            Ok(self.0)
        }
    }

    struct Fail;
    impl SeedSource for Fail {
        fn seed(&mut self) -> Result<u32, ()> {
            Err(())
        }
    }

    struct Panic;
    impl SeedSource for Panic {
        fn seed(&mut self) -> Result<u32, ()> {
            panic!("explicit seeds must not acquire entropy")
        }
    }

    #[test]
    fn fixed_vectors_match_the_qualified_glibc_streams() {
        for (seed, expected) in [
            (
                0,
                [
                    1_804_289_383,
                    846_930_886,
                    1_681_692_777,
                    1_714_636_915,
                    1_957_747_793,
                    424_238_335,
                    719_885_386,
                    1_649_760_492,
                ],
            ),
            (
                7,
                [
                    1_045_618_677,
                    1_863_967_299,
                    1_272_579_899,
                    461_085_871,
                    21_961_325,
                    1_105_564_443,
                    2_138_782_586,
                    68_574_097,
                ],
            ),
            (
                u32::MAX,
                [
                    254_925_627,
                    1_205_188_300,
                    366_127_624,
                    1_401_405_153,
                    76_053_476,
                    1_604_170_158,
                    1_302_235_366,
                    362_229_243,
                ],
            ),
        ] {
            let mut state = RandomState::seeded(seed);
            assert_eq!(expected.map(|_| state.draw()), expected);
        }
        let mut zero = RandomState::seeded(0);
        let mut one = RandomState::seeded(1);
        assert_eq!(zero.draw(), one.draw());
    }

    #[test]
    fn initialization_uses_entropy_only_without_an_explicit_seed() {
        let mut fixed = Fixed(7);
        let mut from_source = RandomState::initialize(None, &mut fixed).unwrap();
        assert_eq!(from_source.draw(), 1_045_618_677);
        assert!(RandomState::initialize(None, &mut Fail).is_err());
        let mut explicit = RandomState::initialize(Some(7), &mut Panic).unwrap();
        assert_eq!(explicit.draw(), 1_045_618_677);
    }

    #[test]
    fn one_state_continues_instead_of_reseeding_at_group_boundaries() {
        let mut continuous = RandomState::seeded(0);
        let first_group: Vec<_> = (0..3).map(|_| continuous.draw()).collect();
        let second_group: Vec<_> = (0..3).map(|_| continuous.draw()).collect();
        let mut restarted = RandomState::seeded(0);
        let restarted_group: Vec<_> = (0..3).map(|_| restarted.draw()).collect();
        assert_ne!(second_group, restarted_group);
        assert_eq!(first_group, restarted_group);
    }
}
