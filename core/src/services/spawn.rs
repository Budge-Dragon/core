//! World population: the one place authoring-tile spawn records resolve to
//! walkable world positions. Every crossing into world space happens here, and
//! only through the sanctioned projections ([`crate::components::tile::TileCoord::to_world`],
//! [`crate::components::tile::TileArea::to_world`],
//! [`crate::components::tile::TileFacing::to_facing`]) — never inline tile
//! arithmetic. The injected [`RngCore`] is the sole source of randomness, routed
//! through the [`crate::rng`] seam.
//!
//! Placement is deterministic given the RNG seed: a `Fixed` spawn draws no
//! random words; a `Spot` spawn draws one facing per instance; an `Area` spawn
//! draws, per instance, a position then a facing — in that order.

use rand_core::RngCore;

use crate::components::collections::{EmptyCollection, OneOrMore};
use crate::components::movement::Movement;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::components::spatial::{Facing, WorldPos, WorldRect};
use crate::components::tile::WalkGrid;
use crate::components::units::{MapNumber, Tick};
use crate::data::atlas::MapHandle;
use crate::data::monster_definitions::{MonsterDefinition, MonsterRole};
use crate::data::spawns::{SpawnPlacement, SpawnSchedule};
use crate::entities::monster_instance::MonsterInstance;
use crate::entities::spawned::Spawned;
use crate::events::spawn::SpawnEvent;
use crate::services::chance::{draw_cardinal, pick_one};

/// The entities and events produced by resolving one spawn record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnResult {
    /// The resolved entities, one per placed instance.
    pub spawned: Vec<Spawned>,
    /// The outcome events, one per placed instance, paired with `spawned`.
    pub events: Vec<SpawnEvent>,
}

/// A map's soccer pitch projected to world space — the single consumer that
/// resolves the authoring-tile [`crate::data::map_definitions::SoccerPitch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedSoccerPitch {
    /// Playing field.
    pub ground: WorldRect,
    /// Left goal area.
    pub left_goal: WorldRect,
    /// Right goal area.
    pub right_goal: WorldRect,
    /// Left team spawn point.
    pub left_spawn: WorldPos,
    /// Right team spawn point.
    pub right_spawn: WorldPos,
}

/// The result of populating one map: its initial entities, their events, and —
/// on Arena — its resolved soccer pitch. Every outcome rides this one returned
/// value; nothing leaves through a side channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapPopulation {
    /// The entities placed at world start.
    pub spawned: Vec<Spawned>,
    /// The outcome events, paired with `spawned`.
    pub events: Vec<SpawnEvent>,
    /// The resolved soccer pitch — `Some` only on Arena (genuine optionality).
    pub soccer_pitch: Option<ResolvedSoccerPitch>,
}

/// Classifies a monster definition at a resolved position into the entity it
/// spawns and the event that announces it. Exhaustive over the five monster
/// roles: the three combat-carrying roles become a live [`Spawned::Mob`] with
/// health seeded full; the two passive roles become a [`Spawned::Placed`] with
/// no health. `Grounded` is the real traversal mode — a placed entity sits on
/// the ground — not a fabricated default. A mob anchors to its spawn position
/// and is ready to act at [`Tick(0)`](Tick).
fn spawn_at(
    monster: &MonsterDefinition,
    position: WorldPos,
    facing: Facing,
    map: MapNumber,
) -> (Spawned, SpawnEvent) {
    let placement = Placement {
        position,
        facing,
        movement: Movement::Grounded,
        map,
    };
    match &monster.role {
        MonsterRole::Monster { combat, .. }
        | MonsterRole::Guard { combat, .. }
        | MonsterRole::Trap { combat, .. } => {
            let instance = MonsterInstance {
                number: monster.number,
                placement,
                health: Pool::full(combat.hp),
                anchor: position,
                next_action: Tick(0),
            };
            let event = SpawnEvent::MobSpawned {
                number: monster.number,
                at: position,
                facing,
            };
            (Spawned::Mob { instance }, event)
        }
        MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
            let spawned = Spawned::Placed {
                number: monster.number,
                placement,
            };
            let event = SpawnEvent::ObjectPlaced {
                number: monster.number,
                at: position,
                facing,
            };
            (spawned, event)
        }
    }
}

