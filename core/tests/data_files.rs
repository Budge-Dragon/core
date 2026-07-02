//! Deserialization contract for the static data files under `/data`.
//!
//! Every schema file must exist, parse into its `DataFile<T>` type, carry
//! `schema_version == 1`, and hold at least one record (`exp_tables` and
//! `game_constants` hold exactly one).

use std::path::PathBuf;

use serde::de::DeserializeOwned;

use mu_core::data::chaos_mixes::ChaosMix;
use mu_core::data::character_classes::CharacterClassRecord;
use mu_core::data::common::DataFile;
use mu_core::data::drop_groups::DropGroup;
use mu_core::data::exp_tables::ExpTable;
use mu_core::data::game_constants::GameConstants;
use mu_core::data::gates_warps::GateWarpRecord;
use mu_core::data::item_definitions::ItemDefinition;
use mu_core::data::item_level_bonus_tables::ItemLevelBonusTable;
use mu_core::data::item_options::ItemOptionDefinition;
use mu_core::data::item_sets::ItemSet;
use mu_core::data::magic_effects::MagicEffect;
use mu_core::data::map_definitions::MapDefinition;
use mu_core::data::monster_definitions::MonsterDefinition;
use mu_core::data::skills::Skill;
use mu_core::data::spawn_areas::SpawnArea;
use mu_core::data::stats::Stat;

fn load<T: DeserializeOwned>(file_name: &str) -> Result<DataFile<T>, String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../data")
        .join(file_name);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("missing data file {}: {error}", path.display()))?;
    let file: DataFile<T> = serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    assert_eq!(file.schema_version, 1, "{file_name}: schema_version");
    assert!(!file.records.is_empty(), "{file_name}: no records");
    Ok(file)
}

#[test]
fn stats_parse() {
    load::<Stat>("stats.json").unwrap();
}

#[test]
fn character_classes_parse() {
    load::<CharacterClassRecord>("character_classes.json").unwrap();
}

#[test]
fn item_definitions_parse() {
    load::<ItemDefinition>("item_definitions.json").unwrap();
}

#[test]
fn item_level_bonus_tables_parse() {
    load::<ItemLevelBonusTable>("item_level_bonus_tables.json").unwrap();
}

#[test]
fn item_options_parse() {
    load::<ItemOptionDefinition>("item_options.json").unwrap();
}

#[test]
fn item_sets_parse() {
    load::<ItemSet>("item_sets.json").unwrap();
}

#[test]
fn skills_parse() {
    load::<Skill>("skills.json").unwrap();
}

#[test]
fn magic_effects_parse() {
    load::<MagicEffect>("magic_effects.json").unwrap();
}

#[test]
fn monster_definitions_parse() {
    load::<MonsterDefinition>("monster_definitions.json").unwrap();
}

#[test]
fn spawn_areas_parse() {
    load::<SpawnArea>("spawn_areas.json").unwrap();
}

#[test]
fn map_definitions_parse() {
    load::<MapDefinition>("map_definitions.json").unwrap();
}

#[test]
fn gates_warps_parse() {
    load::<GateWarpRecord>("gates_warps.json").unwrap();
}

#[test]
fn drop_groups_parse() {
    load::<DropGroup>("drop_groups.json").unwrap();
}

#[test]
fn chaos_mixes_parse() {
    load::<ChaosMix>("chaos_mixes.json").unwrap();
}

#[test]
fn exp_tables_parse() {
    let file = load::<ExpTable>("exp_tables.json").unwrap();
    assert_eq!(
        file.records.len(),
        1,
        "exp_tables.json: expected exactly one record"
    );
}

#[test]
fn game_constants_parse() {
    let file = load::<GameConstants>("game_constants.json").unwrap();
    assert_eq!(
        file.records.len(),
        1,
        "game_constants.json: expected exactly one record"
    );
}
