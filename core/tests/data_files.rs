//! v2 static-data contract, proven against the real regenerated `/data`.
//!
//! Every v2 file is read from disk, deserialized into its `DataFile<T>` record
//! type with its expected record count, the loader
//! total structures (`ClassTable`, `ExpCurve`, `AncientRoster`, `DropBands`) are
//! built, and the whole-dataset [`Atlas`] referential-integrity proof runs over
//! the real cross-references. Reading and parsing are macros so their `unwrap`s
//! expand inside the `#[test]` functions that call them, where `clippy.toml`
//! permits them (`allow-unwrap-in-tests`).

use std::path::PathBuf;

use rand_core::RngCore;

use mu_core::components::class::CharacterClass;
use mu_core::components::interval::Interval;
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::{AmmoLevel, EnhanceLevel};
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::spatial::{Facing, Fixed, WorldPos};
use mu_core::components::tile::{TileArea, TileCoord, WalkGrid};
use mu_core::components::units::{ItemLevel, Level};
use mu_core::data::ancient_sets::{AncientRoster, AncientSet};
use mu_core::data::atlas::{
    Atlas, AtlasError, Landing, ResolvedOutput, ResolvedRecipe, StaticData,
};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::{ChaosMix, UpgradeTarget};
use mu_core::data::classes::{ClassRecord, ClassTable, ClassTableError};
use mu_core::data::common::{DataFile, ItemRef, MapNumber, MonsterNumber};
use mu_core::data::drop_config::DropConfig;
use mu_core::data::exp_tables::{ExpCurve, ExpTable};
use mu_core::data::game_config::GameConfig;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::map_definitions::{MapDefinition, MapEnvironment};
use mu_core::data::monster_definitions::MonsterDefinition;
use mu_core::data::npc_shops::MerchantShop;
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::spawns::{SpawnPlacement, SpawnSchedule};
use mu_core::data::special_drops::{DropBand, DropBands, SpecialDrop, SpecialDropRecord};
use mu_core::data::terrain::{MapTerrain, TerrainBytes};
use mu_core::entities::spawned::Spawned;
use mu_core::events::movement::{StepOutcome, WarpOutcome};
use mu_core::services::movement::{resolve_arrival, resolve_step};
use mu_core::services::spawn::{place_spawn, populate_map};

use mu_core::services::item_rules::{
    ammunition_damage_percent, armor_defense_bonus, effective_drop_level, max_durability,
    weapon_damage_bonus,
};

/// Absolute path of a real `/data/<name>.json`, relative to the crate.
fn data_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("data");
    path.push(format!("{name}.json"));
    path
}

/// Loads the 11 real terrain sidecars (`data/terrain/<map>.bin`, maps `0..=10`)
/// into the `StaticData.terrain` port. A macro like `load!`, so its `unwrap`s
/// expand at the `#[test]` call site where `clippy.toml` permits them.
macro_rules! load_terrain {
    () => {{
        (0u8..=10)
            .map(|map| {
                let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                path.push("..");
                path.push("data");
                path.push("terrain");
                path.push(format!("{map}.bin"));
                let bytes = std::fs::read(&path).unwrap();
                MapTerrain {
                    map: MapNumber(map),
                    bytes: TerrainBytes::new(bytes).unwrap(),
                }
            })
            .collect::<Vec<MapTerrain>>()
    }};
}

/// Reads and deserializes a real data file into its `DataFile<T>`, asserting the
/// expected record count. The `unwrap`s expand at the `#[test]` call site, where
/// `clippy.toml` permits them.
macro_rules! load {
    ($ty:ty, $name:expr, $count:expr) => {{
        let path = data_path($name);
        let text = std::fs::read_to_string(&path).unwrap();
        let file: DataFile<$ty> = serde_json::from_str(&text).unwrap();
        assert_eq!(file.records.len(), $count, "{}.json record count", $name);
        file
    }};
}

