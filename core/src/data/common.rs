//! File envelope, provenance, and the game's numeric identities. Unit and
//! geometry vocabulary lives in [`crate::components`]; this module holds only
//! what every data file embeds for identity and provenance.

use serde::{Deserialize, Serialize};

pub use crate::components::item_ref::ItemRef;
pub use crate::components::units::MapNumber;

/// Envelope of every `/data/*.json` file: the file's records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataFile<T> {
    /// The file's records.
    pub records: Vec<T>,
}

/// Dataset era a record's values were extracted from.
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

/// Extraction provenance carried by a record (embed with `#[serde(flatten)]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Dataset era the record's values come from.
    pub source_version: SourceVersion,
    /// Curation doubt: why this value needs an authentic source before trust.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
}

/// Monster or NPC number: Monster.txt's first column, referenced by
/// MonsterSetBase and the client protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MonsterNumber(
    /// The monster number as the client knows it.
    pub u16,
);

/// Skill number as carried by Skill.txt and the client protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SkillNumber(
    /// The skill number as the client knows it.
    pub u16,
);

/// Gate number: Gate.txt row identity, referenced by Move.txt and the client
/// warp command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GateNumber(
    /// The gate number.
    pub u16,
);
