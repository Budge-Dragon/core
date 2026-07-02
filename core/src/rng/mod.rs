//! Injected randomness.
//!
//! All randomness enters through [`rand_core::RngCore`], passed in by the host
//! — never a global or thread-local generator. This keeps the simulation
//! deterministic and replayable given a seed. The single sampling primitive is
//! [`uniform_below`]; every domain draw routes through it (via
//! [`crate::services::chance`]), so no `%` reduction of `next_u32` exists
//! anywhere in the crate.

use core::num::NonZeroU32;

use rand_core::RngCore;

/// Unbiased uniform integer in `0..bound.get()`, via widening-multiply with
/// rejection (Lemire). Deterministic given the RNG seed; identical on
/// native/wasm/FFI because it is pure integer arithmetic with no modulo of the
/// raw random word.
#[must_use]
pub fn uniform_below(bound: NonZeroU32, rng: &mut impl RngCore) -> u32 {
    let bound = bound.get();
    let (mut high, mut low) = widening_mul(rng.next_u32(), bound);
    if low < bound {
        // (2^32 mod bound), computed without a 64-bit modulo.
        let threshold = bound.wrapping_neg() % bound;
        while low < threshold {
            let (next_high, next_low) = widening_mul(rng.next_u32(), bound);
            high = next_high;
            low = next_low;
        }
    }
    high
}

/// The high and low 32-bit halves of `x * bound`, extracted through byte
/// decomposition so no truncating `as` cast is needed.
fn widening_mul(x: u32, bound: u32) -> (u32, u32) {
    let product = u64::from(x) * u64::from(bound);
    let [b0, b1, b2, b3, b4, b5, b6, b7] = product.to_le_bytes();
    let low = u32::from_le_bytes([b0, b1, b2, b3]);
    let high = u32::from_le_bytes([b4, b5, b6, b7]);
    (high, low)
}
