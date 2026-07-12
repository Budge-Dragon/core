//! The outcome of one skill cast, kind-tagged: a rejection that spent nothing,
//! or a resolved cast with its per-target hits and the caster's resulting
//! placement (a lunge teleports the caster onto its target). One service
//! ([`crate::services::skills::cast`]), one outcome enum.

use serde::{Deserialize, Serialize};

use crate::components::active_effect::ActiveEffects;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::data::effects::Ailment;
use crate::events::combat::Hit;

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
        /// Where the caster stands after the cast (a lunge teleports the
        /// caster onto its target; otherwise unchanged).
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
    /// The caster is dead (in the death→respawn window) — a corpse casts
    /// nothing. Checked before every other precondition, so no resource is
    /// spent and no displacement lands.
    CasterNotAlive,
    /// The caster stood on a safe town tile — offensive casting is forbidden
    /// there. Checked first, before any spend or aim gate.
    CasterInSafezone,
    /// The caster lacked the mana the skill costs.
    InsufficientMana,
    /// The caster lacked the ability (AG) the skill costs.
    InsufficientAbility,
    /// A single-target or aim-centered area skill was aimed beyond its range.
    OutOfRange,
    /// No target fell inside the skill's region.
    NoTargetsInRegion,
}

/// One struck target's result, kind-tagged, mirroring
/// [`crate::events::combat::AttackOutcome`]'s missed/landed/killed split so the
/// two agree by construction. `target_index` is the position of the target in
/// the batch the caller supplied (the Nth candidate `CombatTarget`), not any
/// host identity. The ailment exists only on [`TargetHit::Landed`]; a
/// displacement exists on [`TargetHit::Landed`] and [`TargetHit::Missed`] (the
/// pre-roll displacements — an Earthshake push or a lunge jiggle — move even a
/// missed target) and stays optional there; [`TargetHit::Killed`] carries none
/// (a kill is never pushed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TargetHit {
    /// The strike missed; the target is unchanged in health/effects but a
    /// pre-roll displacement (an Earthshake push or a lunge jiggle) may still
    /// have moved it — `None` when nothing displaced it, mirroring `Landed`'s
    /// field.
    Missed {
        /// Index of the struck target in the supplied batch.
        target_index: usize,
        /// The target's health after the strike (a miss leaves it unchanged).
        health: Pool,
        /// The target's timed effects after the strike — the authoritative
        /// store the host persists; a miss leaves it unchanged.
        active_effects: ActiveEffects,
        /// The target's new placement, if a pre-roll displacement moved it.
        displacement: Option<Placement>,
    },
    /// The strike landed and the target survived.
    Landed {
        /// Index of the struck target in the supplied batch.
        target_index: usize,
        /// The resolved hit.
        hit: Hit,
        /// The target's health after the strike.
        health: Pool,
        /// The target's timed effects after the strike — the authoritative
        /// store the host persists, unchanged by the strike itself; any newly
        /// inflicted ailment is reported separately in `inflicted`.
        active_effects: ActiveEffects,
        /// The ailment inflicted, if any.
        inflicted: Option<Ailment>,
        /// The target's new placement, if the hit displaced it (an Earthshake
        /// push or a jiggle).
        displacement: Option<Placement>,
    },
    /// The strike landed and reduced the target's health to zero.
    Killed {
        /// Index of the struck target in the supplied batch.
        target_index: usize,
        /// The resolved hit.
        hit: Hit,
        /// The target's health after the strike.
        health: Pool,
        /// The target's timed effects after the strike — the authoritative
        /// store the host persists; a lethal strike clears it to
        /// [`ActiveEffects::EMPTY`] (every effect is `StopByDeath`).
        active_effects: ActiveEffects,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;
    use crate::events::combat::{Damage, DamageModifiers, HitQuality};

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
        assert_eq!(
            serde_json::to_string(&CastRejection::CasterInSafezone).unwrap(),
            r#""caster_in_safezone""#
        );
        assert_eq!(
            serde_json::from_str::<CastRejection>(r#""caster_in_safezone""#).unwrap(),
            CastRejection::CasterInSafezone
        );
    }

    #[test]
    fn cast_round_trips_with_hits() {
        let cast = SkillOutcome::Cast {
            caster_placement: placement(),
            hits: vec![
                TargetHit::Landed {
                    target_index: 0,
                    hit: Hit {
                        damage: Damage(7),
                        quality: HitQuality::Normal,
                        modifiers: DamageModifiers::NONE,
                    },
                    health: Pool::new(20, 60).unwrap(),
                    active_effects: ActiveEffects::EMPTY,
                    inflicted: Some(Ailment::Frozen),
                    displacement: Some(placement()),
                },
                TargetHit::Missed {
                    target_index: 1,
                    health: Pool::new(35, 35).unwrap(),
                    active_effects: ActiveEffects::EMPTY,
                    displacement: None,
                },
                // A pushed miss: the pre-roll displacement moved the target.
                TargetHit::Missed {
                    target_index: 3,
                    health: Pool::new(35, 35).unwrap(),
                    active_effects: ActiveEffects::EMPTY,
                    displacement: Some(placement()),
                },
                TargetHit::Killed {
                    target_index: 2,
                    hit: Hit {
                        damage: Damage(50),
                        quality: HitQuality::Critical,
                        modifiers: DamageModifiers::NONE,
                    },
                    health: Pool::new(0, 40).unwrap(),
                    active_effects: ActiveEffects::EMPTY,
                },
            ],
        };
        let json = serde_json::to_string(&cast).unwrap();
        assert!(json.starts_with(r#"{"kind":"cast""#));
        assert_eq!(serde_json::from_str::<SkillOutcome>(&json).unwrap(), cast);
    }

    #[test]
    fn missed_serializes_its_displacement_present_or_null() {
        let pushed = TargetHit::Missed {
            target_index: 1,
            health: Pool::new(300, 300).unwrap(),
            active_effects: ActiveEffects::EMPTY,
            displacement: Some(placement()),
        };
        let pushed_json = serde_json::to_string(&pushed).unwrap();
        assert!(pushed_json.contains(r#""displacement":{"position""#));
        assert_eq!(
            serde_json::from_str::<TargetHit>(&pushed_json).unwrap(),
            pushed
        );

        let ordinary = TargetHit::Missed {
            target_index: 0,
            health: Pool::new(35, 35).unwrap(),
            active_effects: ActiveEffects::EMPTY,
            displacement: None,
        };
        let ordinary_json = serde_json::to_string(&ordinary).unwrap();
        assert!(ordinary_json.contains(r#""displacement":null"#));
        assert_eq!(
            serde_json::from_str::<TargetHit>(&ordinary_json).unwrap(),
            ordinary
        );
    }

    #[test]
    fn killed_carries_no_displacement_key() {
        let killed = TargetHit::Killed {
            target_index: 2,
            hit: Hit {
                damage: Damage(50),
                quality: HitQuality::Critical,
                modifiers: DamageModifiers::NONE,
            },
            health: Pool::new(0, 40).unwrap(),
            active_effects: ActiveEffects::EMPTY,
        };
        let json = serde_json::to_string(&killed).unwrap();
        assert!(!json.contains("displacement"));
    }
}
