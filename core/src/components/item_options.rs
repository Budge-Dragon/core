//! The closed pre-S3 item-option families — normal, excellent (armor and
//! weapon), dinorant, second-wing, and the ancient per-piece bonus tier.
//! Each family is its own type; there is no unifying "option type" tag and no
//! generic option-definition record. Value vocabulary only — magnitudes and
//! their resolution to the resolved-contribution currency are not carried
//! here.

use serde::{Deserialize, Serialize};

/// A normal (Jewel-of-Life-leveled) item option. Which variants a given item
/// may roll is item data, not a fact of this enum; this type is only the
/// closed kind vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NormalOption {
    /// +4 physical damage per option level (weapons, Wings of Satan).
    PhysicalDamage,
    /// +4 wizardry damage per option level (staffs, Wings of Heaven).
    WizardryDamage,
    /// +4 defense per option level (armor).
    Defense,
    /// +5 defense success rate per option level (shields).
    DefenseRate,
    /// +1% health recovery per option level (rings/pendants, Wings of Elf).
    HealthRecoveryPct,
    /// +1% maximum mana per option level. Review: s6-only jewelry option
    /// backported with the 1.0-era Ring of Magic.
    MaxManaPct,
    /// +1% maximum ability (AG) per option level. Review: s6-only jewelry
    /// option backported with the 1.0-era Pendant of Ability.
    MaxAbilityPct,
}

/// The damage kind excellent weapon slots 4 and 5 boost (a pendant's kind is
/// item data: Pendant of Fire boosts physical, Pendant of Lighting wizardry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponDamageKind {
    /// Physical damage.
    Physical,
    /// Wizardry damage.
    Wizardry,
}

/// Which excellent option set an item rolls — a fact of the item's kind. The
/// damage kind is nested on the weapon variant, so a weapon option can never
/// be paired with the armor category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "set", rename_all = "snake_case")]
pub enum ExcellentCategory {
    /// The six armor effects (armor pieces, shields, rings).
    Armor,
    /// The six weapon effects; slots 4 and 5 boost this damage kind.
    Weapon {
        /// The damage kind the weapon's slots 4 and 5 boost.
        damage: WeaponDamageKind,
    },
}

/// The client's fixed 6-slot excellent-armor bitmask set (armor pieces,
/// shields, rings).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcellentArmorOption {
    /// Zen acquisition +40%.
    ZenGain,
    /// Defense success rate +10%.
    DefenseRate,
    /// Reflect 5% of damage.
    DamageReflect,
    /// Damage decrease 4%.
    DamageDecrease,
    /// Maximum mana +4%.
    MaxMana,
    /// Maximum health +4%.
    MaxHealth,
}

impl ExcellentArmorOption {
    /// Authentic client bitmask position `1..=6` (wire encoding fact).
    #[must_use]
    pub const fn slot_index(self) -> u8 {
        match self {
            Self::ZenGain => 1,
            Self::DefenseRate => 2,
            Self::DamageReflect => 3,
            Self::DamageDecrease => 4,
            Self::MaxMana => 5,
            Self::MaxHealth => 6,
        }
    }
}

/// The six excellent weapon effects — identical for physical weapons and
/// staffs; slots 4 and 5 apply to the weapon's own `WeaponDamageKind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcellentWeaponOption {
    /// Restore mana/8 after a monster kill.
    ManaAfterKill,
    /// Restore health/8 after a monster kill.
    HealthAfterKill,
    /// Attack speed +7.
    AttackSpeed,
    /// Weapon damage +2%.
    DamagePct,
    /// Weapon damage +character level / 20.
    DamagePerLevel,
    /// Excellent damage chance +10%.
    ExcellentDamageChance,
}

impl ExcellentWeaponOption {
    /// Authentic client bitmask position `1..=6` (wire encoding fact).
    #[must_use]
    pub const fn slot_index(self) -> u8 {
        match self {
            Self::ManaAfterKill => 1,
            Self::HealthAfterKill => 2,
            Self::AttackSpeed => 3,
            Self::DamagePct => 4,
            Self::DamagePerLevel => 5,
            Self::ExcellentDamageChance => 6,
        }
    }
}

/// The three Dinorant options rolled at chaos-machine creation (095d).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DinorantOption {
    /// Damage taken -5%.
    DamageAbsorb,
    /// Maximum ability (AG) +50.
    MaxAbility,
    /// Attack speed +5.
    AttackSpeed,
}

impl DinorantOption {
    /// Bitmask position `1..=3` — the augment-set encoding fact, the
    /// [`ExcellentArmorOption::slot_index`] grain.
    #[must_use]
    pub const fn slot_index(self) -> u8 {
        match self {
            Self::DamageAbsorb => 1,
            Self::MaxAbility => 2,
            Self::AttackSpeed => 3,
        }
    }
}

