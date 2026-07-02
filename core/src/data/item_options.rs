//! Record shape of `item_options.json` — rollable item option definitions
//! (normal options, luck, excellent, ancient, wing options).

use serde::{Deserialize, Serialize};

use super::common::{Aggregate, OptionId, PowerUp, SourceVersion, StatId};

/// One option definition: a family of related option entries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemOptionDefinition {
    /// The definition's slug, the key item definitions reference.
    pub id: OptionId,
    /// Which option family this definition belongs to.
    pub option_type: OptionType,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Whether drops roll this option randomly.
    pub adds_randomly: bool,
    /// Roll probability per drop, `0.0..=1.0`.
    pub add_chance: f64,
    /// Maximum entries of this definition one item can carry.
    pub max_per_item: u8,
    /// The concrete option entries.
    pub options: Vec<OptionEntry>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Option family (pre-Season-3 set; harmony/guardian/socket are excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptionType {
    /// Plain jewel-raisable option.
    Option,
    /// Luck (+5% crit, +25% jewel success).
    Luck,
    /// Excellent option.
    Excellent,
    /// Ancient set option.
    AncientOption,
    /// Ancient per-piece stat bonus.
    AncientBonus,
    /// Wing option.
    Wing,
}

/// One concrete option entry, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OptionEntry {
    /// A flat power-up, independent of any level.
    Fixed {
        /// Entry number within the definition (client-facing).
        number: u16,
        /// The granted power-up.
        power_up: PowerUp,
    },
    /// A leveled power-up whose value follows a level.
    PerLevel {
        /// Entry number within the definition (client-facing).
        number: u16,
        /// Which level drives the value.
        level_type: LevelType,
        /// Stat the option modifies.
        stat: StatId,
        /// How the value folds into the stat total.
        aggregate: Aggregate,
        /// Value per level.
        levels: Vec<OptionLevelValue>,
    },
}

/// Which level a `per_level` option entry follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LevelType {
    /// The option's own level, raised with jewels.
    OptionLevel,
    /// The item's plus-level.
    ItemLevel,
}

/// Value of a leveled option entry at one level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OptionLevelValue {
    /// The level.
    pub level: u8,
    /// Value granted at that level.
    pub value: f64,
}
