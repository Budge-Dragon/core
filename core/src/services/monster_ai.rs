//! Monster AI: the pure decision that turns a live mob plus its surroundings
//! into one action per tick. It reads the mob's cadence clock and leash anchor,
//! the target it may see, and the walk grid, and returns the advanced mob state
//! together with the [`MonsterIntent`] it chose — state in, state out, no hidden
//! mutation beyond the injected RNG.
//!
//! Decision precedence (highest first): not ready to act, leash-return (a mob
//! that strayed past its territory returns before anything else, so it never
//! wedges chasing into a concave wall), attack a target in range, chase a target
//! in view, then wander. Determinism: only the wander branch draws randomness —
//! exactly one word (the drift heading) via [`crate::services::chance`].

use rand_core::RngCore;

use crate::components::placement::Placement;
use crate::components::spatial::{Facing, Fixed, Radius, UNITS_PER_TILE, WorldPos};
use crate::components::tile::WalkGrid;
use crate::components::units::{Tick, TickDuration, Ticks};
use crate::data::monster_definitions::MobBehavior;
use crate::entities::monster_instance::MonsterInstance;
use crate::events::monster_ai::MonsterIntent;
use crate::events::movement::StepOutcome;
use crate::services::chance::draw_cardinal;
use crate::services::movement::{resolve_drift, resolve_step};

/// A monster's per-action step distance: one whole tile. Authentic: classic MU
/// is tile-grid, so a mob advances exactly one tile per move action — one tile
/// is the grid granularity, not an invented magnitude. Per-monster *speed* is
/// the `move_delay_ms` cadence (the classic Monster.txt move-interval, sourced
/// per monster); `move_range` is the territory radius. Classic carries no
/// separate per-step-distance column, so there is nothing further to source.
const MOB_STEP_SPEED: Fixed = Fixed::from_raw(UNITS_PER_TILE);

/// The leash radius a roaming mob tethers within: its view range. With leash
/// equal to view, a mob chasing a target it can see may overstep the leash by
/// at most one step before leash-return pulls it back on the next action — so
/// the two never fight indefinitely, only alternate at the boundary.
fn leash_radius(behavior: &MobBehavior) -> Radius {
    Radius::from_tiles(behavior.view_range)
}

/// The facing from `pos` toward `target`, keeping `prior` when they coincide (a
/// zero direction has no facing).
fn face_toward(pos: WorldPos, target: WorldPos, prior: Facing) -> Facing {
    match Facing::new(target - pos) {
        Ok(facing) => facing,
        Err(_) => prior,
    }
}

/// The mob rebuilt with a new placement and its cadence advanced by `delay`.
fn rescheduled(mob: &MonsterInstance, placement: Placement, delay: Ticks) -> MonsterInstance {
    MonsterInstance {
        number: mob.number,
        placement,
        health: mob.health,
        anchor: mob.anchor,
        next_action: mob.next_action + delay,
    }
}

/// Applies a resolved movement step and advances the cadence by `delay`. A
/// blocked step leaves the placement unchanged but still reports the movement
/// toward the mob's current position. Returns the new instance and the
/// `(to, facing)` the movement intent announces.
fn advance_after_step(
    mob: &MonsterInstance,
    step: StepOutcome,
    delay: Ticks,
) -> (MonsterInstance, WorldPos, Facing) {
    let placement = match step {
        StepOutcome::Resolved { placement } => placement,
        StepOutcome::Blocked => mob.placement,
    };
    let instance = rescheduled(mob, placement, delay);
    (instance, placement.position, placement.facing)
}

