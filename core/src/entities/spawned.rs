//! The two shapes a spawn resolves to: a live combat mob with a health pool, or
//! a placed object (NPC, soccer ball) that has none. This split mirrors exactly
//! the combat-carrying vs passive partition of the monster roster. The enum is
//! the shape only — the classification of a definition into one variant is a
//! decision, so it lives in [`crate::services::spawn`], not here.

use serde::{Deserialize, Serialize};

use crate::components::placement::Placement;
use crate::data::common::MonsterNumber;
use crate::entities::monster_instance::MonsterInstance;

/// A resolved spawn, kind-tagged. `Mob` is a live combat entity; `Placed` is a
/// stationary object with no health.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Spawned {
    /// A live combat monster (aggressive monster, guard, or trap).
    Mob {
        /// The live monster instance.
        instance: MonsterInstance,
    },
    /// A placed non-combat object (town NPC or the arena soccer ball) — no
    /// health pool.
    Placed {
        /// The object's monster number.
        number: MonsterNumber,
        /// Where it sits and which way it faces.
        placement: Placement,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::pool::Pool;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;

    fn placement() -> Placement {
        Placement {
            position: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_Y,
            movement: Movement::Grounded,
        }
    }

    #[test]
    fn mob_wire_round_trips() {
        let mob = Spawned::Mob {
            instance: MonsterInstance {
                number: MonsterNumber(7),
                placement: placement(),
                health: Pool::full(60),
            },
        };
        let json = serde_json::to_string(&mob).unwrap();
        assert!(json.starts_with(r#"{"kind":"mob""#));
        assert_eq!(serde_json::from_str::<Spawned>(&json).unwrap(), mob);
    }

    #[test]
    fn placed_wire_round_trips_without_health() {
        let placed = Spawned::Placed {
            number: MonsterNumber(248),
            placement: placement(),
        };
        let json = serde_json::to_string(&placed).unwrap();
        assert!(json.starts_with(r#"{"kind":"placed""#));
        assert!(!json.contains("health"));
        assert_eq!(serde_json::from_str::<Spawned>(&json).unwrap(), placed);
    }
}
