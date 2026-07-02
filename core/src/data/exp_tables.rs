//! Record shape of `exp_tables.json` — the precomputed experience curve.
//! Single-record file; it owns the level cap.

use serde::{Deserialize, Serialize};

use super::common::SourceVersion;

/// The experience table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExpTable {
    /// Highest reachable character level.
    pub max_level: u16,
    /// Human-readable description of the curve; documentation only, the
    /// table below is authoritative.
    pub formula: String,
    /// Total experience required per level; index = level.
    pub total_exp_by_level: Vec<u64>,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}
