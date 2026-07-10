//! The twelve classic equipment slots as a total slot-keyed structure. Each
//! slot is an independent `Option<ItemInstance>` (occupied xor empty), so there
//! is no cross-slot invariant to guard this wave and the type needs no
//! validating deserialize. `get`/`with`/`without` dispatch through one
//! exhaustive twelve-arm match — the total-structure pattern — hiding all
//! twelve fields behind a small interface. Kind/slot compatibility needs a
//! definition and is decided by the equip service, never here.

use serde::{Deserialize, Serialize};

use crate::components::item_instance::ItemInstance;

/// The twelve classic equipment slots — the canonical slot vocabulary. The
/// `data` layer names it as [`crate::data::game_config::EquipmentSlot`], a
/// re-export of this type, so a `data`-scoped record can speak of a slot without
/// a component importing `data` and without a second twin enum. Authentic client
/// wire indices are documented per variant; the enum itself carries no ordinal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentSlot {
    /// Left weapon hand. Wire index 0.
    LeftHand,
    /// Right weapon hand. Wire index 1.
    RightHand,
    /// Helm. Wire index 2.
    Helm,
    /// Body armor. Wire index 3.
    Armor,
    /// Pants. Wire index 4.
    Pants,
    /// Gloves. Wire index 5.
    Gloves,
    /// Boots. Wire index 6.
    Boots,
    /// Wings. Wire index 7.
    Wings,
    /// Pet. Wire index 8.
    Pet,
    /// Pendant. Wire index 9.
    Pendant,
    /// First ring. Wire index 10.
    Ring1,
    /// Second ring. Wire index 11.
    Ring2,
}

impl EquipmentSlot {
    /// Every slot, in declaration order — the one canonical enumeration every
    /// full-set walk and random-pool index shares, so a positional pick maps
    /// to the same slot bit-for-bit on every host.
    pub(crate) const ALL: [Self; 12] = [
        Self::LeftHand,
        Self::RightHand,
        Self::Helm,
        Self::Armor,
        Self::Pants,
        Self::Gloves,
        Self::Boots,
        Self::Wings,
        Self::Pet,
        Self::Pendant,
        Self::Ring1,
        Self::Ring2,
    ];
}

/// A character's worn items: one independent slot per [`EquipmentSlot`]. Each
/// field is genuinely optional (the slot may be empty); a sparse wire omits
/// empty slots.
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
    pub fn get(&self, slot: EquipmentSlot) -> Option<&ItemInstance> {
        match slot {
            EquipmentSlot::LeftHand => self.left_hand.as_ref(),
            EquipmentSlot::RightHand => self.right_hand.as_ref(),
            EquipmentSlot::Helm => self.helm.as_ref(),
            EquipmentSlot::Armor => self.armor.as_ref(),
            EquipmentSlot::Pants => self.pants.as_ref(),
            EquipmentSlot::Gloves => self.gloves.as_ref(),
            EquipmentSlot::Boots => self.boots.as_ref(),
            EquipmentSlot::Wings => self.wings.as_ref(),
            EquipmentSlot::Pet => self.pet.as_ref(),
            EquipmentSlot::Pendant => self.pendant.as_ref(),
            EquipmentSlot::Ring1 => self.ring1.as_ref(),
            EquipmentSlot::Ring2 => self.ring2.as_ref(),
        }
    }

    /// This equipment with `item` set into `slot`, replacing any occupant.
    /// Value-in/value-out — the caller's set is not mutated.
    #[must_use]
    pub fn with(mut self, slot: EquipmentSlot, item: ItemInstance) -> Self {
        match slot {
            EquipmentSlot::LeftHand => self.left_hand = Some(item),
            EquipmentSlot::RightHand => self.right_hand = Some(item),
            EquipmentSlot::Helm => self.helm = Some(item),
            EquipmentSlot::Armor => self.armor = Some(item),
            EquipmentSlot::Pants => self.pants = Some(item),
            EquipmentSlot::Gloves => self.gloves = Some(item),
            EquipmentSlot::Boots => self.boots = Some(item),
            EquipmentSlot::Wings => self.wings = Some(item),
            EquipmentSlot::Pet => self.pet = Some(item),
            EquipmentSlot::Pendant => self.pendant = Some(item),
            EquipmentSlot::Ring1 => self.ring1 = Some(item),
            EquipmentSlot::Ring2 => self.ring2 = Some(item),
        }
        self
    }

    /// This equipment with `slot` emptied, handing out any item that was there.
    #[must_use]
    pub fn without(mut self, slot: EquipmentSlot) -> (Self, Option<ItemInstance>) {
        let taken = match slot {
            EquipmentSlot::LeftHand => self.left_hand.take(),
            EquipmentSlot::RightHand => self.right_hand.take(),
            EquipmentSlot::Helm => self.helm.take(),
            EquipmentSlot::Armor => self.armor.take(),
            EquipmentSlot::Pants => self.pants.take(),
            EquipmentSlot::Gloves => self.gloves.take(),
            EquipmentSlot::Boots => self.boots.take(),
            EquipmentSlot::Wings => self.wings.take(),
            EquipmentSlot::Pet => self.pet.take(),
            EquipmentSlot::Pendant => self.pendant.take(),
            EquipmentSlot::Ring1 => self.ring1.take(),
            EquipmentSlot::Ring2 => self.ring2.take(),
        };
        (self, taken)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::units::ItemLevel;

    fn item(number: u16) -> ItemInstance {
        ItemInstance {
            item: ItemRef { group: 0, number },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
            augment: CraftedAugment::None,
        }
    }

    #[test]
    fn empty_has_every_slot_free() {
        let equipment = Equipment::empty();
        for slot in EquipmentSlot::ALL {
            assert!(equipment.get(slot).is_none());
        }
    }

    #[test]
    fn with_sets_exactly_one_slot() {
        let equipment = Equipment::empty().with(EquipmentSlot::Helm, item(7));
        assert_eq!(
            equipment
                .get(EquipmentSlot::Helm)
                .map(|item| item.item.number),
            Some(7)
        );
        for slot in EquipmentSlot::ALL {
            if slot != EquipmentSlot::Helm {
                assert!(equipment.get(slot).is_none());
            }
        }
    }

    #[test]
    fn without_takes_the_occupant_and_empties_the_slot() {
        let equipment = Equipment::empty().with(EquipmentSlot::Ring1, item(9));
        let (equipment, taken) = equipment.without(EquipmentSlot::Ring1);
        assert_eq!(taken.map(|item| item.item.number), Some(9));
        assert!(equipment.get(EquipmentSlot::Ring1).is_none());
    }

    #[test]
    fn without_an_empty_slot_yields_none() {
        let (equipment, taken) = Equipment::empty().without(EquipmentSlot::Boots);
        assert!(taken.is_none());
        assert!(equipment.get(EquipmentSlot::Boots).is_none());
    }

    #[test]
    fn sparse_wire_omits_empty_slots() {
        let equipment = Equipment::empty().with(EquipmentSlot::Wings, item(2));
        let json = serde_json::to_string(&equipment).unwrap();
        assert!(json.starts_with(r#"{"wings":"#));
        assert!(!json.contains("left_hand"));
        assert_eq!(serde_json::from_str::<Equipment>(&json).unwrap(), equipment);
    }

    #[test]
    fn slot_wire_names_are_snake_case() {
        assert_eq!(
            serde_json::to_string(&EquipmentSlot::LeftHand).unwrap(),
            r#""left_hand""#
        );
        assert_eq!(
            serde_json::to_string(&EquipmentSlot::Ring2).unwrap(),
            r#""ring2""#
        );
    }
}
