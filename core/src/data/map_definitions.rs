//! Record shape of `map_definitions.json` — game maps and their terrain
//! sidecars.

use serde::{Deserialize, Serialize};

use super::common::{DropGroupId, MapRef, Point, PowerUp, Rect, SourceVersion, StatRequirement};

/// One map definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MapDefinition {
    /// Map number as the client knows it.
    pub number: i16,
    /// Distinguishes variants sharing a map number; `0` for the plain map.
    pub discriminator: u32,
    /// The map's slug.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Path of the terrain sidecar, relative to the data directory.
    pub terrain: String,
    /// Experience multiplier applied to kills on this map.
    pub exp_multiplier: f64,
    /// Map whose safezone the dead respawn in.
    pub safezone_map: MapRef,
    /// Minimum stats required to enter.
    pub requirements: Vec<StatRequirement>,
    /// Stat modifications applied to every character on the map.
    pub character_power_ups: Vec<PowerUp>,
    /// Map-wide drop groups.
    pub drop_groups: Vec<DropGroupId>,
    /// Arena battle zone; absent = no battle zone.
    pub battle_zone: Option<BattleZone>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// The Arena battle zone layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BattleZone {
    /// Battle mode played in the zone.
    pub battle_type: BattleType,
    /// Playing field.
    pub ground: Rect,
    /// Left goal area.
    pub left_goal: Rect,
    /// Right goal area.
    pub right_goal: Rect,
    /// Left team spawn point.
    pub left_spawn: Point,
    /// Right team spawn point.
    pub right_spawn: Point,
}

/// Battle mode of a battle zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BattleType {
    /// Plain team battle.
    Normal,
    /// Battle soccer.
    Soccer,
}
