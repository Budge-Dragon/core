//! Record shape of `skills.json` — the pre-S3 skill roster.

use serde::{Deserialize, Serialize};

use crate::components::class::ClassSet;
use crate::components::element::Element;
use crate::components::spatial::ConeHalfWidth;

use super::common::{MonsterNumber, Provenance, SkillNumber};
use super::effects::{Ailment, Buff};

/// One skill definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Skill {
    /// Skill number as the client knows it (Skill.txt index).
    pub number: SkillNumber,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Base skill damage (Skill.txt Damage column); 0 for weapon-carried and
    /// non-damage skills.
    pub attack_damage: u16,
    /// Which damage calculation the skill uses.
    pub damage_type: DamageType,
    /// Elemental affinity; absent = non-elemental.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<Element>,
    /// Status ailment a successful hit may inflict, gated by the target's
    /// elemental resistance; absent = the hit inflicts nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inflicts: Option<Ailment>,
    /// Maximum cast distance in tiles; 0 = caster-centered / self.
    pub range: u8,
    /// What the skill does, kind-tagged.
    pub shape: SkillShape,
    /// Resources consumed per cast.
    pub cost: CastCost,
    /// Minimum stats required to learn the skill.
    pub learn: LearnRequirement,
    /// Classes able to learn the skill — total per-class set, serialized as a
    /// list of class names; all-false = monster-only.
    pub classes: ClassSet,
}

/// Which damage calculation a skill uses (Skill.txt damage-type fact).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    /// Deals no damage.
    None,
    /// Physical damage.
    Physical,
    /// Wizardry damage.
    Wizardry,
}

/// Resources consumed per cast (Skill.txt Mana and BP/AG columns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CastCost {
    /// Mana consumed; 0 = none.
    pub mana: u16,
    /// Ability (AG) consumed; 0 = none.
    pub ability: u16,
}

/// Minimum stats required to learn a skill (Skill.txt `ReqLevel` /
/// `ReqEnergy` / `ReqCharisma` columns; `ReqCharisma` is the Command stat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnRequirement {
    /// Minimum character level; 0 = none.
    pub level: u16,
    /// Minimum energy; 0 = none.
    pub energy: u16,
    /// Minimum command; 0 = none.
    pub command: u16,
}

/// What a skill does, kind-tagged. One variant per behavior family the game
/// exhibits; per-variant execution lives in `services`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkillShape {
    /// Single hit on the chosen target.
    DirectHit,
    /// Direct hit where the attacker lunges to the target and the victim is
    /// knocked to a nearby tile (DK weapon skills).
    Lunge,
    /// Multi-target strike over an authored region, with a per-skill
    /// displacement.
    Area {
        /// The region the strike covers.
        geometry: AreaGeometry,
        /// A special displacement applied to each struck target.
        displacement: AreaDisplacement,
    },
    /// Buff the caster applies to themself.
    BuffSelf {
        /// The applied buff.
        buff: Buff,
    },
    /// Buff cast on any player, including the caster.
    BuffPlayer {
        /// The applied buff.
        buff: Buff,
    },
    /// Buff cast on one party member.
    BuffPartyMember {
        /// The applied buff.
        buff: Buff,
    },
    /// Buff applied to every party member in view range.
    BuffParty {
        /// The applied buff.
        buff: Buff,
    },
    /// Restores a player target's health.
    Heal,
    /// Summons a monster that fights for the caster.
    Summon {
        /// The summoned monster.
        monster: MonsterNumber,
    },
    /// Relocates the caster to a chosen walkable tile.
    Teleport,
    /// Begins charging Nova; the release is the Nova skill record.
    NovaCharge,
    /// Teleports the caster's party members to the caster (Dark Lord Summon).
    RecallParty,
}

/// The authored spatial shape of an area skill — a flat per-family enum, sized
/// at half-tile (×2 integer) grain (the `staff_rise_x2` convention). The family
/// alone fixes aim-centeredness: `AimCircle` is aimed (range-gated); the other
/// three are caster-anchored (the aim is never consulted). Magnitudes are
/// authored, never derived from `range`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AreaGeometry {
    /// A disc of radius `radius_x2` half-tiles centred on the aim point.
    AimCircle {
        /// The disc radius in half-tiles.
        radius_x2: u8,
    },
    /// A disc of radius `radius_x2` half-tiles centred on the caster.
    CasterCircle {
        /// The disc radius in half-tiles.
        radius_x2: u8,
    },
    /// A frontal cone of length `length_x2` half-tiles at the caster's facing,
    /// with the exact squared-cosine half-angle.
    Cone {
        /// The cone length in half-tiles.
        length_x2: u8,
        /// The cone half-angle as an exact squared cosine.
        half_angle: ConeHalfWidth,
    },
    /// A forward rectangle `length_x2` half-tiles long and `half_width_x2`
    /// half-tiles to each side, along the caster's facing.
    Beam {
        /// The rectangle length in half-tiles.
        length_x2: u8,
        /// The rectangle half-width in half-tiles, to each side of the axis.
        half_width_x2: u8,
    },
}

