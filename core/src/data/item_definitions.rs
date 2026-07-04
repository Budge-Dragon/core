//! `item_definitions.json` shapes: one kind-tagged record per item.

use serde::{Deserialize, Serialize};

use crate::components::bonus::CombatBonus;
use crate::components::class::ClassSet;
use crate::components::element::Element;
use crate::components::item_options::{ExcellentCategory, NormalOption};
use crate::components::levels::TransformationLevel;
use crate::components::units::{ItemLevel, Zen};

use super::common::{ItemRef, MonsterNumber, Provenance, SkillNumber};

/// One item definition. Shared columns are the authentic Item.txt-era facts;
/// everything behavioral lives on the kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemDefinition {
    /// The game's own item identity (group 0-15 x number).
    pub id: ItemRef,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Inventory cells horizontal.
    pub width: u8,
    /// Inventory cells vertical.
    pub height: u8,
    /// Item.txt Drop flag — whether monsters can drop it.
    pub drops_from_monsters: bool,
    /// Item.txt Level column: the item's base drop level (minimum monster
    /// level for the drop band).
    pub drop_level: u8,
    /// Highest reachable wire item level for this item.
    pub max_item_level: ItemLevel,
    /// Item.txt Dur column. Wear pool for equipment; round count for
    /// ammunition; per-kind interpretation is a services rule.
    pub durability: u8,
    /// How the NPC prices it — explicit, no zero-sentinel.
    pub price: ItemPrice,
    /// What the item is; flattened so the wire record carries `kind` plus the
    /// variant's fields inline.
    #[serde(flatten)]
    pub kind: ItemKind,
}

/// NPC pricing, kind-tagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemPrice {
    /// Fixed zen worth (consumables, scrolls, orbs, jewels).
    Fixed {
        /// The fixed NPC value.
        zen: Zen,
    },
    /// Priced per instance item level by a classic per-level table
    /// (mix materials, event tickets).
    PerLevel {
        /// The per-level price table, clamped to its last entry.
        zen_by_level: PerLevelPrice,
    },
    /// Computed from drop level plus item level by the classic price formula
    /// (services rule).
    Formula,
}

/// A per-level price table in the classic held-apart-last shape: `last` is
/// proven present at parse, so [`PerLevelPrice::at`] past the leading run is
/// total by construction — no runtime indexing, no lookup-shaped fallback.
/// Wire form: a non-empty JSON array of zen values in item-level order
/// (level 0 first); an empty array is a parse error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<Zen>", into = "Vec<Zen>")]
pub struct PerLevelPrice {
    leading: Vec<Zen>,
    last: Zen,
}

impl PerLevelPrice {
    /// The price at an instance item level — total: walks `leading` to the
    /// 0-based level and saturates to `last` past it.
    #[must_use]
    pub fn at(&self, level: ItemLevel) -> Zen {
        let target = usize::from(u8::from(level));
        let mut position = 0usize;
        for &zen in &self.leading {
            if position == target {
                return zen;
            }
            position = position.saturating_add(1);
        }
        self.last
    }
}

impl TryFrom<Vec<Zen>> for PerLevelPrice {
    type Error = PerLevelPriceError;

    fn try_from(mut rows: Vec<Zen>) -> Result<Self, Self::Error> {
        let last = rows.pop().ok_or(PerLevelPriceError::Empty)?;
        Ok(Self {
            leading: rows,
            last,
        })
    }
}

impl From<PerLevelPrice> for Vec<Zen> {
    fn from(table: PerLevelPrice) -> Self {
        let mut rows = table.leading;
        rows.push(table.last);
        rows
    }
}

/// Parse failure: a per-level price table with no entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerLevelPriceError {
    /// The wire array was empty — every table carries at least one price.
    Empty,
}

impl core::fmt::Display for PerLevelPriceError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Empty => write!(f, "a per-level price table must have at least one entry"),
        }
    }
}

impl core::error::Error for PerLevelPriceError {}

