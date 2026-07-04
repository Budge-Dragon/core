//! The twelve classic equipment slots as a total slot-keyed structure. Each
//! slot is an independent `Option<ItemInstance>` (occupied xor empty), so there
//! is no cross-slot invariant to guard this wave and the type needs no
//! validating deserialize. `get`/`with`/`without` dispatch through one
//! exhaustive twelve-arm match — the total-structure pattern — hiding all
//! twelve fields behind a small interface. Kind/slot compatibility needs a
//! definition and is decided by the equip service, never here.

use serde::{Deserialize, Serialize};

use crate::components::item_instance::ItemInstance;

/// The twelve classic equipment slots. Structurally the twin of
/// [`crate::data::game_config::EquipmentSlot`]; the duplication is the
/// dependency rule's price (a component may not import `data`), and the one
/// `data`-to-component translation lives in the equip service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipSlot {
    /// Left weapon hand.
    LeftHand,
    /// Right weapon hand.
    RightHand,
    /// Helm.
    Helm,
    /// Body armor.
    Armor,
    /// Pants.
    Pants,
    /// Gloves.
    Gloves,
    /// Boots.
    Boots,
    /// Wings.
    Wings,
    /// Pet.
    Pet,
    /// Pendant.
    Pendant,
    /// First ring.
    Ring1,
    /// Second ring.
    Ring2,
}

/// A character's worn items: one independent slot per [`EquipSlot`]. Each field
/// is genuinely optional (the slot may be empty); a sparse wire omits empty
/// slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Equipment {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    left_hand: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    right_hand: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    helm: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    armor: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pants: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gloves: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    boots: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    wings: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pet: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pendant: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ring1: Option<ItemInstance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ring2: Option<ItemInstance>,
}

impl Equipment {
    /// A fully unequipped set — every slot empty. A real domain value (a fresh
    /// character wears nothing), not a fabricated default.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            left_hand: None,
            right_hand: None,
            helm: None,
            armor: None,
            pants: None,
            gloves: None,
            boots: None,
            wings: None,
            pet: None,
            pendant: None,
            ring1: None,
            ring2: None,
        }
    }

    /// The item worn in `slot`, if any — genuine optionality of an empty slot.
    #[must_use]
    pub fn get(&self, slot: EquipSlot) -> Option<&ItemInstance> {
        match slot {
            EquipSlot::LeftHand => self.left_hand.as_ref(),
            EquipSlot::RightHand => self.right_hand.as_ref(),
            EquipSlot::Helm => self.helm.as_ref(),
            EquipSlot::Armor => self.armor.as_ref(),
            EquipSlot::Pants => self.pants.as_ref(),
            EquipSlot::Gloves => self.gloves.as_ref(),
            EquipSlot::Boots => self.boots.as_ref(),
            EquipSlot::Wings => self.wings.as_ref(),
            EquipSlot::Pet => self.pet.as_ref(),
            EquipSlot::Pendant => self.pendant.as_ref(),
            EquipSlot::Ring1 => self.ring1.as_ref(),
            EquipSlot::Ring2 => self.ring2.as_ref(),
        }
    }

    /// This equipment with `item` set into `slot`, replacing any occupant.
    /// Value-in/value-out — the caller's set is not mutated.
    #[must_use]
    pub fn with(mut self, slot: EquipSlot, item: ItemInstance) -> Self {
        match slot {
            EquipSlot::LeftHand => self.left_hand = Some(item),
            EquipSlot::RightHand => self.right_hand = Some(item),
            EquipSlot::Helm => self.helm = Some(item),
            EquipSlot::Armor => self.armor = Some(item),
            EquipSlot::Pants => self.pants = Some(item),
            EquipSlot::Gloves => self.gloves = Some(item),
            EquipSlot::Boots => self.boots = Some(item),
            EquipSlot::Wings => self.wings = Some(item),
            EquipSlot::Pet => self.pet = Some(item),
            EquipSlot::Pendant => self.pendant = Some(item),
            EquipSlot::Ring1 => self.ring1 = Some(item),
            EquipSlot::Ring2 => self.ring2 = Some(item),
        }
        self
    }

    /// This equipment with `slot` emptied, handing out any item that was there.
    #[must_use]
    pub fn without(mut self, slot: EquipSlot) -> (Self, Option<ItemInstance>) {
        let taken = match slot {
            EquipSlot::LeftHand => self.left_hand.take(),
            EquipSlot::RightHand => self.right_hand.take(),
            EquipSlot::Helm => self.helm.take(),
            EquipSlot::Armor => self.armor.take(),
            EquipSlot::Pants => self.pants.take(),
            EquipSlot::Gloves => self.gloves.take(),
            EquipSlot::Boots => self.boots.take(),
            EquipSlot::Wings => self.wings.take(),
            EquipSlot::Pet => self.pet.take(),
            EquipSlot::Pendant => self.pendant.take(),
            EquipSlot::Ring1 => self.ring1.take(),
            EquipSlot::Ring2 => self.ring2.take(),
        };
        (self, taken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{Durability, LuckRoll, RarityRoll, SkillRoll};
    use crate::components::item_ref::ItemRef;
    use crate::components::units::ItemLevel;

    const ALL_SLOTS: [EquipSlot; 12] = [
        EquipSlot::LeftHand,
        EquipSlot::RightHand,
        EquipSlot::Helm,
        EquipSlot::Armor,
        EquipSlot::Pants,
        EquipSlot::Gloves,
        EquipSlot::Boots,
        EquipSlot::Wings,
        EquipSlot::Pet,
        EquipSlot::Pendant,
        EquipSlot::Ring1,
        EquipSlot::Ring2,
    ];

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

    #[test]
    fn empty_has_every_slot_free() {
        let equipment = Equipment::empty();
        for slot in ALL_SLOTS {
            assert!(equipment.get(slot).is_none());
        }
    }

    #[test]
    fn with_sets_exactly_one_slot() {
        let equipment = Equipment::empty().with(EquipSlot::Helm, item(7));
        assert_eq!(
            equipment.get(EquipSlot::Helm).map(|item| item.item.number),
            Some(7)
        );
        for slot in ALL_SLOTS {
            if slot != EquipSlot::Helm {
                assert!(equipment.get(slot).is_none());
            }
        }
    }

    #[test]
    fn without_takes_the_occupant_and_empties_the_slot() {
        let equipment = Equipment::empty().with(EquipSlot::Ring1, item(9));
        let (equipment, taken) = equipment.without(EquipSlot::Ring1);
        assert_eq!(taken.map(|item| item.item.number), Some(9));
        assert!(equipment.get(EquipSlot::Ring1).is_none());
    }

    #[test]
    fn without_an_empty_slot_yields_none() {
        let (equipment, taken) = Equipment::empty().without(EquipSlot::Boots);
        assert!(taken.is_none());
        assert!(equipment.get(EquipSlot::Boots).is_none());
    }

    #[test]
    fn sparse_wire_omits_empty_slots() {
        let equipment = Equipment::empty().with(EquipSlot::Wings, item(2));
        let json = serde_json::to_string(&equipment).unwrap();
        assert!(json.starts_with(r#"{"wings":"#));
        assert!(!json.contains("left_hand"));
        assert_eq!(serde_json::from_str::<Equipment>(&json).unwrap(), equipment);
    }

    #[test]
    fn slot_wire_names_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&EquipSlot::LeftHand).unwrap(),
            r#""left_hand""#
        );
        assert_eq!(
            serde_json::to_string(&EquipSlot::Ring2).unwrap(),
            r#""ring2""#
        );
    }
}
