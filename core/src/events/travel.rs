//! The outcome and projection types of the travel services: the warp-command
//! decision, the per-entry warp-menu availability, the enter-gate traversal,
//! and the Town Portal Scroll. Each command outcome is a flat kind-tagged
//! value returned by its service; the projection is a pure per-entry
//! annotation, one status per surviving warp entry. Data only — the decisions
//! live in [`crate::services::travel`].

use serde::{Deserialize, Serialize};

use crate::components::collections::OneOrMore;
use crate::components::placement::Placement;
use crate::components::units::{CarriedZen, Zen};
use crate::data::gates_warps::WarpIndex;

/// One unmet warp requirement, kind-tagged. The projection lists every unmet
/// one in authentic check order (discovery → level → wings → zen); the
/// command rejects on the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WarpLockReason {
    /// The target map is absent from the character's discovered set.
    NotDiscovered,
    /// The character's level is below the entry's class-effective requirement.
    LevelTooLow {
        /// The class-effective level requirement this character faces.
        required: u16,
    },
    /// The destination is a Sky map and the character wears no wings.
    CannotFly,
    /// The character cannot afford the entry's flat fee.
    InsufficientZen {
        /// The flat fee — never fraction-reduced.
        cost: Zen,
    },
}

/// Whether one warp entry is open to the character, kind-tagged. A locked
/// entry carries the complete set of unmet requirements, in check order, so a
/// client renders the truthful reasons without re-deriving any rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WarpAvailability {
    /// Every requirement is met; the warp would execute.
    Available,
    /// One or more requirements are unmet; the warp would be refused.
    Locked {
        /// Every unmet requirement, in check order — never empty.
        reasons: OneOrMore<WarpLockReason>,
    },
}

/// One warp entry's availability annotation. Identity is the warp index alone;
/// the entry's static facts (fee, level, target) live in the warp list, not
/// duplicated into the per-character projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarpEntryStatus {
    /// The warp-list entry this status annotates.
    pub index: WarpIndex,
    /// Whether the entry is open to the character.
    pub availability: WarpAvailability,
}

/// What the warp command produced, kind-tagged: an arrival carrying the
/// server-computed values a client cannot recompute (the sampled landing and
/// the post-debit balance), or a typed refusal. Every refusal leaves the
/// wallet and placement untouched — the fee is charged last, atomically, only
/// on a seated arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WarpTravelOutcome {
    /// The traveler arrived and the fee was debited.
    Arrived {
        /// The sampled landing placement.
        placement: Placement,
        /// The wallet balance after the fee debit.
        balance: CarriedZen,
    },
    /// A dead character cannot warp; nothing charged, nothing moved.
    NotAlive,
    /// The target map is absent from the character's discovered set.
    NotDiscovered,
    /// The character's level is below the class-effective requirement.
    LevelTooLow {
        /// The class-effective level requirement this character faces.
        required: u16,
    },
    /// The destination is a Sky map and the character wears no wings; nothing
    /// charged, nothing moved.
    CannotFly,
    /// The character cannot afford the fee; the wallet is untouched.
    NotEnoughZen {
        /// The flat fee — never fraction-reduced.
        required: Zen,
        /// The unchanged carried balance.
        available: CarriedZen,
    },
    /// The target gate area holds no walkable tile; the traveler was not
    /// moved and nothing was charged.
    NoWalkableLanding,
}

/// What a Town Portal Scroll read produced, kind-tagged. The town-gate arrival
/// is total — the destination spawn gate's walkable landing set is proven
/// non-empty at parse, so no no-landing variant exists. Nothing is consumed on
/// any rejection; success consumes exactly one scroll piece.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TownPortalOutcome {
    /// The traveler was seated at the current map's town spawn gate — alive,
    /// vitals and effects untouched.
    Arrived {
        /// The sampled town-gate landing placement.
        placement: Placement,
    },
    /// A dead character's scroll does nothing and is not consumed.
    NotAlive,
    /// The addressed cell holds no usable town-portal scroll — empty, a
    /// non-consumable, or the wrong consumable; one refusal for all three.
    NoScroll,
}