/// What an item is. Closed pre-S3 set derived from the shipped records. Each
/// variant carries exactly the columns its family has in Item.txt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemKind {
    /// Melee weapon (groups 0-3: swords, axes, maces/scepters, spears).
    Weapon {
        /// One- or two-handed wielding.
        handling: WeaponHandling,
        /// Minimum physical damage.
        min_damage: u16,
        /// Maximum physical damage.
        max_damage: u16,
        /// Attack speed column.
        attack_speed: u16,
        /// Weapon skill usable while equipped when the instance rolled +Skill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<SkillNumber>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Bow (group 4 numbers 0-6). Occupies the bow hand; consumes Arrows.
    Bow {
        /// Minimum physical damage.
        min_damage: u16,
        /// Maximum physical damage.
        max_damage: u16,
        /// Attack speed column.
        attack_speed: u16,
        /// Weapon skill usable while equipped when the instance rolled +Skill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<SkillNumber>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Crossbow (group 4 numbers 8-16). Occupies the crossbow hand; consumes
    /// Bolts.
    Crossbow {
        /// Minimum physical damage.
        min_damage: u16,
        /// Maximum physical damage.
        max_damage: u16,
        /// Attack speed column.
        attack_speed: u16,
        /// Weapon skill usable while equipped when the instance rolled +Skill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<SkillNumber>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Arrows (4/15) — ammunition for bows; durability is the round count.
    Arrows {
        /// Classes able to use it.
        classes: ClassSet,
    },
    /// Bolt (4/7) — ammunition for crossbows; durability is the round count.
    Bolts {
        /// Classes able to use it.
        classes: ClassSet,
    },
    /// Staff (group 5). `magic_power` is the raw Item.txt column; rise is a
    /// services rule, not stored data.
    Staff {
        /// Minimum physical damage.
        min_damage: u16,
        /// Maximum physical damage.
        max_damage: u16,
        /// Attack speed column.
        attack_speed: u16,
        /// Raw Item.txt magic-power column.
        magic_power: u16,
        /// Weapon skill usable while equipped when the instance rolled +Skill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<SkillNumber>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Shield (group 6). Carries its own defense and defense (success) rate.
    Shield {
        /// Defense.
        defense: u16,
        /// Defense success rate.
        defense_rate: u16,
        /// Weapon skill usable while equipped when the instance rolled +Skill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<SkillNumber>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Helm (group 7).
    Helm {
        /// Defense.
        defense: u16,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Body armor (group 8).
    BodyArmor {
        /// Defense.
        defense: u16,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Pants (group 9).
    Pants {
        /// Defense.
        defense: u16,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Gloves (group 10). Attack speed is the Item.txt speed column; 0 = none.
    Gloves {
        /// Defense.
        defense: u16,
        /// Attack speed column.
        attack_speed: u16,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Boots (group 11).
    Boots {
        /// Defense.
        defense: u16,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Wings (group 12 numbers 0-6, Cape of Lord 13/30). Percents are integer
    /// percent points at item level 0; per-level growth is a services rule
    /// keyed by tier.
    Wings {
        /// Wing generation.
        tier: WingTier,
        /// Defense.
        defense: u16,
        /// Damage absorption at item level 0, in percent points.
        absorb_percent: u8,
        /// Damage increase at item level 0, in percent points.
        damage_percent: u8,
        /// Normal-option kinds this wing can roll (Jewel-of-Life leveled).
        jol_options: Vec<NormalOption>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Pet (13/0-3): Guardian Angel, Imp, Horn of Uniria, Horn of Dinorant.
    Pet {
        /// Whether and how it is ridden.
        ride: PetRide,
        /// Fixed bonuses while equipped — resolved `CombatBonus` values
        /// serialized inline.
        bonuses: Vec<CombatBonus>,
        /// Skill granted while equipped (Dinorant's attack, 49).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        skill: Option<SkillNumber>,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Ring (group 13). Resistance absent = the piece grants none.
    Ring {
        /// Resistance the ring grants; absent = none (Ring of Magic).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resistance: Option<Element>,
        /// The jewelry option this ring rolls.
        option: NormalOption,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Pendant (group 13). Pendants roll a weapon-excellent category — a
    /// genuine per-item fact.
    Pendant {
        /// Resistance the pendant grants; absent = none.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resistance: Option<Element>,
        /// The jewelry option this pendant rolls.
        option: NormalOption,
        /// The excellent category the pendant rolls.
        excellent: ExcellentCategory,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Transformation Ring (13/10). The ring's own item level 0..=5 selects the
    /// monster skin; the mapping is a total structure.
    TransformationRing {
        /// Level-to-skin mapping.
        skins: TransformationSkins,
        /// Classes able to equip it.
        classes: ClassSet,
        /// Wear requirements.
        wear: WearRequirements,
    },
    /// Orb (12/7-11): teaches a skill on consumption.
    Orb {
        /// The skill taught.
        teaches: SkillNumber,
        /// Learn requirements.
        learn: LearnRequirements,
        /// Classes able to learn from it.
        classes: ClassSet,
    },
    /// Skill scroll (group 15): teaches a skill on consumption.
    SkillScroll {
        /// The skill taught.
        teaches: SkillNumber,
        /// Learn requirements.
        learn: LearnRequirements,
        /// Classes able to learn from it.
        classes: ClassSet,
    },
    /// Jewel (Chaos 12/15, Bless 14/13, Soul 14/14, Life 14/16, Creation
    /// 14/22).
    Jewel {
        /// Which jewel.
        jewel: JewelKind,
    },
    /// Consumable (group 14 potions, Apple, Antidote, Ale, Town Portal Scroll).
    Consumable {
        /// What consuming it does.
        effect: ConsumeEffect,
    },
    /// Box of Luck family (14/11) — kind tag only; the record carries no
    /// contents.
    LuckyBox,
    /// Finished event-entry ticket; item level selects the event tier.
    EventTicket {
        /// Which event the ticket admits.
        event: EventKind,
    },
    /// Inert chaos-machine ingredient. Recipes reference it by `ItemRef`.
    MixMaterial,
    /// Stat fruit (13/15); item level 0-4 selects the stat (services rule).
    StatFruit,
}

impl ItemKind {
    /// The weapon-skill column of the kind, when its family carries one.
    /// Total over every variant: a family without the column is `None`.
    #[must_use]
    pub fn skill(&self) -> Option<SkillNumber> {
        match self {
            Self::Weapon { skill, .. }
            | Self::Bow { skill, .. }
            | Self::Crossbow { skill, .. }
            | Self::Staff { skill, .. }
            | Self::Shield { skill, .. }
            | Self::Pet { skill, .. } => *skill,
            Self::Arrows { .. }
            | Self::Bolts { .. }
            | Self::Helm { .. }
            | Self::BodyArmor { .. }
            | Self::Pants { .. }
            | Self::Gloves { .. }
            | Self::Boots { .. }
            | Self::Wings { .. }
            | Self::Ring { .. }
            | Self::Pendant { .. }
            | Self::TransformationRing { .. }
            | Self::Orb { .. }
            | Self::SkillScroll { .. }
            | Self::Jewel { .. }
            | Self::Consumable { .. }
            | Self::LuckyBox
            | Self::EventTicket { .. }
            | Self::MixMaterial
            | Self::StatFruit => None,
        }
    }
}

/// How a melee weapon is wielded. One-handed melee wields in either hand and
/// is dual-wield eligible; two-handed occupies the weapon hand alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponHandling {
    /// One-handed.
    OneHanded,
    /// Two-handed.
    TwoHanded,
}

/// Wing generation — selects the per-level growth rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WingTier {
    /// First-generation wings.
    First,
    /// Second-generation wings.
    Second,
}

/// Whether and how a pet is ridden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PetRide {
    /// Not rideable (Guardian Angel, Imp).
    NotRideable,
    /// A ground mount.
    GroundMount,
    /// A flying mount (Dinorant).
    FlyingMount,
}

/// The five pre-S3 jewels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JewelKind {
    /// Jewel of Bless.
    Bless,
    /// Jewel of Soul.
    Soul,
    /// Jewel of Chaos.
    Chaos,
    /// Jewel of Life.
    Life,
    /// Jewel of Creation.
    Creation,
}

/// What consuming the item does — the kind only; magnitudes are rules, not
/// data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConsumeEffect {
    /// Restores health.
    Healing {
        /// Strength of the heal.
        tier: HealingTier,
    },
    /// Restores mana.
    Mana {
        /// Strength of the restore.
        tier: ManaTier,
    },
    /// Cures poison.
    Antidote,
    /// Intoxicates (Ale).
    Alcohol,
    /// Returns to town.
    TownPortal,
}

/// Healing strength ladder (Apple is the weakest heal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealingTier {
    /// Apple.
    Apple,
    /// Small potion.
    Small,
    /// Medium potion.
    Medium,
    /// Large potion.
    Large,
}

/// Mana potion strength ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManaTier {
    /// Small potion.
    Small,
    /// Medium potion.
    Medium,
    /// Large potion.
    Large,
}

/// Devil Square vs Blood Castle entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    /// Devil Square entry.
    DevilSquare,
    /// Blood Castle entry.
    BloodCastle,
}

/// Item.txt wear-requirement columns, raw. Stat columns are stored raw and
/// scaled at equip time by the classic formula; `level` is compared raw.
/// 0 = no requirement (the authentic file encoding).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WearRequirements {
    /// Minimum character level.
    pub level: u16,
    /// Raw strength requirement column.
    pub strength: u16,
    /// Raw agility requirement column.
    pub agility: u16,
    /// Raw vitality requirement column.
    pub vitality: u16,
    /// Raw energy requirement column.
    pub energy: u16,
    /// Raw command requirement column.
    pub command: u16,
}

/// Absolute minima checked as-is when learning from an orb/scroll (consumption
/// requirements are never drop-level scaled — that formula is wear-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearnRequirements {
    /// Minimum character level.
    pub level: u16,
    /// Minimum strength.
    pub strength: u16,
    /// Minimum agility.
    pub agility: u16,
    /// Minimum energy.
    pub energy: u16,
}

/// The six transformation skins keyed by the ring's own transformation level —
/// a total structure parsed from the wire array (exactly six entries, proven
/// at parse).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<MonsterNumber>", into = "Vec<MonsterNumber>")]
pub struct TransformationSkins {
    l0: MonsterNumber,
    l1: MonsterNumber,
    l2: MonsterNumber,
    l3: MonsterNumber,
    l4: MonsterNumber,
    l5: MonsterNumber,
}

impl TransformationSkins {
    /// Total: exhaustive match over `TransformationLevel`'s six variants —
    /// every level has a skin; no indexing, no `Option`.
    #[must_use]
    pub fn skin(&self, level: TransformationLevel) -> MonsterNumber {
        match level {
            TransformationLevel::L0 => self.l0,
            TransformationLevel::L1 => self.l1,
            TransformationLevel::L2 => self.l2,
            TransformationLevel::L3 => self.l3,
            TransformationLevel::L4 => self.l4,
            TransformationLevel::L5 => self.l5,
        }
    }
}

impl TryFrom<Vec<MonsterNumber>> for TransformationSkins {
    type Error = TransformationSkinsError;

    fn try_from(skins: Vec<MonsterNumber>) -> Result<Self, Self::Error> {
        let found = skins.len();
        let [l0, l1, l2, l3, l4, l5] = <[MonsterNumber; 6]>::try_from(skins)
            .map_err(|_| TransformationSkinsError { found })?;
        Ok(Self {
            l0,
            l1,
            l2,
            l3,
            l4,
            l5,
        })
    }
}

impl From<TransformationSkins> for Vec<MonsterNumber> {
    fn from(skins: TransformationSkins) -> Self {
        vec![skins.l0, skins.l1, skins.l2, skins.l3, skins.l4, skins.l5]
    }
}

/// Parse failure: a transformation-ring skin list not exactly six entries long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransformationSkinsError {
    /// The number of skins found.
    pub found: usize,
}

impl core::fmt::Display for TransformationSkinsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "expected 6 transformation skins, found {}", self.found)
    }
}

