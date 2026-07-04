//! The classic NPC buying-price formula: one total routing over [`ItemKind`]
//! from an instance and its resolved definition to a zen price. Per-item price
//! constants ([`ItemPrice::Fixed`] / [`ItemPrice::PerLevel`]) are data; the
//! branch rules over the kinds — cubic, wing, general, the dinorant special,
//! the consumable stack scaling, and the modifier chain — are the rules here.
//! All arithmetic is saturating unsigned integer through the
//! [`crate::services::ratio`] u64 home; the caller receives the final rounded
//! price. `old_buying_price` overlays the legacy jewel value table the
//! chaos-machine value economy divides by.

use core::num::NonZeroU64;

use crate::components::item_instance::{
    CraftedAugment, ExcellentOptions, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use crate::components::item_options::NormalOption;
use crate::components::levels::OptionLevel;
use crate::components::units::{ItemLevel, Zen};
use crate::data::chaos_mixes::row_at;
use crate::data::common::SkillNumber;
use crate::data::item_definitions::{ItemDefinition, ItemKind, ItemPrice, JewelKind, PetRide};
use crate::services::ratio::{nonzero_u64, scale_ratio_u64};

/// Tens — the fine display-rounding unit.
const TEN: NonZeroU64 = nonzero_u64(10);
/// Hundreds — the coarse display-rounding unit.
const HUNDRED: NonZeroU64 = nonzero_u64(100);

// W-SRC: the classic per-item-level drop-level surcharge table
// ({5:4, 6:10, 7:25, 8:45, 9:65, 10:95, 11:135, 12:185, 13:245, 14:305,
// 15:365}), applied by the wing and general branches only, never the cubic.
const PRICE_LEVEL_SURCHARGE: [u64; 16] = [
    0, 0, 0, 0, 0, 4, 10, 25, 45, 65, 95, 135, 185, 245, 305, 365,
];

// W-SRC: the classic worthless-skill set — skills whose presence adds no
// price (Force 66 and the era's summon fillers 223/224/225).
const WORTHLESS_SKILLS: [SkillNumber; 4] = [
    SkillNumber(66),
    SkillNumber(223),
    SkillNumber(224),
    SkillNumber(225),
];

/// The wing item group — group-12 wings price on the wing polynomial while the
/// group-13 cape routes to the cubic branch (the authentic group-based rule).
// W-SRC: capes route cubic; only group-12 wings take the wing polynomial.
const WING_GROUP: u8 = 12;

/// The classic NPC buying price of an instance, total over [`ItemKind`].
/// The definition is the already-resolved record for `item.item` (the
/// `roll_dropped_item` port precedent) — pricing never re-resolves a ref.
#[must_use]
pub fn buying_price(def: &ItemDefinition, item: &ItemInstance) -> Zen {
    let raw = match &def.kind {
        ItemKind::Consumable { .. } => consumable_price(def, item),
        ItemKind::SkillScroll { .. }
        | ItemKind::Orb { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::StatFruit
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial => constant_zen(def, item),
        ItemKind::LuckyBox
        | ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. } => general_price(def, item),
        ItemKind::Ring { .. } | ItemKind::Pendant { .. } | ItemKind::TransformationRing { .. } => {
            cubic_price(def, item)
        }
        ItemKind::Pet { ride, .. } => match ride {
            PetRide::FlyingMount => dinorant_price(item),
            PetRide::GroundMount | PetRide::NotRideable => cubic_price(def, item),
        },
        ItemKind::Wings { .. } => {
            if def.id.group == WING_GROUP {
                wing_price(def, item)
            } else {
                cubic_price(def, item)
            }
        }
    };
    Zen(round_price(raw))
}

/// The legacy NPC value the chaos-machine value economy divides by: the
/// old five-jewel table, and [`buying_price`] for everything else. The ONLY
/// price the crafting divisor path consumes.
#[must_use]
pub fn old_buying_price(def: &ItemDefinition, item: &ItemInstance) -> Zen {
    match &def.kind {
        ItemKind::Jewel { jewel } => Zen(old_jewel_value(*jewel)),
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. } => buying_price(def, item),
    }
}

// W-SRC: the legacy jewel value table the chaos-weapon-family rate divides —
// Bless 100,000 / Soul 70,000 / Chaos 40,000 / Life 450,000 / Creation 450,000.
fn old_jewel_value(jewel: JewelKind) -> u64 {
    match jewel {
        JewelKind::Bless => 100_000,
        JewelKind::Soul => 70_000,
        JewelKind::Chaos => 40_000,
        JewelKind::Life | JewelKind::Creation => 450_000,
    }
}

