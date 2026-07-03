//! The outcome events of world population: one per entity the spawn service
//! places. The event kind mirrors the [`crate::entities::spawned::Spawned`]
//! state split — a live mob and a placed object route to different host
//! systems — so a non-combat placement is never mislabeled a monster spawn.

use serde::{Deserialize, Serialize};

use crate::components::spatial::{Facing, WorldPos};
use crate::data::common::MonsterNumber;

/// What the spawn service produced for one placed entity, kind-tagged. Flat
/// value fields — the aggregate is returned separately; the event carries only
/// what a host needs to announce the placement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpawnEvent {
    /// A live combat monster was spawned.
    MobSpawned {
        /// The monster number spawned.
        number: MonsterNumber,
        /// Where it appeared.
        at: WorldPos,
        /// Which way it faces.
        facing: Facing,
    },
    /// A non-combat object (NPC or soccer ball) was placed.
    ObjectPlaced {
        /// The object's monster number.
        number: MonsterNumber,
        /// Where it was placed.
        at: WorldPos,
        /// Which way it faces.
        facing: Facing,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::tile::TileCoord;

    #[test]
    fn mob_spawned_wire_round_trips() {
        let event = SpawnEvent::MobSpawned {
            number: MonsterNumber(7),
            at: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_X_POS_Y,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"mob_spawned","number":7,"at":{"x":163840,"y":229376},"facing":{"x":1,"y":1}}"#
        );
        assert_eq!(serde_json::from_str::<SpawnEvent>(&json).unwrap(), event);
    }

    #[test]
    fn object_placed_wire_round_trips() {
        let event = SpawnEvent::ObjectPlaced {
            number: MonsterNumber(248),
            at: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_Y,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.starts_with(r#"{"kind":"object_placed""#));
        assert_eq!(serde_json::from_str::<SpawnEvent>(&json).unwrap(), event);
    }
}
