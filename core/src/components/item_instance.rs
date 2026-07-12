//! A rolled item instance — the unique economy asset an item becomes once it
//! drops: its identity, plus-level, rarity payload, orthogonal normal option,
//! luck, skill, durability, and crafted-augment axis. Every field type
//! self-validates on parse, so the aggregate needs no wire mirror of its own;
//! the cross-*reference* invariants (excellent set matches the item's
//! category, crafted augment matches the item's augment capability) are
//! re-proven at reload by [`ItemInstance::reconcile`] once the definition is
//! in hand.
//!
//! [`ItemInstance`] is deliberately **not** `Copy`: an item is a unique asset,
//! and move-only semantics make accidental duplication a compile error rather
//! than a silent dupe. A rejected operation must therefore hand the item back
//! through its outcome, never drop it.

use core::num::{NonZeroU8, NonZeroU16, NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::components::item_options::{
    AncientBonusLevel, DinorantOption, ExcellentArmorOption, ExcellentWeaponOption, NormalOption,
    SecondWingBonus,
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
    /// The chaos-machine crafted-augment axis, orthogonal to `roll`.
    pub augment: CraftedAugment,
}

impl ItemInstance {
    /// Re-proves the cross-*reference* invariants at the reload boundary: an
    /// excellent set must match *this item's* excellent category, and a crafted
    /// augment must match *this item's* augment capability — both live in the
    /// definition, not the instance, so no field can hold them. `Normal` and
    /// `Ancient` rolls carry no set, so the category is irrelevant to them;
    /// [`CraftedAugment::None`] carries no augment, so the slot is irrelevant
    /// to it. `excellent_category` is genuine optionality — the item may have
    /// no excellent category at all.
    ///
    /// # Errors
    /// Returns [`ItemInstanceError::ExcellentSetCategoryMismatch`] when the roll
    /// is excellent and the set's category differs from `excellent_category`
    /// (or the kind has no excellent category), and
    /// [`ItemInstanceError::AugmentSlotMismatch`] when a carried augment does
    /// not match `augment_slot`.
    pub fn reconcile(
        &self,
        excellent_category: Option<ExcellentCat>,
        augment_slot: AugmentSlot,
    ) -> Result<(), ItemInstanceError> {
        match &self.roll {
            RarityRoll::Excellent { options } => match excellent_category {
                Some(category) if options.category() == category => {}
                Some(_) | None => return Err(ItemInstanceError::ExcellentSetCategoryMismatch),
            },
            RarityRoll::Normal | RarityRoll::Ancient { .. } => {}
        }
        match (self.augment, augment_slot) {
            (
                CraftedAugment::None,
                AugmentSlot::None | AugmentSlot::Dinorant | AugmentSlot::WingBonus,
            )
            | (CraftedAugment::Dinorant { .. }, AugmentSlot::Dinorant)
            | (CraftedAugment::WingBonus { .. }, AugmentSlot::WingBonus) => Ok(()),
            (
                CraftedAugment::Dinorant { .. } | CraftedAugment::WingBonus { .. },
                AugmentSlot::None | AugmentSlot::Dinorant | AugmentSlot::WingBonus,
            ) => Err(ItemInstanceError::AugmentSlotMismatch),
        }
    }
}

/// The chaos-machine crafted-augment axis. One kind-tagged enum, not two
/// `Option` fields, so "a dinorant option AND a wing bonus on one item" is
/// unrepresentable — an item is a dinorant XOR a second-wing/cape XOR neither.
/// Pool membership is the crafting service's fact, not this type's: any of the
/// four [`SecondWingBonus`] values fits [`Self::WingBonus`]; which values a
/// wing or the cape draws is enforced by the roll.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CraftedAugment {
    /// No crafted augment — a variant, never an `Option` flag.
    None,
    /// A crafted Dinorant carrying 1–2 distinct dinorant options.
    Dinorant {
        /// The rolled dinorant options.
        options: DinorantOptionSet,
    },
    /// A crafted second wing or cape carrying exactly one bonus option.
    WingBonus {
        /// The rolled bonus.
        bonus: SecondWingBonus,
    },
}

