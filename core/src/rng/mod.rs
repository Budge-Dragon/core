//! Injected randomness.
//!
//! All randomness enters through [`rand_core::RngCore`], passed in by the host
//! — never a global or thread-local generator. This keeps the simulation
//! deterministic and replayable given a seed. The single sampling primitive is
//! [`uniform_below`]; every domain draw routes through it (via
//! [`crate::services::chance`]), so no `%` reduction of `next_u32` exists
//! anywhere in the crate.

use core::num::{NonZeroU32, NonZeroUsize};

use rand_core::RngCore;

// Every target this crate supports has a pointer no wider than 64 bits, so a
// collection length widens into `u64` and a drawn index narrows back with no
// loss — proven here rather than assumed.
const _: () = assert!(usize::BITS <= u64::BITS);

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

/// Unbiased uniform index in `0..bound.get()` over the collection-index domain.
/// The draw runs at a fixed 64-bit width, so both the index and the RNG words
/// consumed are identical on 32- and 64-bit `usize` targets — the pick stays
/// replayable across native, wasm, and FFI regardless of pointer width.
/// Deterministic given the RNG seed.
#[must_use]
pub fn uniform_below_usize(bound: NonZeroUsize, rng: &mut impl RngCore) -> usize {
    narrow_index(uniform_below_u64(widen_index(bound.get()), rng))
}

/// Unbiased uniform integer in `0..bound`, at 64-bit width. `bound` is derived
/// from a `NonZeroUsize` at the sole call site, so it is always at least one —
/// mirroring how [`uniform_below`] treats its `NonZeroU32` as a plain `u32`.
fn uniform_below_u64(bound: u64, rng: &mut impl RngCore) -> u64 {
    let (mut high, mut low) = widening_mul_u64(rng.next_u64(), bound);
    if low < bound {
        // (2^64 mod bound), computed without a 128-bit modulo.
        let threshold = bound.wrapping_neg() % bound;
        while low < threshold {
            let (next_high, next_low) = widening_mul_u64(rng.next_u64(), bound);
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

/// The high and low 64-bit halves of `x * bound`, extracted through byte
/// decomposition so no truncating `as` cast is needed.
fn widening_mul_u64(x: u64, bound: u64) -> (u64, u64) {
    let product = u128::from(x) * u128::from(bound);
    let [
        b0,
        b1,
        b2,
        b3,
        b4,
        b5,
        b6,
        b7,
        b8,
        b9,
        b10,
        b11,
        b12,
        b13,
        b14,
        b15,
    ] = product.to_le_bytes();
    let low = u64::from_le_bytes([b0, b1, b2, b3, b4, b5, b6, b7]);
    let high = u64::from_le_bytes([b8, b9, b10, b11, b12, b13, b14, b15]);
    (high, low)
}

/// Zero-extends a collection length into `u64`. Lossless (pointer width is at
/// most 64 bits, asserted above), cast-free, and total.
fn widen_index(value: usize) -> u64 {
    let mut bytes = [0u8; 8];
    for (slot, byte) in bytes.iter_mut().zip(value.to_le_bytes()) {
        *slot = byte;
    }
    u64::from_le_bytes(bytes)
}

/// Narrows a drawn index back to `usize`. The caller only ever passes a value
/// below the original `usize` bound, so the discarded high bytes are zero —
/// lossless, cast-free, and total.
fn narrow_index(value: u64) -> usize {
    let full = value.to_le_bytes();
    let mut bytes = [0u8; core::mem::size_of::<usize>()];
    for (slot, byte) in bytes.iter_mut().zip(full) {
        *slot = byte;
    }
    usize::from_le_bytes(bytes)
}
