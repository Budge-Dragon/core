//! The outcome events of the ground reaper: one per drop whose despawn tick
//! was reached and which therefore left the ground. Kind-tagged, flat, built
//! from components only — core holds no host drop id, so each event locates
//! the removed drop by value (position + map) and names it by its item
//! identity or zen amount.

use serde::{Deserialize, Serialize};

use crate::components::item_ref::ItemRef;
use crate::components::spatial::WorldPos;
use crate::components::units::{MapNumber, Zen};

/// What the despawn reaper removed from the ground, kind-tagged. Flat value
/// fields — position + map to locate the drop, and the item identity / zen
/// amount to name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DespawnEvent {
    /// A ground item reached its despawn tick and left the ground.
    ItemDespawned {
        /// Where it lay.
        position: WorldPos,
        /// The map it lay on.
        map: MapNumber,
        /// Which item it was.
        item: ItemRef,
    },
    /// A zen pile reached its despawn tick and left the ground.
    ZenDespawned {
        /// Where it lay.
        position: WorldPos,
        /// The map it lay on.
        map: MapNumber,
        /// How much zen it held.
        amount: Zen,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_despawned_wire_round_trips() {
        let event = DespawnEvent::ItemDespawned {
            position: WorldPos::clamped(163_840, 229_376),
            map: MapNumber(0),
            item: ItemRef {
                group: 0,
                number: 3,
            },
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"item_despawned","position":{"x":163840,"y":229376},"map":0,"item":{"group":0,"number":3}}"#
        );
        assert_eq!(serde_json::from_str::<DespawnEvent>(&json).unwrap(), event);
    }

    #[test]
    fn zen_despawned_wire_round_trips() {
        let event = DespawnEvent::ZenDespawned {
            position: WorldPos::clamped(163_840, 229_376),
            map: MapNumber(0),
            amount: Zen(40_000),
        };
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"zen_despawned","position":{"x":163840,"y":229376},"map":0,"amount":40000}"#
        );
        assert_eq!(serde_json::from_str::<DespawnEvent>(&json).unwrap(), event);
    }
}