/// The full [`StaticData`] loaded from the real files — one `load!` per file, so
/// every file's parse + schema + record count is proven on the way.
macro_rules! static_data {
    () => {
        StaticData {
            maps: load!(MapDefinition, "map_definitions", 11),
            gates_warps: load!(GateWarpRecord, "gates_warps", 71),
            monsters: load!(MonsterDefinition, "monster_definitions", 100),
            spawns: load!(Spawn, "spawns", 1847),
            skills: load!(Skill, "skills", 51),
            items: load!(ItemDefinition, "item_definitions", 243),
            box_drops: load!(BoxDrop, "box_drops", 1),
            special_drops: load!(SpecialDropRecord, "special_drops", 9),
            ancient_sets: load!(AncientSet, "ancient_sets", 36),
            chaos_mixes: load!(ChaosMix, "chaos_mixes", 10),
            shops: load!(MerchantShop, "npc_shops", 11),
            classes: load!(ClassRecord, "classes", 8),
            exp_tables: load!(ExpTable, "exp_tables", 1),
            game_config: load!(GameConfig, "game_config", 1),
            terrain: load_terrain!(),
        }
    };
}

#[test]
fn every_v2_file_parses_with_expected_record_count() {
    // Each `load!` asserts the record count; touching each field proves
    // all thirteen files deserialize into their record types.
    let data = static_data!();
    assert_eq!(data.maps.records.len(), 11);
    assert_eq!(data.gates_warps.records.len(), 71);
    assert_eq!(data.monsters.records.len(), 100);
    assert_eq!(data.spawns.records.len(), 1847);
    assert_eq!(data.skills.records.len(), 51);
    assert_eq!(data.items.records.len(), 243);
    assert_eq!(data.box_drops.records.len(), 1);
    assert_eq!(data.special_drops.records.len(), 9);
    assert_eq!(data.ancient_sets.records.len(), 36);
    assert_eq!(data.chaos_mixes.records.len(), 10);
    assert_eq!(data.shops.records.len(), 11);
    assert_eq!(data.classes.records.len(), 8);
    assert_eq!(data.exp_tables.records.len(), 1);
    assert_eq!(data.game_config.records.len(), 1);
}

#[test]
fn class_table_builds_from_real_data() {
    let classes = load!(ClassRecord, "classes", 8);
    let table = ClassTable::try_from(classes.records).unwrap();
    // Dark Lord is the sole command class and carries client code 16.
    let dark_lord = table.record(CharacterClass::DarkLord);
    assert_eq!(dark_lord.number.0, 16);
    assert_eq!(
        table.class_by_number(dark_lord.number),
        Some(CharacterClass::DarkLord)
    );
}

#[test]
fn exp_curve_builds_from_real_data() {
    let exp = load!(ExpTable, "exp_tables", 1);
    let record = exp.records.into_iter().next().unwrap();
    let curve = ExpCurve::parse(record).unwrap();
    assert_eq!(curve.max_level().get(), 400);
    assert_eq!(curve.level(1).unwrap().total_to_hold().0, 0);
    let top = curve.level(400).unwrap().total_to_hold().0;
    assert!(top > 0);
    assert!(curve.level(0).is_err());
    assert!(curve.level(401).is_err());
}

#[test]
fn ancient_roster_builds_from_real_data() {
    let sets = load!(AncientSet, "ancient_sets", 36);
    let roster = AncientRoster::build(sets.records).unwrap();
    assert_eq!(roster.sets().len(), 36);
}

#[test]
fn game_config_and_special_drops_build_from_real_data() {
    let config = load!(GameConfig, "game_config", 1);
    let record = config.records.into_iter().next().unwrap();
    // The category numerators must fit under the 10000 ceiling for the residual
    // "nothing" weight to be well-formed.
    assert!(record.drops.nothing_weight() <= 10_000);

    // Every level-banded special drop parses into a total, ascending band table.
    let special = load!(SpecialDropRecord, "special_drops", 9);
    for record in &special.records {
        if let SpecialDrop::LevelBanded { bands, .. } = &record.drop {
            assert!(!bands.bands().is_empty());
        }
    }
}