/// The augment-capability discriminator of an item — the [`ExcellentCat`]
/// sibling for the crafted-augment axis. Stored as explicit per-item data on the
/// wing/pet [`crate::data::item_definitions::ItemDefinition`] (mirroring the
/// pendant's stored excellent category) and read back at the reload boundary, so
/// no code heuristic on cape/tier identity decides which augment an item may
/// carry. Wire form: a bare `snake_case` name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AugmentSlot {
    /// The kind carries no crafted augment.
    None,
    /// The kind carries dinorant options (Horn of Dinorant).
    Dinorant,
    /// The kind carries a wing bonus (second wings, Cape of Lord).
    WingBonus,
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

/// 1–2 distinct dinorant options as a [`NonZeroU8`] bitmask (bit
/// `slot_index()-1` set = option present) — the [`ExcellentArmorSet`] precedent
/// narrowed to three slots with a population cap of two. `NonZero` proves the
/// set non-empty (zero options is [`CraftedAugment::None`], not this); one bit
/// per slot forbids duplicates structurally; the parse boundary re-proves
/// `count <= 2`. Wire form: a slot-index-sorted array of the option names,
/// re-proven non-empty, duplicate-free, and within the cap on parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<DinorantOption>", into = "Vec<DinorantOption>")]
pub struct DinorantOptionSet(NonZeroU8);

impl DinorantOptionSet {
    /// The three dinorant options in slot-index order — the draw pool and the
    /// decode order.
    pub const OPTIONS: [DinorantOption; 3] = [
        DinorantOption::DamageAbsorb,
        DinorantOption::MaxAbility,
        DinorantOption::AttackSpeed,
    ];

    /// The population cap a crafted set never exceeds.
    pub const MAX_OPTIONS: u8 = 2;

    /// Builds the set from a guaranteed first option plus any extras — total,
    /// since the first option's bit proves the mask nonzero (the roll path,
    /// infallible by construction).
    #[must_use]
    pub fn with_first(
        first: DinorantOption,
        rest: impl IntoIterator<Item = DinorantOption>,
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
    /// Returns [`DinorantOptionSetError::Empty`] when no option is given.
    pub fn from_options(
        options: impl IntoIterator<Item = DinorantOption>,
    ) -> Result<Self, DinorantOptionSetError> {
        let mut mask = 0u8;
        for option in options {
            mask |= slot_bit(option.slot_index()).get();
        }
        match NonZeroU8::new(mask) {
            Some(mask) => Ok(Self(mask)),
            None => Err(DinorantOptionSetError::Empty),
        }
    }

    /// The present options in slot-index ascending order.
    pub fn iter(self) -> impl Iterator<Item = DinorantOption> {
        let mask = self.0.get();
        Self::OPTIONS
            .into_iter()
            .filter(move |option| mask & slot_bit(option.slot_index()).get() != 0)
    }

    /// The number of present options — `1..=2` for any parsed or crafted set.
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

impl TryFrom<Vec<DinorantOption>> for DinorantOptionSet {
    type Error = DinorantOptionSetError;

    fn try_from(options: Vec<DinorantOption>) -> Result<Self, Self::Error> {
        let given = options.len();
        let set = Self::from_options(options)?;
        if usize::from(set.count()) != given {
            return Err(DinorantOptionSetError::Duplicate);
        }
        if set.count() > Self::MAX_OPTIONS {
            return Err(DinorantOptionSetError::TooMany);
        }
        Ok(set)
    }
}

impl From<DinorantOptionSet> for Vec<DinorantOption> {
    fn from(set: DinorantOptionSet) -> Self {
        set.iter().collect()
    }
}

/// Rejection of a malformed dinorant-option wire array at the parse boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DinorantOptionSetError {
    /// The array was empty — an absent augment is [`CraftedAugment::None`].
    Empty,
    /// The array carried the same option twice.
    Duplicate,
    /// The array carried more options than the crafted cap of two.
    TooMany,
}

impl core::fmt::Display for DinorantOptionSetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "a dinorant option set must have at least one option"),
            Self::Duplicate => write!(f, "a dinorant option set lists an option more than once"),
            Self::TooMany => write!(
                f,
                "a dinorant option set carries at most {} options",
                DinorantOptionSet::MAX_OPTIONS
            ),
        }
    }
}

