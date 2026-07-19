//! A character's three depletable resources bundled together. Each is an
//! independent [`Pool`] that self-guards its own `current <= max` invariant, so
//! the bundle carries no cross-field rule — it is a plain named grouping.
//! Characters carry [`Vitals`]; monsters carry a bare `health` [`Pool`] and no
//! fabricated mana or ability.

use serde::{Deserialize, Serialize};

use crate::components::combat_profile::VitalMaxima;
use crate::components::pool::Pool;

/// The three character resource pools: health, mana, and ability. No
/// cross-field invariant — each pool guards itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vitals {
    /// Health points.
    pub health: Pool,
    /// Mana points.
    pub mana: Pool,
    /// Ability (AG) points.
    pub ability: Pool,
}

impl Vitals {
    /// A fully-refilled bundle: each pool seeded at its class-formula maximum.
    /// The seed-at-max path a freshly-created character and a respawn share.
    #[must_use]
    pub(crate) fn full(maxima: VitalMaxima) -> Self {
        Self {
            health: Pool::full(maxima.max_health),
            mana: Pool::full(maxima.max_mana),
            ability: Pool::full(maxima.max_ability),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_carries_three_named_pools() {
        let vitals = Vitals {
            health: Pool::full(110),
            mana: Pool::full(20),
            ability: Pool::full(1),
        };
        let json = serde_json::to_string(&vitals).unwrap();
        assert_eq!(
            json,
            r#"{"health":{"current":110,"max":110},"mana":{"current":20,"max":20},"ability":{"current":1,"max":1}}"#
        );
        assert_eq!(serde_json::from_str::<Vitals>(&json).unwrap(), vitals);
    }
}