#[test]
fn atlas_resolves_the_whole_real_dataset() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    assert_eq!(atlas.maps().count(), 11);
    // 71 gate/warp records include exactly 14 warp entries.
    assert_eq!(atlas.warps().count(), 14);
    // Lorencia's spawn gate is proven present by construction.
    let fallback = atlas.fallback_spawn_gate();
    assert_eq!(fallback.map.0, 0);
}

#[test]
fn every_map_carries_its_pinned_respawn_map() {
    let data = static_data!();
    let expected = [
        (0u8, 0u8),
        (1, 0),
        (2, 2),
        (3, 3),
        (4, 4),
        (5, 0),
        (6, 6),
        (7, 7),
        (8, 8),
        (9, 3),
        (10, 4),
    ];
    for (number, respawn_map) in expected {
        let record = data
            .maps
            .records
            .iter()
            .find(|m| m.number == MapNumber(number))
            .unwrap();
        assert_eq!(
            record.respawn_map,
            MapNumber(respawn_map),
            "map {number} respawn_map"
        );
    }
}

#[test]
fn every_death_map_resolves_a_destination_gate_spanning_the_town_set() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    // Every one of the 11 died-on maps resolves a respawn destination gate whose
    // retained landing tiles are all walkable. The destination set {0,2,3,4,6,7,8}
    // is a subset of the gate-owning maps: map 9 owns a gate yet is never a
    // destination (it redirects to Noria, map 3).
    let mut destinations = std::collections::BTreeSet::new();
    for map in 0u8..=10 {
        let view = atlas.respawn_gate_for_death_map(MapNumber(map)).unwrap();
        let grid = atlas.walk_grid(view.map).unwrap();
        for &landing in view.landing.iter() {
            assert!(
                grid.walkable(landing),
                "map {map} destination landing walkable"
            );
        }
        destinations.insert(view.map.0);
    }
    assert_eq!(
        destinations,
        std::collections::BTreeSet::from([0u8, 2, 3, 4, 6, 7, 8])
    );
    // An arbitrary map no record carries has no respawn destination.
    assert!(atlas.respawn_gate_for_death_map(MapNumber(200)).is_none());
}

#[test]
fn atlas_rejects_a_respawn_map_pointing_at_a_gate_less_map() {
    let mut data = static_data!();
    // Rewrite Devias's respawn_map to Dungeon (map 1), which owns no spawn gate.
    for map in &mut data.maps.records {
        if map.number == MapNumber(2) {
            map.respawn_map = MapNumber(1);
        }
    }
    let err = Atlas::parse(data).unwrap_err();
    assert!(matches!(
        err,
        AtlasError::RespawnMapWithoutSpawnGate {
            map: MapNumber(2),
            respawn_map: MapNumber(1),
        }
    ));
}

