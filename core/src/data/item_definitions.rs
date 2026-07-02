//! Record shape of `item_definitions.json` — every item the game knows.

use serde::{Deserialize, Serialize};

use super::common::{
    Aggregate, BonusTableId, ClassId, EffectId, ItemRef, OptionId, ScaledBy, SetGroupId,
    SkillNumber, SourceVersion, StatId, StatRequirement,
};

/// One item definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemDefinition {
    /// The game's own item identity.
    pub id: ItemRef,
    /// Display name.
    pub name: String,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Inventory width in slots.
    pub width: u8,
    /// Inventory height in slots.
    pub height: u8,
    /// Equipment slot; absent = not equippable.
    pub slot: Option<ItemSlot>,
    /// Whether monsters can drop it.
    pub drops_from_monsters: bool,
    /// Monster level at which it enters the drop pool.
    pub drop_level: u8,
    /// Upper bound of the drop pool window; absent = unbounded.
    pub maximum_drop_level: Option<u8>,
    /// Highest reachable item level (`+11` cap pre-Season-3).
    pub max_item_level: u8,
    /// Base durability.
    pub durability: u8,
    /// Base money value.
    pub value: u32,
    /// Skill the item grants or teaches; absent = none.
    pub skill: Option<ItemSkill>,
    /// Magic effect applied when consumed; absent = not a consumable effect.
    pub consume_effect: Option<EffectId>,
    /// Whether the item is ammunition (arrows/bolts).
    pub is_ammunition: bool,
    /// Classes allowed to use the item.
    pub classes: Vec<ClassId>,
    /// Minimum stats required to equip.
    pub requirements: Vec<StatRequirement>,
    /// Stat modifications granted while equipped.
    pub base_power_ups: Vec<ItemPowerUp>,
    /// Option definitions the item can roll.
    pub possible_options: Vec<OptionId>,
    /// Set groups the item participates in.
    pub possible_set_groups: Vec<SetGroupId>,
    /// Box-of-Luck-style contents, keyed by the source item's level.
    pub box_drops: Vec<BoxDrop>,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Equipment slot an item occupies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemSlot {
    /// Left hand only.
    LeftHand,
    /// Right hand only.
    RightHand,
    /// Either hand.
    LeftOrRightHand,
    /// Helm.
    Helm,
    /// Armor.
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
    /// Ring.
    Ring,
}

/// How an item relates to a skill, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemSkill {
    /// Weapons/shields: usable while equipped when the instance rolled +Skill.
    GrantedWhileEquipped {
        /// The granted skill.
        skill: SkillNumber,
    },
    /// Orbs/scrolls: learned permanently when consumed.
    TaughtOnConsume {
        /// The taught skill.
        skill: SkillNumber,
    },
}

/// A stat modification granted by an item, optionally growing with item level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemPowerUp {
    /// Stat the power-up modifies.
    pub stat: StatId,
    /// Base value contributed.
    pub value: f64,
    /// How the value folds into the stat total.
    pub aggregate: Aggregate,
    /// Dynamic scaling terms added on top of the base value.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scaled_by: Vec<ScaledBy>,
    /// Cap on the final contributed value; absent = uncapped.
    pub max: Option<f64>,
    /// Per-item-level bonus added on top; absent = level-independent.
    pub bonus_table: Option<BonusTableId>,
}

/// One box-opening outcome for a given source item level.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoxDrop {
    /// Item level of the box this outcome applies to.
    pub source_item_level: u8,
    /// Probability of this outcome, `0.0..=1.0`.
    pub chance: f64,
    /// Minimum character level to receive the outcome.
    pub required_character_level: u16,
    /// What drops, kind-tagged.
    #[serde(flatten)]
    pub drop: BoxDropKind,
}

/// What a box drop produces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BoxDropKind {
    /// One item picked from a fixed list.
    ItemList {
        /// Candidate items.
        items: Vec<ItemRef>,
        /// Inclusive `[min, max]` item level of the dropped item.
        level_range: [u8; 2],
    },
    /// A random item from the regular drop pool.
    RandomItem {
        /// Inclusive `[min, max]` item level of the dropped item.
        level_range: [u8; 2],
    },
    /// A pile of money.
    Money {
        /// Amount of money dropped.
        amount: u32,
    },
}
