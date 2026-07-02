//! Record shape of `stats.json` — the stat catalog every other file
//! references by slug.

use serde::{Deserialize, Serialize};

use super::common::{SourceVersion, StatId};

/// Where a stat sits in the attribute system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatScope {
    /// Player-increasable base attribute.
    Base,
    /// Computed from other stats.
    Derived,
    /// Current/maximum pair (health, mana, ...).
    Resource,
    /// 0/1 flag.
    Flag,
    /// Class-local formula helper.
    Intermediate,
}

/// One entry of the stat catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stat {
    /// The stat's slug, the key other files reference.
    pub id: StatId,
    /// Hard cap on the stat's value; absent = uncapped.
    pub max_value: Option<f64>,
    /// Where the stat sits in the attribute system.
    pub scope: StatScope,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}
