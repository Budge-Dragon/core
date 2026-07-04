//! The referential-integrity checks [`Atlas::parse`](super::Atlas::parse) runs
//! over the already-indexed lookups: every monster attack, summon, item skill,
//! ancient piece, chaos-recipe ingredient, special/box drop, and class home
//! resolves to a record that exists. Each returns the first [`AtlasError`] it
//! finds. (Chaos-recipe *output* refs are proven by the resolve module's
//! catalog join, which retains the proof as the definition-joined catalog.)

use std::collections::{BTreeMap, BTreeSet};

use crate::components::levels::TransformationLevel;
use crate::data::ancient_sets::AncientSet;
use crate::data::box_drops::BoxDrop;
use crate::data::chaos_mixes::{ChaosMix, ChaosRecipe};
use crate::data::classes::ClassRecord;
use crate::data::common::{ItemRef, MapNumber, MonsterNumber, SkillNumber};
use crate::data::item_definitions::{ItemDefinition, ItemKind};
use crate::data::monster_definitions::{MonsterAttack, MonsterDefinition, MonsterRole};
use crate::data::skills::{Skill, SkillShape};
use crate::data::special_drops::{SpecialDrop, SpecialDropRecord};

use super::AtlasError;

pub(super) fn check_monster_attacks(
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
    skills: &BTreeMap<SkillNumber, Skill>,
) -> Result<(), AtlasError> {
    for monster in monsters.values() {
        let attack = match &monster.role {
            MonsterRole::Monster { attack, .. } | MonsterRole::Trap { attack, .. } => attack,
            MonsterRole::Guard { .. } | MonsterRole::Npc { .. } | MonsterRole::SoccerBall => {
                continue;
            }
        };
        if let MonsterAttack::Skill { skill } = attack {
            require_skill(skills, *skill)?;
        }
    }
    Ok(())
}

pub(super) fn check_summons(
    skills: &BTreeMap<SkillNumber, Skill>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
) -> Result<(), AtlasError> {
    for skill in skills.values() {
        if let SkillShape::Summon { monster } = skill.shape {
            require_monster(monsters, monster)?;
        }
    }
    Ok(())
}

pub(super) fn check_items(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    skills: &BTreeMap<SkillNumber, Skill>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
) -> Result<(), AtlasError> {
    for item in items.values() {
        match &item.kind {
            ItemKind::Weapon { skill, .. }
            | ItemKind::Bow { skill, .. }
            | ItemKind::Crossbow { skill, .. }
            | ItemKind::Staff { skill, .. }
            | ItemKind::Shield { skill, .. }
            | ItemKind::Pet { skill, .. } => {
                if let Some(skill) = skill {
                    require_skill(skills, *skill)?;
                }
            }
            ItemKind::Orb { teaches, .. } | ItemKind::SkillScroll { teaches, .. } => {
                require_skill(skills, *teaches)?;
            }
            ItemKind::TransformationRing { skins, .. } => {
                for level in TransformationLevel::ALL {
                    require_monster(monsters, skins.skin(level))?;
                }
            }
            ItemKind::Arrows { .. }
            | ItemKind::Bolts { .. }
            | ItemKind::Helm { .. }
            | ItemKind::BodyArmor { .. }
            | ItemKind::Pants { .. }
            | ItemKind::Gloves { .. }
            | ItemKind::Boots { .. }
            | ItemKind::Wings { .. }
            | ItemKind::Ring { .. }
            | ItemKind::Pendant { .. }
            | ItemKind::Jewel { .. }
            | ItemKind::Consumable { .. }
            | ItemKind::LuckyBox
            | ItemKind::EventTicket { .. }
            | ItemKind::MixMaterial
            | ItemKind::StatFruit => {}
        }
    }
    Ok(())
}

