//! The death lifecycle state a character rides: alive, or dead awaiting a
//! scheduled respawn. A small serde value the character composes like
//! [`crate::components::placement::Placement`] — data only. The two transitions
//! between the states (the monster-kill death penalty and the later respawn)
//! live in [`crate::services`]; nothing here decides or rolls.

use serde::{Deserialize, Serialize};

use crate::components::units::Tick;

/// Whether a character is alive or dead. `Dead` carries the tick its respawn is
/// scheduled for; `Alive` carries nothing — the respawn deadline exists only
/// while dead, so it lives on that variant alone and a live character can never
/// hold a stray deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LifeState {
    /// The character is alive.
    Alive,
    /// The character is dead, scheduled to respawn at `respawn_at`.
    Dead {
        /// The tick the scheduled respawn becomes due.
        respawn_at: Tick,
    },
}

impl LifeState {
    /// The alive state as a named constructor — the serde `default` seed for a
    /// legacy or freshly created character whose record omits the life field
    /// (the real "no death recorded" value, not a fabricated default), mirroring
    /// [`crate::components::active_effect::ActiveEffects::empty`].
    #[must_use]
    pub const fn alive() -> Self {
        Self::Alive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alive_round_trips_in_its_kind_tagged_wire_form() {
        let json = serde_json::to_string(&LifeState::Alive).unwrap();
        assert_eq!(json, r#"{"kind":"alive"}"#);
        assert_eq!(
            serde_json::from_str::<LifeState>(&json).unwrap(),
            LifeState::Alive
        );
    }

    #[test]
    fn dead_round_trips_carrying_respawn_at() {
        let dead = LifeState::Dead {
            respawn_at: Tick(903),
        };
        let json = serde_json::to_string(&dead).unwrap();
        assert_eq!(json, r#"{"kind":"dead","respawn_at":903}"#);
        assert_eq!(serde_json::from_str::<LifeState>(&json).unwrap(), dead);
    }

    #[test]
    fn alive_constructor_equals_the_alive_variant() {
        assert_eq!(LifeState::alive(), LifeState::Alive);
    }
}
