//! A rolled item instance — the unique economy asset an item becomes once it
//! drops: its identity, plus-level, rarity payload, orthogonal normal option,
//! luck, skill, and durability. Every field type self-validates on parse, so
//! the aggregate needs no wire mirror of its own; the one cross-*reference*
//! invariant (excellent set matches the item's category) is re-proven at reload
//! by [`ItemInstance::reconcile`] once the definition is in hand.
//!
//! [`ItemInstance`] is deliberately **not** `Copy`: an item is a unique asset,
//! and move-only semantics make accidental duplication a compile error rather
//! than a silent dupe. A rejected operation must therefore hand the item back
//! through its outcome, never drop it.

use core::num::NonZeroU8;

use serde::{Deserialize, Serialize};

use crate::components::item_options::{
    AncientBonusLevel, ExcellentArmorOption, ExcellentWeaponOption, NormalOption,
};
use crate::components::item_quality::ItemRarity;
use crate::components::item_ref::ItemRef;
use crate::components::levels::OptionLevel;
use crate::components::units::ItemLevel;

/// A rolled item instance. Public-field plain-data aggregate (the
/// [`crate::entities::monster_instance::MonsterInstance`] grain): the roll
/// service struct-literals it from typed pieces, and every field type carries
/// its own validating deserialize, so parse re-proves each intra-instance
/// invariant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemInstance {
    /// The game's own item identity this instance is of.
    pub item: ItemRef,
    /// The dropped plus level (`0..=15`); also the sub-kind selector for the
    /// kinds whose item level chooses a variant.
    pub level: ItemLevel,
    /// The rarity payload — the discriminator nesting any excellent set or
    /// ancient bonus so an illegal pairing is unrepresentable.
    pub roll: RarityRoll,
    /// The Jewel-of-Life-leveled normal option, when the drop rolled one.
    /// Genuine optionality.
    pub normal_option: Option<RolledNormalOption>,
    /// Whether the drop rolled luck.
    pub luck: LuckRoll,
    /// Whether the drop rolled its weapon skill.
    pub skill: SkillRoll,
    /// The wear gauge — `current <= max` proven by [`Durability`].
    pub durability: Durability,
}

impl ItemInstance {
    /// Re-proves the one cross-*reference* invariant at the reload boundary: an
    /// excellent set must match *this item's* excellent category (which lives
    /// in the definition, not the instance, so no field can hold it). `Normal`
    /// and `Ancient` rolls carry no set, so the category is irrelevant to them.
    /// `category` is genuine optionality — the item may have no excellent
    /// category at all.
    ///
    /// # Errors
    /// Returns [`ItemInstanceError::ExcellentSetCategoryMismatch`] when the roll
    /// is excellent and the set's category differs from `category` (or the kind
    /// has no excellent category).
    pub fn reconcile(&self, category: Option<ExcellentCat>) -> Result<(), ItemInstanceError> {
        match &self.roll {
            RarityRoll::Excellent { options } => match category {
                Some(category) if options.category() == category => Ok(()),
                Some(_) | None => Err(ItemInstanceError::ExcellentSetCategoryMismatch),
            },
            RarityRoll::Normal | RarityRoll::Ancient { .. } => Ok(()),
        }
    }
}

/// A rolled normal option: which effect, at which Jewel-of-Life level. Both
/// fields self-validate on parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RolledNormalOption {
    /// The normal option effect.
    pub option: NormalOption,
    /// The option level `+1..=+4`.
    pub level: OptionLevel,
}

/// Whether an instance rolled luck — a two-variant enum, never a boolean flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LuckRoll {
    /// The item carries luck.
    Lucky,
    /// The item carries no luck.
    Plain,
}

/// Whether an instance rolled its weapon skill — a two-variant enum, never a
/// boolean flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillRoll {
    /// The item grants its skill while equipped.
    WithSkill,
    /// The item grants no skill.
    NoSkill,
}

/// The rarity payload — the discriminator that makes an illegal rarity pairing
/// unrepresentable: excellent options nest only in [`RarityRoll::Excellent`],
/// the ancient bonus tier only in [`RarityRoll::Ancient`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RarityRoll {
    /// An ordinary item.
    Normal,
    /// An excellent item carrying a non-empty, duplicate-free excellent set.
    Excellent {
        /// The excellent set, keyed to the item's category.
        options: ExcellentOptions,
    },
    /// An ancient item carrying a per-piece bonus tier.
    Ancient {
        /// The ancient bonus tier (`1` or `2`).
        bonus: AncientBonusLevel,
    },
}

