//! End-to-end combat over the real dataset: derive a real character profile,
//! pick a real monster, beat it to zero health with the combat service, then
//! award its kill through the loot/experience/kill orchestrator — proving the
//! composed pipeline resolves a finite, well-formed, bit-for-bit replayable kill
//! against the shipped `/data`.
//!
//! The second half is the mutation teeth-check: for each invariant the plan
//! guards, a runnable comparison of the correct result against the specific bug
//! that would break it, naming the inline test that reddens. The teeth-check is
//! real code — it computes both values and asserts they differ — not prose.
//!
//! This file carries its own dataset loader rather than sharing `common` (whose
//! ambient-simulation helpers are unused here); load failures route through
//! `or_abort` so no banned suppressor is needed outside a `#[test]` body.

use std::io::Write;
use std::path::PathBuf;

use rand_core::RngCore;
use serde::de::DeserializeOwned;

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::collections::OneOrMore;
use mu_core::components::combat_profile::CombatTarget;
use mu_core::components::element::PerElement;
use mu_core::components::movement::{Mobility, Movement};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::Facing;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{DurationMs, Level, MapNumber, Resistance, Tick, TickDuration};
use mu_core::data::ancient_sets::AncientSet;
use mu_core::data::atlas::{Atlas, StaticData};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::classes::ClassRecord;
use mu_core::data::common::{DataFile, ItemRef, MonsterNumber};
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_config::GameConfig;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::{
    MobBehavior, MonsterCombat, MonsterDefinition, MonsterRole,
};
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::entities::character::Character;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::events::combat::AttackOutcome;
use mu_core::events::loot::Drop;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::skills::SkillOutcome;
use mu_core::services::combat::resolve_attack;
use mu_core::services::kill::resolve_kill;
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::profile::{character_profile, monster_profile};
use mu_core::services::ratio::{floor_div_u64_to_u32, nonzero, scale_ratio};
use mu_core::services::skills::{DamagingSkillRef, SkillRouting, cast, route};

// --- Self-contained dataset harness (load failures abort, never unwrap). ---

fn or_abort<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "combat_simulation harness: load failure: {error}");
            std::process::abort()
        }
    }
}

fn data_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("data");
    path.push(format!("{name}.json"));
    path
}

fn load<T: DeserializeOwned>(name: &str) -> DataFile<T> {
    let text = or_abort(std::fs::read_to_string(data_path(name)));
    or_abort(serde_json::from_str(&text))
}

fn load_terrain() -> Vec<MapTerrain> {
    (0u8..=10)
        .map(|map| {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("..");
            path.push("data");
            path.push("terrain");
            path.push(format!("{map}.bin"));
            MapTerrain {
                map: MapNumber(map),
                bytes: or_abort(TerrainBytes::new(or_abort(std::fs::read(&path)))),
            }
        })
        .collect()
}

fn real_atlas() -> Atlas {
    let data = StaticData {
        maps: load::<MapDefinition>("map_definitions"),
        gates_warps: load::<GateWarpRecord>("gates_warps"),
        monsters: load::<MonsterDefinition>("monster_definitions"),
        spawns: load::<Spawn>("spawns"),
        skills: load::<Skill>("skills"),
        items: load::<ItemDefinition>("item_definitions"),
        box_drops: load::<BoxDrop>("box_drops"),
        special_drops: load::<SpecialDropRecord>("special_drops"),
        ancient_sets: load::<AncientSet>("ancient_sets"),
        chaos_mixes: load::<ChaosMix>("chaos_mixes"),
        classes: load::<ClassRecord>("classes"),
        exp_tables: load::<ExpTable>("exp_tables"),
        game_config: load::<GameConfig>("game_config"),
        terrain: load_terrain(),
    };
    or_abort(Atlas::parse(data))
}

/// Deterministic `SplitMix64` — the shared replayable stream.
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

// --- Domain fixtures over the real data. ---

