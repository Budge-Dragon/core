//! End-to-end, multi-tick simulation of the W-MOV wave against the real
//! dataset. Where `data_files.rs` proves each movement service in isolation,
//! this suite drives them *composed* — population, monster AI, flight, warp
//! arrival, and grounded stepping — over whole runs and asserts invariants that
//! only a running world can exhibit.
//!
//! The invariants are labelled I1..I12 after the test-design doc:
//! - I1/I1b delta walkability, I3 determinism, I4a/I4b cadence, I5 identity —
//!   over the whole-world ambient simulation (Group A).
//! - I6/I6' chase convergence and no-regression, I7 leash, I8 attack intent —
//!   over hand-built chase/leash/attack scenarios (Group B).
//! - I9 flight toggle chain, I10 Sky forces flight, I11 warp-arrival-then-step —
//!   over flight/warp/step integration on real maps (Group C).
//! - I12 a proptest sweep of delta walkability and same-seed determinism over a
//!   sampled map (Group D).

mod common;

use common::{Frame, ONE_TILE, TestRng, behaviors_by_number, real_atlas, simulate, tick};

use std::collections::BTreeMap;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use mu_core::components::movement::{CombatLock, FlightChange, Movement, Wings};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Facing, Radius, UNITS_PER_TILE, WorldPos};
use mu_core::components::tile::{TileArea, TileCoord, WalkGrid};
use mu_core::components::units::{DurationMs, MapNumber, Tick};
use mu_core::data::atlas::{Atlas, Landing};
use mu_core::data::common::MonsterNumber;
use mu_core::data::map_definitions::MapEnvironment;
use mu_core::data::monster_definitions::MobBehavior;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightDenialReason, FlightOutcome, StepOutcome, WarpOutcome};
use mu_core::services::movement::{change_flight, resolve_arrival, resolve_step};

/// Whole-run length for the ambient simulation (Group A).
const N: u64 = 200;
/// The pinned ambient seed the deterministic runs replay.
const AMBIENT_SEED: u64 = 0x00C0_FFEE;
/// A second, diverging seed for the determinism inequality check.
const OTHER_SEED: u64 = 0x00DD_BA11;

// --- Group A: whole-world ambient simulation over all 11 real maps.

/// I1, I1b, I4a, I5, asserted for a single tick/mob frame.
fn assert_frame_invariants(frame: &Frame) {
    // I1: a grounded move never carries a mob off a walkable tile.
    assert!(
        !frame.before_walkable || frame.after_walkable,
        "I1 delta walkability violated at tick {}",
        frame.tick
    );
    // I1b: a mob spawned on a walkable tile stays on one.
    assert!(
        !frame.anchor_walkable || frame.after_walkable,
        "I1b spawn-walkable mob left the walkable set at tick {}",
        frame.tick
    );
    // I4a: a throttled mob does nothing and reports Idle.
    if !frame.before.next_action.reached(Tick(frame.tick)) {
        assert_eq!(
            frame.after, frame.before,
            "I4a throttled tick mutated the mob"
        );
        assert_eq!(
            frame.intent,
            MonsterIntent::Idle,
            "I4a throttled tick was not Idle"
        );
    }
    // I5: a decision never rewrites identity, anchor, or health.
    assert_eq!(frame.after.anchor, frame.before.anchor, "I5 anchor changed");
    assert_eq!(frame.after.number, frame.before.number, "I5 number changed");
    assert_eq!(frame.after.health, frame.before.health, "I5 health changed");
}

