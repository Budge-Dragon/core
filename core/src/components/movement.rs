//! Traversal mode: the two-state classification of how an entity crosses the
//! ground plane. Its only rule is whether the tile walk-grid check applies —
//! no altitude, no air combat, no anti-air.

use serde::{Deserialize, Serialize};

/// How an entity traverses the ground plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement {
    /// Bound to walkable tiles.
    Grounded,
    /// Free of the walk grid.
    Flying,
}

impl Movement {
    /// Whether traversal is constrained by the tile walk grid.
    #[must_use]
    pub fn checks_walkability(self) -> bool {
        match self {
            Movement::Grounded => true,
            Movement::Flying => false,
        }
    }
}

/// A requested change of flight mode — the input alphabet of the flight
/// transition. Host-supplied intent; the gate decides whether it is permitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightChange {
    /// Leave the ground and fly.
    EnableFlight,
    /// Return to the ground.
    DisableFlight,
}

/// Whether wings are equipped — a flight-eligibility fact supplied by the host.
/// Deriving it from equipped items is a future wave's job; core only decides
/// with the fact in hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wings {
    /// Wings are equipped.
    Equipped,
    /// No wings equipped.
    None,
}

/// Whether an entity is locked in combat — a flight-eligibility fact supplied
/// by the host. Deriving it from combat state is a future wave's job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CombatLock {
    /// Locked in combat; voluntary flight is barred.
    Locked,
    /// Free of combat.
    Free,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walkability_per_variant() {
        assert!(Movement::Grounded.checks_walkability());
        assert!(!Movement::Flying.checks_walkability());
    }

    #[test]
    fn wire_is_bare_snake_case_string() {
        assert_eq!(
            serde_json::to_string(&Movement::Grounded).unwrap(),
            r#""grounded""#
        );
        assert_eq!(
            serde_json::to_string(&Movement::Flying).unwrap(),
            r#""flying""#
        );
        assert_eq!(
            serde_json::from_str::<Movement>(r#""grounded""#).unwrap(),
            Movement::Grounded
        );
    }

    #[test]
    fn flight_inputs_are_bare_snake_case_strings() {
        assert_eq!(
            serde_json::to_string(&FlightChange::EnableFlight).unwrap(),
            r#""enable_flight""#
        );
        assert_eq!(
            serde_json::to_string(&FlightChange::DisableFlight).unwrap(),
            r#""disable_flight""#
        );
        assert_eq!(
            serde_json::to_string(&Wings::Equipped).unwrap(),
            r#""equipped""#
        );
        assert_eq!(serde_json::to_string(&Wings::None).unwrap(), r#""none""#);
        assert_eq!(
            serde_json::to_string(&CombatLock::Locked).unwrap(),
            r#""locked""#
        );
        assert_eq!(
            serde_json::to_string(&CombatLock::Free).unwrap(),
            r#""free""#
        );
        assert_eq!(
            serde_json::from_str::<FlightChange>(r#""enable_flight""#).unwrap(),
            FlightChange::EnableFlight
        );
        assert_eq!(
            serde_json::from_str::<Wings>(r#""equipped""#).unwrap(),
            Wings::Equipped
        );
        assert_eq!(
            serde_json::from_str::<CombatLock>(r#""free""#).unwrap(),
            CombatLock::Free
        );
    }
}