/// Decides one action for a mob and returns the advanced mob state with the
/// chosen intent. The seven parameters are each a distinct domain input at the
/// right layer — the mob and its behavior, the optional target, the clock and
/// tick length, the walk grid, and the RNG — so none can be dropped or bundled.
#[must_use]
pub fn decide_monster_action(
    mob: &MonsterInstance,
    behavior: &MobBehavior,
    target: Option<WorldPos>,
    now: Tick,
    tick: TickDuration,
    grid: &WalkGrid,
    rng: &mut impl RngCore,
) -> (MonsterInstance, MonsterIntent) {
    if !mob.next_action.reached(now) {
        return (*mob, MonsterIntent::Idle);
    }

    let pos = mob.placement.position;
    let move_delay = behavior.move_delay_ms.in_ticks(tick);

    if !pos.within_range(mob.anchor, leash_radius(behavior)) {
        let step = resolve_step(mob.placement, mob.anchor, MOB_STEP_SPEED, grid);
        let (instance, to, facing) = advance_after_step(mob, step, move_delay);
        return (instance, MonsterIntent::LeashReturn { to, facing });
    }

    if let Some(target) = target {
        if pos.within_range(target, Radius::from_tiles(behavior.attack_range)) {
            let facing = face_toward(pos, target, mob.placement.facing);
            let placement = Placement {
                facing,
                ..mob.placement
            };
            let attack_delay = behavior.attack_delay_ms.in_ticks(tick);
            let instance = rescheduled(mob, placement, attack_delay);
            return (instance, MonsterIntent::Attack { target });
        }
        if pos.within_range(target, Radius::from_tiles(behavior.view_range)) {
            let step = resolve_step(mob.placement, target, MOB_STEP_SPEED, grid);
            let (instance, to, facing) = advance_after_step(mob, step, move_delay);
            return (instance, MonsterIntent::Chase { to, facing });
        }
    }

    if behavior.move_range == 0 {
        let instance = rescheduled(mob, mob.placement, move_delay);
        return (instance, MonsterIntent::Idle);
    }

    let drift = draw_cardinal(rng);
    let step = resolve_drift(mob.placement, drift, MOB_STEP_SPEED, grid);
    let (instance, to, facing) = advance_after_step(mob, step, move_delay);
    (instance, MonsterIntent::Wander { to, facing })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::pool::Pool;
    use crate::components::tile::TileCoord;
    use crate::components::units::{DurationMs, MapNumber};
    use crate::data::common::MonsterNumber;

    /// Deterministic `SplitMix64` for replayable tests.
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

    fn all_walkable() -> WalkGrid {
        WalkGrid::from_words([u64::MAX; 1024])
    }

    fn tick50() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    fn behavior(move_range: u8, attack_range: u8, view_range: u8) -> MobBehavior {
        MobBehavior {
            move_range,
            attack_range,
            view_range,
            move_delay_ms: DurationMs(400),
            attack_delay_ms: DurationMs(1000),
            respawn_ms: DurationMs(0),
        }
    }

    fn mob_at(tile: (u8, u8), anchor: (u8, u8), next_action: Tick) -> MonsterInstance {
        MonsterInstance {
            number: MonsterNumber(7),
            placement: Placement {
                position: TileCoord::new(tile.0, tile.1).to_world(),
                facing: Facing::POS_X,
                movement: Movement::Grounded,
                map: MapNumber(0),
            },
            health: Pool::full(60),
            anchor: TileCoord::new(anchor.0, anchor.1).to_world(),
            next_action,
        }
    }

    #[test]
    fn not_ready_is_idle_and_draws_no_rng_and_keeps_next_action() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(100));
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 1, 5),
            Some(TileCoord::new(11, 10).to_world()),
            Tick(50),
            tick50(),
            &grid,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Idle);
        assert_eq!(after, mob); // next_action unchanged
        let mut fresh = TestRng::new(1);
        assert_eq!(rng.next_u64(), fresh.next_u64()); // no RNG consumed
    }

    #[test]
    fn leash_return_outranks_aggro_when_out_of_territory() {
        let grid = all_walkable();
        // view/leash radius 3 tiles; mob is 10 tiles from anchor -> must return.
        let mob = mob_at((20, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(1);
        // A target sits right next to the mob, but leash wins.
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 1, 3),
            Some(TileCoord::new(20, 10).to_world()),
            Tick(0),
            tick50(),
            &grid,
            &mut rng,
        );
        let MonsterIntent::LeashReturn { to, .. } = intent else {
            panic!("expected leash return, got {intent:?}");
        };
        // Stepped one tile back toward the anchor (west).
        assert_eq!(to, TileCoord::new(19, 10).to_world());
        assert_eq!(after.next_action, Tick(0) + Ticks(8)); // 400ms / 50 = 8
    }

    #[test]
    fn attack_in_range_outranks_chase() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 2, 5),
            Some(target),
            Tick(0),
            tick50(),
            &grid,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Attack { target });
        // Faces the target (east) and did not move.
        assert_eq!(after.placement.position, mob.placement.position);
        assert!(after.placement.facing.vector().x().raw() > 0);
        assert_eq!(after.placement.facing.vector().y().raw(), 0);
        assert_eq!(after.next_action, Tick(0) + Ticks(20)); // 1000ms / 50 = 20
    }

    #[test]
    fn chase_steps_toward_a_visible_target_out_of_attack_range() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(15, 10).to_world();
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 1, 8),
            Some(target),
            Tick(0),
            tick50(),
            &grid,
            &mut rng,
        );
        let MonsterIntent::Chase { to, .. } = intent else {
            panic!("expected chase, got {intent:?}");
        };
        assert_eq!(to, TileCoord::new(11, 10).to_world()); // one tile east
        assert_eq!(after.placement.position, TileCoord::new(11, 10).to_world());
    }

    #[test]
    fn stationary_mob_with_no_target_is_idle_but_reschedules() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(0, 1, 5),
            None,
            Tick(0),
            tick50(),
            &grid,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Idle);
        assert_eq!(after.next_action, Tick(0) + Ticks(8));
        let mut fresh = TestRng::new(1);
        assert_eq!(rng.next_u64(), fresh.next_u64()); // idle draws no RNG
    }

    #[test]
    fn wander_draws_exactly_one_word() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(5);
        let (_, intent) = decide_monster_action(
            &mob,
            &behavior(3, 1, 5),
            None,
            Tick(0),
            tick50(),
            &grid,
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::Wander { .. }));
        // Exactly one word consumed by draw_cardinal.
        let mut probe = TestRng::new(5);
        probe.next_u64();
        assert_eq!(rng.next_u64(), probe.next_u64());
    }

    #[test]
    fn same_seed_yields_identical_decisions() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let decide = |seed: u64| {
            let mut rng = TestRng::new(seed);
            decide_monster_action(
                &mob,
                &behavior(3, 1, 5),
                None,
                Tick(0),
                tick50(),
                &grid,
                &mut rng,
            )
        };
        assert_eq!(decide(9), decide(9));
    }
}