/// I4b (cadence count, moving mobs only) and the whole-run half of I5
/// (anchor/health equal their spawn values every tick).
fn assert_cadence_and_spawn_identity(
    frames: &[Frame],
    behaviors: &BTreeMap<MonsterNumber, MobBehavior>,
) {
    let mut spawn: BTreeMap<(MapNumber, usize), (WorldPos, Pool)> = BTreeMap::new();
    let mut non_idle: BTreeMap<(MapNumber, usize), u64> = BTreeMap::new();
    let mut periods: BTreeMap<(MapNumber, usize), u64> = BTreeMap::new();

    for frame in frames {
        let key = (frame.before.placement.map, frame.mob_index);
        if frame.tick == 0 {
            spawn.insert(key, (frame.before.anchor, frame.before.health));
        }
        let Some(&(anchor, health)) = spawn.get(&key) else {
            continue;
        };
        assert_eq!(frame.after.anchor, anchor, "I5 anchor left its spawn value");
        assert_eq!(frame.after.health, health, "I5 health left its spawn value");

        let Some(behavior) = behaviors.get(&frame.before.number) else {
            continue;
        };
        if behavior.move_range == 0 || behavior.move_delay_ms.0 == 0 {
            continue;
        }
        let period = behavior.move_delay_ms.in_ticks(tick()).0;
        periods.insert(key, period);
        if frame.intent != MonsterIntent::Idle {
            assert_eq!(frame.tick % period, 0, "I4b acted off its cadence");
            *non_idle.entry(key).or_insert(0) += 1;
        }
    }

    for (key, period) in periods {
        let expected = (N - 1) / period + 1;
        let got = non_idle.get(&key).copied().unwrap_or(0);
        assert_eq!(got, expected, "I4b cadence count wrong for {key:?}");
    }
}

#[test]
fn whole_world_ambient_holds_invariants() {
    let atlas = real_atlas();
    let behaviors = behaviors_by_number(&atlas);
    let handles: Vec<_> = atlas.map_handles().collect();
    let frames = simulate(&handles, &behaviors, AMBIENT_SEED, N);
    for frame in &frames {
        assert_frame_invariants(frame);
    }
    assert_cadence_and_spawn_identity(&frames, &behaviors);
}

#[test]
fn whole_world_run_is_deterministic() {
    let atlas = real_atlas();
    let behaviors = behaviors_by_number(&atlas);
    let handles: Vec<_> = atlas.map_handles().collect();
    let run = |seed| simulate(&handles, &behaviors, seed, N);
    // I3: same seed is bit-identical; diverging seeds diverge.
    assert_eq!(
        run(AMBIENT_SEED),
        run(AMBIENT_SEED),
        "I3 same seed must replay"
    );
    assert_ne!(
        run(AMBIENT_SEED),
        run(OTHER_SEED),
        "I3 diverging seeds must differ"
    );
}

// --- Group B: chase / leash / attack over hand-built mobs.

/// One driven action: the tick it fired at, the mob before and after, and the
/// intent chosen.
struct Action {
    now: Tick,
    before: MonsterInstance,
    after: MonsterInstance,
    intent: MonsterIntent,
}

fn all_walkable() -> WalkGrid {
    WalkGrid::from_words([u64::MAX; 1024])
}

fn behavior(
    move_range: u8,
    attack_range: u8,
    view_range: u8,
    move_ms: u32,
    attack_ms: u32,
) -> MobBehavior {
    MobBehavior {
        move_range,
        attack_range,
        view_range,
        move_delay_ms: DurationMs(move_ms),
        attack_delay_ms: DurationMs(attack_ms),
        respawn_ms: DurationMs(0),
    }
}

fn mob_from(position: WorldPos, anchor: WorldPos, map: MapNumber) -> MonsterInstance {
    MonsterInstance {
        number: MonsterNumber(7),
        placement: Placement {
            position,
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map,
        },
        health: Pool::full(60),
        anchor,
        next_action: Tick(0),
    }
}

fn mob_at(tile: (u8, u8), anchor: (u8, u8), map: MapNumber) -> MonsterInstance {
    mob_from(
        TileCoord::new(tile.0, tile.1).to_world(),
        TileCoord::new(anchor.0, anchor.1).to_world(),
        map,
    )
}

/// Drives one mob for `steps` actions, jumping the clock to each action's
/// cadence tick so every step is a real decision (never a throttle).
fn drive(
    mut inst: MonsterInstance,
    behavior: &MobBehavior,
    target: Option<WorldPos>,
    grid: &WalkGrid,
    steps: usize,
) -> Vec<Action> {
    let mut rng = TestRng::new(1);
    let mut actions = Vec::new();
    for _ in 0..steps {
        let now = inst.next_action;
        let (after, intent) = decide(&inst, behavior, target, now, grid, &mut rng);
        actions.push(Action {
            now,
            before: inst,
            after,
            intent,
        });
        inst = after;
    }
    actions
}

