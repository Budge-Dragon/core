//! Record shape of `chaos_mixes.json` — chaos machine crafting recipes.
//!
//! Success rates and their bonuses are integer percent points (`_percent`
//! fields), not fractions — the domain couples them to zen cost and
//! additive percent bonuses.

use serde::{Deserialize, Serialize};

use super::common::{ItemRef, SourceVersion};
use super::item_options::OptionType;

/// One crafting recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChaosMix {
    /// Crafting number as the client knows it.
    pub number: u8,
    /// The recipe's slug.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Which crafting handler runs the recipe.
    pub behavior: MixBehavior,
    /// Money cost of one attempt.
    pub cost: MixCost,
    /// Success rate composition.
    pub success: MixSuccess,
    /// Required and optional ingredients.
    pub inputs: Vec<MixInput>,
    /// What a successful attempt yields.
    pub results: Vec<MixResult>,
    /// How results are picked from the list.
    pub result_selection: ResultSelection,
    /// Whether one attempt may yield multiple results.
    pub multiple_allowed: bool,
    /// Percent chances of bonus rolls on created items.
    pub result_chances: ResultChances,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// Which crafting handler runs a recipe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixBehavior {
    /// Generic data-driven mix.
    Simple,
    /// Chaos weapon / first wings handler.
    ChaosWeaponAndFirstWings,
    /// Second wings handler.
    SecondWings,
    /// Dinorant handler.
    Dinorant,
    /// Devil Square ticket handler.
    TicketDevilSquare,
    /// Blood Castle ticket handler.
    TicketBloodCastle,
}

/// Money cost of one crafting attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MixCost {
    /// Fixed money cost.
    pub flat_zen: u32,
    /// Money per percent point of success rate.
    pub zen_per_success_percent: u32,
}

/// Success rate composition of a recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixSuccess {
    /// Base success rate, in percent points.
    pub base_percent: u8,
    /// Cap on the total success rate, in percent points.
    pub max_percent: u8,
    /// Divisor turning the inputs' summed money value into percent points;
    /// absent = the recipe gains no value-based rate.
    pub npc_price_divisor: Option<u32>,
    /// Percent points added when an input carries luck.
    pub luck_bonus_percent: u8,
}

/// One ingredient line of a recipe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MixInput {
    /// Which items satisfy the line, kind-tagged.
    #[serde(rename = "match")]
    pub matcher: ItemMatch,
    /// How many matching items are consumed.
    pub amount: MixAmount,
    /// What happens to the items on success.
    pub on_success: MixItemAction,
    /// What happens to the items on failure.
    pub on_fail: MixItemAction,
    /// Divisor adding the matched items' money value as percent points;
    /// absent = the line adds no value-based rate.
    pub npc_price_divisor: Option<u32>,
    /// Percent points added per item beyond the minimum amount.
    pub add_percent_per_extra: u8,
    /// Link to a `modify` result upgrading this input; absent = unlinked.
    #[serde(rename = "ref")]
    pub reference: Option<u8>,
}

/// Which items satisfy an ingredient line, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ItemMatch {
    /// A fixed list of item identities.
    SpecificItems {
        /// The accepted items.
        items: Vec<ItemRef>,
    },
    /// Any item within a level window carrying the required option types.
    AnyItem {
        /// Minimum item level.
        min_level: u8,
        /// Maximum item level.
        max_level: u8,
        /// Option types the item must carry.
        required_option_types: Vec<OptionType>,
    },
}

/// How many items an ingredient line consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MixAmount {
    /// Minimum count; `0` = optional ingredient.
    pub min: u8,
    /// Maximum count; absent = unbounded (consume all matching).
    pub max: Option<u8>,
}

/// What happens to an ingredient after the attempt resolves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MixItemAction {
    /// The item is consumed.
    Disappear,
    /// The item is kept unchanged.
    Stays,
    /// Chaos-weapon downgrade rules apply.
    DowngradeChaosWeapon,
}

/// One result line of a recipe, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MixResult {
    /// A new item is created.
    Create {
        /// Item created.
        item: ItemRef,
        /// Inclusive `[min, max]` item level rolled for the result.
        level_range: [u8; 2],
        /// Fixed durability override; absent = the definition's durability.
        durability: Option<u8>,
    },
    /// A linked input is modified in place.
    Modify {
        /// The `ref` link of the input being upgraded.
        #[serde(rename = "ref")]
        reference: u8,
        /// Item levels added to the linked input.
        add_level: u8,
    },
}

/// How results are picked from a recipe's result list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSelection {
    /// One random result from the list.
    Any,
    /// Every result in the list.
    All,
}

/// Percent chances of bonus rolls on created items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResultChances {
    /// Chance the result rolls luck, in percent points.
    #[serde(rename = "luck_percent")]
    pub luck: u8,
    /// Chance the result rolls +Skill, in percent points.
    #[serde(rename = "skill_percent")]
    pub skill: u8,
    /// Chance the result comes out excellent, in percent points.
    #[serde(rename = "excellent_percent")]
    pub excellent: u8,
}