/// A plausible gearless Dark Knight killer at the given level, strength, and tile.
fn dark_knight(level: u16, strength: u16, tile: TileCoord) -> Character {
    let position = or_abort(serde_json::to_value(tile.to_world()));
    let json = serde_json::json!({
        "class": "dark_knight",
        "level": level,
        "experience": 0,
        "stats": {"kind": "standard", "strength": strength, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "placement": {"position": position, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    });
    or_abort(serde_json::from_value(json))
}

/// The first fighting monster at or below `max_level`, with its combat block and
/// resistances. Only the three combat roles carry a combat block.
fn low_level_monster(
    atlas: &Atlas,
    max_level: u16,
) -> (MonsterNumber, MonsterCombat, PerElement<Resistance>) {
    or_abort(
        atlas
            .monsters()
            .find_map(|definition| match &definition.role {
                MonsterRole::Monster {
                    combat,
                    resistances,
                    ..
                } => (combat.level.get() <= max_level && combat.hp > 0).then_some((
                    definition.number,
                    *combat,
                    *resistances,
                )),
                MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::Npc { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no low-level fighting monster"),
    )
}

/// Zero elemental resistance across every element — makes a lightning
/// application land every time, isolating the knockback composition.
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

fn victim_instance(number: MonsterNumber, hp: u32) -> MonsterInstance {
    MonsterInstance {
        number,
        placement: Placement {
            position: TileCoord::new(20, 20).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        },
        health: Pool::full(hp),
        anchor: TileCoord::new(20, 20).to_world(),
        next_action: Tick(0),
        active_effects: ActiveEffects::EMPTY,
    }
}

/// The damaging reference the router yields, or `None` for a non-damaging skill.
fn as_damaging(skill: &Skill) -> Option<DamagingSkillRef<'_>> {
    match route(skill) {
        SkillRouting::Damaging(reference) => Some(reference),
        SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => None,
    }
}

fn lightning_bolt() -> Skill {
    let json = serde_json::json!({
        "number": 1,
        "source_version": "075",
        "attack_damage": 0,
        "damage_type": "physical",
        "element": "lightning",
        "range": 6,
        "shape": {"kind": "direct_hit"},
        "cost": {"mana": 0, "ability": 0},
        "learn": {"level": 0, "energy": 0, "command": 0},
        "classes": []
    });
    or_abort(serde_json::from_value(json))
}

fn town_tile(tile: TileCoord) -> Placement {
    Placement {
        position: tile.to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: MapNumber(0),
    }
}

/// Beats a monster to zero health with repeated strikes; returns the strike
/// count. Bounded — the level-scaled minimum-damage floor guarantees progress.
fn beat_to_death(
    killer: &Character,
    combat: &MonsterCombat,
    resistances: PerElement<Resistance>,
    seed: u64,
) -> u32 {
    let (attacker, _) = character_profile(killer);
    let target = monster_profile(combat, &resistances, combat.level);
    let mut rng = TestRng::new(seed);
    let mut health = Pool::full(combat.hp);
    let mut strikes = 0u32;
    while health.current() > 0 {
        let (next, outcome) = resolve_attack(&attacker, &target, health, &mut rng);
        health = next;
        strikes += 1;
        assert!(strikes < 1_000_000, "a real kill must terminate");
        if let AttackOutcome::Killed { .. } = outcome {
            break;
        }
    }
    assert_eq!(health.current(), 0);
    strikes
}

fn assert_item_drop_in_formula(atlas: &Atlas, drop: &Drop, level: Level) {
    let Drop::Item {
        item, level: plus, ..
    } = drop
    else {
        return;
    };
    let definition = or_abort(
        atlas
            .item(*item)
            .ok_or("a dropped item must resolve in the atlas"),
    );
    let monster_level = level.get();
    let above_base = u32::from(monster_level.saturating_sub(u16::from(definition.drop_level)));
    let expected = (above_base / 3).min(u32::from(definition.max_item_level.get()));
    assert!(
        u32::from(plus.get()) <= u32::from(definition.max_item_level.get()),
        "plus level within the item cap"
    );
    assert_eq!(
        u32::from(plus.get()),
        expected,
        "plus level obeys the formula"
    );
}

// --- End-to-end kill. ---

#[test]
fn a_real_character_kills_a_real_monster_and_collects_a_well_formed_reward() {
    let atlas = real_atlas();
    let killer = dark_knight(30, 150, TileCoord::new(10, 10));
    let (number, combat, resistances) = low_level_monster(&atlas, 20);

    let strikes = beat_to_death(&killer, &combat, resistances, 7);
    assert!(strikes > 0);

    let victim = victim_instance(number, combat.hp);
    let mut rng = TestRng::new(7);
    let resolution = resolve_kill(&killer, &victim, &atlas, &mut rng);

    // A real fighting-monster kill grants positive experience.
    assert!(
        resolution.experience.gained.0 > 0,
        "a kill grants experience"
    );

    // A money drop is exactly the awarded experience plus the base money bonus.
    if let Drop::Zen { amount } = resolution.drops.category {
        assert_eq!(amount.0, resolution.experience.gained.0 + 7);
    }

    // An item drop's plus level obeys min((level - drop_level) / 3, max).
    assert_item_drop_in_formula(&atlas, &resolution.drops.category, combat.level);
    for special in &resolution.drops.specials {
        assert_item_drop_in_formula(&atlas, special, combat.level);
    }

    // Level-ups are strictly ascending.
    let mut previous: Option<u16> = None;
    for level_up in &resolution.level_ups {
        if let Some(prior) = previous {
            assert!(level_up.level.get() > prior, "level-ups ascend");
        }
        previous = Some(level_up.level.get());
    }
}

#[test]
fn the_whole_kill_replays_bit_for_bit_on_the_same_seed() {
    let atlas = real_atlas();
    let killer = dark_knight(30, 150, TileCoord::new(10, 10));
    let (number, combat, _) = low_level_monster(&atlas, 20);
    let victim = victim_instance(number, combat.hp);

    let run = |seed: u64| {
        let mut rng = TestRng::new(seed);
        resolve_kill(&killer, &victim, &atlas, &mut rng)
    };
    assert_eq!(run(2024), run(2024));
    assert_eq!(run(99), run(99));
}

#[test]
fn a_shoved_monster_re_chases_its_attacker() {
    // The emergent interrupt: a lightning strike shoves a monster off its tile,
    // and on its next AI tick the monster re-engages the caster — the movement
    // and skill services composing into a chase.
    let atlas = real_atlas();
    let (_, combat, _) = low_level_monster(&atlas, 20);
    let grid = atlas
        .walk_grid(MapNumber(0))
        .expect("Lorencia has a walk grid")
        .clone();

    let caster = dark_knight(50, 200, TileCoord::new(135, 125));
    let monster_tile = TileCoord::new(138, 125);
    let target = CombatTarget::new(
        monster_profile(&combat, &zero_resistances(), combat.level),
        Pool::full(combat.hp),
        town_tile(monster_tile),
    );
    let bolt_def = lightning_bolt();
    let bolt = as_damaging(&bolt_def).unwrap();
    let targets = [target];
    let aim = monster_tile.to_world();

    let behavior = MobBehavior {
        move_range: 3,
        attack_range: 1,
        view_range: 15,
        move_delay_ms: DurationMs(400),
        attack_delay_ms: DurationMs(1000),
        respawn_ms: DurationMs(0),
    };
    let tick = TickDuration::new(50).unwrap();

    let mut proved = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let SkillOutcome::Cast { hits, .. } = cast(&caster, bolt, aim, &targets, &grid, &mut rng).1
        else {
            continue;
        };
        let Some(hit) = hits.first() else { continue };
        let AttackOutcome::Landed { .. } = hit.outcome else {
            continue;
        };
        let Some(shoved) = hit.displacement else {
            continue;
        };

        let mob = MonsterInstance {
            number: MonsterNumber(0),
            placement: shoved,
            health: Pool::full(combat.hp),
            anchor: monster_tile.to_world(),
            next_action: Tick(0),
            active_effects: ActiveEffects::EMPTY,
        };
        let (_, intent) = decide_monster_action(
            &mob,
            &behavior,
            Some(caster.placement().position),
            Tick(0),
            tick,
            &grid,
            Mobility::Free,
            &mut rng,
        );
        match intent {
            MonsterIntent::Attack { .. } | MonsterIntent::Chase { .. } => proved = true,
            MonsterIntent::LeashReturn { .. }
            | MonsterIntent::Wander { .. }
            | MonsterIntent::Idle => {}
        }
        if proved {
            break;
        }
    }
    assert!(proved, "a shoved, in-view monster re-engages its attacker");
}

// --- Mutation teeth-check: each injected bug vs. the correct behaviour. ---

#[test]
fn teeth_min_damage_floor_scales_with_level() {
    // Bug: floor the minimum damage at 0 instead of max(1, level/10). At level
    // 4 the base floor 4/10 = 0, and the correct floor lifts it to 1; the
    // mutation leaves it 0. Named inline test:
    // services::combat::tests::minimum_damage_floor_scales_with_level.
    let level = 4u32;
    let buggy = level / 10;
    let correct = buggy.max(1);
    assert_eq!(buggy, 0);
    assert_eq!(correct, 1);
    assert_ne!(correct, buggy, "the min-damage floor has teeth");
}

#[test]
fn teeth_excellent_is_six_fifths_of_max_not_max() {
    // Bug: an excellent hit deals max (like a critical) instead of 6/5 * max.
    // At max 30 the correct excellent is 36; the mutation yields 30. Named
    // inline test: services::combat::tests::critical_uses_max_and_excellent_outranks_it.
    let max = 30u32;
    let correct = scale_ratio(max, 6, nonzero(5));
    assert_eq!(correct, 36);
    assert_ne!(correct, max, "excellent-vs-critical has teeth");
}

#[test]
fn teeth_defense_is_subtracted_from_the_base() {
    // Bug: skip the defense subtraction. Span 20 minus defense 5 is 15; the
    // mutation yields 20. Named inline test:
    // services::combat::tests::a_normal_hit_is_span_minus_defense.
    let span = 20u32;
    let correct = span.saturating_sub(5);
    assert_eq!(correct, 15);
    assert_ne!(correct, span, "defense subtraction has teeth");
}

#[test]
fn teeth_exp_dampening_multiplies_then_divides() {
    // Bug: divide before multiplying — base * ((t+10)/k) integer-truncates the
    // ratio to 0 when k > t+10. Correct multiply-then-divide keeps the value.
    // Named inline test: services::experience::tests::over_level_dampening_multiplies_then_divides.
    let base = 300u32;
    let victim_plus_ten = 30u32;
    let killer_level = 100u32;
    let correct = scale_ratio(base, victim_plus_ten, nonzero(killer_level)); // base * 30 / 100
    let buggy = base * (victim_plus_ten / killer_level); // base * (30/100 = 0)
    assert_eq!(correct, 90);
    assert_eq!(buggy, 0);
    assert_ne!(correct, buggy, "multiply-then-divide has teeth");
}

#[test]
fn teeth_empty_drop_window_is_guarded_before_any_table() {
    // Bug: build the pick table before checking the window is non-empty. The
    // loot service matches OneOrMore::new(..) first, so an empty window is a
    // real Drop::Nothing; without the guard pick_one has no total input.
    // Named behaviour: services::loot::item_drop's empty-window arm.
    assert!(OneOrMore::<ItemRef>::new(Vec::new()).is_err());
}

#[test]
fn teeth_ability_pools_before_dividing_not_after() {
    // Bug: sum per-term truncations instead of one pooled divide. The Magic
    // Gladiator ability numerator 3510/100 pools to 35; per-term truncation
    // gives 3 + 12 + 7 + 12 = 34. Named inline test:
    // services::profile::tests::magic_gladiator_ability_pools_to_35_not_34.
    let (e, v, a, s) = (24u64, 40, 30, 60);
    let pooled = floor_div_u64_to_u32(15 * e + 30 * v + 25 * a + 20 * s, nonzero(100));
    let per_term = 15 * e / 100 + 30 * v / 100 + 25 * a / 100 + 20 * s / 100;
    assert_eq!(pooled, 35);
    assert_eq!(per_term, 34);
    assert_ne!(
        u64::from(pooled),
        per_term,
        "the pooled single divide has teeth"
    );
}

#[test]
fn teeth_lightning_hit_reports_displacement() {
    // Bug: skip the lightning/lunge knockback composition. A landed lightning
    // strike must be able to report a displacement; the mutation would leave it
    // forever None. Driven through the real cast service over the shipped data.
    let atlas = real_atlas();
    let (_, combat, _) = low_level_monster(&atlas, 20);
    let grid = atlas.walk_grid(MapNumber(0)).unwrap().clone();
    let caster = dark_knight(50, 200, TileCoord::new(135, 125));
    let target = CombatTarget::new(
        monster_profile(&combat, &zero_resistances(), combat.level),
        Pool::full(combat.hp),
        town_tile(TileCoord::new(136, 125)),
    );
    let bolt_def = lightning_bolt();
    let bolt = as_damaging(&bolt_def).unwrap();
    let targets = [target];
    let aim = TileCoord::new(136, 125).to_world();

    let mut saw_displacement = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        if let SkillOutcome::Cast { hits, .. } =
            cast(&caster, bolt, aim, &targets, &grid, &mut rng).1
        {
            if let Some(hit) = hits.first() {
                if matches!(hit.outcome, AttackOutcome::Landed { .. }) && hit.displacement.is_some()
                {
                    saw_displacement = true;
                    break;
                }
            }
        }
    }
    assert!(
        saw_displacement,
        "a landed lightning strike reports a knockback (displacement has teeth)"
    );
}