/// Thin wrapper over the AI decision at the shared tick length.
fn decide(
    inst: &MonsterInstance,
    behavior: &MobBehavior,
    target: Option<WorldPos>,
    now: Tick,
    grid: &WalkGrid,
    rng: &mut TestRng,
) -> (MonsterInstance, MonsterIntent) {
    mu_core::services::monster_ai::decide_monster_action(
        inst,
        behavior,
        target,
        now,
        tick(),
        grid,
        rng,
    )
}

#[test]
fn i6_chase_converges_on_open_terrain() {
    let grid = all_walkable();
    let target = TileCoord::new(16, 10).to_world();
    let mob = mob_at((10, 10), (10, 10), MapNumber(0));
    let actions = drive(mob, &behavior(1, 1, 8, 400, 1000), Some(target), &grid, 12);

    let mut first_attack = None;
    for (index, action) in actions.iter().enumerate() {
        match action.intent {
            MonsterIntent::Chase { .. } => {
                assert!(
                    first_attack.is_none(),
                    "I6 chase resumed after attack range"
                );
                assert!(
                    action.after.placement.position.distance_sq(target)
                        < action.before.placement.position.distance_sq(target),
                    "I6 chase must strictly close the distance"
                );
            }
            MonsterIntent::Attack { target: struck } => {
                assert_eq!(struck, target);
                assert_eq!(
                    action.after.placement.position, action.before.placement.position,
                    "I6 attack must not move the mob"
                );
                if first_attack.is_none() {
                    first_attack = Some(index);
                }
            }
            MonsterIntent::Wander { .. }
            | MonsterIntent::LeashReturn { .. }
            | MonsterIntent::Idle => {
                panic!("I6 unexpected intent {:?}", action.intent)
            }
        }
    }

    let index = first_attack.expect("I6 the mob must reach attack range");
    // D = 6 tiles, attack_range = 1: bound (D - attack_range) + 2 = 7 actions.
    assert!(
        index <= (6 - 1) + 2,
        "I6 attack must arrive within the bound"
    );
    assert!(
        actions[index]
            .before
            .placement
            .position
            .within_range(target, Radius::from_tiles(1)),
        "I6 attack fires from within attack range"
    );
}

#[test]
fn i6_arrival_clamp_lands_exactly() {
    let grid = all_walkable();
    let target = TileCoord::new(16, 10).to_world();
    let mob = mob_at((10, 10), (10, 10), MapNumber(0));
    let actions = drive(mob, &behavior(1, 0, 8, 400, 1000), Some(target), &grid, 12);

    let mut landed = false;
    for action in &actions {
        assert!(
            action.after.placement.position.x().raw() <= target.x().raw(),
            "I6 arrival clamp must never overshoot"
        );
        if let MonsterIntent::Chase { .. } = action.intent {
            assert!(
                action.after.placement.position.distance_sq(target)
                    < action.before.placement.position.distance_sq(target),
                "I6 chase must strictly close the distance"
            );
        }
        if action.after.placement.position == target {
            landed = true;
        }
    }

    assert!(landed, "I6 arrival clamp must land exactly on the target");
    let last = actions.last().expect("driven actions");
    assert!(matches!(last.intent, MonsterIntent::Attack { .. }));
    assert_eq!(
        last.after.placement.position, target,
        "I6 stays exactly on target"
    );
}