/// The per-item price constant at the instance's level, total over
/// [`ItemPrice`]: `Fixed` verbatim, `PerLevel` clamp-indexed by item level.
/// A constant-priced kind whose record says `Formula` prices by the classic
/// no-Value fallthrough — the general polynomial. Unreachable on the shipped
/// dataset (the generators pin these kinds to `Fixed`/`PerLevel`); a defined
/// fallback, never a fabricated zero.
fn constant_zen(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    match &def.price {
        ItemPrice::Fixed { zen } => zen.0,
        ItemPrice::PerLevel { zen_by_level } => zen_by_level.at(item.level).0,
        ItemPrice::Formula => general_price(def, item),
    }
}

// W-SRC: classic consumable pricing — the fixed base doubles per item level,
// truncates to tens, then scales by the piece count (current durability).
fn consumable_price(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let base = constant_zen(def, item);
    let scaled = if item.level > ItemLevel::ZERO {
        base.saturating_mul(2u64.saturating_pow(u32::from(item.level.get())))
    } else {
        base
    };
    truncate_to(scaled, TEN).saturating_mul(u64::from(item.durability.current()))
}

// W-SRC: the classic dinorant special price — 960,000 + 300,000 per crafted
// dinorant option.
fn dinorant_price(item: &ItemInstance) -> u64 {
    let options = match &item.augment {
        CraftedAugment::Dinorant { options } => u64::from(options.count()),
        CraftedAugment::None | CraftedAugment::WingBonus { .. } => 0,
    };
    960_000u64.saturating_add(300_000u64.saturating_mul(options))
}

// W-SRC: the classic cubic branch — `dl³ + 100`, plus `price × option level`
// iff the normal option is health recovery. No other modifier exists on this
// branch; an excellent roll contributes only the +25 inside `dl`.
fn cubic_price(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let dl = price_drop_level(def, item);
    let mut price = dl.saturating_mul(dl).saturating_mul(dl).saturating_add(100);
    if let Some(rolled) = item.normal_option {
        if rolled.option == NormalOption::HealthRecoveryPct {
            price = price.saturating_add(price.saturating_mul(u64::from(rolled.level.wire())));
        }
    }
    price
}

// W-SRC: the classic wing polynomial — `(DL+40)·DL²·11 + 40,000,000` over the
// surcharged drop level, then the shared modifier chain.
fn wing_price(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let dl = surcharged_drop_level(def, item);
    let base = dl
        .saturating_add(40)
        .saturating_mul(dl)
        .saturating_mul(dl)
        .saturating_mul(11)
        .saturating_add(40_000_000);
    apply_modifiers(base, def, item)
}

// W-SRC: the classic general polynomial — `(DL+40)·DL²/8 + 100` over the
// surcharged drop level, then the shared modifier chain.
fn general_price(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let dl = surcharged_drop_level(def, item);
    let poly = dl.saturating_add(40).saturating_mul(dl).saturating_mul(dl);
    let base = scale_ratio_u64(poly, 1, nonzero_u64(8)).saturating_add(100);
    apply_modifiers(base, def, item)
}

// W-SRC: the classic price drop-level — `drop_level + 3·item level`, +25 once
// when the roll is excellent. The classic price rule has no ancient term.
fn price_drop_level(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let excellent_bonus = match &item.roll {
        RarityRoll::Excellent { .. } => 25,
        RarityRoll::Normal | RarityRoll::Ancient { .. } => 0,
    };
    u64::from(def.drop_level)
        .saturating_add(3u64.saturating_mul(u64::from(item.level.get())))
        .saturating_add(excellent_bonus)
}

/// The wing/general drop level: the price drop-level plus the per-item-level
/// surcharge table (never applied on the cubic branch).
fn surcharged_drop_level(def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let [first, rest @ ..] = &PRICE_LEVEL_SURCHARGE;
    price_drop_level(def, item).saturating_add(row_at(*first, rest, usize::from(item.level.get())))
}

