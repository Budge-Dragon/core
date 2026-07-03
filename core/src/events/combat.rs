//! The outcome of one resolved physical strike, kind-tagged. A miss carries
//! nothing; a landed or lethal hit carries the resolved [`Hit`] — its damage,
//! its quality tier, and the total set of damage modifiers that applied. One
//! service ([`crate::services::combat::resolve_attack`]), one outcome enum.

use serde::{Deserialize, Serialize};

/// Resolved damage dealt by a strike — a bare integer on the wire, like
/// [`crate::components::units::Zen`] and [`crate::components::units::Exp`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Damage(
    /// The damage amount.
    pub u32,
);

/// The quality tier of a landed hit — the three mutually exclusive outcomes of
/// the excellent/critical rolls. Serialized as a bare `snake_case` string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitQuality {
    /// An ordinary hit.
    Normal,
    /// A critical hit — damage equals the attacker's maximum.
    Critical,
    /// An excellent hit — damage is `6/5` of the attacker's maximum.
    Excellent,
}

/// One membership of the damage-modifier set — the vocabulary a
/// [`DamageModifiers`] admits. Serialized as a `snake_case` name in the set's
/// wire array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageModifier {
    /// The hit ignored the target's defense.
    DefenseIgnored,
    /// The hit dealt double damage.
    Doubled,
}

impl DamageModifier {
    /// The vocabulary in declaration order — a fixed-length array, so a new
    /// modifier breaks its length and every match keyed by it.
    const ALL: [Self; 2] = [Self::DefenseIgnored, Self::Doubled];
}

/// The total damage-modifier set that applied to a hit — the
/// [`crate::components::class::ClassSet`] pattern over the two-member modifier
/// vocabulary: a named bool per member, an exhaustive `contains`, and a wire
/// form that is an array of member names. A duplicated entry is a parse error;
/// the empty array is the legal none set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<DamageModifier>", into = "Vec<DamageModifier>")]
pub struct DamageModifiers {
    /// Whether the defense-ignored modifier applied.
    pub defense_ignored: bool,
    /// Whether the doubled modifier applied.
    pub doubled: bool,
}

impl DamageModifiers {
    /// The empty set — no modifier applied. A real domain value (an ordinary
    /// hit), not a fabricated default.
    pub const NONE: Self = Self {
        defense_ignored: false,
        doubled: false,
    };

    /// Total membership query — exhaustive match over the vocabulary.
    #[must_use]
    pub fn contains(self, modifier: DamageModifier) -> bool {
        match modifier {
            DamageModifier::DefenseIgnored => self.defense_ignored,
            DamageModifier::Doubled => self.doubled,
        }
    }

    /// Whether no modifier applied.
    #[must_use]
    pub fn is_empty(self) -> bool {
        !self.defense_ignored && !self.doubled
    }

    fn slot_mut(&mut self, modifier: DamageModifier) -> &mut bool {
        match modifier {
            DamageModifier::DefenseIgnored => &mut self.defense_ignored,
            DamageModifier::Doubled => &mut self.doubled,
        }
    }
}

impl TryFrom<Vec<DamageModifier>> for DamageModifiers {
    type Error = DuplicateModifierEntry;

    fn try_from(modifiers: Vec<DamageModifier>) -> Result<Self, Self::Error> {
        let mut set = Self::NONE;
        for modifier in modifiers {
            let slot = set.slot_mut(modifier);
            if *slot {
                return Err(DuplicateModifierEntry(modifier));
            }
            *slot = true;
        }
        Ok(set)
    }
}

impl From<DamageModifiers> for Vec<DamageModifier> {
    fn from(set: DamageModifiers) -> Self {
        DamageModifier::ALL
            .into_iter()
            .filter(|&modifier| set.contains(modifier))
            .collect()
    }
}

/// Parse failure: a modifier listed more than once in a modifier-set array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateModifierEntry(
    /// The modifier that appeared more than once.
    pub DamageModifier,
);

impl core::fmt::Display for DuplicateModifierEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "damage modifier listed more than once: {:?}", self.0)
    }
}

impl core::error::Error for DuplicateModifierEntry {}

/// A resolved hit: the damage dealt, its quality tier, and the modifiers that
/// applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// The damage dealt.
    pub damage: Damage,
    /// The quality tier.
    pub quality: HitQuality,
    /// The modifiers that applied.
    pub modifiers: DamageModifiers,
}

/// What one physical strike produced, kind-tagged: a miss, a landed hit that
/// left the target alive, or a lethal hit. A miss carries no fields; `Killed`
/// carries its [`Hit`] but no drop list — loot resolution is a separate service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttackOutcome {
    /// The strike missed; the target is unchanged.
    Missed,
    /// The strike landed and the target survived.
    Landed {
        /// The resolved hit.
        hit: Hit,
    },
    /// The strike landed and reduced the target's health to zero.
    Killed {
        /// The resolved hit.
        hit: Hit,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn damage_is_a_bare_integer_on_the_wire() {
        assert_eq!(serde_json::to_string(&Damage(42)).unwrap(), "42");
        assert_eq!(serde_json::from_str::<Damage>("42").unwrap(), Damage(42));
    }

    #[test]
    fn hit_quality_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&HitQuality::Excellent).unwrap(),
            r#""excellent""#
        );
        assert_eq!(
            serde_json::from_str::<HitQuality>(r#""critical""#).unwrap(),
            HitQuality::Critical
        );
    }

    #[test]
    fn modifiers_contains_is_total() {
        let set = DamageModifiers {
            defense_ignored: true,
            doubled: false,
        };
        assert!(set.contains(DamageModifier::DefenseIgnored));
        assert!(!set.contains(DamageModifier::Doubled));
        assert!(!set.is_empty());
        assert!(DamageModifiers::NONE.is_empty());
    }

    #[test]
    fn modifiers_serialize_as_a_name_array() {
        let set = DamageModifiers {
            defense_ignored: true,
            doubled: true,
        };
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["defense_ignored","doubled"]"#);
        assert_eq!(serde_json::from_str::<DamageModifiers>(&json).unwrap(), set);
        // The empty array is the legal none set.
        assert_eq!(
            serde_json::from_str::<DamageModifiers>("[]").unwrap(),
            DamageModifiers::NONE
        );
    }

    #[test]
    fn modifiers_reject_duplicates() {
        assert!(serde_json::from_str::<DamageModifiers>(r#"["doubled","doubled"]"#).is_err());
    }

    #[test]
    fn attack_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&AttackOutcome::Missed).unwrap(),
            r#"{"kind":"missed"}"#
        );
        let landed = AttackOutcome::Landed {
            hit: Hit {
                damage: Damage(12),
                quality: HitQuality::Normal,
                modifiers: DamageModifiers::NONE,
            },
        };
        assert_eq!(
            serde_json::to_string(&landed).unwrap(),
            r#"{"kind":"landed","hit":{"damage":12,"quality":"normal","modifiers":[]}}"#
        );
        let killed = AttackOutcome::Killed {
            hit: Hit {
                damage: Damage(99),
                quality: HitQuality::Critical,
                modifiers: DamageModifiers {
                    defense_ignored: true,
                    doubled: false,
                },
            },
        };
        let json = serde_json::to_string(&killed).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"killed","hit":{"damage":99,"quality":"critical","modifiers":["defense_ignored"]}}"#
        );
        assert_eq!(
            serde_json::from_str::<AttackOutcome>(&json).unwrap(),
            killed
        );
    }
}
