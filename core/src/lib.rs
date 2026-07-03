//! Pure game logic for the MU Online rewrite.
//!
//! This crate is host-agnostic: the same code runs on a native server, inside
//! a SpacetimeDB module, in the browser via `wasm32-unknown-unknown`, and
//! behind FFI bindings for Unity.
//!
//! # Portability rules
//!
//! - No wall-clock time — all timing is tick-based.
//! - No async or threading.
//! - No logging — services return events instead.
//! - No engine or database types/IDs — plain Rust types only.
//! - RNG is injected via trait ([`rand_core::RngCore`]), never global.
//! - Static game data is defined as structs here and loaded by the host.
//! - No float math — integer and `Q40.24` fixed-point only, so replay is
//!   bit-identical across native, wasm, and FFI.
//!
//! This core is the authoritative simulation (client proposes, server decides):
//! services take a typed intent plus state and compute the result — they never
//! trust a client-claimed outcome. The full architectural laws, including the
//! six authoritative-server invariants, live in `CLAUDE.md`.

pub mod components;
pub mod data;
pub mod entities;
pub mod events;
pub mod rng;
pub mod services;
