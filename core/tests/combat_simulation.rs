//! End-to-end combat over the real dataset: derive a real character profile,
//! pick a real monster, beat it to zero health with the combat service, then
//! award its kill through the loot/experience/kill orchestrator — proving the
//! composed pipeline resolves a finite, well-formed, bit-for-bit replayable kill
//! against the shipped `/data`.
//!
//! The second half pins the reward services to hand-derived golden expectations:
//! experience bands computed from the rule statement, drop windows and plus
//! levels computed from the item dataset, and the money-drop coupling — every
//! oracle is derived by hand from the rules and the data, never re-transcribed
//! from a production expression.
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
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::movement::{Mobility, Movement};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::Facing;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{
    DurationMs, Exp, ItemLevel, Level, MapNumber, Resistance, Tick, TickDuration, Zen,
};
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
use mu_core::events::kill::KillResolution;
use mu_core::events::loot::{Drop, DropResolution};
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::progression::ExpAward;
use mu_core::events::skills::SkillOutcome;
use mu_core::services::combat::resolve_attack;
use mu_core::services::experience::award_kill_experience;
use mu_core::services::kill::resolve_kill;
use mu_core::services::loot::resolve_kill_drops;
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::profile::{character_profile, monster_profile};
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

// --- Experience contracts: hand-derived golden bands over the real curve. ---

#[test]
fn exp_low_level_award_is_the_base_scaled_band() {
    // Hand-derived from the rule statement: a level-30 victim gives base
    // (30 + 25) * 30 / 3 = 550; a level-30 killer is within the ten-level gap
    // (no dampening) and the victim is below 65 (no bonus); the 5/4 era factor
    // lifts it to 687; the authored 80..=120 percent jitter bounds the award
    // to [549, 824].
    let atlas = real_atlas();
    let killer = dark_knight(30, 150, TileCoord::new(10, 10));
    let (gained, level_ups) = award_kill_experience(
        &killer,
        Level::new(30).unwrap(),
        &atlas,
        &mut TestRng::new(7),
    );
    assert!(
        (549..=824).contains(&gained.0),
        "a level-30 victim awards within the hand-derived band, got {}",
        gained.0
    );
    // Level 31 requires a 351_000 total on the shipped curve — far above any
    // single award in the band, so a level-30 killer with zero experience
    // cannot level from this kill.
    assert!(level_ups.is_empty());
}

#[test]
fn exp_over_level_dampening_keeps_a_positive_award() {
    // Hand-derived: a level-20 victim gives base (20 + 25) * 20 / 3 = 300; a
    // level-100 killer exceeds the ten-level gap, so the award dampens to
    // 300 * 30 / 100 = 90; the 5/4 factor lifts it to 112; jitter bounds it to
    // [89, 134]. Load-bearing against a divide-first mutation: 30 / 100
    // truncates to zero, collapsing the whole award to 0, below the band.
    let atlas = real_atlas();
    let killer = dark_knight(100, 150, TileCoord::new(10, 10));
    let (gained, _) = award_kill_experience(
        &killer,
        Level::new(20).unwrap(),
        &atlas,
        &mut TestRng::new(7),
    );
    assert!(
        (89..=134).contains(&gained.0),
        "an over-leveled kill still awards within the dampened band, got {}",
        gained.0
    );
}

#[test]
fn exp_high_level_victim_award_exercises_the_bonus_path() {
    // Path exerciser, not a bonus-term certifier: a level-108 victim (the real
    // monster #77's level) gives base (108 + 25) * 108 / 3 = 4788, plus the
    // high-level bonus (108 - 64) * (108 / 4) = 1188 = 5976; no dampening
    // (killer 108 is within the gap); the 5/4 factor lifts it to 7470; jitter
    // bounds it to [5976, 8964]. The no-bonus band [4788, 7182] overlaps this
    // one, so a bonus-drop mutation is NOT guaranteed to redden here — no real
    // (<= 108) victim level separates the bands once jitter applies.
    let atlas = real_atlas();
    let killer = dark_knight(108, 150, TileCoord::new(10, 10));
    let (gained, _) = award_kill_experience(
        &killer,
        Level::new(108).unwrap(),
        &atlas,
        &mut TestRng::new(7),
    );
    assert!(
        (5976..=8964).contains(&gained.0),
        "a high-level victim awards within the with-bonus band, got {}",
        gained.0
    );
}

