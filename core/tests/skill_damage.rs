//! Skill & wizardry damage over the real `/data` Atlas (W-SKILLDMG): the
//! `DamageType` dispatch that selects every skill strike's span, the asymmetric
//! `attack_damage` add, the per-type excellent order, the class `SkillMultiplier`
//! (skill strikes only, never plain swings), the wizardry-absent and None-type
//! scratch collapses, and the fixed RNG draw discipline across every branch —
//! all proven through the public `cast`/`resolve_attack` ports against the
//! shipped skill roster and class derivations.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body
//! so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use rand_core::RngCore;

use dataset::{or_abort, real_atlas};
use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::combat_profile::{CombatProfile, CombatTarget};
use mu_core::components::element::PerElement;
use mu_core::components::interval::Interval;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::Facing;
use mu_core::components::tile::{TerrainGrid, TileCoord};
use mu_core::components::units::{Level, MapNumber, Resistance};
use mu_core::data::skills::{DamageType, Skill};
use mu_core::entities::character::Character;
use mu_core::events::combat::AttackOutcome;
use mu_core::events::skills::{SkillOutcome, TargetHit};
use mu_core::services::combat::{ExcellentOrder, StrikeBasis, resolve_attack};
use mu_core::services::profile::{character_profile, monster_profile};
use mu_core::services::skills::{DamagingSkillRef, SkillRouting, cast, route};
use rng::TestRng;

// --- Fixtures. ----------------------------------------------------------------

/// A gearless level-50 caster of any class/stat mix at tile (10, 10), facing
/// +X, with deep vitals so every cast in a sweep is funded — built the only way
/// a character can be, by deserialising its wire form. Command classes get the
/// `with_command` stat shape.
fn caster(class: &str, strength: u16, energy: u16) -> Character {
    let stats = if class == "dark_lord" {
        serde_json::json!({"kind": "with_command", "strength": strength, "agility": 100, "vitality": 100, "energy": energy, "command": 50})
    } else {
        serde_json::json!({"kind": "standard", "strength": strength, "agility": 100, "vitality": 100, "energy": energy})
    };
    let json = serde_json::json!({
        "class": class,
        "level": 50,
        "experience": 0,
        "stats": stats,
        "unspent_points": 0,
        "zen": 0,
        "placement": {
            "position": or_abort(serde_json::to_value(TileCoord::new(10, 10).to_world())),
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": 0
        },
        "vitals": {
            "health": {"current": 10_000, "max": 10_000},
            "mana": {"current": 100_000, "max": 100_000},
            "ability": {"current": 100_000, "max": 100_000}
        }
    });
    or_abort(serde_json::from_value(json))
}

