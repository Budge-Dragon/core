//! Container behavior: the thin `(state, intent) -> (state, outcome)` adapters
//! over the [`Inventory`] and [`Equipment`] components. Each folds the
//! component's `Result` into a per-operation outcome enum with no `unwrap` —
//! both arms bound — and never re-implements a geometry rule. The equip service
//! is the one place item-kind/slot compatibility and two-handed dual-hand
//! occupancy are decided (the component accepts any instance in any slot).
//! Container operations draw zero RNG.

use serde::{Deserialize, Serialize};

use crate::components::equipment::{Equipment, EquipmentSlot};
use crate::components::inventory::{Cell, Footprint, Inventory, PlacementRejection};
use crate::components::item_instance::ItemInstance;
use crate::data::atlas::Atlas;
use crate::data::item_definitions::{ItemKind, WeaponHandling};
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
/// incompatible slot is rejected before an occupied one, and two-handed
/// dual-hand occupancy is checked last against the paired hand (resolved through
/// the atlas). On rejection the item rides the outcome back.
#[must_use]
pub fn equip(
    equipment: Equipment,
    item: ItemInstance,
    def_kind: &ItemKind,
    slot: EquipmentSlot,
    atlas: &Atlas,
) -> (Equipment, EquipOutcome) {
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
    if two_handed_conflict(&equipment, hand_occupation(def_kind), slot, atlas) {
        return (
            equipment,
            EquipOutcome::Rejected {
                reason: EquipRejection::TwoHandedConflict,
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
    let (equipment, taken) = equipment.without(slot);
    match taken {
        Some(item) => (equipment, UnequipOutcome::Unequipped { slot, item }),
        None => (equipment, UnequipOutcome::SlotEmpty),
    }
}

/// Whether an item of `kind` may be worn in `slot` — the exhaustive
/// `(ItemKind x EquipmentSlot)` compatibility rule. Non-equippable kinds accept
/// no slot. Two-handed dual-hand occupancy is a separate rule, enforced by the
/// equip service's two-handed check and by [`reconcile_equipment`] at reload.
fn slot_accepts(kind: &ItemKind, slot: EquipmentSlot) -> bool {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. } => {
            matches!(slot, EquipmentSlot::LeftHand | EquipmentSlot::RightHand)
        }
        ItemKind::Helm { .. } => matches!(slot, EquipmentSlot::Helm),
        ItemKind::BodyArmor { .. } => matches!(slot, EquipmentSlot::Armor),
        ItemKind::Pants { .. } => matches!(slot, EquipmentSlot::Pants),
        ItemKind::Gloves { .. } => matches!(slot, EquipmentSlot::Gloves),
        ItemKind::Boots { .. } => matches!(slot, EquipmentSlot::Boots),
        ItemKind::Wings { .. } => matches!(slot, EquipmentSlot::Wings),
        ItemKind::Pet { .. } => matches!(slot, EquipmentSlot::Pet),
        ItemKind::Pendant { .. } => matches!(slot, EquipmentSlot::Pendant),
        ItemKind::Ring { .. } | ItemKind::TransformationRing { .. } => {
            matches!(slot, EquipmentSlot::Ring1 | EquipmentSlot::Ring2)
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

/// How many hands a hand item claims when worn: a two-handed weapon claims both,
/// everything else claims one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandOccupation {
    /// Claims a single hand; the paired hand stays free for another one-hander.
    OneHand,
    /// Claims both hands; the paired hand must stay empty.
    TwoHands,
}

/// How many hands an item claims in a hand slot. Total over [`ItemKind`];
/// non-hand kinds never reach a hand slot ([`slot_accepts`] gates them), so
/// [`HandOccupation::OneHand`] is a harmless total answer for them.
// W-SRC: two-handedness is structural, not a stored flag. Melee weapons carry an
// explicit `WeaponHandling`; bows and crossbows have no handling field because
// they are two-handed by construction (they leave no hand for a shield) — an
// authentic game fact, not an invented column. Staves are treated one-handed for
// want of a two-handed flag; a handling distinction for staves (and any
// two-handed staff) is a possible future data-model refinement, deliberately not
// fabricated here.
fn hand_occupation(kind: &ItemKind) -> HandOccupation {
    match kind {
        ItemKind::Weapon {
            handling: WeaponHandling::TwoHanded,
            ..
        }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. } => HandOccupation::TwoHands,
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            ..
        }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => HandOccupation::OneHand,
    }
}

/// The paired hand of a hand slot; `None` for a non-hand slot. Only the two hand
/// slots pair — every other slot is independent.
fn paired_hand(slot: EquipmentSlot) -> Option<EquipmentSlot> {
    match slot {
        EquipmentSlot::LeftHand => Some(EquipmentSlot::RightHand),
        EquipmentSlot::RightHand => Some(EquipmentSlot::LeftHand),
        EquipmentSlot::Helm
        | EquipmentSlot::Armor
        | EquipmentSlot::Pants
        | EquipmentSlot::Gloves
        | EquipmentSlot::Boots
        | EquipmentSlot::Wings
        | EquipmentSlot::Pet
        | EquipmentSlot::Pendant
        | EquipmentSlot::Ring1
        | EquipmentSlot::Ring2 => None,
    }
}

/// Whether a worn hand occupant is a two-handed weapon, resolved through the
/// atlas. An occupant the atlas cannot identify is not a known two-handed weapon
/// — a total fold of genuine optionality, never a should-never-happen panic.
fn is_two_handed(occupant: &ItemInstance, atlas: &Atlas) -> bool {
    match atlas.item(occupant.item) {
        Some(def) => hand_occupation(&def.kind) == HandOccupation::TwoHands,
        None => false,
    }
}

/// Whether wearing an `incoming`-occupancy item in `slot` would break two-handed
/// dual-hand occupancy: a two-handed item needs its paired hand empty, and no
/// item may share a hand pair with a worn two-handed weapon. A non-hand slot has
/// no paired hand, so it never conflicts.
fn two_handed_conflict(
    equipment: &Equipment,
    incoming: HandOccupation,
    slot: EquipmentSlot,
    atlas: &Atlas,
) -> bool {
    let Some(paired) = paired_hand(slot) else {
        return false;
    };
    let Some(occupant) = equipment.get(paired) else {
        return false;
    };
    match incoming {
        HandOccupation::TwoHands => true,
        HandOccupation::OneHand => is_two_handed(occupant, atlas),
    }
}

/// Re-proves two-handed dual-hand occupancy at the reload boundary — the
/// instance×definition cross-reference the [`Equipment`] component cannot hold
/// alone (a slot carries an [`ItemInstance`], whose handedness lives in the
/// definition). A hand wearing a two-handed weapon requires the other hand empty.
///
/// # Errors
/// Returns [`EquipmentConflict::TwoHandedWithOffhand`] when a hand wears a
/// two-handed weapon while the other hand is also occupied.
pub fn reconcile_equipment(equipment: &Equipment, atlas: &Atlas) -> Result<(), EquipmentConflict> {
    match (
        equipment.get(EquipmentSlot::LeftHand),
        equipment.get(EquipmentSlot::RightHand),
    ) {
        (Some(left), Some(right)) => {
            if is_two_handed(left, atlas) || is_two_handed(right, atlas) {
                Err(EquipmentConflict::TwoHandedWithOffhand)
            } else {
                Ok(())
            }
        }
        (Some(_) | None, None) | (None, Some(_)) => Ok(()),
    }
}

/// Why a reloaded equipment set violates two-handed dual-hand occupancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentConflict {
    /// A hand wears a two-handed weapon while the other hand is also occupied.
    TwoHandedWithOffhand,
}

