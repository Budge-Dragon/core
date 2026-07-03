//! Derivation of a fighter's [`CombatProfile`] from its source: a character's
//! class, level, and stats, or a monster's combat block. Every derived stat is a
//! pooled single divide — the whole numerator is summed in `u64` and floored
//! once, never a sum of per-term truncations. Gearless characters and pre-S3
//! monsters carry zero special-hit chances; equipment feeds them in a later
//! wave.

use crate::components::class::CharacterClass;
use crate::components::combat_profile::{CombatProfile, VitalMaxima};
use crate::components::element::PerElement;
use crate::components::interval::Interval;
use crate::components::stats::Stats;
use crate::components::units::{Level, Percent, Resistance};
use crate::data::monster_definitions::MonsterCombat;
use crate::entities::character::Character;
use crate::services::ratio::{floor_div_u64_to_u32, nonzero};

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
    }
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
}
