//! Movement and flight decisions: the pure functions that change how and where
//! an entity crosses the ground plane. Flight-mode changes gate on host-supplied
//! eligibility facts then transition; grounded/flying steps resolve a greedy
//! seek-with-arrival move against the walk grid; warp/gate arrivals pick a
//! walkable landing tile. Every outcome is a returned event value.
//!
//! Determinism: only [`resolve_arrival`] draws randomness — exactly one word
//! (the landing-tile pick) when the area has a walkable tile, none otherwise —
//! routed through [`crate::services::chance`]. The other functions draw nothing.

use rand_core::RngCore;

use crate::components::collections::{EmptyCollection, OneOrMore};
use crate::components::movement::{CombatLock, FlightChange, Movement, Wings};
use crate::components::placement::Placement;
use crate::components::spatial::{Displacement, Facing, Fixed, WorldPos, WorldVec};
use crate::components::tile::WalkGrid;
use crate::data::atlas::Landing;
use crate::data::map_definitions::MapEnvironment;
use crate::events::movement::{FlightDenialReason, FlightOutcome, StepOutcome, WarpOutcome};
use crate::services::chance::pick_one;

/// The eligibility decision made before a flight-mode transition.
enum FlightGate {
    /// The change is permitted; run the transition.
    Permit,
    /// The change is denied for a single reason; the mode is left unchanged.
    Deny {
        /// Why the change was rejected.
        reason: FlightDenialReason,
    },
}

/// Applies a flight-mode change to a movement mode — the pure 2×2 transition. A
/// change that would not alter the mode is an idempotent no-op that emits
/// nothing. Eligibility is decided separately, before this runs.
#[must_use]
fn apply_flight_change(movement: Movement, change: FlightChange) -> (Movement, Vec<FlightOutcome>) {
    match (movement, change) {
        (Movement::Grounded, FlightChange::EnableFlight) => (
            Movement::Flying,
            vec![FlightOutcome::ModeChanged {
                mode: Movement::Flying,
            }],
        ),
        (Movement::Flying, FlightChange::DisableFlight) => (
            Movement::Grounded,
            vec![FlightOutcome::ModeChanged {
                mode: Movement::Grounded,
            }],
        ),
        (Movement::Flying, FlightChange::EnableFlight) => (Movement::Flying, Vec::new()),
        (Movement::Grounded, FlightChange::DisableFlight) => (Movement::Grounded, Vec::new()),
    }
}

/// Off-Sky voluntary flight eligibility: requires wings, then freedom from
/// combat lock. Capability (wings) outranks the transient combat lock, so the
/// no-wings denial is reported before the combat-locked one.
fn enable_off_sky(wings: Wings, combat_lock: CombatLock) -> FlightGate {
    match wings {
        Wings::None => FlightGate::Deny {
            reason: FlightDenialReason::NoWings,
        },
        Wings::Equipped => match combat_lock {
            CombatLock::Locked => FlightGate::Deny {
                reason: FlightDenialReason::CombatLocked,
            },
            CombatLock::Free => FlightGate::Permit,
        },
    }
}

/// The eligibility gate that runs before the transition. Environment and mode
/// legality live here — the step service never re-checks them. A Sky map forces
/// flight (grounding is denied, enabling is always permitted); off Sky, enabling
/// needs wings and freedom from combat, disabling is always permitted.
fn flight_gate(
    change: FlightChange,
    env: MapEnvironment,
    wings: Wings,
    combat_lock: CombatLock,
) -> FlightGate {
    match (env, change) {
        (MapEnvironment::Sky, FlightChange::EnableFlight) => FlightGate::Permit,
        (MapEnvironment::Sky, FlightChange::DisableFlight) => FlightGate::Deny {
            reason: FlightDenialReason::SkyForcesFlight,
        },
        (MapEnvironment::Ground | MapEnvironment::Underwater, FlightChange::EnableFlight) => {
            enable_off_sky(wings, combat_lock)
        }
        (MapEnvironment::Ground | MapEnvironment::Underwater, FlightChange::DisableFlight) => {
            FlightGate::Permit
        }
    }
}

/// A voluntary flight change: gate on eligibility first, then transition. A
/// denied change leaves the mode unchanged and reports the reason; a permitted
/// change transitions the mode, an idempotent no-op that emits nothing when the
/// mode would not change.
#[must_use]
pub fn change_flight(
    movement: Movement,
    change: FlightChange,
    env: MapEnvironment,
    wings: Wings,
    combat_lock: CombatLock,
) -> (Movement, Vec<FlightOutcome>) {
    match flight_gate(change, env, wings, combat_lock) {
        FlightGate::Deny { reason } => (movement, vec![FlightOutcome::Denied { reason }]),
        FlightGate::Permit => apply_flight_change(movement, change),
    }
}

