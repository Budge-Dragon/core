//! End-to-end timed effects over the real dataset: apply an effect, advance it
//! across ticks, and watch it expire — proving the poison damage-over-time, the
//! buff/heal casts, and the transient effective-profile / mobility derivations
//! compose correctly against the shipped `/data`.
//!
//! The second half is the mutation teeth-check: for each invariant the design
//! guards, a runnable comparison of the correct result against the specific bug
//! that would break it. The poison teeth-check pins the ★caster-scaled★ model —
//! a poison that read the target's HP instead of the caster's energy is caught.

use std::io::Write;
use std::path::PathBuf;

use serde::de::DeserializeOwned;

use mu_core::components::active_effect::{ActiveEffect, ActiveEffects};
use mu_core::components::element::PerElement;
use mu_core::components::movement::{Mobility, Movement, SlowRatio};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, StepMagnitude, UNITS_PER_TILE};
use mu_core::components::stats::Stats;
use mu_core::components::tile::{TileCoord, WalkGrid};
use mu_core::components::units::{MapNumber, Resistance, Tick, TickDuration};
use mu_core::data::ancient_sets::AncientSet;
use mu_core::data::atlas::{Atlas, StaticData};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::classes::ClassRecord;
use mu_core::data::common::DataFile;
use mu_core::data::effects::Ailment;
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_config::GameConfig;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::{MonsterCombat, MonsterDefinition, MonsterRole};
use mu_core::data::npc_shops::MerchantShop;
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::entities::character::Character;
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::movement::StepOutcome;
use mu_core::services::effects::{
    ApplicableBuff, advance_effects, apply_ailment, apply_buff, mobility,
};
use mu_core::services::movement::resolve_step;
use mu_core::services::profile::{character_profile, effective_profile};
use mu_core::services::skills::{HealRef, SkillRouting, cast_heal, route};

// --- Self-contained dataset harness (load failures abort, never unwrap). ---

/// Resolves a `Result` the real checked-in dataset makes infallible; an `Err`
/// here is a broken checkout, not a test condition, so it aborts (no banned
/// suppressor outside a `#[test]` body).
fn or_abort<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "effect_simulation harness: load failure: {error}");
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
        shops: load::<MerchantShop>("npc_shops"),
        classes: load::<ClassRecord>("classes"),
        exp_tables: load::<ExpTable>("exp_tables"),
        game_config: load::<GameConfig>("game_config"),
        terrain: load_terrain(),
    };
    or_abort(Atlas::parse(data))
}

/// The shared simulation tick length: 50 ms.
fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// The cadence in ticks at the shared 50 ms tick length: 3000 ms / 50 = 60.
const POISON_CADENCE_TICKS: u64 = 60;

/// A real gearless Dark Wizard caster at the given energy — the wizardry stat
/// poison and the buff/heal magnitudes scale off.
fn wizard(energy: u16) -> Character {
    let json = serde_json::json!({
        "class": "dark_wizard",
        "level": 80,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 40, "agility": 40, "vitality": 40, "energy": energy},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 163_840, "y": 229_376}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 400, "max": 400},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    });
    or_abort(serde_json::from_value(json))
}

/// The energy stat of a character (the wizardry stat direct spells scale off).
fn energy_of(character: &Character) -> u16 {
    match character.stats() {
        Stats::Standard { energy, .. } | Stats::WithCommand { energy, .. } => energy,
    }
}

/// The first fighting monster's combat block and resistances from the real Atlas.
fn first_monster(atlas: &Atlas) -> (MonsterCombat, PerElement<Resistance>) {
    let found = atlas
        .monsters()
        .find_map(|definition| match &definition.role {
            MonsterRole::Monster {
                combat,
                resistances,
                ..
            }
            | MonsterRole::Guard {
                combat,
                resistances,
                ..
            }
            | MonsterRole::Trap {
                combat,
                resistances,
                ..
            } => Some((*combat, *resistances)),
            MonsterRole::Npc { .. } | MonsterRole::SoccerBall => None,
        });
    or_abort(found.ok_or("the dataset ships fighting monsters"))
}

/// The first heal skill the real dataset ships, routed to a [`HealRef`].
fn heal_skill(atlas: &Atlas) -> HealRef<'_> {
    let found = atlas.skills().find_map(|skill| match route(skill) {
        SkillRouting::Heal(reference) => Some(reference),
        SkillRouting::Damaging(_) | SkillRouting::Buff(_) | SkillRouting::Deferred => None,
    });
    or_abort(found.ok_or("the dataset ships a heal skill"))
}