impl core::error::Error for DinorantOptionSetError {}

/// An item's persistent wear-progress ledger — the scaled-integer accumulator
/// that carries the sub-durability-point remainder of combat wear across hits,
/// sessions, and hosts, so a fractional per-event rate (HealthDamage/2000,
/// 1 hit/10000, HealthDamage/100000) never floors to nothing and never
/// silently drops between calls. Units follow the item's single wear path
/// (damage for the defensive/pet pools, hits for the offensive pool); each
/// worn item wears on exactly one path, so one ledger is unambiguous.
/// `u32`-wide (the pet divisor 100000 exceeds `u16`). A fresh roll and a
/// repaired item carry [`WearLedger::EMPTY`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct WearLedger(u32);

impl WearLedger {
    /// The empty ledger — a fresh roll's and a repaired item's real value.
    pub const EMPTY: Self = Self(0);

    /// Whether the ledger carries no progress — the outbound wire conversion
    /// drops an empty ledger so shipped `{current, max}` forms stay
    /// byte-identical.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Advances by `amount` units at `divisor` units-per-point, splitting off
    /// the whole durability points crossed: the returned ledger holds the
    /// carried remainder (`< divisor`), the `u32` the points to subtract. Pure
    /// modular arithmetic; the divisor is the wear path's constant supplied by
    /// the service, so the ledger holds no rule of its own.
    #[must_use]
    pub fn advanced(self, amount: u32, divisor: NonZeroU32) -> (Self, u32) {
        let total = self.0.saturating_add(amount);
        (Self(total % divisor.get()), total / divisor.get())
    }
}

/// Wire mirror of [`Durability`]; `current <= max` is re-proven on the way in,
/// since a persisted value loaded from a host is untrusted. The wear ledger is
/// skipped when empty, so every shipped `{current, max}` form stays
/// byte-identical on the way out and still parses on the way in.
#[derive(Serialize, Deserialize)]
struct DurabilityWire {
    current: u8,
    max: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wear_progress: Option<WearLedger>,
}

/// An item's wear gauge — a dedicated `u8` proving the authentic 255 wire cap,
/// plus the persisted [`WearLedger`] carrying sub-point combat-wear progress.
/// The invariant `current <= max` holds by construction: [`Durability::new`]
/// rejects an over-full gauge and [`Durability::full`] cannot produce one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "DurabilityWire", into = "DurabilityWire")]
pub struct Durability {
    current: u8,
    max: u8,
    wear_progress: WearLedger,
}

impl Durability {
    /// Builds a durability gauge with an empty wear ledger, rejecting a
    /// current value above the maximum.
    ///
    /// # Errors
    /// Returns [`DurabilityError::CurrentExceedsMax`] when `current > max`.
    pub fn new(current: u8, max: u8) -> Result<Self, DurabilityError> {
        if current > max {
            return Err(DurabilityError::CurrentExceedsMax { current, max });
        }
        Ok(Self {
            current,
            max,
            wear_progress: WearLedger::EMPTY,
        })
    }

