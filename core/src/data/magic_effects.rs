//! Record shape of `magic_effects.json` — timed effects applied by skills
//! and consumables.

use serde::{Deserialize, Serialize};

use super::common::{EffectId, PowerUp, ScaledBy, SourceVersion};

/// One magic effect definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MagicEffect {
    /// The effect's slug, the key skills and consumables reference.
    pub id: EffectId,
    /// Effect number as the client knows it (wire id, not a reference key).
    pub number: i16,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Effects sharing a sub-type replace each other instead of stacking.
    pub sub_type: u8,
    /// Whether death removes the effect.
    pub stop_by_death: bool,
    /// Probability the effect applies, `0.0..=1.0`; absent in JSON = `1.0`.
    #[serde(default = "chance_certain")]
    pub chance: f64,
    /// How long the effect lasts; absent = instant (applied once, e.g. heal).
    pub duration: Option<EffectDuration>,
    /// Stat modifications while active.
    pub power_ups: Vec<PowerUp>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Duration of a magic effect: a constant plus optional stat scaling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EffectDuration {
    /// Constant part of the duration.
    pub constant_ms: u64,
    /// Stat-scaled additions, in milliseconds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scaled_by: Vec<ScaledBy>,
    /// Cap on the total duration; absent = uncapped.
    pub max_ms: Option<u64>,
}

fn chance_certain() -> f64 {
    1.0
}