impl RarityRoll {
    /// The rarity tier this payload denotes.
    #[must_use]
    pub fn rarity(&self) -> ItemRarity {
        match self {
            Self::Normal => ItemRarity::Normal,
            Self::Excellent { .. } => ItemRarity::Excellent,
            Self::Ancient { .. } => ItemRarity::Ancient,
        }
    }
}

/// An excellent set, tagged by the item's category — the armor and weapon
/// option enums are distinct types, so an armor option in a weapon set is a
/// compile error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "set", rename_all = "snake_case")]
pub enum ExcellentOptions {
    /// The armor excellent effects (armor pieces, shields, rings).
    Armor {
        /// The present armor options.
        options: ExcellentArmorSet,
    },
    /// The weapon excellent effects.
    Weapon {
        /// The present weapon options.
        options: ExcellentWeaponSet,
    },
}

impl ExcellentOptions {
    /// The bare category discriminator of this set.
    #[must_use]
    pub fn category(&self) -> ExcellentCat {
        match self {
            Self::Armor { .. } => ExcellentCat::Armor,
            Self::Weapon { .. } => ExcellentCat::Weapon,
        }
    }
}

/// The bare excellent-set discriminator — armor or weapon — used as the
/// construction-time proof input at reload and derived from the item's kind at
/// roll time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExcellentCat {
    /// The armor excellent set.
    Armor,
    /// The weapon excellent set.
    Weapon,
}

/// A single set bit as a nonzero mask. Total: `slot_index` is `1..=6`, so the
/// shift is `0..=5` and the value is one of `1,2,4,8,16,32` — always nonzero;
/// the `None` arm is a defined fallback for the unreachable zero input, never a
/// panic.
const fn slot_bit(slot_index: u8) -> NonZeroU8 {
    match NonZeroU8::new(1u8 << slot_index.saturating_sub(1)) {
        Some(bit) => bit,
        None => NonZeroU8::MIN,
    }
}

/// The client's fixed 6-slot excellent-armor set, backed by a [`NonZeroU8`]
/// bitmask (bit `slot_index()-1` set = option present). `NonZero` proves the
/// set is non-empty — first-option-guaranteed — structurally; one bit per slot
/// forbids duplicates structurally. Wire form: a slot-index-sorted array of the
/// option names, re-proven non-empty and duplicate-free on parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<ExcellentArmorOption>",
    into = "Vec<ExcellentArmorOption>"
)]
pub struct ExcellentArmorSet(NonZeroU8);

impl ExcellentArmorSet {
    /// The six armor options in slot-index order — the draw pool and the decode
    /// order.
    pub const OPTIONS: [ExcellentArmorOption; 6] = [
        ExcellentArmorOption::ZenGain,
        ExcellentArmorOption::DefenseRate,
        ExcellentArmorOption::DamageReflect,
        ExcellentArmorOption::DamageDecrease,
        ExcellentArmorOption::MaxMana,
        ExcellentArmorOption::MaxHealth,
    ];

    /// Builds the set from a guaranteed first option plus any extras — total,
    /// since the first option's bit proves the mask nonzero (the roll path,
    /// infallible by construction).
    #[must_use]
    pub fn with_first(
        first: ExcellentArmorOption,
        rest: impl IntoIterator<Item = ExcellentArmorOption>,
    ) -> Self {
        let mut mask = slot_bit(first.slot_index());
        for option in rest {
            mask |= slot_bit(option.slot_index());
        }
        Self(mask)
    }

    /// Builds the set from an iterator of options, OR-ing each slot bit. The
    /// reload/parse constructor.
    ///
    /// # Errors
    /// Returns [`ExcellentSetError::Empty`] when no option is given.
    pub fn from_options(
        options: impl IntoIterator<Item = ExcellentArmorOption>,
    ) -> Result<Self, ExcellentSetError> {
        let mut mask = 0u8;
        for option in options {
            mask |= slot_bit(option.slot_index()).get();
        }
        match NonZeroU8::new(mask) {
            Some(mask) => Ok(Self(mask)),
            None => Err(ExcellentSetError::Empty),
        }
    }

    /// The present options in slot-index ascending order.
    pub fn iter(self) -> impl Iterator<Item = ExcellentArmorOption> {
        let mask = self.0.get();
        Self::OPTIONS
            .into_iter()
            .filter(move |option| mask & slot_bit(option.slot_index()).get() != 0)
    }

    /// The number of present options — `1..=6`.
    #[must_use]
    pub fn count(self) -> u8 {
        let mask = self.0.get();
        let mut count = 0u8;
        for option in Self::OPTIONS {
            if mask & slot_bit(option.slot_index()).get() != 0 {
                count = count.saturating_add(1);
            }
        }
        count
    }
}