/// Commits a resolved step offset to a placement. The destination is
/// `placement.position + step`, clamped into the world by the `+` operator; a
/// grounded step onto a non-walkable destination cell is blocked (only the
/// destination cell is validated, not the traversed path). Otherwise the entity
/// arrives, faces along the step (keeping its prior facing when the step has no
/// direction), and keeps its map and movement mode.
fn commit(placement: Placement, step: WorldVec, grid: &WalkGrid) -> StepOutcome {
    let destination = placement.position + step;
    if placement.movement.checks_walkability() && !grid.walkable(destination) {
        return StepOutcome::Blocked;
    }
    let facing = match Facing::new(step) {
        Ok(facing) => facing,
        Err(_) => placement.facing,
    };
    StepOutcome::Resolved {
        placement: Placement {
            position: destination,
            facing,
            movement: placement.movement,
            map: placement.map,
        },
    }
}

/// A greedy step toward a target offset with Reynolds arrival: within one step
/// of the target the step is the full remaining offset (arrive exactly, never
/// overshoot or oscillate); farther out, the direction rescaled to `speed`. The
/// zero offset has no direction.
fn seek(to_target: WorldVec, speed: Fixed) -> Displacement {
    if to_target == WorldVec::ZERO {
        return Displacement::NoDirection;
    }
    let reach = WorldVec::new(speed, Fixed::from_raw(0)).length_sq();
    if to_target.length_sq() <= reach {
        return Displacement::Scaled { vector: to_target };
    }
    to_target.normalized_to(speed)
}

/// Steps a placement toward a world target at `speed`, arrival-clamped so it
/// lands on the target within one step. Already at the target, it is an
/// unchanged no-op.
#[must_use]
pub fn resolve_step(
    placement: Placement,
    target: WorldPos,
    speed: Fixed,
    grid: &WalkGrid,
) -> StepOutcome {
    match seek(target - placement.position, speed) {
        Displacement::NoDirection => StepOutcome::Resolved { placement },
        Displacement::Scaled { vector } => commit(placement, vector, grid),
    }
}

/// Drifts a placement one `speed` step along a fixed facing — the wander move. A
/// facing is a proven non-zero direction, so the no-direction arm returns the
/// unchanged placement only to keep the match total.
#[must_use]
pub fn resolve_drift(
    placement: Placement,
    direction: Facing,
    speed: Fixed,
    grid: &WalkGrid,
) -> StepOutcome {
    match direction.vector().normalized_to(speed) {
        Displacement::NoDirection => StepOutcome::Resolved { placement },
        Displacement::Scaled { vector } => commit(placement, vector, grid),
    }
}

