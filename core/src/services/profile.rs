//! Derivation of a fighter's [`CombatProfile`] from its source: a character's
//! class, level, and stats (with [`equipped_profile`] folding the worn
//! [`Equipment`] onto that gearless base), or a monster's combat block. Every
//! derived stat is a pooled single divide — the whole numerator is summed in
//! `u64` and floored once, never a sum of per-term truncations. Pre-S3
//! monsters carry zero special-hit chances; a character's come from its gear.

use crate::components::active_effect::ActiveEffects;
use crate::components::bonus::CombatBonus;
use crate::components::class::CharacterClass;
use crate::components::combat_profile::{CombatProfile, VitalMaxima, WeaponMode};
use crate::components::element::{Element, PerElement};
use crate::components::equipment::{Equipment, EquipmentSlot};
use crate::components::interval::Interval;
use crate::components::item_instance::{
    ExcellentArmorSet, ExcellentOptions, ExcellentWeaponSet, ItemInstance, LuckRoll, RarityRoll,
};
use crate::components::item_options::{
    ExcellentArmorOption, ExcellentCategory, ExcellentWeaponOption, NormalOption, WeaponDamageKind,
};
use crate::components::levels::{AmmoLevel, EnhanceLevel};
use crate::components::stats::Stats;
use crate::components::units::{ItemLevel, Level, Percent, Resistance};
use crate::data::atlas::Atlas;
use crate::data::item_definitions::{ItemDefinition, ItemKind, WeaponHandling};
use crate::data::monster_definitions::MonsterCombat;
use crate::entities::character::Character;
use crate::services::effects::effect_bonus;
use crate::services::item_rules::{
    ammunition_damage_percent, armor_defense_bonus, jewelry_resistance, shield_defense_bonus,
    shield_defense_rate_bonus, staff_rise_x2, weapon_damage_bonus, wing_absorb_percent,
    wing_damage_percent, wing_defense_bonus,
};
use crate::services::ratio::{floor_div_u64_to_u32, nonzero, scale_ratio};

/// The five trainable stats a character folds to, widened for pooled
/// arithmetic. Command is zero for the four non-command classes.
struct Attributes {
    level: u64,
    strength: u64,
    agility: u64,
    vitality: u64,
    energy: u64,
    command: u64,
}

/// Derives a character's combat profile and the class-formula vital capacities.
/// Reads class, level, and stats; the stored [`crate::components::vitals::Vitals`]
/// maxima are ignored — the class formula is authoritative on the compute path.
#[must_use]
pub fn character_profile(character: &Character) -> (CombatProfile, VitalMaxima) {
    let attributes = attributes_of(character.level(), character.stats());
    match character.class() {
        CharacterClass::DarkWizard | CharacterClass::SoulMaster => wizard_profile(&attributes),
        CharacterClass::DarkKnight | CharacterClass::BladeKnight => knight_profile(&attributes),
        CharacterClass::FairyElf | CharacterClass::MuseElf => elf_profile(&attributes),
        CharacterClass::MagicGladiator => magic_gladiator_profile(&attributes),
        CharacterClass::DarkLord => dark_lord_profile(&attributes),
    }
}

/// Derives a monster's combat profile from its Monster.txt combat block, its
/// resistance table, and its level. Physical damage is the authored span;
/// wizardry is absent, and pre-S3 monsters carry zero special-hit chances.
#[must_use]
pub fn monster_profile(
    combat: &MonsterCombat,
    resistances: &PerElement<Resistance>,
    level: Level,
) -> CombatProfile {
    CombatProfile {
        level,
        physical: Interval::spanning(combat.min_phys_damage, combat.max_phys_damage),
        wizardry: None,
        defense: combat.defense,
        attack_rate: combat.attack_rate,
        defense_rate: combat.defense_rate,
        resistances: *resistances,
        critical_chance: Percent::ZERO,
        excellent_chance: Percent::ZERO,
        defense_ignore_chance: Percent::ZERO,
        double_damage_chance: Percent::ZERO,
        incoming_damage_reduction: Percent::ZERO,
        flat_damage_add: 0,
        // A monster carries no gear: every gear magnitude is its real
        // zero/identity, and a monster always swings a single natural weapon.
        wizardry_rise_x2: 0,
        incoming_dd_pct: Percent::ZERO,
        wing_damage_pct: Percent::ZERO,
        wing_absorb_pct: Percent::ZERO,
        weapon_mode: WeaponMode::Single,
    }
}

// ── The equipment→profile fold ──────────────────────────────────────────────

// W-SRC: option/luck/excellent magnitudes and the span/defense assembly rules,
// sourced from OpenMU (era-shared pre-S3 data):
// normal option +4 per level (defense/physical/wizardry families) and the
// shield defense-RATE +5 per level (GameConfigurationInitializerBase.cs:76-143);
// luck +5% critical (GameConfigurationInitializerBase.cs:214-232); excellent
// damage ×1.02, damage +TotalLevel/20, excellent-chance +10%, defense-rate
// ×1.1, DamageDecrease 4% (Items/ExcellentOptions.cs); the excellent weapon
// implicit minDmg×25/dropLevel + 5 (ItemPowerUpFactory.cs:329-349);
// double-wield span ×0.55 (CharacterClassInitialization.cs:187-196); complete
// suits — rate ×1.1 at any level, defense ×(1+(setLevel−9)×0.05) at uniform
// level ≥ 10 (Items/ArmorInitializerBase.cs:54-107,447-477); jewelry base
// resistance 1 with Maximum aggregation (Version075/Items/Jewelery.cs:17,
// 171-173); defense halves ONCE on the whole sum
// (CharacterClassInitialization.cs:102-105).
/// The normal-option step: +4 per option level (physical/wizardry/defense).
const NORMAL_OPTION_STEP: u64 = 4;
/// The shield normal-option defense-rate step: +5 per option level.
const SHIELD_RATE_OPTION_STEP: u64 = 5;
/// Luck's flat critical-chance contribution, percent points.
const LUCK_CRITICAL_PCT: u64 = 5;
/// The excellent-damage-chance option's contribution, percent points.
const EXCELLENT_CHANCE_PCT: u64 = 10;
/// The excellent `DamageDecrease` option's contribution, percent points.
const EXCELLENT_DD_PCT: u64 = 4;
/// Excellent damage option: span ×102/100.
const EXCELLENT_DAMAGE_NUM: u32 = 102;
/// Excellent defense-rate option: rate ×110/100.
const EXCELLENT_RATE_NUM: u32 = 110;
/// Excellent weapon implicit: `min_damage × 25 / drop_level + 5`.
const EXCELLENT_IMPLICIT_NUM: u64 = 25;
/// Excellent weapon implicit flat tail.
const EXCELLENT_IMPLICIT_ADD: u64 = 5;
/// Excellent `DamagePerLevel` option divisor: `+ character_level / 20`.
const DAMAGE_PER_LEVEL_DIVISOR: u64 = 20;
/// Double-wield span numerator: ×55/100 (the strike head doubles back to 110%).
const DOUBLE_WIELD_SPAN_NUM: u32 = 55;
/// Complete-suit defense-rate multiplier: ×11/10 at any suit level.
const SUIT_RATE_NUM: u32 = 11;
/// Complete-suit defense-rate denominator.
const SUIT_RATE_DEN: u32 = 10;
/// Complete-suit defense step: +5 percent points per set level above 9.
const SUIT_DEFENSE_STEP: u32 = 5;
/// The first set level the uniform-suit defense multiplier fires at.
const SUIT_DEFENSE_FLOOR_LEVEL: u32 = 10;
/// The percent-ratio denominator shared by every span/rate multiplier.
const PCT_DEN: u32 = 100;

/// One worn, Atlas-resolved piece the fold reads: the rolled instance beside
/// its definition.
#[derive(Clone, Copy)]
struct WornPiece<'a> {
    item: &'a ItemInstance,
    def: &'a ItemDefinition,
}

/// Folds a character's worn equipment into the gearless combat profile,
/// producing the EQUIPPED profile a strike reads. Pure, draws no RNG. Resolves
/// each worn item through the Atlas (the data-loading port) — definitions are
/// never denormalized onto the instance. A broken (durability-0) worn item
/// contributes NOTHING and drops out of its suit while staying worn. The
/// empty-`Equipment` fold is the identity: every field is byte-identical to
/// [`character_profile`]'s gearless output. Vital maxima are untouched by gear
/// this wave — they stay the [`character_profile`] values.
#[must_use]
pub fn equipped_profile(character: &Character, worn: &Equipment, atlas: &Atlas) -> CombatProfile {
    let (base, _maxima) = character_profile(character);
    let resolved = resolve_worn(worn, atlas);
    fold_worn(
        base,
        character.class(),
        character.level(),
        character.stats(),
        &resolved,
    )
}

/// Every occupied slot resolved to its definition. An identity the Atlas does
/// not carry resolves to nothing — genuine optionality of an open item key.
fn resolve_worn<'a>(worn: &'a Equipment, atlas: &'a Atlas) -> Vec<WornPiece<'a>> {
    EquipmentSlot::ALL
        .iter()
        .filter_map(|&slot| {
            let item = worn.get(slot)?;
            let def = atlas.item(item.item)?;
            Some(WornPiece { item, def })
        })
        .collect()
}

