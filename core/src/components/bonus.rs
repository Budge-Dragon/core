//! The resolved-contribution vocabulary: `CombatBonus`, the one currency every
//! item/option/set/effect/pet producer emits, plus the small enumerated
//! `ConditionalSetBonus` residue whose effect depends on a runtime equipment
//! fact. A `CombatBonus` is always a concrete, already-scaled contribution —
//! the producing domain does its own scaling and emits a flat variant; there
//! is no operator, aggregate, or scaling vocabulary here.

use serde::{Deserialize, Serialize};

use crate::components::element::Element;
use crate::components::units::{Percent, Resistance};

/// One resolved contribution to a character's combat state. Closed set: every
/// variant has a confirmed pre-S3 producer; adding a variant is a conscious,
/// build-breaking act every consumer match must then handle. Serialized
/// kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CombatBonus {
    /// Added strength points.
    Strength {
        /// Points added.
        amount: u16,
    },
    /// Added agility points.
    Agility {
        /// Points added.
        amount: u16,
    },
    /// Added vitality points.
    Vitality {
        /// Points added.
        amount: u16,
    },
    /// Added energy points.
    Energy {
        /// Points added.
        amount: u16,
    },
    /// Added command points (Dark Lord gear only; the ancient Broy set's
    /// "Leadership" bonus is this variant).
    Command {
        /// Points added.
        amount: u16,
    },

    /// Added maximum health.
    MaxHealth {
        /// Health added.
        amount: u32,
    },
    /// Maximum health increased by a percentage.
    MaxHealthPct {
        /// Percent added.
        percent: Percent,
    },
    /// Added maximum mana.
    MaxMana {
        /// Mana added.
        amount: u32,
    },
    /// Maximum mana increased by a percentage.
    MaxManaPct {
        /// Percent added.
        percent: Percent,
    },
    /// Added maximum ability (AG).
    MaxAbility {
        /// Ability added.
        amount: u32,
    },
    /// Maximum ability increased by a percentage.
    MaxAbilityPct {
        /// Percent added.
        percent: Percent,
    },

    /// Health recovery rate increased by a percentage.
    HealthRecoveryPct {
        /// Percent added.
        percent: Percent,
    },
    /// Added ability (AG) recovery.
    AbilityRecovery {
        /// Recovery added.
        amount: u32,
    },

    /// Added defense.
    Defense {
        /// Defense added.
        amount: u32,
    },
    /// Defense increased by a percentage.
    DefensePct {
        /// Percent added.
        percent: Percent,
    },
    /// Added defense success rate.
    DefenseRate {
        /// Defense rate added.
        amount: u32,
    },
    /// Defense success rate increased by a percentage.
    DefenseRatePct {
        /// Percent added.
        percent: Percent,
    },

    /// Added attack success rate.
    AttackRate {
        /// Attack rate added.
        amount: u32,
    },
    /// Added attack speed.
    AttackSpeed {
        /// Attack speed added.
        amount: u16,
    },
    /// Added physical damage.
    PhysicalDamage {
        /// Damage added.
        amount: u32,
    },
    /// Added minimum physical damage.
    MinPhysicalDamage {
        /// Minimum damage added.
        amount: u32,
    },
    /// Added maximum physical damage.
    MaxPhysicalDamage {
        /// Maximum damage added.
        amount: u32,
    },
    /// Added wizardry damage.
    WizardryDamage {
        /// Damage added.
        amount: u32,
    },
    /// Wizardry damage increased by a percentage.
    WizardryDamagePct {
        /// Percent added.
        percent: Percent,
    },
    /// Added skill damage.
    SkillDamage {
        /// Damage added.
        amount: u32,
    },
    /// Added flat final damage.
    Damage {
        /// Damage added.
        amount: u32,
    },
    /// Two-handed-weapon damage increased by a percentage (conditional on the
    /// equipped-weapons view at application, not at aggregation).
    TwoHandedWeaponDamagePct {
        /// Percent added.
        percent: Percent,
    },
    /// Outgoing damage increased by a percentage (folds multiplicatively).
    DamagePct {
        /// Percent added.
        percent: Percent,
    },

    /// Critical-hit chance increased by a percentage.
    CriticalChancePct {
        /// Percent added.
        percent: Percent,
    },
    /// Added critical damage.
    CriticalDamage {
        /// Damage added.
        amount: u32,
    },
    /// Excellent-hit chance increased by a percentage.
    ExcellentChancePct {
        /// Percent added.
        percent: Percent,
    },
    /// Added excellent damage.
    ExcellentDamage {
        /// Damage added.
        amount: u32,
    },
    /// Double-damage chance increased by a percentage.
    DoubleDamageChancePct {
        /// Percent added.
        percent: Percent,
    },
    /// Defense-ignore chance increased by a percentage.
    DefenseIgnoreChancePct {
        /// Percent added.
        percent: Percent,
    },

    /// Incoming damage reduced by a percentage (folds multiplicatively).
    IncomingDamagePct {
        /// Percent reduced.
        percent: Percent,
    },
    /// A percentage of damage reflected to the attacker.
    DamageReflectPct {
        /// Percent reflected.
        percent: Percent,
    },

    /// Recover a fixed fraction of maximum health after a monster kill.
    HealthPerKill,
    /// Recover a fixed fraction of maximum mana after a monster kill.
    ManaPerKill,

    /// Zen acquisition increased by a percentage.
    ZenDropPct {
        /// Percent added.
        percent: Percent,
    },

    /// Added resistance to one element.
    ElementalResistance {
        /// The element resisted.
        element: Element,
        /// Resistance added, in the byte-over-255 unit.
        amount: Resistance,
    },
    /// Added damage of one element. Review: the only producer is s6-dataset
    /// ancient jewelry — flagged pending an authentic pre-S3 source.
    ElementalDamage {
        /// The damage element.
        element: Element,
        /// Damage added.
        amount: u32,
    },
}

