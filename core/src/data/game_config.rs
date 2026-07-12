//! Record shape of `game_config.json` — dataset-scoped configuration of one
//! game edition, grouped by domain concern. Single-record file per dataset.
//! Defines no unit newtypes.

use serde::{Deserialize, Serialize};

pub use crate::components::equipment::EquipmentSlot;

use crate::components::interval::Interval;
use crate::components::units::{DurationMs, TickDuration, Zen};

use super::common::Provenance;
use super::drop_config::DropConfig;
use super::option_roll::OptionRollPolicy;

/// The game-edition configuration record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameConfig {
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Real-time length of one simulation tick (ours). Guarded nonzero.
    pub tick_duration_ms: TickDuration,
    /// Ground-item despawn timing (authentic 60 s).
    pub item_drop_duration_ms: DurationMs,
    /// Per-kill drop rolls and jewel roster. Shape owned by the drops domain;
    /// its category-sum invariant is proven by its own serde parse.
    pub drops: DropConfig,
    /// Creation-time option rolls and the two per-drop caps. Shape owned by the
    /// options domain.
    pub option_roll: OptionRollPolicy,
    /// Party and experience-award facts.
    pub progression: ProgressionConfig,
    /// Zen storage caps.
    pub zen_caps: ZenCaps,
    /// Storage-grid geometry as the classic client renders it.
    pub inventory: InventoryGeometry,
}

/// Party and experience-award facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressionConfig {
    /// Uniform per-kill experience jitter. Review-flagged in the record:
    /// OpenMU mechanism and values (0.8-1.2).
    pub exp_jitter_percent: RatePercentRange,
}

/// Inclusive rate-multiplier range in whole percent points. A rate range, not a
/// probability: values exceed 100 by design, so this is deliberately not
/// `Percent` and not `ChancePer10000`. `min <= max` proven once at load.
pub type RatePercentRange = Interval<u16>;

/// Zen storage caps (classic 2,000,000,000).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZenCaps {
    /// Character inventory zen cap.
    pub inventory: Zen,
    /// Vault (warehouse) zen cap.
    pub vault: Zen,
}

/// Storage-grid geometry. Every classic storage is a rows x columns grid the
/// client renders and items occupy cell rectangles in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InventoryGeometry {
    /// Main inventory, 8x8.
    pub main: GridSize,
    /// Vault (warehouse), 15x8.
    pub vault: GridSize,
    /// Personal store, 4x8 (~1.0-era feature; review-flagged in the record).
    pub personal_store: GridSize,
    /// Trade window, 4x8.
    pub trade: GridSize,
    /// Chaos-machine window, 4x8.
    pub chaos_machine: GridSize,
}

/// A rows x columns storage grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GridSize {
    /// Grid rows.
    pub rows: u8,
    /// Grid columns.
    pub columns: u8,
}