impl core::fmt::Display for EquipmentConflict {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TwoHandedWithOffhand => write!(
                f,
                "a two-handed weapon is worn while the other hand is occupied"
            ),
        }
    }
}

impl core::error::Error for EquipmentConflict {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{Durability, LuckRoll, RarityRoll, SkillRoll};
    use crate::components::item_ref::ItemRef;
    use crate::components::spatial::WorldPos;
    use crate::components::units::{ItemLevel, MapNumber, Tick};

    // The equip service now resolves a paired-hand occupant through the Atlas, so
    // its two-handed rule and `reconcile_equipment` are exercised against the real
    // dataset in `core/tests/item_roll_integration.rs`. These inline tests cover
    // the Atlas-free container geometry and the unequip fold.

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
    fn unequip_empty_slot_reports_slot_empty() {
        let (equipment, outcome) = unequip(Equipment::empty(), EquipmentSlot::Boots);
        assert_eq!(outcome, UnequipOutcome::SlotEmpty);
        assert!(equipment.get(EquipmentSlot::Boots).is_none());
    }

    #[test]
    fn unequip_hands_the_item_out() {
        let equipment = Equipment::empty().with(EquipmentSlot::Helm, item(7));
        let (equipment, outcome) = unequip(equipment, EquipmentSlot::Helm);
        match outcome {
            UnequipOutcome::Unequipped { slot, item } => {
                assert_eq!(slot, EquipmentSlot::Helm);
                assert_eq!(item.item.number, 7);
            }
            UnequipOutcome::SlotEmpty => panic!("the slot held an item"),
        }
        assert!(equipment.get(EquipmentSlot::Helm).is_none());
    }
}
