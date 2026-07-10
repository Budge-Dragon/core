//! Item-level growth curves, durability bonuses, and the effective-drop-level
//! rule. The +level growth curves and durability bonuses are named const value
//! families read through accessors total over the [`EnhanceLevel`] enum
//! (exhaustive match, no indexing). This module is the one home of
//! [`effective_drop_level`] and its three surcharge constants, total over
//! [`EnhanceLevel`].

use crate::components::item_quality::ItemRarity;
use crate::components::levels::{AmmoLevel, EnhanceLevel};

use crate::data::item_definitions::WingTier;

/// Reads a twelve-entry const table by [`EnhanceLevel`] through an exhaustive
/// match — total by construction, no indexing, no wildcard arm.
fn by_enhance_level<T: Copy>(level: EnhanceLevel, table: &[T; 12]) -> T {
    let [l0, l1, l2, l3, l4, l5, l6, l7, l8, l9, l10, l11] = table;
    match level {
        EnhanceLevel::L0 => *l0,
        EnhanceLevel::L1 => *l1,
        EnhanceLevel::L2 => *l2,
        EnhanceLevel::L3 => *l3,
        EnhanceLevel::L4 => *l4,
        EnhanceLevel::L5 => *l5,
        EnhanceLevel::L6 => *l6,
        EnhanceLevel::L7 => *l7,
        EnhanceLevel::L8 => *l8,
        EnhanceLevel::L9 => *l9,
        EnhanceLevel::L10 => *l10,
        EnhanceLevel::L11 => *l11,
    }
}

// ── +level growth curves (classic client progressions; 075 bonus tables) ──

/// Weapon damage bonus per enhancement level.
const WEAPON_DAMAGE_BONUS_BY_LEVEL: [u16; 12] = [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36];
/// Armor defense bonus per enhancement level.
const ARMOR_DEFENSE_BONUS_BY_LEVEL: [u16; 12] = [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36];
/// Shield defense bonus per enhancement level.
const SHIELD_DEFENSE_BONUS_BY_LEVEL: [u16; 12] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
/// Shield defense-rate bonus per enhancement level.
const SHIELD_DEFENSE_RATE_BONUS_BY_LEVEL: [u16; 12] = [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36];
/// Wing defense bonus per enhancement level.
const WING_DEFENSE_BONUS_BY_LEVEL: [u16; 12] = [0, 3, 6, 9, 12, 15, 18, 21, 24, 27, 31, 36];
/// Staff rise growth for even magic-power (client rounding of MP/2).
const STAFF_RISE_BONUS_BY_LEVEL_EVEN: [u16; 12] = [0, 3, 7, 10, 14, 17, 21, 24, 28, 31, 35, 40];
/// Staff rise growth for odd magic-power.
const STAFF_RISE_BONUS_BY_LEVEL_ODD: [u16; 12] = [0, 4, 7, 11, 14, 18, 21, 25, 28, 32, 36, 40];
/// Wing damage-absorption growth, percent points per level.
const WING_ABSORB_PCT_PER_LEVEL: u8 = 2;
/// First-generation wing damage-increase growth, percent points per level.
const FIRST_WING_DAMAGE_PCT_PER_LEVEL: u8 = 2;
/// Second-generation wing damage-increase growth, percent points per level.
/// Review: S6-sourced backport riding the 2nd-wings curation.
const SECOND_WING_DAMAGE_PCT_PER_LEVEL: u8 = 1;
// W-SRC: a +0 ring/pendant already grants resistance 1; the per-level curve
// adds on top, so the shipped +0..+4 jewelry spans 1..=5
// (Version075/Items/Jewelery.cs:17,171-173).
/// Jewelry's base resistance at item level 0.
const JEWELRY_RESISTANCE_BASE: u8 = 1;
/// Jewelry resistance growth per item level; the +4 ceiling is the jewelry
/// `max_item_level`.
const JEWELRY_RESISTANCE_PER_LEVEL: u8 = 1;

/// Weapon damage bonus at an enhancement level.
#[must_use]
pub fn weapon_damage_bonus(level: EnhanceLevel) -> u16 {
    by_enhance_level(level, &WEAPON_DAMAGE_BONUS_BY_LEVEL)
}

/// Armor defense bonus at an enhancement level.
#[must_use]
pub fn armor_defense_bonus(level: EnhanceLevel) -> u16 {
    by_enhance_level(level, &ARMOR_DEFENSE_BONUS_BY_LEVEL)
}