#[test]
fn a_kill_that_crosses_multiple_levels_lists_them_ascending() {
    // Hand-derived: monster #37 is a real level-60 fighter, so a level-1 killer
    // with zero experience gains base (60 + 25) * 60 / 3 = 1700, undampened and
    // unbonused, scaled 5/4 to 2125, jittered to [1700, 2550]. On the shipped
    // curve the totals to hold are L2=100, L3=440, L4=1080, L5=2080, L6=3500,
    // so every jitter crosses levels 2, 3, and 4, at most level 5, never 6 —
    // the level-ups list is contiguous and ascending from level 2.
    let atlas = real_atlas();
    let killer = dark_knight(1, 150, TileCoord::new(10, 10));
    let victim = victim_instance(MonsterNumber(37), 5000);
    let mut rng = TestRng::new(7);
    let resolution = resolve_kill(&killer, &victim, &atlas, &mut rng);

    let gained = resolution.experience.gained.0;
    assert!(
        (1700..=2550).contains(&gained),
        "a level-60 victim awards within the hand-derived band, got {gained}"
    );
    assert!(
        resolution.level_ups.len() >= 3,
        "every jitter crosses at least levels 2, 3, and 4, got {}",
        resolution.level_ups.len()
    );
    for (expected_level, level_up) in (2u16..).zip(resolution.level_ups.iter()) {
        assert_eq!(
            level_up.level.get(),
            expected_level,
            "level-ups ascend contiguously from level 2"
        );
    }
}

// --- Loot contracts: hand-derived goldens over the real item dataset. ---

#[test]
fn a_money_drop_is_the_awarded_experience_plus_seven() {
    // The awarded experience is chosen by hand (1000); the base money bonus
    // from the rule statement is 7, so a money drop carries exactly 1007 zen.
    // Monster #24 is a real level-20 fighter; the money category is a 50%
    // roll, so a 64-seed search always observes it.
    let atlas = real_atlas();
    let victim = victim_instance(MonsterNumber(24), 600);
    let victim_level = Level::new(20).unwrap();

    let mut found = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let resolution = resolve_kill_drops(&victim, victim_level, Exp(1000), &atlas, &mut rng);
        if let Drop::Zen { amount } = resolution.category {
            assert_eq!(
                amount,
                Zen(1007),
                "a money drop is the awarded experience plus seven"
            );
            found = true;
            break;
        }
    }
    assert!(found, "the 50% money category lands within 64 seeds");
}

#[test]
fn resolve_kill_threads_the_awarded_experience_into_the_money_drop() {
    // Orchestrator coupling: the money drop must ride the experience the same
    // resolution reports as gained. Hand-derived band for victim level 20 and
    // killer level 30: base 300, no dampening (30 does not exceed 20 + 10),
    // scaled 375, jittered to [300, 450].
    let atlas = real_atlas();
    let killer = dark_knight(30, 150, TileCoord::new(10, 10));
    let victim = victim_instance(MonsterNumber(24), 600);

    let mut found = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let resolution = resolve_kill(&killer, &victim, &atlas, &mut rng);
        if let Drop::Zen { amount } = resolution.drops.category {
            let gained = resolution.experience.gained.0;
            assert!(
                (300..=450).contains(&gained),
                "the gained experience sits in the hand-derived band, got {gained}"
            );
            assert_eq!(
                amount.0,
                gained + 7,
                "the money drop is the resolution's own gained experience plus seven"
            );
            found = true;
            break;
        }
    }
    assert!(found, "the 50% money category lands within 64 seeds");
}

