//! Shared plumbing for the end-to-end movement simulation suite.
//!
//! `core/tests/` files are separate test-crate binaries, so they cannot reach
//! the private helpers in `data_files.rs`. This module — a `mod.rs` in a
//! subdirectory, so it is compiled as part of the including binary rather than
//! as its own test binary — re-exposes the pieces the simulation tests share:
//! the deterministic [`TestRng`], the real-dataset [`Atlas`] loader
//! [`real_atlas`] (re-exported from [`dataset`], the loader shared beyond the
//! simulation suite), the host-side behavior join [`behaviors_by_number`], and
//! the ambient-simulation driver [`simulate`] that produces the per-tick,
//! per-mob [`Frame`] trace the invariant tests read.
//!
//! The driver plays the host: it populates every map in `map_handles()` order
//! and threads one seeded RNG stream through population and then every tick in
//! `Vec` index order, so the whole run is replayable bit-for-bit. Live mobs are
//! kept as an ordered `Vec` (never a map keyed by monster number — many mobs
//! share a number), which is what makes iteration order load-bearing and
//! determinism observable.

pub mod dataset;

use std::collections::BTreeMap;

use rand_core::RngCore;

use mu_core::components::spatial::{Fixed, UNITS_PER_TILE};
use mu_core::components::tile::WalkGrid;
use mu_core::components::units::{Tick, TickDuration};
use mu_core::data::atlas::{Atlas, MapHandle};
use mu_core::data::common::MonsterNumber;
use mu_core::data::monster_definitions::{MobBehavior, MonsterRole};
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::entities::spawned::Spawned;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::services::effects::mobility;
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::spawn::populate_map;

use dataset::or_abort;
pub use dataset::real_atlas;

/// A one-tile step in sub-units — the mob movement grain used by the step
/// services in the integration tests.
pub const ONE_TILE: Fixed = Fixed::from_raw(UNITS_PER_TILE);

/// The simulation tick length shared by the whole suite: 50 ms.
#[must_use]
pub fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// Deterministic `SplitMix64` — the exact stream `data_files.rs` uses, mirrored
/// here so population and every tick replay identically across targets.
pub struct TestRng {
    state: u64,
}

impl TestRng {
    /// Seeds the stream.
    #[must_use]
    pub fn new(seed: u64) -> Self {
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

/// The host-side behavior join: a monster number to the `MobBehavior` of the
/// definition it names. Only the three combat roles carry a behavior; the
/// passive roles (`Npc`, `SoccerBall`) never spawn a live mob, so they
/// contribute no entry.
#[must_use]
pub fn behaviors_by_number(atlas: &Atlas) -> BTreeMap<MonsterNumber, MobBehavior> {
    let mut behaviors = BTreeMap::new();
    for definition in atlas.monsters() {
        match &definition.role {
            MonsterRole::Monster { behavior, .. }
            | MonsterRole::Guard { behavior, .. }
            | MonsterRole::Trap { behavior, .. } => {
                behaviors.insert(definition.number, *behavior);
            }
            MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {}
        }
    }
    behaviors
}

/// One tick's decision for one mob, captured whole: the mob before and after,
/// the intent it chose, and the walkability of the positions the delta
/// invariants care about. The full `before`/`after` instances let every
/// invariant read what it needs; equality of the ordered `Vec<Frame>` is the
/// determinism trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    /// The tick this decision was made at.
    pub tick: u64,
    /// The mob's index in its map's ordered live set.
    pub mob_index: usize,
    /// The mob before the decision.
    pub before: MonsterInstance,
    /// The mob after the decision.
    pub after: MonsterInstance,
    /// The intent the decision returned.
    pub intent: MonsterIntent,
    /// Whether the mob's position before the decision was walkable.
    pub before_walkable: bool,
    /// Whether the mob's position after the decision is walkable.
    pub after_walkable: bool,
    /// Whether the mob's spawn anchor is walkable (constant for the run).
    pub anchor_walkable: bool,
}

/// One live map in the ambient simulation: its walk grid and its ordered set of
/// live mobs, each paired with the behavior the host joined to it.
struct LiveMap {
    grid: WalkGrid,
    mobs: Vec<(MonsterInstance, MobBehavior)>,
}

/// Runs the ambient simulation over the given map handles and returns the full
/// per-tick, per-mob trace. Populates each map in the handle order with one
/// seeded RNG stream, then ticks `Tick(0)..Tick(n)`, deciding every mob's
/// action (no player target — pure ambient roaming) in `Vec` index order and
/// threading the same RNG stream through. Deterministic given `(handles, seed,
/// n)`.
#[must_use]
pub fn simulate(
    handles: &[MapHandle<'_>],
    behaviors: &BTreeMap<MonsterNumber, MobBehavior>,
    seed: u64,
    n: u64,
) -> Vec<Frame> {
    let mut rng = TestRng::new(seed);
    let mut live: Vec<LiveMap> = Vec::new();
    for handle in handles {
        let population = populate_map(handle, &mut rng);
        let mut mobs = Vec::new();
        for spawned in &population.spawned {
            if let Spawned::Mob { instance } = spawned {
                if let Some(behavior) = behaviors.get(&instance.number) {
                    mobs.push((*instance, *behavior));
                }
            }
        }
        live.push(LiveMap {
            grid: handle.walk_grid().clone(),
            mobs,
        });
    }

    let tick = tick();
    let mut frames = Vec::new();
    for t in 0..n {
        let now = Tick(t);
        for map in &mut live {
            let LiveMap { grid, mobs } = map;
            for (index, slot) in mobs.iter_mut().enumerate() {
                let (before, behavior) = *slot;
                // The host derives the mob's movement capability from its active
                // effects and supplies it, keeping the AI service effect-unaware.
                let capability = mobility(&before.active_effects);
                let (after, intent) = decide_monster_action(
                    &before, &behavior, None, now, tick, grid, capability, &mut rng,
                );
                frames.push(Frame {
                    tick: t,
                    mob_index: index,
                    before,
                    after,
                    intent,
                    before_walkable: grid.walkable(before.placement.position),
                    after_walkable: grid.walkable(after.placement.position),
                    anchor_walkable: grid.walkable(before.anchor),
                });
                slot.0 = after;
            }
        }
    }
    frames
}
