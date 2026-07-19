//! Projection of a character's public [`CharacterAppearance`] from its class
//! and worn gear — the read-model another player reads to draw this one. Pure,
//! Atlas-free, infallible, and draws no RNG: every fact is instance-local (the
//! class on the character, and each visible slot's identity, plus-level, and
//! rarity on its worn [`ItemInstance`]). The three jewelry slots are never
//! read, so they are absent from the projection by construction. Read-only
//! derivation: both inputs stay borrowed and the character is never returned.

use crate::components::appearance::{CharacterAppearance, VisibleItem};
use crate::components::equipment::{Equipment, EquipmentSlot};
use crate::entities::character::Character;

/// Projects a character's public appearance from its class and worn equipment.
/// Reads the class for the body and each of the nine visible slots' worn item;
/// pendant and both rings are never consulted, so jewelry never enters the
/// package. An empty slot projects to `None`.
#[must_use]
pub fn project_appearance(character: &Character, worn: &Equipment) -> CharacterAppearance {
    CharacterAppearance {
        class: character.class(),
        left_hand: worn.get(EquipmentSlot::LeftHand).map(VisibleItem::of),
        right_hand: worn.get(EquipmentSlot::RightHand).map(VisibleItem::of),
        helm: worn.get(EquipmentSlot::Helm).map(VisibleItem::of),
        armor: worn.get(EquipmentSlot::Armor).map(VisibleItem::of),
        pants: worn.get(EquipmentSlot::Pants).map(VisibleItem::of),
        gloves: worn.get(EquipmentSlot::Gloves).map(VisibleItem::of),
        boots: worn.get(EquipmentSlot::Boots).map(VisibleItem::of),
        wings: worn.get(EquipmentSlot::Wings).map(VisibleItem::of),
        pet: worn.get(EquipmentSlot::Pet).map(VisibleItem::of),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::class::CharacterClass;
    use crate::components::item_instance::{
        CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::movement::Movement;
    use crate::components::placement::Placement;
    use crate::components::pool::Pool;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::{ItemLevel, MapNumber};
    use crate::components::vitals::Vitals;
    use crate::data::classes::ClassRecord;

    fn placement() -> Placement {
        Placement {
            position: TileCoord::new(180, 120).to_world(),
            facing: Facing::POS_Y,
            movement: Movement::Grounded,
            map: MapNumber(0),
        }
    }

    fn vitals() -> Vitals {
        Vitals {
            health: Pool::full(700),
            mana: Pool::full(200),
            ability: Pool::full(1),
        }
    }

    /// A fresh Dark Knight, built through the proven creation constructor over a
    /// parse-gated class record — the projection only reads `class`, so the
    /// stats and level are immaterial here.
    fn dark_knight() -> Character {
        let record: ClassRecord = serde_json::from_value(serde_json::json!({
            "class": "dark_knight", "number": 4,
            "creation": {"kind": "always"}, "evolution": {"kind": "terminal"},
            "home_map": 0, "points_per_level": 5,
            "starting_stats": {"kind": "standard", "strength": 28, "agility": 20, "vitality": 25, "energy": 10},
            "starting_kit": [],
            "fruit_points_divisor": 400, "warp_requirement": {"kind": "full"},
            "source_version": "075"
        }))
        .unwrap();
        Character::fresh(&record, placement(), vitals())
    }

    fn item(group: u8, number: u16) -> ItemInstance {
        ItemInstance {
            item: ItemRef { group, number },
            level: ItemLevel::new(7).unwrap(),
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::new(5, 40).unwrap(),
            augment: CraftedAugment::None,
        }
    }

    #[test]
    fn a_naked_character_projects_class_and_no_worn_items() {
        let appearance = project_appearance(&dark_knight(), &Equipment::empty());
        assert_eq!(appearance.class, CharacterClass::DarkKnight);
        assert!(appearance.left_hand.is_none());
        assert!(appearance.right_hand.is_none());
        assert!(appearance.helm.is_none());
        assert!(appearance.armor.is_none());
        assert!(appearance.pants.is_none());
        assert!(appearance.gloves.is_none());
        assert!(appearance.boots.is_none());
        assert!(appearance.wings.is_none());
        assert!(appearance.pet.is_none());
    }

    #[test]
    fn each_worn_visible_slot_projects_its_item() {
        let worn = Equipment::empty()
            .with(EquipmentSlot::LeftHand, item(1, 0))
            .with(EquipmentSlot::Helm, item(7, 3))
            .with(EquipmentSlot::Wings, item(12, 2));
        let appearance = project_appearance(&dark_knight(), &worn);
        assert_eq!(appearance.left_hand, Some(VisibleItem::of(&item(1, 0))));
        assert_eq!(appearance.helm, Some(VisibleItem::of(&item(7, 3))));
        assert_eq!(appearance.wings, Some(VisibleItem::of(&item(12, 2))));
        // Untouched visible slots stay empty.
        assert!(appearance.right_hand.is_none());
        assert!(appearance.armor.is_none());
        assert!(appearance.pet.is_none());
    }

    #[test]
    fn worn_jewelry_never_enters_the_projection() {
        // A pendant and both rings seated in the worn set: the projection reads
        // none of them, so the appearance is byte-identical to the naked one.
        let bare = project_appearance(&dark_knight(), &Equipment::empty());
        let bejeweled = Equipment::empty()
            .with(EquipmentSlot::Pendant, item(13, 8))
            .with(EquipmentSlot::Ring1, item(13, 9))
            .with(EquipmentSlot::Ring2, item(13, 21));
        let appearance = project_appearance(&dark_knight(), &bejeweled);
        assert_eq!(appearance, bare);
    }

    #[test]
    fn the_projection_is_a_read_only_derivation() {
        // Both inputs stay usable after projecting — the derivation borrows and
        // never consumes them.
        let character = dark_knight();
        let worn = Equipment::empty().with(EquipmentSlot::Armor, item(8, 1));
        let first = project_appearance(&character, &worn);
        let second = project_appearance(&character, &worn);
        assert_eq!(first, second);
    }
}