fn zero_resistances() -> PerElement<Resistance> {
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

/// A defender profile with tunable defense and defense rate, zero special-hit
/// chances — derived through the real monster-profile port.
fn defender_profile(defense: u16, defense_rate: u16) -> CombatProfile {
    let combat = mu_core::data::monster_definitions::MonsterCombat {
        level: or_abort(Level::new(20)),
        hp: 1_000_000,
        min_phys_damage: 5,
        max_phys_damage: 10,
        defense,
        attack_rate: 10,
        defense_rate,
    };
    monster_profile(&combat, &zero_resistances(), combat.level)
}

/// A deep-health seated defender at `tile` with the given defense, zero
/// defense rate (no overrate), no effects.
fn seated_target(tile: (u8, u8), defense: u16) -> CombatTarget {
    let placement = Placement {
        position: TileCoord::new(tile.0, tile.1).to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: MapNumber(0),
    };
    CombatTarget::new(
        defender_profile(defense, 0),
        Pool::full(1_000_000),
        placement,
        ActiveEffects::EMPTY,
    )
}

fn all_walkable() -> TerrainGrid {
    TerrainGrid::from_words([u64::MAX; 1024])
}

/// The damaging reference the router yields; a non-damaging skill aborts (this
/// harness only ever selects damaging records).
fn damaging_ref(skill: &Skill) -> DamagingSkillRef<'_> {
    match route(skill) {
        SkillRouting::Damaging(reference) => reference,
        SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => {
            or_abort(Err::<DamagingSkillRef<'_>, _>("expected a damaging skill"))
        }
    }
}

/// Where the one target sits and where the cast aims for a given skill: a
/// zero-range skill (caster-centred or self-anchored) strikes a target on the
/// caster's own tile; everything else strikes the tile straight ahead, inside
/// every shape the roster carries (cones and lines face +X).
fn strike_tiles(skill: &Skill) -> (u8, u8) {
    if skill.range == 0 { (10, 10) } else { (11, 10) }
}

/// Casts `skill` once from `caster` at a single seated zero-defense target and
/// returns the landed/killed damage, or `None` on a miss.
fn cast_damage(caster: &Character, skill: &Skill, seed: u64) -> Option<u32> {
    let tile = strike_tiles(skill);
    let targets = [seated_target(tile, 0)];
    let aim = TileCoord::new(tile.0, tile.1).to_world();
    let (_, outcome) = cast(
        caster,
        &character_profile(caster).0,
        damaging_ref(skill).locate(aim),
        &targets,
        &all_walkable(),
        &mut TestRng::new(seed),
    );
    landed_damage(&outcome)
}

/// The damage the single struck target took, or `None` for a miss/rejection.
fn landed_damage(outcome: &SkillOutcome) -> Option<u32> {
    match outcome {
        SkillOutcome::Cast { hits, .. } => match hits.first() {
            Some(TargetHit::Landed { hit, .. } | TargetHit::Killed { hit, .. }) => {
                Some(hit.damage.0)
            }
            Some(TargetHit::Missed { .. }) | None => None,
        },
        SkillOutcome::Rejected { .. } => None,
    }
}

/// The first landed damage across a seed sweep, with the seed that landed it.
fn first_landed(caster: &Character, skill: &Skill) -> (u64, u32) {
    for seed in 0u64..256 {
        if let Some(damage) = cast_damage(caster, skill, seed) {
            return (seed, damage);
        }
    }
    or_abort(Err::<(u64, u32), _>(format!(
        "skill {:?}: no seed in 0..256 lands a hit",
        skill.number
    )))
}

/// The augmented span bounds `[min + D, max + D + D/2]`.
fn augmented(base: Interval<u16>, d: u16) -> (u32, u32) {
    (
        u32::from(base.min()) + u32::from(d),
        u32::from(base.max()) + u32::from(d) + u32::from(d / 2),
    )
}

// --- Per-DamageType damage over the real skill roster. -------------------------

#[test]
fn every_wizardry_skill_scales_with_energy_and_never_reads_the_weapon_span() {
    let atlas = real_atlas();
    for energy in [162u16, 720u16] {
        let wizard = caster("dark_wizard", 40, energy);
        let profile = character_profile(&wizard).0;
        let wiz = profile.wizardry().expect("a Dark Wizard carries wizardry");
        assert_eq!(
            (wiz.min(), wiz.max()),
            (energy / 9, energy / 4),
            "the gearless wizardry span is Energy-derived"
        );
        let phys_max = u32::from(profile.physical().max());
        for skill in atlas.skills() {
            if skill.damage_type != DamageType::Wizardry
                || !matches!(route(skill), SkillRouting::Damaging(_))
            {
                continue;
            }
            let (aug_min, aug_max) = augmented(wiz, skill.attack_damage);
            let (seed, damage) = first_landed(&wizard, skill);
            // DW multiplier is ×1000 (identity) and the target is
            // zero-defense, so a landed hit is exactly a draw from the
            // augmented wizardry span — never the physical weapon span.
            assert!(
                damage >= aug_min && damage <= aug_max,
                "skill {:?} energy {energy} seed {seed}: {damage} outside [{aug_min}, {aug_max}]",
                skill.number
            );
            assert!(
                damage > phys_max,
                "skill {:?}: {damage} must exceed the weapon span max {phys_max}",
                skill.number
            );
        }
    }
}

#[test]
fn every_physical_skill_honors_its_attack_damage_over_the_real_roster() {
    let atlas = real_atlas();
    let knight = caster("dark_knight", 200, 30);
    let profile = character_profile(&knight).0;
    let span = profile.physical();
    assert_eq!((span.min(), span.max()), (33, 50));
    // DK skill multiplier: 2000 + 30 energy.
    let mult = 2030u32;
    for skill in atlas.skills() {
        if skill.damage_type != DamageType::Physical
            || !matches!(route(skill), SkillRouting::Damaging(_))
        {
            continue;
        }
        let (aug_min, aug_max) = augmented(span, skill.attack_damage);
        let low = aug_min * mult / 1000;
        let high = aug_max * mult / 1000;
        let (seed, damage) = first_landed(&knight, skill);
        assert!(
            damage >= low && damage <= high,
            "skill {:?} seed {seed}: {damage} outside [{low}, {high}]",
            skill.number
        );
        if skill.attack_damage > 0 {
            // A D>0 skill out-damages the same caster's plain swing by its
            // span add: even its minimum exceeds the multiplied bare max.
            assert!(
                damage > u32::from(span.max()) * mult / 1000,
                "skill {:?}: {damage} must exceed the bare multiplied span",
                skill.number
            );
        }
    }
}

#[test]
fn a_caster_without_wizardry_deals_the_scratch_on_every_wizardry_skill() {
    // A DK (no wizardry interval) casting any real wizardry skill lands the
    // level-floor × multiplier scratch: max(1, 50/10) = 5, × 2030/1000 = 10 —
    // never the weapon span, never the skill's attack_damage.
    let atlas = real_atlas();
    let knight = caster("dark_knight", 200, 30);
    for skill in atlas.skills() {
        if skill.damage_type != DamageType::Wizardry
            || !matches!(route(skill), SkillRouting::Damaging(_))
        {
            continue;
        }
        let (seed, damage) = first_landed(&knight, skill);
        assert_eq!(
            damage, 10,
            "skill {:?} seed {seed}: a wizardry-absent cast is the exact scratch",
            skill.number
        );
    }
}

#[test]
fn a_none_type_skill_deals_the_scratch_and_its_authored_damage_never_enters() {
    // Record 50 carries attack_damage 120 and DamageType::None: the 120 is
    // discarded and the cast lands floor 5 × 2030/1000 = 10.
    let atlas = real_atlas();
    let knight = caster("dark_knight", 200, 30);
    let mut none_records = 0u32;
    for skill in atlas.skills() {
        if skill.damage_type != DamageType::None
            || !matches!(route(skill), SkillRouting::Damaging(_))
        {
            continue;
        }
        assert!(skill.attack_damage > 0, "record 50 authors a real damage");
        let (seed, damage) = first_landed(&knight, skill);
        assert_eq!(
            damage, 10,
            "skill {:?} seed {seed}: a None-type cast is the exact scratch",
            skill.number
        );
        none_records += 1;
    }
    assert!(
        none_records > 0,
        "the roster carries a None-type damaging skill"
    );
}

#[test]
fn the_wizardry_excellent_order_holds_against_real_spans() {
    // A real DW span (energy 100 → [11, 25]) augmented by the first D>0
    // wizardry skill, excellent forced through the wire, against a defense-30
    // target: wizardry subtracts defense BEFORE the ×6/5; the physical order
    // would multiply first — the defense-position gap separates them.
    let atlas = real_atlas();
    let wizard = caster("dark_wizard", 40, 100);
    let profile = character_profile(&wizard).0;
    let wiz = profile.wizardry().expect("a Dark Wizard carries wizardry");
    let skill = or_abort(
        atlas
            .skills()
            .find(|skill| {
                skill.damage_type == DamageType::Wizardry
                    && skill.attack_damage > 0
                    && matches!(route(skill), SkillRouting::Damaging(_))
            })
            .ok_or("the roster has a D>0 wizardry skill"),
    );
    let (aug_min, aug_max) = augmented(wiz, skill.attack_damage);
    let excellent_attacker = with_excellent(&profile, 100);
    let target = defender_profile(30, 0);
    let strike = |order: ExcellentOrder| {
        let basis = StrikeBasis::Skill {
            span: or_abort(Interval::new(
                or_abort(u16::try_from(aug_min)),
                or_abort(u16::try_from(aug_max)),
            )),
            excellent_order: order,
            multiplier_per_mille: 1000,
        };
        let mut rng = TestRng::new(9);
        let (_, outcome) = resolve_attack(
            &excellent_attacker,
            &target,
            Pool::full(100_000),
            &basis,
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => hit.damage.0,
            AttackOutcome::Missed => panic!("a full-rate strike never misses"),
        }
    };
    let wizardry = strike(ExcellentOrder::DefenseThenMultiply);
    let physical = strike(ExcellentOrder::MultiplyThenDefense);
    assert_eq!(wizardry, (aug_max - 30) * 6 / 5);
    assert_eq!(physical, aug_max * 6 / 5 - 30);
    assert!(
        wizardry < physical,
        "the defense-position gap separates the orders: {wizardry} vs {physical}"
    );
}

/// The profile with its excellent chance forced through the wire — the only way
/// an integration test reaches a field the smart constructors own.
fn with_excellent(profile: &CombatProfile, percent: u8) -> CombatProfile {
    let mut value = or_abort(serde_json::to_value(profile));
    let object = or_abort(value.as_object_mut().ok_or("a profile is an object"));
    object.insert("excellent_chance".to_owned(), serde_json::json!(percent));
    or_abort(serde_json::from_value(value))
}

// --- Per-class SkillMultiplier over the real class table. ----------------------

/// The first zero-damage physical skill — the bare-weapon-span skill whose only
/// difference from a plain swing is the class multiplier.
fn weapon_skill(atlas: &mu_core::data::atlas::Atlas) -> &Skill {
    or_abort(
        atlas
            .skills()
            .find(|skill| {
                skill.damage_type == DamageType::Physical
                    && skill.attack_damage == 0
                    && matches!(route(skill), SkillRouting::Damaging(_))
            })
            .ok_or("the roster has a D=0 weapon skill"),
    )
}

/// A plain swing under `seed` against the standard zero-defense target, or
/// `None` on a miss.
fn plain_swing(profile: &CombatProfile, seed: u64) -> Option<u32> {
    let target = defender_profile(0, 0);
    let mut rng = TestRng::new(seed);
    let (_, outcome) = resolve_attack(
        profile,
        &target,
        Pool::full(1_000_000),
        &StrikeBasis::PlainSwing,
        &mut rng,
    );
    match outcome {
        AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => Some(hit.damage.0),
        AttackOutcome::Missed => None,
    }
}

/// Asserts that under a shared seed the caster's D=0 skill strike equals its
/// plain swing scaled by exactly `mult` per-mille, and returns one compared
/// pair. A D=0 skill draws the identical span roll as the plain swing, so the
/// multiplier is the only difference.
fn assert_skill_is_scaled_swing(caster_of: &Character, skill: &Skill, mult: u32) -> (u32, u32) {
    let profile = character_profile(caster_of).0;
    for seed in 0u64..64 {
        let (Some(skill_dmg), Some(plain_dmg)) = (
            cast_damage(caster_of, skill, seed),
            plain_swing(&profile, seed),
        ) else {
            continue;
        };
        assert_eq!(
            skill_dmg,
            plain_dmg * mult / 1000,
            "class {:?} seed {seed}: the skill is the plain swing × {mult}/1000",
            caster_of.class()
        );
        return (skill_dmg, plain_dmg);
    }
    or_abort(Err::<(u32, u32), _>("no seed in 0..64 lands both strikes"))
}

#[test]
fn every_class_multiplies_skills_by_its_own_ferocity_and_swings_untouched() {
    let atlas = real_atlas();
    let skill = weapon_skill(&atlas);
    // DW/SM and FE/ME ×1 (the skill equals the swing); MG flat ×2; DK/BK
    // 2000 + Energy; DL 2000 + Energy/2 (integer floor on 101).
    let cases: [(&str, u16, u16, u32); 8] = [
        ("dark_wizard", 40, 300, 1000),
        ("soul_master", 40, 300, 1000),
        ("fairy_elf", 40, 300, 1000),
        ("muse_elf", 40, 300, 1000),
        ("magic_gladiator", 90, 300, 2000),
        ("dark_knight", 200, 30, 2030),
        ("blade_knight", 200, 500, 2500),
        ("dark_lord", 200, 101, 2050),
    ];
    for (class, strength, energy, mult) in cases {
        let hero = caster(class, strength, energy);
        let (skill_dmg, plain_dmg) = assert_skill_is_scaled_swing(&hero, skill, mult);
        if mult == 1000 {
            assert_eq!(skill_dmg, plain_dmg, "{class}: no ferocity, skill == swing");
        }
    }
}

#[test]
fn the_knight_energy_term_sharpens_the_skill_while_the_swing_stays_equal() {
    let atlas = real_atlas();
    let skill = weapon_skill(&atlas);
    // Two knights of identical strength: equal spans, equal plain swings under
    // one seed; only the skill multiplier (2000 + E) separates their skills.
    let low = caster("dark_knight", 200, 30);
    let high = caster("dark_knight", 200, 500);
    let low_profile = character_profile(&low).0;
    let high_profile = character_profile(&high).0;
    assert_eq!(low_profile.physical(), high_profile.physical());
    let mut compared = false;
    for seed in 0u64..64 {
        let (Some(low_plain), Some(high_plain)) = (
            plain_swing(&low_profile, seed),
            plain_swing(&high_profile, seed),
        ) else {
            continue;
        };
        assert_eq!(low_plain, high_plain, "plain swings carry no energy term");
        let (Some(low_skill), Some(high_skill)) = (
            cast_damage(&low, skill, seed),
            cast_damage(&high, skill, seed),
        ) else {
            continue;
        };
        assert_eq!(low_skill, low_plain * 2030 / 1000);
        assert_eq!(high_skill, high_plain * 2500 / 1000);
        assert!(high_skill > low_skill, "energy sharpens the knight's skill");
        compared = true;
        break;
    }
    assert!(compared, "a seed in 0..64 lands all four strikes");
}

#[test]
fn the_gladiator_multiplier_is_flat_and_the_lord_takes_half_energy() {
    let atlas = real_atlas();
    let skill = weapon_skill(&atlas);
    // MG: flat ×2000 regardless of energy (energy moves its SPAN, so each
    // caster is checked against its own swing).
    for energy in [100u16, 600u16] {
        let gladiator = caster("magic_gladiator", 90, energy);
        assert_skill_is_scaled_swing(&gladiator, skill, 2000);
    }
    // DL: 2000 + Energy/2, integer floor (101 → 2050).
    let lord = caster("dark_lord", 200, 101);
    assert_skill_is_scaled_swing(&lord, skill, 2050);
}

#[test]
fn a_monster_plain_swing_never_acquires_a_multiplier_over_real_data() {
    // The first fighting monster strikes with its own span on a PlainSwing
    // basis: every landed hit stays at or under its span max — no ~2× class
    // ferocity ever touches a monster's swing.
    let atlas = real_atlas();
    let (combat, resistances) = or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                mu_core::data::monster_definitions::MonsterRole::Monster {
                    combat,
                    resistances,
                    ..
                } => Some((*combat, *resistances)),
                mu_core::data::monster_definitions::MonsterRole::Guard { .. }
                | mu_core::data::monster_definitions::MonsterRole::Trap { .. }
                | mu_core::data::monster_definitions::MonsterRole::Npc { .. }
                | mu_core::data::monster_definitions::MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has a fighting monster"),
    );
    let attacker = monster_profile(&combat, &resistances, combat.level);
    let target = defender_profile(0, 0);
    let ceiling = u32::from(combat.max_phys_damage).max(1);
    let mut rng = TestRng::new(3);
    let mut landed = 0u32;
    for _ in 0..500 {
        let (_, outcome) = resolve_attack(
            &attacker,
            &target,
            Pool::full(1_000_000),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        if let AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } = outcome {
            assert!(
                hit.damage.0 <= ceiling,
                "a plain swing stays within its own span: {} > {ceiling}",
                hit.damage.0
            );
            landed += 1;
        }
    }
    assert!(landed > 0, "the monster lands hits in 500 tries");
}

