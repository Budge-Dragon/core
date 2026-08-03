//! The real checked-in `/data` dataset, loaded whole — the single
//! [`StaticData`] field list every dataset-driven suite shares, so a new data
//! file joins the test harness with exactly one edit here.
//!
//! Two ports out: [`real_static_data`] hands the un-parsed [`StaticData`] to
//! negative tests that corrupt a record before [`Atlas::parse`];
//! [`real_atlas`] hands everything else the parsed [`Atlas`]. Load failures
//! route through [`or_fail`]; the checked-in dataset makes them infallible
//! (proven by `data_files.rs`).
//!
//! Compiled in two shapes: as `common::dataset` inside the simulation suite's
//! `common` module, and directly via `#[path]` by test binaries that need the
//! dataset without the simulation plumbing.

use std::path::PathBuf;

use serde::de::DeserializeOwned;

use mu_core::data::ancient_sets::AncientSet;
use mu_core::data::atlas::{Atlas, StaticData};
use mu_core::data::box_drops::BoxDrop;
use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::classes::ClassRecord;
use mu_core::data::common::{DataFile, MapNumber};
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_config::GameConfig;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::MonsterDefinition;
use mu_core::data::npc_shops::MerchantShop;
use mu_core::data::skills::Skill;
use mu_core::data::spawns::Spawn;
use mu_core::data::special_drops::SpecialDropRecord;
use mu_core::data::terrain::{MapTerrain, TerrainBytes};

/// Resolves a `Result` the calling test's own setup makes impossible — a load
/// of the checked-in dataset is one such case, a fixture lookup the scenario
/// just seeded is another. Fails the calling test, so a harness fault names the
/// test it broke and leaves the rest of the binary running.
#[cfg(test)]
pub fn or_fail<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("mu-core test harness: {error}"),
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

/// Reads and deserializes a real data file into its `DataFile<T>`.
fn load<T: DeserializeOwned>(name: &str) -> DataFile<T> {
    let text = or_fail(std::fs::read_to_string(data_path(name)));
    or_fail(serde_json::from_str(&text))
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
            MapTerrain {
                map: MapNumber(map),
                bytes: or_fail(TerrainBytes::new(or_fail(std::fs::read(&path)))),
            }
        })
        .collect()
}

/// The whole real dataset, un-parsed: every v2 file plus the 11 terrain
/// sidecars. The mutable pre-parse port — negative tests corrupt a record on
/// the returned value before handing it to [`Atlas::parse`].
#[must_use]
pub fn real_static_data() -> StaticData {
    StaticData {
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
        // Schema-only family: no minigame.json ships this era, so the real
        // dataset's absent-file default is the empty record list.
        mini_games: DataFile {
            records: Vec::new(),
        },
        terrain: load_terrain(),
    }
}

/// The real, whole-dataset [`Atlas`] — [`real_static_data`] cross-checked by
/// [`Atlas::parse`].
#[must_use]
pub fn real_atlas() -> Atlas {
    or_fail(Atlas::parse(real_static_data()))
}
