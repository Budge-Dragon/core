//! Skill area geometry & displacement over the real `/data` Atlas (W-AREA):
//! the 18 authored `AreaGeometry` records at their ratified sizes, the
//! aim-bound gate (aim-centered shapes range-gate their aim before spend;
//! caster-anchored shapes never read it), the Earthshake directional push
//! (continuous swept knockback along the real away-vector, 3 tiles, per-tile
//! wall/safe stop, same-point no-push, on-miss but never on-kill), the lunge
//! teleport-onto-target across walls with its
//! `MovesTarget` jiggle, the lightning continuous ~1-tile jiggle, and the
//! per-branch RNG draw discipline — all proven through the public `route`/`locate`/`cast`
//! ports against the shipped skill roster.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body
//! so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use std::collections::BTreeSet;

use rand_core::RngCore;

use dataset::{or_abort, real_atlas};
use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::combat_profile::CombatTarget;
use mu_core::components::element::PerElement;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, UNITS_PER_TILE, WorldPos};
use mu_core::components::tile::{TerrainGrid, TileCoord};
use mu_core::components::units::{Level, MapNumber, Resistance};
use mu_core::components::vitals::Vitals;
use mu_core::data::atlas::Atlas;
use mu_core::data::common::SkillNumber;
use mu_core::data::skills::{AreaDisplacement, AreaGeometry, Skill, SkillShape};
use mu_core::entities::character::Character;
use mu_core::events::skills::{CastRejection, SkillOutcome, TargetHit};
use mu_core::services::profile::{character_profile, monster_profile};
use mu_core::services::skills::{DamagingSkill, DamagingSkillRef, SkillRouting, cast, route};
use rng::TestRng;

// --- Fixtures. ----------------------------------------------------------------

/// One tile in sub-units — the displacement bounds below are tile-grained.
const TILE: i64 = UNITS_PER_TILE;

