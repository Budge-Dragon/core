//! Record shape of `spawns.json` — world population, the classic
//! MonsterSetBase roster.

use serde::{Deserialize, Serialize};

use crate::components::geometry::{Direction, Point, Rect};

use super::common::{MapNumber, MonsterNumber, Provenance};

/// One spawn record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spawn {
    /// Map the spawn belongs to.
    pub map: MapNumber,
    /// Monster/NPC that spawns.
    pub monster: MonsterNumber,
    /// Where instances appear, kind-tagged.
    pub placement: SpawnPlacement,
    /// When the spawn is present, kind-tagged.
    pub schedule: SpawnSchedule,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// Where a spawn record places its instances, kind-tagged — the classic
/// MonsterSetBase distinction between single-spot rows and area rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpawnPlacement {
    /// One stationary object at an exact tile with a fixed facing — NPCs,
    /// guard posts, traps, the soccer ball. Always one instance.
    Fixed {
        /// The tile.
        position: Point,
        /// Facing on spawn (the trap's firing direction for
        /// `TrapTargeting::Directional`).
        facing: Direction,
    },
    /// Mobile monsters spawned at one tile.
    Spot {
        /// The tile.
        position: Point,
        /// Instances kept alive at this spot.
        quantity: u16,
    },
    /// Mobile monsters spawned at random walkable tiles in a rectangle.
    Area {
        /// The spawn rectangle.
        area: Rect,
        /// Instances kept alive inside the rectangle.
        quantity: u16,
    },
}

/// When a spawn record's instances exist in the world, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpawnSchedule {
    /// Present from world start; dead instances respawn after the definition's
    /// respawn delay.
    Permanent,
    /// A wandering-merchant location; at most one wandering spawn is active
    /// across the world at a time.
    Wandering,
}
