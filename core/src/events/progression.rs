//! What a kill's experience award produced: the experience gained, the levels
//! it crossed, and what applying that award did to a character. Two flat records
//! plus the growth event returned by the experience service; the host applies
//! them to the killer's stored total and level.

use serde::{Deserialize, Serialize};

use crate::components::units::{Exp, Level};

/// The experience one kill granted the killer. A bare gained amount; the host
/// adds it to the killer's stored total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpAward {
    /// Experience gained from the kill.
    pub gained: Exp,
}

/// One level the killer crossed as a result of a kill's experience award. The
/// service emits these in ascending order, one per level reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelUp {
    /// The level now reached.
    pub level: Level,
}

/// What applying a kill's experience did to a character: the top level it
/// crossed and whether the award ran into the level cap. Returned as a
/// `Vec<GrowthEvent>` — empty when experience moved but no level crossed and
/// nothing was discarded; the host delivers each outward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrowthEvent {
    /// One or more levels were crossed: the character now holds `reached`, and
    /// `points_granted` unspent stat points were banked. `points_granted` is the
    /// APPLIED delta — the observable increase after the `u16` wallet saturates,
    /// not the nominal grant. The refilled pools ride on the returned character.
    LevelsGained {
        /// The top level now held.
        reached: Level,
        /// Unspent stat points actually banked by this award.
        points_granted: u16,
    },
    /// The award met the level cap and its over-cap surplus was discarded; the
    /// stored total now rests exactly at the cap. Bare marker — the capped total
    /// rides on the returned character.
    MaxLevelReached,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_award_wire_pins() {
        let award = ExpAward { gained: Exp(1234) };
        assert_eq!(serde_json::to_string(&award).unwrap(), r#"{"gained":1234}"#);
        assert_eq!(
            serde_json::from_str::<ExpAward>(r#"{"gained":1234}"#).unwrap(),
            award
        );
    }

    #[test]
    fn level_up_wire_pins() {
        let level_up = LevelUp {
            level: Level::new(51).unwrap(),
        };
        assert_eq!(serde_json::to_string(&level_up).unwrap(), r#"{"level":51}"#);
        assert_eq!(
            serde_json::from_str::<LevelUp>(r#"{"level":51}"#).unwrap(),
            level_up
        );
    }

    #[test]
    fn growth_event_wire_pins() {
        let gained = GrowthEvent::LevelsGained {
            reached: Level::new(31).unwrap(),
            points_granted: 5,
        };
        assert_eq!(
            serde_json::to_string(&gained).unwrap(),
            r#"{"kind":"levels_gained","reached":31,"points_granted":5}"#
        );
        assert_eq!(
            serde_json::from_str::<GrowthEvent>(
                r#"{"kind":"levels_gained","reached":31,"points_granted":5}"#
            )
            .unwrap(),
            gained
        );
        let maxed = GrowthEvent::MaxLevelReached;
        assert_eq!(
            serde_json::to_string(&maxed).unwrap(),
            r#"{"kind":"max_level_reached"}"#
        );
        assert_eq!(
            serde_json::from_str::<GrowthEvent>(r#"{"kind":"max_level_reached"}"#).unwrap(),
            maxed
        );
    }
}