#[test]
fn atlas_retains_the_chaos_recipe_catalog_joined_in_scan_order() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let recipes: Vec<&ResolvedRecipe> = atlas.chaos_recipes().collect();
    assert_eq!(recipes.len(), 10);

    // Descending authentic crafting-number order: Cape (24) first, Chaos
    // Weapon (1) last, +11 (4) strictly before +10 (3).
    assert!(matches!(
        recipes.first(),
        Some(ResolvedRecipe::CapeOfLord { .. })
    ));
    assert!(matches!(
        recipes.last(),
        Some(ResolvedRecipe::ChaosWeapon { .. })
    ));
    let upgrade_targets: Vec<UpgradeTarget> = recipes
        .iter()
        .filter_map(|recipe| match recipe {
            ResolvedRecipe::ItemUpgrade { target, .. } => Some(*target),
            ResolvedRecipe::ChaosWeapon { .. }
            | ResolvedRecipe::FirstWings { .. }
            | ResolvedRecipe::SecondWings { .. }
            | ResolvedRecipe::CapeOfLord { .. }
            | ResolvedRecipe::Dinorant { .. }
            | ResolvedRecipe::Fruits { .. }
            | ResolvedRecipe::DevilSquareTicket { .. }
            | ResolvedRecipe::BloodCastleTicket { .. } => None,
        })
        .collect();
    assert_eq!(
        upgrade_targets,
        vec![UpgradeTarget::PlusEleven, UpgradeTarget::PlusTen]
    );

    // The join carries real definitions: multi-candidate families hold a
    // Choice pool of the record's size, single-output families a Single.
    for recipe in &recipes {
        match recipe {
            ResolvedRecipe::ChaosWeapon {
                weapons: output, ..
            }
            | ResolvedRecipe::FirstWings { wings: output, .. } => match output {
                ResolvedOutput::Choice(pool) => assert_eq!(pool.count().get(), 3),
                ResolvedOutput::Single(_) => panic!("a 3-weapon family joins a Choice"),
            },
            ResolvedRecipe::SecondWings { wings: output, .. } => match output {
                ResolvedOutput::Choice(pool) => assert_eq!(pool.count().get(), 4),
                ResolvedOutput::Single(_) => panic!("second wings join a 4-wing Choice"),
            },
            ResolvedRecipe::CapeOfLord { cape: output, .. }
            | ResolvedRecipe::Dinorant {
                dinorant: output, ..
            }
            | ResolvedRecipe::Fruits { fruit: output, .. }
            | ResolvedRecipe::DevilSquareTicket {
                invitation: output, ..
            }
            | ResolvedRecipe::BloodCastleTicket { cloak: output, .. } => match output {
                ResolvedOutput::Single(_) => {}
                ResolvedOutput::Choice(_) => panic!("a single-output family joins a Single"),
            },
            ResolvedRecipe::ItemUpgrade { .. } => {}
        }
    }
}

#[test]
fn atlas_retains_the_resolved_dataset_for_by_id_lookup() {
    let atlas = Atlas::parse(static_data!()).unwrap();

    // By-id lookups reach the retained definitions instead of re-scanning a Vec;
    // an open id that names no record is genuine `None`.
    assert!(
        atlas
            .item(ItemRef {
                group: 0,
                number: 0
            })
            .is_some()
    );
    assert!(atlas.monster(MonsterNumber(0)).is_some());
    assert!(
        atlas
            .item(ItemRef {
                group: 200,
                number: 0
            })
            .is_none()
    );

    // The resolved structures the loader validated are held on the Atlas.
    assert_eq!(
        atlas.classes().record(CharacterClass::DarkLord).number.0,
        16
    );
    assert_eq!(atlas.exp_curve().max_level().get(), 400);
    assert_eq!(atlas.ancient_roster().sets().len(), 36);
    assert!(atlas.drop_config().nothing_weight() <= 10_000);

    // The drop pool is a per-level index: a wide window yields droppable items,
    // and every pooled ref resolves through the retained item lookup.
    let window = Interval::new(0u8, 255u8).unwrap();
    let droppable: Vec<ItemRef> = atlas.drop_pool().in_window(window).collect();
    assert!(!droppable.is_empty());
    assert!(droppable.iter().all(|&id| atlas.item(id).is_some()));
}

#[test]
fn atlas_loads_terrain_walk_grids() {
    let atlas = Atlas::parse(static_data!()).unwrap();

    // `walk_grid` is total: parse proves every map carries exactly one sidecar.
    let maps: Vec<MapNumber> = atlas.maps().map(|map| map.number).collect();
    assert_eq!(maps.len(), 11);
    for map in maps {
        assert!(atlas.walk_grid(map).is_some());
    }

    // Lorencia (map 0) is a fully walkable town: a town tile and an open tile
    // both walk (SafeZone `0x01` stays walkable).
    let lorencia = atlas.walk_grid(MapNumber(0)).unwrap();
    assert!(lorencia.walkable(TileCoord::new(135, 125).to_world()));
    assert!(lorencia.walkable(TileCoord::new(10, 10).to_world()));
    // (0,0) is a NoMove (`0x02`) wall tile — the exact case a mask checking only
    // NoGround would wrongly report walkable.
    assert!(!lorencia.walkable(TileCoord::new(0, 0).to_world()));

    // Map 8 (Tarkan) is roughly half blocked: its (0,0) corner is a
    // NoGround (`0x04`) tile and is not walkable.
    let tarkan = atlas.walk_grid(MapNumber(8)).unwrap();
    assert!(!tarkan.walkable(TileCoord::new(0, 0).to_world()));
}

