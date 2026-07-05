//! Static game data definitions.
//!
//! One module per `/data/*.json` schema file, plus [`common`] for shapes shared
//! across files and [`atlas`] for the dataset-wide referential-integrity proof.
//! Every file deserializes into [`common::DataFile`]`<T>` with the module's
//! record type as `T`. The core defines the types and the rules that read them;
//! hosts load and provide the actual data at startup.

pub mod ancient_sets;
pub mod atlas;
pub mod box_drops;
pub mod chaos_mixes;
pub mod classes;
pub mod common;
pub mod drop_config;
mod drop_pool;
pub mod effects;
pub mod exp_tables;
pub mod game_config;
pub mod gates_warps;
pub mod item_definitions;
pub mod map_definitions;
pub mod monster_definitions;
pub mod npc_shops;
pub mod option_roll;
pub mod skills;
pub mod spawns;
pub mod special_drops;
pub mod terrain;
