//! Shared shapes used by every static-data schema file.

use serde::{Deserialize, Serialize};

/// Envelope of every `/data/*.json` file: a schema version plus its records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DataFile<T> {
    /// Schema revision the records conform to.
    pub schema_version: u32,
    /// The file's records.
    pub records: Vec<T>,
}

/// Dataset era a record was extracted from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SourceVersion {
    /// The 0.75 dataset (including 0.75 initializers reused by 0.95d).
    #[serde(rename = "075")]
    V075,
    /// The 0.95d dataset.
    #[serde(rename = "095d")]
    V095d,
    /// A curated 1.0-era backport from the Season 6 dataset.
    #[serde(rename = "s6")]
    S6,
}

/// Reference to an item definition by the game's own item identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemRef {
    /// Item group (weapon, armor, jewel, ...).
    pub group: u8,
    /// Item number within its group.
    pub number: u16,
}

/// Reference to a map definition by number plus discriminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MapRef {
    /// Map number as the client knows it.
    pub number: i16,
    /// Distinguishes variants sharing a map number; `0` for the plain map.
    pub discriminator: u32,
}

/// Slug reference into `stats.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct StatId(
    /// The stat slug, e.g. `"total_strength"`.
    pub String,
);

/// Slug reference into `character_classes.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ClassId(
    /// The class slug, e.g. `"dark_knight"`.
    pub String,
);

/// Slug reference into `magic_effects.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct EffectId(
    /// The effect slug, e.g. `"greater_damage"`.
    pub String,
);

/// Slug reference into `item_options.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct OptionId(
    /// The option-definition slug, e.g. `"excellent_physical"`.
    pub String,
);

/// Slug reference into `item_level_bonus_tables.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BonusTableId(
    /// The bonus-table slug, e.g. `"weapon_damage"`.
    pub String,
);

/// Slug reference into `item_sets.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SetGroupId(
    /// The set-group slug, e.g. `"warrior_leather"`.
    pub String,
);

/// Slug reference into `drop_groups.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DropGroupId(
    /// The drop-group slug, e.g. `"default_money"`.
    pub String,
);

/// Reference to a skill by its game number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SkillNumber(
    /// The skill number as the client knows it.
    pub u16,
);

/// Reference to a monster or NPC definition by its game number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MonsterNumber(
    /// The monster number as the client knows it.
    pub u16,
);

/// Reference to a gate record in `gates_warps.json` by its gate number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GateNumber(
    /// The gate number.
    pub u16,
);

/// How a power-up folds into a stat total.
///
/// Semantics: `total = sum(add_raw) * product(multiplicate) + sum(add_final)`;
/// `maximum` takes the larger of the values instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Aggregate {
    /// Summed before multipliers apply.
    AddRaw,
    /// Multiplies the summed raw value.
    Multiplicate,
    /// Added after multipliers apply.
    AddFinal,
    /// The larger of the contributed values wins.
    Maximum,
}

/// Arithmetic operator used by stat formulas and scaling entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    /// `input * operand`.
    Multiply,
    /// `input + operand`.
    Add,
    /// `input ^ operand`.
    Exponentiate,
    /// `operand ^ input`.
    ExponentiateByAttribute,
    /// `min(input, operand)`.
    Minimum,
    /// `max(input, operand)`.
    Maximum,
}

/// A dynamic scaling term: the referenced stat combined with a constant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScaledBy {
    /// Stat whose current value feeds the scaling.
    pub stat: StatId,
    /// How the stat combines with the operand.
    pub operator: Operator,
    /// Constant operand of the scaling term.
    pub operand: f64,
}

/// A stat modification granted by an item, option, effect, or map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerUp {
    /// Stat the power-up modifies.
    pub stat: StatId,
    /// Base value contributed.
    pub value: f64,
    /// How the value folds into the stat total.
    pub aggregate: Aggregate,
    /// Dynamic scaling terms added on top of the base value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scaled_by: Vec<ScaledBy>,
    /// Cap on the final contributed value; absent = uncapped.
    pub max: Option<f64>,
}

/// A plain stat/value pair (constant stat values, monster stats).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatValue {
    /// Stat being set.
    pub stat: StatId,
    /// The value.
    pub value: f64,
}

/// A minimum-stat requirement (equipping items, casting skills, entering maps).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatRequirement {
    /// Stat that is checked.
    pub stat: StatId,
    /// Minimum required value.
    pub value: u32,
}

/// Inclusive rectangle in map coordinates; points have `x1 == x2, y1 == y2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Rect {
    /// Left edge.
    pub x1: u8,
    /// Top edge.
    pub y1: u8,
    /// Right edge.
    pub x2: u8,
    /// Bottom edge.
    pub y2: u8,
}

/// A single map coordinate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate.
    pub x: u8,
    /// Y coordinate.
    pub y: u8,
}

/// Eight-way compass direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// West.
    West,
    /// South-west.
    SouthWest,
    /// South.
    South,
    /// South-east.
    SouthEast,
    /// East.
    East,
    /// North-east.
    NorthEast,
    /// North.
    North,
    /// North-west.
    NorthWest,
}