    /// A full gauge: `current == max`, empty ledger — a freshly rolled item is
    /// at full durability, and a repair-to-max wipes any carried wear progress
    /// (the scaled-integer mirror of the classic set-to-max).
    #[must_use]
    pub fn full(max: u8) -> Self {
        Self {
            current: max,
            max,
            wear_progress: WearLedger::EMPTY,
        }
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

    /// Rescales the gauge onto a new maximum, preserving the worn fraction:
    /// `new_max * current / old_max`, integer floor. Total by construction:
    /// `current <= max` bounds the quotient by `new_max`, so the result is a
    /// valid gauge on every path, and the zero-max gauge (`0/0`, trivially
    /// full) rescales to full.
    #[must_use]
    pub fn rescaled(self, new_max: u8) -> Self {
        let Some(old_max) = NonZeroU16::new(u16::from(self.max)) else {
            return Self::full(new_max);
        };
        let scaled = u16::from(new_max).saturating_mul(u16::from(self.current)) / old_max;
        // The saturating narrow of a quotient bounded by `new_max` — saturating
        // to the bound itself keeps `current <= max` structural, never a masked
        // lookup absence. The wear ledger is untouched: its divisor is
        // path-based, never max-based, so a crafting max change carries the
        // remainder forward.
        let current = u8::try_from(scaled).unwrap_or(new_max);
        Self {
            current,
            max: new_max,
            wear_progress: self.wear_progress,
        }
    }

    /// This gauge after `amount` units of wear at `divisor` units-per-point:
    /// advances the persisted ledger, subtracts the whole points crossed from
    /// `current` SATURATING at 0 (a broken gauge stays worn — never removed),
    /// and carries the remainder. The wear-toward-broken twin of
    /// [`Durability::decremented`] (which returns `None` at the last point,
    /// removing the item): gear wears to a broken 0 and stays; ammunition and
    /// non-trainable pets decrement to removal. The two decrement seams are
    /// type-distinct — the 0-count-vs-broken distinction is which method the
    /// wear pool calls, never a runtime branch.
    #[must_use]
    pub fn worn(self, amount: u32, divisor: NonZeroU32) -> Self {
        let (ledger, points) = self.wear_progress.advanced(amount, divisor);
        // Boundary saturation of the crossed-points count into the gauge's u8
        // home (the `rescaled` grain), not a lookup absence.
        let lost = u8::try_from(points).unwrap_or(u8::MAX);
        Self {
            current: self.current.saturating_sub(lost),
            max: self.max,
            wear_progress: ledger,
        }
    }

    /// This gauge after one piece is consumed: the next lower gauge, or `None`
    /// when the consumed piece was the last (the count reaches zero and the
    /// whole item is removed rather than left as a zero-count stack). The
    /// consume twin of the stack-raise gauge, mirroring
    /// [`crate::components::active_effect::PoisonTicks::decrement`] — the
    /// last-piece signal the inventory acts on.
    #[must_use]
    pub fn decremented(self) -> Option<Self> {
        NonZeroU8::new(self.current.saturating_sub(1)).map(|current| Self {
            current: current.get(),
            max: self.max,
            wear_progress: self.wear_progress,
        })
    }
}

impl TryFrom<DurabilityWire> for Durability {
    type Error = DurabilityError;

