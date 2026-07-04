//! Container behavior: the thin `(state, intent) -> (state, outcome)` adapters
//! over the [`Inventory`] and [`Equipment`] components. Each folds the
//! component's `Result` into a per-operation outcome enum with no `unwrap` —
//! both arms bound — and never re-implements a geometry rule. The equip service
//! is the one place the `data` slot vocabulary crosses into the component
//! [`EquipSlot`] and the one place item-kind/slot compatibility is decided
//! (the component accepts any instance in any slot). Container operations draw
//! zero RNG.

use serde::{Deserialize, Serialize};

use crate::components::equipment::{EquipSlot, Equipment};
use crate::components::inventory::{Cell, Footprint, Inventory, PlacementRejection};
use crate::components::item_instance::ItemInstance;
use crate::data::game_config::EquipmentSlot;
use crate::data::item_definitions::ItemKind;
use crate::entities::world_item::WorldItem;
use crate::events::inventory::{
    EquipOutcome, EquipRejection, MoveOutcome, PlaceOutcome, RemoveOutcome, UnequipOutcome,
};

/// The parsed request to place an item: where, how large, and the move-only
/// item itself. Built by the host from the definition's footprint (its Atlas
/// lookup), so the service never reaches into `data` for geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceIntent {
    /// The anchor cell to place at.
    pub anchor: Cell,
    /// The item's cell footprint.
    pub footprint: Footprint,
    /// The item to place.
    pub item: ItemInstance,
}

/// Places an item into the inventory, folding the component result into a
/// [`PlaceOutcome`]. On rejection the inventory is unchanged and the item rides
/// the outcome back.
#[must_use]
pub fn place_item(inventory: Inventory, intent: PlaceIntent) -> (Inventory, PlaceOutcome) {
    let PlaceIntent {
        anchor,
        footprint,
        item,
    } = intent;
    match inventory.place(anchor, footprint, item) {
        Ok(inventory) => (inventory, PlaceOutcome::Placed { at: anchor }),
        Err((inventory, item, reason)) => (inventory, PlaceOutcome::Rejected { reason, item }),
    }
}

/// Removes the item covering `cell`, folding the component result into a
/// [`RemoveOutcome`]. On success the item rides the outcome out.
#[must_use]
pub fn remove_item(inventory: Inventory, cell: Cell) -> (Inventory, RemoveOutcome) {
    match inventory.remove(cell) {
        Ok((inventory, item)) => (inventory, RemoveOutcome::Removed { at: cell, item }),
        Err((inventory, reason)) => (inventory, RemoveOutcome::Rejected { reason }),
    }
}

/// Moves the item covering `from` so its anchor is `to`, folding the component
/// result into a [`MoveOutcome`]. No item crosses the boundary.
#[must_use]
pub fn move_item(inventory: Inventory, from: Cell, to: Cell) -> (Inventory, MoveOutcome) {
    match inventory.move_to(from, to) {
        Ok(inventory) => (inventory, MoveOutcome::Moved { from, to }),
        Err((inventory, reason)) => (inventory, MoveOutcome::Rejected { reason }),
    }
}

/// What a ground-item pickup produced, kind-tagged: the item entered the
/// inventory, or the placement was rejected and the untouched world item handed
/// back. Lives in the service (not `events`) because it carries a whole
/// [`WorldItem`] entity, and an event never imports an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PickupOutcome {
    /// The ground item was picked up and stored at `at`.
    PickedUp {
        /// The anchor cell it was stored at.
        at: Cell,
    },
    /// The pickup was rejected; the untouched ground item is handed back.
    Rejected {
        /// Why the pickup was rejected.
        reason: PlacementRejection,
        /// The ground item, reassembled exactly as it was.
        item: WorldItem,
    },
}