/// A gearless level-50 caster at tile (10, 10) facing +X, with deep vitals so
/// every cast in a sweep is funded — built the only way a character can be, by
/// deserialising its wire form.
fn caster(class: &str, strength: u16, energy: u16) -> Character {
    let json = serde_json::json!({
        "class": class,
        "level": 50,
        "experience": 0,
        "stats": {"kind": "standard", "strength": strength, "agility": 100, "vitality": 100, "energy": energy},
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

fn resistances(lightning: u8) -> PerElement<Resistance> {
    PerElement {
        ice: Resistance(0),
        poison: Resistance(0),
        lightning: Resistance(lightning),
        fire: Resistance(0),
        earth: Resistance(0),
        wind: Resistance(0),
        water: Resistance(0),
    }
}

/// A seated monster target at `tile`: `hp` health, zero defense, a tunable
/// defense rate (raise it above zero when a sweep needs the Missed branch) and
/// lightning resistance — derived through the real monster-profile port.
fn seated(tile: (u8, u8), hp: u32, defense_rate: u16, lightning: u8) -> CombatTarget {
    let combat = mu_core::data::monster_definitions::MonsterCombat {
        level: or_abort(Level::new(20)),
        hp: 1_000_000,
        min_phys_damage: 5,
        max_phys_damage: 10,
        defense: 0,
        attack_rate: 10,
        defense_rate,
    };
    let profile = monster_profile(&combat, &resistances(lightning), combat.level);
    let placement = Placement {
        position: TileCoord::new(tile.0, tile.1).to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: MapNumber(0),
    };
    CombatTarget::new(profile, Pool::full(hp), placement, ActiveEffects::EMPTY)
}

fn all_walkable() -> TerrainGrid {
    TerrainGrid::from_words([u64::MAX; 1024])
}

/// A grid whose walkable set is exactly the listed tiles.
fn grid_with(walkable: &[(u8, u8)]) -> TerrainGrid {
    let mut words = [0u64; 1024];
    for &(x, y) in walkable {
        let bit = (usize::from(y) << 8) | usize::from(x);
        let word = or_abort(words.get_mut(bit >> 6).ok_or("tile bit within the grid"));
        *word |= 1u64 << (bit & 63);
    }
    TerrainGrid::from_words(words)
}

/// A fully walkable grid except the listed tiles, which are walls — the inverse
/// of [`grid_with`], for a push whose whole runway is open save one blocking cell.
fn all_walkable_except(walls: &[(u8, u8)]) -> TerrainGrid {
    let mut words = [u64::MAX; 1024];
    for &(x, y) in walls {
        let bit = (usize::from(y) << 8) | usize::from(x);
        let word = or_abort(words.get_mut(bit >> 6).ok_or("tile bit within the grid"));
        *word &= !(1u64 << (bit & 63));
    }
    TerrainGrid::from_words(words)
}

/// A fully walkable grid on which the listed tiles are also safe (town-truce) —
/// a push runway that is open everywhere except a safe cell the sweep must refuse.
fn all_walkable_with_safe(safe_tiles: &[(u8, u8)]) -> TerrainGrid {
    let mut safe = [0u64; 1024];
    for &(x, y) in safe_tiles {
        let bit = (usize::from(y) << 8) | usize::from(x);
        let word = or_abort(safe.get_mut(bit >> 6).ok_or("tile bit within the grid"));
        *word |= 1u64 << (bit & 63);
    }
    TerrainGrid::from_bitsets([u64::MAX; 1024], safe)
}

/// The real skill record numbered `number` — the §0 ratification table is keyed
/// by these client numbers, so the per-record pins address them directly.
fn skill_number(atlas: &Atlas, number: u16) -> &Skill {
    or_abort(
        atlas
            .skill(SkillNumber(number))
            .ok_or(format!("the roster carries skill {number}")),
    )
}

/// The damaging reference the router yields; a non-damaging skill aborts.
fn damaging_ref(skill: &Skill) -> DamagingSkillRef<'_> {
    match route(skill) {
        SkillRouting::Damaging(reference) => reference,
        SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => {
            or_abort(Err::<DamagingSkillRef<'_>, _>("expected a damaging skill"))
        }
    }
}

/// One cast of `skill` through the public port.
fn cast_once(
    hero: &Character,
    skill: &Skill,
    aim: WorldPos,
    targets: &[CombatTarget],
    grid: &TerrainGrid,
    seed: u64,
) -> (Vitals, SkillOutcome) {
    cast(
        hero,
        &character_profile(hero).0,
        damaging_ref(skill).locate(aim),
        targets,
        grid,
        &mut TestRng::new(seed),
    )
}

/// The batch positions of the targets the cast covered — hit presence is the
/// coverage observable (missed or landed alike; only region membership decides).
fn covered_indices(outcome: &SkillOutcome) -> BTreeSet<usize> {
    match outcome {
        SkillOutcome::Cast { hits, .. } => hits
            .iter()
            .map(|hit| match hit {
                TargetHit::Missed { target_index, .. }
                | TargetHit::Landed { target_index, .. }
                | TargetHit::Killed { target_index, .. } => *target_index,
            })
            .collect(),
        SkillOutcome::Rejected { .. } => BTreeSet::new(),
    }
}

/// The tile-grained (dx, dy) between a displacement and its start.
fn tile_delta(from: WorldPos, to: Placement) -> (i64, i64) {
    (
        (to.position.x().raw() - from.x().raw()) / TILE,
        (to.position.y().raw() - from.y().raw()) / TILE,
    )
}

// --- The 18 ratified geometry records (§0 table). -------------------------------

/// Table shorthand: an aim-centred disc of `radius_x2` half-tiles.
fn aim_circle(radius_x2: u8) -> AreaGeometry {
    AreaGeometry::AimCircle { radius_x2 }
}

/// Table shorthand: a caster-centred disc of `radius_x2` half-tiles.
fn caster_circle(radius_x2: u8) -> AreaGeometry {
    AreaGeometry::CasterCircle { radius_x2 }
}

/// Table shorthand: a forward rect of `length_x2` × `half_width_x2` half-tiles.
fn beam(length_x2: u8, half_width_x2: u8) -> AreaGeometry {
    AreaGeometry::Beam {
        length_x2,
        half_width_x2,
    }
}

/// Table shorthand: a cone of `length_x2` half-tiles at the exact `num/den`
/// squared-cosine half-angle.
fn cone(length_x2: u8, num: u64, den: u64) -> AreaGeometry {
    AreaGeometry::Cone {
        length_x2,
        half_angle: or_abort(mu_core::components::spatial::ConeHalfWidth::new(
            num,
            or_abort(core::num::NonZeroU64::new(den).ok_or("nonzero denominator")),
        )),
    }
}

#[test]
fn every_area_record_carries_its_ratified_authored_geometry() {
    let atlas = real_atlas();
    let none = AreaDisplacement::None;
    let push = AreaDisplacement::DirectionalPush;
    let expected: [(u16, AreaGeometry, AreaDisplacement); 18] = [
        (5, aim_circle(2), none),       // Flame — the ~36× shrink to r=1
        (8, beam(8, 3), none),          // Twister — length 4, half 1.5
        (9, caster_circle(12), none),   // Evil Spirit — r=6
        (10, caster_circle(4), none),   // Hellfire — revived, r=2
        (12, beam(16, 3), none),        // Aqua Beam — length 8, half 1.5
        (13, aim_circle(2), none),      // Cometfall — r=1
        (14, caster_circle(8), none),   // Inferno — revived, r=4
        (24, cone(14, 196, 277), none), // Triple Shot — length 7, exact ratio
        (39, aim_circle(3), none),      // Ice Storm — r=1.5, half-tile grain
        (40, caster_circle(12), none),  // Nova — r=6 (release only)
        (41, caster_circle(4), none),   // Twisting Slash — r=2
        (42, caster_circle(6), none),   // Rageful Blow — r=3
        (43, aim_circle(2), none),      // Death Stab — tightened to r=1
        (52, beam(16, 2), none),        // Penetration — length 8, half 1
        (55, beam(4, 4), none),         // Fire Slash — rect, not a cone
        (56, cone(12, 1, 2), none),     // Power Slash — DEG_45, not DEG_90
        (61, aim_circle(2), none),      // Fire Burst — tightened to r=1
        (62, caster_circle(10), push),  // Earthshake — r=5, the one push
    ];
    for (number, geometry, displacement) in expected {
        let skill = skill_number(&atlas, number);
        match skill.shape {
            SkillShape::Area {
                geometry: authored,
                displacement: authored_displacement,
            } => {
                assert_eq!(authored, geometry, "skill {number}: authored geometry");
                assert_eq!(
                    authored_displacement, displacement,
                    "skill {number}: authored displacement"
                );
            }
            SkillShape::DirectHit
            | SkillShape::Lunge
            | SkillShape::BuffSelf { .. }
            | SkillShape::BuffPlayer { .. }
            | SkillShape::BuffPartyMember { .. }
            | SkillShape::BuffParty { .. }
            | SkillShape::Heal
            | SkillShape::Summon { .. }
            | SkillShape::Teleport
            | SkillShape::NovaCharge
            | SkillShape::RecallParty => panic!("skill {number} must be an area shape"),
        }
    }
    // The table is exhaustive: the roster carries exactly these 18 area records.
    let area_count = atlas
        .skills()
        .filter(|skill| matches!(skill.shape, SkillShape::Area { .. }))
        .count();
    assert_eq!(area_count, 18);
}

#[test]
fn the_aim_centered_set_is_exactly_the_five_ratified_records() {
    let atlas = real_atlas();
    let mut aim_centered = BTreeSet::new();
    let mut caster_anchored = 0usize;
    for skill in atlas.skills() {
        let SkillShape::Area { geometry, .. } = skill.shape else {
            continue;
        };
        match geometry {
            AreaGeometry::AimCircle { .. } => {
                aim_centered.insert(skill.number.0);
            }
            AreaGeometry::CasterCircle { .. }
            | AreaGeometry::Cone { .. }
            | AreaGeometry::Beam { .. } => caster_anchored += 1,
        }
    }
    // Flame, Cometfall, IceStorm, DeathStab, FireBurst — and nothing else.
    assert_eq!(aim_centered, BTreeSet::from([5, 13, 39, 43, 61]));
    assert_eq!(caster_anchored, 13);
}

// --- Ratified coverage over the real roster. -------------------------------------

#[test]
fn the_real_flame_covers_only_its_one_tile_aim_circle() {
    let atlas = real_atlas();
    let flame = skill_number(&atlas, 5);
    let hero = caster("dark_wizard", 40, 400);
    // One tile from the aim, and two tiles out — the old range-6 disc covered both.
    let targets = [
        seated((11, 10), 100_000, 0, 0),
        seated((12, 10), 100_000, 0, 0),
    ];
    let aim = TileCoord::new(10, 10).to_world();
    let (_, outcome) = cast_once(&hero, flame, aim, &targets, &all_walkable(), 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));
}

#[test]
fn the_real_hellfire_and_inferno_strike_their_revived_caster_circles() {
    let atlas = real_atlas();
    let hero = caster("dark_wizard", 40, 400);
    let aim = hero.placement().position;

    // Hellfire: data range 0 (the dead-skill row), authored r=2 — an adjacent
    // 2-tile target is struck, a 3-tile one is not. No zero circle, no
    // NoTargetsInRegion.
    let hellfire = skill_number(&atlas, 10);
    assert_eq!(hellfire.range, 0, "the data range stays 0 (decoupled)");
    let targets = [
        seated((12, 10), 100_000, 0, 0),
        seated((13, 10), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, hellfire, aim, &targets, &all_walkable(), 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));

    // Inferno: data range 0, authored r=4 — three tiles in, five tiles out.
    let inferno = skill_number(&atlas, 14);
    assert_eq!(inferno.range, 0, "the data range stays 0 (decoupled)");
    let targets = [
        seated((13, 10), 100_000, 0, 0),
        seated((15, 10), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, inferno, aim, &targets, &all_walkable(), 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));
}

#[test]
fn the_real_triple_shot_cone_uses_the_exact_ratio_and_its_authored_length() {
    let atlas = real_atlas();
    let triple = skill_number(&atlas, 24);
    assert_eq!(
        triple.range, 6,
        "the cast range stays 6 under the length-7 cone"
    );
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;
    let targets = [
        // Six ahead, three off-axis: cos² = 0.8 ≥ 196/277 ≈ 0.708, dist √45 ≤ 7.
        seated((16, 13), 100_000, 0, 0),
        // Five ahead, four off-axis: cos² ≈ 0.61 — inside the old DEG_45 (0.5),
        // outside the exact ratio.
        seated((15, 14), 100_000, 0, 0),
        // Straight ahead at the authored length 7, PAST the cast range 6.
        seated((17, 10), 100_000, 0, 0),
        // Eight ahead: past the authored length.
        seated((18, 10), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, triple, aim, &targets, &all_walkable(), 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0, 2]));
}

#[test]
fn the_real_power_slash_cone_excludes_a_ninety_degree_flank() {
    let atlas = real_atlas();
    let slash = skill_number(&atlas, 56);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;
    // Straight ahead is in; 90° off the facing — covered by the shipped DEG_90
    // semicircle — is out under the authentic DEG_45.
    let targets = [
        seated((13, 10), 100_000, 0, 0),
        seated((10, 15), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, slash, aim, &targets, &all_walkable(), 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));
}

#[test]
fn the_real_fire_slash_and_beams_cover_their_authored_rectangles() {
    let atlas = real_atlas();
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;
    let grid = all_walkable();

    // Fire Slash: a short wide forward rect (length 2, half-width 2), not a
    // cone — one ahead two off-axis is in, three ahead on-axis is out.
    let fire_slash = skill_number(&atlas, 55);
    let targets = [
        seated((11, 12), 100_000, 0, 0),
        seated((13, 10), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, fire_slash, aim, &targets, &grid, 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));

    // Twister: length 4, half-width 1.5 — a 1-tile-off target the shipped
    // half-tile beam missed is covered; 2 tiles off is not.
    let twister = skill_number(&atlas, 8);
    let targets = [
        seated((13, 11), 100_000, 0, 0),
        seated((13, 12), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, twister, aim, &targets, &grid, 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));

    // Aqua Beam: length 8, half-width 1.5.
    let aqua = skill_number(&atlas, 12);
    let targets = [
        seated((17, 11), 100_000, 0, 0),
        seated((17, 12), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, aqua, aim, &targets, &grid, 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));

    // Penetration: length 8, half-width 1 — one off-axis in, two off-axis out.
    let penetration = skill_number(&atlas, 52);
    let targets = [
        seated((17, 11), 100_000, 0, 0),
        seated((17, 12), 100_000, 0, 0),
    ];
    let (_, outcome) = cast_once(&hero, penetration, aim, &targets, &grid, 1);
    assert_eq!(covered_indices(&outcome), BTreeSet::from([0]));
}

// --- The aim gate over real data. -----------------------------------------------

#[test]
fn a_real_aim_centered_cast_beyond_range_rejects_out_of_range_with_nothing_spent() {
    let atlas = real_atlas();
    let hero = caster("dark_wizard", 40, 400);
    // Flame (range 6) and Ice Storm (range 6): a ten-tile aim is refused before
    // any spend, even with a target seated on the aim itself.
    for number in [5u16, 39] {
        let skill = skill_number(&atlas, number);
        let targets = [seated((20, 10), 100_000, 0, 0)];
        let aim = TileCoord::new(20, 10).to_world();
        let (vitals, outcome) = cast_once(&hero, skill, aim, &targets, &all_walkable(), 1);
        assert_eq!(
            outcome,
            SkillOutcome::Rejected {
                reason: CastRejection::OutOfRange
            },
            "skill {number}"
        );
        assert_eq!(vitals, hero.vitals(), "skill {number}: nothing spent");
    }
}

#[test]
fn a_real_caster_anchored_cast_never_reads_the_aim() {
    let atlas = real_atlas();
    let hero = caster("dark_wizard", 40, 400);
    // Hellfire (caster circle r=2) and Nova (caster circle r=6): an absurd far
    // aim neither rejects OutOfRange nor moves the area — the covered target is
    // struck, and the whole outcome is byte-identical across two wildly
    // different aims under the same seed.
    for (number, tile) in [(10u16, (12u8, 10u8)), (40, (14, 10))] {
        let skill = skill_number(&atlas, number);
        let targets = [seated(tile, 100_000, 0, 0)];
        let run = |aim: WorldPos| cast_once(&hero, skill, aim, &targets, &all_walkable(), 11);
        let (_, absurd) = run(TileCoord::new(250, 3).to_world());
        assert_eq!(
            covered_indices(&absurd),
            BTreeSet::from([0]),
            "skill {number}: the far aim cannot reject a caster-anchored cast"
        );
        assert_eq!(
            or_abort(serde_json::to_string(&run(
                TileCoord::new(10, 10).to_world()
            ))),
            or_abort(serde_json::to_string(&run(
                TileCoord::new(250, 3).to_world()
            ))),
            "skill {number}: the outcome is invariant to the aim"
        );
    }
}

#[test]
fn a_real_in_range_empty_circle_rejects_no_targets_with_nothing_spent() {
    let atlas = real_atlas();
    let flame = skill_number(&atlas, 5);
    let hero = caster("dark_wizard", 40, 400);
    // In-range aim, but no target inside the r=1 circle — the standing
    // reject-before-spend OUR-pin, re-ratified this wave.
    let targets = [seated((30, 30), 100_000, 0, 0)];
    let aim = TileCoord::new(14, 10).to_world();
    let (vitals, outcome) = cast_once(&hero, flame, aim, &targets, &all_walkable(), 1);
    assert_eq!(
        outcome,
        SkillOutcome::Rejected {
            reason: CastRejection::NoTargetsInRegion
        }
    );
    assert_eq!(vitals, hero.vitals(), "nothing spent");
}

// --- Earthshake over real data. ---------------------------------------------------

#[test]
fn the_real_earthshake_pushes_struck_and_missed_monsters_three_tiles_away() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;
    let pushed_to = TileCoord::new(15, 10).to_world();
    // A deep-health target two tiles east, defense rate 10 so the sweep reaches
    // both the landed and the missed branch — both scatter to exactly (15,10).
    let targets = [seated((12, 10), 1_000_000, 10, 0)];
    let mut saw_landed = false;
    let mut saw_missed = false;
    for seed in 0u64..128 {
        let (_, outcome) = cast_once(&hero, quake, aim, &targets, &all_walkable(), seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake over a covered target resolves");
        };
        match hits[0] {
            TargetHit::Landed { displacement, .. } => {
                let moved = displacement.expect("a landed quake scatters over open ground");
                assert_eq!(moved.position, pushed_to, "seed {seed}");
                saw_landed = true;
            }
            TargetHit::Missed { displacement, .. } => {
                let moved = displacement.expect("a missed quake still scatters (G2)");
                assert_eq!(moved.position, pushed_to, "seed {seed}");
                saw_missed = true;
            }
            TargetHit::Killed { .. } => panic!("a million-HP target cannot be killed"),
        }
        if saw_landed && saw_missed {
            break;
        }
    }
    assert!(saw_landed && saw_missed, "both branches reached in 0..128");
}

