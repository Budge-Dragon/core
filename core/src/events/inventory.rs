//! The outcome events of the container operations: place, remove, move, equip,
//! and unequip. One kind-tagged outcome enum per operation — the movement-service
//! grain — because each op's payload differs: a rejected place hands an
//! [`ItemInstance`] back, a successful remove/unequip hands one out, a move
//! carries two cells. A move-only item is never dropped: every reject arm that
//! consumed one hands it back.
//!
//! These events carry only components; the ground-item pickup outcome (which
//! hands back a whole world-item entity on reject) lives in the inventory
//! service, since an event never imports an entity.

use serde::{Deserialize, Serialize};

use crate::components::equipment::EquipmentSlot;
use crate::components::inventory::{Cell, PlacementRejection};
use crate::components::item_instance::ItemInstance;

/// What a place produced, kind-tagged: the item was stored at a cell, or the
/// placement was rejected and the item handed back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlaceOutcome {
    /// The item was stored; `at` is its anchor cell.
    Placed {
        /// The anchor cell it was stored at.
        at: Cell,
    },
    /// The placement was rejected; the bounced item is handed back.
    Rejected {
        /// Why the placement was rejected.
        reason: PlacementRejection,
        /// The item that could not be placed.
        item: ItemInstance,
    },
}

/// What a remove produced, kind-tagged: the item was taken out, or no item
/// covered the addressed cell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoveOutcome {
    /// The item was removed from `at` and handed out.
    Removed {
        /// The cell the removal was addressed to.
        at: Cell,
        /// The removed item.
        item: ItemInstance,
    },
    /// The removal was rejected; nothing changed.
    Rejected {
        /// Why the removal was rejected.
        reason: PlacementRejection,
    },
}

/// What a move produced, kind-tagged: the item was re-anchored, or the move was
/// rejected. No item crosses the boundary — it stayed inside the container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MoveOutcome {
    /// The item moved from `from` to `to`.
    Moved {
        /// The cell moved from.
        from: Cell,
        /// The cell moved to.
        to: Cell,
    },
    /// The move was rejected; nothing changed.
    Rejected {
        /// Why the move was rejected.
        reason: PlacementRejection,
    },
}

/// What an equip produced, kind-tagged: the item was worn in a slot, or the
/// equip was rejected and the item handed back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EquipOutcome {
    /// The item was worn in `slot`.
    Equipped {
        /// The slot the item was worn in.
        slot: EquipmentSlot,
    },
    /// The equip was rejected; the bounced item is handed back.
    Rejected {
        /// Why the equip was rejected.
        reason: EquipRejection,
        /// The item that could not be equipped.
        item: ItemInstance,
    },
}

/// What an unequip produced, kind-tagged: the slot's item was taken off, or the
/// slot was already empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnequipOutcome {
    /// The item was taken off `slot` and handed out.
    Unequipped {
        /// The slot the item was taken off.
        slot: EquipmentSlot,
        /// The removed item.
        item: ItemInstance,
    },
    /// The slot held no item; nothing changed.
    SlotEmpty,
}

/// Why an equip was rejected — service-produced, decided from the item's kind,
/// the target slot, and the wearer's eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipRejection {
    /// The item's kind cannot go in that slot.
    IncompatibleSlot,
    /// The slot already holds an item.
    SlotOccupied,
    /// Two-handed dual-hand occupancy is broken: a two-handed weapon needs its
    /// paired hand empty (or ammunition beside a bow/crossbow), or the item
    /// would share a hand pair with a worn two-handed weapon.
    TwoHandedConflict,
    /// The wearer's class is not in the item's qualified-class list.
    ClassMismatch,
    /// A scaled stat requirement, or the raw level requirement, exceeds the
    /// wearer's total.
    RequirementsNotMet,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placed_and_move_wire_pins() {
        assert_eq!(
            serde_json::to_string(&PlaceOutcome::Placed {
                at: Cell { row: 1, col: 2 }
            })
            .unwrap(),
            r#"{"kind":"placed","at":{"row":1,"col":2}}"#
        );
        assert_eq!(
            serde_json::to_string(&MoveOutcome::Moved {
                from: Cell { row: 0, col: 0 },
                to: Cell { row: 3, col: 4 }
            })
            .unwrap(),
            r#"{"kind":"moved","from":{"row":0,"col":0},"to":{"row":3,"col":4}}"#
        );
    }

    #[test]
    fn rejection_wire_pins() {
        assert_eq!(
            serde_json::to_string(&MoveOutcome::Rejected {
                reason: PlacementRejection::CellsOccupied
            })
            .unwrap(),
            r#"{"kind":"rejected","reason":{"kind":"cells_occupied"}}"#
        );
        assert_eq!(
            serde_json::to_string(&EquipRejection::IncompatibleSlot).unwrap(),
            r#""incompatible_slot""#
        );
        assert_eq!(
            serde_json::to_string(&EquipRejection::TwoHandedConflict).unwrap(),
            r#""two_handed_conflict""#
        );
        assert_eq!(
            serde_json::to_string(&EquipRejection::ClassMismatch).unwrap(),
            r#""class_mismatch""#
        );
        assert_eq!(
            serde_json::to_string(&EquipRejection::RequirementsNotMet).unwrap(),
            r#""requirements_not_met""#
        );
    }

    #[test]
    fn equipped_and_slot_empty_wire_pins() {
        assert_eq!(
            serde_json::to_string(&EquipOutcome::Equipped {
                slot: EquipmentSlot::Helm
            })
            .unwrap(),
            r#"{"kind":"equipped","slot":"helm"}"#
        );
        assert_eq!(
            serde_json::to_string(&UnequipOutcome::SlotEmpty).unwrap(),
            r#"{"kind":"slot_empty"}"#
        );
    }
}
