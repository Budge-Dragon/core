//! What a monster kill's death step and the later respawn produced. The death
//! step docks experience and zen and marks the character dead, returning a
//! `Vec<DeathEvent>`; the respawn seats the character back in town, returning a
//! single [`Respawned`] carrying the sampled landing a client cannot recompute.
//! Outcome data only — the two transitions live in [`crate::services`].

use serde::{Deserialize, Serialize};

use crate::components::spatial::{Facing, WorldPos};
use crate::components::units::{CarriedZen, Exp, MapNumber, Tick, Zen};

/// One observable outcome of the death step, kind-tagged. `Died` fires on every
/// alive death; each dock fires only when its magnitude is non-zero (a
/// sub-level-10 or max-level death docks no experience; a penalty that floors to
/// zero docks no zen), so a zero-magnitude outcome emits no event and an
/// already-dead re-death emits none at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeathEvent {
    /// The character was marked dead, scheduled to respawn at `respawn_at`.
    Died {
        /// The tick the scheduled respawn becomes due.
        respawn_at: Tick,
    },
    /// The death penalty docked experience.
    ExperienceDocked {
        /// The experience removed.
        lost: Exp,
        /// The stored experience remaining after the dock.
        remaining: Exp,
    },
    /// The death penalty docked carried zen.
    ZenDocked {
        /// The zen removed.
        lost: Zen,
        /// The carried balance remaining after the dock.
        remaining: CarriedZen,
    },
}

/// The load-bearing respawn outcome: where the character stood back up. Carries
/// the sampled landing tile — a uniform draw over the gate's walkable set that a
/// client cannot recompute — so it is a returned value, not derivable state. The
/// refilled vitals and cleared effects ride on the returned character and need
/// no event. A plain struct, not a single-variant enum: the respawn either
/// happened (`Some`) or the character was already alive (`None`), a distinction
/// the transition carries in its `Option<Respawned>` return.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Respawned {
    /// The map the character respawned on.
    pub map: MapNumber,
    /// The walkable tile the character landed on.
    pub position: WorldPos,
    /// The direction the character faces on respawn.
    pub facing: Facing,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::tile::TileCoord;

    #[test]
    fn died_round_trips_in_its_kind_tagged_wire_form() {
        let died = DeathEvent::Died {
            respawn_at: Tick(560),
        };
        let json = serde_json::to_string(&died).unwrap();
        assert_eq!(json, r#"{"kind":"died","respawn_at":560}"#);
        assert_eq!(serde_json::from_str::<DeathEvent>(&json).unwrap(), died);
    }

    #[test]
    fn experience_docked_round_trips_with_bare_integer_amounts() {
        let docked = DeathEvent::ExperienceDocked {
            lost: Exp(3149),
            remaining: Exp(5_000_000),
        };
        let json = serde_json::to_string(&docked).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"experience_docked","lost":3149,"remaining":5000000}"#
        );
        assert_eq!(serde_json::from_str::<DeathEvent>(&json).unwrap(), docked);
    }

    #[test]
    fn zen_docked_round_trips_with_zen_lost_and_carried_remaining() {
        let docked = DeathEvent::ZenDocked {
            lost: Zen(10_000),
            remaining: CarriedZen::new(990_000).unwrap(),
        };
        let json = serde_json::to_string(&docked).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"zen_docked","lost":10000,"remaining":990000}"#
        );
        assert_eq!(serde_json::from_str::<DeathEvent>(&json).unwrap(), docked);
    }

    #[test]
    fn respawned_round_trips_as_a_plain_struct() {
        let respawned = Respawned {
            map: MapNumber(3),
            position: TileCoord::new(174, 112).to_world(),
            facing: Facing::POS_Y,
        };
        let json = serde_json::to_string(&respawned).unwrap();
        assert_eq!(serde_json::from_str::<Respawned>(&json).unwrap(), respawned);
        // No `kind` tag — a plain struct, not a kind-tagged enum.
        assert!(!json.contains("kind"));
    }
}
