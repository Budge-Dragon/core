//! The safezone model over the real `/data` terrain (W-GROUND): the folded
//! safe-bit census — Lorencia's 1,552-tile town core, its 1,008 excluded
//! `Safezone|Blocked` tiles, the zero-safezone maps, Exile's copied-Arena
//! pockets — plus the combat firewall (caster refusal, covered-set exclusion,
//! push and jiggle stops) and the monster-AI safezone semantics (universal
//! target filter, the basic/trap suppression, the guard exemption), every
//! tile discovered from the shipped terrain rather than hard-coded.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]`
//! body so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use dataset::{or_abort, real_atlas, real_static_data};
use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::combat_profile::CombatTarget;
use mu_core::components::element::{Element, PerElement};
use mu_core::components::movement::{Mobility, Movement};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, UNITS_PER_TILE};
use mu_core::components::tile::{TERRAIN_LEN, TerrainGrid, TileCoord};
use mu_core::components::units::{Level, MapNumber, Resistance, Tick, TickDuration};
use mu_core::data::atlas::Atlas;
use mu_core::data::monster_definitions::{MobBehavior, MonsterCombat, MonsterRole};
use mu_core::data::skills::{AreaDisplacement, AreaGeometry, DamageType, Skill};
use mu_core::entities::character::Character;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::skills::{CastRejection, SkillOutcome, TargetHit};
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::profile::{character_profile, monster_profile};
use mu_core::services::skills::{DamagingSkill, DamagingSkillRef, SkillRouting, cast, route};
use rng::TestRng;

// --- Fixtures over the real terrain. ------------------------------------------

/// Lorencia's map number — the map whose town core the firewall drives.
fn lorencia() -> MapNumber {
    MapNumber(0)
}

/// The raw terrain bytes of `map`, straight from the shipped sidecar.
fn raw_terrain(map: MapNumber) -> [u8; TERRAIN_LEN] {
    let data = real_static_data();
    *or_abort(
        data.terrain
            .into_iter()
            .find(|terrain| terrain.map == map)
            .ok_or("the dataset carries the map's terrain sidecar"),
    )
    .bytes
    .as_array()
}

/// Lorencia's parsed unified terrain grid.
fn lorencia_grid(atlas: &Atlas) -> &TerrainGrid {
    or_abort(
        atlas
            .terrain_grid(lorencia())
            .ok_or("Lorencia has a terrain grid"),
    )
}

/// Whether the tile is walkable and NOT safe — an open field tile.
fn field(grid: &TerrainGrid, x: u8, y: u8) -> bool {
    let pos = TileCoord::new(x, y).to_world();
    grid.walkable(pos) && !grid.safe(pos)
}

/// The first `(field, safe)` +X-adjacent tile pair — the town boundary,
/// discovered from the real grid.
fn boundary_pair(grid: &TerrainGrid) -> (TileCoord, TileCoord) {
    for y in 0u8..=u8::MAX {
        for x in 0u8..u8::MAX {
            if field(grid, x, y) && grid.safe(TileCoord::new(x + 1, y).to_world()) {
                return (TileCoord::new(x, y), TileCoord::new(x + 1, y));
            }
        }
    }
    or_abort(Err::<(TileCoord, TileCoord), _>(
        "Lorencia has a field tile bordering its safe core",
    ))
}

/// The first +X row lane of three field tiles followed by a safe tile — the
/// push runway into the town core, discovered from the real grid.
fn push_lane(grid: &TerrainGrid) -> [TileCoord; 4] {
    for y in 0u8..=u8::MAX {
        for x in 0u8..u8::MAX - 3 {
            if field(grid, x, y)
                && field(grid, x + 1, y)
                && field(grid, x + 2, y)
                && grid.safe(TileCoord::new(x + 3, y).to_world())
            {
                return [
                    TileCoord::new(x, y),
                    TileCoord::new(x + 1, y),
                    TileCoord::new(x + 2, y),
                    TileCoord::new(x + 3, y),
                ];
            }
        }
    }
    or_abort(Err::<[TileCoord; 4], _>(
        "Lorencia has a three-tile field lane into its safe core",
    ))
}

