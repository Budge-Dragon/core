//! The decided action of a monster AI tick, kind-tagged. The decision service
//! returns exactly one intent per acting mob; the movement variants carry the
//! resolved destination and facing, `Attack` carries only its target (the
//! intent to strike — damage resolution is a future combat wave).

use serde::{Deserialize, Serialize};

use crate::components::spatial::{Facing, WorldPos};

/// What a monster decided to do this action, kind-tagged and flat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MonsterIntent {
    /// It did nothing — either not yet ready to act or a stationary mob.
    Idle,
    /// It drifted to a random nearby tile.
    Wander {
        /// Where it moved to.
        to: WorldPos,
        /// Which way it now faces.
        facing: Facing,
    },
    /// It stepped toward a target it can see.
    Chase {
        /// Where it moved to.
        to: WorldPos,
        /// Which way it now faces.
        facing: Facing,
    },
    /// It stepped back toward its leash anchor, having strayed too far.
    LeashReturn {
        /// Where it moved to.
        to: WorldPos,
        /// Which way it now faces.
        facing: Facing,
    },
    /// It intends to strike a target in range (intent only; no damage here).
    Attack {
        /// The position of the target it strikes at.
        target: WorldPos,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::tile::TileCoord;

    #[test]
    fn idle_and_attack_wire_pins() {
        assert_eq!(
            serde_json::to_string(&MonsterIntent::Idle).unwrap(),
            r#"{"kind":"idle"}"#
        );
        let attack = MonsterIntent::Attack {
            target: TileCoord::new(4, 5).to_world(),
        };
        let json = serde_json::to_string(&attack).unwrap();
        assert!(json.starts_with(r#"{"kind":"attack""#));
        assert_eq!(
            serde_json::from_str::<MonsterIntent>(&json).unwrap(),
            attack
        );
    }

    #[test]
    fn move_intents_round_trip() {
        let wander = MonsterIntent::Wander {
            to: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_X,
        };
        let json = serde_json::to_string(&wander).unwrap();
        assert!(json.starts_with(r#"{"kind":"wander""#));
        assert_eq!(
            serde_json::from_str::<MonsterIntent>(&json).unwrap(),
            wander
        );

        let chase = MonsterIntent::Chase {
            to: TileCoord::new(6, 7).to_world(),
            facing: Facing::NEG_Y,
        };
        let leash = MonsterIntent::LeashReturn {
            to: TileCoord::new(1, 1).to_world(),
            facing: Facing::POS_Y,
        };
        for intent in [chase, leash] {
            let json = serde_json::to_string(&intent).unwrap();
            assert_eq!(
                serde_json::from_str::<MonsterIntent>(&json).unwrap(),
                intent
            );
        }
    }
}
