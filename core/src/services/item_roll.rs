//! The drop-time option roll: turns a decided `{item, level, rarity}` drop into
//! a full [`ItemInstance`], rolling the orthogonal normal option, luck, skill,
//! and rarity payload against the injected [`OptionRollPolicy`]. `level` and
//! `rarity` are inputs from the loot service's `Drop::Item` — never re-rolled.
//!
//! Determinism rests on a fixed RNG draw order (design §E.2): each step draws
//! its words only when its per-kind gate fires, so an ineligible kind
//! (jewels, consumables, ammunition, orbs, materials, fruit) consumes zero
//! words. Every gate is total over [`ItemKind`] with explicit or-patterns.

use core::num::NonZeroUsize;

use rand_core::RngCore;

use crate::components::item_instance::{
    Durability, ExcellentArmorSet, ExcellentCat, ExcellentOptions, ExcellentWeaponSet,
    ItemInstance, LuckRoll, RarityRoll, RolledNormalOption, SkillRoll,
};
use crate::components::item_options::{AncientBonusLevel, ExcellentCategory, NormalOption};
use crate::components::item_quality::ItemRarity;
use crate::components::levels::OptionLevel;
use crate::components::units::{ItemLevel, Percent};
use crate::data::item_definitions::{ItemDefinition, ItemKind};
use crate::data::option_roll::OptionRollPolicy;
use crate::rng::uniform_below_usize;
use crate::services::chance::{roll_per_10000, roll_percent};
use crate::services::item_rules::max_durability;

/// The Jewel-of-Life option levels in ascending order — the filtered draw pool
/// of [`draw_option_level`].
const OPTION_LEVELS: [OptionLevel; 4] = [
    OptionLevel::L1,
    OptionLevel::L2,
    OptionLevel::L3,
    OptionLevel::L4,
];

/// The two ancient bonus tiers — the uniform draw pool for an ancient roll.
const ANCIENT_BONUSES: [AncientBonusLevel; 2] = [AncientBonusLevel::One, AncientBonusLevel::Two];

/// Whether an item's durability follows the enhancement/rarity curve or is a
/// flat stack/round count.
enum DurabilityCurve {
    /// Wearable equipment — durability grows with enhancement and rarity.
    Wear,
    /// A stackable/ammunition item — the raw base count, no curve.
    Flat,
}

/// Rolls a dropped item into a full instance. `level` and `rarity` are the
/// loot service's decided inputs, never re-rolled. Injected RNG only; no float,
/// no clock. Every draw routes through the [`crate::services::chance`] seam in
/// the fixed order documented in the module header.
#[must_use]
pub fn roll_dropped_item(
    def: &ItemDefinition,
    level: ItemLevel,
    rarity: ItemRarity,
    policy: &OptionRollPolicy,
    rng: &mut impl RngCore,
) -> ItemInstance {
    let kind = &def.kind;

    // Steps 1-2: the normal option and, if present, its level.
    let normal_option = match eligible_normal_option(kind) {
        Some(option) if roll_per_10000(policy.item_option_roll_per_10000, rng) => {
            Some(RolledNormalOption {
                option,
                level: draw_option_level(policy.max_dropped_option_level, rng),
            })
        }
        Some(_) | None => None,
    };

    // Step 3: luck.
    let luck = if grants_luck(kind) && roll_per_10000(policy.luck_roll_per_10000, rng) {
        LuckRoll::Lucky
    } else {
        LuckRoll::Plain
    };

    // Step 4: skill. Excellent/ancient items always carry their skill (no
    // draw); an ordinary item rolls it at 50%.
    // W-SRC: 50% skill chance on a normal drop (facts 5:44 "skill: 50% chance").
    let skill = if grants_skill(kind) {
        match rarity {
            ItemRarity::Excellent | ItemRarity::Ancient => SkillRoll::WithSkill,
            ItemRarity::Normal => {
                if roll_percent(Percent::clamped(50), rng) {
                    SkillRoll::WithSkill
                } else {
                    SkillRoll::NoSkill
                }
            }
        }
    } else {
        SkillRoll::NoSkill
    };

    // Step 5: the rarity payload, matching the decided input rarity.
    let roll = match rarity {
        ItemRarity::Normal => RarityRoll::Normal,
        ItemRarity::Ancient => RarityRoll::Ancient {
            bonus: draw_ancient_bonus(rng),
        },
        ItemRarity::Excellent => match excellent_category(kind) {
            Some(category) => RarityRoll::Excellent {
                options: roll_excellent(category, policy, rng),
            },
            // `loot::item_drop` gates the excellent pool on excellent-capability
            // (`is_excellent_capable`), so an authentic drop never pairs
            // Excellent with a kind that has no excellent set: this arm is
            // unreachable for real drops and remains only to keep the roll total
            // over its bare `ItemRarity` input — total, no panic.
            None => RarityRoll::Normal,
        },
    };

    let durability = roll_durability(def, level, rarity, kind);

    ItemInstance {
        item: def.id,
        level,
        roll,
        normal_option,
        luck,
        skill,
        durability,
    }
}