impl TryFrom<Vec<ExcellentArmorOption>> for ExcellentArmorSet {
    type Error = ExcellentSetError;

    fn try_from(options: Vec<ExcellentArmorOption>) -> Result<Self, Self::Error> {
        let given = options.len();
        let set = Self::from_options(options)?;
        if usize::from(set.count()) != given {
            return Err(ExcellentSetError::Duplicate);
        }
        Ok(set)
    }
}

impl From<ExcellentArmorSet> for Vec<ExcellentArmorOption> {
    fn from(set: ExcellentArmorSet) -> Self {
        set.iter().collect()
    }
}

/// The client's fixed 6-slot excellent-weapon set — the [`ExcellentArmorSet`]
/// twin over the weapon option enum, same [`NonZeroU8`] bitmask guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<ExcellentWeaponOption>",
    into = "Vec<ExcellentWeaponOption>"
)]
pub struct ExcellentWeaponSet(NonZeroU8);

impl ExcellentWeaponSet {
    /// The six weapon options in slot-index order — the draw pool and the
    /// decode order.
    pub const OPTIONS: [ExcellentWeaponOption; 6] = [
        ExcellentWeaponOption::ManaAfterKill,
        ExcellentWeaponOption::HealthAfterKill,
        ExcellentWeaponOption::AttackSpeed,
        ExcellentWeaponOption::DamagePct,
        ExcellentWeaponOption::DamagePerLevel,
        ExcellentWeaponOption::ExcellentDamageChance,
    ];

    /// Builds the set from a guaranteed first option plus any extras — total,
    /// since the first option's bit proves the mask nonzero.
    #[must_use]
    pub fn with_first(
        first: ExcellentWeaponOption,
        rest: impl IntoIterator<Item = ExcellentWeaponOption>,
    ) -> Self {
        let mut mask = slot_bit(first.slot_index());
        for option in rest {
            mask |= slot_bit(option.slot_index());
        }
        Self(mask)
    }

    /// Builds the set from an iterator of options, OR-ing each slot bit. The
    /// reload/parse constructor.
    ///
    /// # Errors
    /// Returns [`ExcellentSetError::Empty`] when no option is given.
    pub fn from_options(
        options: impl IntoIterator<Item = ExcellentWeaponOption>,
    ) -> Result<Self, ExcellentSetError> {
        let mut mask = 0u8;
        for option in options {
            mask |= slot_bit(option.slot_index()).get();
        }
        match NonZeroU8::new(mask) {
            Some(mask) => Ok(Self(mask)),
            None => Err(ExcellentSetError::Empty),
        }
    }

    /// The present options in slot-index ascending order.
    pub fn iter(self) -> impl Iterator<Item = ExcellentWeaponOption> {
        let mask = self.0.get();
        Self::OPTIONS
            .into_iter()
            .filter(move |option| mask & slot_bit(option.slot_index()).get() != 0)
    }

    /// The number of present options — `1..=6`.
    #[must_use]
    pub fn count(self) -> u8 {
        let mask = self.0.get();
        let mut count = 0u8;
        for option in Self::OPTIONS {
            if mask & slot_bit(option.slot_index()).get() != 0 {
                count = count.saturating_add(1);
            }
        }
        count
    }
}

impl TryFrom<Vec<ExcellentWeaponOption>> for ExcellentWeaponSet {
    type Error = ExcellentSetError;

    fn try_from(options: Vec<ExcellentWeaponOption>) -> Result<Self, Self::Error> {
        let given = options.len();
        let set = Self::from_options(options)?;
        if usize::from(set.count()) != given {
            return Err(ExcellentSetError::Duplicate);
        }
        Ok(set)
    }
}

impl From<ExcellentWeaponSet> for Vec<ExcellentWeaponOption> {
    fn from(set: ExcellentWeaponSet) -> Self {
        set.iter().collect()
    }
}

/// Rejection of a malformed excellent-set wire array at the parse boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcellentSetError {
    /// The array was empty — an excellent set has at least one option.
    Empty,
    /// The array carried the same option twice.
    Duplicate,
}

impl core::fmt::Display for ExcellentSetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "an excellent set must have at least one option"),
            Self::Duplicate => write!(f, "an excellent set lists an option more than once"),
        }
    }
}

impl core::error::Error for ExcellentSetError {}

/// Wire mirror of [`Durability`]; `current <= max` is re-proven on the way in,
/// since a persisted value loaded from a host is untrusted.
#[derive(Serialize, Deserialize)]
struct DurabilityWire {
    current: u8,
    max: u8,
}

