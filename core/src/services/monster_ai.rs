//! Monster AI: the pure decision that turns a live mob plus its surroundings
//! into one action per tick. It reads the mob's cadence clock and leash anchor,
//! the target it may see, and the terrain grid, and returns the advanced mob state
//! together with the [`MonsterIntent`] it chose — state in, state out, no hidden
//! mutation beyond the injected RNG.
//!
//! Decision precedence (highest first): not ready to act, leash-return (a mob
//! that strayed past its territory returns before anything else, so it never
//! wedges chasing into a concave wall), attack a target in range, chase a target
//! in view, then wander. Determinism: only the wander branch draws randomness —
//! a free continuous drift heading via [`crate::services::chance::draw_heading`],
//! which consumes a variable but deterministic-per-seed number of words.
//!
//! Safezones bound the decision: a target standing on a safe tile is no target
//! for any role — except a `Patrols` guard, which still hunts a flagged
//! murderer (a hunted [`Standing`]) who fled onto one. The behavior's
//! [`SafezoneDisposition`] gates the rest — an `Excluded` mob never swings from
//! a safe tile and never steps onto one, while a `Patrols` guard walks and
//! fights across town freely.

use rand_core::RngCore;

use crate::components::movement::Mobility;
use crate::components::placement::Placement;
use crate::components::reputation::Standing;
use crate::components::spatial::{Facing, Radius, StepMagnitude, WorldPos};
use crate::components::tile::TerrainGrid;
use crate::components::units::{Tick, TickDuration, Ticks};
use crate::data::monster_definitions::{MobBehavior, SafezoneDisposition};
use crate::entities::monster_instance::MonsterInstance;
use crate::events::monster_ai::MonsterIntent;
use crate::events::movement::StepOutcome;
use crate::services::chance::draw_heading;
use crate::services::movement::{resolve_drift, resolve_step};

/// A mob's aggro target for one tick: where the target stands, plus the
/// [`Standing`] that decides whether a guard pursues it onto a safe tile. A
/// struct rather than an enum — a mob only ever targets a player this era, so
/// one shape carries both facts. Deliberately not serializable: it is transient
/// per-tick decision input the host rebuilds from live state each tick, never
/// stored entity state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiTarget {
    /// Where the target stands.
    pub position: WorldPos,
    /// The target's player-kill standing, which decides guard pursuit onto safe
    /// tiles.
    pub standing: Standing,
}

/// A monster's per-action step distance: one whole tile. Authentic: classic MU
/// is tile-grid, so a mob advances exactly one tile per move action — one tile
/// is the grid granularity, not an invented magnitude. Per-monster *speed* is
/// the `move_delay_ms` cadence (the classic Monster.txt move-interval, sourced
/// per monster); `move_range` is the territory radius. Classic carries no
/// separate per-step-distance column, so there is nothing further to source.
const MOB_STEP_SPEED: StepMagnitude = StepMagnitude::ONE_TILE;

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
        active_effects: mob.active_effects,
    }
}

/// The per-step speed a mobility confers, or `None` when the mob is
/// immobilized (no leash/chase/wander step is taken). `Free` moves at the base
/// mob step speed; `Slowed` scales the whole tile down by the effect's slow
/// ratio through [`StepMagnitude::tile_fraction`] — a slow can only shrink the
/// step, so the ≤1-tile bound holds by construction and [`MOB_STEP_SPEED`] is
/// decided in this one module and nowhere else.
fn step_speed(mobility: Mobility) -> Option<StepMagnitude> {
    match mobility {
        Mobility::Free => Some(MOB_STEP_SPEED),
        Mobility::Slowed { ratio } => Some(StepMagnitude::tile_fraction(ratio.num(), ratio.den())),
        Mobility::Immobilized => None,
    }
}

/// Whether a mob standing at `pos` may swing. A guard (`Patrols`) attacks even
/// from a safe tile; a basic mob or trap (`Excluded`) refuses to swing from
/// one. Total over [`SafezoneDisposition`].
fn attacks_from_here(disposition: SafezoneDisposition, pos: WorldPos, grid: &TerrainGrid) -> bool {
    match disposition {
        SafezoneDisposition::Patrols => true,
        SafezoneDisposition::Excluded => !grid.safe(pos),
    }
}