/// The 2nd-wing / cape bonus options ("excellent wing options"). Which values a
/// crafted item draws from is the chaos-machine service's fact: second wings
/// draw the three non-[`Command`](Self::Command) values, the Cape of Lord all
/// four. Review: 1.0-era content shipped only in OpenMU's s6 dataset. HP/mana
/// values follow the wing's +level by formula, not a per-level table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondWingBonus {
    /// Maximum health +(50 + 5*item level).
    MaxHealth,
    /// Maximum mana +(50 + 5*item level).
    MaxMana,
    /// Ignore defense with 3% chance.
    IgnoreDefenseChance,
    /// Command +10 (the cape-only fourth option). Review: 1.0-era content
    /// shipped only in OpenMU's s6 dataset, like its siblings.
    Command,
}

/// The two per-piece bonus tiers an ancient item rolls at creation — the
/// client's own 1|2 encoding in the ancient byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum AncientBonusLevel {
    /// Encoded 1: +5 of the piece's bonus stat.
    One,
    /// Encoded 2: +10 of the piece's bonus stat.
    Two,
}

impl AncientBonusLevel {
    /// The client-encoded value `1` or `2`.
    #[must_use]
    pub fn encoded(self) -> u8 {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

impl TryFrom<u8> for AncientBonusLevel {
    type Error = AncientBonusLevelOutOfRange;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::One),
            2 => Ok(Self::Two),
            value => Err(AncientBonusLevelOutOfRange { value }),
        }
    }
}

impl From<AncientBonusLevel> for u8 {
    fn from(level: AncientBonusLevel) -> Self {
        level.encoded()
    }
}

/// Parse failure: an ancient bonus tier byte outside the client's `1|2`
/// encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AncientBonusLevelOutOfRange {
    /// The rejected wire value.
    pub value: u8,
}

impl core::fmt::Display for AncientBonusLevelOutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ancient bonus tier {} is not 1 or 2", self.value)
    }
}

impl core::error::Error for AncientBonusLevelOutOfRange {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_option_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&NormalOption::DefenseRate).unwrap(),
            r#""defense_rate""#
        );
        assert_eq!(
            serde_json::from_str::<NormalOption>(r#""health_recovery_pct""#).unwrap(),
            NormalOption::HealthRecoveryPct
        );
    }

    #[test]
    fn excellent_category_nests_weapon_damage_kind() {
        let weapon = ExcellentCategory::Weapon {
            damage: WeaponDamageKind::Wizardry,
        };
        let json = serde_json::to_string(&weapon).unwrap();
        assert_eq!(json, r#"{"set":"weapon","damage":"wizardry"}"#);
        assert_eq!(
            serde_json::from_str::<ExcellentCategory>(&json).unwrap(),
            weapon
        );
        assert_eq!(
            serde_json::to_string(&ExcellentCategory::Armor).unwrap(),
            r#"{"set":"armor"}"#
        );
    }

    #[test]
    fn armor_slot_indices_are_the_client_bitmask_positions() {
        assert_eq!(ExcellentArmorOption::ZenGain.slot_index(), 1);
        assert_eq!(ExcellentArmorOption::MaxHealth.slot_index(), 6);
    }

    #[test]
    fn weapon_slot_indices_are_the_client_bitmask_positions() {
        assert_eq!(ExcellentWeaponOption::ManaAfterKill.slot_index(), 1);
        assert_eq!(ExcellentWeaponOption::ExcellentDamageChance.slot_index(), 6);
    }

    #[test]
    fn dinorant_slot_indices_are_the_augment_set_positions() {
        assert_eq!(DinorantOption::DamageAbsorb.slot_index(), 1);
        assert_eq!(DinorantOption::MaxAbility.slot_index(), 2);
        assert_eq!(DinorantOption::AttackSpeed.slot_index(), 3);
    }

    #[test]
    fn second_wing_bonus_serializes_snake_case_including_command() {
        assert_eq!(
            serde_json::to_string(&SecondWingBonus::Command).unwrap(),
            r#""command""#
        );
        assert_eq!(
            serde_json::from_str::<SecondWingBonus>(r#""ignore_defense_chance""#).unwrap(),
            SecondWingBonus::IgnoreDefenseChance
        );
    }

    #[test]
    fn ancient_bonus_level_round_trips_and_rejects() {
        assert_eq!(
            AncientBonusLevel::try_from(1).unwrap(),
            AncientBonusLevel::One
        );
        assert_eq!(
            AncientBonusLevel::try_from(2).unwrap(),
            AncientBonusLevel::Two
        );
        assert_eq!(u8::from(AncientBonusLevel::Two), 2);
        assert_eq!(
            AncientBonusLevel::try_from(0),
            Err(AncientBonusLevelOutOfRange { value: 0 })
        );
        assert_eq!(
            AncientBonusLevel::try_from(3),
            Err(AncientBonusLevelOutOfRange { value: 3 })
        );
    }

    #[test]
    fn ancient_bonus_level_serializes_as_wire_integer() {
        assert_eq!(serde_json::to_string(&AncientBonusLevel::One).unwrap(), "1");
        assert_eq!(
            serde_json::from_str::<AncientBonusLevel>("2").unwrap(),
            AncientBonusLevel::Two
        );
        assert!(serde_json::from_str::<AncientBonusLevel>("0").is_err());
    }
}
