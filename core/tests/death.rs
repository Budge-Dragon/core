//! The monster-kill death lifecycle (W-DEATH) over the real `/data` Atlas: the
//! core [`resolve_death`] and [`respawn`] services applied directly against the
//! shipped experience curve, class table, maps, and spawn gates. Proves the exp
//! band-dock with its no-de-level floor, the sub-10 and max-level exemptions, the
//! 1/2/3% zen brackets, the integer-floor tiny balance, Dead-marking with vitals
//! and effects untouched, idempotence, gate selection (own map / Lorencia
//! fallback / first-of-many), the walkable landing, the three-vital refill, the
//! effect clear, determinism, and the full die -> penalty -> respawn -> persist
//! loop.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body so
//! `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use serde_json::{Value, json};

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::life::LifeState;
use mu_core::components::movement::Movement;
use mu_core::components::spatial::{Facing, WorldPos};
use mu_core::components::tile::TileArea;
use mu_core::components::units::{CarriedZen, Exp, MapNumber, Tick, TickDuration, Zen};
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::events::death::{DeathEvent, Respawned};
use mu_core::services::death::{resolve_death, respawn};
use mu_core::services::profile::character_profile;

use dataset::{or_abort, real_atlas};
use rng::TestRng;

/// The suite tick base: 50 ms, so the 3000 ms respawn delay is 60 whole ticks.
fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// The respawn delay in whole ticks against [`tick`]: `ceil(3000 / 50)`.
const RESPAWN_DELAY_TICKS: u64 = 60;

/// The heading every real respawn produces — every real gate is direction-less,
/// so this is the death service's `DEFAULT_FACING` pin.
const RESPAWN_FACING: Facing = Facing::POS_Y;

/// Total experience the curve requires to hold `lvl`, read from the real table.
fn total(atlas: &Atlas, lvl: u16) -> u64 {
    or_abort(atlas.exp_curve().level(lvl)).total_to_hold().0
}

/// A gearless Dark Knight built the only way a character can be — by
/// deserialising its wire form (the class↔stats gate re-proves on load) — with
/// the level, experience, carried zen, map, vitals, active effects, and life
/// each test seeds.
fn dark_knight(
    lvl: u16,
    exp: u64,
    zen: u64,
    map: u8,
    vitals: Value,
    effects: Value,
    life: Value,
) -> Character {
    let mut json = json!({
        "class": "dark_knight",
        "level": lvl,
        "experience": exp,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 7,
        "zen": zen,
        "placement": {
            "position": {"x": 0, "y": 0},
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": map,
        },
    });
    // Move the seeded pieces in so they are consumed, not borrowed by the macro.
    let object = or_abort(json.as_object_mut().ok_or("dark_knight json is an object"));
    object.insert("vitals".to_owned(), vitals);
    object.insert("active_effects".to_owned(), effects);
    object.insert("life".to_owned(), life);
    or_abort(serde_json::from_value(json))
}

fn alive() -> Value {
    json!({"kind": "alive"})
}

fn dead(respawn_at: u64) -> Value {
    json!({"kind": "dead", "respawn_at": respawn_at})
}

fn full_vitals(health: u32, mana: u32, ability: u32) -> Value {
    json!({
        "health": {"current": health, "max": health},
        "mana": {"current": mana, "max": mana},
        "ability": {"current": ability, "max": ability},
    })
}

/// Three depleted pools: current zero, the maxima intact so the refill shows.
fn zero_vitals(max_health: u32, max_mana: u32, max_ability: u32) -> Value {
    json!({
        "health": {"current": 0, "max": max_health},
        "mana": {"current": 0, "max": max_mana},
        "ability": {"current": 0, "max": max_ability},
    })
}

fn no_effects() -> Value {
    json!([])
}

/// A single active poison — the effect that must survive the dead beat and clear
/// only on respawn.
fn a_poison() -> Value {
    json!([{"kind": "poisoned", "per_tick_damage": 12, "remaining": 6, "next_tick": 60, "cadence": 60}])
}