/// The shared wing/general modifier chain, applied in the classic order, each
/// step compounding on the running total: narrow-wield/shield discount, skill,
/// luck, normal option, per wing-bonus augment, per excellent option.
fn apply_modifiers(base: u64, def: &ItemDefinition, item: &ItemInstance) -> u64 {
    let mut price = base;
    // W-SRC: the classic ×80/100 discount is width-based (`group < 6 AND
    // width < 2`, so narrow staffs get it too) plus every shield.
    if narrow_wield_or_shield(def) {
        price = scale_ratio_u64(price, 80, HUNDRED);
    }
    // W-SRC: skill surcharge +price·3/2, unless the skill is worthless.
    if item.skill == SkillRoll::WithSkill {
        if let Some(skill) = def.kind.skill() {
            if !WORTHLESS_SKILLS.contains(&skill) {
                price = price.saturating_add(scale_ratio_u64(price, 3, nonzero_u64(2)));
            }
        }
    }
    // W-SRC: luck surcharge +price·25/100.
    if item.luck == LuckRoll::Lucky {
        price = price.saturating_add(scale_ratio_u64(price, 25, HUNDRED));
    }
    if let Some(rolled) = item.normal_option {
        price = price.saturating_add(option_surcharge(price, rolled.level));
    }
    // W-SRC: +price·1/4 per wing-bonus augment, compounding.
    if let CraftedAugment::WingBonus { .. } = &item.augment {
        price = price.saturating_add(scale_ratio_u64(price, 1, nonzero_u64(4)));
    }
    // W-SRC: ×2 per excellent option, compounding.
    for _ in 0..excellent_option_count(item) {
        price = price.saturating_mul(2);
    }
    price
}

// W-SRC: normal-option surcharge — L1 adds price·3/5; L≥2 adds
// price·7·2^(L−1)/10 (14/28/56 over 10).
fn option_surcharge(price: u64, level: OptionLevel) -> u64 {
    match level {
        OptionLevel::L1 => scale_ratio_u64(price, 3, nonzero_u64(5)),
        OptionLevel::L2 => scale_ratio_u64(price, 14, TEN),
        OptionLevel::L3 => scale_ratio_u64(price, 28, TEN),
        OptionLevel::L4 => scale_ratio_u64(price, 56, TEN),
    }
}

/// Whether the classic width discount applies: a width-1 wielded item
/// (melee weapon, bow, crossbow, or staff) or any shield. Total over
/// [`ItemKind`].
fn narrow_wield_or_shield(def: &ItemDefinition) -> bool {
    match &def.kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. } => def.width < 2,
        ItemKind::Shield { .. } => true,
        ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. } => false,
    }
}

/// The number of excellent options an instance carries — zero off the
/// excellent roll.
fn excellent_option_count(item: &ItemInstance) -> u64 {
    match &item.roll {
        RarityRoll::Excellent { options } => match options {
            ExcellentOptions::Armor { options } => u64::from(options.count()),
            ExcellentOptions::Weapon { options } => u64::from(options.count()),
        },
        RarityRoll::Normal | RarityRoll::Ancient { .. } => 0,
    }
}

// W-SRC: classic display rounding, applied last on every path — ≥1000
// truncates to hundreds, ≥100 to tens.
fn round_price(value: u64) -> u64 {
    if value >= 1000 {
        truncate_to(value, HUNDRED)
    } else if value >= 100 {
        truncate_to(value, TEN)
    } else {
        value
    }
}

