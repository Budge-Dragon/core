//! Closed rosters of timed effects. Magnitudes, durations, and stacking rules
//! live in the effects services.

use serde::{Deserialize, Serialize};

/// Beneficial timed effects granted by buff skills and consumables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Buff {
    /// DK Defense skill: damage taken halved.
    Defense,
    /// Elf Greater Damage: energy-scaled attack-damage bonus.
    GreaterDamage,
    /// Elf Greater Defense: energy-scaled defense bonus.
    GreaterDefense,
    /// Soul Master Soul Barrier: percentage damage reduction.
    SoulBarrier,
    /// Knight Swell Life: party max-HP increase.
    SwellLife,
    /// Dark Lord Increase Critical Damage: party critical-damage bonus.
    CriticalDamageIncrease,
    /// Muse Elf Infinity Arrow: arrows are not consumed.
    InfiniteArrow,
    /// Ale: attack-speed bonus.
    Alcohol,
}

/// Harmful statuses inflicted by hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ailment {
    /// Periodic damage over time.
    Poisoned,
    /// Movement slowed.
    Iced,
    /// Cannot move.
    Frozen,
    /// Defense reduced.
    DefenseReduction,
}