#[test]
fn the_real_earthshake_never_pushes_a_killed_monster() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;
    // A frail 1-HP monster: any landing quake kills it, and Killed carries no
    // displacement at all — the victim stays on its tile.
    let targets = [seated((12, 10), 1, 0, 0)];
    let mut saw_kill = false;
    for seed in 0u64..64 {
        let (_, outcome) = cast_once(&hero, quake, aim, &targets, &all_walkable(), seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake over a covered target resolves");
        };
        match hits[0] {
            TargetHit::Killed { .. } => {
                saw_kill = true;
                break;
            }
            TargetHit::Missed { displacement, .. } => {
                assert!(displacement.is_some(), "a missed quake still scatters");
            }
            TargetHit::Landed { .. } => panic!("any landing hit kills the 1-HP victim"),
        }
    }
    assert!(saw_kill, "a landing strike kills the 1-HP victim in 0..64");
}

#[test]
fn the_real_earthshake_push_stops_at_a_wall_and_a_same_point_target_is_not_pushed() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    // Per-tile wall stop: (13,10) and (14,10) walkable, (15,10) blocked — the
    // two walkable steps are kept, the third refused. Missed and landed hits
    // scatter alike, so every seed proves the stop.
    let walled = grid_with(&[(12, 10), (13, 10), (14, 10)]);
    let targets = [seated((12, 10), 1_000_000, 0, 0)];
    for seed in 0u64..16 {
        let (_, outcome) = cast_once(&hero, quake, aim, &targets, &walled, seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake over a covered target resolves");
        };
        let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) =
            hits[0]
        else {
            panic!("a million-HP target cannot be killed");
        };
        let moved = displacement.expect("two walkable steps are kept");
        assert_eq!(
            moved.position,
            TileCoord::new(14, 10).to_world(),
            "seed {seed}"
        );
    }

    // Same-point target: attacker and target share (10,10), so the away-vector
    // has no direction — the victim is not displaced at all (no random
    // fallback heading), on the missed and landed branch alike.
    let shared = [seated((10, 10), 1_000_000, 0, 0)];
    for seed in 0u64..16 {
        let (_, outcome) = cast_once(&hero, quake, aim, &shared, &all_walkable(), seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake over a covered target resolves");
        };
        let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) =
            hits[0]
        else {
            panic!("a million-HP target cannot be killed");
        };
        assert_eq!(
            displacement, None,
            "seed {seed}: a same-point target is not pushed"
        );
    }
}