/// Truncates a value down to a multiple of `unit`.
fn truncate_to(value: u64, unit: NonZeroU64) -> u64 {
    (value / unit.get()).saturating_mul(unit.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::class::ClassSet;
    use crate::components::item_instance::{
        Durability, ExcellentArmorSet, ExcellentWeaponSet, RolledNormalOption,
    };
    use crate::components::item_options::{
        ExcellentArmorOption, ExcellentWeaponOption, SecondWingBonus,
    };
    use crate::components::item_ref::ItemRef;
    use crate::data::common::{Provenance, SourceVersion};
    use crate::data::item_definitions::{
        ConsumeEffect, HealingTier, PerLevelPrice, WeaponHandling, WearRequirements, WingTier,
    };

    fn provenance() -> Provenance {
        Provenance {
            source_version: SourceVersion::V075,
            review: None,
        }
    }

    fn wear() -> WearRequirements {
        WearRequirements {
            level: 0,
            strength: 0,
            agility: 0,
            vitality: 0,
            energy: 0,
            command: 0,
        }
    }

    fn def(
        id: ItemRef,
        width: u8,
        drop_level: u8,
        price: ItemPrice,
        kind: ItemKind,
    ) -> ItemDefinition {
        ItemDefinition {
            id,
            provenance: provenance(),
            width,
            height: 1,
            drops_from_monsters: false,
            drop_level,
            max_item_level: ItemLevel::new(15).unwrap(),
            durability: 20,
            price,
            kind,
        }
    }

    fn weapon_kind(skill: Option<SkillNumber>) -> ItemKind {
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            min_damage: 1,
            max_damage: 2,
            attack_speed: 20,
            skill,
            classes: ClassSet::NONE,
            wear: wear(),
        }
    }

    fn wing_kind() -> ItemKind {
        ItemKind::Wings {
            tier: WingTier::First,
            defense: 10,
            absorb_percent: 12,
            damage_percent: 12,
            jol_options: vec![NormalOption::HealthRecoveryPct],
            classes: ClassSet::NONE,
            wear: wear(),
        }
    }

    fn ring_kind() -> ItemKind {
        ItemKind::Ring {
            resistance: None,
            option: NormalOption::HealthRecoveryPct,
            classes: ClassSet::NONE,
            wear: wear(),
        }
    }

    fn jewel_def(jewel: JewelKind, zen: u64) -> ItemDefinition {
        def(
            ItemRef {
                group: 14,
                number: 13,
            },
            1,
            25,
            ItemPrice::Fixed { zen: Zen(zen) },
            ItemKind::Jewel { jewel },
        )
    }

    fn instance(id: ItemRef, level: u8) -> ItemInstance {
        ItemInstance {
            item: id,
            level: ItemLevel::new(level).unwrap(),
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(20),
            augment: CraftedAugment::None,
        }
    }

    fn option(option: NormalOption, level: OptionLevel) -> RolledNormalOption {
        RolledNormalOption { option, level }
    }

    const WING_ID: ItemRef = ItemRef {
        group: 12,
        number: 0,
    };
    const CAPE_ID: ItemRef = ItemRef {
        group: 13,
        number: 30,
    };
    const WEAPON_ID: ItemRef = ItemRef {
        group: 0,
        number: 3,
    };

    #[test]
    fn a_clean_first_wing_prices_on_the_wing_branch_at_the_pinned_anchor() {
        // The ≈55.4M anchor that yields the 13-point second-wings base rate.
        let wing = def(WING_ID, 3, 100, ItemPrice::Formula, wing_kind());
        assert_eq!(buying_price(&wing, &instance(WING_ID, 0)), Zen(55_400_000));
    }

    #[test]
    fn a_cape_routes_to_the_cubic_branch_by_group() {
        let cape = def(CAPE_ID, 2, 180, ItemPrice::Formula, wing_kind());
        // 180³ + 100 = 5,832,100 — already a multiple of 100.
        assert_eq!(buying_price(&cape, &instance(CAPE_ID, 0)), Zen(5_832_100));
    }

    #[test]
    fn the_general_branch_applies_every_modifier_in_order() {
        // dl = 10 + 3·7 + 25 (excellent) = 56; DL = 56 + 25 (level-7 row) = 81.
        // base 99,335 → width 79,468 → skill 198,670 → luck 248,337 →
        // option L1 397,339 → ×2×2 excellent 1,589,356 → rounded 1,589,300.
        let weapon = def(
            WEAPON_ID,
            1,
            10,
            ItemPrice::Formula,
            weapon_kind(Some(SkillNumber(19))),
        );
        let mut item = instance(WEAPON_ID, 7);
        item.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Weapon {
                options: ExcellentWeaponSet::with_first(
                    ExcellentWeaponOption::AttackSpeed,
                    [ExcellentWeaponOption::DamagePct],
                ),
            },
        };
        item.skill = SkillRoll::WithSkill;
        item.luck = LuckRoll::Lucky;
        item.normal_option = Some(option(NormalOption::PhysicalDamage, OptionLevel::L1));
        assert_eq!(buying_price(&weapon, &item), Zen(1_589_300));
    }

    #[test]
    fn a_worthless_skill_adds_no_surcharge() {
        let worthless = def(
            WEAPON_ID,
            2,
            10,
            ItemPrice::Formula,
            weapon_kind(Some(SkillNumber(66))),
        );
        let valued = def(
            WEAPON_ID,
            2,
            10,
            ItemPrice::Formula,
            weapon_kind(Some(SkillNumber(19))),
        );
        let mut item = instance(WEAPON_ID, 7);
        item.skill = SkillRoll::WithSkill;
        assert_eq!(buying_price(&worthless, &item), Zen(37_700));
        assert_eq!(buying_price(&valued, &item), Zen(94_300));
        // The bare item prices exactly like the worthless-skill one.
        assert_eq!(
            buying_price(&worthless, &instance(WEAPON_ID, 7)),
            Zen(37_700)
        );
    }

    #[test]
    fn higher_option_levels_scale_by_the_classic_seven_doubling() {
        let weapon = def(WEAPON_ID, 2, 10, ItemPrice::Formula, weapon_kind(None));
        let mut item = instance(WEAPON_ID, 0);
        item.normal_option = Some(option(NormalOption::PhysicalDamage, OptionLevel::L2));
        assert_eq!(buying_price(&weapon, &item), Zen(1_700));
        item.normal_option = Some(option(NormalOption::PhysicalDamage, OptionLevel::L4));
        assert_eq!(buying_price(&weapon, &item), Zen(4_700));
    }

    #[test]
    fn a_wing_bonus_augment_adds_a_quarter() {
        let wing = def(WING_ID, 3, 100, ItemPrice::Formula, wing_kind());
        let mut item = instance(WING_ID, 0);
        item.augment = CraftedAugment::WingBonus {
            bonus: SecondWingBonus::Command,
        };
        assert_eq!(buying_price(&wing, &item), Zen(69_250_000));
    }

    #[test]
    fn the_cubic_branch_scales_only_by_the_health_recovery_option() {
        let ring = def(WEAPON_ID, 1, 20, ItemPrice::Formula, ring_kind());
        // dl = 20 + 9 = 29 → 29³ + 100 = 24,489 → 24,400.
        assert_eq!(buying_price(&ring, &instance(WEAPON_ID, 3)), Zen(24_400));
        let mut healing = instance(WEAPON_ID, 3);
        healing.normal_option = Some(option(NormalOption::HealthRecoveryPct, OptionLevel::L2));
        // price += price·2 → 73,467 → 73,400.
        assert_eq!(buying_price(&ring, &healing), Zen(73_400));
        // A non-health option adds nothing on the cubic branch.
        let mut plain = instance(WEAPON_ID, 3);
        plain.normal_option = Some(option(NormalOption::Defense, OptionLevel::L2));
        assert_eq!(buying_price(&ring, &plain), Zen(24_400));
    }

    #[test]
    fn an_excellent_cubic_item_gains_only_the_drop_level_bonus() {
        let ring = def(WEAPON_ID, 1, 20, ItemPrice::Formula, ring_kind());
        let mut item = instance(WEAPON_ID, 3);
        item.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Armor {
                options: ExcellentArmorSet::with_first(ExcellentArmorOption::MaxHealth, []),
            },
        };
        // dl = 20 + 9 + 25 = 54 → 54³ + 100 = 157,564 → 157,500 — no ×2.
        assert_eq!(buying_price(&ring, &item), Zen(157_500));
    }

    #[test]
    fn a_consumable_doubles_per_level_and_scales_by_piece_count() {
        let potion = def(
            WEAPON_ID,
            1,
            10,
            ItemPrice::Fixed { zen: Zen(83) },
            ItemKind::Consumable {
                effect: ConsumeEffect::Healing {
                    tier: HealingTier::Small,
                },
            },
        );
        let mut stack = instance(WEAPON_ID, 0);
        stack.durability = Durability::full(3);
        // 83 → tens 80 → ×3 pieces = 240.
        assert_eq!(buying_price(&potion, &stack), Zen(240));
        let mut leveled = instance(WEAPON_ID, 2);
        leveled.durability = Durability::full(3);
        // 83·4 = 332 → tens 330 → ×3 = 990.
        assert_eq!(buying_price(&potion, &leveled), Zen(990));
    }

    #[test]
    fn jewels_price_fixed_and_old_values_come_from_the_legacy_table() {
        let cases = [
            (JewelKind::Bless, 9_000_000, 100_000),
            (JewelKind::Soul, 6_000_000, 70_000),
            (JewelKind::Chaos, 810_000, 40_000),
            (JewelKind::Life, 45_000_000, 450_000),
            (JewelKind::Creation, 36_000_000, 450_000),
        ];
        for (jewel, current, old) in cases {
            let record = jewel_def(jewel, current);
            let item = instance(record.id, 0);
            assert_eq!(buying_price(&record, &item), Zen(current));
            assert_eq!(old_buying_price(&record, &item), Zen(old));
        }
    }

    #[test]
    fn old_buying_price_falls_through_to_buying_price_for_a_non_jewel() {
        let weapon = def(WEAPON_ID, 2, 10, ItemPrice::Formula, weapon_kind(None));
        let mut item = instance(WEAPON_ID, 6);
        item.normal_option = Some(option(NormalOption::PhysicalDamage, OptionLevel::L1));
        assert_eq!(
            old_buying_price(&weapon, &item),
            buying_price(&weapon, &item)
        );
    }

    #[test]
    fn a_per_level_price_reads_the_table_clamped_to_its_last_row() {
        let feather = def(
            WEAPON_ID,
            1,
            78,
            ItemPrice::PerLevel {
                zen_by_level: PerLevelPrice::try_from(vec![Zen(180_000), Zen(7_500_000)]).unwrap(),
            },
            ItemKind::MixMaterial,
        );
        assert_eq!(
            buying_price(&feather, &instance(WEAPON_ID, 0)),
            Zen(180_000)
        );
        assert_eq!(
            buying_price(&feather, &instance(WEAPON_ID, 1)),
            Zen(7_500_000)
        );
        assert_eq!(
            buying_price(&feather, &instance(WEAPON_ID, 15)),
            Zen(7_500_000)
        );
    }

    #[test]
    fn the_dinorant_special_prices_per_crafted_option() {
        use crate::components::item_instance::DinorantOptionSet;
        use crate::components::item_options::DinorantOption;
        let dinorant = def(
            WEAPON_ID,
            1,
            110,
            ItemPrice::Formula,
            ItemKind::Pet {
                ride: PetRide::FlyingMount,
                bonuses: Vec::new(),
                skill: Some(SkillNumber(49)),
                classes: ClassSet::NONE,
                wear: wear(),
            },
        );
        let bare = instance(WEAPON_ID, 0);
        assert_eq!(buying_price(&dinorant, &bare), Zen(960_000));
        let mut augmented = instance(WEAPON_ID, 0);
        augmented.augment = CraftedAugment::Dinorant {
            options: DinorantOptionSet::with_first(
                DinorantOption::DamageAbsorb,
                [DinorantOption::AttackSpeed],
            ),
        };
        assert_eq!(buying_price(&dinorant, &augmented), Zen(1_560_000));
    }

    #[test]
    fn a_ground_pet_routes_to_the_cubic_branch() {
        let uniria = def(
            WEAPON_ID,
            1,
            25,
            ItemPrice::Formula,
            ItemKind::Pet {
                ride: PetRide::GroundMount,
                bonuses: Vec::new(),
                skill: None,
                classes: ClassSet::NONE,
                wear: wear(),
            },
        );
        // 25³ + 100 = 15,725 → 15,700.
        assert_eq!(buying_price(&uniria, &instance(WEAPON_ID, 0)), Zen(15_700));
    }

    #[test]
    fn the_width_discount_hits_narrow_wields_and_every_shield() {
        // Narrow sword: dl 16 → base 1,892 → ×80/100 = 1,513 → 1,500.
        let sword = def(WEAPON_ID, 1, 16, ItemPrice::Formula, weapon_kind(None));
        assert_eq!(buying_price(&sword, &instance(WEAPON_ID, 0)), Zen(1_500));
        // A wide (2-cell) shield still gets the discount.
        let shield = def(
            WEAPON_ID,
            2,
            3,
            ItemPrice::Formula,
            ItemKind::Shield {
                defense: 1,
                defense_rate: 4,
                skill: None,
                classes: ClassSet::NONE,
                wear: wear(),
            },
        );
        // dl 3 → base 148 → ×80/100 = 118 → tens 110.
        assert_eq!(buying_price(&shield, &instance(WEAPON_ID, 0)), Zen(110));
    }

    #[test]
    fn rounding_truncates_hundreds_then_tens_and_leaves_small_values() {
        assert_eq!(round_price(1_589_356), 1_589_300);
        assert_eq!(round_price(1000), 1000);
        assert_eq!(round_price(999), 990);
        assert_eq!(round_price(118), 110);
        assert_eq!(round_price(99), 99);
        assert_eq!(round_price(0), 0);
    }
}
