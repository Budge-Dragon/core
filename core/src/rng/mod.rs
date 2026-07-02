//! Injected randomness.
//!
//! All randomness enters through [`rand_core::RngCore`], passed in by the
//! host — never a global or thread-local generator. This keeps the simulation
//! deterministic and replayable given a seed.