/// The single normal option a kind may roll, if any. Total over [`ItemKind`].
fn eligible_normal_option(kind: &ItemKind) -> Option<NormalOption> {
    match kind {
        ItemKind::Weapon { .. } | ItemKind::Bow { .. } | ItemKind::Crossbow { .. } => {
            Some(NormalOption::PhysicalDamage)
        }
        ItemKind::Staff { .. } => Some(NormalOption::WizardryDamage),
        ItemKind::Shield { .. } => Some(NormalOption::DefenseRate),
        ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. } => Some(NormalOption::Defense),
        // Pre-S3 wings carry a single Jewel-of-Life option; take it (none if the
        // wing lists no option).
        ItemKind::Wings { jol_options, .. } => jol_options.first().copied(),
        ItemKind::Ring { option, .. } | ItemKind::Pendant { option, .. } => Some(*option),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Pet { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => None,
    }
}

/// Whether a kind can roll luck. Equipment does; jewelry and non-equippables do
/// not. Total over [`ItemKind`].
// W-SRC: jewelry (ring/pendant) grants no luck (facts 2:32 sourced-absent).
fn grants_luck(kind: &ItemKind) -> bool {
    match kind {
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
        | ItemKind::Wings { .. } => true,
        ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Pet { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => false,
    }
}

/// Whether a kind can grant its weapon skill — true exactly when the definition
/// carries a skill. Reads the field, not a kind list. Total over [`ItemKind`].
fn grants_skill(kind: &ItemKind) -> bool {
    match kind {
        ItemKind::Weapon { skill, .. }
        | ItemKind::Bow { skill, .. }
        | ItemKind::Crossbow { skill, .. }
        | ItemKind::Staff { skill, .. }
        | ItemKind::Shield { skill, .. }
        | ItemKind::Pet { skill, .. } => skill.is_some(),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
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
        | ItemKind::StatFruit => false,
    }
}

/// The excellent set category a kind rolls, if any. Total over [`ItemKind`].
fn excellent_category(kind: &ItemKind) -> Option<ExcellentCat> {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. } => Some(ExcellentCat::Weapon),
        ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Ring { .. } => Some(ExcellentCat::Armor),
        ItemKind::Pendant { excellent, .. } => Some(excellent_cat_of(*excellent)),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => None,
    }
}

/// Whether a kind can roll an excellent set at all — the capability predicate
/// the loot pool gates on so an `Excellent` drop is only ever stamped on a kind
/// that has an excellent set. Exposes the capability without leaking the private
/// [`ExcellentCat`] discriminator.
pub(crate) fn is_excellent_capable(kind: &ItemKind) -> bool {
    excellent_category(kind).is_some()
}

/// Bridges a definition's stored [`ExcellentCategory`] (which carries the weapon
/// damage kind) down to the bare [`ExcellentCat`] discriminator.
fn excellent_cat_of(category: ExcellentCategory) -> ExcellentCat {
    match category {
        ExcellentCategory::Armor => ExcellentCat::Armor,
        ExcellentCategory::Weapon { .. } => ExcellentCat::Weapon,
    }
}

/// How a kind's durability is computed. Total over [`ItemKind`].
fn durability_curve(kind: &ItemKind) -> DurabilityCurve {
    match kind {
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
        | ItemKind::TransformationRing { .. } => DurabilityCurve::Wear,
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => DurabilityCurve::Flat,
    }
}

/// The item's full durability. Wearable kinds follow the enhancement/rarity
/// curve when the level is an enhancement level; box-tier levels (12..=15, which
/// have no enhancement level) and flat/stackable kinds take the raw base count.
fn roll_durability(
    def: &ItemDefinition,
    level: ItemLevel,
    rarity: ItemRarity,
    kind: &ItemKind,
) -> Durability {
    match durability_curve(kind) {
        DurabilityCurve::Flat => Durability::full(def.durability),
        DurabilityCurve::Wear => match level.enhance_level() {
            Some(enhance) => Durability::full(max_durability(def.durability, enhance, rarity)),
            None => Durability::full(def.durability),
        },
    }
}

