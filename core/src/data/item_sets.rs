//! Record shape of `item_sets.json` — ancient sets and generic armor sets.

use serde::{Deserialize, Serialize};

use super::common::{ItemRef, PowerUp, SetGroupId, SourceVersion, StatId};

/// One set group.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemSet {
    /// The set's slug, the key item definitions reference.
    pub id: SetGroupId,
    /// Set number as the client knows it.
    pub number: u16,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Minimum equipped pieces before any set option applies.
    pub min_item_count: u8,
    /// Whether only distinct pieces count toward the minimum.
    pub count_distinct: bool,
    /// Required item level of the pieces; `0` = any level.
    pub set_level: u8,
    /// Whether all options apply as soon as the minimum count is met.
    pub always_applies: bool,
    /// The pieces the set consists of.
    pub pieces: Vec<SetPiece>,
    /// Options unlocked as pieces accumulate.
    pub set_options: Vec<SetOption>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// One piece of a set.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetPiece {
    /// The item serving as this piece.
    pub item: ItemRef,
    /// Ancient discriminator (1 or 2); `0` = generic non-ancient piece.
    pub discriminator: u8,
    /// Stat raised by the per-piece ancient bonus; absent = no piece bonus.
    pub bonus_stat: Option<StatId>,
}

/// One unlockable set option.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetOption {
    /// Option number within the set (client-facing).
    pub number: u16,
    /// The granted power-up.
    pub power_up: PowerUp,
}
