//! Record shapes of `character_classes.json` — playable classes plus one
//! global pseudo-record of formulas shared by all classes.

use serde::{Deserialize, Serialize};

use super::common::{Aggregate, ClassId, MapRef, Operator, SourceVersion, StatId, StatValue};

/// A record of `character_classes.json`: a playable class or the single
/// pseudo-record with `"id": "global"`.
///
/// The spec carries no `kind` tag here — the `global` id discriminates.
/// `Class` is tried first; [`ClassGlobals`] rejects unknown fields, so a
/// malformed class record cannot silently parse as the global record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CharacterClassRecord {
    /// A playable character class.
    Class(CharacterClass),
    /// Formulas and constant values shared by every class.
    Global(ClassGlobals),
}

/// A playable character class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterClass {
    /// The class slug, the key other files reference.
    pub id: ClassId,
    /// Class number as the client knows it.
    pub number: u8,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Whether players can create this class directly.
    pub created_by_player: bool,
    /// Account level that unlocks creation; `0` = always available.
    pub creation_unlock_level: u16,
    /// Map a new character of this class starts on.
    pub home_map: MapRef,
    /// Stat points granted per level-up.
    pub points_per_level: u8,
    /// Which fruit point-calculation the class uses.
    pub fruit_calculation: FruitCalculation,
    /// Percent knocked off warp level requirements (34 for Magic Gladiator).
    pub warp_level_reduction_percent: u8,
    /// Starting attribute values.
    pub base_stats: Vec<BaseStat>,
    /// Constant stat values the class always has.
    pub const_values: Vec<StatValue>,
    /// Attribute relationships between stats (data, not code).
    pub stat_formulas: Vec<StatFormula>,
    /// Second-class evolution; absent = the class does not evolve.
    pub evolution: Option<Evolution>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Formulas and constant values shared by every class.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClassGlobals {
    /// The fixed slug `"global"`.
    pub id: String,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Constant stat values every class has.
    pub const_values: Vec<StatValue>,
    /// Attribute relationships shared by every class.
    pub stat_formulas: Vec<StatFormula>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Which fruit point-calculation a class uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FruitCalculation {
    /// Standard calculation.
    Default,
    /// Magic Gladiator variant.
    MagicGladiator,
    /// Dark Lord variant.
    DarkLord,
}

/// A starting attribute value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaseStat {
    /// Stat being set.
    pub stat: StatId,
    /// Starting value.
    pub value: f64,
    /// Whether level-up points can raise it.
    pub increasable: bool,
}

/// One attribute relationship: `target += input <operator> operand`,
/// folded per its aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatFormula {
    /// Stat the formula contributes to.
    pub target: StatId,
    /// Stat the formula reads.
    pub input: StatId,
    /// How input and operand combine.
    pub operator: Operator,
    /// Constant or stat-valued operand.
    pub operand: FormulaOperand,
    /// How the result folds into the target's total.
    pub aggregate: Aggregate,
}

/// Operand of a stat formula, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormulaOperand {
    /// A fixed number.
    Constant {
        /// The constant value.
        value: f64,
    },
    /// Another stat's current value (dynamic multipliers, 0/1 gates).
    Stat {
        /// The stat read as operand.
        stat: StatId,
    },
}

/// Second-class evolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evolution {
    /// Class evolved into.
    pub class: ClassId,
    /// Character level at which evolution unlocks.
    pub at_level: u16,
}
