//! Traversal mode: the two-state classification of how an entity crosses the
//! ground plane. Its only rule is whether the tile walkability check applies —
//! no altitude, no air combat, no anti-air.

use core::num::NonZeroU32;

use serde::{Deserialize, Serialize};

/// How an entity traverses the ground plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement {
    /// Bound to walkable tiles.
    Grounded,
    /// Free of the terrain grid's walkability constraint.
    Flying,
}

impl Movement {
    /// Whether traversal is constrained by the terrain grid's walkability.
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

/// An entity's per-step movement capability — the transient, effect-modulated
/// classification of how far it may step this action. Derived each tick from an
/// entity's active effects (in [`crate::services::effects`]) and supplied to the
/// movement decision as a plain input, so the movement and AI services stay
/// effect-unaware. A pure movement-capability vocabulary; no effect knowledge
/// leaks into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Mobility {
    /// Unhindered — steps at the base speed.
    Free,
    /// Slowed — steps at the base speed scaled down by `ratio`.
    Slowed {
        /// The fraction of the base step speed a step is scaled to.
        ratio: SlowRatio,
    },
    /// Immobilized — no step is taken at all.
    Immobilized,
}

/// A movement slow as an exact integer ratio of a base step speed —
/// `num`/`den`, held with a non-zero denominator so a zero-division slow is
/// unrepresentable and no float ever enters. The single slow this wave carries
/// is the Iced half-speed [`SlowRatio::HALVED`]; a movement service applies it
/// to the base speed *it* owns (via the shared integer-ratio primitive), so this
/// carries the fraction and never the base magnitude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlowRatio {
    num: u32,
    den: NonZeroU32,
}

impl SlowRatio {
    /// The Iced slow: movement at ×1/2 the base step speed.
    pub const HALVED: Self = Self {
        num: 1,
        den: NonZeroU32::MIN.saturating_add(1),
    };

    /// The ratio numerator.
    #[must_use]
    pub fn num(self) -> u32 {
        self.num
    }

    /// The ratio denominator — non-zero by construction.
    #[must_use]
    pub fn den(self) -> NonZeroU32 {
        self.den
    }
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
