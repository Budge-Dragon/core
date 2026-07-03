//! What one kill dropped, kind-tagged. A [`Drop`] is one produced item: zen, an
//! item instance at a plus level and rarity, or nothing. A [`DropResolution`]
//! bundles the single category roll's outcome with any special drops the kill
//! also yielded. Data only; the loot service decides, this reports.

use serde::{Deserialize, Serialize};

use crate::components::item_quality::ItemRarity;
use crate::components::units::{ItemLevel, Zen};
use crate::data::common::ItemRef;

/// One thing a kill produced, kind-tagged: a zen amount, an item instance, or
/// nothing. `Nothing` is a real outcome (the roll landed in the no-drop
/// remainder), not an absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Drop {
    /// A zen drop.
    Zen {
        /// The zen amount.
        amount: Zen,
    },
    /// An item instance drop.
    Item {
        /// Which item dropped.
        item: ItemRef,
        /// The dropped instance's plus level.
        level: ItemLevel,
        /// The dropped instance's rarity tier.
        rarity: ItemRarity,
    },
    /// The kill dropped nothing.
    Nothing,
}

/// A kill's full drop outcome: the single category roll's result plus every
/// special drop the kill also produced. Specials are additive — a kill can
/// yield a category drop and one or more specials in the same event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DropResolution {
    /// The category roll's outcome (money, item, jewel, excellent, or nothing).
    pub category: Drop,
    /// Special drops the kill also produced, in resolution order.
    pub specials: Vec<Drop>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zen_drop_wire_pins() {
        let drop = Drop::Zen { amount: Zen(42) };
        assert_eq!(
            serde_json::to_string(&drop).unwrap(),
            r#"{"kind":"zen","amount":42}"#
        );
        assert_eq!(
            serde_json::from_str::<Drop>(r#"{"kind":"zen","amount":42}"#).unwrap(),
            drop
        );
    }

    #[test]
    fn item_drop_wire_pins() {
        let drop = Drop::Item {
            item: ItemRef {
                group: 0,
                number: 3,
            },
            level: ItemLevel::new(2).unwrap(),
            rarity: ItemRarity::Excellent,
        };
        assert_eq!(
            serde_json::to_string(&drop).unwrap(),
            r#"{"kind":"item","item":{"group":0,"number":3},"level":2,"rarity":"excellent"}"#
        );
    }

    #[test]
    fn nothing_drop_wire_pins() {
        assert_eq!(
            serde_json::to_string(&Drop::Nothing).unwrap(),
            r#"{"kind":"nothing"}"#
        );
    }

    #[test]
    fn drop_resolution_round_trips() {
        let resolution = DropResolution {
            category: Drop::Zen { amount: Zen(107) },
            specials: vec![Drop::Item {
                item: ItemRef {
                    group: 14,
                    number: 13,
                },
                level: ItemLevel::ZERO,
                rarity: ItemRarity::Normal,
            }],
        };
        let json = serde_json::to_string(&resolution).unwrap();
        assert_eq!(
            json,
            r#"{"category":{"kind":"zen","amount":107},"specials":[{"kind":"item","item":{"group":14,"number":13},"level":0,"rarity":"normal"}]}"#
        );
        assert_eq!(
            serde_json::from_str::<DropResolution>(&json).unwrap(),
            resolution
        );
    }
}