#[test]
fn atlas_rejects_a_terrain_sidecar_for_an_unknown_map() {
    let mut data = static_data!();
    let stray = TerrainBytes::new(vec![0u8; 256 * 256]).unwrap();
    data.terrain.push(MapTerrain {
        map: MapNumber(200),
        bytes: stray,
    });
    let err = Atlas::parse(data).unwrap_err();
    assert!(matches!(err, AtlasError::TerrainForUnknownMap { .. }));
}

#[test]
fn terrain_bytes_reject_a_wrong_length_buffer() {
    assert!(TerrainBytes::new(vec![0u8; 100]).is_err());
    assert!(TerrainBytes::new(vec![0u8; 256 * 256]).is_ok());
}

// --- Negative tests: type invariants that valid on-disk data cannot exercise.

#[test]
fn class_table_rejects_duplicate_records() {
    let classes = load!(ClassRecord, "classes", 8);
    let mut duped = classes.records.clone();
    duped.push(classes.records[0].clone());
    let err = ClassTable::try_from(duped).unwrap_err();
    assert!(matches!(
        err,
        ClassTableError::DuplicateClass(_) | ClassTableError::DuplicateNumber(_)
    ));
}

#[test]
fn atlas_rejects_a_dangling_monster_reference() {
    let mut data = static_data!();
    let dangling = r#"{"records":[
     {"map":0,"monster":9999,"placement":{"kind":"spot","position":{"x":10,"y":10},"quantity":1},"schedule":{"kind":"permanent"},"source_version":"075"}
    ]}"#;
    data.spawns = serde_json::from_str(dangling).unwrap();
    let err = Atlas::parse(data).unwrap_err();
    assert!(matches!(err, AtlasError::UnknownMonsterRef { .. }));
}

#[test]
fn drop_bands_reject_non_ascending_and_resolve_levels() {
    let band = |min: u16, lvl: u8| DropBand {
        min_monster_level: Level::new(min).unwrap(),
        item_level: ItemLevel::new(lvl).unwrap(),
    };
    let bands = DropBands::try_from(vec![band(2, 1), band(36, 2)]).unwrap();
    assert_eq!(bands.item_level_for(Level::new(1).unwrap()), None);
    assert_eq!(
        bands.item_level_for(Level::new(2).unwrap()).unwrap().get(),
        1
    );
    assert_eq!(
        bands.item_level_for(Level::new(50).unwrap()).unwrap().get(),
        2
    );
    assert!(DropBands::try_from(vec![band(36, 2), band(2, 1)]).is_err());
}

#[test]
fn drop_config_rejects_category_sum_above_ceiling() {
    let json = r#"{"money_roll_per_10000":6000,"item_roll_per_10000":6000,"jewel_roll_per_10000":0,"excellent_roll_per_10000":0,"skill_roll_per_10000":5000,"jewel_drops":[{"group":14,"number":13}]}"#;
    assert!(serde_json::from_str::<DropConfig>(json).is_err());
}

#[test]
fn item_rules_curve_accessors_are_total_over_enhance_level() {
    let levels = [
        EnhanceLevel::L0,
        EnhanceLevel::L1,
        EnhanceLevel::L2,
        EnhanceLevel::L3,
        EnhanceLevel::L4,
        EnhanceLevel::L5,
        EnhanceLevel::L6,
        EnhanceLevel::L7,
        EnhanceLevel::L8,
        EnhanceLevel::L9,
        EnhanceLevel::L10,
        EnhanceLevel::L11,
    ];
    for level in levels {
        let _ = weapon_damage_bonus(level);
        let _ = armor_defense_bonus(level);
    }
    assert_eq!(weapon_damage_bonus(EnhanceLevel::L0), 0);
    assert_eq!(weapon_damage_bonus(EnhanceLevel::L11), 36);
    assert_eq!(armor_defense_bonus(EnhanceLevel::L10), 31);
}

