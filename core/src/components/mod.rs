//! Reusable building blocks shared by entities.
//!
//! Components are small, serializable value types (stats, positions,
//! inventories, buffs) that entities compose. They hold data and invariants
//! only; behavior lives in [`crate::services`].

pub mod bonus;
pub mod class;
pub mod collections;
pub mod element;
pub mod interval;
pub mod item_options;
pub mod item_quality;
pub mod levels;
pub mod movement;
pub mod placement;
pub mod pool;
pub mod spatial;
pub mod stats;
pub mod tile;
pub mod units;
pub mod vitals;