/// Resolves one spawn record to world space. The placement primitive — a
/// unit-testable seam that takes hand-built inputs and never touches an Atlas.
///
/// - `Fixed` places one instance at the tile centre facing the authored
///   compass direction, drawing no random words.
/// - `Spot` places `quantity` instances at the tile centre, each facing a drawn
///   cardinal.
/// - `Area` places `quantity` instances, each on a walkable tile centre inside
///   the rectangle, drawing a position then a facing per instance. An area with
///   no walkable tile contributes zero instances — a genuine domain case, never
///   an unbounded retry.
#[must_use]
pub fn place_spawn(
    monster: &MonsterDefinition,
    placement: &SpawnPlacement,
    grid: &WalkGrid,
    map: MapNumber,
    rng: &mut impl RngCore,
) -> SpawnResult {
    match placement {
        SpawnPlacement::Fixed { position, facing } => {
            let (spawned, event) = spawn_at(monster, position.to_world(), facing.to_facing(), map);
            SpawnResult {
                spawned: vec![spawned],
                events: vec![event],
            }
        }
        SpawnPlacement::Spot { position, quantity } => {
            let centre = position.to_world();
            let mut result = SpawnResult {
                spawned: Vec::new(),
                events: Vec::new(),
            };
            for _ in 0..*quantity {
                let facing = draw_cardinal(rng);
                let (spawned, event) = spawn_at(monster, centre, facing, map);
                result.spawned.push(spawned);
                result.events.push(event);
            }
            result
        }
        SpawnPlacement::Area { area, quantity } => {
            let cells: Vec<WorldPos> = grid.walkable_positions_in(area.to_world()).collect();
            match OneOrMore::new(cells) {
                Err(EmptyCollection) => SpawnResult {
                    spawned: Vec::new(),
                    events: Vec::new(),
                },
                Ok(cells) => {
                    let mut result = SpawnResult {
                        spawned: Vec::new(),
                        events: Vec::new(),
                    };
                    for _ in 0..*quantity {
                        let position = *pick_one(&cells, rng);
                        let facing = draw_cardinal(rng);
                        let (spawned, event) = spawn_at(monster, position, facing, map);
                        result.spawned.push(spawned);
                        result.events.push(event);
                    }
                    result
                }
            }
        }
    }
}

