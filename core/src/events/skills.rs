//! The outcome of one skill cast, kind-tagged: a rejection that spent nothing,
//! or a resolved cast with its per-target hits and the caster's resulting
//! placement (a lunge dashes the caster). One service
//! ([`crate::services::skills::cast`]), one outcome enum.

use serde::{Deserialize, Serialize};

use crate::components::active_effect::ActiveEffects;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::data::effects::Ailment;
use crate::events::combat::AttackOutcome;

/// What a skill cast produced, kind-tagged: rejected before spending anything,
/// or cast — carrying the caster's resulting placement and one [`TargetHit`] per
/// struck target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillOutcome {
    /// The cast was rejected; no resource was spent and no target was touched.
    Rejected {
        /// Why the cast was rejected.
        reason: CastRejection,
    },
    /// The cast resolved.
    Cast {
        /// Where the caster stands after the cast (a lunge dashes the caster;
        /// otherwise unchanged).
        caster_placement: Placement,
        /// One hit per struck target, in target-batch order.
        hits: Vec<TargetHit>,
    },
}

/// Why a cast was rejected — the first failing precondition, checked before any
/// resource is spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CastRejection {
    /// The caster lacked the mana the skill costs.
    InsufficientMana,
    /// The caster lacked the ability (AG) the skill costs.
    InsufficientAbility,
    /// A single-target skill was aimed beyond its range.
    OutOfRange,
    /// No target fell inside the skill's region.
    NoTargetsInRegion,
}

/// One struck target's result. `target_index` is the position of the target in
/// the batch the caller supplied (the Nth candidate `CombatTarget`), not any
/// host identity. `inflicted` and `displacement` are genuine optionality — a hit
/// need not inflict an ailment, and need not move the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetHit {
    /// Index of the struck target in the supplied batch.
    pub target_index: usize,
    /// The strike outcome.
    pub outcome: AttackOutcome,
    /// The target's health after the strike.
    pub health: Pool,
    /// The target's timed effects after the strike — the authoritative store the
    /// host persists. A lethal strike clears it to [`ActiveEffects::EMPTY`] (every
    /// effect is `StopByDeath`); a non-lethal strike leaves it unchanged, and any
    /// newly inflicted ailment is reported separately in `inflicted`.
    pub active_effects: ActiveEffects,
    /// The ailment inflicted, if any.
    pub inflicted: Option<Ailment>,
    /// The target's new placement, if the hit displaced it (knockback).
    pub displacement: Option<Placement>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;
    use crate::events::combat::{Damage, DamageModifiers, Hit, HitQuality};

    fn placement() -> Placement {
        Placement {
            position: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        }
    }

    #[test]
    fn rejection_wire_pins() {
        let rejected = SkillOutcome::Rejected {
            reason: CastRejection::InsufficientMana,
        };
        assert_eq!(
            serde_json::to_string(&rejected).unwrap(),
            r#"{"kind":"rejected","reason":"insufficient_mana"}"#
        );
        assert_eq!(
            serde_json::from_str::<SkillOutcome>(
                r#"{"kind":"rejected","reason":"no_targets_in_region"}"#
            )
            .unwrap(),
            SkillOutcome::Rejected {
                reason: CastRejection::NoTargetsInRegion
            }
        );
    }

    #[test]
    fn cast_round_trips_with_hits() {
        let cast = SkillOutcome::Cast {
            caster_placement: placement(),
            hits: vec![TargetHit {
                target_index: 0,
                outcome: AttackOutcome::Landed {
                    hit: Hit {
                        damage: Damage(7),
                        quality: HitQuality::Normal,
                        modifiers: DamageModifiers::NONE,
                    },
                },
                health: Pool::new(20, 60).unwrap(),
                active_effects: crate::components::active_effect::ActiveEffects::EMPTY,
                inflicted: Some(Ailment::Frozen),
                displacement: Some(placement()),
            }],
        };
        let json = serde_json::to_string(&cast).unwrap();
        assert!(json.starts_with(r#"{"kind":"cast""#));
        assert_eq!(serde_json::from_str::<SkillOutcome>(&json).unwrap(), cast);
    }
}
