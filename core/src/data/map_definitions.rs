//! Record shape of `map_definitions.json` — one record per game map.

use serde::{Deserialize, Serialize};

use crate::components::geometry::{Point, Rect};

use super::common::{MapNumber, Provenance};

/// One game map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapDefinition {
    /// Client map number — the game's own map identity.
    pub number: MapNumber,
    /// Traversal medium; entry and movement rules in services match on it.
    pub environment: MapEnvironment,
    /// Battle-soccer pitch; present only on Arena.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soccer_pitch: Option<SoccerPitch>,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// Traversal medium of a map. Closed pre-S3 set; a map is exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MapEnvironment {
    /// Ordinary ground map.
    Ground,
    /// Underwater map (Atlans); underwater movement/combat rules apply.
    Underwater,
    /// Sky map (Icarus); entry requires the ability to fly.
    Sky,
}

/// The Arena battle-soccer pitch: a ground field, two goals, two team spawns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoccerPitch {
    /// Playing field.
    pub ground: Rect,
    /// Left goal area.
    pub left_goal: Rect,
    /// Right goal area.
    pub right_goal: Rect,
    /// Left team spawn point.
    pub left_spawn: Point,
    /// Right team spawn point.
    pub right_spawn: Point,
}