/// The resolved per-tick poison damage a caster inflicts, read off the applied
/// effect.
fn poison_per_tick(caster: &Character) -> u32 {
    let (_, applied) = apply_ailment(
        Ailment::Poisoned,
        energy_of(caster),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let per_tick = match applied {
        ActiveEffect::Poisoned {
            per_tick_damage, ..
        } => Some(per_tick_damage),
        ActiveEffect::Defense { .. }
        | ActiveEffect::GreaterDamage { .. }
        | ActiveEffect::GreaterDefense { .. }
        | ActiveEffect::Iced { .. }
        | ActiveEffect::Frozen { .. }
        | ActiveEffect::DefenseReduction { .. } => None,
    };
    or_abort(per_tick.ok_or("poison ailment must apply poison"))
}

#[test]
fn poison_drops_a_real_monster_monotonically_by_exactly_per_tick() {
    let atlas = real_atlas();
    let (combat, _) = first_monster(&atlas);
    let caster = wizard(120);
    let per_tick = poison_per_tick(&caster);

    // A pool large enough to survive the whole six-tick stream.
    let max_health = per_tick.saturating_mul(20).max(combat.hp);
    let (mut store, _) = apply_ailment(
        Ailment::Poisoned,
        energy_of(&caster),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let mut health = Pool::full(max_health);
    let start = health.current();

    let mut fired = 0;
    let mut prev = health.current();
    for step in 1..=6u64 {
        let now = Tick(POISON_CADENCE_TICKS * step);
        let (next_store, next_health, events) = advance_effects(store, health, now);
        let ticks = events
            .iter()
            .filter(|event| matches!(event, EffectEvent::PoisonTick { .. }))
            .count();
        assert_eq!(
            ticks, 1,
            "exactly one tick fires per cadence at step {step}"
        );
        // Health drops by exactly the per-tick magnitude, monotonically.
        assert_eq!(prev - next_health.current(), per_tick, "step {step}");
        prev = next_health.current();
        fired += ticks;
        store = next_store;
        health = next_health;
    }
    assert_eq!(fired, 6, "six ticks total, never a seventh");
    assert_eq!(start - health.current(), per_tick.saturating_mul(6));
    assert!(store.poison().is_none(), "poison self-terminates");
}

#[test]
fn a_strong_casters_poison_kills_where_a_weak_casters_does_less() {
    let strong = wizard(500);
    let weak = wizard(10);
    let strong_tick = poison_per_tick(&strong);
    let weak_tick = poison_per_tick(&weak);
    // ★POISON★ caster-scaling: stronger caster, stronger poison.
    assert!(strong_tick > weak_tick);

    // The strong caster's poison finishes a low-HP target within its stream.
    let (store, _) = apply_ailment(
        Ailment::Poisoned,
        energy_of(&strong),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let frail = Pool::full(strong_tick + 1);
    let (after, health, events) = advance_effects(store, frail, Tick(10_000));
    assert_eq!(health.current(), 0);
    assert_eq!(after, ActiveEffects::EMPTY, "death clears every effect");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EffectEvent::PoisonKilled { .. }))
    );

    // The same frail-sized target survives the weak caster's whole stream.
    let (weak_store, _) = apply_ailment(
        Ailment::Poisoned,
        energy_of(&weak),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let survivor = Pool::full(weak_tick.saturating_mul(6) + 1);
    let (_, weak_health, _) = advance_effects(weak_store, survivor, Tick(10_000));
    assert!(weak_health.current() > 0, "a weak caster's poison is weak");
}

#[test]
fn a_greater_damage_buff_makes_the_effective_profile_hit_harder() {
    let caster = wizard(70);
    let base = character_profile(&caster).0;
    let (store, _) = apply_buff(
        ApplicableBuff::GreaterDamage,
        energy_of(&caster),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let buffed = effective_profile(base, &store);
    // Greater Damage folds a flat post-defense add and leaves the physical span
    // untouched, so crit/excellent (which read the span) never amplify the buff.
    assert!(buffed.flat_damage_add() > base.flat_damage_add());
    assert_eq!(buffed.physical(), base.physical());
    // An unaffected profile is byte-identical to the base (empty fold identity).
    assert_eq!(effective_profile(base, &ActiveEffects::EMPTY), base);
}

#[test]
fn iced_halves_a_step_and_frozen_blocks_it() {
    let iced = ActiveEffects::EMPTY.with(ActiveEffect::Iced { expiry: Tick(600) });
    let frozen = ActiveEffects::EMPTY.with(ActiveEffect::Frozen { expiry: Tick(600) });

    // Iced confers the half-speed slow ratio; Frozen immobilizes; an empty store
    // leaves movement free.
    assert_eq!(mobility(&ActiveEffects::EMPTY), Mobility::Free);
    assert_eq!(mobility(&frozen), Mobility::Immobilized);
    let ratio = match mobility(&iced) {
        Mobility::Slowed { ratio } => ratio,
        Mobility::Free | Mobility::Immobilized => panic!("iced must slow"),
    };
    assert_eq!(ratio, SlowRatio::HALVED);

    // The half-speed ratio applied to a one-tile base carries into the movement
    // service as a half-tile step.
    let iced_speed = StepMagnitude::tile_fraction(1, std::num::NonZeroU32::new(2).unwrap());
    let grid = WalkGrid::from_words([u64::MAX; 1024]);
    let start = Placement {
        position: TileCoord::new(10, 10).to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: MapNumber(0),
    };
    let far = TileCoord::new(40, 10).to_world();
    let moved = match resolve_step(start, far, iced_speed, &grid) {
        StepOutcome::Resolved { placement } => placement.position,
        StepOutcome::Blocked => panic!("all-walkable never blocks"),
    };
    assert_eq!(
        moved.x().raw() - start.position.x().raw(),
        UNITS_PER_TILE / 2
    );
}

// --- Mutation teeth-check: each injected bug vs. the correct behaviour. ---

#[test]
fn teeth_poison_fires_six_ticks_not_seven() {
    let caster = wizard(120);
    let per_tick = poison_per_tick(&caster);
    let (store, _) = apply_ailment(
        Ailment::Poisoned,
        energy_of(&caster),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let pool = Pool::full(per_tick.saturating_mul(50));
    let (_, health, _) = advance_effects(store, pool, Tick(100_000));
    let total = pool.current() - health.current();
    // Correct: 6 ticks. The off-by-one bug (a seventh tick) would be 7×per_tick.
    assert_eq!(total, per_tick.saturating_mul(6));
    assert_ne!(total, per_tick.saturating_mul(7));
}

#[test]
fn teeth_heal_is_bounded_by_max_not_a_raw_add() {
    let atlas = real_atlas();
    let heal = heal_skill(&atlas);
    let caster = wizard(30); // heal = 5 + 30/5 = 11
    let nearly_full = Pool::new(98, 100).unwrap();
    let (_, outcome) = cast_heal(&caster, heal, nearly_full);
    let restored = match outcome {
        BuffCastOutcome::Healed { amount } => amount,
        BuffCastOutcome::Rejected { .. } | BuffCastOutcome::Applied { .. } => {
            panic!("a free heal applies")
        }
    };
    // Correct: only 2 of 11 fit under the max. A raw add would report 11.
    assert_eq!(restored, 2);
    assert_ne!(restored, 11);
}

#[test]
fn teeth_iced_slows_rather_than_being_ignored() {
    let iced = ActiveEffects::EMPTY.with(ActiveEffect::Iced { expiry: Tick(600) });
    // Correct: iced slows. A bug that ignored the factor would leave it Free.
    assert_ne!(mobility(&iced), Mobility::Free);
    match mobility(&iced) {
        Mobility::Slowed { ratio } => {
            assert_eq!(ratio, SlowRatio::HALVED);
            // A genuine slow: the fraction is strictly below one.
            assert!(ratio.num() < ratio.den().get());
        }
        Mobility::Free | Mobility::Immobilized => panic!("iced must slow"),
    }
}

#[test]
fn teeth_reapplying_a_buff_refreshes_rather_than_doubling_the_fold() {
    let caster = wizard(70);
    let base = character_profile(&caster).0;
    let (once, _) = apply_buff(
        ApplicableBuff::GreaterDamage,
        energy_of(&caster),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let (twice, _) = apply_buff(
        ApplicableBuff::GreaterDamage,
        energy_of(&caster),
        once,
        Tick(100),
        tick(),
    );
    let single = effective_profile(base, &once).flat_damage_add();
    let reapplied = effective_profile(base, &twice).flat_damage_add();
    // Correct: one slot, one fold — reapplying does not stack. An appended
    // duplicate would fold the flat add twice.
    assert_eq!(single, reapplied);
    assert!(single > 0, "the buff folds a positive flat add");
    assert_ne!(reapplied, single.saturating_mul(2));
}

#[test]
fn teeth_poison_scales_off_the_caster_not_the_target_hp() {
    // The reverted bug: poison = 3% of the TARGET's max HP, so two casters on the
    // same target would inflict identical per-tick damage. Caster-scaling makes
    // them differ — that is what this asserts.
    let weak = wizard(10);
    let strong = wizard(400);
    assert!(
        poison_per_tick(&strong) > poison_per_tick(&weak),
        "per-tick must scale with caster energy, not target HP"
    );
    // The applied per-tick is independent of the target it is advanced against —
    // it is fixed at apply from the caster, so a small and a large pool take the
    // identical first-tick damage.
    let (store, _) = apply_ailment(
        Ailment::Poisoned,
        energy_of(&strong),
        ActiveEffects::EMPTY,
        Tick(0),
        tick(),
    );
    let per_tick = poison_per_tick(&strong);
    let small = Pool::full(per_tick.saturating_mul(20));
    let large = Pool::full(per_tick.saturating_mul(2000));
    let (_, small_after, _) = advance_effects(store, small, Tick(POISON_CADENCE_TICKS));
    let (_, large_after, _) = advance_effects(store, large, Tick(POISON_CADENCE_TICKS));
    assert_eq!(
        small.current() - small_after.current(),
        large.current() - large_after.current(),
        "the first tick's damage is the same regardless of the target's HP"
    );
}