/// Shield defense bonus at an enhancement level. Consumed by the equipment
/// fold; hosts read the folded profile, never a raw curve.
#[must_use]
pub(crate) fn shield_defense_bonus(level: EnhanceLevel) -> u16 {
    by_enhance_level(level, &SHIELD_DEFENSE_BONUS_BY_LEVEL)
}

/// Shield defense-rate bonus at an enhancement level. Consumed by the
/// equipment fold.
#[must_use]
pub(crate) fn shield_defense_rate_bonus(level: EnhanceLevel) -> u16 {
    by_enhance_level(level, &SHIELD_DEFENSE_RATE_BONUS_BY_LEVEL)
}

/// Wing defense bonus at an enhancement level. Consumed by the equipment fold.
#[must_use]
pub(crate) fn wing_defense_bonus(level: EnhanceLevel) -> u16 {
    by_enhance_level(level, &WING_DEFENSE_BONUS_BY_LEVEL)
}

/// Staff rise, doubled: `magic_power + 2 * curve[parity][level]`. The doubled
/// form carries the client's half-point steps without fractional state, and is
/// integer-exact at every level. Consumed by the equipment fold.
#[must_use]
pub(crate) fn staff_rise_x2(magic_power: u16, level: EnhanceLevel) -> u16 {
    let curve = if magic_power % 2 == 0 {
        &STAFF_RISE_BONUS_BY_LEVEL_EVEN
    } else {
        &STAFF_RISE_BONUS_BY_LEVEL_ODD
    };
    let bonus = by_enhance_level(level, curve);
    magic_power.saturating_add(bonus.saturating_mul(2))
}

/// Wing damage absorption at an item level: `base + 2 * level`, in percent
/// points. Consumed by the equipment fold.
#[must_use]
pub(crate) fn wing_absorb_percent(base: u8, level: EnhanceLevel) -> u8 {
    base.saturating_add(WING_ABSORB_PCT_PER_LEVEL.saturating_mul(level.wire()))
}

/// Wing damage increase at an item level, in percent points; the per-level
/// step depends on the wing generation. Consumed by the equipment fold.
#[must_use]
pub(crate) fn wing_damage_percent(base: u8, tier: WingTier, level: EnhanceLevel) -> u8 {
    let step = match tier {
        WingTier::First => FIRST_WING_DAMAGE_PCT_PER_LEVEL,
        WingTier::Second => SECOND_WING_DAMAGE_PCT_PER_LEVEL,
    };
    base.saturating_add(step.saturating_mul(level.wire()))
}

/// Jewelry resistance granted at an item level: base 1 plus 1 per level, so
/// the shipped +0..+4 jewelry grants an integer 1..=5. Consumed by the
/// equipment fold's per-element Maximum aggregation.
#[must_use]
pub(crate) fn jewelry_resistance(level: EnhanceLevel) -> u8 {
    JEWELRY_RESISTANCE_BASE
        .saturating_add(JEWELRY_RESISTANCE_PER_LEVEL.saturating_mul(level.wire()))
}

// ── ammunition (ships only at levels 0..=2) ──

// W-SRC: era-confirmed 0.95d values — {0, +3%, +5%} for ammo levels 0/1/2
// (Version095d/Items/Weapons.cs:32,92,213-215; absent in 0.75, S6 adds a +7%
// fourth tier). The equipment fold consumes it unconditionally for a
// bow/crossbow wielder with ammo — the elf archery-mode buff gating is a
// stated in-wave divergence.
/// Ammunition damage bonus, percent points.
#[must_use]
pub fn ammunition_damage_percent(level: AmmoLevel) -> u8 {
    match level {
        AmmoLevel::L0 => 0,
        AmmoLevel::L1 => 3,
        AmmoLevel::L2 => 5,
    }
}

// ── durability (classic per-level bonus table) ──

/// Durability bonus per enhancement level.
const DURABILITY_BONUS_BY_LEVEL: [u8; 12] = [0, 1, 2, 3, 4, 6, 8, 10, 12, 14, 17, 21];
/// Extra durability granted to excellent items.
const EXCELLENT_DURABILITY_BONUS: u8 = 15;
/// Extra durability granted to ancient items.
const ANCIENT_DURABILITY_BONUS: u8 = 20;