/// Resolves a warp/gate arrival. Collects the landing area's walkable tiles;
/// with none, the traveler is not moved ([`WarpOutcome::NoWalkableLanding`]).
/// Otherwise it picks one uniformly (one RNG draw), faces per the landing's
/// authored facing or keeps the traveler's when unspecified, and takes the mode
/// the destination environment forces (Sky → Flying, Ground/Underwater →
/// Grounded).
#[must_use]
pub fn resolve_arrival(
    traveler_facing: Facing,
    landing: &Landing,
    grid: &WalkGrid,
    env: MapEnvironment,
    rng: &mut impl RngCore,
) -> WarpOutcome {
    let cells: Vec<WorldPos> = grid.walkable_positions_in(landing.area).collect();
    match OneOrMore::new(cells) {
        Err(EmptyCollection) => WarpOutcome::NoWalkableLanding,
        Ok(cells) => {
            let position = *pick_one(&cells, rng);
            let facing = match landing.facing {
                Some(facing) => facing,
                None => traveler_facing,
            };
            let movement = match env {
                MapEnvironment::Sky => Movement::Flying,
                MapEnvironment::Ground | MapEnvironment::Underwater => Movement::Grounded,
            };
            WarpOutcome::Arrived {
                placement: Placement {
                    position,
                    facing,
                    movement,
                    map: landing.map,
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::spatial::{UNITS_PER_TILE, WORLD_EXTENT};
    use crate::components::tile::{TileArea, TileCoord};
    use crate::components::units::MapNumber;

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

    const STEP: Fixed = Fixed::from_raw(UNITS_PER_TILE);

    fn grid_with(walkable: &[(u8, u8)]) -> WalkGrid {
        let mut words = [0u64; 1024];
        for &(x, y) in walkable {
            let bit = (usize::from(y) << 8) | usize::from(x);
            words[bit >> 6] |= 1u64 << (bit & 63);
        }
        WalkGrid::from_words(words)
    }

    fn placed(tile: (u8, u8), movement: Movement) -> Placement {
        Placement {
            position: TileCoord::new(tile.0, tile.1).to_world(),
            facing: Facing::POS_X,
            movement,
            map: MapNumber(0),
        }
    }

    #[test]
    fn apply_flight_change_covers_all_four_tuples() {
        assert_eq!(
            apply_flight_change(Movement::Grounded, FlightChange::EnableFlight),
            (
                Movement::Flying,
                vec![FlightOutcome::ModeChanged {
                    mode: Movement::Flying
                }]
            )
        );
        assert_eq!(
            apply_flight_change(Movement::Flying, FlightChange::DisableFlight),
            (
                Movement::Grounded,
                vec![FlightOutcome::ModeChanged {
                    mode: Movement::Grounded
                }]
            )
        );
        // Redundant changes are idempotent no-ops emitting nothing.
        assert_eq!(
            apply_flight_change(Movement::Flying, FlightChange::EnableFlight),
            (Movement::Flying, Vec::new())
        );
        assert_eq!(
            apply_flight_change(Movement::Grounded, FlightChange::DisableFlight),
            (Movement::Grounded, Vec::new())
        );
    }

    #[test]
    fn flight_gate_truth_table() {
        // Sky forces flight: enabling permitted, disabling denied.
        assert!(matches!(
            flight_gate(
                FlightChange::EnableFlight,
                MapEnvironment::Sky,
                Wings::None,
                CombatLock::Locked
            ),
            FlightGate::Permit
        ));
        assert!(matches!(
            flight_gate(
                FlightChange::DisableFlight,
                MapEnvironment::Sky,
                Wings::Equipped,
                CombatLock::Free
            ),
            FlightGate::Deny {
                reason: FlightDenialReason::SkyForcesFlight
            }
        ));
        // Off Sky, disabling is always permitted.
        for env in [MapEnvironment::Ground, MapEnvironment::Underwater] {
            assert!(matches!(
                flight_gate(
                    FlightChange::DisableFlight,
                    env,
                    Wings::None,
                    CombatLock::Locked
                ),
                FlightGate::Permit
            ));
            // Enabling: no wings denies first (capability before transient lock).
            assert!(matches!(
                flight_gate(
                    FlightChange::EnableFlight,
                    env,
                    Wings::None,
                    CombatLock::Free
                ),
                FlightGate::Deny {
                    reason: FlightDenialReason::NoWings
                }
            ));
            assert!(matches!(
                flight_gate(
                    FlightChange::EnableFlight,
                    env,
                    Wings::Equipped,
                    CombatLock::Locked
                ),
                FlightGate::Deny {
                    reason: FlightDenialReason::CombatLocked
                }
            ));
            assert!(matches!(
                flight_gate(
                    FlightChange::EnableFlight,
                    env,
                    Wings::Equipped,
                    CombatLock::Free
                ),
                FlightGate::Permit
            ));
        }
    }

    #[test]
    fn change_flight_denied_leaves_mode_and_reports_reason() {
        assert_eq!(
            change_flight(
                Movement::Flying,
                FlightChange::DisableFlight,
                MapEnvironment::Sky,
                Wings::Equipped,
                CombatLock::Free
            ),
            (
                Movement::Flying,
                vec![FlightOutcome::Denied {
                    reason: FlightDenialReason::SkyForcesFlight
                }]
            )
        );
        assert_eq!(
            change_flight(
                Movement::Grounded,
                FlightChange::EnableFlight,
                MapEnvironment::Ground,
                Wings::Equipped,
                CombatLock::Free
            ),
            (
                Movement::Flying,
                vec![FlightOutcome::ModeChanged {
                    mode: Movement::Flying
                }]
            )
        );
    }

    #[test]
    fn grounded_step_blocks_on_non_walkable_destination() {
        // Start at (1,0), target (0,0). One tile step lands on (0,0), which is
        // not walkable, so a grounded step is blocked.
        let grid = grid_with(&[(1, 0)]);
        let start = placed((1, 0), Movement::Grounded);
        let target = TileCoord::new(0, 0).to_world();
        assert_eq!(
            resolve_step(start, target, STEP, &grid),
            StepOutcome::Blocked
        );
    }

    #[test]
    fn grounded_step_resolves_onto_walkable_destination() {
        let grid = grid_with(&[(1, 0), (0, 0)]);
        let start = placed((1, 0), Movement::Grounded);
        let target = TileCoord::new(0, 0).to_world();
        match resolve_step(start, target, STEP, &grid) {
            StepOutcome::Resolved { placement } => {
                assert_eq!(placement.position, target);
                // Faces west along the step (magnitude-invariant, not unit-normalized).
                assert!(placement.facing.vector().x().raw() < 0);
                assert_eq!(placement.facing.vector().y().raw(), 0);
                assert_eq!(placement.movement, Movement::Grounded);
            }
            StepOutcome::Blocked => panic!("walkable destination must resolve"),
        }
    }

    #[test]
    fn flying_step_crosses_a_non_walkable_cell() {
        let grid = grid_with(&[(1, 0)]); // (0,0) blocked
        let start = placed((1, 0), Movement::Flying);
        let target = TileCoord::new(0, 0).to_world();
        match resolve_step(start, target, STEP, &grid) {
            StepOutcome::Resolved { placement } => {
                assert_eq!(placement.position, target);
                assert_eq!(placement.movement, Movement::Flying);
            }
            StepOutcome::Blocked => panic!("flying ignores walkability"),
        }
    }

    #[test]
    fn step_at_target_is_unchanged_no_op() {
        let grid = grid_with(&[]);
        let start = placed((5, 5), Movement::Grounded);
        let outcome = resolve_step(start, start.position, STEP, &grid);
        assert_eq!(outcome, StepOutcome::Resolved { placement: start });
    }

    #[test]
    fn drift_clamps_at_the_world_edge() {
        let grid = grid_with(&[]);
        let start = Placement {
            position: WorldPos::clamped(WORLD_EXTENT, 0),
            facing: Facing::POS_X,
            movement: Movement::Flying,
            map: MapNumber(0),
        };
        match resolve_drift(start, Facing::POS_X, STEP, &grid) {
            StepOutcome::Resolved { placement } => {
                assert_eq!(placement.position.x().raw(), WORLD_EXTENT);
            }
            StepOutcome::Blocked => panic!("flying drift is never blocked"),
        }
    }

    fn landing(area: (u8, u8, u8, u8), facing: Option<Facing>) -> Landing {
        Landing {
            map: MapNumber(3),
            area: TileArea::new(area.0, area.1, area.2, area.3)
                .unwrap()
                .to_world(),
            facing,
        }
    }

    #[test]
    fn arrival_lands_walkable_and_inside() {
        let grid = grid_with(&[(11, 12), (13, 14)]);
        let land = landing((10, 10, 20, 20), Some(Facing::POS_Y));
        let mut rng = TestRng::new(1);
        match resolve_arrival(
            Facing::POS_X,
            &land,
            &grid,
            MapEnvironment::Ground,
            &mut rng,
        ) {
            WarpOutcome::Arrived { placement } => {
                assert!(grid.walkable(placement.position));
                assert!(land.area.contains(placement.position));
                assert_eq!(placement.map, MapNumber(3));
                assert_eq!(placement.facing, Facing::POS_Y);
                assert_eq!(placement.movement, Movement::Grounded);
            }
            WarpOutcome::NoWalkableLanding => panic!("area has walkable tiles"),
        }
    }

    #[test]
    fn arrival_with_no_walkable_tile_reports_no_landing() {
        let grid = grid_with(&[]);
        let land = landing((10, 10, 20, 20), None);
        let mut rng = TestRng::new(1);
        assert_eq!(
            resolve_arrival(
                Facing::POS_X,
                &land,
                &grid,
                MapEnvironment::Ground,
                &mut rng
            ),
            WarpOutcome::NoWalkableLanding
        );
    }

    #[test]
    fn arrival_none_facing_keeps_traveler_facing() {
        let grid = grid_with(&[(11, 12)]);
        let land = landing((10, 10, 20, 20), None);
        let mut rng = TestRng::new(1);
        match resolve_arrival(
            Facing::NEG_Y,
            &land,
            &grid,
            MapEnvironment::Ground,
            &mut rng,
        ) {
            WarpOutcome::Arrived { placement } => assert_eq!(placement.facing, Facing::NEG_Y),
            WarpOutcome::NoWalkableLanding => panic!("area has a walkable tile"),
        }
    }

    #[test]
    fn sky_arrival_forces_flying() {
        let grid = grid_with(&[(11, 12)]);
        let land = landing((10, 10, 20, 20), None);
        let mut rng = TestRng::new(1);
        match resolve_arrival(Facing::POS_X, &land, &grid, MapEnvironment::Sky, &mut rng) {
            WarpOutcome::Arrived { placement } => assert_eq!(placement.movement, Movement::Flying),
            WarpOutcome::NoWalkableLanding => panic!("area has a walkable tile"),
        }
    }

    #[test]
    fn arrival_is_deterministic_and_draws_one_word() {
        let grid = grid_with(&[(11, 12), (13, 14), (18, 19)]);
        let land = landing((10, 10, 20, 20), None);
        let mut a = TestRng::new(7);
        let mut b = TestRng::new(7);
        let ra = resolve_arrival(Facing::POS_X, &land, &grid, MapEnvironment::Ground, &mut a);
        let rb = resolve_arrival(Facing::POS_X, &land, &grid, MapEnvironment::Ground, &mut b);
        assert_eq!(ra, rb);
        // One draw consumed: the next word still agrees.
        assert_eq!(a.next_u64(), b.next_u64());
    }
}
