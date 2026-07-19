//! The public visible package one player needs to *draw* another: the body
//! (the character class, which fixes model and gender) plus the item worn in
//! each of the nine visible slots, each carried as its public cosmetic facts
//! only. Private economy facts (durability, rolled option magnitudes, luck,
//! skill, wear, crafted augments) have no field here, so they are absent by
//! construction rather than filtered. Jewelry — pendant and both rings — is
//! invisible on the body: those three slots have no field, so a private jewel
//! is unrepresentable in an appearance. Data only: the projection service
//! composes these from a character and its worn set; nothing here decides.

use serde::{Deserialize, Serialize};

use crate::components::class::CharacterClass;
use crate::components::item_instance::ItemInstance;
use crate::components::item_quality::ItemRarity;
use crate::components::item_ref::ItemRef;
use crate::components::units::ItemLevel;

/// One visible worn item — the public cosmetic projection of an
/// [`ItemInstance`]. It carries exactly the three facts another client renders
/// (which model, how bright the glow, which shine tier); the private economy
/// facts are deliberately absent, having no field to inhabit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisibleItem {
    /// Which model/texture to draw — the item's game identity.
    pub item: ItemRef,
    /// Glow intensity — the item's plus-level.
    pub level: ItemLevel,
    /// Shine tier — normal, excellent, or ancient.
    pub rarity: ItemRarity,
}

impl VisibleItem {
    /// Projects the public cosmetic facts of a worn instance — the single place
    /// "what is public about a worn item" is decided. Reads only the identity,
    /// plus-level, and rarity tier; every private fact is left behind because
    /// this type has nowhere to carry it.
    #[must_use]
    pub fn of(item: &ItemInstance) -> Self {
        Self {
            item: item.item,
            level: item.level,
            rarity: item.roll.rarity(),
        }
    }
}

/// What one player needs to draw another: the body (class) and the item in each
/// of the nine visible slots. Pendant and both rings have no field here, so
/// jewelry is unrepresentable in an appearance — invisibility is structural,
/// never a runtime filter. Each visible slot is genuinely optional (the slot
/// may be empty); a sparse wire omits empty slots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterAppearance {
    /// The body — the character class fixes both the model and the gender.
    pub class: CharacterClass,
    /// The item in the left weapon hand, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_hand: Option<VisibleItem>,
    /// The item in the right weapon hand, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_hand: Option<VisibleItem>,
    /// The worn helm, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub helm: Option<VisibleItem>,
    /// The worn body armor, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub armor: Option<VisibleItem>,
    /// The worn pants, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pants: Option<VisibleItem>,
    /// The worn gloves, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gloves: Option<VisibleItem>,
    /// The worn boots, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boots: Option<VisibleItem>,
    /// The worn wings, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wings: Option<VisibleItem>,
    /// The worn pet, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pet: Option<VisibleItem>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, ExcellentArmorSet, ExcellentOptions, LuckRoll, RarityRoll,
        RolledNormalOption, SkillRoll,
    };
    use crate::components::item_options::{AncientBonusLevel, ExcellentArmorOption, NormalOption};
    use crate::components::levels::OptionLevel;

    fn loaded_instance(roll: RarityRoll) -> ItemInstance {
        // A worn item dressed with every private fact set to a non-trivial
        // value: worn-down durability, a rolled normal option, luck, and skill.
        // None of these may reach the visible projection.
        ItemInstance {
            item: ItemRef {
                group: 2,
                number: 5,
            },
            level: ItemLevel::new(9).unwrap(),
            roll,
            normal_option: Some(RolledNormalOption {
                option: NormalOption::PhysicalDamage,
                level: OptionLevel::L4,
            }),
            luck: LuckRoll::Lucky,
            skill: SkillRoll::WithSkill,
            durability: Durability::new(3, 40).unwrap(),
            augment: CraftedAugment::None,
        }
    }

    #[test]
    fn of_carries_only_identity_level_and_rarity() {
        let instance = loaded_instance(RarityRoll::Normal);
        let visible = VisibleItem::of(&instance);
        assert_eq!(
            visible,
            VisibleItem {
                item: ItemRef {
                    group: 2,
                    number: 5,
                },
                level: ItemLevel::new(9).unwrap(),
                rarity: ItemRarity::Normal,
            }
        );
    }

    #[test]
    fn of_maps_each_rarity_tier() {
        assert_eq!(
            VisibleItem::of(&loaded_instance(RarityRoll::Normal)).rarity,
            ItemRarity::Normal
        );
        assert_eq!(
            VisibleItem::of(&loaded_instance(RarityRoll::Ancient {
                bonus: AncientBonusLevel::One,
            }))
            .rarity,
            ItemRarity::Ancient
        );
        assert_eq!(
            VisibleItem::of(&loaded_instance(RarityRoll::Excellent {
                options: ExcellentOptions::Armor {
                    options: ExcellentArmorSet::with_first(ExcellentArmorOption::MaxHealth, []),
                },
            }))
            .rarity,
            ItemRarity::Excellent
        );
    }

    fn appearance() -> CharacterAppearance {
        CharacterAppearance {
            class: CharacterClass::DarkKnight,
            left_hand: Some(VisibleItem::of(&loaded_instance(RarityRoll::Normal))),
            right_hand: None,
            helm: None,
            armor: Some(VisibleItem::of(&loaded_instance(RarityRoll::Ancient {
                bonus: AncientBonusLevel::Two,
            }))),
            pants: None,
            gloves: None,
            boots: None,
            wings: None,
            pet: None,
        }
    }

    #[test]
    fn wire_round_trips() {
        let value = appearance();
        let json = serde_json::to_string(&value).unwrap();
        assert_eq!(
            serde_json::from_str::<CharacterAppearance>(&json).unwrap(),
            value
        );
    }

    #[test]
    fn sparse_wire_omits_empty_slots() {
        let json = serde_json::to_string(&appearance()).unwrap();
        assert!(json.contains(r#""class":"dark_knight""#));
        assert!(json.contains("left_hand"));
        assert!(json.contains("armor"));
        // Empty visible slots are absent from the wire.
        assert!(!json.contains("right_hand"));
        assert!(!json.contains("helm"));
        assert!(!json.contains("wings"));
        assert!(!json.contains("pet"));
    }

    #[test]
    fn jewelry_slots_have_no_wire_representation() {
        // Structural invisibility: there is no field for pendant or rings, so a
        // serialized appearance can never carry those keys.
        let value: serde_json::Value = serde_json::to_value(appearance()).unwrap();
        let object = value.as_object().unwrap();
        assert!(!object.contains_key("pendant"));
        assert!(!object.contains_key("ring1"));
        assert!(!object.contains_key("ring2"));
    }

    #[test]
    fn a_visible_item_serializes_its_three_public_facts() {
        let visible = VisibleItem::of(&loaded_instance(RarityRoll::Ancient {
            bonus: AncientBonusLevel::One,
        }));
        let value: serde_json::Value = serde_json::to_value(visible).unwrap();
        let object = value.as_object().unwrap();
        // Exactly the three cosmetic facts — no durability, option, luck, skill.
        assert_eq!(object.len(), 3);
        assert!(object.contains_key("item"));
        assert!(object.contains_key("level"));
        assert_eq!(
            object.get("rarity").and_then(|r| r.as_str()),
            Some("ancient")
        );
    }
}