// --- Determinism. ---------------------------------------------------------------

/// An RNG that counts the words it hands out.
struct CountingRng {
    inner: TestRng,
    words: u32,
}

impl RngCore for CountingRng {
    fn next_u64(&mut self) -> u64 {
        self.words += 1;
        self.inner.next_u64()
    }

    fn next_u32(&mut self) -> u32 {
        self.words += 1;
        let [b0, b1, b2, b3, _, _, _, _] = self.inner.next_u64().to_le_bytes();
        u32::from_le_bytes([b0, b1, b2, b3])
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.inner.fill_bytes(dst);
    }
}

/// One cast counted: the words drawn and whether the hit landed.
fn counted_cast(hero: &Character, skill: &Skill, seed: u64) -> (u32, bool) {
    let tile = strike_tiles(skill);
    let targets = [seated_target(tile, 0)];
    let aim = TileCoord::new(tile.0, tile.1).to_world();
    let mut rng = CountingRng {
        inner: TestRng::new(seed),
        words: 0,
    };
    let (_, outcome) = cast(
        hero,
        &character_profile(hero).0,
        damaging_ref(skill).locate(aim),
        &targets,
        &all_walkable(),
        &mut rng,
    );
    (rng.words, landed_damage(&outcome).is_some())
}

#[test]
fn every_damage_type_branch_draws_the_same_word_count() {
    // Four non-elemental direct-hit casts differing only in the span the
    // DamageType selects — physical, wizardry, wizardry-collapsed-to-[0,0],
    // and None-[0,0] — advance the RNG by the identical word count.
    let atlas = real_atlas();
    let find = |wanted: DamageType, d_positive: bool| {
        or_abort(
            atlas
                .skills()
                .find(|skill| {
                    skill.damage_type == wanted
                        && (skill.attack_damage > 0) == d_positive
                        && skill.element.is_none()
                        && matches!(
                            route(skill),
                            SkillRouting::Damaging(reference)
                                if matches!(reference.shape(), mu_core::services::skills::DamagingSkill::DirectHit)
                        )
                })
                .ok_or("the roster has the direct-hit skill shape"),
        )
    };
    let physical = find(DamageType::Physical, true);
    let wizardry = find(DamageType::Wizardry, true);
    let none = find(DamageType::None, true);
    let wizard = caster("dark_wizard", 40, 100);
    let knight = caster("dark_knight", 200, 30);
    for seed in 0u64..64 {
        let runs = [
            counted_cast(&knight, physical, seed),
            counted_cast(&wizard, wizardry, seed),
            // The collapse: a DK casting wizardry strikes a [0,0] span.
            counted_cast(&knight, wizardry, seed),
            counted_cast(&knight, none, seed),
        ];
        if runs.iter().any(|(_, landed)| !landed) {
            continue;
        }
        let words = runs[0].0;
        for (drawn, _) in runs {
            assert_eq!(drawn, words, "seed {seed}: identical draw count");
        }
        return;
    }
    panic!("no seed in 0..64 lands all four casts");
}

#[test]
fn identical_inputs_and_seeds_replay_byte_identical() {
    let atlas = real_atlas();
    let wizard = caster("dark_wizard", 40, 400);
    let skill = or_abort(
        atlas
            .skills()
            .find(|skill| {
                skill.damage_type == DamageType::Wizardry
                    && matches!(route(skill), SkillRouting::Damaging(_))
            })
            .ok_or("the roster has a wizardry skill"),
    );
    let tile = strike_tiles(skill);
    let targets = [seated_target(tile, 0)];
    let aim = TileCoord::new(tile.0, tile.1).to_world();
    let run = || {
        let (vitals, outcome) = cast(
            &wizard,
            &character_profile(&wizard).0,
            damaging_ref(skill).locate(aim),
            &targets,
            &all_walkable(),
            &mut TestRng::new(41),
        );
        (
            or_abort(serde_json::to_string(&vitals)),
            or_abort(serde_json::to_string(&outcome)),
        )
    };
    assert_eq!(run(), run());
}