/// Picks a ground item up into the inventory at `anchor`. On success the world
/// item is consumed; on rejection the inventory is unchanged and the untouched
/// world item is reassembled from the handed-back instance plus its original
/// position, map, and despawn tick. `footprint` comes from the caller's Atlas
/// lookup.
#[must_use]
pub fn pickup(
    world_item: WorldItem,
    inventory: Inventory,
    anchor: Cell,
    footprint: Footprint,
) -> (Inventory, PickupOutcome) {
    let WorldItem {
        instance,
        position,
        map,
        despawn,
    } = world_item;
    match inventory.place(anchor, footprint, instance) {
        Ok(inventory) => (inventory, PickupOutcome::PickedUp { at: anchor }),
        Err((inventory, instance, reason)) => (
            inventory,
            PickupOutcome::Rejected {
                reason,
                item: WorldItem {
                    instance,
                    position,
                    map,
                    despawn,
                },
            },
        ),
    }
}

/// Equips an item into a slot. Capability outranks transient state: an
/// incompatible slot is rejected before an occupied one. On rejection the item
/// rides the outcome back.
#[must_use]
pub fn equip(
    equipment: Equipment,
    item: ItemInstance,
    def_kind: &ItemKind,
    slot: EquipmentSlot,
) -> (Equipment, EquipOutcome) {
    let slot = translate_slot(slot);
    if !slot_accepts(def_kind, slot) {
        return (
            equipment,
            EquipOutcome::Rejected {
                reason: EquipRejection::IncompatibleSlot,
                item,
            },
        );
    }
    if equipment.get(slot).is_some() {
        return (
            equipment,
            EquipOutcome::Rejected {
                reason: EquipRejection::SlotOccupied,
                item,
            },
        );
    }
    (equipment.with(slot, item), EquipOutcome::Equipped { slot })
}

/// Unequips a slot, folding the component result into an [`UnequipOutcome`]. On
/// success the removed item rides the outcome out.
#[must_use]
pub fn unequip(equipment: Equipment, slot: EquipmentSlot) -> (Equipment, UnequipOutcome) {
    let slot = translate_slot(slot);
    let (equipment, taken) = equipment.without(slot);
    match taken {
        Some(item) => (equipment, UnequipOutcome::Unequipped { slot, item }),
        None => (equipment, UnequipOutcome::SlotEmpty),
    }
}

/// The one total translation from the `data` slot vocabulary to the component
/// [`EquipSlot`].
fn translate_slot(slot: EquipmentSlot) -> EquipSlot {
    match slot {
        EquipmentSlot::LeftHand => EquipSlot::LeftHand,
        EquipmentSlot::RightHand => EquipSlot::RightHand,
        EquipmentSlot::Helm => EquipSlot::Helm,
        EquipmentSlot::Armor => EquipSlot::Armor,
        EquipmentSlot::Pants => EquipSlot::Pants,
        EquipmentSlot::Gloves => EquipSlot::Gloves,
        EquipmentSlot::Boots => EquipSlot::Boots,
        EquipmentSlot::Wings => EquipSlot::Wings,
        EquipmentSlot::Pet => EquipSlot::Pet,
        EquipmentSlot::Pendant => EquipSlot::Pendant,
        EquipmentSlot::Ring1 => EquipSlot::Ring1,
        EquipmentSlot::Ring2 => EquipSlot::Ring2,
    }
}