#[test]
fn the_real_earthshake_pushes_a_diagonal_target_along_the_true_line_not_an_octant_snap() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    // Caster (10,10), target (13,13): the away-vector is a true 45° diagonal. The
    // continuous sweep throws the victim three tiles *straight-line* along that
    // real line — ~2.12 tiles on each axis — not the ~4.24-tile (13,13)→(16,16)
    // a heading snapped to the nearest neighbour would have produced.
    let start = TileCoord::new(13, 13).to_world();
    let targets = [seated((13, 13), 1_000_000, 0, 0)];
    let (_, outcome) = cast_once(&hero, quake, aim, &targets, &all_walkable(), 0);
    let SkillOutcome::Cast { hits, .. } = outcome else {
        panic!("a funded quake over a covered target resolves");
    };
    let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) = hits[0]
    else {
        panic!("a million-HP target cannot be killed");
    };
    let moved = displacement.expect("a diagonal quake scatters over open ground");

    // The exact deterministic endpoint the swept knockback lands on.
    assert_eq!(moved.position, WorldPos::clamped(1_023_759, 1_023_759));

    // It advances ~2 tiles on BOTH axes (Chebyshev per-axis 2), never the
    // 3-and-0 an axis snap would give.
    assert_eq!(
        tile_delta(start, moved),
        (2, 2),
        "the diagonal push advances on both axes"
    );

    // Straight-line displacement is three tiles, not the diagonal octant's ~4.24.
    let three_tiles = 3 * TILE.unsigned_abs();
    let straight = start.distance_sq(moved.position).isqrt();
    assert!(
        straight.abs_diff(three_tiles) < TILE.unsigned_abs() / 8,
        "straight-line push is ~3 tiles, got {straight} sub-units (3 tiles = {three_tiles})"
    );
}

#[test]
fn the_real_earthshake_pushes_all_four_diagonals_along_the_true_line() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    // Caster (10,10). Each quadrant's target sits three tiles away on a true 45°
    // diagonal; the continuous sweep throws the victim ~3 tiles straight-line
    // along the real away-vector — ~2.12 tiles on EACH axis, with the sign of the
    // away-vector on each. A sign error in the direction math lands the victim in
    // the wrong quadrant; an axis snap zeroes one axis (a 3-and-0 hop).
    // `(target, expected tile-delta, exact endpoint)`.
    let cases = [
        ((13u8, 13u8), (2i64, 2i64), (1_023_759i64, 1_023_759i64)), // +x +y
        ((7, 7), (-2, -2), (352_497, 352_497)),                     // −x −y
        ((13, 7), (2, -2), (1_023_759, 352_497)),                   // +x −y
        ((7, 13), (-2, 2), (352_497, 1_023_759)),                   // −x +y
    ];
    for ((tx, ty), delta, (ex, ey)) in cases {
        let start = TileCoord::new(tx, ty).to_world();
        let targets = [seated((tx, ty), 1_000_000, 0, 0)];
        let (_, outcome) = cast_once(&hero, quake, aim, &targets, &all_walkable(), 0);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake over a covered target resolves");
        };
        let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) =
            hits[0]
        else {
            panic!("a million-HP target cannot be killed");
        };
        let moved = displacement.expect("a diagonal quake scatters over open ground");

        // The exact deterministic endpoint of this quadrant's swept knockback.
        assert_eq!(
            moved.position,
            WorldPos::clamped(ex, ey),
            "target ({tx},{ty}): endpoint"
        );

        // Per-axis Chebyshev ~2 WITH the away-vector's sign on each axis — the
        // whole (sign, magnitude) pair, so a flipped sign or a snapped axis fails.
        assert_eq!(tile_delta(start, moved), delta, "target ({tx},{ty}): delta");

        // |dx| == |dy|: a true 45° line keeps the axes equal to the sub-unit — an
        // octant/axis snap would zero one axis or split them unevenly.
        let raw = (
            moved.position.x().raw() - start.x().raw(),
            moved.position.y().raw() - start.y().raw(),
        );
        assert_eq!(
            raw.0.abs(),
            raw.1.abs(),
            "target ({tx},{ty}): equal advance on both axes"
        );

        // Straight-line displacement is three tiles, not the ~4.24 a whole-tile
        // diagonal neighbour hop (13,13)->(16,16) would give.
        let three_tiles = 3 * TILE.unsigned_abs();
        let straight = start.distance_sq(moved.position).isqrt();
        assert!(
            straight.abs_diff(three_tiles) < TILE.unsigned_abs() / 8,
            "target ({tx},{ty}): straight-line ~3 tiles, got {straight} sub-units"
        );
    }
}