pub(super) fn check_ancient_sets(
    sets: &[AncientSet],
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<(), AtlasError> {
    for set in sets {
        for piece in &set.pieces {
            require_item(items, piece.item)?;
        }
    }
    Ok(())
}

/// Proves every chaos-recipe *ingredient* ref resolves. Output refs are
/// deliberately absent here: the resolve module's catalog join proves them by
/// joining each to its definition, retaining the proof instead of discarding
/// it.
pub(super) fn check_chaos_mixes(
    mixes: &[ChaosMix],
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<(), AtlasError> {
    for mix in mixes {
        for item in ingredient_refs(&mix.recipe) {
            require_item(items, item)?;
        }
    }
    Ok(())
}

/// A recipe's ingredient refs — every `ItemRef` matched against placed items,
/// never the outputs. Total over [`ChaosRecipe`].
fn ingredient_refs(recipe: &ChaosRecipe) -> Vec<ItemRef> {
    match recipe {
        ChaosRecipe::ChaosWeapon { .. } | ChaosRecipe::ItemUpgrade { .. } => Vec::new(),
        ChaosRecipe::FirstWings { chaos_weapons, .. } => chaos_weapons.to_vec(),
        ChaosRecipe::SecondWings {
            first_wings,
            feather,
            ..
        } => first_wings.iter().copied().chain([feather.item]).collect(),
        ChaosRecipe::CapeOfLord {
            first_wings, crest, ..
        } => first_wings.iter().copied().chain([crest.item]).collect(),
        ChaosRecipe::Dinorant { horn, .. } => vec![*horn],
        ChaosRecipe::Fruits { catalyst, .. } => vec![*catalyst],
        ChaosRecipe::DevilSquareTicket { eye, key, .. } => vec![*eye, *key],
        ChaosRecipe::BloodCastleTicket { scroll, bone, .. } => vec![*scroll, *bone],
    }
}

pub(super) fn check_special_drops(
    records: &[SpecialDropRecord],
    items: &BTreeMap<ItemRef, ItemDefinition>,
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
    map_numbers: &BTreeSet<MapNumber>,
) -> Result<(), AtlasError> {
    for record in records {
        match &record.drop {
            SpecialDrop::LevelBanded { item, .. } => require_item(items, *item)?,
            SpecialDrop::MonsterBound {
                monster,
                items: drops,
                ..
            } => {
                require_monster(monsters, *monster)?;
                for &item in drops.iter() {
                    require_item(items, item)?;
                }
            }
            SpecialDrop::MapBound { map, item, .. } => {
                if !map_numbers.contains(map) {
                    return Err(AtlasError::UnknownMapRef { map: *map });
                }
                require_item(items, *item)?;
            }
        }
    }
    Ok(())
}

pub(super) fn check_box_drops(
    records: &[BoxDrop],
    items: &BTreeMap<ItemRef, ItemDefinition>,
) -> Result<(), AtlasError> {
    for record in records {
        require_item(items, record.box_item)?;
        for &item in record.items.iter() {
            require_item(items, item)?;
        }
    }
    Ok(())
}

pub(super) fn check_classes(
    classes: &[ClassRecord],
    map_numbers: &BTreeSet<MapNumber>,
) -> Result<(), AtlasError> {
    for class in classes {
        if !map_numbers.contains(&class.home_map) {
            return Err(AtlasError::UnknownMapRef {
                map: class.home_map,
            });
        }
    }
    Ok(())
}

pub(super) fn require_skill(
    skills: &BTreeMap<SkillNumber, Skill>,
    skill: SkillNumber,
) -> Result<(), AtlasError> {
    if skills.contains_key(&skill) {
        Ok(())
    } else {
        Err(AtlasError::UnknownSkillRef { skill })
    }
}

pub(super) fn require_monster(
    monsters: &BTreeMap<MonsterNumber, MonsterDefinition>,
    monster: MonsterNumber,
) -> Result<(), AtlasError> {
    if monsters.contains_key(&monster) {
        Ok(())
    } else {
        Err(AtlasError::UnknownMonsterRef { monster })
    }
}

pub(super) fn require_item(
    items: &BTreeMap<ItemRef, ItemDefinition>,
    item: ItemRef,
) -> Result<(), AtlasError> {
    if items.contains_key(&item) {
        Ok(())
    } else {
        Err(AtlasError::UnknownItemRef { item })
    }
}
