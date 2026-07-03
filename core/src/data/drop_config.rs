//! The `drops` section of `game_config.json` — global per-kill drop tuning.

use serde::{Deserialize, Serialize};

use crate::components::collections::OneOrMore;
use crate::components::units::ChancePer10000;

use super::common::ItemRef;

/// Global drop tuning. One category roll per kill: money, item, jewel,
/// excellent partition a 0..10,000 space in that fixed order; the remainder is
/// no drop. Constructed only by parse (`RawDropConfig` proves the four category
/// rates sum to <= 10,000), so the `Nothing` remainder is non-negative by
/// construction and no service re-checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawDropConfig", into = "RawDropConfig")]
pub struct DropConfig {
    money_roll_per_10000: ChancePer10000,
    item_roll_per_10000: ChancePer10000,
    jewel_roll_per_10000: ChancePer10000,
    excellent_roll_per_10000: ChancePer10000,
    skill_roll_per_10000: ChancePer10000,
    jewel_drops: OneOrMore<ItemRef>,
    review: Option<String>,
}

impl DropConfig {
    /// Per-kill chance the kill drops zen.
    #[must_use]
    pub fn money_roll(&self) -> ChancePer10000 {
        self.money_roll_per_10000
    }

    /// Per-kill chance the kill drops an item from the monster-level pool.
    #[must_use]
    pub fn item_roll(&self) -> ChancePer10000 {
        self.item_roll_per_10000
    }

    /// Per-kill chance the kill drops a jewel from `jewel_drops`.
    #[must_use]
    pub fn jewel_roll(&self) -> ChancePer10000 {
        self.jewel_roll_per_10000
    }

    /// Per-kill chance the kill drops an excellent item.
    #[must_use]
    pub fn excellent_roll(&self) -> ChancePer10000 {
        self.excellent_roll_per_10000
    }

    /// Chance a dropped weapon that carries an equip skill drops with it.
    #[must_use]
    pub fn skill_roll(&self) -> ChancePer10000 {
        self.skill_roll_per_10000
    }

    /// The jewels the world jewel roll draws from.
    #[must_use]
    pub fn jewel_drops(&self) -> &OneOrMore<ItemRef> {
        &self.jewel_drops
    }

    /// The remainder weight assigned to "no drop": 10,000 minus the four
    /// category rates. Non-negative by construction.
    #[must_use]
    pub fn nothing_weight(&self) -> u16 {
        ChancePer10000::DENOMINATOR - self.category_sum()
    }

    fn category_sum(&self) -> u16 {
        self.money_roll_per_10000.numerator()
            + self.item_roll_per_10000.numerator()
            + self.jewel_roll_per_10000.numerator()
            + self.excellent_roll_per_10000.numerator()
    }
}

/// Wire mirror of [`DropConfig`], validated on the way in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct RawDropConfig {
    money_roll_per_10000: ChancePer10000,
    item_roll_per_10000: ChancePer10000,
    jewel_roll_per_10000: ChancePer10000,
    excellent_roll_per_10000: ChancePer10000,
    skill_roll_per_10000: ChancePer10000,
    jewel_drops: OneOrMore<ItemRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    review: Option<String>,
}

impl TryFrom<RawDropConfig> for DropConfig {
    type Error = DropConfigError;

    fn try_from(raw: RawDropConfig) -> Result<Self, Self::Error> {
        let sum = u32::from(raw.money_roll_per_10000.numerator())
            + u32::from(raw.item_roll_per_10000.numerator())
            + u32::from(raw.jewel_roll_per_10000.numerator())
            + u32::from(raw.excellent_roll_per_10000.numerator());
        if sum > u32::from(ChancePer10000::DENOMINATOR) {
            return Err(DropConfigError::CategorySumAbove10000 { sum });
        }
        Ok(Self {
            money_roll_per_10000: raw.money_roll_per_10000,
            item_roll_per_10000: raw.item_roll_per_10000,
            jewel_roll_per_10000: raw.jewel_roll_per_10000,
            excellent_roll_per_10000: raw.excellent_roll_per_10000,
            skill_roll_per_10000: raw.skill_roll_per_10000,
            jewel_drops: raw.jewel_drops,
            review: raw.review,
        })
    }
}

impl From<DropConfig> for RawDropConfig {
    fn from(config: DropConfig) -> Self {
        Self {
            money_roll_per_10000: config.money_roll_per_10000,
            item_roll_per_10000: config.item_roll_per_10000,
            jewel_roll_per_10000: config.jewel_roll_per_10000,
            excellent_roll_per_10000: config.excellent_roll_per_10000,
            skill_roll_per_10000: config.skill_roll_per_10000,
            jewel_drops: config.jewel_drops,
            review: config.review,
        }
    }
}

/// Parse failure: the four category rates would over-fill the 0..10,000 space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropConfigError {
    /// The category rates sum beyond 10,000.
    CategorySumAbove10000 {
        /// The offending sum.
        sum: u32,
    },
}

impl core::fmt::Display for DropConfigError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CategorySumAbove10000 { sum } => {
                write!(f, "drop category rates sum to {sum}, above 10000")
            }
        }
    }
}

impl core::error::Error for DropConfigError {}