#[test]
fn the_real_earthshake_pushes_a_two_to_one_slope_at_its_true_angle() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    // Caster (10,10), target (14,12): the away-vector is (4,2) — a 2:1 slope at
    // ~26.57°, a heading NO 8-way (45° grid) NOR 16-way (22.5° grid) snap can
    // represent (the nearest 16-way headings, 22.5° and 45°, give 2.414:1 and
    // 1:1). The continuous sweep throws the victim three tiles straight along
    // THAT real line: it advances ~2× as far on x as on y. This is the test that
    // proves TRUE continuity — it cannot pass under any fixed-direction snap.
    let start = TileCoord::new(14, 12).to_world();
    let targets = [seated((14, 12), 1_000_000, 0, 0)];
    let (_, outcome) = cast_once(&hero, quake, aim, &targets, &all_walkable(), 0);
    let SkillOutcome::Cast { hits, .. } = outcome else {
        panic!("a funded quake over a covered target resolves");
    };
    let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) = hits[0]
    else {
        panic!("a million-HP target cannot be killed");
    };
    let moved = displacement.expect("a 2:1-slope quake scatters over open ground");

    // The exact deterministic endpoint of the swept knockback along the 2:1 line.
    assert_eq!(moved.position, WorldPos::clamped(1_126_123, 907_127));

    let dx = moved.position.x().raw() - start.x().raw();
    let dy = moved.position.y().raw() - start.y().raw();
    // Both axes advance in the away-vector's (+,+) quadrant.
    assert!(
        dx > 0 && dy > 0,
        "advances in the (+,+) quadrant: dx={dx} dy={dy}"
    );
    // The x advance is ~twice the y advance — the defining 2:1 continuity proof.
    // A 22.5° snap would give 2.414:1 (dx≈2.414·dy), a 45° snap 1:1, a 0° snap
    // dy==0; each falls far outside this eighth-tile band around exactly 2:1.
    assert!(
        dx.abs_diff(2 * dy) < TILE.unsigned_abs() / 8,
        "x advance is ~2× y (2:1 slope): dx={dx} dy={dy}"
    );
    // dy is a real advance (excludes the 0° axis snap) and dx ≠ dy (excludes 45°).
    assert!(
        dy > TILE / 4,
        "y genuinely advances (not an axis snap): dy={dy}"
    );
    assert_ne!(dx, dy, "the axes differ (not a 45° diagonal snap)");

    // Straight-line displacement is three tiles.
    let three_tiles = 3 * TILE.unsigned_abs();
    let straight = start.distance_sq(moved.position).isqrt();
    assert!(
        straight.abs_diff(three_tiles) < TILE.unsigned_abs() / 8,
        "straight-line push is ~3 tiles, got {straight} sub-units"
    );
}

#[test]
fn the_real_earthshake_diagonal_push_stops_at_a_wall_keeping_gained_ground() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    // Caster (10,10), target (13,13): on open ground the 45° sweep lands the
    // victim in tile (15,15) at (1_023_759,1_023_759). A wall on that final tile
    // refuses the last increment like any block; the two earlier increments are
    // kept, so the victim stops in tile (14,14) — ground gained, wall never
    // crossed (the diagonal mirror of the axis-aligned wall-stop above).
    let start = TileCoord::new(13, 13).to_world();
    let open_ground_endpoint = WorldPos::clamped(1_023_759, 1_023_759);
    let walled = all_walkable_except(&[(15, 15)]);
    let targets = [seated((13, 13), 1_000_000, 0, 0)];
    for seed in 0u64..16 {
        let (_, outcome) = cast_once(&hero, quake, aim, &targets, &walled, seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake over a covered target resolves");
        };
        let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) =
            hits[0]
        else {
            panic!("a million-HP target cannot be killed");
        };
        let moved = displacement.expect("the earlier diagonal increments are kept");

        // The exact deterministic endpoint the walled sweep stops on.
        assert_eq!(
            moved.position,
            WorldPos::clamped(977_418, 977_418),
            "seed {seed}"
        );
        // Ground was gained (past the start tile) but the wall was never entered:
        // the victim rests in tile (14,14), one tile short of the open-ground land.
        assert_eq!(
            TileCoord::from_world(moved.position),
            TileCoord::new(14, 14),
            "seed {seed}: stops on the last walkable diagonal tile"
        );
        assert_ne!(
            moved.position, open_ground_endpoint,
            "seed {seed}: the wall shortened the push"
        );
        assert_ne!(
            moved.position, start,
            "seed {seed}: ground was still gained"
        );
    }
}

#[test]
fn the_real_earthshake_diagonal_push_refuses_a_safezone_tile_exactly_like_a_wall() {
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    // Same 45° push as the wall-stop, but the final tile (15,15) is a SAFE
    // town-truce tile rather than a wall. A safe destination refuses the step
    // just as a wall does — so the victim stops on the identical tile with the
    // identical endpoint, and is never shoved into the safezone.
    let safe_grid = all_walkable_with_safe(&[(15, 15)]);
    let wall_grid = all_walkable_except(&[(15, 15)]);
    let safe_tile = TileCoord::new(15, 15).to_world();
    assert!(safe_grid.safe(safe_tile), "the (15,15) tile is safe");
    assert!(safe_grid.walkable(safe_tile), "a safe tile is walkable");
    assert!(
        !wall_grid.walkable(safe_tile),
        "the wall tile is not walkable"
    );

    let targets = [seated((13, 13), 1_000_000, 0, 0)];
    for seed in 0u64..16 {
        let displaced = |grid: &TerrainGrid| {
            let (_, outcome) = cast_once(&hero, quake, aim, &targets, grid, seed);
            let SkillOutcome::Cast { hits, .. } = outcome else {
                panic!("a funded quake over a covered target resolves");
            };
            let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) =
                hits[0]
            else {
                panic!("a million-HP target cannot be killed");
            };
            displacement.expect("the earlier diagonal increments are kept")
        };
        let over_safe = displaced(&safe_grid);
        let over_wall = displaced(&wall_grid);

        // The exact endpoint, identical to the wall-stop — a safe tile refuses the
        // sweep at the same point a wall does.
        assert_eq!(
            over_safe.position,
            WorldPos::clamped(977_418, 977_418),
            "seed {seed}"
        );
        assert_eq!(
            over_safe.position, over_wall.position,
            "seed {seed}: the safezone stops the push exactly like a wall"
        );
        assert!(
            !safe_grid.safe(over_safe.position),
            "seed {seed}: the victim never lands on the safe tile"
        );
    }
}