/// A per-skill special displacement its struck targets take, authored on the
/// area shape. Almost every area skill displaces nothing (`None`); Earthshake
/// alone throws its targets away from the caster (`DirectionalPush`). Authored
/// data, so no service infers a push from a skill's name and no lightning tag
/// decides it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AreaDisplacement {
    /// No special displacement; a lightning-element hit still jiggles via the
    /// generic elemental modifier, driven by `element`, not by this field.
    None,
    /// The struck target is thrown up to three tiles 8-way away from the caster
    /// (Earthshake's quake).
    DirectionalPush,
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::num::NonZeroU64;

    #[test]
    fn area_geometry_round_trips_kind_tagged_snake_case() {
        let cases = [
            (
                AreaGeometry::AimCircle { radius_x2: 2 },
                r#"{"kind":"aim_circle","radius_x2":2}"#,
            ),
            (
                AreaGeometry::CasterCircle { radius_x2: 12 },
                r#"{"kind":"caster_circle","radius_x2":12}"#,
            ),
            (
                AreaGeometry::Cone {
                    length_x2: 14,
                    half_angle: ConeHalfWidth::new(196, NonZeroU64::new(277).unwrap()).unwrap(),
                },
                r#"{"kind":"cone","length_x2":14,"half_angle":{"num":196,"den":277}}"#,
            ),
            (
                AreaGeometry::Beam {
                    length_x2: 8,
                    half_width_x2: 3,
                },
                r#"{"kind":"beam","length_x2":8,"half_width_x2":3}"#,
            ),
        ];
        for (geometry, wire) in cases {
            assert_eq!(serde_json::to_string(&geometry).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<AreaGeometry>(wire).unwrap(),
                geometry
            );
        }
    }

    #[test]
    fn cone_half_angle_survives_the_exact_ratio() {
        let wire = r#"{"kind":"cone","length_x2":14,"half_angle":{"num":196,"den":277}}"#;
        let geometry = serde_json::from_str::<AreaGeometry>(wire).unwrap();
        let AreaGeometry::Cone { half_angle, .. } = geometry else {
            panic!("expected a cone");
        };
        assert_eq!(half_angle.num(), 196);
        assert_eq!(half_angle.den_get(), 277);
        assert_ne!(half_angle, ConeHalfWidth::DEG_45);
    }

    #[test]
    fn the_flat_geometry_enum_admits_no_all_null_shape() {
        // A variant missing its own size field is rejected at parse; no
        // optional-soup shape with null magnitudes exists.
        assert!(serde_json::from_str::<AreaGeometry>(r#"{"kind":"aim_circle"}"#).is_err());
        assert!(serde_json::from_str::<AreaGeometry>(r#"{"kind":"beam","length_x2":4}"#).is_err());
    }

    #[test]
    fn area_displacement_round_trips_as_a_bare_string() {
        assert_eq!(
            serde_json::to_string(&AreaDisplacement::None).unwrap(),
            r#""none""#
        );
        assert_eq!(
            serde_json::to_string(&AreaDisplacement::DirectionalPush).unwrap(),
            r#""directional_push""#
        );
        assert_eq!(
            serde_json::from_str::<AreaDisplacement>(r#""directional_push""#).unwrap(),
            AreaDisplacement::DirectionalPush
        );
    }

    #[test]
    fn area_shape_carries_geometry_and_displacement_side_by_side() {
        let shape = SkillShape::Area {
            geometry: AreaGeometry::CasterCircle { radius_x2: 10 },
            displacement: AreaDisplacement::DirectionalPush,
        };
        let wire = r#"{"kind":"area","geometry":{"kind":"caster_circle","radius_x2":10},"displacement":"directional_push"}"#;
        assert_eq!(serde_json::to_string(&shape).unwrap(), wire);
        assert_eq!(serde_json::from_str::<SkillShape>(wire).unwrap(), shape);
    }
}