/// Maximum durability of an item at a level and rarity, capped at 255 by the
/// `u8` wire encoding (the saturating adds never exceed it).
#[must_use]
pub fn max_durability(base: u8, level: EnhanceLevel, rarity: ItemRarity) -> u8 {
    let level_bonus = by_enhance_level(level, &DURABILITY_BONUS_BY_LEVEL);
    let rarity_bonus = match rarity {
        ItemRarity::Normal => 0,
        ItemRarity::Excellent => EXCELLENT_DURABILITY_BONUS,
        ItemRarity::Ancient => ANCIENT_DURABILITY_BONUS,
    };
    base.saturating_add(level_bonus)
        .saturating_add(rarity_bonus)
}

// ── effective drop level (the single definition of the classic rule) ──

/// Drop levels added per item level.
pub const DROP_LEVEL_PER_ITEM_LEVEL: u16 = 3;
/// Drop-level surcharge for excellent items.
pub const EXCELLENT_DROP_LEVEL_BONUS: u16 = 25;
/// Drop-level surcharge for ancient items.
pub const ANCIENT_DROP_LEVEL_BONUS: u16 = 30;

/// `drop_level + 3 * item_level`, plus 25 excellent / 30 ancient.
#[must_use]
pub fn effective_drop_level(drop_level: u8, level: EnhanceLevel, rarity: ItemRarity) -> u16 {
    let surcharge = match rarity {
        ItemRarity::Normal => 0,
        ItemRarity::Excellent => EXCELLENT_DROP_LEVEL_BONUS,
        ItemRarity::Ancient => ANCIENT_DROP_LEVEL_BONUS,
    };
    u16::from(drop_level)
        .saturating_add(DROP_LEVEL_PER_ITEM_LEVEL.saturating_mul(u16::from(level.wire())))
        .saturating_add(surcharge)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The crate-internal curves' value pins (the equipment spec §0.5 figures) —
    // inline because the curves are `pub(crate)`; the fold OUTPUT these feed
    // is proven in `core/tests/` and the profile fold's own tests.

    #[test]
    fn shield_and_wing_curves_pin_their_plus_ten_values() {
        assert_eq!(shield_defense_bonus(EnhanceLevel::L10), 10);
        assert_eq!(shield_defense_rate_bonus(EnhanceLevel::L10), 31);
        assert_eq!(wing_defense_bonus(EnhanceLevel::L10), 31);
        assert_eq!(shield_defense_bonus(EnhanceLevel::L0), 0);
        assert_eq!(shield_defense_rate_bonus(EnhanceLevel::L0), 0);
        assert_eq!(wing_defense_bonus(EnhanceLevel::L0), 0);
    }

    #[test]
    fn staff_rise_x2_keeps_the_odd_half_point_and_the_even_whole_point() {
        // Odd magic power 59 at +1: 59 + 2·4 = 67 (rise 33.5, carried whole).
        assert_eq!(staff_rise_x2(59, EnhanceLevel::L1), 67);
        // Even magic power 60 at +2: 60 + 2·7 = 74 (rise 37).
        assert_eq!(staff_rise_x2(60, EnhanceLevel::L2), 74);
        // +0 is the bare magic power on both parities.
        assert_eq!(staff_rise_x2(59, EnhanceLevel::L0), 59);
        assert_eq!(staff_rise_x2(60, EnhanceLevel::L0), 60);
    }

    #[test]
    fn wing_percent_curves_step_two_per_level_and_one_for_second_generation() {
        assert_eq!(wing_absorb_percent(12, EnhanceLevel::L2), 16);
        assert_eq!(
            wing_damage_percent(12, WingTier::First, EnhanceLevel::L2),
            16
        );
        assert_eq!(
            wing_damage_percent(32, WingTier::Second, EnhanceLevel::L2),
            34
        );
        assert_eq!(wing_absorb_percent(12, EnhanceLevel::L0), 12);
    }

    #[test]
    fn jewelry_resistance_grants_base_one_plus_level() {
        // The base-1 correction: a +0 ring already grants 1; +4 grants 5.
        assert_eq!(jewelry_resistance(EnhanceLevel::L0), 1);
        assert_eq!(jewelry_resistance(EnhanceLevel::L1), 2);
        assert_eq!(jewelry_resistance(EnhanceLevel::L4), 5);
    }
}