// --- Lunge over real data. --------------------------------------------------------

/// The first lunge-shaped skill on the roster — found by pattern, never a
/// hard-coded index.
fn lunge_skill(atlas: &Atlas) -> &Skill {
    or_abort(
        atlas
            .skills()
            .find(|skill| {
                matches!(
                    route(skill),
                    SkillRouting::Damaging(reference)
                        if matches!(reference.shape(), DamagingSkill::Lunge)
                )
            })
            .ok_or("the roster has a lunge skill"),
    )
}

#[test]
fn a_real_lunge_teleports_the_caster_onto_its_target_across_a_wall() {
    let atlas = real_atlas();
    let lunge = lunge_skill(&atlas);
    assert!(
        lunge.range >= 3,
        "the first lunge reaches across a two-tile wall"
    );
    let hero = caster("dark_knight", 200, 30);
    // (11,10) and (12,10) are UNWALKABLE between the caster and the target —
    // the classic teleport crosses them regardless.
    let walled = grid_with(&[(10, 10), (13, 10)]);
    let targets = [seated((13, 10), 1_000_000, 0, 0)];
    let aim = TileCoord::new(13, 10).to_world();
    for seed in 0u64..8 {
        let (_, outcome) = cast_once(&hero, lunge, aim, &targets, &walled, seed);
        let SkillOutcome::Cast {
            caster_placement, ..
        } = outcome
        else {
            panic!("a funded lunge over a covered target resolves");
        };
        assert_eq!(
            caster_placement.position, aim,
            "seed {seed}: the caster lands on the target's exact cell"
        );
    }
}

#[test]
fn a_real_missed_lunge_still_nudges_its_victim_and_a_killing_lunge_does_not() {
    let atlas = real_atlas();
    let lunge = lunge_skill(&atlas);
    let hero = caster("dark_knight", 200, 30);
    let start = TileCoord::new(12, 10).to_world();
    let aim = start;

    // Missed-but-alive: the MovesTarget jiggle fires pre-roll, so a whiffed
    // lunge still nudges the victim within ±1 per axis — and the caster
    // teleports regardless of the miss.
    let sturdy = [seated((12, 10), 1_000_000, 10, 0)];
    let mut saw_missed_nudge = false;
    for seed in 0u64..128 {
        let (_, outcome) = cast_once(&hero, lunge, aim, &sturdy, &all_walkable(), seed);
        let SkillOutcome::Cast {
            caster_placement,
            hits,
        } = outcome
        else {
            panic!("a funded lunge over a covered target resolves");
        };
        assert_eq!(
            caster_placement.position, start,
            "the teleport always fires"
        );
        if let TargetHit::Missed {
            displacement: Some(moved),
            ..
        } = hits[0]
        {
            let (dx, dy) = tile_delta(start, moved);
            assert!(dx.abs() <= 1 && dy.abs() <= 1, "seed {seed}: a ±1 nudge");
            assert_ne!(moved.position, start, "a reported nudge is a net move");
            saw_missed_nudge = true;
            break;
        }
    }
    assert!(saw_missed_nudge, "a seed in 0..128 whiffs and still nudges");

    // Killed: no nudge (Killed carries no displacement), teleport still fires.
    let frail = [seated((12, 10), 1, 0, 0)];
    let mut saw_kill = false;
    for seed in 0u64..64 {
        let (_, outcome) = cast_once(&hero, lunge, aim, &frail, &all_walkable(), seed);
        let SkillOutcome::Cast {
            caster_placement,
            hits,
        } = outcome
        else {
            panic!("a funded lunge over a covered target resolves");
        };
        assert_eq!(
            caster_placement.position, start,
            "the teleport always fires"
        );
        if matches!(hits[0], TargetHit::Killed { .. }) {
            saw_kill = true;
            break;
        }
    }
    assert!(saw_kill, "a landing lunge kills the 1-HP victim in 0..64");
}

// --- The lightning jiggle over real data. ------------------------------------------

/// The first lightning-element direct-hit skill on the roster — found by
/// pattern, never a hard-coded index.
fn lightning_direct_skill(atlas: &Atlas) -> &Skill {
    or_abort(
        atlas
            .skills()
            .find(|skill| {
                skill.element == Some(mu_core::components::element::Element::Lightning)
                    && matches!(
                        route(skill),
                        SkillRouting::Damaging(reference)
                            if matches!(reference.shape(), DamagingSkill::DirectHit)
                    )
            })
            .ok_or("the roster has a lightning direct-hit skill"),
    )
}

#[test]
fn a_real_lightning_skill_jiggles_a_landed_target_and_never_a_missed_one() {
    let atlas = real_atlas();
    let bolt = lightning_direct_skill(&atlas);
    let hero = caster("dark_wizard", 40, 400);
    let start = TileCoord::new(11, 10).to_world();
    let targets = [seated((11, 10), 1_000_000, 10, 0)];
    let aim = start;
    let tile = UNITS_PER_TILE;
    let mut saw_move = false;
    let mut directions = BTreeSet::new();
    for seed in 0u64..128 {
        let (_, outcome) = cast_once(&hero, bolt, aim, &targets, &all_walkable(), seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded bolt over a covered target resolves");
        };
        match hits[0] {
            TargetHit::Landed {
                displacement: Some(moved),
                ..
            } => {
                let dx = moved.position.x().raw() - start.x().raw();
                let dy = moved.position.y().raw() - start.y().raw();
                assert!(
                    dx.abs() <= tile && dy.abs() <= tile,
                    "seed {seed}: within one tile per axis"
                );
                assert_ne!(moved.position, start);
                saw_move = true;
                directions.insert((dx.signum(), dy.signum()));
            }
            // The continuous jiggle always nudges on open ground — a landed
            // applied hit never reports a stay.
            TargetHit::Landed {
                displacement: None, ..
            } => {}
            // The elemental jiggle is landed-only: a miss never displaces.
            TargetHit::Missed { displacement, .. } => assert_eq!(displacement, None),
            TargetHit::Killed { .. } => panic!("a million-HP target cannot be killed"),
        }
    }
    assert!(
        saw_move,
        "a seed in 0..128 lands an applied jiggle that moves"
    );
    assert!(
        directions.len() > 2,
        "the continuous nudge reaches a spread of directions: {directions:?}"
    );
}