/// The exhaustively enumerated conditional set bonuses — the only sanctioned
/// residue outside the single `CombatBonus` vocabulary. Its members' effects
/// depend on a runtime equipment fact and therefore cannot be resolved
/// contributions at load. Serialized kind-tagged, in a namespace disjoint from
/// `CombatBonus`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConditionalSetBonus {
    /// Defense increased by a percentage while a shield is equipped.
    DefenseWithShieldPct {
        /// Percent added when the condition holds.
        percent: Percent,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pct(points: u8) -> Percent {
        Percent::new(points).unwrap()
    }

    #[test]
    fn amount_variant_is_kind_tagged() {
        let bonus = CombatBonus::Defense { amount: 4 };
        assert_eq!(
            serde_json::to_string(&bonus).unwrap(),
            r#"{"kind":"defense","amount":4}"#
        );
    }

    #[test]
    fn min_physical_damage_wire_shape() {
        let bonus = CombatBonus::MinPhysicalDamage { amount: 5 };
        assert_eq!(
            serde_json::to_string(&bonus).unwrap(),
            r#"{"kind":"min_physical_damage","amount":5}"#
        );
    }

    #[test]
    fn percent_variant_serializes_bare_integer() {
        let bonus = CombatBonus::CriticalChancePct { percent: pct(5) };
        assert_eq!(
            serde_json::to_string(&bonus).unwrap(),
            r#"{"kind":"critical_chance_pct","percent":5}"#
        );
    }

    #[test]
    fn unit_variant_carries_only_its_kind() {
        assert_eq!(
            serde_json::to_string(&CombatBonus::HealthPerKill).unwrap(),
            r#"{"kind":"health_per_kill"}"#
        );
    }

    #[test]
    fn elemental_resistance_round_trips() {
        let bonus = CombatBonus::ElementalResistance {
            element: Element::Ice,
            amount: Resistance(25),
        };
        let json = serde_json::to_string(&bonus).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"elemental_resistance","element":"ice","amount":25}"#
        );
        assert_eq!(serde_json::from_str::<CombatBonus>(&json).unwrap(), bonus);
    }

    #[test]
    fn command_is_the_leadership_bonus() {
        let bonus = CombatBonus::Command { amount: 20 };
        assert_eq!(
            serde_json::to_string(&bonus).unwrap(),
            r#"{"kind":"command","amount":20}"#
        );
    }

    #[test]
    fn conditional_residue_has_a_disjoint_namespace() {
        let residue = ConditionalSetBonus::DefenseWithShieldPct { percent: pct(5) };
        let json = serde_json::to_string(&residue).unwrap();
        assert_eq!(json, r#"{"kind":"defense_with_shield_pct","percent":5}"#);
        // The residue kind is not a CombatBonus kind — the split is unambiguous.
        assert!(serde_json::from_str::<CombatBonus>(&json).is_err());
        assert_eq!(
            serde_json::from_str::<ConditionalSetBonus>(&json).unwrap(),
            residue
        );
    }
}
