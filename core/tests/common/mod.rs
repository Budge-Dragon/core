//! Shared plumbing for the end-to-end movement simulation suite.
//!
//! `core/tests/` files are separate test-crate binaries, so they cannot reach
//! the private helpers in `data_files.rs`. This module — a `mod.rs` in a
//! subdirectory, so it is compiled as part of the including binary rather than
//! as its own test binary — re-exposes the pieces the simulation tests share:
//! the deterministic [`TestRng`], the real-dataset [`Atlas`] loader
//! [`real_atlas`], the host-side behavior join [`behaviors_by_number`], and the
//! ambient-simulation driver [`simulate`] that produces the per-tick,
//! per-mob [`Frame`] trace the invariant tests read.
//!
//! The driver plays the host: it populates every map in `map_handles()` order
//! and threads one seeded RNG stream through population and then every tick in
//! `Vec` index order, so the whole run is replayable bit-for-bit. Live mobs are
//! kept as an ordered `Vec` (never a map keyed by monster number — many mobs
//! share a number), which is what makes iteration order load-bearing and
//! determinism observable.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use rand_core::RngCore;

use mu_core::components::spatial::{Fixed, UNITS_PER_TILE};
use mu_core::components::tile::WalkGrid;
use mu_core::components::units::{Tick, TickDuration};
use mu_core::data::ancient_sets::AncientSet;
use mu_core::data::atlas::{Atlas, MapHandle, StaticData};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::classes::ClassRecord;
use mu_core::data::common::{DataFile, MapNumber, MonsterNumber};
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_config::GameConfig;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::{MobBehavior, MonsterDefinition, MonsterRole};
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::entities::spawned::Spawned;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::services::effects::mobility;
use mu_core::services::monster_ai::decide_monster_action;
use mu_core::services::spawn::populate_map;

/// A one-tile step in sub-units — the mob movement grain used by the step
/// services in the integration tests.
pub const ONE_TILE: Fixed = Fixed::from_raw(UNITS_PER_TILE);

/// The simulation tick length shared by the whole suite: 50 ms.
#[must_use]
pub fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// Resolves a `Result` the real checked-in dataset makes infallible: the files
/// load and parse (proven by `data_files.rs`), so an `Err` here is a broken
/// checkout, not a test condition. Reports it and aborts — a lint-clean
/// divergence, since `unwrap`/`expect`/`panic` are forbidden outside `#[test]`
/// bodies and this harness code is shared, not a test function.
fn or_abort<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "mu-core simulation harness: load failure: {error}");
            std::process::abort()
        }
    }
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

/// Absolute path of a real `/data/<name>.json`, relative to the crate.
fn data_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("data");
    path.push(format!("{name}.json"));
    path
}

/// Reads and deserializes a real data file into its `DataFile<T>`. Load
/// failures are confined to `or_abort` — the checked-in dataset makes them
/// infallible, so no `unwrap` (and no banned suppressor) is needed.
macro_rules! load {
    ($ty:ty, $name:expr) => {{
        let text = or_abort(std::fs::read_to_string(data_path($name)));
        let file: DataFile<$ty> = or_abort(serde_json::from_str(&text));
        file
    }};
}

/// The 11 real terrain sidecars (`data/terrain/<map>.bin`, maps `0..=10`).
fn load_terrain() -> Vec<MapTerrain> {
    (0u8..=10)
        .map(|map| {
            let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            path.push("..");
            path.push("data");
            path.push("terrain");
            path.push(format!("{map}.bin"));
            let bytes = or_abort(std::fs::read(&path));
            MapTerrain {
                map: MapNumber(map),
                bytes: or_abort(TerrainBytes::new(bytes)),
            }
        })
        .collect()
}

/// The real, whole-dataset [`Atlas`] — every v2 file plus the 11 terrain
/// sidecars, cross-checked by `Atlas::parse`.
#[must_use]
pub fn real_atlas() -> Atlas {
    let data = StaticData {
        maps: load!(MapDefinition, "map_definitions"),
        gates_warps: load!(GateWarpRecord, "gates_warps"),
        monsters: load!(MonsterDefinition, "monster_definitions"),
        spawns: load!(Spawn, "spawns"),
        skills: load!(Skill, "skills"),
        items: load!(ItemDefinition, "item_definitions"),
        box_drops: load!(BoxDrop, "box_drops"),
        special_drops: load!(SpecialDropRecord, "special_drops"),
        ancient_sets: load!(AncientSet, "ancient_sets"),
        chaos_mixes: load!(ChaosMix, "chaos_mixes"),
        classes: load!(ClassRecord, "classes"),
        exp_tables: load!(ExpTable, "exp_tables"),
        game_config: load!(GameConfig, "game_config"),
        terrain: load_terrain(),
    };
    or_abort(Atlas::parse(data))
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