/// The first field tile whose eight neighbours are all walkable and at least
/// one is safe — the jiggle subject's seat, discovered from the real grid.
fn jiggle_spot(grid: &TerrainGrid) -> TileCoord {
    for y in 1u8..u8::MAX {
        for x in 1u8..u8::MAX {
            if !field(grid, x, y) {
                continue;
            }
            let neighbours = [
                (x - 1, y - 1),
                (x, y - 1),
                (x + 1, y - 1),
                (x - 1, y),
                (x + 1, y),
                (x - 1, y + 1),
                (x, y + 1),
                (x + 1, y + 1),
            ];
            let all_walkable = neighbours
                .iter()
                .all(|&(nx, ny)| grid.walkable(TileCoord::new(nx, ny).to_world()));
            let any_safe = neighbours
                .iter()
                .any(|&(nx, ny)| grid.safe(TileCoord::new(nx, ny).to_world()));
            if all_walkable && any_safe {
                return TileCoord::new(x, y);
            }
        }
    }
    or_abort(Err::<TileCoord, _>(
        "Lorencia has an open field tile bordering its safe core",
    ))
}

/// A gearless level-50 caster of `class` seated at `tile` on Lorencia with
/// deep vitals, built the only way a character can be — through the wire.
fn caster(class: &str, strength: u16, energy: u16, tile: TileCoord) -> Character {
    let json = serde_json::json!({
        "class": class,
        "level": 50,
        "experience": 0,
        "stats": {"kind": "standard", "strength": strength, "agility": 100, "vitality": 100, "energy": energy},
        "unspent_points": 0,
        "zen": 0,
        "placement": {
            "position": or_abort(serde_json::to_value(tile.to_world())),
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

/// A deep-health, zero-defense-rate monster target seated at `tile` — every
/// strike lands, none kills.
fn seated(tile: TileCoord) -> CombatTarget {
    let combat = MonsterCombat {
        level: or_abort(Level::new(20)),
        hp: 1_000_000,
        min_phys_damage: 5,
        max_phys_damage: 10,
        defense: 0,
        attack_rate: 10,
        defense_rate: 0,
    };
    let zero = PerElement {
        ice: Resistance(0),
        poison: Resistance(0),
        lightning: Resistance(0),
        fire: Resistance(0),
        earth: Resistance(0),
        wind: Resistance(0),
        water: Resistance(0),
    };
    let placement = Placement {
        position: tile.to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: lorencia(),
    };
    CombatTarget::new(
        monster_profile(&combat, &zero, combat.level),
        Pool::full(1_000_000),
        placement,
        ActiveEffects::EMPTY,
    )
}

/// The damaging reference of a routed skill; a non-damaging route aborts.
fn as_damaging(skill: &Skill) -> DamagingSkillRef<'_> {
    match route(skill) {
        SkillRouting::Damaging(reference) => reference,
        SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => {
            or_abort(Err::<DamagingSkillRef<'_>, _>("expected a damaging skill"))
        }
    }
}

/// The first damaging skill matching `predicate`, found from the shipped
/// catalog — never a hard-coded number.
fn find_skill<'a>(atlas: &'a Atlas, predicate: impl Fn(&Skill) -> bool, err: &str) -> &'a Skill {
    or_abort(
        atlas
            .skills()
            .find(|skill| matches!(route(skill), SkillRouting::Damaging(_)) && predicate(skill))
            .ok_or(err),
    )
}

/// The fire caster-circle Nova — the sweeping area strike.
fn nova(atlas: &Atlas) -> &Skill {
    find_skill(
        atlas,
        |skill| {
            matches!(
                as_damaging(skill).shape(),
                DamagingSkill::Area {
                    geometry: AreaGeometry::CasterCircle { radius_x2: 12 },
                    ..
                }
            ) && skill.element == Some(Element::Fire)
        },
        "the dataset has no nova area skill",
    )
}

/// The directional-push quake — Earthshake.
fn earthshake(atlas: &Atlas) -> &Skill {
    find_skill(
        atlas,
        |skill| {
            matches!(
                as_damaging(skill).shape(),
                DamagingSkill::Area {
                    displacement: AreaDisplacement::DirectionalPush,
                    ..
                }
            )
        },
        "the dataset has no directional-push skill",
    )
}

/// The lightning-element direct hit — the jiggling bolt.
fn lightning_bolt(atlas: &Atlas) -> &Skill {
    find_skill(
        atlas,
        |skill| {
            matches!(as_damaging(skill).shape(), DamagingSkill::DirectHit)
                && skill.element == Some(Element::Lightning)
                && skill.damage_type == DamageType::Wizardry
        },
        "the dataset has no lightning direct-hit skill",
    )
}

/// The first real Guard-role behavior — a town guard's timing/range columns
/// with its `Patrols` disposition, straight from the shipped roster.
fn guard_behavior(atlas: &Atlas) -> MobBehavior {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Guard { behavior, .. } => Some(*behavior),
                MonsterRole::Monster { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has a town guard"),
    )
}

/// The first real basic-monster behavior that can move and attack.
fn basic_behavior(atlas: &Atlas) -> MobBehavior {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster { behavior, .. } => {
                    (behavior.attack_range >= 1 && behavior.move_range >= 1).then_some(*behavior)
                }
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has a mobile attacking monster"),
    )
}

/// A ready-to-act mob anchored on its own tile.
fn mob_at(tile: TileCoord) -> MonsterInstance {
    MonsterInstance {
        number: mu_core::data::common::MonsterNumber(0),
        placement: Placement {
            position: tile.to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: lorencia(),
        },
        health: Pool::full(1_000),
        anchor: tile.to_world(),
        next_action: Tick(0),
        active_effects: ActiveEffects::EMPTY,
    }
}

fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

// --- The safe-bit census over every shipped sidecar (P3, P4). ------------------

#[test]
fn lorencias_town_core_is_the_folded_conjunction_of_its_raw_bytes() {
    // The bijection: for every one of the 65,536 tiles, the parsed grid's
    // safe() equals the raw `SafeZone AND walkable` conjunction — and the
    // counts pin Lorencia's 1,552-tile town core and its 1,008 excluded
    // Safezone|Blocked tiles (safe-bit set on a blocked tile is NOT safe).
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let bytes = raw_terrain(lorencia());

    let mut safe_tiles = 0u32;
    let mut safe_blocked = 0u32;
    for (index, &attr) in bytes.iter().enumerate() {
        let x = or_abort(u8::try_from(index % 256));
        let y = or_abort(u8::try_from(index / 256));
        let pos = TileCoord::new(x, y).to_world();
        let raw_walkable = attr & 0x06 == 0;
        let raw_safe = raw_walkable && attr & 0x01 != 0;
        assert_eq!(grid.walkable(pos), raw_walkable, "walkable at ({x},{y})");
        assert_eq!(grid.safe(pos), raw_safe, "safe at ({x},{y})");
        if raw_safe {
            safe_tiles += 1;
        }
        if attr & 0x01 != 0 && attr & 0x06 != 0 {
            safe_blocked += 1;
            assert!(!grid.safe(pos), "a Safezone|Blocked tile is not safe");
            assert!(
                !grid.walkable(pos),
                "a Safezone|Blocked tile is not walkable"
            );
        }
    }
    assert_eq!(safe_tiles, 1_552, "Lorencia's town core");
    assert_eq!(safe_blocked, 1_008, "Lorencia's Safezone|Blocked exclusion");
}

#[test]
fn zero_safezone_maps_answer_not_safe_everywhere_and_exile_keeps_its_copied_pockets() {
    let atlas = real_atlas();

    // The Dungeon (map 1) and Devil Square (map 9) sidecars carry no walkable
    // SafeZone tile at all — the whole map answers not-safe.
    for map in [MapNumber(1), MapNumber(9)] {
        let grid = or_abort(atlas.terrain_grid(map).ok_or("the map has a grid"));
        let mut safe_tiles = 0u32;
        for y in 0u8..=u8::MAX {
            for x in 0u8..=u8::MAX {
                if grid.safe(TileCoord::new(x, y).to_world()) {
                    safe_tiles += 1;
                }
            }
        }
        assert_eq!(safe_tiles, 0, "map {} carries no safe tile", map.0);
    }

    // Exile (map 5) keeps its authentic copied-Arena bytes: 447 safe tiles,
    // tile-for-tile identical to Arena's (map 6).
    let exile = or_abort(atlas.terrain_grid(MapNumber(5)).ok_or("Exile has a grid"));
    let arena = or_abort(atlas.terrain_grid(MapNumber(6)).ok_or("Arena has a grid"));
    let mut exile_safe = 0u32;
    for y in 0u8..=u8::MAX {
        for x in 0u8..=u8::MAX {
            let pos = TileCoord::new(x, y).to_world();
            assert_eq!(
                exile.safe(pos),
                arena.safe(pos),
                "copied pocket at ({x},{y})"
            );
            if exile.safe(pos) {
                exile_safe += 1;
            }
        }
    }
    assert_eq!(exile_safe, 447, "Exile's copied-Arena safe pockets");
}

#[test]
fn every_shipped_map_resolves_one_unified_grid_answering_both_queries() {
    let atlas = real_atlas();
    let mut maps = 0u32;
    for definition in atlas.maps() {
        let grid = or_abort(
            atlas
                .terrain_grid(definition.number)
                .ok_or("every map resolves a unified terrain grid"),
        );
        // One grid answers both queries, and safe implies walkable on every
        // tile of every shipped map — the fold holds dataset-wide.
        for y in 0u8..=u8::MAX {
            for x in 0u8..=u8::MAX {
                let pos = TileCoord::new(x, y).to_world();
                if grid.safe(pos) {
                    assert!(grid.walkable(pos), "safe implies walkable at ({x},{y})");
                }
            }
        }
        maps += 1;
    }
    assert_eq!(maps, 11, "the eleven shipped maps each resolve a grid");
}

// --- The combat firewall over real Lorencia (terms a, b, c, e). ---------------

#[test]
fn a_real_cast_from_a_lorencia_safe_tile_is_rejected_before_any_spend() {
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let (field_tile, safe_tile) = boundary_pair(grid);

    let town_caster = caster("dark_wizard", 40, 200, safe_tile);
    let skill = nova(&atlas);
    let targets = [seated(field_tile)];
    let before = town_caster.vitals();
    let mut rng = TestRng::new(7);
    let (vitals, outcome) = cast(
        &town_caster,
        &character_profile(&town_caster).0,
        as_damaging(skill).locate(safe_tile.to_world()),
        &targets,
        grid,
        &mut rng,
    );
    assert_eq!(
        outcome,
        SkillOutcome::Rejected {
            reason: CastRejection::CasterInSafezone
        }
    );
    assert_eq!(
        vitals.mana.current(),
        before.mana.current(),
        "nothing spent"
    );
    assert_eq!(vitals.ability.current(), before.ability.current());
}

#[test]
fn a_real_area_cast_excludes_the_safe_tile_stander_from_its_covered_set() {
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let (field_tile, safe_tile) = boundary_pair(grid);

    // The caster stands two tiles out on the field; the Nova disc covers both
    // the town-stander and the field-stander geometrically.
    let caster_tile = TileCoord::new(field_tile.x().saturating_sub(1), field_tile.y());
    let field_caster = caster("dark_wizard", 40, 200, caster_tile);
    let skill = nova(&atlas);
    let targets = [seated(safe_tile), seated(field_tile)];
    let mut rng = TestRng::new(7);
    let (_, outcome) = cast(
        &field_caster,
        &character_profile(&field_caster).0,
        as_damaging(skill).locate(caster_tile.to_world()),
        &targets,
        grid,
        &mut rng,
    );
    let SkillOutcome::Cast { hits, .. } = outcome else {
        panic!("a funded field cast resolves");
    };
    assert!(!hits.is_empty(), "the field-stander is struck");
    for hit in &hits {
        let index = match hit {
            TargetHit::Missed { target_index, .. }
            | TargetHit::Landed { target_index, .. }
            | TargetHit::Killed { target_index, .. } => *target_index,
        };
        assert_eq!(index, 1, "only the field-stander produces a hit");
    }
}

#[test]
fn a_real_earthshake_push_stops_at_the_town_core_boundary() {
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let [caster_tile, target_tile, last_field, first_safe] = push_lane(grid);

    // The quake pushes the mob due east, away from the caster: the opening
    // increment gains the last field tile, the next is refused at the safe
    // boundary like a wall — gained ground kept, town never entered.
    let knight = caster("dark_knight", 200, 30, caster_tile);
    let skill = earthshake(&atlas);
    let targets = [seated(target_tile)];
    let mut rng = TestRng::new(11);
    let (_, outcome) = cast(
        &knight,
        &character_profile(&knight).0,
        as_damaging(skill).locate(caster_tile.to_world()),
        &targets,
        grid,
        &mut rng,
    );
    let SkillOutcome::Cast { hits, .. } = outcome else {
        panic!("a funded quake resolves");
    };
    let displacement = match or_abort(hits.first().ok_or("the covered mob is resolved")) {
        TargetHit::Landed { displacement, .. } | TargetHit::Missed { displacement, .. } => {
            *displacement
        }
        TargetHit::Killed { .. } => panic!("a deep-health mob is never killed"),
    };
    let moved = or_abort(displacement.ok_or("the push moves the mob"));
    assert_eq!(
        moved.position,
        last_field.to_world(),
        "the push stops on the last field tile"
    );
    assert!(!grid.safe(moved.position));
    assert_ne!(moved.position, first_safe.to_world());
}

#[test]
fn a_real_lightning_jiggle_never_lands_its_target_on_a_safe_tile() {
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let spot = jiggle_spot(grid);
    let caster_tile = TileCoord::new(spot.x().saturating_sub(2), spot.y());

    let wizard = caster("dark_wizard", 40, 200, caster_tile);
    let skill = lightning_bolt(&atlas);
    let targets = [seated(spot)];
    let tile_units = UNITS_PER_TILE;

    let mut saw_move = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let (_, outcome) = cast(
            &wizard,
            &character_profile(&wizard).0,
            as_damaging(skill).locate(spot.to_world()),
            &targets,
            grid,
            &mut rng,
        );
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded bolt resolves");
        };
        if let Some(TargetHit::Landed {
            displacement: Some(moved),
            ..
        }) = hits.first()
        {
            assert!(
                !grid.safe(moved.position),
                "seed {seed}: a jiggle never lands on a safe tile"
            );
            let dx = moved.position.x().raw() - spot.to_world().x().raw();
            let dy = moved.position.y().raw() - spot.to_world().y().raw();
            assert!(dx.abs() <= tile_units && dy.abs() <= tile_units);
            saw_move = true;
        }
    }
    assert!(saw_move, "some seed lands a real jiggle move");
}