    fn try_from(wire: DurabilityWire) -> Result<Self, Self::Error> {
        if wire.current > wire.max {
            return Err(DurabilityError::CurrentExceedsMax {
                current: wire.current,
                max: wire.max,
            });
        }
        Ok(Self {
            current: wire.current,
            max: wire.max,
            // Parse-boundary default for a genuinely-optional wire field: an
            // absent ledger IS the empty ledger.
            wear_progress: wire.wear_progress.unwrap_or(WearLedger::EMPTY),
        })
    }
}

impl From<Durability> for DurabilityWire {
    fn from(durability: Durability) -> Self {
        Self {
            current: durability.current,
            max: durability.max,
            wear_progress: (!durability.wear_progress.is_empty())
                .then_some(durability.wear_progress),
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

/// Rejection of an item instance whose excellent set or crafted augment does
/// not match its item's capabilities, checked at the reload boundary. The
/// intra-instance invariants (durability, set shape, level bounds) are
/// re-proven by each field's own deserialize, so this holds only the residual
/// cross-references.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemInstanceError {
    /// The excellent set's category differs from the item's, or the item has no
    /// excellent category at all.
    ExcellentSetCategoryMismatch,
    /// The carried crafted augment differs from the item's augment capability.
    AugmentSlotMismatch,
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
            Self::AugmentSlotMismatch => {
                write!(
                    f,
                    "crafted augment does not match the item's augment capability"
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
            augment: CraftedAugment::None,
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
    fn durability_rescale_floors_the_worn_fraction() {
        // Shrinking: 20 * 22 / 30 = 14.67 floors to 14.
        let gauge = Durability::new(22, 30).unwrap();
        assert_eq!(gauge.rescaled(20), Durability::new(14, 20).unwrap());
        // Widening: 30 * 15 / 20 = 22.5 floors to 22.
        let gauge = Durability::new(15, 20).unwrap();
        assert_eq!(gauge.rescaled(30), Durability::new(22, 30).unwrap());
    }

    #[test]
    fn durability_rescale_keeps_full_full_and_empty_empty() {
        assert_eq!(Durability::full(30).rescaled(41), Durability::full(41));
        let empty = Durability::new(0, 30).unwrap();
        assert_eq!(empty.rescaled(41), Durability::new(0, 41).unwrap());
    }

    #[test]
    fn durability_rescale_folds_the_zero_max_gauge_to_full() {
        // A 0/0 gauge is trivially full, so it rescales to full.
        let gauge = Durability::new(0, 0).unwrap();
        assert_eq!(gauge.rescaled(25), Durability::full(25));
        // Rescaling onto a zero maximum lands on the 0/0 gauge.
        assert_eq!(Durability::full(30).rescaled(0), Durability::full(0));
    }

    #[test]
    fn durability_decremented_lowers_and_signals_the_last_piece() {
        // Above one piece the gauge lowers by one, its ceiling unchanged.
        assert_eq!(
            Durability::new(3, 3).unwrap().decremented(),
            Some(Durability::new(2, 3).unwrap())
        );
        assert_eq!(
            Durability::new(2, 3).unwrap().decremented(),
            Some(Durability::new(1, 3).unwrap())
        );
        // The last piece signals removal (`None`), never a zero-count gauge.
        assert_eq!(Durability::new(1, 3).unwrap().decremented(), None);
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

    fn per_2000() -> NonZeroU32 {
        NonZeroU32::new(2000).unwrap()
    }

    #[test]
    fn wear_ledger_carries_the_remainder_and_splits_whole_points() {
        // EQ-WEAR-4: 1500 → no point (rem 1500); +1500 → acc 3000 → one point,
        // remainder 1000. Integer end-to-end.
        let (ledger, points) = WearLedger::EMPTY.advanced(1500, per_2000());
        assert_eq!(points, 0);
        let (ledger, points) = ledger.advanced(1500, per_2000());
        assert_eq!(points, 1);
        assert_eq!(ledger.advanced(1000, per_2000()), (WearLedger::EMPTY, 1));
    }

    #[test]
    fn wear_ledger_flat_hit_counter_never_floors_to_nothing() {
        // EQ-WEAR-5: hit 1 advances the counter; the 10000th crossing yields
        // the point with the counter back to empty.
        let per_10000 = NonZeroU32::new(10_000).unwrap();
        let mut ledger = WearLedger::EMPTY;
        for _ in 0..9_999u32 {
            let (next, points) = ledger.advanced(1, per_10000);
            assert_eq!(points, 0);
            ledger = next;
        }
        assert_eq!(ledger.advanced(1, per_10000), (WearLedger::EMPTY, 1));
    }

    #[test]
    fn worn_wears_toward_broken_and_keeps_the_gauge_at_zero() {
        // Sub-point wear leaves the gauge; a crossing subtracts; a broken
        // gauge saturates at 0 and stays representable (never removed).
        let gauge = Durability::new(1, 22).unwrap();
        let sub_point = gauge.worn(1500, per_2000());
        assert_eq!(sub_point.current(), 1);
        let broken = sub_point.worn(1500, per_2000());
        assert_eq!(broken.current(), 0);
        assert_eq!(broken.max(), 22);
        // Further wear on a broken gauge stays at 0.
        assert_eq!(broken.worn(4000, per_2000()).current(), 0);
    }

    #[test]
    fn wear_progress_rides_the_wire_only_when_non_empty() {
        // A carried remainder is persisted item state: the round trip preserves
        // it, so the next crossing is identical after a save/load.
        let worn = Durability::full(22).worn(1500, per_2000());
        let json = serde_json::to_string(&worn).unwrap();
        assert_eq!(json, r#"{"current":22,"max":22,"wear_progress":1500}"#);
        let reloaded = serde_json::from_str::<Durability>(&json).unwrap();
        assert_eq!(reloaded, worn);
        assert_eq!(reloaded.worn(500, per_2000()).current(), 21);
        // The shipped {current, max} wire form still parses (ledger defaults
        // empty), and a full gauge serializes without the ledger field.
        let shipped = serde_json::from_str::<Durability>(r#"{"current":22,"max":22}"#).unwrap();
        assert_eq!(shipped, Durability::full(22));
        assert_eq!(
            serde_json::to_string(&Durability::full(22)).unwrap(),
            r#"{"current":22,"max":22}"#
        );
    }

    #[test]
    fn repair_to_full_zeroes_the_ledger_and_rescale_keeps_it() {
        let worn = Durability::full(22).worn(1500, per_2000());
        // Repair-to-max is `Durability::full(max)` — a fresh EMPTY ledger.
        assert_eq!(
            serde_json::to_string(&Durability::full(worn.max())).unwrap(),
            r#"{"current":22,"max":22}"#
        );
        // A crafting max change carries the remainder (path-based divisor,
        // never max-based).
        let rescaled = worn.rescaled(30);
        assert_eq!(rescaled.worn(500, per_2000()).current(), 29);
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
        assert_eq!(
            instance.reconcile(Some(ExcellentCat::Armor), AugmentSlot::None),
            Ok(())
        );
        assert_eq!(
            instance.reconcile(Some(ExcellentCat::Weapon), AugmentSlot::None),
            Err(ItemInstanceError::ExcellentSetCategoryMismatch)
        );
        assert_eq!(
            instance.reconcile(None, AugmentSlot::None),
            Err(ItemInstanceError::ExcellentSetCategoryMismatch)
        );
    }

    #[test]
    fn reconcile_ignores_category_for_normal_and_ancient() {
        let normal = normal_instance();
        assert_eq!(normal.reconcile(None, AugmentSlot::None), Ok(()));
        assert_eq!(
            normal.reconcile(Some(ExcellentCat::Weapon), AugmentSlot::None),
            Ok(())
        );
        let mut ancient = normal_instance();
        ancient.roll = RarityRoll::Ancient {
            bonus: AncientBonusLevel::Two,
        };
        assert_eq!(ancient.reconcile(None, AugmentSlot::None), Ok(()));
    }

    #[test]
    fn reconcile_matches_augment_slot_and_rejects_mismatch() {
        let mut dinorant = normal_instance();
        dinorant.augment = CraftedAugment::Dinorant {
            options: DinorantOptionSet::with_first(DinorantOption::DamageAbsorb, []),
        };
        assert_eq!(dinorant.reconcile(None, AugmentSlot::Dinorant), Ok(()));
        assert_eq!(
            dinorant.reconcile(None, AugmentSlot::None),
            Err(ItemInstanceError::AugmentSlotMismatch)
        );
        assert_eq!(
            dinorant.reconcile(None, AugmentSlot::WingBonus),
            Err(ItemInstanceError::AugmentSlotMismatch)
        );

        let mut wing = normal_instance();
        wing.augment = CraftedAugment::WingBonus {
            bonus: SecondWingBonus::Command,
        };
        assert_eq!(wing.reconcile(None, AugmentSlot::WingBonus), Ok(()));
        assert_eq!(
            wing.reconcile(None, AugmentSlot::Dinorant),
            Err(ItemInstanceError::AugmentSlotMismatch)
        );
    }

    #[test]
    fn reconcile_accepts_no_augment_on_any_slot() {
        let bare = normal_instance();
        assert_eq!(bare.reconcile(None, AugmentSlot::None), Ok(()));
        assert_eq!(bare.reconcile(None, AugmentSlot::Dinorant), Ok(()));
        assert_eq!(bare.reconcile(None, AugmentSlot::WingBonus), Ok(()));
    }

    #[test]
    fn dinorant_set_iterates_in_slot_order_and_counts() {
        let set = DinorantOptionSet::with_first(
            DinorantOption::AttackSpeed,
            [DinorantOption::DamageAbsorb],
        );
        assert_eq!(set.count(), 2);
        assert_eq!(
            set.iter().collect::<Vec<_>>(),
            vec![DinorantOption::DamageAbsorb, DinorantOption::AttackSpeed]
        );
        let single = DinorantOptionSet::with_first(DinorantOption::MaxAbility, []);
        assert_eq!(single.count(), 1);
    }

    #[test]
    fn dinorant_set_wire_rejects_empty_duplicate_and_too_many() {
        assert!(serde_json::from_str::<DinorantOptionSet>("[]").is_err());
        assert!(
            serde_json::from_str::<DinorantOptionSet>(r#"["max_ability","max_ability"]"#).is_err()
        );
        assert!(
            serde_json::from_str::<DinorantOptionSet>(
                r#"["damage_absorb","max_ability","attack_speed"]"#
            )
            .is_err()
        );
        assert_eq!(
            DinorantOptionSet::try_from(vec![
                DinorantOption::DamageAbsorb,
                DinorantOption::MaxAbility,
                DinorantOption::AttackSpeed,
            ]),
            Err(DinorantOptionSetError::TooMany)
        );
    }

    #[test]
    fn dinorant_set_wire_is_a_slot_sorted_name_array() {
        let set = DinorantOptionSet::with_first(
            DinorantOption::AttackSpeed,
            [DinorantOption::DamageAbsorb],
        );
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["damage_absorb","attack_speed"]"#);
        assert_eq!(
            serde_json::from_str::<DinorantOptionSet>(&json).unwrap(),
            set
        );
    }

    #[test]
    fn crafted_augment_wire_round_trips_every_kind() {
        let none = CraftedAugment::None;
        assert_eq!(serde_json::to_string(&none).unwrap(), r#"{"kind":"none"}"#);
        let dinorant = CraftedAugment::Dinorant {
            options: DinorantOptionSet::with_first(
                DinorantOption::DamageAbsorb,
                [DinorantOption::AttackSpeed],
            ),
        };
        let dinorant_json = serde_json::to_string(&dinorant).unwrap();
        assert_eq!(
            dinorant_json,
            r#"{"kind":"dinorant","options":["damage_absorb","attack_speed"]}"#
        );
        assert_eq!(
            serde_json::from_str::<CraftedAugment>(&dinorant_json).unwrap(),
            dinorant
        );
        let wing = CraftedAugment::WingBonus {
            bonus: SecondWingBonus::Command,
        };
        let wing_json = serde_json::to_string(&wing).unwrap();
        assert_eq!(wing_json, r#"{"kind":"wing_bonus","bonus":"command"}"#);
        assert_eq!(
            serde_json::from_str::<CraftedAugment>(&wing_json).unwrap(),
            wing
        );
    }

    #[test]
    fn augment_is_a_required_instance_field() {
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&normal_instance()).unwrap()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("augment");
        assert!(serde_json::from_value::<ItemInstance>(value).is_err());
    }

    #[test]
    fn normal_instance_wire_round_trips() {
        let instance = normal_instance();
        let json = serde_json::to_string(&instance).unwrap();
        assert_eq!(
            json,
            r#"{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}}"#
        );
        assert_eq!(
            serde_json::from_str::<ItemInstance>(&json).unwrap(),
            instance
        );
    }

    #[test]
    fn augmented_instance_wire_round_trips() {
        let mut instance = normal_instance();
        instance.augment = CraftedAugment::Dinorant {
            options: DinorantOptionSet::with_first(DinorantOption::MaxAbility, []),
        };
        let json = serde_json::to_string(&instance).unwrap();
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