/// The world rectangle a gate's tile area projects to.
fn gate_rect(x1: u8, y1: u8, x2: u8, y2: u8) -> mu_core::components::spatial::WorldRect {
    or_abort(TileArea::new(x1, y1, x2, y2)).to_world()
}

/// Asserts a respawn landing sits on a walkable tile inside the given gate area.
fn assert_landed_inside(
    atlas: &Atlas,
    map: u8,
    pos: WorldPos,
    rect: mu_core::components::spatial::WorldRect,
) {
    let grid = or_abort(atlas.walk_grid(MapNumber(map)).ok_or("map has a walk grid"));
    assert!(rect.contains(pos), "landing sits inside the gate area");
    assert!(grid.walkable(pos), "landing sits on a walkable tile");
}

#[test]
fn a_mid_band_level_100_kill_docks_one_percent_of_the_band_and_never_de_levels() {
    let atlas = real_atlas();
    let t100 = total(&atlas, 100);
    let t101 = total(&atlas, 101);
    let band = t101 - t100;
    let expected_loss = band / 100;
    // Well inside the band, comfortably above the nominal loss.
    let exp = t100 + band / 2;

    let hero = dark_knight(
        100,
        exp,
        0,
        3,
        full_vitals(500, 100, 60),
        no_effects(),
        alive(),
    );
    let (dead_hero, events) = resolve_death(&hero, Tick(500), tick(), &atlas);

    assert_eq!(dead_hero.level().get(), 100, "the floor never de-levels");
    assert_eq!(dead_hero.experience(), Exp(exp - expected_loss));
    assert_eq!(
        dead_hero.life(),
        LifeState::Dead {
            respawn_at: Tick(500 + RESPAWN_DELAY_TICKS)
        }
    );
    assert_eq!(
        events,
        vec![
            DeathEvent::Died {
                respawn_at: Tick(500 + RESPAWN_DELAY_TICKS)
            },
            DeathEvent::ExperienceDocked {
                lost: Exp(expected_loss),
                remaining: Exp(exp - expected_loss),
            },
        ]
    );
}

#[test]
fn a_death_a_sliver_past_the_level_threshold_floors_at_the_level_start() {
    let atlas = real_atlas();
    let t100 = total(&atlas, 100);
    let sliver = 100;
    let exp = t100 + sliver;
    // The nominal 1% loss dwarfs the sliver of progress, so the floor bites.
    let band = total(&atlas, 101) - t100;
    assert!(band / 100 > sliver, "the nominal loss exceeds the sliver");

    let hero = dark_knight(
        100,
        exp,
        0,
        3,
        full_vitals(500, 100, 60),
        no_effects(),
        alive(),
    );
    let (dead_hero, events) = resolve_death(&hero, Tick(0), tick(), &atlas);

    assert_eq!(dead_hero.level().get(), 100, "no de-level");
    assert_eq!(
        dead_hero.experience(),
        Exp(t100),
        "floored at the level start, never below"
    );
    // The reported loss is the applied sliver, not the larger nominal.
    assert_eq!(
        events,
        vec![
            DeathEvent::Died {
                respawn_at: Tick(RESPAWN_DELAY_TICKS)
            },
            DeathEvent::ExperienceDocked {
                lost: Exp(sliver),
                remaining: Exp(t100),
            },
        ]
    );
}

#[test]
fn a_death_below_level_ten_is_free_of_experience_and_zen() {
    let atlas = real_atlas();
    let exp = total(&atlas, 6) + 500;
    let hero = dark_knight(
        6,
        exp,
        250_000,
        3,
        full_vitals(200, 60, 30),
        no_effects(),
        alive(),
    );

    let (dead_hero, events) = resolve_death(&hero, Tick(40), tick(), &atlas);

    assert_eq!(dead_hero.experience(), Exp(exp), "experience untouched");
    assert_eq!(
        dead_hero.zen(),
        or_abort(CarriedZen::new(250_000)),
        "carried zen untouched"
    );
    assert_eq!(
        events,
        vec![DeathEvent::Died {
            respawn_at: Tick(40 + RESPAWN_DELAY_TICKS)
        }],
        "only Died — no exp or zen dock below level 10"
    );
}

