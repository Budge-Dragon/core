//! Record shape of `drop_groups.json` — reusable drop tables referenced by
//! maps and monsters.

use serde::{Deserialize, Serialize};

use super::common::{DropGroupId, ItemRef, MonsterNumber, SourceVersion};

/// One drop group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DropGroup {
    /// The group's slug, the key maps and monsters reference.
    pub id: DropGroupId,
    /// Probability the group yields a drop, `0.0..=1.0`.
    pub chance: f64,
    /// Restricts the group to kills of this monster; absent = any monster.
    pub monster: Option<MonsterNumber>,
    /// Minimum monster level the group applies to; absent = unbounded.
    pub min_monster_level: Option<u16>,
    /// Maximum monster level the group applies to; absent = unbounded.
    pub max_monster_level: Option<u16>,
    /// What drops, kind-tagged.
    #[serde(flatten)]
    pub drop: DropGroupKind,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// What a drop group produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DropGroupKind {
    /// Money.
    Money,
    /// A random item from the monster-level drop pool.
    RandomItem,
    /// A random jewel.
    Jewel,
    /// A random excellent item.
    Excellent,
    /// A random ancient set item.
    Ancient,
    /// One item picked from a fixed list.
    ItemList {
        /// Candidate items.
        items: Vec<ItemRef>,
        /// Item level of the dropped item.
        item_level: u8,
    },
}