#[test]
fn effective_drop_level_applies_surcharges() {
    assert_eq!(
        effective_drop_level(6, EnhanceLevel::L0, ItemRarity::Normal),
        6
    );
    assert_eq!(
        effective_drop_level(6, EnhanceLevel::L1, ItemRarity::Normal),
        9
    );
    assert_eq!(
        effective_drop_level(6, EnhanceLevel::L0, ItemRarity::Excellent),
        31
    );
    assert_eq!(
        effective_drop_level(6, EnhanceLevel::L0, ItemRarity::Ancient),
        36
    );
}

#[test]
fn ammunition_and_durability_accessors() {
    assert_eq!(ammunition_damage_percent(AmmoLevel::L0), 0);
    assert_eq!(ammunition_damage_percent(AmmoLevel::L2), 5);
    assert_eq!(max_durability(20, EnhanceLevel::L0, ItemRarity::Normal), 20);
    assert_eq!(
        max_durability(20, EnhanceLevel::L11, ItemRarity::Excellent),
        20 + 21 + 15
    );
}

// --- Spawn placement over the real dataset.

/// Deterministic `SplitMix64` for replayable population over real data.
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

fn position_of(spawned: &Spawned) -> WorldPos {
    match spawned {
        Spawned::Mob { instance } => instance.placement.position,
        Spawned::Placed { placement, .. } => placement.position,
    }
}

/// The instance count a permanent spawn contributes, computed independently of
/// the RNG loop (Fixed → 1, Spot → quantity, Area → quantity when at least one
/// tile in the rect is walkable, else 0).
fn expected_instances(placement: SpawnPlacement, grid: &WalkGrid) -> usize {
    match placement {
        SpawnPlacement::Fixed { .. } => 1,
        SpawnPlacement::Spot { quantity, .. } => usize::from(quantity),
        SpawnPlacement::Area { area, quantity } => {
            if grid.walkable_positions_in(area.to_world()).next().is_some() {
                usize::from(quantity)
            } else {
                0
            }
        }
    }
}

#[test]
fn every_real_map_populates_with_full_health_mobs() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let mut rng = TestRng::new(2024);
    let mut handles = 0;
    for handle in atlas.map_handles() {
        handles += 1;
        let population = populate_map(&handle, &mut rng);
        for spawned in &population.spawned {
            if let Spawned::Mob { instance } = spawned {
                assert_eq!(instance.health.current(), instance.health.max());
                assert!(instance.health.max() > 0);
            }
        }
    }
    assert_eq!(handles, 11);
}

#[test]
fn every_area_placed_instance_sits_on_a_walkable_tile() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let mut rng = TestRng::new(99);
    for handle in atlas.map_handles() {
        let grid = handle.walk_grid();
        for entry in handle.spawns() {
            if entry.spawn.schedule != SpawnSchedule::Permanent {
                continue;
            }
            if let SpawnPlacement::Area { .. } = entry.spawn.placement {
                let result = place_spawn(
                    entry.monster,
                    &entry.spawn.placement,
                    grid,
                    handle.definition().number,
                    &mut rng,
                );
                for spawned in &result.spawned {
                    assert!(grid.walkable(position_of(spawned)));
                }
            }
        }
    }
}

#[test]
fn wandering_spawns_are_excluded_from_initial_population() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let mut rng = TestRng::new(7);
    for handle in atlas.map_handles() {
        let grid = handle.walk_grid();
        let expected: usize = handle
            .spawns()
            .filter(|entry| entry.spawn.schedule == SpawnSchedule::Permanent)
            .map(|entry| expected_instances(entry.spawn.placement, grid))
            .sum();
        let population = populate_map(&handle, &mut rng);
        assert_eq!(population.spawned.len(), expected);
        assert_eq!(population.events.len(), expected);
    }
}