#[test]
fn i7_leash_keeps_the_mob_bounded() {
    let grid = all_walkable();
    let anchor = TileCoord::new(10, 10).to_world();
    let target = TileCoord::new(14, 10).to_world();
    let mob = mob_at((13, 10), (10, 10), MapNumber(0));
    let actions = drive(mob, &behavior(1, 0, 3, 400, 1000), Some(target), &grid, 40);

    let tile = UNITS_PER_TILE.unsigned_abs();
    let bound = 3 * tile + tile + 2;
    let mut leashed = false;
    for action in &actions {
        let distance = action.after.placement.position.distance_sq(anchor).isqrt();
        assert!(distance <= bound, "I7 mob strayed past the leash bound");
        if let MonsterIntent::LeashReturn { .. } = action.intent {
            leashed = true;
            assert!(
                action.after.placement.position.distance_sq(anchor)
                    <= action.before.placement.position.distance_sq(anchor),
                "I7 a leash return must not increase distance to the anchor"
            );
        }
    }
    assert!(leashed, "I7 at least one leash return must fire");
}

#[test]
fn i8_attack_is_intent_only() {
    let grid = all_walkable();
    let target = TileCoord::new(11, 10).to_world();
    let mob = mob_at((10, 10), (10, 10), MapNumber(0));
    let actions = drive(mob, &behavior(1, 2, 5, 400, 1000), Some(target), &grid, 10);

    let attack_delay = DurationMs(1000).in_ticks(tick());
    for action in &actions {
        assert_eq!(action.intent, MonsterIntent::Attack { target });
        assert_eq!(
            action.after.placement.position, action.before.placement.position,
            "I8 attack must not move the mob"
        );
        assert!(
            action.after.placement.facing.vector().x().raw() > 0,
            "I8 must face the target"
        );
        assert_eq!(action.after.placement.facing.vector().y().raw(), 0);
        assert_eq!(
            action.after.next_action,
            action.now + attack_delay,
            "I8 attack cadence"
        );
    }
}

/// I6': a start/target pair on real terrain, both walkable, target within view.
fn pick_chase_pair(grid: &WalkGrid) -> Option<(WorldPos, WorldPos)> {
    let offsets = [(5i32, 0i32), (0, 5), (-5, 0), (0, -5), (4, 3), (3, 4)];
    for sy in 0u8..=255 {
        for sx in 0u8..=255 {
            let start = TileCoord::new(sx, sy);
            if !grid.walkable(start.to_world()) {
                continue;
            }
            for (dx, dy) in offsets {
                let (Ok(tx), Ok(ty)) = (
                    u8::try_from(i32::from(sx) + dx),
                    u8::try_from(i32::from(sy) + dy),
                ) else {
                    continue;
                };
                let target = TileCoord::new(tx, ty);
                if target != start && grid.walkable(target.to_world()) {
                    return Some((start.to_world(), target.to_world()));
                }
            }
        }
    }
    None
}

#[test]
fn i6_prime_chase_no_regression_on_real_terrain() {
    let atlas = real_atlas();
    let grid = atlas.walk_grid(MapNumber(8)).unwrap();
    let (start, target) = pick_chase_pair(grid).expect("map 8 has a walkable chase pair");
    let mob = mob_from(start, start, MapNumber(8));
    let actions = drive(mob, &behavior(1, 1, 10, 400, 1000), Some(target), grid, 15);

    let mut chased = false;
    for action in &actions {
        let MonsterIntent::Chase { .. } = action.intent else {
            continue;
        };
        chased = true;
        assert!(
            action.after.placement.position.distance_sq(target)
                <= action.before.placement.position.distance_sq(target),
            "I6' distance to the target must be non-increasing"
        );
        if grid.walkable(action.before.placement.position) {
            assert!(
                grid.walkable(action.after.placement.position),
                "I6' visited tiles stay walkable"
            );
        }
        let step = (action.after.placement.position - action.before.placement.position)
            .length_sq()
            .isqrt();
        assert!(
            step <= UNITS_PER_TILE.unsigned_abs() + 1,
            "I6' a step is at most one tile"
        );
    }
    assert!(chased, "I6' the mob must actually chase");
}

// --- Group C: flight + warp + step integration on real maps.

