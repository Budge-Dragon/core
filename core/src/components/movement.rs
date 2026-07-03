//! Traversal mode: the two-state classification of how an entity crosses the
//! ground plane. Its only rule is whether the tile walk-grid check applies —
//! no altitude, no air combat, no anti-air.

use serde::{Deserialize, Serialize};

/// How an entity traverses the ground plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Movement {
    /// Bound to walkable tiles.
    Grounded,
    /// Free of the walk grid.
    Flying,
}

impl Movement {
    /// Whether traversal is constrained by the tile walk grid.
    #[must_use]
    pub fn checks_walkability(self) -> bool {
        match self {
            Movement::Grounded => true,
            Movement::Flying => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walkability_per_variant() {
        assert!(Movement::Grounded.checks_walkability());
        assert!(!Movement::Flying.checks_walkability());
    }

    #[test]
    fn wire_is_bare_snake_case_string() {
        assert_eq!(
            serde_json::to_string(&Movement::Grounded).unwrap(),
            r#""grounded""#
        );
        assert_eq!(
            serde_json::to_string(&Movement::Flying).unwrap(),
            r#""flying""#
        );
        assert_eq!(
            serde_json::from_str::<Movement>(r#""grounded""#).unwrap(),
            Movement::Grounded
        );
    }
}