#[test]
fn arena_resolves_the_soccer_pitch_and_lorencia_has_none() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let mut rng = TestRng::new(1);

    let arena = atlas.map_handle(MapNumber(6)).unwrap();
    let pitch = populate_map(&arena, &mut rng)
        .soccer_pitch
        .expect("arena resolves a soccer pitch");
    assert_eq!(
        pitch.ground,
        TileArea::new(55, 141, 69, 180).unwrap().to_world()
    );
    assert_eq!(pitch.left_spawn, TileCoord::new(60, 156).to_world());

    let lorencia = atlas.map_handle(MapNumber(0)).unwrap();
    assert!(populate_map(&lorencia, &mut rng).soccer_pitch.is_none());
}

#[test]
fn whole_dataset_population_is_deterministic() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let populate = |seed: u64| {
        let mut rng = TestRng::new(seed);
        atlas
            .map_handles()
            .map(|handle| populate_map(&handle, &mut rng))
            .collect::<Vec<_>>()
    };
    assert_eq!(populate(555), populate(555));
}

#[test]
fn map_handles_enumerate_every_map_and_join_spawns() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    assert_eq!(atlas.map_handles().count(), 11);

    // A present map yields a walk grid directly (no Option) and spawn entries
    // joined to their monster definition.
    let lorencia = atlas.map_handle(MapNumber(0)).unwrap();
    let _grid: &WalkGrid = lorencia.walk_grid();
    let mut joined = 0;
    for entry in lorencia.spawns() {
        assert_eq!(entry.monster.number, entry.spawn.monster);
        joined += 1;
    }
    assert!(joined > 0);

    // An open key that names no map yields no handle.
    assert!(atlas.map_handle(MapNumber(200)).is_none());
}

// --- Movement and flight over the real Atlas.

/// A one-tile step in sub-units — the mob movement grain.
const ONE_TILE: Fixed = Fixed::from_raw(65_536);

fn grounded(tile: (u8, u8), map: MapNumber) -> Placement {
    Placement {
        position: TileCoord::new(tile.0, tile.1).to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map,
    }
}

/// The first enter gate found scanning maps in order, with its resolved
/// landing. The only public path to an enter gate is a positional trigger
/// query, so this walks the grid until one covers a tile. `None` only if the
/// dataset carries no enter gate at all.
fn first_enter_gate_landing(atlas: &Atlas) -> Option<Landing> {
    for map in atlas.maps().map(|m| m.number).collect::<Vec<_>>() {
        for y in 0u8..=255 {
            for x in 0u8..=255 {
                let pos = TileCoord::new(x, y).to_world();
                if let Some(view) = atlas.enter_gate_at(map, pos) {
                    return Some(view.landing);
                }
            }
        }
    }
    None
}

/// Asserts a landing resolves to a walkable tile inside its own area on every
/// seed, or reports no-landing only when the area truly holds no walkable tile.
fn assert_landing_resolves(landing: &Landing, grid: &WalkGrid, env: MapEnvironment) {
    for seed in 0u64..32 {
        let mut rng = TestRng::new(seed);
        match resolve_arrival(Facing::POS_X, landing, grid, env, &mut rng) {
            WarpOutcome::Arrived { placement } => {
                assert!(grid.walkable(placement.position), "seed {seed}");
                assert!(landing.area.contains(placement.position), "seed {seed}");
                assert_eq!(placement.map, landing.map);
            }
            WarpOutcome::NoWalkableLanding => {
                assert!(
                    grid.walkable_positions_in(landing.area).next().is_none(),
                    "seed {seed}: reported no landing but the area has a walkable tile"
                );
            }
        }
    }
}