#[test]
fn a_blocked_jiggle_destination_keeps_the_target_in_place_over_real_data() {
    let atlas = real_atlas();
    let bolt = lightning_direct_skill(&atlas);
    let hero = caster("dark_wizard", 40, 400);
    let start = TileCoord::new(11, 10).to_world();
    let targets = [seated((11, 10), 1_000_000, 0, 0)];
    let aim = start;
    // Only the target's own tile is walkable: the same seed whose open-ground
    // jiggle moved the target now reports no move — blocked = stay, no re-roll
    // (the identical two words are consumed, so the branch stays aligned).
    let sealed = grid_with(&[(11, 10)]);
    let mut compared = false;
    for seed in 0u64..64 {
        let (_, open) = cast_once(&hero, bolt, aim, &targets, &all_walkable(), seed);
        let SkillOutcome::Cast { hits, .. } = open else {
            panic!("a funded bolt over a covered target resolves");
        };
        if !matches!(
            hits[0],
            TargetHit::Landed {
                displacement: Some(_),
                ..
            }
        ) {
            continue;
        }
        let (_, walled) = cast_once(&hero, bolt, aim, &targets, &sealed, seed);
        let SkillOutcome::Cast { hits, .. } = walled else {
            panic!("a funded bolt over a covered target resolves");
        };
        assert!(
            matches!(
                hits[0],
                TargetHit::Landed {
                    displacement: None,
                    ..
                }
            ),
            "seed {seed}: the sealed grid turns the same jiggle into a stay"
        );
        compared = true;
        break;
    }
    assert!(
        compared,
        "a seed in 0..64 lands a moving jiggle on open ground"
    );
}

#[test]
fn the_displacement_is_dispatched_per_skill_over_real_data() {
    // Earthshake -> the directional 3-tile push; a lunge -> the ±1 jiggle plus
    // the caster teleport; lightning (direct hit AND Cometfall's aim circle) ->
    // the ±1 jiggle. The shared lightning tag decides nothing: Earthshake is
    // lightning-tagged yet pushes.
    let atlas = real_atlas();
    let hero = caster("dark_knight", 200, 30);
    let wizard = caster("dark_wizard", 40, 400);

    // Earthshake: Chebyshev 3, never 1.
    let quake = skill_number(&atlas, 62);
    assert_eq!(
        quake.element,
        Some(mu_core::components::element::Element::Lightning),
        "the authentic lightning tag rides the record, inert for dispatch"
    );
    // Missed and landed hits push alike, so every seed proves the dispatch.
    let start = TileCoord::new(12, 10).to_world();
    let targets = [seated((12, 10), 1_000_000, 0, 0)];
    for seed in 0u64..16 {
        let (_, outcome) = cast_once(
            &hero,
            quake,
            hero.placement().position,
            &targets,
            &all_walkable(),
            seed,
        );
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded quake resolves");
        };
        let (TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. }) =
            hits[0]
        else {
            panic!("a million-HP target cannot be killed");
        };
        let moved = or_abort(displacement.ok_or("open ground: the push moves"));
        let (dx, dy) = tile_delta(start, moved);
        assert_eq!(dx.abs().max(dy.abs()), 3, "seed {seed}: the quake pushes 3");
    }

    // Cometfall (a lightning-element aim circle with displacement "none"):
    // its landed applied hit jiggles within ±1 — never a 3-tile push.
    let cometfall = skill_number(&atlas, 13);
    let start = TileCoord::new(11, 10).to_world();
    let targets = [seated((11, 10), 1_000_000, 0, 0)];
    let mut comet_jiggled = false;
    for seed in 0u64..64 {
        let (_, outcome) = cast_once(&wizard, cometfall, start, &targets, &all_walkable(), seed);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded cometfall resolves");
        };
        if let TargetHit::Landed {
            displacement: Some(moved),
            ..
        } = hits[0]
        {
            let (dx, dy) = tile_delta(start, moved);
            assert!(dx.abs() <= 1 && dy.abs() <= 1, "seed {seed}: a ±1 jiggle");
            comet_jiggled = true;
            break;
        }
    }
    assert!(
        comet_jiggled,
        "a seed in 0..64 lands an applied cometfall jiggle"
    );
}

// --- Determinism & the RNG draw contract over real data. ----------------------------

/// An RNG that counts the words it hands out.
struct CountingRng {
    inner: TestRng,
    words: u32,
}

impl CountingRng {
    fn new(seed: u64) -> Self {
        Self {
            inner: TestRng::new(seed),
            words: 0,
        }
    }
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

/// The kind tag of the single hit of a resolved cast — the branch key the
/// draw-count comparisons align on.
fn hit_kind(outcome: &SkillOutcome) -> &'static str {
    match outcome {
        SkillOutcome::Cast { hits, .. } => match hits.first() {
            Some(TargetHit::Missed { .. }) => "missed",
            Some(TargetHit::Landed { .. }) => "landed",
            Some(TargetHit::Killed { .. }) => "killed",
            None => "none",
        },
        SkillOutcome::Rejected { .. } => "rejected",
    }
}

/// One counted cast: the words drawn and the outcome.
fn counted_cast(
    hero: &Character,
    skill: &Skill,
    aim: WorldPos,
    targets: &[CombatTarget],
    seed: u64,
) -> (u32, SkillOutcome) {
    let mut rng = CountingRng::new(seed);
    let (_, outcome) = cast(
        hero,
        &character_profile(hero).0,
        damaging_ref(skill).locate(aim),
        targets,
        &all_walkable(),
        &mut rng,
    );
    (rng.words, outcome)
}

#[test]
fn identical_inputs_and_seeds_replay_byte_identical_including_displacements() {
    let atlas = real_atlas();
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;
    for skill in [skill_number(&atlas, 62), lunge_skill(&atlas)] {
        let tile = if skill.range == 0 { (10, 10) } else { (12, 10) };
        let targets = [seated(tile, 1_000_000, 0, 0)];
        let target_aim = if matches!(skill.shape, SkillShape::Lunge) {
            TileCoord::new(tile.0, tile.1).to_world()
        } else {
            aim
        };
        let run = |seed: u64| {
            let (vitals, outcome) =
                cast_once(&hero, skill, target_aim, &targets, &all_walkable(), seed);
            (
                or_abort(serde_json::to_string(&vitals)),
                or_abort(serde_json::to_string(&outcome)),
            )
        };
        assert_eq!(run(41), run(41), "skill {:?}", skill.number);
    }
}

