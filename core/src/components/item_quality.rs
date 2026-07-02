//! The item quality tier vocabulary the items domain owns — the value enum the
//! drop-level and durability rules key off. Value vocabulary only; the rules
//! that consume it live in [`crate::services`].

use serde::{Deserialize, Serialize};

/// Item rarity tier — the quality band an item instance carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemRarity {
    /// An ordinary item.
    Normal,
    /// An excellent item.
    Excellent,
    /// An ancient item.
    Ancient,
}
