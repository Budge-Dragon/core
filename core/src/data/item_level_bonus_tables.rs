//! Record shape of `item_level_bonus_tables.json` — per-item-level bonus
//! values referenced by item power-ups.

use serde::{Deserialize, Serialize};

use super::common::{BonusTableId, SourceVersion};

/// One bonus table: a dense array indexed by item level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemLevelBonusTable {
    /// The table's slug, the key item power-ups reference.
    pub id: BonusTableId,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Bonus value per item level; index = level, length = cap + 1.
    pub values_by_level: Vec<f64>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}
