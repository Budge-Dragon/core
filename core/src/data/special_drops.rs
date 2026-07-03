//! Record shape of `special_drops.json` — per-fact special drops keyed by the
//! game's own identities.

use serde::{Deserialize, Serialize};

use crate::components::collections::OneOrMore;
use crate::components::units::{ChancePer10000, ItemLevel, Level};

use super::common::{ItemRef, MapNumber, MonsterNumber, Provenance};

/// One special-drop record: a fact plus its provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialDropRecord {
    /// The drop fact, kind-tagged by scope.
    #[serde(flatten)]
    pub drop: SpecialDrop,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// A special drop, scoped by the game's own identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpecialDrop {
    /// World-wide drop whose plus-level is banded by monster level.
    LevelBanded {
        /// What drops.
        item: ItemRef,
        /// Roll chance per band-eligible kill.
        chance_per_10000: ChancePer10000,
        /// Ascending thresholds to plus level; below the first band nothing
        /// drops.
        bands: DropBands,
    },
    /// Loot bound to one monster number, dropped on every kill.
    MonsterBound {
        /// The monster whose kill yields this.
        monster: MonsterNumber,
        /// Uniform pick among these; a single entry is a fixed drop.
        items: OneOrMore<ItemRef>,
        /// Plus level of the dropped instance.
        item_level: ItemLevel,
    },
    /// Map-bound world drop from monsters at or above a level floor.
    MapBound {
        /// The map the kill must happen on.
        map: MapNumber,
        /// Minimum monster level for the roll.
        min_monster_level: Level,
        /// What drops.
        item: ItemRef,
        /// Plus level of the dropped instance.
        item_level: ItemLevel,
        /// Roll chance per eligible kill.
        chance_per_10000: ChancePer10000,
    },
}

/// One band: kills at `min_monster_level` and above — until the next band's
/// threshold — drop the item at `item_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropBand {
    /// Inclusive lower monster-level edge of the band.
    pub min_monster_level: Level,
    /// Plus level dropped inside the band.
    pub item_level: ItemLevel,
}

/// Non-empty band table with strictly ascending thresholds, parsed once. Gaps
/// and overlaps are unrepresentable: every band ends where the next begins and
/// the last band is open-ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<DropBand>", into = "Vec<DropBand>")]
pub struct DropBands {
    bands: Vec<DropBand>,
}

impl DropBands {
    /// Total over the parsed structure: plus level for a monster level; `None`
    /// below the first band (genuine absence — monster too low).
    #[must_use]
    pub fn item_level_for(&self, monster_level: Level) -> Option<ItemLevel> {
        let mut chosen = None;
        for band in &self.bands {
            if band.min_monster_level <= monster_level {
                chosen = Some(band.item_level);
            }
        }
        chosen
    }

    /// The bands in ascending order.
    #[must_use]
    pub fn bands(&self) -> &[DropBand] {
        &self.bands
    }
}

impl TryFrom<Vec<DropBand>> for DropBands {
    type Error = DropBandsError;

    fn try_from(bands: Vec<DropBand>) -> Result<Self, Self::Error> {
        let mut previous: Option<Level> = None;
        for band in &bands {
            match previous {
                Some(prev) if band.min_monster_level <= prev => {
                    return Err(DropBandsError::NotStrictlyAscending {
                        at: band.min_monster_level,
                    });
                }
                Some(_) | None => previous = Some(band.min_monster_level),
            }
        }
        if previous.is_none() {
            return Err(DropBandsError::Empty);
        }
        Ok(Self { bands })
    }
}

impl From<DropBands> for Vec<DropBand> {
    fn from(bands: DropBands) -> Self {
        bands.bands
    }
}

/// Parse failure assembling a band table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropBandsError {
    /// No bands.
    Empty,
    /// A band threshold is not strictly greater than its predecessor.
    NotStrictlyAscending {
        /// The offending threshold.
        at: Level,
    },
}

impl core::fmt::Display for DropBandsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "a band table must have at least one band"),
            Self::NotStrictlyAscending { at } => {
                write!(f, "band threshold {at:?} is not strictly ascending")
            }
        }
    }
}

impl core::error::Error for DropBandsError {}