/// Draws a normal option level uniformly among `L1..=cap` — one word, total.
/// Walks the variant list filtered to `<= cap`, the [`crate::services::chance`]
/// index-draw grain, so no narrowing cast is needed.
fn draw_option_level(cap: OptionLevel, rng: &mut impl RngCore) -> OptionLevel {
    // The eligible levels are exactly L1..=cap, so their count is cap's wire
    // value (1..=4).
    let bound = NonZeroUsize::MIN.saturating_add(usize::from(cap.wire()).saturating_sub(1));
    let target = uniform_below_usize(bound, rng);
    let mut position = 0usize;
    for level in OPTION_LEVELS {
        if level <= cap {
            if position == target {
                return level;
            }
            position = position.saturating_add(1);
        }
    }
    OptionLevel::L1
}

/// Draws an ancient bonus tier uniformly from `{One, Two}` — one word, total.
fn draw_ancient_bonus(rng: &mut impl RngCore) -> AncientBonusLevel {
    let bound = NonZeroUsize::MIN.saturating_add(ANCIENT_BONUSES.len() - 1);
    let target = uniform_below_usize(bound, rng);
    let mut position = 0usize;
    for bonus in ANCIENT_BONUSES {
        if position == target {
            return bonus;
        }
        position = position.saturating_add(1);
    }
    AncientBonusLevel::One
}

/// Rolls the excellent set for a category: the first option is guaranteed
/// (drawn uniformly), then each additional slot up to the policy cap rolls the
/// extra-option chance, drawing from the shrinking pool on success. Distinct by
/// construction — a bounded partial shuffle, never rejection-resampling — so the
/// per-branch word count is fixed and determinism holds.
fn roll_excellent(
    category: ExcellentCat,
    policy: &OptionRollPolicy,
    rng: &mut impl RngCore,
) -> ExcellentOptions {
    match category {
        ExcellentCat::Armor => {
            let (first, rest) = draw_distinct(ExcellentArmorSet::OPTIONS, policy, rng);
            ExcellentOptions::Armor {
                options: ExcellentArmorSet::with_first(first, rest),
            }
        }
        ExcellentCat::Weapon => {
            let (first, rest) = draw_distinct(ExcellentWeaponSet::OPTIONS, policy, rng);
            ExcellentOptions::Weapon {
                options: ExcellentWeaponSet::with_first(first, rest),
            }
        }
    }
}

/// Draws the guaranteed first option plus any extra distinct options from a
/// six-element pool via a bounded partial shuffle. The extra slots are capped at
/// `policy.max_excellent_options_per_drop - 1` and stop when the pool empties.
fn draw_distinct<T: Copy>(
    all: [T; 6],
    policy: &OptionRollPolicy,
    rng: &mut impl RngCore,
) -> (T, Vec<T>) {
    let mut pool: Vec<T> = all.to_vec();
    let first = take_one(&mut pool, rng);
    let mut rest = Vec::new();
    let extra_slots = usize::from(policy.max_excellent_options_per_drop).saturating_sub(1);
    for _ in 0..extra_slots {
        if pool.is_empty() {
            break;
        }
        if roll_per_10000(policy.extra_excellent_option_roll_per_10000, rng) {
            rest.push(take_one(&mut pool, rng));
        }
    }
    (first, rest)
}