#[test]
fn a_max_level_death_loses_no_experience_but_still_pays_the_zen() {
    let atlas = real_atlas();
    let cap = atlas.exp_curve().cap_total();
    let max_level = atlas.exp_curve().max_level().get();
    let hero = dark_knight(
        max_level,
        cap.0,
        1_000_000,
        3,
        full_vitals(900, 200, 200),
        no_effects(),
        alive(),
    );

    let (dead_hero, events) = resolve_death(&hero, Tick(10), tick(), &atlas);

    assert_eq!(
        dead_hero.experience(),
        cap,
        "no level-401 band exists to lose from"
    );
    assert_eq!(dead_hero.zen(), or_abort(CarriedZen::new(970_000)));
    assert_eq!(
        events,
        vec![
            DeathEvent::Died {
                respawn_at: Tick(10 + RESPAWN_DELAY_TICKS)
            },
            DeathEvent::ZenDocked {
                lost: Zen(30_000),
                remaining: or_abort(CarriedZen::new(970_000)),
            },
        ],
        "max level docks 3% zen, no experience"
    );
}

#[test]
fn an_untrusted_over_cap_level_docks_no_experience_and_never_overflows() {
    let atlas = real_atlas();
    let cap = atlas.exp_curve().cap_total();
    // A persisted/untrusted level far above the era cap (400) still reaches the
    // band math: the next-level lookup at `u16::MAX + 1` would overflow, but the
    // add saturates to `u16::MAX`, misses the curve, and an over-cap level has no
    // band to lose — so no experience is docked and nothing panics.
    let hero = dark_knight(
        u16::MAX,
        cap.0,
        0,
        3,
        full_vitals(900, 200, 200),
        no_effects(),
        alive(),
    );

    let (dead_hero, events) = resolve_death(&hero, Tick(10), tick(), &atlas);

    assert_eq!(
        dead_hero.experience(),
        cap,
        "an over-cap level has no next-level band, so no experience is docked"
    );
    assert_eq!(
        events,
        vec![DeathEvent::Died {
            respawn_at: Tick(10 + RESPAWN_DELAY_TICKS)
        }],
        "only Died — the saturating next-level lookup folds to no penalty, no overflow"
    );
}

#[test]
fn the_zen_brackets_dock_one_two_and_three_percent_at_real_levels() {
    let atlas = real_atlas();
    for (lvl, lost, remaining) in [
        (100, 10_000, 990_000),
        (180, 20_000, 980_000),
        (300, 30_000, 970_000),
    ] {
        // Seat experience exactly at the level start so no experience is docked —
        // the assertion isolates the zen bracket.
        let exp = total(&atlas, lvl);
        let hero = dark_knight(
            lvl,
            exp,
            1_000_000,
            3,
            full_vitals(700, 150, 100),
            no_effects(),
            alive(),
        );

        let (dead_hero, events) = resolve_death(&hero, Tick(1), tick(), &atlas);

        assert_eq!(
            dead_hero.zen(),
            or_abort(CarriedZen::new(remaining)),
            "level {lvl} carried zen"
        );
        assert_eq!(
            events,
            vec![
                DeathEvent::Died {
                    respawn_at: Tick(1 + RESPAWN_DELAY_TICKS)
                },
                DeathEvent::ZenDocked {
                    lost: Zen(lost),
                    remaining: or_abort(CarriedZen::new(remaining)),
                },
            ],
            "level {lvl} docks its bracket zen only"
        );
    }
}

#[test]
fn a_tiny_balance_docks_no_zen_because_the_percentage_floors_to_zero() {
    let atlas = real_atlas();
    let exp = total(&atlas, 100);
    let hero = dark_knight(
        100,
        exp,
        50,
        3,
        full_vitals(700, 150, 100),
        no_effects(),
        alive(),
    );

    let (dead_hero, events) = resolve_death(&hero, Tick(1), tick(), &atlas);

    assert_eq!(
        dead_hero.zen(),
        or_abort(CarriedZen::new(50)),
        "1% of 50 floors to 0 — nothing docked"
    );
    assert_eq!(
        events,
        vec![DeathEvent::Died {
            respawn_at: Tick(1 + RESPAWN_DELAY_TICKS)
        }],
        "no ZenDocked when the floor is zero"
    );
}

