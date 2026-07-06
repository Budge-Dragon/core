//! Record shape of `map_definitions.json` — one record per game map.

use serde::{Deserialize, Serialize};

use crate::components::tile::{TileArea, TileCoord};

use super::common::{MapNumber, Provenance};

/// One game map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapDefinition {
    /// Client map number — the game's own map identity.
    pub number: MapNumber,
    /// Traversal medium; entry and movement rules in services match on it.
    pub environment: MapEnvironment,
    /// The town a death on this map respawns at: the map itself when it owns a
    /// spawn gate, else Lorencia (0), with the two authentic overrides applied
    /// (Devil Square → Noria, Icarus → Lost Tower). Required on the wire — a
    /// missing value is a generation bug, not a default.
    pub respawn_map: MapNumber,
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
    pub ground: TileArea,
    /// Left goal area.
    pub left_goal: TileArea,
    /// Right goal area.
    pub right_goal: TileArea,
    /// Left team spawn point.
    pub left_spawn: TileCoord,
    /// Right team spawn point.
    pub right_spawn: TileCoord,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_map_definition_carrying_respawn_map() {
        let json = r#"{"number":8,"environment":"ground","respawn_map":8,"source_version":"s6"}"#;
        let def: MapDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(def.respawn_map, MapNumber(8));

        let wire = serde_json::to_string(&def).unwrap();
        let reparsed: MapDefinition = serde_json::from_str(&wire).unwrap();
        assert_eq!(reparsed, def);
    }

    #[test]
    fn rejects_a_map_definition_that_omits_respawn_map() {
        // respawn_map is required — a record without it is a parse error at the
        // data boundary, never a fabricated default.
        let json = r#"{"number":8,"environment":"ground","source_version":"s6"}"#;
        assert!(serde_json::from_str::<MapDefinition>(json).is_err());
    }
}
