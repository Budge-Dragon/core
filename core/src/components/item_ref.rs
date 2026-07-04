//! Item identity as the game knows it — the `{group, number}` key every item
//! definition and instance is addressed by. Lives in [`crate::components`] (the
//! lowest vocabulary layer) because an item instance composes it; the static
//! data layer re-exports it, never the reverse.

use serde::{Deserialize, Serialize};

/// Item identity as the game knows it: `ItemList` group section plus index
/// (client wire `group * 512 + number`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ItemRef {
    /// Item group (weapon, armor, jewel, ...).
    pub group: u8,
    /// Item number within its group.
    pub number: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips() {
        let item = ItemRef {
            group: 0,
            number: 3,
        };
        let json = serde_json::to_string(&item).unwrap();
        assert_eq!(json, r#"{"group":0,"number":3}"#);
        assert_eq!(serde_json::from_str::<ItemRef>(&json).unwrap(), item);
    }
}