#[test]
fn earthshake_draws_no_element_roll_and_no_push_word() {
    // Differential word-count pin over the real roster: Evil Spirit (record 9)
    // is a non-elemental caster circle with no displacement, so its post-strike
    // draw count is zero. On the same seed (same profiles, so the identical
    // strike branch) the quake's count equals it exactly — Earthshake's inert
    // lightning tag draws no element roll, and the swept push draws no word at
    // any range, a same-point target included (DET-2).
    let atlas = real_atlas();
    let quake = skill_number(&atlas, 62);
    let evil_spirit = skill_number(&atlas, 9);
    let hero = caster("dark_knight", 200, 30);
    let aim = hero.placement().position;

    let apart = [seated((12, 10), 1_000_000, 0, 0)];
    let mut compared = false;
    for seed in 0u64..16 {
        let (quake_words, quake_outcome) = counted_cast(&hero, quake, aim, &apart, seed);
        let (spirit_words, spirit_outcome) = counted_cast(&hero, evil_spirit, aim, &apart, seed);
        assert_eq!(
            hit_kind(&quake_outcome),
            hit_kind(&spirit_outcome),
            "seed {seed}"
        );
        assert_eq!(
            quake_words, spirit_words,
            "seed {seed}: a different-tile quake draws nothing beyond the strike"
        );
        compared = true;
    }
    assert!(compared);

    let shared = [seated((10, 10), 1_000_000, 0, 0)];
    for seed in 0u64..16 {
        let (quake_words, quake_outcome) = counted_cast(&hero, quake, aim, &shared, seed);
        let (spirit_words, spirit_outcome) = counted_cast(&hero, evil_spirit, aim, &shared, seed);
        assert_eq!(
            hit_kind(&quake_outcome),
            hit_kind(&spirit_outcome),
            "seed {seed}"
        );
        assert_eq!(
            quake_words, spirit_words,
            "seed {seed}: a same-point target draws no push word either"
        );
    }
}

#[test]
fn the_lunge_jiggle_draws_a_variable_surplus_on_missed_and_landed_hits() {
    // The first non-elemental direct hit is the lunge's draw-count twin: same
    // strike sequence per branch, no element roll on either, and no
    // displacement on the direct hit — so the lunge's surplus over it IS the
    // jiggle's continuous heading draw (a variable, deterministic-per-seed count
    // of at least two words), present on the missed AND the landed branch alike
    // (DET-1/3).
    let atlas = real_atlas();
    let lunge = lunge_skill(&atlas);
    let direct = or_abort(
        atlas
            .skills()
            .find(|skill| {
                skill.element.is_none()
                    && matches!(
                        route(skill),
                        SkillRouting::Damaging(reference)
                            if matches!(reference.shape(), DamagingSkill::DirectHit)
                    )
            })
            .ok_or("the roster has a non-elemental direct-hit skill"),
    );
    let hero = caster("dark_knight", 200, 30);
    let targets = [seated((11, 10), 1_000_000, 10, 0)];
    let aim = TileCoord::new(11, 10).to_world();
    let mut saw = BTreeSet::new();
    for seed in 0u64..128 {
        let (lunge_words, lunge_outcome) = counted_cast(&hero, lunge, aim, &targets, seed);
        let (direct_words, direct_outcome) = counted_cast(&hero, direct, aim, &targets, seed);
        let kind = hit_kind(&lunge_outcome);
        assert_eq!(kind, hit_kind(&direct_outcome), "seed {seed}");
        assert!(
            lunge_words >= direct_words + 2,
            "seed {seed} ({kind}): the jiggle draws at least the two-axis heading surplus"
        );
        // Deterministic per seed: a re-run consumes the identical word count.
        let (lunge_words_again, _) = counted_cast(&hero, lunge, aim, &targets, seed);
        assert_eq!(
            lunge_words, lunge_words_again,
            "seed {seed}: the jiggle's word count is deterministic per seed"
        );
        saw.insert(kind);
    }
    assert!(
        saw.contains("missed") && saw.contains("landed"),
        "the sweep reaches both branches: {saw:?}"
    );
}

#[test]
fn the_lightning_jiggle_draws_element_then_a_variable_surplus_and_immunity_draws_none() {
    // Against the same non-elemental direct-hit twin: a landed lightning hit on
    // a zero-resist target draws the element-application roll (one word) then
    // the jiggle's continuous heading (at least two, variable) — a surplus of at
    // least three; a missed one draws nothing extra (no roll on a miss); an
    // immune (resist 255) target short-circuits the roll, so a landed hit draws
    // nothing extra either (JIG-5 / the §5 table).
    let atlas = real_atlas();
    let bolt = lightning_direct_skill(&atlas);
    let direct = or_abort(
        atlas
            .skills()
            .find(|skill| {
                skill.element.is_none()
                    && matches!(
                        route(skill),
                        SkillRouting::Damaging(reference)
                            if matches!(reference.shape(), DamagingSkill::DirectHit)
                    )
            })
            .ok_or("the roster has a non-elemental direct-hit skill"),
    );
    let hero = caster("dark_wizard", 40, 400);
    let aim = TileCoord::new(11, 10).to_world();

    let vulnerable = [seated((11, 10), 1_000_000, 10, 0)];
    let mut saw = BTreeSet::new();
    for seed in 0u64..128 {
        let (bolt_words, bolt_outcome) = counted_cast(&hero, bolt, aim, &vulnerable, seed);
        let (direct_words, direct_outcome) = counted_cast(&hero, direct, aim, &vulnerable, seed);
        let kind = hit_kind(&bolt_outcome);
        assert_eq!(kind, hit_kind(&direct_outcome), "seed {seed}");
        match kind {
            // Zero resistance always applies: 1 element word then the heading.
            "landed" => assert!(
                bolt_words >= direct_words + 3,
                "seed {seed} (landed): element roll then the heading surplus"
            ),
            // A miss rolls no element and no jiggle.
            "missed" => assert_eq!(
                bolt_words, direct_words,
                "seed {seed} (missed): no roll, no jiggle"
            ),
            other => panic!("unexpected branch {other}"),
        }
        saw.insert(kind);
    }
    assert!(
        saw.contains("missed") && saw.contains("landed"),
        "the sweep reaches both branches: {saw:?}"
    );

    let immune = [seated((11, 10), 1_000_000, 0, 255)];
    let mut compared = false;
    for seed in 0u64..16 {
        let (bolt_words, bolt_outcome) = counted_cast(&hero, bolt, aim, &immune, seed);
        let (direct_words, direct_outcome) = counted_cast(&hero, direct, aim, &immune, seed);
        let kind = hit_kind(&bolt_outcome);
        assert_eq!(kind, hit_kind(&direct_outcome), "seed {seed}");
        if kind == "landed" {
            assert_eq!(
                bolt_words, direct_words,
                "seed {seed}: immunity short-circuits — no application word, no jiggle"
            );
            // An immune landed hit neither inflicts nor displaces.
            let SkillOutcome::Cast { hits, .. } = bolt_outcome else {
                panic!("a resolved cast");
            };
            assert!(matches!(
                hits[0],
                TargetHit::Landed {
                    displacement: None,
                    inflicted: None,
                    ..
                }
            ));
            compared = true;
        }
    }
    assert!(compared, "a seed in 0..16 lands on the immune target");
}