/// Removes and returns one element drawn uniformly from a non-empty pool — the
/// Fisher-Yates partial-shuffle primitive, one word. The caller guarantees the
/// pool is non-empty (guaranteed first, then `is_empty` guard).
fn take_one<T>(pool: &mut Vec<T>, rng: &mut impl RngCore) -> T {
    let bound = NonZeroUsize::MIN.saturating_add(pool.len().saturating_sub(1));
    let index = uniform_below_usize(bound, rng);
    pool.swap_remove(index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::class::ClassSet;
    use crate::components::item_ref::ItemRef;
    use crate::components::units::{ChancePer10000, Zen};
    use crate::data::common::{Provenance, SkillNumber, SourceVersion};
    use crate::data::item_definitions::{
        ConsumeEffect, HealingTier, ItemPrice, JewelKind, WeaponHandling, WearRequirements,
    };

    /// Deterministic `SplitMix64` for replayable tests.
    struct TestRng {
        state: u64,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }
    }

    impl RngCore for TestRng {
        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            for chunk in dst.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                    *slot = *byte;
                }
            }
        }
    }

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

    fn weapon_def(skill: Option<SkillNumber>) -> ItemDefinition {
        ItemDefinition {
            id: ItemRef {
                group: 0,
                number: 3,
            },
            provenance: provenance(),
            width: 1,
            height: 3,
            drops_from_monsters: true,
            drop_level: 10,
            max_item_level: ItemLevel::new(15).unwrap(),
            durability: 20,
            price: ItemPrice::Formula,
            kind: ItemKind::Weapon {
                handling: WeaponHandling::OneHanded,
                min_damage: 5,
                max_damage: 12,
                attack_speed: 30,
                skill,
                classes: ClassSet::NONE,
                wear: wear(),
            },
        }
    }

    fn jewel_def() -> ItemDefinition {
        ItemDefinition {
            id: ItemRef {
                group: 14,
                number: 13,
            },
            provenance: provenance(),
            width: 1,
            height: 1,
            drops_from_monsters: true,
            drop_level: 0,
            max_item_level: ItemLevel::ZERO,
            durability: 1,
            price: ItemPrice::Fixed { zen: Zen(9) },
            kind: ItemKind::Jewel {
                jewel: JewelKind::Bless,
            },
        }
    }

    fn consumable_def() -> ItemDefinition {
        ItemDefinition {
            id: ItemRef {
                group: 14,
                number: 0,
            },
            provenance: provenance(),
            width: 1,
            height: 1,
            drops_from_monsters: true,
            drop_level: 0,
            max_item_level: ItemLevel::ZERO,
            durability: 1,
            price: ItemPrice::Fixed { zen: Zen(1) },
            kind: ItemKind::Consumable {
                effect: ConsumeEffect::Healing {
                    tier: HealingTier::Small,
                },
            },
        }
    }

    fn always() -> OptionRollPolicy {
        OptionRollPolicy {
            item_option_roll_per_10000: ChancePer10000::ALWAYS,
            luck_roll_per_10000: ChancePer10000::ALWAYS,
            extra_excellent_option_roll_per_10000: ChancePer10000::ALWAYS,
            second_wing_bonus_roll_per_10000: ChancePer10000::ALWAYS,
            dinorant_option_roll_per_10000: ChancePer10000::ALWAYS,
            max_excellent_options_per_drop: 3,
            max_dropped_option_level: OptionLevel::L4,
            review: None,
        }
    }

    fn never() -> OptionRollPolicy {
        OptionRollPolicy {
            item_option_roll_per_10000: ChancePer10000::NEVER,
            luck_roll_per_10000: ChancePer10000::NEVER,
            extra_excellent_option_roll_per_10000: ChancePer10000::NEVER,
            second_wing_bonus_roll_per_10000: ChancePer10000::NEVER,
            dinorant_option_roll_per_10000: ChancePer10000::NEVER,
            max_excellent_options_per_drop: 3,
            max_dropped_option_level: OptionLevel::L4,
            review: None,
        }
    }

    #[test]
    fn always_policy_gives_a_weapon_its_normal_option_and_luck() {
        let def = weapon_def(None);
        let mut rng = TestRng::new(1);
        let instance = roll_dropped_item(
            &def,
            ItemLevel::new(7).unwrap(),
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        assert_eq!(instance.luck, LuckRoll::Lucky);
        let option = instance
            .normal_option
            .expect("always policy rolls an option");
        assert_eq!(option.option, NormalOption::PhysicalDamage);
    }

    #[test]
    fn never_policy_gives_a_weapon_no_option_and_no_luck() {
        let def = weapon_def(None);
        let mut rng = TestRng::new(1);
        let instance = roll_dropped_item(
            &def,
            ItemLevel::new(7).unwrap(),
            ItemRarity::Normal,
            &never(),
            &mut rng,
        );
        assert_eq!(instance.luck, LuckRoll::Plain);
        assert!(instance.normal_option.is_none());
    }

    #[test]
    fn excellent_weapon_gets_a_weapon_set_and_skill_when_defined() {
        let def = weapon_def(Some(SkillNumber(19)));
        for seed in 0u64..32 {
            let mut rng = TestRng::new(seed);
            let instance = roll_dropped_item(
                &def,
                ItemLevel::new(9).unwrap(),
                ItemRarity::Excellent,
                &always(),
                &mut rng,
            );
            assert_eq!(instance.roll.rarity(), ItemRarity::Excellent);
            assert_eq!(instance.skill, SkillRoll::WithSkill);
            match &instance.roll {
                RarityRoll::Excellent { options } => {
                    assert_eq!(options.category(), ExcellentCat::Weapon);
                    match options {
                        ExcellentOptions::Weapon { options } => {
                            let count = options.count();
                            assert!((1..=3).contains(&count), "count {count}");
                        }
                        ExcellentOptions::Armor { .. } => panic!("weapon expected"),
                    }
                }
                RarityRoll::Normal | RarityRoll::Ancient { .. } => panic!("excellent expected"),
            }
        }
    }

    #[test]
    fn excellent_with_never_extra_yields_exactly_one_option() {
        let def = weapon_def(None);
        let mut rng = TestRng::new(3);
        let instance = roll_dropped_item(
            &def,
            ItemLevel::ZERO,
            ItemRarity::Excellent,
            &never(),
            &mut rng,
        );
        match &instance.roll {
            RarityRoll::Excellent {
                options: ExcellentOptions::Weapon { options },
            } => assert_eq!(options.count(), 1),
            RarityRoll::Excellent { .. } | RarityRoll::Normal | RarityRoll::Ancient { .. } => {
                panic!("excellent weapon expected")
            }
        }
    }

    #[test]
    fn ancient_draws_a_bonus_tier_and_no_excellent_set() {
        let def = weapon_def(None);
        let mut saw_one = false;
        let mut saw_two = false;
        for seed in 0u64..32 {
            let mut rng = TestRng::new(seed);
            let instance = roll_dropped_item(
                &def,
                ItemLevel::new(5).unwrap(),
                ItemRarity::Ancient,
                &always(),
                &mut rng,
            );
            match instance.roll {
                RarityRoll::Ancient { bonus } => match bonus {
                    AncientBonusLevel::One => saw_one = true,
                    AncientBonusLevel::Two => saw_two = true,
                },
                RarityRoll::Normal | RarityRoll::Excellent { .. } => panic!("ancient expected"),
            }
        }
        assert!(saw_one && saw_two, "both ancient tiers should appear");
    }

    #[test]
    fn wearable_durability_follows_the_curve_and_is_full() {
        let def = weapon_def(None);
        let mut rng = TestRng::new(1);
        let instance = roll_dropped_item(
            &def,
            ItemLevel::new(7).unwrap(),
            ItemRarity::Excellent,
            &always(),
            &mut rng,
        );
        let expected = max_durability(
            def.durability,
            ItemLevel::new(7).unwrap().enhance_level().unwrap(),
            ItemRarity::Excellent,
        );
        assert_eq!(instance.durability.current(), instance.durability.max());
        assert_eq!(instance.durability.max(), expected);
    }

    #[test]
    fn box_tier_wearable_takes_the_base_durability() {
        let def = weapon_def(None);
        let mut rng = TestRng::new(1);
        // Level 12 has no enhancement level (a box tier), so durability is the base.
        let instance = roll_dropped_item(
            &def,
            ItemLevel::new(12).unwrap(),
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        assert_eq!(instance.durability.max(), def.durability);
    }

    #[test]
    fn a_jewel_consumes_zero_random_words() {
        let def = jewel_def();
        let mut rng = TestRng::new(9);
        let mut probe = TestRng::new(9);
        let instance = roll_dropped_item(
            &def,
            ItemLevel::ZERO,
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        assert_eq!(instance.roll, RarityRoll::Normal);
        assert!(instance.normal_option.is_none());
        assert_eq!(instance.luck, LuckRoll::Plain);
        assert_eq!(instance.skill, SkillRoll::NoSkill);
        assert_eq!(instance.durability.max(), def.durability);
        // No draw happened: the generators still agree.
        assert_eq!(rng.next_u64(), probe.next_u64());
    }

    #[test]
    fn a_consumable_is_flat_and_bare() {
        let def = consumable_def();
        let mut rng = TestRng::new(2);
        let mut probe = TestRng::new(2);
        let instance = roll_dropped_item(
            &def,
            ItemLevel::ZERO,
            ItemRarity::Normal,
            &always(),
            &mut rng,
        );
        assert_eq!(instance.durability.max(), def.durability);
        assert_eq!(rng.next_u64(), probe.next_u64());
    }

    #[test]
    fn same_seed_gives_equal_instance_and_equal_word_consumption() {
        let def = weapon_def(Some(SkillNumber(19)));
        let mut a = TestRng::new(7);
        let mut b = TestRng::new(7);
        let ia = roll_dropped_item(
            &def,
            ItemLevel::new(9).unwrap(),
            ItemRarity::Excellent,
            &always(),
            &mut a,
        );
        let ib = roll_dropped_item(
            &def,
            ItemLevel::new(9).unwrap(),
            ItemRarity::Excellent,
            &always(),
            &mut b,
        );
        assert_eq!(ia, ib);
        assert_eq!(a.next_u64(), b.next_u64());
    }
}