#[test]
fn resolve_death_marks_dead_leaving_vitals_at_zero_and_a_poison_in_place() {
    let atlas = real_atlas();
    let exp = total(&atlas, 100);
    let hero = dark_knight(
        100,
        exp,
        1_000_000,
        3,
        zero_vitals(500, 400, 400),
        a_poison(),
        alive(),
    );
    let poison_before = hero.active_effects().poison();
    assert!(
        poison_before.is_some(),
        "the hero died with a poison active"
    );

    let (dead_hero, _events) = resolve_death(&hero, Tick(900), tick(), &atlas);

    assert_eq!(
        dead_hero.life(),
        LifeState::Dead {
            respawn_at: Tick(900 + RESPAWN_DELAY_TICKS)
        }
    );
    assert_eq!(
        dead_hero.vitals().health.current(),
        0,
        "resolve_death does not heal"
    );
    assert_eq!(dead_hero.vitals().mana.current(), 0);
    assert_eq!(dead_hero.vitals().ability.current(), 0);
    assert_eq!(
        dead_hero.active_effects().poison(),
        poison_before,
        "resolve_death does not clear — the poison rides through the dead beat"
    );
}

#[test]
fn a_second_resolve_death_on_a_dead_character_is_a_no_op() {
    let atlas = real_atlas();
    let exp = total(&atlas, 100) + 200_000;
    let hero = dark_knight(
        100,
        exp,
        1_000_000,
        3,
        full_vitals(500, 400, 400),
        no_effects(),
        alive(),
    );

    let (dead_once, _events) = resolve_death(&hero, Tick(500), tick(), &atlas);
    let (dead_twice, events) = resolve_death(&dead_once, Tick(9_999), tick(), &atlas);

    assert!(events.is_empty(), "a re-death emits no event");
    assert_eq!(
        or_abort(serde_json::to_string(&dead_twice)),
        or_abort(serde_json::to_string(&dead_once)),
        "the input is returned byte-identical — no second penalty, no re-mark"
    );
}

#[test]
fn respawn_seats_a_map_3_death_on_a_walkable_tile_inside_real_gate_27() {
    let atlas = real_atlas();
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        3,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    let rect = gate_rect(171, 108, 177, 117);
    assert_eq!(revived.placement().map, MapNumber(3));
    assert_landed_inside(&atlas, 3, revived.placement().position, rect);
    assert_eq!(revived.placement().facing, RESPAWN_FACING);
    assert_eq!(revived.life(), LifeState::Alive);
    assert_eq!(
        respawned,
        Some(Respawned {
            map: MapNumber(3),
            position: revived.placement().position,
            facing: RESPAWN_FACING,
        })
    );
}

#[test]
fn respawn_seats_a_map_8_tarkan_death_inside_real_gate_57() {
    let atlas = real_atlas();
    // Map 8 (Tarkan) now owns spawn gate 57 (an s6 backport), so a Tarkan death
    // respawns in Tarkan itself — not exiled to Lorencia as the shipped respawn did.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        8,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    let rect = gate_rect(187, 63, 203, 69);
    assert_eq!(
        revived.placement().map,
        MapNumber(8),
        "Tarkan, its own respawn_map"
    );
    assert_ne!(
        revived.placement().map,
        MapNumber(0),
        "not exiled to Lorencia"
    );
    assert_landed_inside(&atlas, 8, revived.placement().position, rect);
    assert_eq!(revived.life(), LifeState::Alive);
    assert_eq!(
        respawned,
        Some(Respawned {
            map: MapNumber(8),
            position: revived.placement().position,
            facing: RESPAWN_FACING,
        })
    );
}