#[test]
fn an_item_drops_plus_level_comes_from_the_monster_level_window() {
    // Hand-derived from the item dataset: the [89, 100] window below a
    // level-100 monster (real monster #75) holds exactly ONE droppable item —
    // the staff (group 5, number 8), drop level 90, max item level 11 — so the
    // pick is seed-independent. Ten levels above its drop level thirds down to
    // plus 3, below the cap. The item category is a 30% roll.
    let atlas = real_atlas();
    let victim = victim_instance(MonsterNumber(75), 50_000);
    let victim_level = Level::new(100).unwrap();
    let expected = Drop::Item {
        item: ItemRef {
            group: 5,
            number: 8,
        },
        level: ItemLevel::new(3).unwrap(),
        rarity: ItemRarity::Normal,
    };

    let mut found = false;
    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let resolution = resolve_kill_drops(&victim, victim_level, Exp(0), &atlas, &mut rng);
        if let Drop::Item { .. } = resolution.category {
            assert_eq!(
                resolution.category, expected,
                "the window's single droppable item falls at plus 3"
            );
            found = true;
            break;
        }
    }
    assert!(found, "the 30% item category lands within 64 seeds");
}

#[test]
fn an_item_drops_plus_is_capped_at_its_max_item_level() {
    // Hand-derived from the item dataset: the [85, 96] window below a level-96
    // monster (real monster #72) holds two droppable items — the skill scroll
    // (group 15, number 13), drop level 88, max item level 0, and the staff
    // (group 5, number 8). Eight levels above the scroll's drop level thirds
    // to 2 uncapped, so a returned plus of 0 can only be the cap binding.
    let atlas = real_atlas();
    let victim = victim_instance(MonsterNumber(72), 41_000);
    let victim_level = Level::new(96).unwrap();
    let scroll = ItemRef {
        group: 15,
        number: 13,
    };

    let mut found = false;
    for seed in 0u64..512 {
        let mut rng = TestRng::new(seed);
        let resolution = resolve_kill_drops(&victim, victim_level, Exp(0), &atlas, &mut rng);
        if let Drop::Item { item, level, .. } = resolution.category {
            if item == scroll {
                assert_eq!(
                    level,
                    ItemLevel::ZERO,
                    "the scroll's plus level clamps to its max item level"
                );
                found = true;
                break;
            }
        }
    }
    assert!(found, "the scroll pick lands within 512 seeds");
}

#[test]
fn a_passive_victim_yields_no_reward_and_draws_no_randomness() {
    // Monster #200 is the real soccer ball and #235 a real quest NPC — neither
    // carries a combat block, so a kill yields the zero reward and returns
    // before any roll: the RNG stream must be untouched.
    let atlas = real_atlas();
    let killer = dark_knight(30, 150, TileCoord::new(10, 10));
    let zero_reward = KillResolution {
        drops: DropResolution {
            category: Drop::Nothing,
            specials: Vec::new(),
        },
        experience: ExpAward { gained: Exp(0) },
        level_ups: Vec::new(),
    };

    for number in [MonsterNumber(200), MonsterNumber(235)] {
        let victim = victim_instance(number, 1);
        let mut rng = TestRng::new(11);
        let resolution = resolve_kill(&killer, &victim, &atlas, &mut rng);
        assert_eq!(resolution, zero_reward);
        assert_eq!(
            rng.next_u64(),
            TestRng::new(11).next_u64(),
            "a passive-victim kill consumes no randomness"
        );
    }
}

// --- Skill and drop-window guards over the shipped data. ---

#[test]
fn teeth_empty_drop_window_is_guarded_before_any_table() {
    // Bug: build the pick table before checking the window is non-empty. The
    // loot service matches OneOrMore::new(..) first, so an empty window is a
    // real Drop::Nothing; without the guard pick_one has no total input.
    // Named behaviour: services::loot::item_drop's empty-window arm.
    assert!(OneOrMore::<ItemRef>::new(Vec::new()).is_err());
}

#[test]
fn a_landed_lightning_strike_reports_a_knockback() {
    // Drives the real cast service over the shipped data: a landed lightning
    // strike must be able to report a displacement. A mutation dropping the
    // knockback composition would leave it forever None.
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
        "a landed lightning strike reports a knockback"
    );
}