#[test]
fn i9_flight_toggle_chain_on_ground_map() {
    let atlas = real_atlas();
    let env = atlas
        .map_handle(MapNumber(0))
        .unwrap()
        .definition()
        .environment;
    assert_eq!(env, MapEnvironment::Ground);

    let mut movement = Movement::Grounded;
    let chain = [
        (
            FlightChange::EnableFlight,
            Movement::Flying,
            Some(Movement::Flying),
        ),
        (FlightChange::EnableFlight, Movement::Flying, None),
        (
            FlightChange::DisableFlight,
            Movement::Grounded,
            Some(Movement::Grounded),
        ),
        (FlightChange::DisableFlight, Movement::Grounded, None),
    ];
    for (change, expect_mode, expect_event) in chain {
        let (next, events) =
            change_flight(movement, change, env, Wings::Equipped, CombatLock::Free);
        assert_eq!(next, expect_mode, "I9 mode after the change");
        match expect_event {
            Some(mode) => assert_eq!(events, vec![FlightOutcome::ModeChanged { mode }]),
            None => assert!(events.is_empty(), "I9 a redundant change emits nothing"),
        }
        movement = next;
    }

    let (mode, events) = change_flight(
        movement,
        FlightChange::EnableFlight,
        env,
        Wings::None,
        CombatLock::Free,
    );
    assert_eq!(mode, movement, "I9 a denial leaves the mode unchanged");
    assert_eq!(
        events,
        vec![FlightOutcome::Denied {
            reason: FlightDenialReason::NoWings
        }]
    );
    let (mode, events) = change_flight(
        movement,
        FlightChange::EnableFlight,
        env,
        Wings::Equipped,
        CombatLock::Locked,
    );
    assert_eq!(mode, movement, "I9 a denial leaves the mode unchanged");
    assert_eq!(
        events,
        vec![FlightOutcome::Denied {
            reason: FlightDenialReason::CombatLocked
        }]
    );
}

fn first_walkable_tile(grid: &WalkGrid) -> Option<TileCoord> {
    for y in 0u8..=255 {
        for x in 0u8..=255 {
            let tile = TileCoord::new(x, y);
            if grid.walkable(tile.to_world()) {
                return Some(tile);
            }
        }
    }
    None
}

/// A landing on Sky map 10: a real warp landing if one targets it, else a
/// landing constructed over a discovered walkable tile. `None` only if the map
/// carried no walkable tile at all.
fn sky_landing(atlas: &Atlas, grid: &WalkGrid) -> Option<Landing> {
    if let Some(landing) = atlas
        .warps()
        .map(|warp| warp.landing)
        .find(|landing| landing.map == MapNumber(10))
    {
        return Some(landing);
    }
    let tile = first_walkable_tile(grid)?;
    let area = TileArea::new(tile.x(), tile.y(), tile.x(), tile.y())
        .ok()?
        .to_world();
    Some(Landing {
        map: MapNumber(10),
        area,
        facing: None,
    })
}

#[test]
fn i10_sky_map_forces_flight() {
    let atlas = real_atlas();
    let handle = atlas.map_handle(MapNumber(10)).unwrap();
    assert_eq!(handle.definition().environment, MapEnvironment::Sky);
    let grid = handle.walk_grid();

    // (a) Grounding is denied on a Sky map.
    let (mode, events) = change_flight(
        Movement::Flying,
        FlightChange::DisableFlight,
        MapEnvironment::Sky,
        Wings::Equipped,
        CombatLock::Free,
    );
    assert_eq!(mode, Movement::Flying);
    assert_eq!(
        events,
        vec![FlightOutcome::Denied {
            reason: FlightDenialReason::SkyForcesFlight
        }]
    );

    // (b) Enabling is always permitted — even with no wings, combat-locked.
    let (mode, events) = change_flight(
        Movement::Grounded,
        FlightChange::EnableFlight,
        MapEnvironment::Sky,
        Wings::None,
        CombatLock::Locked,
    );
    assert_eq!(mode, Movement::Flying);
    assert_eq!(
        events,
        vec![FlightOutcome::ModeChanged {
            mode: Movement::Flying
        }]
    );

    // (c) Arrival onto a Sky map forces Flying.
    let landing = sky_landing(&atlas, grid).expect("map 10 has a walkable landing");
    let mut rng = TestRng::new(1);
    match resolve_arrival(Facing::POS_X, &landing, grid, MapEnvironment::Sky, &mut rng) {
        WarpOutcome::Arrived { placement } => {
            assert_eq!(
                placement.movement,
                Movement::Flying,
                "I10 Sky arrival forces Flying"
            );
            assert!(grid.walkable(placement.position));
        }
        WarpOutcome::NoWalkableLanding => panic!("map 10 has walkable landing tiles"),
    }
}