#[test]
fn respawn_sends_a_map_10_icarus_death_down_to_lost_tower_gate_42() {
    let atlas = real_atlas();
    // Seed a flyer over Icarus (a Sky map that owns no town gate); its respawn_map
    // override (10 -> 4) settles the death on the ground of Lost Tower.
    let grounded = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        10,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );
    let mut value = serde_json::to_value(&grounded).unwrap();
    value["placement"]["movement"] = json!("flying");
    let hero: Character = serde_json::from_value(value).unwrap();
    assert_eq!(
        hero.placement().movement,
        Movement::Flying,
        "seeded flying over Icarus"
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    let rect = gate_rect(203, 70, 213, 81);
    assert_eq!(
        revived.placement().map,
        MapNumber(4),
        "Lost Tower — the override, not Lorencia"
    );
    assert_landed_inside(&atlas, 4, revived.placement().position, rect);
    assert_eq!(
        revived.placement().movement,
        Movement::Grounded,
        "a sky death stands up on the ground"
    );
    assert_eq!(revived.life(), LifeState::Alive);
    assert_eq!(respawned.map(|r| r.map), Some(MapNumber(4)));
}

#[test]
fn respawn_sends_a_map_9_devil_square_death_out_to_noria_not_the_arena() {
    let atlas = real_atlas();
    // Devil Square (map 9) owns its own gate 58, but its respawn_map override
    // (9 -> 3) routes the death out to Noria, the town hosting the event's door.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        9,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    let noria_gate = gate_rect(171, 108, 177, 117);
    let arena_gate = gate_rect(133, 91, 141, 99);
    assert_eq!(
        revived.placement().map,
        MapNumber(3),
        "Noria — the override"
    );
    assert_landed_inside(&atlas, 3, revived.placement().position, noria_gate);
    assert!(
        !arena_gate.contains(revived.placement().position),
        "not seated back inside Devil Square's own gate 58"
    );
    assert_eq!(respawned.map(|r| r.map), Some(MapNumber(3)));
}

#[test]
fn respawn_seats_a_map_2_devias_death_in_devias_unchanged() {
    let atlas = real_atlas();
    // Devias (map 2) owns gate 22; its respawn_map is self (2 -> 2), unchanged.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        2,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(revived.placement().map, MapNumber(2));
    assert_landed_inside(
        &atlas,
        2,
        revived.placement().position,
        gate_rect(197, 35, 218, 50),
    );
    assert_eq!(respawned.map(|r| r.map), Some(MapNumber(2)));
}

#[test]
fn respawn_seats_a_map_4_lost_tower_death_in_lost_tower_unchanged() {
    let atlas = real_atlas();
    // Lost Tower (map 4) owns gate 42; its respawn_map is self (4 -> 4), unchanged
    // — and the same town an Icarus death is redirected to.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        4,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(revived.placement().map, MapNumber(4));
    assert_landed_inside(
        &atlas,
        4,
        revived.placement().position,
        gate_rect(203, 70, 213, 81),
    );
    assert_eq!(respawned.map(|r| r.map), Some(MapNumber(4)));
}

#[test]
fn respawn_resolves_a_gate_less_dungeon_death_to_lorencia() {
    let atlas = real_atlas();
    // Dungeon (map 1) owns no town gate; its respawn_map defaults to Lorencia (1 -> 0).
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        1,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(revived.placement().map, MapNumber(0), "Lorencia");
    assert_landed_inside(
        &atlas,
        0,
        revived.placement().position,
        gate_rect(133, 118, 151, 135),
    );
    assert_eq!(respawned.map(|r| r.map), Some(MapNumber(0)));
}

#[test]
fn respawn_resolves_a_gate_less_exile_death_to_lorencia() {
    let atlas = real_atlas();
    // Exile (map 5) is an unreachable placeholder with no gate; respawn_map 5 -> 0.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        5,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, _respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(revived.placement().map, MapNumber(0), "Lorencia");
    assert_landed_inside(
        &atlas,
        0,
        revived.placement().position,
        gate_rect(133, 118, 151, 135),
    );
}