/// What an enter-gate traversal produced, kind-tagged. A dedicated enum:
/// enter gates charge nothing and are never gated by discovery, so no balance
/// and no discovery refusal are representable here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnterGateOutcome {
    /// The traveler stepped through and was seated on the landing.
    Arrived {
        /// The sampled landing placement.
        placement: Placement,
    },
    /// A dead character cannot traverse; nothing moved.
    NotAlive,
    /// The character's level is below the gate's class-effective requirement.
    LevelTooLow {
        /// The class-effective level requirement this character faces.
        required: u16,
    },
    /// The destination is a Sky map and the character wears no wings; nothing
    /// moved.
    CannotFly,
    /// The landing area holds no walkable tile; the traveler was not moved.
    NoWalkableLanding,
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
            map: MapNumber(4),
        }
    }

    const PLACEMENT_JSON: &str = r#"{"position":{"x":163840,"y":229376},"facing":{"x":0,"y":1},"movement":"grounded","map":4}"#;

    fn round_trips<T>(value: &T)
    where
        T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + core::fmt::Debug,
    {
        let json = serde_json::to_string(value).unwrap();
        assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), value);
    }

    #[test]
    fn warp_lock_reason_wire_pins() {
        assert_eq!(
            serde_json::to_string(&WarpLockReason::NotDiscovered).unwrap(),
            r#"{"kind":"not_discovered"}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpLockReason::LevelTooLow { required: 50 }).unwrap(),
            r#"{"kind":"level_too_low","required":50}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpLockReason::CannotFly).unwrap(),
            r#"{"kind":"cannot_fly"}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpLockReason::InsufficientZen { cost: Zen(5000) }).unwrap(),
            r#"{"kind":"insufficient_zen","cost":5000}"#
        );
        for reason in [
            WarpLockReason::NotDiscovered,
            WarpLockReason::LevelTooLow { required: 50 },
            WarpLockReason::CannotFly,
            WarpLockReason::InsufficientZen { cost: Zen(5000) },
        ] {
            round_trips(&reason);
        }
    }

    #[test]
    fn warp_availability_and_entry_status_wire_pins() {
        assert_eq!(
            serde_json::to_string(&WarpAvailability::Available).unwrap(),
            r#"{"kind":"available"}"#
        );
        let locked = WarpAvailability::Locked {
            reasons: OneOrMore::with_head(
                WarpLockReason::NotDiscovered,
                vec![WarpLockReason::InsufficientZen { cost: Zen(2000) }],
            ),
        };
        assert_eq!(
            serde_json::to_string(&locked).unwrap(),
            r#"{"kind":"locked","reasons":[{"kind":"not_discovered"},{"kind":"insufficient_zen","cost":2000}]}"#
        );
        round_trips(&locked);

        let status = WarpEntryStatus {
            index: WarpIndex(8),
            availability: WarpAvailability::Available,
        };
        assert_eq!(
            serde_json::to_string(&status).unwrap(),
            r#"{"index":8,"availability":{"kind":"available"}}"#
        );
        round_trips(&status);
    }

    #[test]
    fn warp_travel_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::Arrived {
                placement: placement(),
                balance: CarriedZen::new(5000).unwrap(),
            })
            .unwrap(),
            format!(r#"{{"kind":"arrived","placement":{PLACEMENT_JSON},"balance":5000}}"#)
        );
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::NotAlive).unwrap(),
            r#"{"kind":"not_alive"}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::NotDiscovered).unwrap(),
            r#"{"kind":"not_discovered"}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::LevelTooLow { required: 33 }).unwrap(),
            r#"{"kind":"level_too_low","required":33}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::CannotFly).unwrap(),
            r#"{"kind":"cannot_fly"}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::NotEnoughZen {
                required: Zen(5000),
                available: CarriedZen::new(4999).unwrap(),
            })
            .unwrap(),
            r#"{"kind":"not_enough_zen","required":5000,"available":4999}"#
        );
        assert_eq!(
            serde_json::to_string(&WarpTravelOutcome::NoWalkableLanding).unwrap(),
            r#"{"kind":"no_walkable_landing"}"#
        );
        for outcome in [
            WarpTravelOutcome::Arrived {
                placement: placement(),
                balance: CarriedZen::new(5000).unwrap(),
            },
            WarpTravelOutcome::NotAlive,
            WarpTravelOutcome::NotDiscovered,
            WarpTravelOutcome::LevelTooLow { required: 33 },
            WarpTravelOutcome::CannotFly,
            WarpTravelOutcome::NotEnoughZen {
                required: Zen(5000),
                available: CarriedZen::new(4999).unwrap(),
            },
            WarpTravelOutcome::NoWalkableLanding,
        ] {
            round_trips(&outcome);
        }
    }

    #[test]
    fn town_portal_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&TownPortalOutcome::Arrived {
                placement: placement()
            })
            .unwrap(),
            format!(r#"{{"kind":"arrived","placement":{PLACEMENT_JSON}}}"#)
        );
        assert_eq!(
            serde_json::to_string(&TownPortalOutcome::NotAlive).unwrap(),
            r#"{"kind":"not_alive"}"#
        );
        assert_eq!(
            serde_json::to_string(&TownPortalOutcome::NoScroll).unwrap(),
            r#"{"kind":"no_scroll"}"#
        );
        for outcome in [
            TownPortalOutcome::Arrived {
                placement: placement(),
            },
            TownPortalOutcome::NotAlive,
            TownPortalOutcome::NoScroll,
        ] {
            round_trips(&outcome);
        }
    }

    #[test]
    fn enter_gate_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&EnterGateOutcome::Arrived {
                placement: placement()
            })
            .unwrap(),
            format!(r#"{{"kind":"arrived","placement":{PLACEMENT_JSON}}}"#)
        );
        assert_eq!(
            serde_json::to_string(&EnterGateOutcome::NotAlive).unwrap(),
            r#"{"kind":"not_alive"}"#
        );
        assert_eq!(
            serde_json::to_string(&EnterGateOutcome::LevelTooLow { required: 40 }).unwrap(),
            r#"{"kind":"level_too_low","required":40}"#
        );
        assert_eq!(
            serde_json::to_string(&EnterGateOutcome::CannotFly).unwrap(),
            r#"{"kind":"cannot_fly"}"#
        );
        assert_eq!(
            serde_json::to_string(&EnterGateOutcome::NoWalkableLanding).unwrap(),
            r#"{"kind":"no_walkable_landing"}"#
        );
        for outcome in [
            EnterGateOutcome::Arrived {
                placement: placement(),
            },
            EnterGateOutcome::NotAlive,
            EnterGateOutcome::LevelTooLow { required: 40 },
            EnterGateOutcome::CannotFly,
            EnterGateOutcome::NoWalkableLanding,
        ] {
            round_trips(&outcome);
        }
    }
}
