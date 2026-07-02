//! Reusable building blocks shared by entities.
//!
//! Components are small, serializable value types (stats, positions,
//! inventories, buffs) that entities compose. They hold data and invariants
//! only; behavior lives in [`crate::services`].
