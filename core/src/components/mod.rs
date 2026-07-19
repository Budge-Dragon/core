//! Reusable building blocks shared by entities.
//!
//! Components are small, serializable value types (stats, positions,
//! inventories, buffs) that entities compose. They hold data and invariants
//! only; behavior lives in [`crate::services`].

pub mod active_effect;
pub mod bonus;
pub mod character_slot;
pub mod class;
pub mod collections;
pub mod combat_profile;
pub mod discovered_maps;
pub mod drop_claim;
pub mod element;
pub mod equipment;
pub mod interval;
pub mod inventory;
pub mod item_instance;
pub mod item_options;
pub mod item_quality;
pub mod item_ref;
pub mod levels;
pub mod life;
pub mod movement;
pub mod party;
pub mod placement;
pub mod pool;
pub mod reputation;
pub mod spatial;
pub mod stats;
pub mod tile;
pub mod trade_window;
pub mod units;
pub mod unlocked_classes;
pub mod vitals;