#[test]
fn respawn_on_a_map_outside_the_eleven_falls_back_to_lorencia() {
    let atlas = real_atlas();
    // An arbitrary Placement.map no record carries: the destination lookup returns
    // nothing (honest unknown-map optionality) and respawn seats the Lorencia fallback.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        200,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );
    assert!(
        atlas.respawn_gate_for_death_map(MapNumber(200)).is_none(),
        "map 200 has no respawn destination"
    );

    let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(revived.placement().map, MapNumber(0), "Lorencia fallback");
    assert_landed_inside(
        &atlas,
        0,
        revived.placement().position,
        gate_rect(133, 118, 151, 135),
    );
    assert_eq!(respawned.map(|r| r.map), Some(MapNumber(0)));
}

#[test]
fn respawn_carries_the_destination_map_not_the_died_on_map() {
    let atlas = real_atlas();
    // For a redirected death, both the returned placement and the Respawned outcome
    // carry the destination map, not the map the character died on.
    for (died_on, destination) in [(9u8, 3u8), (10, 4)] {
        let hero = dark_knight(
            100,
            total(&atlas, 100),
            500_000,
            died_on,
            zero_vitals(500, 400, 400),
            no_effects(),
            dead(1),
        );

        let (revived, respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

        assert_eq!(
            revived.placement().map,
            MapNumber(destination),
            "died on {died_on}, placement carries destination"
        );
        assert_eq!(
            respawned.map(|r| r.map),
            Some(MapNumber(destination)),
            "died on {died_on}, Respawned carries destination"
        );
    }
}

#[test]
fn respawn_on_a_multi_gate_map_lands_in_the_first_gate() {
    let atlas = real_atlas();
    // Map 6 carries three spawn gates (50, 51, 52); respawn takes the first.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        6,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived, _respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    let first_gate = gate_rect(101, 115, 103, 117);
    let second_gate = gate_rect(107, 115, 107, 115);
    let third_gate = gate_rect(107, 114, 107, 114);
    let pos = revived.placement().position;
    assert_landed_inside(&atlas, 6, pos, first_gate);
    assert!(!second_gate.contains(pos), "not the second gate");
    assert!(!third_gate.contains(pos), "not the third gate");
}

#[test]
fn respawn_refills_all_three_vitals_to_the_class_formula_maxima() {
    let atlas = real_atlas();
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        3,
        zero_vitals(1, 1, 1),
        no_effects(),
        dead(1),
    );

    let (revived, _respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    let (_profile, maxima) = character_profile(&revived);
    assert_eq!(revived.vitals().health.current(), maxima.max_health);
    assert_eq!(revived.vitals().health.max(), maxima.max_health);
    assert_eq!(revived.vitals().mana.current(), maxima.max_mana);
    assert_eq!(revived.vitals().mana.max(), maxima.max_mana);
    assert_eq!(revived.vitals().ability.current(), maxima.max_ability);
    assert_eq!(revived.vitals().ability.max(), maxima.max_ability);
}

#[test]
fn respawn_clears_a_poison_that_survived_the_death() {
    let atlas = real_atlas();
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        3,
        zero_vitals(500, 400, 400),
        a_poison(),
        dead(1),
    );
    assert!(
        hero.active_effects().poison().is_some(),
        "seeded with a poison"
    );

    let (revived, _respawned) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(
        revived.active_effects(),
        ActiveEffects::EMPTY,
        "every active effect is wiped on respawn"
    );
}

#[test]
fn respawn_is_deterministic_across_the_redirect_for_the_same_seed() {
    let atlas = real_atlas();
    // Death on map 9 (Devil Square) redirects to Noria (3); the redirect draws no
    // RNG, so the same seed lands on the identical destination tile both runs.
    let hero = dark_knight(
        100,
        total(&atlas, 100),
        500_000,
        9,
        zero_vitals(500, 400, 400),
        no_effects(),
        dead(1),
    );

    let (revived_a, respawned_a) = respawn(&hero, &atlas, &mut TestRng::new(7));
    let (revived_b, respawned_b) = respawn(&hero, &atlas, &mut TestRng::new(7));

    assert_eq!(
        respawned_a.map(|r| r.map),
        Some(MapNumber(3)),
        "redirected to Noria"
    );
    assert_eq!(
        respawned_a, respawned_b,
        "identical landing on the same seed"
    );
    assert_eq!(
        or_abort(serde_json::to_string(&revived_a)),
        or_abort(serde_json::to_string(&revived_b)),
        "byte-identical revived character"
    );
}

