//! The single deterministic `SplitMix64` test stream, shared by every suite
//! that needs injected randomness.
//!
//! It lives in its own leaf module so both inclusion shapes reach it without a
//! second textual copy of the algorithm (the `TEST-RNG-DUP` debt): the movement
//! suite gets it re-exported as `common::TestRng` from [`super`], and the paper
//! host includes this file directly with `#[path]`. The constants and byte
//! order are the exact ones the determinism pins depend on — never re-derive
//! them.

use rand_core::RngCore;

/// Deterministic `SplitMix64` — the exact stream the simulation suites replay,
/// seeded once and threaded in a fixed order so a run reproduces bit-for-bit.
pub struct TestRng {
    state: u64,
}

impl TestRng {
    /// Seeds the stream.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl RngCore for TestRng {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_u32(&mut self) -> u32 {
        let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
        u32::from_le_bytes([b0, b1, b2, b3])
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        for chunk in dst.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                *slot = *byte;
            }
        }
    }
}
