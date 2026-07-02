//! Static game data definitions.
//!
//! One module per `/data/*.json` schema file, plus [`common`] for shapes
//! shared across files. Every file deserializes into
//! [`common::DataFile`]`<T>` with the module's record type as `T`.
//! The core defines the types and the rules that read them; hosts load
//! and provide the actual data at startup.

pub mod chaos_mixes;
pub mod character_classes;
pub mod common;
pub mod drop_groups;
pub mod exp_tables;
pub mod game_constants;
pub mod gates_warps;
pub mod item_definitions;
pub mod item_level_bonus_tables;
pub mod item_options;
pub mod item_sets;
pub mod magic_effects;
pub mod map_definitions;
pub mod monster_definitions;
pub mod skills;
pub mod spawn_areas;
pub mod stats;