/// Whether an item of `kind` may be worn in `slot` — the exhaustive
/// `(ItemKind x EquipSlot)` compatibility rule. Non-equippable kinds accept no
/// slot. Two-handed dual-hand occupancy is deferred: a two-handed weapon in one
/// hand does not yet block the other.
fn slot_accepts(kind: &ItemKind, slot: EquipSlot) -> bool {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. } => matches!(slot, EquipSlot::LeftHand | EquipSlot::RightHand),
        ItemKind::Helm { .. } => matches!(slot, EquipSlot::Helm),
        ItemKind::BodyArmor { .. } => matches!(slot, EquipSlot::Armor),
        ItemKind::Pants { .. } => matches!(slot, EquipSlot::Pants),
        ItemKind::Gloves { .. } => matches!(slot, EquipSlot::Gloves),
        ItemKind::Boots { .. } => matches!(slot, EquipSlot::Boots),
        ItemKind::Wings { .. } => matches!(slot, EquipSlot::Wings),
        ItemKind::Pet { .. } => matches!(slot, EquipSlot::Pet),
        ItemKind::Pendant { .. } => matches!(slot, EquipSlot::Pendant),
        ItemKind::Ring { .. } | ItemKind::TransformationRing { .. } => {
            matches!(slot, EquipSlot::Ring1 | EquipSlot::Ring2)
        }
        ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::class::ClassSet;
    use crate::components::item_instance::{Durability, LuckRoll, RarityRoll, SkillRoll};
    use crate::components::item_ref::ItemRef;
    use crate::components::spatial::WorldPos;
    use crate::components::units::{ItemLevel, MapNumber, Tick};
    use crate::data::common::SkillNumber;
    use crate::data::item_definitions::{ItemKind, WeaponHandling, WearRequirements};

    fn item(number: u16) -> ItemInstance {
        ItemInstance {
            item: ItemRef { group: 0, number },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
        }
    }

    fn footprint(width: u8, height: u8) -> Footprint {
        Footprint::new(width, height).unwrap()
    }

    fn cell(row: u8, col: u8) -> Cell {
        Cell { row, col }
    }

    fn weapon_kind() -> ItemKind {
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            min_damage: 1,
            max_damage: 2,
            attack_speed: 10,
            skill: Some(SkillNumber(19)),
            classes: ClassSet::NONE,
            wear: WearRequirements {
                level: 0,
                strength: 0,
                agility: 0,
                vitality: 0,
                energy: 0,
                command: 0,
            },
        }
    }

    fn helm_kind() -> ItemKind {
        ItemKind::Helm {
            defense: 5,
            classes: ClassSet::NONE,
            wear: WearRequirements {
                level: 0,
                strength: 0,
                agility: 0,
                vitality: 0,
                energy: 0,
                command: 0,
            },
        }
    }

    #[test]
    fn place_then_reject_hands_the_item_back() {
        let inventory = Inventory::empty(8, 8);
        let (inventory, outcome) = place_item(
            inventory,
            PlaceIntent {
                anchor: cell(0, 0),
                footprint: footprint(2, 2),
                item: item(1),
            },
        );
        assert_eq!(outcome, PlaceOutcome::Placed { at: cell(0, 0) });
        let (inventory, outcome) = place_item(
            inventory,
            PlaceIntent {
                anchor: cell(1, 1),
                footprint: footprint(2, 2),
                item: item(2),
            },
        );
        match outcome {
            PlaceOutcome::Rejected { reason, item } => {
                assert_eq!(reason, PlacementRejection::CellsOccupied);
                assert_eq!(item.item.number, 2);
            }
            PlaceOutcome::Placed { .. } => panic!("overlap should reject"),
        }
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn remove_and_move_fold_the_component_result() {
        let (inventory, _) = place_item(
            Inventory::empty(8, 8),
            PlaceIntent {
                anchor: cell(0, 0),
                footprint: footprint(1, 1),
                item: item(5),
            },
        );
        let (inventory, outcome) = move_item(inventory, cell(0, 0), cell(3, 3));
        assert_eq!(
            outcome,
            MoveOutcome::Moved {
                from: cell(0, 0),
                to: cell(3, 3)
            }
        );
        let (inventory, outcome) = remove_item(inventory, cell(3, 3));
        match outcome {
            RemoveOutcome::Removed { at, item } => {
                assert_eq!(at, cell(3, 3));
                assert_eq!(item.item.number, 5);
            }
            RemoveOutcome::Rejected { .. } => panic!("item is present"),
        }
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn pickup_reject_reassembles_the_untouched_world_item() {
        let occupied = place_item(
            Inventory::empty(4, 4),
            PlaceIntent {
                anchor: cell(0, 0),
                footprint: footprint(4, 4),
                item: item(1),
            },
        )
        .0;
        let world_item = WorldItem {
            instance: item(9),
            position: WorldPos::clamped(100, 200),
            map: MapNumber(3),
            despawn: Tick(1200),
        };
        let (inventory, outcome) = pickup(world_item, occupied, cell(0, 0), footprint(2, 2));
        match outcome {
            PickupOutcome::Rejected { reason, item } => {
                assert_eq!(reason, PlacementRejection::CellsOccupied);
                assert_eq!(item.instance.item.number, 9);
                assert_eq!(item.position, WorldPos::clamped(100, 200));
                assert_eq!(item.map, MapNumber(3));
                assert_eq!(item.despawn, Tick(1200));
            }
            PickupOutcome::PickedUp { .. } => panic!("full grid should reject"),
        }
        // The inventory is unchanged (still just the 4x4 filler).
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn pickup_success_consumes_the_world_item() {
        let world_item = WorldItem {
            instance: item(9),
            position: WorldPos::clamped(100, 200),
            map: MapNumber(3),
            despawn: Tick(1200),
        };
        let (inventory, outcome) = pickup(
            world_item,
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
        );
        assert_eq!(outcome, PickupOutcome::PickedUp { at: cell(0, 0) });
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn equip_incompatible_slot_is_rejected_before_occupancy() {
        let equipment = Equipment::empty();
        let (equipment, outcome) = equip(equipment, item(1), &weapon_kind(), EquipmentSlot::Helm);
        match outcome {
            EquipOutcome::Rejected { reason, item } => {
                assert_eq!(reason, EquipRejection::IncompatibleSlot);
                assert_eq!(item.item.number, 1);
            }
            EquipOutcome::Equipped { .. } => panic!("a weapon does not go in the helm slot"),
        }
        assert!(equipment.get(EquipSlot::Helm).is_none());
    }

    #[test]
    fn equip_into_a_valid_slot_then_occupied_is_rejected() {
        let (equipment, outcome) = equip(
            Equipment::empty(),
            item(1),
            &helm_kind(),
            EquipmentSlot::Helm,
        );
        assert_eq!(
            outcome,
            EquipOutcome::Equipped {
                slot: EquipSlot::Helm
            }
        );
        let (equipment, outcome) = equip(equipment, item(2), &helm_kind(), EquipmentSlot::Helm);
        match outcome {
            EquipOutcome::Rejected { reason, item } => {
                assert_eq!(reason, EquipRejection::SlotOccupied);
                assert_eq!(item.item.number, 2);
            }
            EquipOutcome::Equipped { .. } => panic!("slot already occupied"),
        }
        assert_eq!(
            equipment.get(EquipSlot::Helm).map(|item| item.item.number),
            Some(1)
        );
    }

    #[test]
    fn weapon_accepts_either_hand() {
        for slot in [EquipmentSlot::LeftHand, EquipmentSlot::RightHand] {
            let (_, outcome) = equip(Equipment::empty(), item(1), &weapon_kind(), slot);
            match outcome {
                EquipOutcome::Equipped { .. } => {}
                EquipOutcome::Rejected { .. } => panic!("a weapon fits either hand"),
            }
        }
    }

    #[test]
    fn unequip_empty_slot_reports_slot_empty() {
        let (equipment, outcome) = unequip(Equipment::empty(), EquipmentSlot::Boots);
        assert_eq!(outcome, UnequipOutcome::SlotEmpty);
        assert!(equipment.get(EquipSlot::Boots).is_none());
    }

    #[test]
    fn unequip_hands_the_item_out() {
        let (equipment, _) = equip(
            Equipment::empty(),
            item(7),
            &helm_kind(),
            EquipmentSlot::Helm,
        );
        let (equipment, outcome) = unequip(equipment, EquipmentSlot::Helm);
        match outcome {
            UnequipOutcome::Unequipped { slot, item } => {
                assert_eq!(slot, EquipSlot::Helm);
                assert_eq!(item.item.number, 7);
            }
            UnequipOutcome::SlotEmpty => panic!("the slot held an item"),
        }
        assert!(equipment.get(EquipSlot::Helm).is_none());
    }
}
