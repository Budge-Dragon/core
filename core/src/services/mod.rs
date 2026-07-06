//! Stateless game rules: combat math, damage formulas, drop resolution,
//! leveling, and skill application.
//!
//! Services are pure functions over entities and components. They take the
//! current state plus an injected RNG and return updated state along with
//! [`crate::events`] — they never log, block, or touch the host.

pub mod chance;
pub mod combat;
pub mod craft;
pub mod death;
pub mod effects;
pub mod experience;
pub mod inventory;
pub mod item_roll;
pub mod item_rules;
pub mod kill;
pub mod loot;
pub mod monster_ai;
pub mod movement;
pub mod party;
pub mod price;
pub mod profile;
pub mod ratio;
pub mod shop;
pub mod skills;
pub mod spawn;
pub mod trade;
