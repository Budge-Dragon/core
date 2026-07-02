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

pub mod components;
pub mod data;
pub mod entities;
pub mod events;
pub mod rng;
pub mod services;