/// Populates a map at world start: resolves its `Permanent` spawns to entities
/// and events (skipping `Wandering` ones via an explicit arm), then resolves
/// its soccer pitch to world space when present.
#[must_use]
pub fn populate_map(handle: &MapHandle<'_>, rng: &mut impl RngCore) -> MapPopulation {
    let mut spawned = Vec::new();
    let mut events = Vec::new();
    for entry in handle.spawns() {
        match entry.spawn.schedule {
            SpawnSchedule::Permanent => {
                let mut result = place_spawn(
                    entry.monster,
                    &entry.spawn.placement,
                    handle.walk_grid(),
                    handle.definition().number,
                    rng,
                );
                spawned.append(&mut result.spawned);
                events.append(&mut result.events);
            }
            SpawnSchedule::Wandering => {}
        }
    }
    let soccer_pitch = handle
        .definition()
        .soccer_pitch
        .map(|pitch| ResolvedSoccerPitch {
            ground: pitch.ground.to_world(),
            left_goal: pitch.left_goal.to_world(),
            right_goal: pitch.right_goal.to_world(),
            left_spawn: pitch.left_spawn.to_world(),
            right_spawn: pitch.right_spawn.to_world(),
        });
    MapPopulation {
        spawned,
        events,
        soccer_pitch,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::element::PerElement;
    use crate::components::tile::{TileArea, TileCoord, TileFacing};
    use crate::components::units::{DurationMs, Level, Resistance};
    use crate::data::common::{MonsterNumber, Provenance, SourceVersion};
    use crate::data::monster_definitions::{
        MobBehavior, MonsterAttack, MonsterCombat, TrapTargeting,
    };

    /// Deterministic `SplitMix64` for replayable tests; cast-free extraction of
    /// the low 32 bits keeps clippy's cast lints quiet in test code too.
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

    fn provenance() -> Provenance {
        Provenance {
            source_version: SourceVersion::V075,
            review: None,
        }
    }

    fn combat(hp: u32) -> MonsterCombat {
        MonsterCombat {
            level: Level::MIN,
            hp,
            min_phys_damage: 0,
            max_phys_damage: 0,
            defense: 0,
            attack_rate: 0,
            defense_rate: 0,
        }
    }

    fn resistances() -> PerElement<Resistance> {
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

    fn behavior() -> MobBehavior {
        MobBehavior {
            move_range: 0,
            attack_range: 0,
            view_range: 0,
            move_delay_ms: DurationMs(0),
            attack_delay_ms: DurationMs(0),
            respawn_ms: DurationMs(0),
        }
    }

    fn definition(number: u16, role: MonsterRole) -> MonsterDefinition {
        MonsterDefinition {
            number: MonsterNumber(number),
            provenance: provenance(),
            role,
        }
    }

    fn monster(number: u16, hp: u32) -> MonsterDefinition {
        definition(
            number,
            MonsterRole::Monster {
                combat: combat(hp),
                resistances: resistances(),
                behavior: behavior(),
                attack: MonsterAttack::Plain,
            },
        )
    }

    fn grid_with(walkable: &[(u8, u8)]) -> WalkGrid {
        let mut words = [0u64; 1024];
        for &(x, y) in walkable {
            let bit = (usize::from(y) << 8) | usize::from(x);
            words[bit >> 6] |= 1u64 << (bit & 63);
        }
        WalkGrid::from_words(words)
    }

    fn all_walkable() -> WalkGrid {
        WalkGrid::from_words([u64::MAX; 1024])
    }

    fn position_of(spawned: &Spawned) -> WorldPos {
        match spawned {
            Spawned::Mob { instance } => instance.placement.position,
            Spawned::Placed { placement, .. } => placement.position,
        }
    }

    fn facing_of(spawned: &Spawned) -> Facing {
        match spawned {
            Spawned::Mob { instance } => instance.placement.facing,
            Spawned::Placed { placement, .. } => placement.facing,
        }
    }

    /// The eight cardinal facings `draw_cardinal` can return — mirrored here so
    /// the test needs no access to the private const in `chance`.
    const CARDINALS: [Facing; 8] = [
        Facing::POS_X,
        Facing::POS_X_POS_Y,
        Facing::POS_Y,
        Facing::NEG_X_POS_Y,
        Facing::NEG_X,
        Facing::NEG_X_NEG_Y,
        Facing::NEG_Y,
        Facing::POS_X_NEG_Y,
    ];

    fn is_cardinal(facing: Facing) -> bool {
        CARDINALS.contains(&facing)
    }

    #[test]
    fn fixed_places_one_at_tile_centre_facing_authored_compass() {
        let mut rng = TestRng::new(1);
        let result = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Fixed {
                position: TileCoord::new(2, 3),
                facing: TileFacing::SouthEast,
            },
            &grid_with(&[]),
            MapNumber(0),
            &mut rng,
        );
        assert_eq!(result.spawned.len(), 1);
        assert_eq!(result.events.len(), 1);
        match &result.spawned[0] {
            Spawned::Mob { instance } => {
                assert_eq!(instance.placement.position, TileCoord::new(2, 3).to_world());
                assert_eq!(instance.placement.facing, Facing::POS_X_POS_Y);
                assert_eq!(instance.placement.movement, Movement::Grounded);
                assert_eq!(instance.health, Pool::full(60));
            }
            Spawned::Placed { .. } => panic!("a monster role must spawn a mob"),
        }
        assert_eq!(
            result.events[0],
            SpawnEvent::MobSpawned {
                number: MonsterNumber(7),
                at: TileCoord::new(2, 3).to_world(),
                facing: Facing::POS_X_POS_Y,
            }
        );
    }

    #[test]
    fn fixed_draws_no_random_words() {
        let mut ran = TestRng::new(9);
        let _ = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Fixed {
                position: TileCoord::new(2, 3),
                facing: TileFacing::South,
            },
            &grid_with(&[]),
            MapNumber(0),
            &mut ran,
        );
        let mut fresh = TestRng::new(9);
        assert_eq!(ran.next_u64(), fresh.next_u64());
    }

    #[test]
    fn spot_places_quantity_at_centre_each_facing_a_cardinal() {
        let mut rng = TestRng::new(3);
        let result = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Spot {
                position: TileCoord::new(2, 3),
                quantity: 3,
            },
            &grid_with(&[]),
            MapNumber(0),
            &mut rng,
        );
        assert_eq!(result.spawned.len(), 3);
        for spawned in &result.spawned {
            assert_eq!(position_of(spawned), TileCoord::new(2, 3).to_world());
            assert!(is_cardinal(facing_of(spawned)));
            match spawned {
                Spawned::Mob { instance } => assert_eq!(instance.health, Pool::full(60)),
                Spawned::Placed { .. } => panic!("a monster role must spawn a mob"),
            }
        }
    }

    #[test]
    fn area_places_quantity_each_walkable_and_inside() {
        let area = TileArea::new(10, 10, 20, 20).unwrap();
        let walkable = [(11u8, 12u8), (13, 14), (18, 19), (20, 20), (10, 10)];
        let grid = grid_with(&walkable);
        let mut rng = TestRng::new(11);
        let result = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Area { area, quantity: 45 },
            &grid,
            MapNumber(0),
            &mut rng,
        );
        assert_eq!(result.spawned.len(), 45);
        assert_eq!(result.events.len(), 45);
        let rect = area.to_world();
        for spawned in &result.spawned {
            let pos = position_of(spawned);
            assert!(grid.walkable(pos));
            assert!(rect.contains(pos));
        }
    }

    #[test]
    fn area_with_no_walkable_tile_places_zero() {
        let area = TileArea::new(10, 10, 20, 20).unwrap();
        let mut rng = TestRng::new(7);
        let result = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Area { area, quantity: 30 },
            &grid_with(&[]),
            MapNumber(0),
            &mut rng,
        );
        assert!(result.spawned.is_empty());
        assert!(result.events.is_empty());
    }

    #[test]
    fn area_samples_with_replacement_over_a_single_walkable_tile() {
        let area = TileArea::new(10, 10, 20, 20).unwrap();
        let only = TileCoord::new(15, 16);
        let grid = grid_with(&[(15, 16)]);
        let mut rng = TestRng::new(5);
        let result = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Area { area, quantity: 10 },
            &grid,
            MapNumber(0),
            &mut rng,
        );
        assert_eq!(result.spawned.len(), 10);
        for spawned in &result.spawned {
            assert_eq!(position_of(spawned), only.to_world());
        }
    }

    #[test]
    fn events_pair_with_placements() {
        let area = TileArea::new(0, 0, 30, 30).unwrap();
        let mut rng = TestRng::new(21);
        let result = place_spawn(
            &monster(7, 60),
            &SpawnPlacement::Area { area, quantity: 12 },
            &all_walkable(),
            MapNumber(0),
            &mut rng,
        );
        assert_eq!(result.spawned.len(), result.events.len());
        for (spawned, event) in result.spawned.iter().zip(result.events.iter()) {
            match event {
                SpawnEvent::MobSpawned { at, facing, .. } => {
                    assert_eq!(*at, position_of(spawned));
                    assert_eq!(*facing, facing_of(spawned));
                }
                SpawnEvent::ObjectPlaced { .. } => panic!("a monster role emits mob_spawned"),
            }
        }
    }

    #[test]
    fn all_five_roles_dispatch_exhaustively() {
        let grid = grid_with(&[]);
        let fixed = |facing| SpawnPlacement::Fixed {
            position: TileCoord::new(1, 1),
            facing,
        };
        let mut rng = TestRng::new(1);
        let guard = definition(
            247,
            MonsterRole::Guard {
                combat: combat(15_000),
                resistances: resistances(),
                behavior: behavior(),
            },
        );
        let trap = definition(
            102,
            MonsterRole::Trap {
                targeting: TrapTargeting::Directional,
                combat: combat(200),
                resistances: resistances(),
                behavior: behavior(),
                attack: MonsterAttack::Plain,
            },
        );
        let npc = definition(248, MonsterRole::Npc { window: None });
        let ball = definition(145, MonsterRole::SoccerBall);

        let mut mob_hp = |def: &MonsterDefinition, hp: u32| {
            let result = place_spawn(def, &fixed(TileFacing::East), &grid, MapNumber(0), &mut rng);
            match &result.spawned[0] {
                Spawned::Mob { instance } => assert_eq!(instance.health, Pool::full(hp)),
                Spawned::Placed { .. } => panic!("expected a mob"),
            }
        };
        mob_hp(&monster(7, 60), 60);
        mob_hp(&guard, 15_000);
        mob_hp(&trap, 200);

        for placed in [&npc, &ball] {
            let result = place_spawn(
                placed,
                &fixed(TileFacing::East),
                &grid,
                MapNumber(0),
                &mut rng,
            );
            match &result.spawned[0] {
                Spawned::Placed { number, .. } => assert_eq!(*number, placed.number),
                Spawned::Mob { .. } => panic!("expected a placed object"),
            }
        }
    }

    #[test]
    fn same_seed_yields_identical_placements_and_consumption() {
        let area = TileArea::new(0, 0, 40, 40).unwrap();
        let placement = SpawnPlacement::Area { area, quantity: 25 };
        let mut a = TestRng::new(7);
        let mut b = TestRng::new(7);
        let ra = place_spawn(
            &monster(7, 60),
            &placement,
            &all_walkable(),
            MapNumber(0),
            &mut a,
        );
        let rb = place_spawn(
            &monster(7, 60),
            &placement,
            &all_walkable(),
            MapNumber(0),
            &mut b,
        );
        assert_eq!(ra.spawned, rb.spawned);
        assert_eq!(ra.events, rb.events);
        // Identical RNG-word consumption: the next draw agrees.
        assert_eq!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn different_seeds_diverge_over_a_large_area() {
        let area = TileArea::new(0, 0, 40, 40).unwrap();
        let placement = SpawnPlacement::Area { area, quantity: 20 };
        let mut a = TestRng::new(7);
        let mut b = TestRng::new(42);
        let ra = place_spawn(
            &monster(7, 60),
            &placement,
            &all_walkable(),
            MapNumber(0),
            &mut a,
        );
        let rb = place_spawn(
            &monster(7, 60),
            &placement,
            &all_walkable(),
            MapNumber(0),
            &mut b,
        );
        let pa: Vec<WorldPos> = ra.spawned.iter().map(position_of).collect();
        let pb: Vec<WorldPos> = rb.spawned.iter().map(position_of).collect();
        assert_ne!(pa, pb);
    }

    #[test]
    fn every_placed_position_is_walkable_and_inside_across_seeds() {
        let area = TileArea::new(5, 5, 25, 25).unwrap();
        let walkable = [(6u8, 7u8), (10, 11), (12, 20), (24, 25), (5, 5), (25, 25)];
        let grid = grid_with(&walkable);
        let rect = area.to_world();
        for seed in 0u64..64 {
            let mut rng = TestRng::new(seed);
            let result = place_spawn(
                &monster(7, 60),
                &SpawnPlacement::Area { area, quantity: 30 },
                &grid,
                MapNumber(0),
                &mut rng,
            );
            assert_eq!(result.spawned.len(), 30);
            for spawned in &result.spawned {
                let pos = position_of(spawned);
                assert!(grid.walkable(pos), "seed {seed}");
                assert!(rect.contains(pos), "seed {seed}");
            }
        }
    }
}
