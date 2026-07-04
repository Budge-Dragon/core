//! A live monster in the world: a spawned combat entity with a placement and a
//! health pool. The instance carries no mana or ability — those belong to
//! characters. Its health is seeded full by the spawn service from the
//! definition's combat block; the instance itself never invents a value.

use serde::{Deserialize, Serialize};

use crate::components::active_effect::ActiveEffects;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::components::spatial::WorldPos;
use crate::components::units::Tick;
use crate::data::common::MonsterNumber;

/// A live monster: which definition it is, where it stands, its health, the
/// spawn origin it tethers to, and when it may next act. No cross-field
/// invariant — the [`Pool`] self-guards its own bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonsterInstance {
    /// The monster number this instance was spawned from.
    pub number: MonsterNumber,
    /// Where it stands and which way it faces.
    pub placement: Placement,
    /// Its health pool, seeded full at spawn.
    pub health: Pool,
    /// The spawn origin it leashes to; diverges from `placement` as it roams.
    pub anchor: WorldPos,
    /// The next tick at which it may act — its cadence clock.
    pub next_action: Tick,
    /// Its live timed effects; seeded empty at spawn. A record that predates
    /// timed effects carries none — the real "no active effects" value.
    #[serde(default = "ActiveEffects::empty")]
    pub active_effects: ActiveEffects,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;

    fn placement() -> Placement {
        Placement {
            position: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_Y,
            movement: Movement::Grounded,
            map: MapNumber(0),
        }
    }

    #[test]
    fn full_health_is_current_equals_max() {
        let instance = MonsterInstance {
            number: MonsterNumber(7),
            placement: placement(),
            health: Pool::full(60),
            anchor: placement().position,
            next_action: Tick(0),
            active_effects: ActiveEffects::EMPTY,
        };
        assert_eq!(instance.health.current(), 60);
        assert_eq!(instance.health.max(), 60);
        assert_eq!(instance.number, MonsterNumber(7));
        assert_eq!(instance.placement, placement());
        assert_eq!(instance.anchor, placement().position);
        assert_eq!(instance.next_action, Tick(0));
    }

    #[test]
    fn boundary_one_hp_is_full() {
        let instance = MonsterInstance {
            number: MonsterNumber(1),
            placement: placement(),
            health: Pool::full(1),
            anchor: placement().position,
            next_action: Tick(0),
            active_effects: ActiveEffects::EMPTY,
        };
        assert_eq!(instance.health.current(), 1);
        assert_eq!(instance.health.max(), 1);
    }

    #[test]
    fn wire_round_trips() {
        let instance = MonsterInstance {
            number: MonsterNumber(7),
            placement: placement(),
            health: Pool::full(60),
            anchor: placement().position,
            next_action: Tick(9),
            active_effects: ActiveEffects::EMPTY,
        };
        let json = serde_json::to_string(&instance).unwrap();
        assert_eq!(
            serde_json::from_str::<MonsterInstance>(&json).unwrap(),
            instance
        );
        // A record predating timed effects (no field) still parses, empty.
        let legacy = r#"{"number":7,"placement":{"position":{"x":163840,"y":229376},"facing":{"x":0,"y":1},"movement":"grounded","map":0},"health":{"current":60,"max":60},"anchor":{"x":163840,"y":229376},"next_action":9}"#;
        assert_eq!(
            serde_json::from_str::<MonsterInstance>(legacy)
                .unwrap()
                .active_effects,
            ActiveEffects::EMPTY
        );
    }
}