/// An item's wear gauge — a dedicated `u8` proving the authentic 255 wire cap.
/// The invariant `current <= max` holds by construction: [`Durability::new`]
/// rejects an over-full gauge and [`Durability::full`] cannot produce one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DurabilityWire", into = "DurabilityWire")]
pub struct Durability {
    current: u8,
    max: u8,
}

impl Durability {
    /// Builds a durability gauge, rejecting a current value above the maximum.
    ///
    /// # Errors
    /// Returns [`DurabilityError::CurrentExceedsMax`] when `current > max`.
    pub fn new(current: u8, max: u8) -> Result<Self, DurabilityError> {
        if current > max {
            return Err(DurabilityError::CurrentExceedsMax { current, max });
        }
        Ok(Self { current, max })
    }

    /// A full gauge: `current == max` — a freshly rolled item is at full
    /// durability.
    #[must_use]
    pub fn full(max: u8) -> Self {
        Self { current: max, max }
    }

    /// The current value.
    #[must_use]
    pub const fn current(self) -> u8 {
        self.current
    }

    /// The maximum value.
    #[must_use]
    pub const fn max(self) -> u8 {
        self.max
    }
}

impl TryFrom<DurabilityWire> for Durability {
    type Error = DurabilityError;

    fn try_from(wire: DurabilityWire) -> Result<Self, Self::Error> {
        Self::new(wire.current, wire.max)
    }
}

impl From<Durability> for DurabilityWire {
    fn from(durability: Durability) -> Self {
        Self {
            current: durability.current,
            max: durability.max,
        }
    }
}

/// Rejection of a malformed durability gauge at construction or the data-load
/// boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityError {
    /// The current value exceeded the maximum.
    CurrentExceedsMax {
        /// The offending current value.
        current: u8,
        /// The maximum it exceeded.
        max: u8,
    },
}

impl core::fmt::Display for DurabilityError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CurrentExceedsMax { current, max } => {
                write!(f, "durability current {current} exceeds max {max}")
            }
        }
    }
}

impl core::error::Error for DurabilityError {}

/// Rejection of an item instance whose excellent set does not match its item's
/// excellent category, checked at the reload boundary. The intra-instance
/// invariants (durability, set shape, level bounds) are re-proven by each
/// field's own deserialize, so this holds only the residual cross-reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemInstanceError {
    /// The excellent set's category differs from the item's, or the item has no
    /// excellent category at all.
    ExcellentSetCategoryMismatch,
}

impl core::fmt::Display for ItemInstanceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ExcellentSetCategoryMismatch => {
                write!(
                    f,
                    "excellent set does not match the item's excellent category"
                )
            }
        }
    }
}