fn walkable_neighbor(grid: &WalkGrid, pos: WorldPos) -> Option<WorldPos> {
    let tile = TileCoord::from_world(pos);
    let (x, y) = (tile.x(), tile.y());
    let mut candidates = Vec::new();
    if x < 255 {
        candidates.push(TileCoord::new(x + 1, y));
    }
    if x > 0 {
        candidates.push(TileCoord::new(x - 1, y));
    }
    if y < 255 {
        candidates.push(TileCoord::new(x, y + 1));
    }
    if y > 0 {
        candidates.push(TileCoord::new(x, y - 1));
    }
    candidates
        .into_iter()
        .map(TileCoord::to_world)
        .find(|&p| grid.walkable(p))
}

/// I11: the first Ground warp whose arrival tile has a walkable neighbour, with
/// that landing, the resolved arrival, the neighbour, and the map's grid.
fn ground_warp_arrival(atlas: &Atlas) -> Option<(Landing, Placement, WorldPos, &WalkGrid)> {
    for warp in atlas.warps() {
        let handle = atlas.map_handle(warp.landing.map)?;
        if handle.definition().environment != MapEnvironment::Ground {
            continue;
        }
        let grid = handle.walk_grid();
        let mut rng = TestRng::new(7);
        let placement = match resolve_arrival(
            Facing::POS_X,
            &warp.landing,
            grid,
            MapEnvironment::Ground,
            &mut rng,
        ) {
            WarpOutcome::Arrived { placement } => placement,
            WarpOutcome::NoWalkableLanding => continue,
        };
        if let Some(neighbor) = walkable_neighbor(grid, placement.position) {
            return Some((warp.landing, placement, neighbor, grid));
        }
    }
    None
}

#[test]
fn i11_ground_warp_arrival_then_step() {
    let atlas = real_atlas();
    let (landing, placement, neighbor, grid) =
        ground_warp_arrival(&atlas).expect("a Ground warp with a walkable neighbour");

    assert!(grid.walkable(placement.position), "I11 arrival is walkable");
    assert!(
        landing.area.contains(placement.position),
        "I11 arrival is inside the area"
    );
    assert_eq!(
        placement.map, landing.map,
        "I11 arrival is on the landing map"
    );
    assert_eq!(
        placement.movement,
        Movement::Grounded,
        "I11 Ground arrival is Grounded"
    );

    match resolve_step(placement, neighbor, ONE_TILE, grid) {
        StepOutcome::Resolved { placement: stepped } => {
            assert!(
                grid.walkable(stepped.position),
                "I11 the step lands walkable"
            );
            assert_eq!(stepped.map, placement.map, "I11 the step keeps the map");
        }
        StepOutcome::Blocked => panic!("I11 a walkable neighbour must resolve"),
    }
}

// --- Group D: proptest sweep over seed and sampled map.

#[test]
fn i12_sampled_map_delta_and_determinism() {
    let atlas = real_atlas();
    let behaviors = behaviors_by_number(&atlas);
    let handles: Vec<_> = atlas.map_handles().collect();
    let bounded = 40u64;

    let mut runner = TestRunner::new(ProptestConfig::with_cases(64));
    runner
        .run(&(any::<u64>(), 0usize..handles.len()), |(seed, index)| {
            let one = std::slice::from_ref(&handles[index]);
            let first = simulate(one, &behaviors, seed, bounded);
            for frame in &first {
                prop_assert!(
                    !frame.before_walkable || frame.after_walkable,
                    "I12 delta walkability at tick {}",
                    frame.tick
                );
            }
            let second = simulate(one, &behaviors, seed, bounded);
            prop_assert_eq!(first, second, "I12 same-seed determinism");
            Ok(())
        })
        .unwrap();
}
