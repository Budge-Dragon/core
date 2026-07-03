//! What a kill's experience award produced: the experience gained and the
//! levels it crossed. Two flat records returned by the experience service; the
//! host applies them to the killer's stored total and level.

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
}
