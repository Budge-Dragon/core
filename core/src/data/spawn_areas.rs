//! Record shape of `spawn_areas.json` — where and how monsters spawn.

use serde::{Deserialize, Serialize};

use super::common::{Direction, MapRef, MonsterNumber, Rect, SourceVersion};

/// One spawn area.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpawnArea {
    /// Map the spawn belongs to.
    pub map: MapRef,
    /// Monster that spawns.
    pub monster: MonsterNumber,
    /// Spawn rectangle; a point for fixed NPC positions.
    pub area: Rect,
    /// Simultaneous instances kept alive.
    pub quantity: u16,
    /// Facing direction for fixed spawns; absent = unspecified.
    pub direction: Option<Direction>,
    /// When the spawn happens, kind-tagged.
    pub trigger: SpawnTrigger,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// When a spawn area produces its monsters, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SpawnTrigger {
    /// Kept at quantity permanently.
    Automatic,
    /// Wandering spawn (traveling merchants).
    Wandering,
    /// Spawned once when an event starts.
    OnceAtEventStart,
    /// Spawned by event logic on demand.
    ManuallyForEvent,
    /// Kept at quantity while an event wave runs.
    AutomaticDuringWave {
        /// The event wave number.
        wave: u8,
    },
    /// Spawned once when an event wave starts.
    OnceAtWaveStart {
        /// The event wave number.
        wave: u8,
    },
}