#[test]
fn grounded_steps_respect_real_terrain_walls() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let lorencia = atlas.walk_grid(MapNumber(0)).unwrap();

    // (0,0) is a NoMove wall: a grounded step onto it from (1,0) is blocked.
    let wall = TileCoord::new(0, 0).to_world();
    assert_eq!(
        resolve_step(grounded((1, 0), MapNumber(0)), wall, ONE_TILE, lorencia),
        StepOutcome::Blocked
    );

    // A flying step crosses the same wall.
    let flyer = Placement {
        movement: Movement::Flying,
        ..grounded((1, 0), MapNumber(0))
    };
    match resolve_step(flyer, wall, ONE_TILE, lorencia) {
        StepOutcome::Resolved { placement } => assert_eq!(placement.position, wall),
        StepOutcome::Blocked => panic!("flying ignores walkability"),
    }

    // A grounded step onto an adjacent walkable town tile resolves onto it.
    let town = TileCoord::new(135, 125).to_world();
    assert!(lorencia.walkable(town));
    let neighbor = [
        TileCoord::new(136, 125).to_world(),
        TileCoord::new(134, 125).to_world(),
        TileCoord::new(135, 126).to_world(),
        TileCoord::new(135, 124).to_world(),
    ]
    .into_iter()
    .find(|&n| lorencia.walkable(n))
    .expect("a town-centre tile has a walkable neighbour");
    let on_town = Placement {
        position: town,
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: MapNumber(0),
    };
    match resolve_step(on_town, neighbor, ONE_TILE, lorencia) {
        StepOutcome::Resolved { placement } => assert_eq!(placement.position, neighbor),
        StepOutcome::Blocked => panic!("a walkable neighbour must resolve"),
    }

    // Tarkan (map 8) (0,0) is NoGround: a grounded step there is blocked.
    let tarkan = atlas.walk_grid(MapNumber(8)).unwrap();
    assert_eq!(
        resolve_step(
            grounded((1, 0), MapNumber(8)),
            TileCoord::new(0, 0).to_world(),
            ONE_TILE,
            tarkan
        ),
        StepOutcome::Blocked
    );
}

#[test]
fn every_real_warp_and_an_enter_gate_landing_resolve() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let mut warps = 0;
    for warp in atlas.warps() {
        warps += 1;
        let handle = atlas.map_handle(warp.landing.map).unwrap();
        assert_landing_resolves(
            &warp.landing,
            handle.walk_grid(),
            handle.definition().environment,
        );
    }
    assert_eq!(warps, 14);

    let enter = first_enter_gate_landing(&atlas).expect("the dataset has an enter gate");
    let handle = atlas.map_handle(enter.map).unwrap();
    assert_landing_resolves(&enter, handle.walk_grid(), handle.definition().environment);
}

#[test]
fn unspecified_landing_facing_keeps_the_traveler_facing() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let landing = atlas
        .warps()
        .map(|warp| warp.landing)
        .find(|landing| landing.facing.is_none())
        .expect("a real warp landing with unspecified facing");
    let handle = atlas.map_handle(landing.map).unwrap();
    let env = handle.definition().environment;
    let grid = handle.walk_grid();
    let traveler = Facing::NEG_Y;
    let mut rng = TestRng::new(4);
    match resolve_arrival(traveler, &landing, grid, env, &mut rng) {
        WarpOutcome::Arrived { placement } => assert_eq!(placement.facing, traveler),
        WarpOutcome::NoWalkableLanding => panic!("this landing has walkable tiles"),
    }
}

#[test]
fn whole_dataset_arrival_is_deterministic() {
    let atlas = Atlas::parse(static_data!()).unwrap();
    let run = |seed: u64| {
        atlas
            .warps()
            .map(|warp| {
                let handle = atlas.map_handle(warp.landing.map).unwrap();
                let mut rng = TestRng::new(seed);
                resolve_arrival(
                    Facing::POS_X,
                    &warp.landing,
                    handle.walk_grid(),
                    handle.definition().environment,
                    &mut rng,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(321), run(321));
}