#[test]
fn the_full_loop_dies_takes_the_penalty_respawns_and_persists() {
    let atlas = real_atlas();
    let t100 = total(&atlas, 100);
    let band = total(&atlas, 101) - t100;
    let expected_exp_loss = band / 100;
    let exp = t100 + band / 2;

    let hero = dark_knight(
        100,
        exp,
        1_000_000,
        3,
        zero_vitals(500, 400, 400),
        a_poison(),
        alive(),
    );

    // Die: penalty applied, marked Dead, vitals and poison left in place.
    let (dead_hero, _death_events) = resolve_death(&hero, Tick(500), tick(), &atlas);
    assert_eq!(
        dead_hero.life(),
        LifeState::Dead {
            respawn_at: Tick(500 + RESPAWN_DELAY_TICKS)
        }
    );
    assert_eq!(dead_hero.experience(), Exp(exp - expected_exp_loss));
    assert_eq!(dead_hero.zen(), or_abort(CarriedZen::new(990_000)));
    assert_eq!(dead_hero.vitals().health.current(), 0, "not healed yet");
    assert!(
        dead_hero.active_effects().poison().is_some(),
        "poison still present"
    );

    // Respawn: alive, full, cleared, standing in map 3's gate.
    let (revived, respawned) = respawn(&dead_hero, &atlas, &mut TestRng::new(7));
    assert_eq!(revived.life(), LifeState::Alive);
    assert_eq!(revived.active_effects(), ActiveEffects::EMPTY);
    assert_landed_inside(
        &atlas,
        3,
        revived.placement().position,
        gate_rect(171, 108, 177, 117),
    );
    assert_eq!(
        respawned,
        Some(Respawned {
            map: MapNumber(3),
            position: revived.placement().position,
            facing: RESPAWN_FACING,
        })
    );
    let (_profile, maxima) = character_profile(&revived);
    assert_eq!(revived.vitals().health.current(), maxima.max_health);

    // Level, class, stats, points, and the post-penalty exp and zen all survive.
    assert_eq!(revived.level(), dead_hero.level());
    assert_eq!(revived.class(), dead_hero.class());
    assert_eq!(revived.stats(), dead_hero.stats());
    assert_eq!(revived.unspent_points(), dead_hero.unspent_points());
    assert_eq!(revived.experience(), Exp(exp - expected_exp_loss));
    assert_eq!(revived.zen(), or_abort(CarriedZen::new(990_000)));

    // The revived hero round-trips through persist (the class↔stats gate re-proves).
    let wire = or_abort(serde_json::to_string(&revived));
    let reloaded: Character = or_abort(serde_json::from_str(&wire));
    assert_eq!(reloaded, revived);
}

#[test]
fn every_gated_map_resolves_a_walkable_respawn_gate_over_real_data() {
    let atlas = real_atlas();
    // The parse-time invariant holds over the shipped dataset: every map carrying
    // a spawn gate resolves a respawn point whose every retained landing tile is
    // walkable. Map 6 carries orphan gates 51/52 on blocked tiles, yet parse still
    // succeeds because only a map's first gate (50) is a respawn point. Map 8
    // (Tarkan) now owns spawn gate 57.
    for map in [0u8, 2, 3, 4, 6, 7, 8, 9] {
        let gate = or_abort(
            atlas
                .spawn_gate(MapNumber(map))
                .ok_or("gated map resolves a respawn gate"),
        );
        assert_eq!(gate.map, MapNumber(map));
        let grid = or_abort(atlas.walk_grid(MapNumber(map)).ok_or("map has a walk grid"));
        for &landing in gate.landing.iter() {
            assert!(grid.walkable(landing), "map {map} landing tile is walkable");
        }
    }
    assert_eq!(atlas.fallback_spawn_gate().map, MapNumber(0));
}