/// The Atlas-free fold over resolved worn pieces — every gear dimension folded
/// onto the gearless base in one place, so the whole derivation is testable
/// over hand-built definitions.
fn fold_worn(
    base: CombatProfile,
    class: CharacterClass,
    level: Level,
    stats: Stats,
    resolved: &[WornPiece<'_>],
) -> CombatProfile {
    // W-SRC: a worn item at durability 0 contributes NOTHING — base values,
    // curves, options, suit membership — while staying equipped
    // (ItemPowerUpFactory.cs:38-41,97-98).
    let pieces: Vec<WornPiece<'_>> = resolved
        .iter()
        .copied()
        .filter(|piece| piece.item.durability.current() > 0)
        .collect();
    let suit = suit_status(&pieces);
    let mode = weapon_mode_of(class, &pieces);
    let (wing_damage_pct, wing_absorb_pct) = wing_percents(&pieces);
    let profile = CombatProfile {
        level: base.level,
        physical: physical_span(
            base.physical,
            &pieces,
            level,
            mode,
            ammo_span_percent(&pieces),
        ),
        wizardry: base
            .wizardry
            .map(|wizardry| wizardry_span(wizardry, &pieces, level)),
        defense: folded_defense(class, agility_of(stats), &pieces, suit),
        attack_rate: base.attack_rate,
        defense_rate: folded_defense_rate(base.defense_rate, &pieces, suit),
        resistances: folded_resistances(base.resistances, &pieces),
        critical_chance: folded_chance(
            base.critical_chance,
            luck_count(&pieces),
            LUCK_CRITICAL_PCT,
        ),
        excellent_chance: folded_chance(
            base.excellent_chance,
            weapon_option_count(&pieces, ExcellentWeaponOption::ExcellentDamageChance),
            EXCELLENT_CHANCE_PCT,
        ),
        defense_ignore_chance: base.defense_ignore_chance,
        double_damage_chance: base.double_damage_chance,
        incoming_damage_reduction: base.incoming_damage_reduction,
        flat_damage_add: base.flat_damage_add,
        wizardry_rise_x2: rise_of(&pieces),
        incoming_dd_pct: folded_chance(
            Percent::ZERO,
            armor_option_count(&pieces, ExcellentArmorOption::DamageDecrease),
            EXCELLENT_DD_PCT,
        ),
        wing_damage_pct,
        wing_absorb_pct,
        weapon_mode: mode,
    };
    // Pet bonuses are already-resolved `CombatBonus` contributions — the same
    // currency the effect fold consumes, folded through the same seam.
    pieces
        .iter()
        .flat_map(|piece| pet_bonuses(&piece.def.kind))
        .copied()
        .fold(profile, fold_profile_bonus)
}

/// The agility stat on either stat shape.
fn agility_of(stats: Stats) -> u16 {
    match stats {
        Stats::Standard { agility, .. } | Stats::WithCommand { agility, .. } => agility,
    }
}

/// A rolled normal option's contribution when it is the `wanted` effect:
/// `step × option_level`, else 0 — one routing rule shared by every span,
/// defense, and rate fold.
fn normal_option_add(item: &ItemInstance, wanted: NormalOption, step: u64) -> u64 {
    match item.normal_option {
        Some(rolled) if rolled.option == wanted => {
            step.saturating_mul(u64::from(rolled.level.wire()))
        }
        Some(_) | None => 0,
    }
}

/// The physical min/max flats of a hand weapon; `None` for every non-weapon
/// kind (the [`ItemKind::skill`] accessor grain).
fn weapon_flats(kind: &ItemKind) -> Option<(u16, u16)> {
    match kind {
        ItemKind::Weapon {
            min_damage,
            max_damage,
            ..
        }
        | ItemKind::Bow {
            min_damage,
            max_damage,
            ..
        }
        | ItemKind::Crossbow {
            min_damage,
            max_damage,
            ..
        }
        | ItemKind::Staff {
            min_damage,
            max_damage,
            ..
        } => Some((*min_damage, *max_damage)),
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
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
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => None,
    }
}

/// The `min_damage` the excellent weapon implicit scales — physical weapons
/// only (staffs carry their own rise implicit, out of this wave's fold).
fn excellent_implicit_min(kind: &ItemKind) -> Option<u16> {
    match kind {
        ItemKind::Weapon { min_damage, .. }
        | ItemKind::Bow { min_damage, .. }
        | ItemKind::Crossbow { min_damage, .. } => Some(*min_damage),
        ItemKind::Staff { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
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
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => None,
    }
}

/// Which damage span a kind's excellent WEAPON options boost: staffs boost
/// wizardry, pendants their data-declared kind, every other carrier physical.
/// Total — a weapon set on a kind with no weapon category is unrepresentable
/// (reconcile proves it), so the physical arm is a harmless total answer.
fn weapon_option_span(kind: &ItemKind) -> WeaponDamageKind {
    match kind {
        ItemKind::Staff { .. } => WeaponDamageKind::Wizardry,
        ItemKind::Pendant { excellent, .. } => match excellent {
            ExcellentCategory::Weapon { damage } => *damage,
            ExcellentCategory::Armor => WeaponDamageKind::Physical,
        },
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::Ring { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => WeaponDamageKind::Physical,
    }
}

/// The instance's excellent WEAPON set, when it carries one.
fn excellent_weapon_set(item: &ItemInstance) -> Option<ExcellentWeaponSet> {
    match &item.roll {
        RarityRoll::Excellent {
            options: ExcellentOptions::Weapon { options },
        } => Some(*options),
        RarityRoll::Excellent {
            options: ExcellentOptions::Armor { .. },
        }
        | RarityRoll::Normal
        | RarityRoll::Ancient { .. } => None,
    }
}

/// The instance's excellent ARMOR set, when it carries one.
fn excellent_armor_set(item: &ItemInstance) -> Option<ExcellentArmorSet> {
    match &item.roll {
        RarityRoll::Excellent {
            options: ExcellentOptions::Armor { options },
        } => Some(*options),
        RarityRoll::Excellent {
            options: ExcellentOptions::Weapon { .. },
        }
        | RarityRoll::Normal
        | RarityRoll::Ancient { .. } => None,
    }
}

/// The additive and multiplicative excellent-weapon contributions to one span
/// kind: `(flat_add, damage_pct_steps)` — the `DamagePerLevel` add and the
/// number of ×102/100 applications. Kill-hook and speed options route to
/// nothing (no combat field).
fn excellent_weapon_contribution(
    piece: WornPiece<'_>,
    span: WeaponDamageKind,
    wearer_level: Level,
) -> (u64, u32) {
    if weapon_option_span(&piece.def.kind) != span {
        return (0, 0);
    }
    let Some(set) = excellent_weapon_set(piece.item) else {
        return (0, 0);
    };
    let mut flat: u64 = 0;
    let mut pct_steps: u32 = 0;
    for option in set.iter() {
        match option {
            ExcellentWeaponOption::DamagePct => pct_steps = pct_steps.saturating_add(1),
            ExcellentWeaponOption::DamagePerLevel => {
                flat =
                    flat.saturating_add(u64::from(wearer_level.get()) / DAMAGE_PER_LEVEL_DIVISOR);
            }
            ExcellentWeaponOption::ManaAfterKill
            | ExcellentWeaponOption::HealthAfterKill
            | ExcellentWeaponOption::AttackSpeed
            | ExcellentWeaponOption::ExcellentDamageChance => {}
        }
    }
    (flat, pct_steps)
}

/// The equipped physical span: gearless span + every worn weapon's flats,
/// +level curve, normal physical option, and excellent adds — then the
/// multiplicative steps in pinned order: ×102/100 per excellent damage
/// option, × the ammunition percent, × double-wield 55/100.
fn physical_span(
    base: Interval<u16>,
    pieces: &[WornPiece<'_>],
    wearer_level: Level,
    mode: WeaponMode,
    ammo_percent: u8,
) -> Interval<u16> {
    let mut min = u64::from(base.min());
    let mut max = u64::from(base.max());
    let mut pct_steps: u32 = 0;
    for piece in pieces {
        let option =
            normal_option_add(piece.item, NormalOption::PhysicalDamage, NORMAL_OPTION_STEP);
        min = min.saturating_add(option);
        max = max.saturating_add(option);
        if let Some((weapon_min, weapon_max)) = weapon_flats(&piece.def.kind) {
            // W-SRC: the weapon +level curve applies to BOTH span ends
            // (Version075/Items/Weapons.cs:27,289-293).
            let curve = u64::from(weapon_damage_bonus(
                piece.item.level.enhance_level_or_zero(),
            ));
            min = min
                .saturating_add(u64::from(weapon_min))
                .saturating_add(curve);
            max = max
                .saturating_add(u64::from(weapon_max))
                .saturating_add(curve);
        }
        let (flat, steps) =
            excellent_weapon_contribution(*piece, WeaponDamageKind::Physical, wearer_level);
        min = min.saturating_add(flat);
        max = max.saturating_add(flat);
        pct_steps = pct_steps.saturating_add(steps);
        if let (Some(_), Some(implicit_min)) = (
            excellent_weapon_set(piece.item),
            excellent_implicit_min(&piece.def.kind),
        ) {
            let implicit = excellent_weapon_implicit(implicit_min, piece.def.drop_level);
            min = min.saturating_add(implicit);
            max = max.saturating_add(implicit);
        }
    }
    let mut min = narrow_u32(min);
    let mut max = narrow_u32(max);
    for _ in 0..pct_steps {
        min = scale_ratio(min, EXCELLENT_DAMAGE_NUM, nonzero(PCT_DEN));
        max = scale_ratio(max, EXCELLENT_DAMAGE_NUM, nonzero(PCT_DEN));
    }
    min = scale_ratio(
        min,
        PCT_DEN.saturating_add(u32::from(ammo_percent)),
        nonzero(PCT_DEN),
    );
    max = scale_ratio(
        max,
        PCT_DEN.saturating_add(u32::from(ammo_percent)),
        nonzero(PCT_DEN),
    );
    match mode {
        WeaponMode::Single => {}
        WeaponMode::DoubleWield => {
            min = scale_ratio(min, DOUBLE_WIELD_SPAN_NUM, nonzero(PCT_DEN));
            max = scale_ratio(max, DOUBLE_WIELD_SPAN_NUM, nonzero(PCT_DEN));
        }
    }
    Interval::spanning(narrow_u16(min), narrow_u16(max))
}

/// The equipped wizardry span: gearless span + normal wizardry options
/// (staffs, Wings of Heaven) + wizardry-kind excellent adds, then ×102/100 per
/// wizardry-kind excellent damage option. The staff RISE never lands here —
/// it multiplies the whole `(WizBase + D)` parenthesis at the skill seam.
fn wizardry_span(
    base: Interval<u16>,
    pieces: &[WornPiece<'_>],
    wearer_level: Level,
) -> Interval<u16> {
    let mut min = u64::from(base.min());
    let mut max = u64::from(base.max());
    let mut pct_steps: u32 = 0;
    for piece in pieces {
        let option =
            normal_option_add(piece.item, NormalOption::WizardryDamage, NORMAL_OPTION_STEP);
        min = min.saturating_add(option);
        max = max.saturating_add(option);
        let (flat, steps) =
            excellent_weapon_contribution(*piece, WeaponDamageKind::Wizardry, wearer_level);
        min = min.saturating_add(flat);
        max = max.saturating_add(flat);
        pct_steps = pct_steps.saturating_add(steps);
    }
    let mut min = narrow_u32(min);
    let mut max = narrow_u32(max);
    for _ in 0..pct_steps {
        min = scale_ratio(min, EXCELLENT_DAMAGE_NUM, nonzero(PCT_DEN));
        max = scale_ratio(max, EXCELLENT_DAMAGE_NUM, nonzero(PCT_DEN));
    }
    Interval::spanning(narrow_u16(min), narrow_u16(max))
}

/// The excellent weapon implicit: `min_damage × 25 / drop_level + 5`, integer
/// floor. A zero drop level saturates to 1 — a boundary fold, no wearable
/// carries one.
fn excellent_weapon_implicit(min_damage: u16, drop_level: u8) -> u64 {
    (u64::from(min_damage) * EXCELLENT_IMPLICIT_NUM / u64::from(drop_level.max(1)))
        .saturating_add(EXCELLENT_IMPLICIT_ADD)
}

/// The raw pre-halving stat defense term — the gearless per-class defense
/// divisor's numerator at half depth (`agility / (N/2)`; every class's shipped
/// divisor N is even, so `floor(floor(agility/(N/2))/2) = floor(agility/N)`
/// keeps the gearless value byte-identical under the /2-once fold).
fn stat_defense_pre_half(class: CharacterClass, agility: u16) -> u64 {
    let half_divisor: u64 = match class {
        CharacterClass::DarkWizard | CharacterClass::SoulMaster => 4,
        CharacterClass::DarkKnight | CharacterClass::BladeKnight => 3,
        CharacterClass::FairyElf | CharacterClass::MuseElf => 10,
        CharacterClass::MagicGladiator => 5,
        CharacterClass::DarkLord => 7,
    };
    u64::from(agility) / half_divisor
}

/// One worn piece's contribution to the raw defense sum: its base defense plus
/// its family's +level curve. Non-defense kinds contribute 0.
fn defense_contribution(kind: &ItemKind, enhance: EnhanceLevel) -> u64 {
    match kind {
        ItemKind::Helm { defense, .. }
        | ItemKind::BodyArmor { defense, .. }
        | ItemKind::Pants { defense, .. }
        | ItemKind::Gloves { defense, .. }
        | ItemKind::Boots { defense, .. } => {
            u64::from(*defense) + u64::from(armor_defense_bonus(enhance))
        }
        ItemKind::Shield { defense, .. } => {
            u64::from(*defense) + u64::from(shield_defense_bonus(enhance))
        }
        ItemKind::Wings { defense, .. } => {
            u64::from(*defense) + u64::from(wing_defense_bonus(enhance))
        }
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
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
        | ItemKind::StatFruit => 0,
    }
}

/// One worn piece's contribution to the defense-rate sum: shields carry their
/// base rate plus the ARMOR +level curve; armor pieces individually carry none.
fn rate_contribution(kind: &ItemKind, enhance: EnhanceLevel) -> u64 {
    match kind {
        // W-SRC: shield defense-RATE grows on the armor curve while shield
        // DEFENSE grows +1/level (ArmorInitializerBase.cs:21,46-47,173-178).
        ItemKind::Shield { defense_rate, .. } => {
            u64::from(*defense_rate) + u64::from(shield_defense_rate_bonus(enhance))
        }
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
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
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => 0,
    }
}

/// Whether a kind is one of the five suit armor pieces.
fn armor_suit_member(kind: &ItemKind) -> bool {
    matches!(
        kind,
        ItemKind::Helm { .. }
            | ItemKind::BodyArmor { .. }
            | ItemKind::Pants { .. }
            | ItemKind::Gloves { .. }
            | ItemKind::Boots { .. }
    )
}

/// Complete-suit status over the worn (non-broken) pieces — a missing,
/// mismatched, or broken armor piece makes the suit [`SuitStatus::Incomplete`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SuitStatus {
    /// A missing, mismatched, or broken armor piece — no suit multiplier.
    Incomplete,
    /// Five matched armor pieces at mixed levels — the rate ×11/10 applies,
    /// the level-scaled defense multiplier does not.
    CompleteMixed,
    /// Five matched armor pieces all at the same level.
    CompleteUniform {
        /// The shared item level.
        level: ItemLevel,
    },
}

/// Derives the suit status from the worn pieces' item numbers and levels — a
/// matched suit's five armor pieces share one `ItemRef.number` across their
/// groups; no new data schema.
fn suit_status(pieces: &[WornPiece<'_>]) -> SuitStatus {
    let members: Vec<(u16, ItemLevel)> = pieces
        .iter()
        .filter(|piece| armor_suit_member(&piece.def.kind))
        .map(|piece| (piece.item.item.number, piece.item.level))
        .collect();
    let [(number, level), rest @ ..] = members.as_slice() else {
        return SuitStatus::Incomplete;
    };
    if members.len() != 5 || rest.iter().any(|(other, _)| other != number) {
        return SuitStatus::Incomplete;
    }
    if rest.iter().all(|(_, other)| other == level) {
        SuitStatus::CompleteUniform { level: *level }
    } else {
        SuitStatus::CompleteMixed
    }
}

/// The complete-suit defense multiplier as a ratio: `(100 + (level−9)·5)/100`
/// for a uniform suit at level ≥ 10, else the ×1 identity.
fn suit_defense_ratio(suit: SuitStatus) -> (u32, u32) {
    match suit {
        SuitStatus::Incomplete | SuitStatus::CompleteMixed => (1, 1),
        SuitStatus::CompleteUniform { level } => {
            let level = u32::from(level.get());
            if level >= SUIT_DEFENSE_FLOOR_LEVEL {
                (
                    PCT_DEN + (level - (SUIT_DEFENSE_FLOOR_LEVEL - 1)) * SUIT_DEFENSE_STEP,
                    PCT_DEN,
                )
            } else {
                (1, 1)
            }
        }
    }
}

/// The equipped defense: the raw pre-half stat term plus every worn piece's
/// defense contribution and normal defense option, × the suit multiplier,
/// halved ONCE — a single floor over the whole pooled numerator, never
/// per-item halving.
fn folded_defense(
    class: CharacterClass,
    agility: u16,
    pieces: &[WornPiece<'_>],
    suit: SuitStatus,
) -> u16 {
    let mut sum = stat_defense_pre_half(class, agility);
    for piece in pieces {
        sum = sum
            .saturating_add(defense_contribution(
                &piece.def.kind,
                piece.item.level.enhance_level_or_zero(),
            ))
            .saturating_add(normal_option_add(
                piece.item,
                NormalOption::Defense,
                NORMAL_OPTION_STEP,
            ));
    }
    let (num, den) = suit_defense_ratio(suit);
    pooled_u16(sum.saturating_mul(u64::from(num)), den.saturating_mul(2))
}

/// The equipped defense rate: gearless rate + shield contributions + shield
/// rate options, × 110/100 per excellent defense-rate option, × 11/10 for a
/// complete suit (any level).
fn folded_defense_rate(base_rate: u16, pieces: &[WornPiece<'_>], suit: SuitStatus) -> u16 {
    let mut sum = u64::from(base_rate);
    let mut excellent_steps: u32 = 0;
    for piece in pieces {
        sum = sum
            .saturating_add(rate_contribution(
                &piece.def.kind,
                piece.item.level.enhance_level_or_zero(),
            ))
            .saturating_add(normal_option_add(
                piece.item,
                NormalOption::DefenseRate,
                SHIELD_RATE_OPTION_STEP,
            ));
        if let Some(set) = excellent_armor_set(piece.item) {
            for option in set.iter() {
                match option {
                    ExcellentArmorOption::DefenseRate => {
                        excellent_steps = excellent_steps.saturating_add(1);
                    }
                    ExcellentArmorOption::ZenGain
                    | ExcellentArmorOption::DamageReflect
                    | ExcellentArmorOption::DamageDecrease
                    | ExcellentArmorOption::MaxMana
                    | ExcellentArmorOption::MaxHealth => {}
                }
            }
        }
    }
    let mut rate = narrow_u32(sum);
    for _ in 0..excellent_steps {
        rate = scale_ratio(rate, EXCELLENT_RATE_NUM, nonzero(PCT_DEN));
    }
    match suit {
        SuitStatus::Incomplete => {}
        SuitStatus::CompleteMixed | SuitStatus::CompleteUniform { .. } => {
            rate = scale_ratio(rate, SUIT_RATE_NUM, nonzero(SUIT_RATE_DEN));
        }
    }
    narrow_u16(rate)
}

/// The resistance element a jewelry kind grants, when it grants one.
fn jewelry_element(kind: &ItemKind) -> Option<Element> {
    match kind {
        ItemKind::Ring { resistance, .. } | ItemKind::Pendant { resistance, .. } => *resistance,
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
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

/// Per-element Maximum fold over worn jewelry: each piece grants
/// `base 1 + level`, and two pieces of one element never stack — the highest
/// counts.
fn folded_resistances(
    base: PerElement<Resistance>,
    pieces: &[WornPiece<'_>],
) -> PerElement<Resistance> {
    let mut table = base;
    for piece in pieces {
        let Some(element) = jewelry_element(&piece.def.kind) else {
            continue;
        };
        // The curve already carries the base 1, so a +0 piece grants 1.
        let granted = Resistance(jewelry_resistance(piece.item.level.enhance_level_or_zero()));
        let slot = match element {
            Element::Ice => &mut table.ice,
            Element::Poison => &mut table.poison,
            Element::Lightning => &mut table.lightning,
            Element::Fire => &mut table.fire,
            Element::Earth => &mut table.earth,
            Element::Wind => &mut table.wind,
            Element::Water => &mut table.water,
        };
        *slot = (*slot).max(granted);
    }
    table
}

/// How many worn pieces rolled luck.
fn luck_count(pieces: &[WornPiece<'_>]) -> u64 {
    pieces
        .iter()
        .filter(|piece| matches!(piece.item.luck, LuckRoll::Lucky))
        .map(|_| 1_u64)
        .sum()
}

/// How many worn pieces carry the given excellent WEAPON option.
fn weapon_option_count(pieces: &[WornPiece<'_>], wanted: ExcellentWeaponOption) -> u64 {
    pieces
        .iter()
        .filter_map(|piece| excellent_weapon_set(piece.item))
        .flat_map(ExcellentWeaponSet::iter)
        .filter(|option| *option == wanted)
        .map(|_| 1_u64)
        .sum()
}

/// How many worn pieces carry the given excellent ARMOR option.
fn armor_option_count(pieces: &[WornPiece<'_>], wanted: ExcellentArmorOption) -> u64 {
    pieces
        .iter()
        .filter_map(|piece| excellent_armor_set(piece.item))
        .flat_map(ExcellentArmorSet::iter)
        .filter(|option| *option == wanted)
        .map(|_| 1_u64)
        .sum()
}

/// A percent chance raised by `count × step` points, clamped at 100.
fn folded_chance(base: Percent, count: u64, step: u64) -> Percent {
    Percent::clamped(u64::from(base.points()).saturating_add(count.saturating_mul(step)))
}

/// The worn staff's doubled rise, or `0` (the ×1 identity) when no staff is
/// worn.
fn rise_of(pieces: &[WornPiece<'_>]) -> u16 {
    // A max-fold: no worn staff leaves the 0 (×1) identity; a worn staff
    // supplies its rise. Total — no Option to mask.
    pieces.iter().fold(0, |rise, piece| {
        rise.max(match &piece.def.kind {
            ItemKind::Staff { magic_power, .. } => {
                staff_rise_x2(*magic_power, piece.item.level.enhance_level_or_zero())
            }
            ItemKind::Weapon { .. }
            | ItemKind::Bow { .. }
            | ItemKind::Crossbow { .. }
            | ItemKind::Arrows { .. }
            | ItemKind::Bolts { .. }
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
            | ItemKind::Jewel { .. }
            | ItemKind::Consumable { .. }
            | ItemKind::LuckyBox
            | ItemKind::EventTicket { .. }
            | ItemKind::MixMaterial
            | ItemKind::StatFruit => 0,
        })
    })
}

/// The worn wing's `(damage %, absorb %)` at its level, or the zero identities
/// when no wing is worn.
fn wing_percents(pieces: &[WornPiece<'_>]) -> (Percent, Percent) {
    let found = pieces.iter().find_map(|piece| match &piece.def.kind {
        ItemKind::Wings {
            tier,
            absorb_percent,
            damage_percent,
            ..
        } => {
            let enhance = piece.item.level.enhance_level_or_zero();
            Some((
                Percent::clamped(u64::from(wing_damage_percent(
                    *damage_percent,
                    *tier,
                    enhance,
                ))),
                Percent::clamped(u64::from(wing_absorb_percent(*absorb_percent, enhance))),
            ))
        }
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
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
        | ItemKind::StatFruit => None,
    });
    match found {
        Some(percents) => percents,
        // No worn wing: the zero identities.
        None => (Percent::ZERO, Percent::ZERO),
    }
}

/// A worn pet's resolved bonuses; the empty slice for every non-pet kind.
fn pet_bonuses(kind: &ItemKind) -> &[CombatBonus] {
    match kind {
        ItemKind::Pet { bonuses, .. } => bonuses,
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Shield { .. }
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
        | ItemKind::StatFruit => &[],
    }
}

/// Whether both hands hold double-wield-carrying weapons: width-1 one-handed
/// sword/axe/mace rows (groups 0-2), a DK/MG capability.
// W-SRC: only width-1 one-handed rows of groups 0-2 carry
// DoubleWieldWeaponCount (Version075/Items/Weapons.cs:322-325); pre-S3 wiring
// is the Dark Knight and Magic Gladiator lines (ClassDarkKnight.cs:49,
// ClassMagicGladiator.cs:52).
fn weapon_mode_of(class: CharacterClass, pieces: &[WornPiece<'_>]) -> WeaponMode {
    match class {
        CharacterClass::DarkKnight
        | CharacterClass::BladeKnight
        | CharacterClass::MagicGladiator => {}
        CharacterClass::DarkWizard
        | CharacterClass::SoulMaster
        | CharacterClass::FairyElf
        | CharacterClass::MuseElf
        | CharacterClass::DarkLord => return WeaponMode::Single,
    }
    let wielded = pieces
        .iter()
        .filter(|piece| dual_wield_weapon(piece.def))
        .count();
    if wielded == 2 {
        WeaponMode::DoubleWield
    } else {
        WeaponMode::Single
    }
}

/// Whether a definition is a width-1 one-handed sword/axe/mace — a
/// double-wield-carrying row.
fn dual_wield_weapon(def: &ItemDefinition) -> bool {
    matches!(
        def.kind,
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            ..
        }
    ) && def.width == 1
        && def.id.group <= 2
}

/// Whether a kind fires ammunition (the ammo damage percent's gate).
fn consumes_ammo(kind: &ItemKind) -> bool {
    matches!(kind, ItemKind::Bow { .. } | ItemKind::Crossbow { .. })
}

/// The worn ammunition's tier, when the kind is ammunition. A level past the
/// ammo table folds to the +0 tier — data-unreachable, never an unwrap.
fn ammo_tier(kind: &ItemKind, level: ItemLevel) -> Option<AmmoLevel> {
    match kind {
        ItemKind::Arrows { .. } | ItemKind::Bolts { .. } => {
            Some(match AmmoLevel::try_from(level.get()) {
                Ok(tier) => tier,
                Err(_) => AmmoLevel::L0,
            })
        }
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
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => None,
    }
}

/// The ammunition damage percent the physical span multiplies by: the worn
/// ammo's tier percent when a bow/crossbow is wielded beside it, else 0 (the
/// identity). Unconditional for a bow-wielder with ammo — the archery-mode
/// buff gating is a stated in-wave divergence.
fn ammo_span_percent(pieces: &[WornPiece<'_>]) -> u8 {
    if !pieces.iter().any(|piece| consumes_ammo(&piece.def.kind)) {
        return 0;
    }
    match pieces
        .iter()
        .find_map(|piece| ammo_tier(&piece.def.kind, piece.item.level))
    {
        Some(tier) => ammunition_damage_percent(tier),
        None => 0,
    }
}

/// Saturating narrow of a pooled `u64` accumulator into the `u32` ratio home —
/// boundary saturation of a combat magnitude, never a masked lookup.
fn narrow_u32(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

// W-SRC: the Defense-reduction ailment scales defense to ×9/10. OpenMU inflicts
// it via DefenseReductionEffectInitializer; no pre-S3 skill or monster applies it
// (Fire Slash / Beast Uppercut are S4+), so we include it as a deliberate design
// choice, not an era-authentic mechanic.
/// Defense-reduction numerator.
const DEFENSE_REDUCTION_NUM: u32 = 9;
/// Defense-reduction denominator.
const DEFENSE_REDUCTION_DEN: u32 = 10;

/// The transient combat profile a strike reads once the entity's active effects
/// are folded in — never persisted; re-derived every strike from
/// `(base, effects)` so an effect reverts exactly on expiry. Folds each active
/// effect's [`CombatBonus`] contribution into `base`, then applies the
/// Defense-reduction ailment as a named ×9/10 derivation against the
/// possibly-folded defense. An entity with no effects yields `base` unchanged
/// (the empty fold is the identity).
#[must_use]
pub fn effective_profile(base: CombatProfile, effects: &ActiveEffects) -> CombatProfile {
    let folded = effects
        .active()
        .into_iter()
        .filter_map(|effect| effect_bonus(&effect))
        .fold(base, fold_profile_bonus);
    match effects.defense_reduction() {
        Some(_) => reduce_defense(folded),
        None => folded,
    }
}

/// Folds one resolved [`CombatBonus`] into a profile. Only the three effect
/// contributions that map to a profile field fold in; every other variant is a
/// no-op — a future stat-aggregation wave extends this fold — enumerated as an
/// explicit or-pattern so a new variant breaks the build.
fn fold_profile_bonus(profile: CombatProfile, bonus: CombatBonus) -> CombatProfile {
    match bonus {
        // W-SRC: Greater Damage (GreaterDamageEffectInitializer.cs) is a flat add
        // folded onto the outgoing damage AFTER defense subtraction — never a raise
        // of the physical span, which crit/excellent would then amplify.
        CombatBonus::Damage { amount } => CombatProfile {
            flat_damage_add: profile.flat_damage_add.saturating_add(amount),
            ..profile
        },
        CombatBonus::Defense { amount } => CombatProfile {
            defense: profile.defense.saturating_add(narrow_u16(amount)),
            ..profile
        },
        CombatBonus::IncomingDamagePct { percent } => CombatProfile {
            incoming_damage_reduction: combine_reduction(
                profile.incoming_damage_reduction,
                percent,
            ),
            ..profile
        },
        CombatBonus::PhysicalDamage { .. }
        | CombatBonus::Strength { .. }
        | CombatBonus::Agility { .. }
        | CombatBonus::Vitality { .. }
        | CombatBonus::Energy { .. }
        | CombatBonus::Command { .. }
        | CombatBonus::MaxHealth { .. }
        | CombatBonus::MaxHealthPct { .. }
        | CombatBonus::MaxMana { .. }
        | CombatBonus::MaxManaPct { .. }
        | CombatBonus::MaxAbility { .. }
        | CombatBonus::MaxAbilityPct { .. }
        | CombatBonus::HealthRecoveryPct { .. }
        | CombatBonus::AbilityRecovery { .. }
        | CombatBonus::DefensePct { .. }
        | CombatBonus::DefenseRate { .. }
        | CombatBonus::DefenseRatePct { .. }
        | CombatBonus::AttackRate { .. }
        | CombatBonus::AttackSpeed { .. }
        | CombatBonus::MinPhysicalDamage { .. }
        | CombatBonus::MaxPhysicalDamage { .. }
        | CombatBonus::WizardryDamage { .. }
        | CombatBonus::WizardryDamagePct { .. }
        | CombatBonus::SkillDamage { .. }
        | CombatBonus::TwoHandedWeaponDamagePct { .. }
        | CombatBonus::DamagePct { .. }
        | CombatBonus::CriticalChancePct { .. }
        | CombatBonus::CriticalDamage { .. }
        | CombatBonus::ExcellentChancePct { .. }
        | CombatBonus::ExcellentDamage { .. }
        | CombatBonus::DoubleDamageChancePct { .. }
        | CombatBonus::DefenseIgnoreChancePct { .. }
        | CombatBonus::DamageReflectPct { .. }
        | CombatBonus::HealthPerKill
        | CombatBonus::ManaPerKill
        | CombatBonus::ZenDropPct { .. }
        | CombatBonus::ElementalResistance { .. }
        | CombatBonus::ElementalDamage { .. } => profile,
    }
}

/// The profile with its defense scaled to ×9/10 — the Defense-reduction ailment
/// derivation. Percent cannot express a ×9/10 *decrease* (it models an increase),
/// so this is a named ratio step, not an additive bonus.
fn reduce_defense(profile: CombatProfile) -> CombatProfile {
    CombatProfile {
        defense: narrow_u16(scale_ratio(
            u32::from(profile.defense),
            DEFENSE_REDUCTION_NUM,
            nonzero(DEFENSE_REDUCTION_DEN),
        )),
        ..profile
    }
}

/// Two damage-reduction percentages combined multiplicatively: the fractions of
/// damage each lets through are multiplied, and the combined reduction is the
/// complement. Folding one reduction into a zero base is the identity.
fn combine_reduction(existing: Percent, add: Percent) -> Percent {
    let kept_existing = u32::from(Percent::DENOMINATOR - existing.points());
    let kept_add = u32::from(Percent::DENOMINATOR - add.points());
    let kept = scale_ratio(
        kept_existing,
        kept_add,
        nonzero(u32::from(Percent::DENOMINATOR)),
    );
    Percent::clamped(u64::from(
        u32::from(Percent::DENOMINATOR).saturating_sub(kept),
    ))
}

/// Saturating narrow of a resolved contribution to the `u16` a profile field
/// stores.
fn narrow_u16(value: u32) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

fn attributes_of(level: Level, stats: Stats) -> Attributes {
    let (strength, agility, vitality, energy, command) = match stats {
        Stats::Standard {
            strength,
            agility,
            vitality,
            energy,
        } => (strength, agility, vitality, energy, 0),
        Stats::WithCommand {
            strength,
            agility,
            vitality,
            energy,
            command,
        } => (strength, agility, vitality, energy, command),
    };
    Attributes {
        level: u64::from(level.get()),
        strength: u64::from(strength),
        agility: u64::from(agility),
        vitality: u64::from(vitality),
        energy: u64::from(energy),
        command: u64::from(command),
    }
}

/// The attack rate shared by every non-command class: `(20L + 6A + S) / 4`.
fn shared_attack_rate(a: &Attributes) -> u16 {
    pooled_u16(20 * a.level + 6 * a.agility + a.strength, 4)
}

/// Zero elemental resistance across every element — a gearless character carries
/// none. A real domain value, not a fabricated default.
fn gearless_resistances() -> PerElement<Resistance> {
    PerElement {
        ice: Resistance(0),
        poison: Resistance(0),
        lightning: Resistance(0),
        fire: Resistance(0),
        earth: Resistance(0),
        wind: Resistance(0),
        water: Resistance(0),
    }
}

fn gearless_profile(
    level: u64,
    physical: Interval<u16>,
    wizardry: Option<Interval<u16>>,
    defense: u16,
    attack_rate: u16,
    defense_rate: u16,
) -> CombatProfile {
    CombatProfile {
        level: Level::clamped(level),
        physical,
        wizardry,
        defense,
        attack_rate,
        defense_rate,
        resistances: gearless_resistances(),
        critical_chance: Percent::ZERO,
        excellent_chance: Percent::ZERO,
        defense_ignore_chance: Percent::ZERO,
        double_damage_chance: Percent::ZERO,
        incoming_damage_reduction: Percent::ZERO,
        flat_damage_add: 0,
        // A bare character has no staff rise, no wings, no excellent gear,
        // and one (or no) weapon — real domain zeros, not fabricated defaults.
        wizardry_rise_x2: 0,
        incoming_dd_pct: Percent::ZERO,
        wing_damage_pct: Percent::ZERO,
        wing_absorb_pct: Percent::ZERO,
        weapon_mode: WeaponMode::Single,
    }
}

fn wizard_profile(a: &Attributes) -> (CombatProfile, VitalMaxima) {
    let profile = gearless_profile(
        a.level,
        Interval::spanning(pooled_u16(a.strength, 8), pooled_u16(a.strength, 4)),
        Some(Interval::spanning(
            pooled_u16(a.energy, 9),
            pooled_u16(a.energy, 4),
        )),
        pooled_u16(a.agility, 8),
        shared_attack_rate(a),
        pooled_u16(a.agility, 3),
    );
    let maxima = VitalMaxima {
        max_health: pooled_u32(30 + a.level + 2 * a.vitality, 1),
        max_mana: pooled_u32(2 * a.energy + 2 * a.level, 1),
        max_ability: pooled_u32(
            20 * a.energy + 30 * a.vitality + 40 * a.agility + 20 * a.strength,
            100,
        ),
    };
    (profile, maxima)
}

fn knight_profile(a: &Attributes) -> (CombatProfile, VitalMaxima) {
    let profile = gearless_profile(
        a.level,
        Interval::spanning(pooled_u16(a.strength, 6), pooled_u16(a.strength, 4)),
        None,
        pooled_u16(a.agility, 6),
        shared_attack_rate(a),
        pooled_u16(a.agility, 3),
    );
    let maxima = VitalMaxima {
        max_health: pooled_u32(35 + 2 * a.level + 3 * a.vitality, 1),
        max_mana: pooled_u32(20 + 2 * a.energy + a.level, 2),
        max_ability: pooled_u32(
            100 * a.energy + 30 * a.vitality + 20 * a.agility + 15 * a.strength,
            100,
        ),
    };
    (profile, maxima)
}

fn elf_profile(a: &Attributes) -> (CombatProfile, VitalMaxima) {
    let strength_agility = a.strength + a.agility;
    let profile = gearless_profile(
        a.level,
        Interval::spanning(
            pooled_u16(strength_agility, 7),
            pooled_u16(strength_agility, 4),
        ),
        None,
        pooled_u16(a.agility, 20),
        shared_attack_rate(a),
        pooled_u16(a.agility, 4),
    );
    let maxima = VitalMaxima {
        max_health: pooled_u32(39 + a.level + 2 * a.vitality, 1),
        max_mana: pooled_u32(12 + 3 * a.energy + 3 * a.level, 2),
        max_ability: pooled_u32(
            20 * a.energy + 30 * a.vitality + 20 * a.agility + 30 * a.strength,
            100,
        ),
    };
    (profile, maxima)
}

fn magic_gladiator_profile(a: &Attributes) -> (CombatProfile, VitalMaxima) {
    let strength_energy = 2 * a.strength + a.energy;
    let profile = gearless_profile(
        a.level,
        Interval::spanning(
            pooled_u16(strength_energy, 12),
            pooled_u16(strength_energy, 8),
        ),
        Some(Interval::spanning(
            pooled_u16(a.energy, 9),
            pooled_u16(a.energy, 4),
        )),
        pooled_u16(a.agility, 10),
        shared_attack_rate(a),
        pooled_u16(a.agility, 3),
    );
    let maxima = VitalMaxima {
        max_health: pooled_u32(57 + a.level + 2 * a.vitality, 1),
        max_mana: pooled_u32(7 + 2 * a.energy + a.level, 1),
        max_ability: pooled_u32(
            15 * a.energy + 30 * a.vitality + 25 * a.agility + 20 * a.strength,
            100,
        ),
    };
    (profile, maxima)
}

fn dark_lord_profile(a: &Attributes) -> (CombatProfile, VitalMaxima) {
    let strength_energy = 2 * a.strength + a.energy;
    let attack_rate = pooled_u16(
        150 * a.level + 75 * a.agility + 5 * a.strength + 3 * a.command,
        30,
    );
    let profile = gearless_profile(
        a.level,
        Interval::spanning(
            pooled_u16(strength_energy, 14),
            pooled_u16(strength_energy, 10),
        ),
        None,
        pooled_u16(a.agility, 14),
        attack_rate,
        pooled_u16(a.agility, 7),
    );
    // Command classes are created with energy at least 15, so the `energy - 15`
    // mana term is a proven-non-negative domain quantity, not a guarded subtract.
    let maxima = VitalMaxima {
        max_health: pooled_u32(97 + 3 * a.level + 4 * a.vitality, 2),
        max_mana: pooled_u32(76 + 3 * (a.energy - 15) + 2 * a.level, 2),
        max_ability: pooled_u32(
            15 * a.energy + 10 * a.vitality + 20 * a.agility + 30 * a.strength + 30 * a.command,
            100,
        ),
    };
    (profile, maxima)
}

/// Pooled single divide narrowed to a `u16` combat magnitude, saturating rather
/// than truncating an out-of-range quotient.
fn pooled_u16(numerator: u64, denominator: u32) -> u16 {
    // Boundary saturation of a combat magnitude — never a masked lookup.
    u16::try_from(floor_div_u64_to_u32(numerator, nonzero(denominator))).unwrap_or(u16::MAX)
}

/// Pooled single divide as a `u32` vital capacity.
fn pooled_u32(numerator: u64, denominator: u32) -> u32 {
    floor_div_u64_to_u32(numerator, nonzero(denominator))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::element::Element;
    use crate::components::movement::Movement;
    use crate::components::placement::Placement;
    use crate::components::pool::Pool;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;
    use crate::components::vitals::Vitals;
    use crate::entities::character::Character;

    fn placement() -> Placement {
        Placement {
            position: TileCoord::new(10, 10).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        }
    }

    fn vitals() -> Vitals {
        // Stored maxima are deliberately wrong to prove the class formula wins.
        Vitals {
            health: Pool::full(1),
            mana: Pool::full(1),
            ability: Pool::full(1),
        }
    }

    fn character(class: CharacterClass, level: u16, stats: Stats) -> Character {
        let json = serde_json::json!({
            "class": serde_json::to_value(class).unwrap(),
            "level": level,
            "experience": 0,
            "stats": serde_json::to_value(stats).unwrap(),
            "unspent_points": 0,
            "zen": 0,
            "placement": serde_json::to_value(placement()).unwrap(),
            "vitals": serde_json::to_value(vitals()).unwrap(),
        });
        serde_json::from_value(json).unwrap()
    }

    fn standard(strength: u16, agility: u16, vitality: u16, energy: u16) -> Stats {
        Stats::Standard {
            strength,
            agility,
            vitality,
            energy,
        }
    }

    #[test]
    fn dark_wizard_matches_the_verified_derivation() {
        let hero = character(CharacterClass::DarkWizard, 40, standard(40, 20, 25, 36));
        let (profile, maxima) = character_profile(&hero);
        assert_eq!(profile.attack_rate(), 240);
        assert_eq!(profile.physical().min(), 5);
        assert_eq!(profile.physical().max(), 10);
        assert_eq!(profile.wizardry().unwrap().min(), 4);
        assert_eq!(profile.wizardry().unwrap().max(), 9);
        assert_eq!(profile.defense(), 2);
        assert_eq!(profile.defense_rate(), 6);
        assert_eq!(maxima.max_health, 120);
        assert_eq!(maxima.max_mana, 152);
        assert_eq!(maxima.max_ability, 30);
    }

    #[test]
    fn dark_knight_matches_the_verified_derivation() {
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 40, 50, 30));
        let (profile, maxima) = character_profile(&hero);
        assert_eq!(profile.attack_rate(), 325);
        assert_eq!(profile.physical().min(), 10);
        assert_eq!(profile.physical().max(), 15);
        assert_eq!(profile.wizardry(), None);
        assert_eq!(profile.defense(), 6);
        assert_eq!(profile.defense_rate(), 13);
        assert_eq!(maxima.max_health, 285);
        assert_eq!(maxima.max_mana, 65);
        assert_eq!(maxima.max_ability, 62);
    }

    #[test]
    fn fairy_elf_matches_the_verified_derivation() {
        let hero = character(CharacterClass::FairyElf, 30, standard(30, 40, 20, 20));
        let (profile, maxima) = character_profile(&hero);
        assert_eq!(profile.attack_rate(), 217);
        assert_eq!(profile.physical().min(), 10);
        assert_eq!(profile.physical().max(), 17);
        assert_eq!(profile.defense(), 2);
        assert_eq!(profile.defense_rate(), 10);
        assert_eq!(maxima.max_health, 109);
        assert_eq!(maxima.max_mana, 81);
        assert_eq!(maxima.max_ability, 27);
    }

    #[test]
    fn magic_gladiator_ability_pools_to_35_not_34() {
        let hero = character(CharacterClass::MagicGladiator, 50, standard(60, 30, 40, 24));
        let (profile, maxima) = character_profile(&hero);
        assert_eq!(profile.attack_rate(), 310);
        assert_eq!(profile.physical().min(), 12);
        assert_eq!(profile.physical().max(), 18);
        assert_eq!(profile.wizardry().unwrap().min(), 2);
        assert_eq!(profile.wizardry().unwrap().max(), 6);
        assert_eq!(profile.defense(), 3);
        assert_eq!(profile.defense_rate(), 10);
        assert_eq!(maxima.max_health, 187);
        assert_eq!(maxima.max_mana, 105);
        // The pooled single divide is 3510/100 = 35, not the per-term sum 34.
        assert_eq!(maxima.max_ability, 35);
    }

    #[test]
    fn dark_lord_ability_pools_to_35_not_34() {
        let stats = Stats::WithCommand {
            strength: 42,
            agility: 28,
            vitality: 30,
            energy: 35,
            command: 30,
        };
        let hero = character(CharacterClass::DarkLord, 40, stats);
        let (profile, maxima) = character_profile(&hero);
        assert_eq!(profile.attack_rate(), 280);
        assert_eq!(profile.physical().min(), 8);
        assert_eq!(profile.physical().max(), 11);
        assert_eq!(profile.wizardry(), None);
        assert_eq!(profile.defense(), 2);
        assert_eq!(profile.defense_rate(), 4);
        assert_eq!(maxima.max_health, 168);
        assert_eq!(maxima.max_mana, 108);
        assert_eq!(maxima.max_ability, 35);
    }

    #[test]
    fn second_tier_folds_to_its_base_line() {
        let stats = standard(60, 40, 50, 30);
        let base = character_profile(&character(CharacterClass::DarkKnight, 50, stats));
        let tier2 = character_profile(&character(CharacterClass::BladeKnight, 50, stats));
        assert_eq!(base.0, tier2.0);
        assert_eq!(base.1, tier2.1);
    }

    #[test]
    fn gearless_chances_are_all_zero() {
        let hero = character(CharacterClass::DarkWizard, 40, standard(40, 20, 25, 36));
        let (profile, _) = character_profile(&hero);
        assert_eq!(profile.critical_chance(), Percent::ZERO);
        assert_eq!(profile.excellent_chance(), Percent::ZERO);
        assert_eq!(profile.defense_ignore_chance(), Percent::ZERO);
        assert_eq!(profile.double_damage_chance(), Percent::ZERO);
    }

    #[test]
    fn only_wizard_and_magic_gladiator_carry_wizardry() {
        let stats = standard(40, 30, 30, 30);
        for class in [
            CharacterClass::DarkWizard,
            CharacterClass::SoulMaster,
            CharacterClass::MagicGladiator,
        ] {
            let hero = character(class, 40, stats);
            assert!(character_profile(&hero).0.wizardry().is_some(), "{class:?}");
        }
        for class in [
            CharacterClass::DarkKnight,
            CharacterClass::BladeKnight,
            CharacterClass::FairyElf,
            CharacterClass::MuseElf,
        ] {
            let hero = character(class, 40, stats);
            assert_eq!(character_profile(&hero).0.wizardry(), None, "{class:?}");
        }
    }

    #[test]
    fn monster_profile_copies_combat_and_zeroes_chances() {
        let combat = MonsterCombat {
            level: Level::new(12).unwrap(),
            hp: 500,
            min_phys_damage: 20,
            max_phys_damage: 30,
            defense: 8,
            attack_rate: 100,
            defense_rate: 25,
        };
        let resistances = PerElement {
            ice: Resistance(0),
            poison: Resistance(0),
            lightning: Resistance(50),
            fire: Resistance(0),
            earth: Resistance(0),
            wind: Resistance(0),
            water: Resistance(0),
        };
        let profile = monster_profile(&combat, &resistances, combat.level);
        assert_eq!(profile.physical().min(), 20);
        assert_eq!(profile.physical().max(), 30);
        assert_eq!(profile.wizardry(), None);
        assert_eq!(profile.defense(), 8);
        assert_eq!(profile.attack_rate(), 100);
        assert_eq!(profile.defense_rate(), 25);
        assert_eq!(profile.resistance(Element::Lightning), Resistance(50));
        assert_eq!(profile.excellent_chance(), Percent::ZERO);
    }

    fn base_profile() -> CombatProfile {
        let combat = MonsterCombat {
            level: Level::new(20).unwrap(),
            hp: 100,
            min_phys_damage: 10,
            max_phys_damage: 20,
            defense: 100,
            attack_rate: 50,
            defense_rate: 50,
        };
        monster_profile(&combat, &gearless_resistances(), combat.level)
    }

    #[test]
    fn effective_profile_of_no_effects_is_the_base() {
        use crate::components::active_effect::ActiveEffects;
        let base = base_profile();
        assert_eq!(effective_profile(base, &ActiveEffects::EMPTY), base);
    }

    #[test]
    fn greater_damage_folds_into_a_flat_add_not_the_physical_span() {
        use crate::components::active_effect::{ActiveEffect, ActiveEffects};
        use crate::components::units::Tick;
        let base = base_profile();
        let effects = ActiveEffects::EMPTY.with(ActiveEffect::GreaterDamage {
            amount: 5,
            expiry: Tick(100),
        });
        let profile = effective_profile(base, &effects);
        // The buff is a flat post-defense add; the physical span is untouched, so
        // crit/excellent (which read the span's max) cannot amplify it.
        assert_eq!(profile.flat_damage_add(), 5);
        assert_eq!(profile.physical(), base.physical());
    }

    #[test]
    fn defense_buff_folds_into_incoming_reduction() {
        use crate::components::active_effect::{ActiveEffect, ActiveEffects};
        use crate::components::units::Tick;
        let effects = ActiveEffects::EMPTY.with(ActiveEffect::Defense { expiry: Tick(80) });
        let profile = effective_profile(base_profile(), &effects);
        assert_eq!(
            profile.incoming_damage_reduction(),
            Percent::new(50).unwrap()
        );
    }

    #[test]
    fn defense_reduction_scales_defense_to_nine_tenths() {
        use crate::components::active_effect::{ActiveEffect, ActiveEffects};
        use crate::components::units::Tick;
        let effects =
            ActiveEffects::EMPTY.with(ActiveEffect::DefenseReduction { expiry: Tick(80) });
        // 100 × 9/10 = 90.
        assert_eq!(effective_profile(base_profile(), &effects).defense(), 90);
    }

    #[test]
    fn greater_defense_folds_before_the_reduction_derivation() {
        use crate::components::active_effect::{ActiveEffect, ActiveEffects};
        use crate::components::units::Tick;
        let effects = ActiveEffects::EMPTY
            .with(ActiveEffect::GreaterDefense {
                amount: 10,
                expiry: Tick(100),
            })
            .with(ActiveEffect::DefenseReduction { expiry: Tick(80) });
        // (100 + 10) × 9/10 = 99: the ×9/10 derivation applies to the folded defense.
        assert_eq!(effective_profile(base_profile(), &effects).defense(), 99);
    }

    // ── The equipment→profile fold (white-box over hand-built definitions) ──

    use crate::components::class::ClassSet;
    use crate::components::item_instance::{
        CraftedAugment, Durability, RolledNormalOption, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::levels::OptionLevel;
    use crate::data::common::{Provenance, SourceVersion};
    use crate::data::item_definitions::{ItemPrice, PetRide, WearRequirements, WingTier};

    fn no_wear() -> WearRequirements {
        WearRequirements {
            level: 0,
            strength: 0,
            agility: 0,
            vitality: 0,
            energy: 0,
            command: 0,
        }
    }

    fn definition(
        group: u8,
        number: u16,
        drop_level: u8,
        width: u8,
        kind: ItemKind,
    ) -> ItemDefinition {
        ItemDefinition {
            id: ItemRef { group, number },
            provenance: Provenance {
                source_version: SourceVersion::V075,
                review: None,
            },
            width,
            height: 1,
            drops_from_monsters: true,
            drop_level,
            max_item_level: ItemLevel::new(11).unwrap(),
            durability: 30,
            price: ItemPrice::Formula,
            kind,
        }
    }

    fn worn_instance(def: &ItemDefinition, level: u8) -> ItemInstance {
        ItemInstance {
            item: def.id,
            level: ItemLevel::new(level).unwrap(),
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
            augment: CraftedAugment::None,
        }
    }

    fn one_hand_weapon(min: u16, max: u16) -> ItemKind {
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            min_damage: min,
            max_damage: max,
            attack_speed: 0,
            skill: None,
            classes: ClassSet::NONE,
            wear: no_wear(),
        }
    }

    fn armor_kind(group: u8, defense: u16) -> ItemKind {
        match group {
            7 => ItemKind::Helm {
                defense,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
            8 => ItemKind::BodyArmor {
                defense,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
            9 => ItemKind::Pants {
                defense,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
            10 => ItemKind::Gloves {
                defense,
                attack_speed: 0,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
            _ => ItemKind::Boots {
                defense,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        }
    }

    /// The BDD §0.5 D fixture: a full matched +10 suit (bases 20/30/24/14/18),
    /// Satan-base wings (20), and a base-10 shield, every piece at level 10.
    fn full_suit() -> Vec<(ItemInstance, ItemDefinition)> {
        let mut list = Vec::new();
        for (group, defense) in [(7u8, 20u16), (8, 30), (9, 24), (10, 14), (11, 18)] {
            let def = definition(group, 5, 20, 2, armor_kind(group, defense));
            let item = worn_instance(&def, 10);
            list.push((item, def));
        }
        let wings = definition(
            12,
            0,
            100,
            3,
            ItemKind::Wings {
                tier: WingTier::First,
                defense: 20,
                absorb_percent: 12,
                damage_percent: 12,
                jol_options: Vec::new(),
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let wings_item = worn_instance(&wings, 10);
        list.push((wings_item, wings));
        let shield = definition(
            6,
            0,
            20,
            2,
            ItemKind::Shield {
                defense: 10,
                defense_rate: 0,
                skill: None,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let shield_item = worn_instance(&shield, 10);
        list.push((shield_item, shield));
        list
    }

    /// Runs the Atlas-free fold over hand-built pieces — the white-box door.
    fn folded(hero: &Character, list: &[(ItemInstance, ItemDefinition)]) -> CombatProfile {
        let pieces: Vec<WornPiece<'_>> = list
            .iter()
            .map(|(item, def)| WornPiece { item, def })
            .collect();
        fold_worn(
            character_profile(hero).0,
            hero.class(),
            hero.level(),
            hero.stats(),
            &pieces,
        )
    }

    #[test]
    fn the_empty_fold_is_the_gearless_identity_per_class() {
        for class in [
            CharacterClass::DarkWizard,
            CharacterClass::DarkKnight,
            CharacterClass::FairyElf,
            CharacterClass::MagicGladiator,
        ] {
            let hero = character(class, 50, standard(60, 40, 50, 30));
            assert_eq!(folded(&hero, &[]), character_profile(&hero).0, "{class:?}");
        }
        let stats = Stats::WithCommand {
            strength: 42,
            agility: 28,
            vitality: 30,
            energy: 35,
            command: 30,
        };
        let lord = character(CharacterClass::DarkLord, 40, stats);
        assert_eq!(folded(&lord, &[]), character_profile(&lord).0);
    }

    #[test]
    fn a_weapon_its_curve_and_normal_option_widen_the_physical_span() {
        // EQ-SPAN-1: [33+20+9+12, 50+40+9+12] = [74, 111].
        let hero = character(CharacterClass::DarkKnight, 50, standard(200, 100, 50, 30));
        let def = definition(0, 1, 20, 2, one_hand_weapon(20, 40));
        let mut item = worn_instance(&def, 3);
        item.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L3,
        });
        let profile = folded(&hero, &[(item, def)]);
        assert_eq!(profile.physical(), Interval::new(74, 111).unwrap());
    }

    #[test]
    fn an_excellent_weapon_adds_its_implicits_then_multiplies() {
        // EQ-SPAN-2: additive [93, 130] (implicit 20·25/50+5 = 15, +90/20 = 4),
        // then ×102/100 → [94, 132].
        let hero = character(CharacterClass::DarkKnight, 90, standard(200, 100, 50, 30));
        let def = definition(0, 1, 50, 2, one_hand_weapon(20, 40));
        let mut item = worn_instance(&def, 3);
        item.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L3,
        });
        item.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Weapon {
                options: ExcellentWeaponSet::with_first(
                    ExcellentWeaponOption::DamagePct,
                    [ExcellentWeaponOption::DamagePerLevel],
                ),
            },
        };
        let profile = folded(&hero, &[(item, def)]);
        assert_eq!(profile.physical(), Interval::new(94, 132).unwrap());
    }

    #[test]
    fn a_staff_folds_its_doubled_rise_and_leaves_the_wizardry_span_alone() {
        // EQ-STAFF-1 fold side: Legendary Staff magic power 59 (odd) at +1 →
        // rise_x2 = 59 + 2·4 = 67; the span multiplier lives at the skill seam.
        let hero = character(CharacterClass::DarkWizard, 50, standard(40, 20, 25, 100));
        let def = definition(
            5,
            0,
            36,
            2,
            ItemKind::Staff {
                min_damage: 0,
                max_damage: 0,
                attack_speed: 0,
                magic_power: 59,
                skill: None,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let item = worn_instance(&def, 1);
        let profile = folded(&hero, &[(item, def)]);
        assert_eq!(profile.wizardry_rise_x2(), 67);
        assert_eq!(profile.wizardry(), Some(Interval::new(11, 25).unwrap()));
    }

    #[test]
    fn defense_sums_raw_and_halves_once_with_the_uniform_suit_multiplier() {
        // EQ-DEF-1: floor((40 + 261 + 51 + 20) × 105 / 200) = 195.
        // EQ-DEF-2: defense rate floor((40 + 31) × 11/10) = 78.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 120, 50, 30));
        let profile = folded(&hero, &full_suit());
        assert_eq!(profile.defense(), 195);
        assert_eq!(profile.defense_rate(), 78);
    }

    #[test]
    fn a_mixed_level_suit_keeps_the_rate_multiplier_but_not_the_defense_one() {
        // Boots at +9 (armor curve 27): raw = 40+51+61+55+45+45+51+20 = 368 →
        // floor(368/2) = 184 (no ×1.05); the rate ×1.1 fires at any suit level.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 120, 50, 30));
        let mut list = full_suit();
        let boots = worn_instance(&list[4].1, 9);
        list[4].0 = boots;
        let profile = folded(&hero, &list);
        assert_eq!(profile.defense(), 184);
        assert_eq!(profile.defense_rate(), 78);
    }

    #[test]
    fn a_broken_piece_contributes_nothing_and_breaks_the_suit() {
        // EQ-DEF-4: boots at durability 0 drop their 49 defense AND the suit:
        // floor((372 − 49) / 2) = 161; the rate loses the suit ×1.1 → 71.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 120, 50, 30));
        let mut list = full_suit();
        list[4].0.durability = Durability::new(0, 30).unwrap();
        let profile = folded(&hero, &list);
        assert_eq!(profile.defense(), 161);
        assert_eq!(profile.defense_rate(), 71);
    }

    #[test]
    fn the_nested_floor_equality_holds_per_class() {
        // floor(stat_defense_pre_half / 2) == the shipped gearless defense for
        // every class over sample agilities — the /2-once reshape is
        // byte-compatible (E4).
        for agility in [0u16, 7, 23, 40, 119, 120, 999] {
            for class in [
                CharacterClass::DarkWizard,
                CharacterClass::DarkKnight,
                CharacterClass::FairyElf,
                CharacterClass::MagicGladiator,
            ] {
                let hero = character(class, 50, standard(60, agility, 50, 30));
                assert_eq!(
                    u16::try_from(stat_defense_pre_half(class, agility) / 2).unwrap(),
                    character_profile(&hero).0.defense(),
                    "{class:?} at agility {agility}"
                );
            }
            let stats = Stats::WithCommand {
                strength: 42,
                agility,
                vitality: 30,
                energy: 35,
                command: 30,
            };
            let lord = character(CharacterClass::DarkLord, 40, stats);
            assert_eq!(
                u16::try_from(stat_defense_pre_half(CharacterClass::DarkLord, agility) / 2)
                    .unwrap(),
                character_profile(&lord).0.defense(),
                "dark lord at agility {agility}"
            );
        }
    }

    #[test]
    fn jewelry_grants_base_one_plus_level_with_maximum_aggregation() {
        // EQ-JEWEL-1/2: +0 → 1; +4 → 5; two ice rings (+1 → 2, +3 → 4) → 4.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 40, 50, 30));
        let ring = definition(
            13,
            21,
            20,
            1,
            ItemKind::Ring {
                resistance: Some(Element::Ice),
                option: NormalOption::HealthRecoveryPct,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let plain = worn_instance(&ring, 0);
        let profile = folded(&hero, &[(plain, ring.clone())]);
        assert_eq!(profile.resistance(Element::Ice), Resistance(1));
        let plus4 = worn_instance(&ring, 4);
        let profile = folded(&hero, &[(plus4, ring.clone())]);
        assert_eq!(profile.resistance(Element::Ice), Resistance(5));
        let low = worn_instance(&ring, 1);
        let high = worn_instance(&ring, 3);
        let profile = folded(&hero, &[(low, ring.clone()), (high, ring)]);
        assert_eq!(profile.resistance(Element::Ice), Resistance(4));
        assert_eq!(profile.resistance(Element::Fire), Resistance(0));
    }

    #[test]
    fn luck_and_the_excellent_chance_option_raise_their_chances() {
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 40, 50, 30));
        let def = definition(0, 1, 20, 2, one_hand_weapon(20, 40));
        let mut item = worn_instance(&def, 0);
        item.luck = LuckRoll::Lucky;
        item.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Weapon {
                options: ExcellentWeaponSet::with_first(
                    ExcellentWeaponOption::ExcellentDamageChance,
                    [],
                ),
            },
        };
        let profile = folded(&hero, &[(item, def)]);
        assert_eq!(profile.critical_chance(), Percent::new(5).unwrap());
        assert_eq!(profile.excellent_chance(), Percent::new(10).unwrap());
    }

    #[test]
    fn wings_and_excellent_damage_decrease_fold_to_their_own_fields() {
        // Wings 12/12 at +2 → 16/16; excellent armor DamageDecrease → 4%. The
        // gearless profile carries the zero identities.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 40, 50, 30));
        let gearless = character_profile(&hero).0;
        assert_eq!(gearless.wing_damage_pct(), Percent::ZERO);
        assert_eq!(gearless.wing_absorb_pct(), Percent::ZERO);
        assert_eq!(gearless.incoming_dd_pct(), Percent::ZERO);
        let wings = definition(
            12,
            0,
            100,
            3,
            ItemKind::Wings {
                tier: WingTier::First,
                defense: 20,
                absorb_percent: 12,
                damage_percent: 12,
                jol_options: Vec::new(),
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let wings_item = worn_instance(&wings, 2);
        let armor = definition(8, 5, 20, 2, armor_kind(8, 30));
        let mut armor_item = worn_instance(&armor, 0);
        armor_item.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Armor {
                options: ExcellentArmorSet::from_options([ExcellentArmorOption::DamageDecrease])
                    .unwrap(),
            },
        };
        let profile = folded(&hero, &[(wings_item, wings), (armor_item, armor)]);
        assert_eq!(profile.wing_damage_pct(), Percent::new(16).unwrap());
        assert_eq!(profile.wing_absorb_pct(), Percent::new(16).unwrap());
        assert_eq!(profile.incoming_dd_pct(), Percent::new(4).unwrap());
    }

    #[test]
    fn double_wield_scales_the_span_and_flags_the_mode_for_dk_mg_only() {
        // EQ-DW-1 span side: stat [33,50] + [30,70] + [37,80] = [100,200],
        // ×55/100 → [55,110]; the head's ×2 restores the net 110%.
        let knight = character(CharacterClass::DarkKnight, 50, standard(200, 100, 50, 30));
        let left = definition(0, 1, 20, 1, one_hand_weapon(30, 70));
        let right = definition(2, 2, 20, 1, one_hand_weapon(37, 80));
        let pair = vec![
            (worn_instance(&left, 0), left.clone()),
            (worn_instance(&right, 0), right.clone()),
        ];
        let profile = folded(&knight, &pair);
        assert_eq!(profile.weapon_mode(), WeaponMode::DoubleWield);
        assert_eq!(profile.physical(), Interval::new(55, 110).unwrap());
        // One weapon: Single, no ×55/100.
        let single = folded(&knight, &pair[..1]);
        assert_eq!(single.weapon_mode(), WeaponMode::Single);
        assert_eq!(single.physical(), Interval::new(63, 120).unwrap());
        // A non-DK/MG class never double-wields.
        let elf = character(CharacterClass::FairyElf, 50, standard(200, 100, 50, 30));
        assert_eq!(folded(&elf, &pair).weapon_mode(), WeaponMode::Single);
        // A width-2 one-hander is not a dual-wield row.
        let wide = definition(0, 3, 20, 2, one_hand_weapon(30, 70));
        let wide_pair = vec![
            (worn_instance(&wide, 0), wide.clone()),
            (worn_instance(&right, 0), right),
        ];
        assert_eq!(
            folded(&knight, &wide_pair).weapon_mode(),
            WeaponMode::Single
        );
    }

    #[test]
    fn pet_bonuses_fold_through_the_combat_bonus_currency() {
        // EQ-PET-1: Dinorant's data-carried absorb −5% lands on the incoming
        // reduction; MaxAbility/AttackSpeed have no combat field.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 40, 50, 30));
        let gearless = character_profile(&hero).0;
        let pet = definition(
            13,
            3,
            110,
            1,
            ItemKind::Pet {
                ride: PetRide::FlyingMount,
                bonuses: vec![
                    CombatBonus::IncomingDamagePct {
                        percent: Percent::new(5).unwrap(),
                    },
                    CombatBonus::MaxAbility { amount: 50 },
                    CombatBonus::AttackSpeed { amount: 5 },
                ],
                skill: None,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let item = worn_instance(&pet, 0);
        let profile = folded(&hero, &[(item, pet)]);
        assert_eq!(
            profile.incoming_damage_reduction(),
            Percent::new(5).unwrap()
        );
        assert_eq!(profile.physical(), gearless.physical());
        assert_eq!(profile.defense(), gearless.defense());
    }

    #[test]
    fn ammunition_multiplies_a_bow_wielders_span() {
        // EQ-AMMO-1: elf [10,17] + bow [20,40] = [30,57], × 103/100 → [30,58];
        // no ammo → no multiplier.
        let elf = character(CharacterClass::FairyElf, 50, standard(30, 40, 20, 20));
        let bow = definition(
            4,
            1,
            20,
            2,
            ItemKind::Bow {
                min_damage: 20,
                max_damage: 40,
                attack_speed: 0,
                skill: None,
                classes: ClassSet::NONE,
                wear: no_wear(),
            },
        );
        let arrows = definition(
            4,
            15,
            0,
            1,
            ItemKind::Arrows {
                classes: ClassSet::NONE,
            },
        );
        let with_ammo = vec![
            (worn_instance(&bow, 0), bow.clone()),
            (worn_instance(&arrows, 1), arrows),
        ];
        assert_eq!(
            folded(&elf, &with_ammo).physical(),
            Interval::new(30, 58).unwrap()
        );
        let bare_bow = vec![(worn_instance(&bow, 0), bow)];
        assert_eq!(
            folded(&elf, &bare_bow).physical(),
            Interval::new(30, 57).unwrap()
        );
    }

    #[test]
    fn the_fold_takes_no_rng_and_is_a_pure_function() {
        // EQ-DET-3: two folds of identical inputs are identical values; no
        // signature on the fold path takes an RngCore.
        let hero = character(CharacterClass::DarkKnight, 50, standard(60, 120, 50, 30));
        let list = full_suit();
        assert_eq!(folded(&hero, &list), folded(&hero, &list));
    }
}