/// A movement step gated by disposition: an `Excluded` mob's step onto a safe
/// tile is refused (`Blocked`) — it never walks into town; a `Patrols` guard
/// steps freely. Total over `(StepOutcome, SafezoneDisposition)`.
fn disposition_gated(
    step: StepOutcome,
    disposition: SafezoneDisposition,
    grid: &TerrainGrid,
) -> StepOutcome {
    match (step, disposition) {
        (StepOutcome::Resolved { placement }, SafezoneDisposition::Excluded)
            if grid.safe(placement.position) =>
        {
            StepOutcome::Blocked
        }
        (step, SafezoneDisposition::Excluded | SafezoneDisposition::Patrols) => step,
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
/// chosen intent. The eight parameters are each a distinct domain input at the
/// right layer — the mob and its behavior, the optional target, the clock and
/// tick length, the terrain grid, the movement capability, and the RNG — so none
/// can be dropped or bundled. `mobility` is supplied by the caller (derived from
/// the mob's active effects via [`crate::services::effects::mobility`]) so this
/// service stays effect-unaware: it only gates movement. An immobilized mob
/// takes no leash/chase/wander step (each resolves to an in-place idle), but an
/// in-range attack still fires — immobilization is a movement-only gate.
#[must_use]
pub fn decide_monster_action(
    mob: &MonsterInstance,
    behavior: &MobBehavior,
    target: Option<AiTarget>,
    now: Tick,
    tick: TickDuration,
    grid: &TerrainGrid,
    mobility: Mobility,
    rng: &mut impl RngCore,
) -> (MonsterInstance, MonsterIntent) {
    if !mob.next_action.reached(now) {
        return (*mob, MonsterIntent::Idle);
    }

    let pos = mob.placement.position;
    // A safezone-stander is no target for any role — never selected, and a
    // locked target is dropped the tick it steps onto a safe tile — except a
    // guard hunting a flagged murderer, which pursues onto safe tiles. Reduced
    // to the bare position once the safezone gate has decided.
    let target = target
        .filter(|t| {
            !grid.safe(t.position)
                || (behavior.disposition.hunts_on_safe_tiles() && t.standing.is_hunted())
        })
        .map(|t| t.position);
    let move_delay = behavior.move_delay_ms.in_ticks(tick);
    let speed = step_speed(mobility);

    // Leash-return is a movement branch: only a mob able to step returns, and it
    // still outranks aggro (an immobilized stray skips straight to combat).
    if let Some(speed) = speed {
        if !pos.within_range(mob.anchor, leash_radius(behavior)) {
            let step = disposition_gated(
                resolve_step(mob.placement, mob.anchor, speed, grid),
                behavior.disposition,
                grid,
            );
            let (instance, to, facing) = advance_after_step(mob, step, move_delay);
            return (instance, MonsterIntent::LeashReturn { to, facing });
        }
    }

    if let Some(target) = target {
        if pos.within_range(target, Radius::from_tiles(behavior.attack_range))
            && attacks_from_here(behavior.disposition, pos, grid)
        {
            let facing = face_toward(pos, target, mob.placement.facing);
            let placement = Placement {
                facing,
                ..mob.placement
            };
            let attack_delay = behavior.attack_delay_ms.in_ticks(tick);
            let instance = rescheduled(mob, placement, attack_delay);
            return (instance, MonsterIntent::Attack { target });
        }
        if let Some(speed) = speed {
            if pos.within_range(target, Radius::from_tiles(behavior.view_range)) {
                let step = disposition_gated(
                    resolve_step(mob.placement, target, speed, grid),
                    behavior.disposition,
                    grid,
                );
                let (instance, to, facing) = advance_after_step(mob, step, move_delay);
                return (instance, MonsterIntent::Chase { to, facing });
            }
        }
    }

    // Wander is a movement branch; an immobilized or territory-bound mob idles.
    match speed {
        Some(speed) if behavior.move_range > 0 => {
            let drift = draw_heading(rng);
            let step = disposition_gated(
                resolve_drift(mob.placement, drift, speed, grid),
                behavior.disposition,
                grid,
            );
            let (instance, to, facing) = advance_after_step(mob, step, move_delay);
            (instance, MonsterIntent::Wander { to, facing })
        }
        Some(_) | None => {
            let instance = rescheduled(mob, mob.placement, move_delay);
            (instance, MonsterIntent::Idle)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::{Movement, SlowRatio};
    use crate::components::pool::Pool;
    use crate::components::reputation::PkStage;
    use crate::components::spatial::UNITS_PER_TILE;
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

    fn all_walkable() -> TerrainGrid {
        TerrainGrid::from_words([u64::MAX; 1024])
    }

    fn safe_bit(words: &mut [u64; 1024], tile: (u8, u8)) {
        let bit = (usize::from(tile.1) << 8) | usize::from(tile.0);
        words[bit >> 6] |= 1u64 << (bit & 63);
    }

    /// All tiles walkable; exactly `tiles` are safe.
    fn grid_safe_at(tiles: &[(u8, u8)]) -> TerrainGrid {
        let mut safe = [0u64; 1024];
        for &tile in tiles {
            safe_bit(&mut safe, tile);
        }
        TerrainGrid::from_bitsets([u64::MAX; 1024], safe)
    }

    /// All tiles walkable and safe except `tiles`.
    fn grid_safe_except(tiles: &[(u8, u8)]) -> TerrainGrid {
        let mut safe = [u64::MAX; 1024];
        for &tile in tiles {
            let bit = (usize::from(tile.1) << 8) | usize::from(tile.0);
            safe[bit >> 6] &= !(1u64 << (bit & 63));
        }
        TerrainGrid::from_bitsets([u64::MAX; 1024], safe)
    }

    fn tick50() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    fn behavior_of(
        move_range: u8,
        attack_range: u8,
        view_range: u8,
        disposition: SafezoneDisposition,
    ) -> MobBehavior {
        MobBehavior {
            move_range,
            attack_range,
            view_range,
            move_delay_ms: DurationMs(400),
            attack_delay_ms: DurationMs(1000),
            respawn_ms: DurationMs(0),
            disposition,
        }
    }

    fn behavior(move_range: u8, attack_range: u8, view_range: u8) -> MobBehavior {
        behavior_of(
            move_range,
            attack_range,
            view_range,
            SafezoneDisposition::Excluded,
        )
    }

    fn guard_behavior(move_range: u8, attack_range: u8, view_range: u8) -> MobBehavior {
        behavior_of(
            move_range,
            attack_range,
            view_range,
            SafezoneDisposition::Patrols,
        )
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
            active_effects: crate::components::active_effect::ActiveEffects::EMPTY,
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
            Some(AiTarget {
                position: TileCoord::new(11, 10).to_world(),
                standing: Standing::Clean,
            }),
            Tick(50),
            tick50(),
            &grid,
            Mobility::Free,
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
            Some(AiTarget {
                position: TileCoord::new(20, 10).to_world(),
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
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
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
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
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
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
            Mobility::Free,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Idle);
        assert_eq!(after.next_action, Tick(0) + Ticks(8));
        let mut fresh = TestRng::new(1);
        assert_eq!(rng.next_u64(), fresh.next_u64()); // idle draws no RNG
    }

    #[test]
    fn wander_consumes_rng_unlike_idle() {
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
            Mobility::Free,
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::Wander { .. }));
        // The wander branch draws a continuous heading — a VARIABLE but
        // deterministic-per-seed number of words (Marsaglia disk rejection,
        // ≥2), so the stream advances past a fresh generator (unlike idle,
        // which draws none). Same-seed reproduction is proven by
        // `same_seed_yields_identical_decisions`.
        let mut fresh = TestRng::new(5);
        assert_ne!(rng.next_u64(), fresh.next_u64(), "wander must consume RNG");
    }

    #[test]
    fn immobilized_mob_cannot_step_but_still_attacks_in_range() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        // In attack range: the movement gate does not bar an attack.
        let (_, intent) = decide_monster_action(
            &mob,
            &behavior(1, 2, 5),
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Immobilized,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Attack { target });
    }

    #[test]
    fn immobilized_mob_out_of_view_idles_in_place() {
        let grid = all_walkable();
        // A stray far from its anchor would leash-return if it could step.
        let mob = mob_at((20, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(3, 1, 3),
            None,
            Tick(0),
            tick50(),
            &grid,
            Mobility::Immobilized,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Idle);
        assert_eq!(after.placement.position, mob.placement.position);
        // Reschedules by the move delay and draws no wander word.
        assert_eq!(after.next_action, Tick(0) + Ticks(8));
        let mut fresh = TestRng::new(1);
        assert_eq!(rng.next_u64(), fresh.next_u64());
    }

    #[test]
    fn slowed_mob_chases_at_the_reduced_speed() {
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(20, 10).to_world();
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 1, 15),
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Slowed {
                ratio: SlowRatio::HALVED,
            },
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::Chase { .. }));
        // A half-tile step east lands short of the full-tile chase position.
        let stepped = after.placement.position.x().raw() - mob.placement.position.x().raw();
        assert_eq!(stepped, UNITS_PER_TILE / 2);
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
                Mobility::Free,
                &mut rng,
            )
        };
        assert_eq!(decide(9), decide(9));
    }

    #[test]
    fn no_role_targets_a_safezone_stander() {
        // MAI-1 (universal): the adjacent target stands on a safe tile, so
        // neither an excluded mob nor a patrolling guard attacks or chases it —
        // with no territory to wander, each idles as if there were no target.
        let grid = grid_safe_at(&[(11, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(11, 10).to_world();
        for behavior in [behavior(0, 2, 5), guard_behavior(0, 2, 5)] {
            let mut rng = TestRng::new(1);
            let (after, intent) = decide_monster_action(
                &mob,
                &behavior,
                Some(AiTarget {
                    position: target,
                    standing: Standing::Clean,
                }),
                Tick(0),
                tick50(),
                &grid,
                Mobility::Free,
                &mut rng,
            );
            assert_eq!(intent, MonsterIntent::Idle);
            assert_eq!(after.placement.position, mob.placement.position);
        }
    }

    #[test]
    fn a_locked_target_that_entered_a_safezone_is_dropped() {
        // MAI-2: the target sits in chase geometry (out of attack reach, in
        // view) but on a safe tile — the same per-tick test as never-selecting,
        // so the mob neither chases nor attacks and falls back to wandering.
        let grid = grid_safe_at(&[(13, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(13, 10).to_world();
        let mut rng = TestRng::new(1);
        let (_, intent) = decide_monster_action(
            &mob,
            &behavior(1, 1, 8),
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(!matches!(
            intent,
            MonsterIntent::Attack { .. } | MonsterIntent::Chase { .. }
        ));
    }

    #[test]
    fn an_excluded_mob_on_a_safe_tile_does_not_attack() {
        // MAI-3: the mob's own tile is safe and the in-range target is not —
        // an excluded mob refuses to swing from a safezone and chases off the
        // tile instead.
        let grid = grid_safe_at(&[(10, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 2, 8),
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(!matches!(intent, MonsterIntent::Attack { .. }));
        assert!(matches!(intent, MonsterIntent::Chase { .. }));
        assert_eq!(after.placement.position, target);
    }

    #[test]
    fn a_guard_on_a_safe_tile_still_attacks() {
        // MAI-6: the same geometry as the excluded refusal, but a patrolling
        // guard swings from a safe town tile.
        let grid = grid_safe_at(&[(10, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(11, 10).to_world();
        let mut rng = TestRng::new(1);
        let (_, intent) = decide_monster_action(
            &mob,
            &guard_behavior(1, 2, 8),
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert_eq!(intent, MonsterIntent::Attack { target });
    }

    #[test]
    fn an_excluded_chase_step_onto_a_safe_tile_is_refused() {
        // MAI-4: every tile between the mob and its target is safe, so the
        // chase step is blocked at the safezone line and the mob stays put.
        let grid = grid_safe_except(&[(10, 10), (13, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let target = TileCoord::new(13, 10).to_world();
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(1, 1, 8),
            Some(AiTarget {
                position: target,
                standing: Standing::Clean,
            }),
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::Chase { .. }));
        assert_eq!(after.placement.position, mob.placement.position);
    }

    #[test]
    fn an_excluded_wander_onto_safe_tiles_stays_put_but_consumes_rng() {
        // MAI-4 (wander): every neighbouring tile is safe, so whatever heading
        // the drift draw picks, the step is refused and the mob holds.
        let grid = grid_safe_except(&[(10, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(5);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(3, 1, 5),
            None,
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::Wander { .. }));
        assert_eq!(after.placement.position, mob.placement.position);
        // The heading draw still consumed RNG (the step was gated, not skipped):
        // the stream advanced past a fresh generator.
        let mut fresh = TestRng::new(5);
        assert_ne!(
            rng.next_u64(),
            fresh.next_u64(),
            "the drift draw consumed RNG"
        );
    }

    #[test]
    fn an_excluded_leash_return_stops_at_the_safe_line() {
        // MAI-4 (leash): the stray's whole way home is safe tiles, so the
        // return step is refused and the mob holds at the line.
        let grid = grid_safe_except(&[(20, 10)]);
        let mob = mob_at((20, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(1);
        let (after, intent) = decide_monster_action(
            &mob,
            &behavior(3, 1, 3),
            None,
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::LeashReturn { .. }));
        assert_eq!(after.placement.position, mob.placement.position);
    }

    #[test]
    fn a_guard_patrols_onto_safe_tiles_freely() {
        // MAI-5: the same all-safe surroundings that pin an excluded mob leave
        // a guard free — its patrol step resolves onto a safe tile.
        let grid = grid_safe_except(&[(10, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let mut rng = TestRng::new(5);
        let (after, intent) = decide_monster_action(
            &mob,
            &guard_behavior(3, 1, 5),
            None,
            Tick(0),
            tick50(),
            &grid,
            Mobility::Free,
            &mut rng,
        );
        assert!(matches!(intent, MonsterIntent::Wander { .. }));
        assert_ne!(after.placement.position, mob.placement.position);
        assert!(grid.safe(after.placement.position));
    }

    #[test]
    fn a_patrols_guard_hunts_a_first_stage_player_on_a_safe_tile() {
        // The target parks on a safe tile in the guard's reach. A hunted (>=
        // first-stage) murderer is pursued and struck; a clean or warning-stage
        // player on the same tile is still no target; and an Excluded mob never
        // hunts a murderer onto a safe tile.
        let grid = grid_safe_at(&[(11, 10)]);
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let safe_tile = TileCoord::new(11, 10).to_world();
        let action_for = |behavior: MobBehavior, standing: Standing| {
            let mut rng = TestRng::new(1);
            decide_monster_action(
                &mob,
                &behavior,
                Some(AiTarget {
                    position: safe_tile,
                    standing,
                }),
                Tick(0),
                tick50(),
                &grid,
                Mobility::Free,
                &mut rng,
            )
            .1
        };
        let hunted = Standing::Flagged {
            stage: PkStage::FirstStage,
            decays_at: Tick(9),
        };
        let warning = Standing::Flagged {
            stage: PkStage::Warning,
            decays_at: Tick(9),
        };
        assert_eq!(
            action_for(guard_behavior(0, 2, 5), hunted),
            MonsterIntent::Attack { target: safe_tile }
        );
        assert_eq!(
            action_for(guard_behavior(0, 2, 5), Standing::Clean),
            MonsterIntent::Idle
        );
        assert_eq!(
            action_for(guard_behavior(0, 2, 5), warning),
            MonsterIntent::Idle
        );
        assert_eq!(action_for(behavior(0, 2, 5), hunted), MonsterIntent::Idle);
    }

    #[test]
    fn off_safe_behavior_is_unchanged_for_any_standing() {
        // Off a safe tile the standing never enters the decision: attack and
        // chase geometry resolve byte-identically for clean and every flagged
        // rung.
        let grid = all_walkable();
        let mob = mob_at((10, 10), (10, 10), Tick(0));
        let decide_with = |position: WorldPos, standing: Standing, behavior: &MobBehavior| {
            let mut rng = TestRng::new(1);
            decide_monster_action(
                &mob,
                behavior,
                Some(AiTarget { position, standing }),
                Tick(0),
                tick50(),
                &grid,
                Mobility::Free,
                &mut rng,
            )
            .1
        };
        let standings = [
            Standing::Clean,
            Standing::Flagged {
                stage: PkStage::Warning,
                decays_at: Tick(9),
            },
            Standing::Flagged {
                stage: PkStage::FirstStage,
                decays_at: Tick(9),
            },
            Standing::Flagged {
                stage: PkStage::SecondStage,
                decays_at: Tick(9),
            },
        ];
        let attack_target = TileCoord::new(11, 10).to_world();
        for standing in standings {
            assert_eq!(
                decide_with(attack_target, standing, &behavior(1, 2, 8)),
                MonsterIntent::Attack {
                    target: attack_target
                }
            );
        }
        let chase_target = TileCoord::new(15, 10).to_world();
        for standing in standings {
            assert_eq!(
                decide_with(chase_target, standing, &behavior(1, 1, 8)),
                decide_with(chase_target, Standing::Clean, &behavior(1, 1, 8))
            );
        }
    }
}