impl core::error::Error for TransformationSkinsError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(rows: &[u64]) -> PerLevelPrice {
        PerLevelPrice::try_from(rows.iter().copied().map(Zen).collect::<Vec<_>>()).unwrap()
    }

    fn level(value: u8) -> ItemLevel {
        ItemLevel::new(value).unwrap()
    }

    #[test]
    fn per_level_price_reads_each_leading_row_and_saturates_to_last() {
        // Loch's Feather: level 0 -> 180000, level >= 1 -> 7500000.
        let feather = table(&[180_000, 7_500_000]);
        assert_eq!(feather.at(ItemLevel::ZERO), Zen(180_000));
        assert_eq!(feather.at(level(1)), Zen(7_500_000));
        assert_eq!(feather.at(level(15)), Zen(7_500_000));

        let invitation = table(&[60_000, 60_000, 84_000, 120_000, 180_000]);
        assert_eq!(invitation.at(level(2)), Zen(84_000));
        assert_eq!(invitation.at(level(4)), Zen(180_000));
        assert_eq!(invitation.at(level(9)), Zen(180_000));
    }

    #[test]
    fn per_level_price_with_one_row_is_that_row_everywhere() {
        let flat = table(&[42]);
        assert_eq!(flat.at(ItemLevel::ZERO), Zen(42));
        assert_eq!(flat.at(level(15)), Zen(42));
    }

    #[test]
    fn per_level_price_rejects_an_empty_table() {
        assert_eq!(
            PerLevelPrice::try_from(Vec::new()),
            Err(PerLevelPriceError::Empty)
        );
        assert!(serde_json::from_str::<PerLevelPrice>("[]").is_err());
    }

    #[test]
    fn per_level_price_wire_round_trips_as_a_plain_array() {
        let price = ItemPrice::PerLevel {
            zen_by_level: table(&[180_000, 7_500_000]),
        };
        let json = serde_json::to_string(&price).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"per_level","zen_by_level":[180000,7500000]}"#
        );
        assert_eq!(serde_json::from_str::<ItemPrice>(&json).unwrap(), price);
    }
}
