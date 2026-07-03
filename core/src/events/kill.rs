//! The complete outcome of one kill: what it dropped, the experience it
//! granted, and the levels that experience crossed. One record bundling the two
//! independent rewards (loot and progression) the kill service orchestrates.

use serde::{Deserialize, Serialize};

use crate::events::loot::DropResolution;
use crate::events::progression::{ExpAward, LevelUp};

/// Everything one kill produced: its drops, its experience award, and any
/// level-ups the award crossed. The kill service composes the independent loot
/// and experience results into this single event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillResolution {
    /// What the kill dropped.
    pub drops: DropResolution,
    /// The experience the kill granted.
    pub experience: ExpAward,
    /// Levels the killer crossed, ascending.
    pub level_ups: Vec<LevelUp>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::units::{Exp, Level, Zen};
    use crate::events::loot::Drop;

    #[test]
    fn kill_resolution_round_trips() {
        let resolution = KillResolution {
            drops: DropResolution {
                category: Drop::Zen { amount: Zen(107) },
                specials: Vec::new(),
            },
            experience: ExpAward { gained: Exp(100) },
            level_ups: vec![LevelUp {
                level: Level::new(2).unwrap(),
            }],
        };
        let json = serde_json::to_string(&resolution).unwrap();
        assert_eq!(
            json,
            r#"{"drops":{"category":{"kind":"zen","amount":107},"specials":[]},"experience":{"gained":100},"level_ups":[{"level":2}]}"#
        );
        assert_eq!(
            serde_json::from_str::<KillResolution>(&json).unwrap(),
            resolution
        );
    }
}
