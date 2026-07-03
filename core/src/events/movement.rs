//! The outcome events of the movement service: flight-mode changes, grounded
//! steps, and warp/gate arrivals. Each is a flat kind-tagged value returned by
//! its service — one service, one outcome enum, mirroring
//! [`crate::events::spawn::SpawnEvent`].

use serde::{Deserialize, Serialize};

use crate::components::movement::Movement;
use crate::components::placement::Placement;

/// What a flight-mode change produced, kind-tagged: the mode actually changed,
/// or the change was denied for a single reason. A redundant change (already in
/// the requested mode) emits neither — the service returns an empty outcome
/// list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlightOutcome {
    /// The traversal mode changed to `mode`.
    ModeChanged {
        /// The mode now in effect.
        mode: Movement,
    },
    /// The change was denied and the mode is unchanged.
    Denied {
        /// Why the change was rejected.
        reason: FlightDenialReason,
    },
}

/// Why a flight-mode change was denied. A single flat reason; the precedence
/// among them (capability before transient state) is decided by the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightDenialReason {
    /// No wings equipped, so voluntary flight is impossible.
    NoWings,
    /// Locked in combat, so voluntary flight is barred.
    CombatLocked,
    /// The map is a Sky map, which forces flight — grounding is impossible.
    SkyForcesFlight,
}

/// What a grounded/flying step produced, kind-tagged: the entity moved to a new
/// placement, or a grounded step was blocked by a non-walkable destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepOutcome {
    /// The step resolved; `placement` is where the entity now stands.
    Resolved {
        /// The new placement.
        placement: Placement,
    },
    /// A grounded step onto a non-walkable destination cell; no move happened.
    Blocked,
}

/// What a warp/gate arrival produced, kind-tagged: the traveler landed on a
/// walkable tile, or the landing area held no walkable tile at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WarpOutcome {
    /// The traveler arrived; `placement` is where they landed.
    Arrived {
        /// The arrival placement.
        placement: Placement,
    },
    /// The landing area had no walkable tile; the traveler was not moved.
    NoWalkableLanding,
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn flight_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&FlightOutcome::ModeChanged {
                mode: Movement::Flying
            })
            .unwrap(),
            r#"{"kind":"mode_changed","mode":"flying"}"#
        );
        assert_eq!(
            serde_json::to_string(&FlightOutcome::Denied {
                reason: FlightDenialReason::NoWings
            })
            .unwrap(),
            r#"{"kind":"denied","reason":"no_wings"}"#
        );
        let denied = FlightOutcome::Denied {
            reason: FlightDenialReason::SkyForcesFlight,
        };
        assert_eq!(
            serde_json::from_str::<FlightOutcome>(
                r#"{"kind":"denied","reason":"sky_forces_flight"}"#
            )
            .unwrap(),
            denied
        );
    }

    #[test]
    fn step_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&StepOutcome::Blocked).unwrap(),
            r#"{"kind":"blocked"}"#
        );
        let resolved = StepOutcome::Resolved {
            placement: placement(),
        };
        let json = serde_json::to_string(&resolved).unwrap();
        assert!(json.starts_with(r#"{"kind":"resolved""#));
        assert_eq!(
            serde_json::from_str::<StepOutcome>(&json).unwrap(),
            resolved
        );
    }

    #[test]
    fn warp_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&WarpOutcome::NoWalkableLanding).unwrap(),
            r#"{"kind":"no_walkable_landing"}"#
        );
        let arrived = WarpOutcome::Arrived {
            placement: placement(),
        };
        let json = serde_json::to_string(&arrived).unwrap();
        assert!(json.starts_with(r#"{"kind":"arrived""#));
        assert_eq!(serde_json::from_str::<WarpOutcome>(&json).unwrap(), arrived);
    }
}
