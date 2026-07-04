//! The `option_roll` section of `game_config.json` — option roll chances and
//! the two per-drop caps. Every chance is `ChancePer10000`; both caps are the
//! only per-drop caps in the domain, homed here together. Review: every value
//! is an OpenMU initializer default pending authentic sources — flagged in the
//! data.

use serde::{Deserialize, Serialize};

use crate::components::levels::OptionLevel;
use crate::components::units::ChancePer10000;

/// Option roll chances and per-drop caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OptionRollPolicy {
    /// Chance a dropped item carries a normal option.
    pub item_option_roll_per_10000: ChancePer10000,
    /// Chance a dropped item carries luck.
    pub luck_roll_per_10000: ChancePer10000,
    /// Chance of each additional excellent option beyond the guaranteed first,
    /// rolled per remaining slot.
    pub extra_excellent_option_roll_per_10000: ChancePer10000,
    /// Cap on excellent options rolled onto one drop.
    pub max_excellent_options_per_drop: u8,
    /// Highest normal option level a drop can carry. Typed `OptionLevel`: the
    /// cap is itself a legal level, and cap comparisons are `Ord` over the
    /// enum's variants.
    pub max_dropped_option_level: OptionLevel,
    /// Review flag — every shipped value is an OpenMU default pending authentic
    /// sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
}