// --- Monster-AI safezone semantics over real defs and real terrain. -----------

#[test]
fn no_role_targets_a_safezone_stander() {
    // A player parked on a safe town tile is no target for a basic mob NOR a
    // guard — the universal filter, one tile from either's fangs.
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let (field_tile, safe_tile) = boundary_pair(grid);

    for behavior in [basic_behavior(&atlas), guard_behavior(&atlas)] {
        let mob = mob_at(field_tile);
        let mut rng = TestRng::new(3);
        let (_, intent) = decide_monster_action(
            &mob,
            &behavior,
            Some(safe_tile.to_world()),
            Tick(0),
            tick(),
            grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(
            !matches!(
                intent,
                MonsterIntent::Attack { .. } | MonsterIntent::Chase { .. }
            ),
            "a safezone-stander is never attacked or chased"
        );
    }
}

#[test]
fn a_basic_mob_on_a_safe_tile_never_attacks_but_a_real_guard_does() {
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let (field_tile, safe_tile) = boundary_pair(grid);

    // The basic mob standing in town refuses to swing at the in-range field
    // target (it may still chase off the safe tile).
    let mob = mob_at(safe_tile);
    let mut rng = TestRng::new(3);
    let (_, intent) = decide_monster_action(
        &mob,
        &basic_behavior(&atlas),
        Some(field_tile.to_world()),
        Tick(0),
        tick(),
        grid,
        Mobility::Free,
        &mut rng,
    );
    assert!(
        !matches!(intent, MonsterIntent::Attack { .. }),
        "a basic mob never swings from a safe tile"
    );

    // The real town guard on the same tile attacks the same target.
    let guard = mob_at(safe_tile);
    let mut rng = TestRng::new(3);
    let (_, intent) = decide_monster_action(
        &guard,
        &guard_behavior(&atlas),
        Some(field_tile.to_world()),
        Tick(0),
        tick(),
        grid,
        Mobility::Free,
        &mut rng,
    );
    assert_eq!(
        intent,
        MonsterIntent::Attack {
            target: field_tile.to_world()
        },
        "a guard attacks from inside the safezone"
    );
}

#[test]
fn a_wandering_basic_mob_never_enters_the_safe_core_while_a_guard_patrols_in() {
    let atlas = real_atlas();
    let grid = lorencia_grid(&atlas);
    let (field_tile, _) = boundary_pair(grid);

    // Sixty-four wander draws from the boundary tile: the basic mob's step
    // onto the adjacent safe tile is refused every time...
    for seed in 0u64..64 {
        let mob = mob_at(field_tile);
        let mut rng = TestRng::new(seed);
        let (advanced, _) = decide_monster_action(
            &mob,
            &basic_behavior(&atlas),
            None,
            Tick(0),
            tick(),
            grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(
            !grid.safe(advanced.placement.position),
            "seed {seed}: a basic mob never stands on a safe tile"
        );
    }

    // ...while the guard's patrol step resolves onto it for some draw.
    let mut guard_entered = false;
    for seed in 0u64..64 {
        let guard = mob_at(field_tile);
        let mut rng = TestRng::new(seed);
        let (advanced, _) = decide_monster_action(
            &guard,
            &guard_behavior(&atlas),
            None,
            Tick(0),
            tick(),
            grid,
            Mobility::Free,
            &mut rng,
        );
        if grid.safe(advanced.placement.position) {
            guard_entered = true;
            break;
        }
    }
    assert!(guard_entered, "a guard patrols onto the safe town tiles");
}
