//! Character appearance projection (W-APPEAR) over the real `/data` Atlas: the
//! core [`project_appearance`] read-model exercised against freshly-created
//! characters wearing their authored starter kits. Proves the body is the
//! character class, that each occupied visible slot projects exactly its worn
//! item's public cosmetic facts (identity, plus-level, rarity), that a naked
//! character projects class only, that no visible item ever leaks a private
//! economy fact (durability, options, luck, skill) onto the wire, and that
//! jewelry — pendant and both rings — never appears even when seated in the
//! worn set.
//!
//! Load failures route through `or_fail`; every assertion is a `#[test]` body
//! so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use mu_core::components::appearance::{CharacterAppearance, VisibleItem};
use mu_core::components::class::CharacterClass;
use mu_core::components::equipment::EquipmentSlot;
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::item_ref::ItemRef;
use mu_core::components::units::ItemLevel;
use mu_core::data::atlas::Atlas;
use mu_core::services::appearance::project_appearance;
use mu_core::services::creation::{CreatedCharacter, create_character};

use dataset::real_atlas;
use rng::TestRng;

/// An arbitrary fixed stream every scenario shares.
const SEED: u64 = 7;

/// The five classes a player can create.
const CREATABLE: [CharacterClass; 5] = [
    CharacterClass::DarkWizard,
    CharacterClass::DarkKnight,
    CharacterClass::FairyElf,
    CharacterClass::MagicGladiator,
    CharacterClass::DarkLord,
];

/// The nine visible slots, paired with the appearance field each projects into
/// — the crate-internal slot enumeration is not exported, so the visible
/// vocabulary is listed here (pendant and both rings are deliberately absent).
fn visible_slots(appearance: &CharacterAppearance) -> [(EquipmentSlot, Option<VisibleItem>); 9] {
    [
        (EquipmentSlot::LeftHand, appearance.left_hand),
        (EquipmentSlot::RightHand, appearance.right_hand),
        (EquipmentSlot::Helm, appearance.helm),
        (EquipmentSlot::Armor, appearance.armor),
        (EquipmentSlot::Pants, appearance.pants),
        (EquipmentSlot::Gloves, appearance.gloves),
        (EquipmentSlot::Boots, appearance.boots),
        (EquipmentSlot::Wings, appearance.wings),
        (EquipmentSlot::Pet, appearance.pet),
    ]
}

/// Creates a fresh character of `class` over the real atlas on a `seed` stream.
fn create(atlas: &Atlas, class: CharacterClass, seed: u64) -> CreatedCharacter {
    create_character(class, atlas, &mut TestRng::new(seed))
}

#[test]
fn the_body_is_the_character_class() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let appearance = project_appearance(&created.character, &created.equipment);
        assert_eq!(appearance.class, class, "{class:?} body class");
    }
}

#[test]
fn a_fresh_dark_wizard_projects_class_only() {
    let atlas = real_atlas();
    let created = create(&atlas, CharacterClass::DarkWizard, SEED);
    let appearance = project_appearance(&created.character, &created.equipment);
    assert_eq!(appearance.class, CharacterClass::DarkWizard);
    for (slot, projected) in visible_slots(&appearance) {
        assert!(projected.is_none(), "{slot:?} is empty on a naked wizard");
    }
}

#[test]
fn a_starter_dark_knight_shows_exactly_its_left_hand_axe() {
    let atlas = real_atlas();
    let created = create(&atlas, CharacterClass::DarkKnight, SEED);
    let appearance = project_appearance(&created.character, &created.equipment);
    // The authored kit is a plain, level-0 small axe (group 1, number 0).
    assert_eq!(
        appearance.left_hand,
        Some(VisibleItem {
            item: ItemRef {
                group: 1,
                number: 0,
            },
            level: ItemLevel::ZERO,
            rarity: ItemRarity::Normal,
        })
    );
    // No other visible slot is filled.
    for (slot, projected) in visible_slots(&appearance) {
        if slot != EquipmentSlot::LeftHand {
            assert!(projected.is_none(), "{slot:?} is empty");
        }
    }
}

#[test]
fn every_visible_slot_projects_exactly_its_worn_item() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let appearance = project_appearance(&created.character, &created.equipment);
        for (slot, projected) in visible_slots(&appearance) {
            let expected = created.equipment.get(slot).map(VisibleItem::of);
            assert_eq!(projected, expected, "{class:?} {slot:?} projection");
        }
    }
}

#[test]
fn no_visible_item_leaks_a_private_fact_onto_the_wire() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let appearance = project_appearance(&created.character, &created.equipment);
        let value: serde_json::Value = serde_json::to_value(&appearance).unwrap();
        let object = value.as_object().unwrap();
        for (key, slot_value) in object {
            if key == "class" {
                continue;
            }
            let item = slot_value.as_object().unwrap();
            // A visible item carries exactly its three public facts — never
            // durability, options, luck, skill, or an augment.
            let mut keys: Vec<&str> = item.keys().map(String::as_str).collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec!["item", "level", "rarity"],
                "{class:?} {key} exposes only cosmetic facts"
            );
        }
    }
}

#[test]
fn seated_jewelry_never_appears_in_the_projection() {
    // Rings carry a level requirement no fresh character meets, so they are
    // seated directly into the worn set (the by-construction path); the
    // projection reads none of the three jewelry slots, so the appearance is
    // byte-identical with and without them, and no jewelry key rides the wire.
    let atlas = real_atlas();
    let created = create(&atlas, CharacterClass::DarkKnight, SEED);
    let bare = project_appearance(&created.character, &created.equipment);

    let ring = |number| ItemInstance {
        item: ItemRef { group: 13, number },
        level: ItemLevel::ZERO,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(20),
        augment: CraftedAugment::None,
    };
    let bejeweled = created
        .equipment
        .clone()
        .with(EquipmentSlot::Pendant, ring(8))
        .with(EquipmentSlot::Ring1, ring(9))
        .with(EquipmentSlot::Ring2, ring(21));
    let appearance = project_appearance(&created.character, &bejeweled);
    assert_eq!(appearance, bare, "jewelry does not change the projection");

    let value: serde_json::Value = serde_json::to_value(&appearance).unwrap();
    let object = value.as_object().unwrap();
    assert!(!object.contains_key("pendant"));
    assert!(!object.contains_key("ring1"));
    assert!(!object.contains_key("ring2"));
}