impl core::error::Error for ItemInstanceError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn armor_set(options: &[ExcellentArmorOption]) -> ExcellentArmorSet {
        ExcellentArmorSet::from_options(options.iter().copied()).unwrap()
    }

    fn normal_instance() -> ItemInstance {
        ItemInstance {
            item: ItemRef {
                group: 0,
                number: 3,
            },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
        }
    }

    #[test]
    fn durability_new_rejects_over_full_and_full_seeds_at_max() {
        assert_eq!(
            Durability::new(31, 30),
            Err(DurabilityError::CurrentExceedsMax {
                current: 31,
                max: 30
            })
        );
        let full = Durability::full(30);
        assert_eq!(full.current(), 30);
        assert_eq!(full.max(), 30);
        assert_eq!(Durability::new(0, 0).unwrap().max(), 0);
    }

    #[test]
    fn durability_wire_round_trips_and_rejects() {
        let durability = Durability::new(12, 30).unwrap();
        let json = serde_json::to_string(&durability).unwrap();
        assert_eq!(json, r#"{"current":12,"max":30}"#);
        assert_eq!(
            serde_json::from_str::<Durability>(&json).unwrap(),
            durability
        );
        assert!(serde_json::from_str::<Durability>(r#"{"current":31,"max":30}"#).is_err());
    }

    #[test]
    fn armor_set_from_a_single_option_is_non_empty() {
        let set = armor_set(&[ExcellentArmorOption::MaxHealth]);
        assert_eq!(set.count(), 1);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![ExcellentArmorOption::MaxHealth]
        );
    }

    #[test]
    fn armor_set_iterates_in_slot_index_order_regardless_of_input_order() {
        let set = armor_set(&[
            ExcellentArmorOption::MaxHealth,
            ExcellentArmorOption::ZenGain,
            ExcellentArmorOption::DamageReflect,
        ]);
        assert_eq!(set.count(), 3);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![
                ExcellentArmorOption::ZenGain,
                ExcellentArmorOption::DamageReflect,
                ExcellentArmorOption::MaxHealth,
            ]
        );
    }

    #[test]
    fn with_first_dedupes_and_stays_non_empty() {
        let set = ExcellentWeaponSet::with_first(
            ExcellentWeaponOption::AttackSpeed,
            [
                ExcellentWeaponOption::AttackSpeed,
                ExcellentWeaponOption::DamagePct,
            ],
        );
        assert_eq!(set.count(), 2);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![
                ExcellentWeaponOption::AttackSpeed,
                ExcellentWeaponOption::DamagePct,
            ]
        );
    }

    #[test]
    fn armor_set_wire_is_a_slot_sorted_name_array() {
        let set = armor_set(&[
            ExcellentArmorOption::MaxHealth,
            ExcellentArmorOption::ZenGain,
        ]);
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["zen_gain","max_health"]"#);
        assert_eq!(
            serde_json::from_str::<ExcellentArmorSet>(&json).unwrap(),
            set
        );
    }

    #[test]
    fn armor_set_wire_rejects_empty_and_duplicate() {
        assert!(serde_json::from_str::<ExcellentArmorSet>("[]").is_err());
        assert!(serde_json::from_str::<ExcellentArmorSet>(r#"["zen_gain","zen_gain"]"#).is_err());
    }

    #[test]
    fn excellent_options_reports_its_category() {
        let armor = ExcellentOptions::Armor {
            options: armor_set(&[ExcellentArmorOption::ZenGain]),
        };
        assert_eq!(armor.category(), ExcellentCat::Armor);
        let weapon = ExcellentOptions::Weapon {
            options: ExcellentWeaponSet::with_first(ExcellentWeaponOption::AttackSpeed, []),
        };
        assert_eq!(weapon.category(), ExcellentCat::Weapon);
    }

    #[test]
    fn rarity_roll_maps_to_the_rarity_tier() {
        assert_eq!(RarityRoll::Normal.rarity(), ItemRarity::Normal);
        assert_eq!(
            RarityRoll::Excellent {
                options: ExcellentOptions::Armor {
                    options: armor_set(&[ExcellentArmorOption::ZenGain]),
                },
            }
            .rarity(),
            ItemRarity::Excellent
        );
        assert_eq!(
            RarityRoll::Ancient {
                bonus: AncientBonusLevel::One,
            }
            .rarity(),
            ItemRarity::Ancient
        );
    }

    #[test]
    fn reconcile_matches_excellent_category_and_rejects_mismatch() {
        let mut instance = normal_instance();
        instance.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Armor {
                options: armor_set(&[ExcellentArmorOption::ZenGain]),
            },
        };
        assert_eq!(instance.reconcile(Some(ExcellentCat::Armor)), Ok(()));
        assert_eq!(
            instance.reconcile(Some(ExcellentCat::Weapon)),
            Err(ItemInstanceError::ExcellentSetCategoryMismatch)
        );
        assert_eq!(
            instance.reconcile(None),
            Err(ItemInstanceError::ExcellentSetCategoryMismatch)
        );
    }

    #[test]
    fn reconcile_ignores_category_for_normal_and_ancient() {
        let normal = normal_instance();
        assert_eq!(normal.reconcile(None), Ok(()));
        assert_eq!(normal.reconcile(Some(ExcellentCat::Weapon)), Ok(()));
        let mut ancient = normal_instance();
        ancient.roll = RarityRoll::Ancient {
            bonus: AncientBonusLevel::Two,
        };
        assert_eq!(ancient.reconcile(None), Ok(()));
    }

    #[test]
    fn normal_instance_wire_round_trips() {
        let instance = normal_instance();
        let json = serde_json::to_string(&instance).unwrap();
        assert_eq!(
            json,
            r#"{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30}}"#
        );
        assert_eq!(
            serde_json::from_str::<ItemInstance>(&json).unwrap(),
            instance
        );
    }

    #[test]
    fn excellent_instance_wire_round_trips() {
        let mut instance = normal_instance();
        instance.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Weapon {
                options: ExcellentWeaponSet::with_first(
                    ExcellentWeaponOption::ManaAfterKill,
                    [ExcellentWeaponOption::AttackSpeed],
                ),
            },
        };
        instance.skill = SkillRoll::WithSkill;
        let json = serde_json::to_string(&instance).unwrap();
        assert_eq!(
            serde_json::from_str::<ItemInstance>(&json).unwrap(),
            instance
        );
    }
}
