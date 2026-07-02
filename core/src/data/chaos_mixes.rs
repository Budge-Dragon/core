//! Record shape of `chaos_mixes.json` — the chaos machine's closed recipe
//! catalog.
//!
//! Each record is one bespoke recipe. The kind IS the recipe family; a variant
//! carries the family's facts and economics only — no behavior. Success rates
//! and chances are `Percent`; fees and value rates are `Zen`; ingredient levels
//! are the wire `ItemLevel`. Every bound is proven at deserialize.

use core::num::NonZeroU8;

use serde::{Deserialize, Serialize};

use crate::components::interval::Interval;
use crate::components::units::{ItemLevel, Percent, Zen};

use super::common::{ItemRef, SourceVersion};

/// One chaos machine recipe record: provenance envelope around the recipe.
///
/// The provenance pair is inlined rather than a flattened `Provenance`: this is
/// the single deliberate carve-out from the crate-wide flattened-provenance
/// convention, kept so the recipe stays the record's one nested payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChaosMix {
    /// Display name of the mix (authentic recipe name).
    pub name: String,
    /// Dataset era the record's values were extracted from.
    pub source_version: SourceVersion,
    /// Review note naming OpenMU-default values or era doubts; absent =
    /// uncontested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
    /// The recipe, kind-tagged by family.
    pub recipe: ChaosRecipe,
}

/// Inclusive item-level window an ingredient must fall in; edge order
/// (`min <= max`) is proven at deserialize.
pub type ItemLevelWindow = Interval<ItemLevel>;

/// An exact item at an exact level (e.g. Loch's Feather +0 vs Monarch's Crest
/// +1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemAtLevel {
    /// The item.
    pub item: ItemRef,
    /// The exact required level.
    pub level: ItemLevel,
}

/// The closed set of pre-S3 item-upgrade targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeTarget {
    /// +9 item to +10.
    PlusTen,
    /// +10 item to +11.
    PlusEleven,
}

/// The wing-tier mix economics, shared verbatim by `second_wings` and
/// `cape_of_lord` (same fee, cap, value rates, and bonus chances).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WingEconomics {
    /// Flat attempt fee.
    pub fee_zen: Zen,
    /// Authentic 90% success cap.
    pub max_success_percent: Percent,
    /// Zen of the base wing's NPC value per success percent point.
    pub wing_value_zen_per_percent: Zen,
    /// Zen of summed excellent-item NPC value per success percent point.
    pub excellent_value_zen_per_percent: Zen,
    /// Chance the created item rolls luck.
    pub luck_chance_percent: Percent,
    /// Chance the created item rolls one excellent option.
    pub excellent_chance_percent: Percent,
}

/// A chaos machine recipe. Each variant carries exactly its own facts and
/// economics; no variant carries behavior.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChaosRecipe {
    /// One or more option-items are sacrificed for a random chaos weapon.
    ChaosWeapon {
        /// Level window of the sacrificed option-items.
        sacrifice_levels: ItemLevelWindow,
        /// The three craftable chaos weapons; one is created at random.
        weapons: [ItemRef; 3],
    },
    /// A chaos weapon (plus optional extra sacrifices) becomes a first wing.
    FirstWings {
        /// The accepted chaos weapons (exactly one placed).
        chaos_weapons: [ItemRef; 3],
        /// Level window of the placed chaos weapon (must carry an option).
        chaos_weapon_levels: ItemLevelWindow,
        /// Level window of optional extra option-item sacrifices.
        extra_sacrifice_levels: ItemLevelWindow,
        /// The three first wings; one is created at random.
        wings: [ItemRef; 3],
    },
    /// A first wing plus Loch's Feather becomes a second wing.
    SecondWings {
        /// The accepted first wings (exactly one placed).
        first_wings: [ItemRef; 3],
        /// Level window of the placed first wing.
        wing_levels: ItemLevelWindow,
        /// Level window of optional excellent-item sacrifices.
        excellent_levels: ItemLevelWindow,
        /// Loch's Feather at +0 (exactly one).
        feather: ItemAtLevel,
        /// Fee, cap, value rates, and bonus chances of the wing tier.
        economics: WingEconomics,
        /// The four second wings; one is created at random.
        wings: [ItemRef; 4],
    },
    /// A first wing plus Monarch's Crest becomes the Cape of Lord.
    CapeOfLord {
        /// The accepted first wings (exactly one placed).
        first_wings: [ItemRef; 3],
        /// Level window of the placed first wing.
        wing_levels: ItemLevelWindow,
        /// Level window of optional excellent-item sacrifices.
        excellent_levels: ItemLevelWindow,
        /// Monarch's Crest: Loch's Feather at +1 (exactly one).
        crest: ItemAtLevel,
        /// Fee, cap, value rates, and bonus chances of the wing tier.
        economics: WingEconomics,
        /// The cape created on success.
        cape: ItemRef,
    },
    /// One item at `target - 1` plus jewels; the placed item is upgraded in
    /// place on success and destroyed on failure.
    ItemUpgrade {
        /// The upgrade target level.
        target: UpgradeTarget,
        /// Jewels of Bless consumed.
        bless: NonZeroU8,
        /// Jewels of Soul consumed.
        soul: NonZeroU8,
        /// Base success rate.
        base_success_percent: Percent,
        /// Flat attempt fee.
        fee_zen: Zen,
    },
    /// Horns of Uniria plus a Jewel of Chaos become a Dinorant.
    Dinorant {
        /// Horn of Uniria.
        horn: ItemRef,
        /// Horns consumed.
        horn_count: NonZeroU8,
        /// Success rate.
        success_percent: Percent,
        /// Flat attempt fee.
        fee_zen: Zen,
        /// The Dinorant created on success.
        dinorant: ItemRef,
    },
    /// A Jewel of Creation plus a Jewel of Chaos become a stat fruit.
    Fruits {
        /// Jewel of Creation.
        catalyst: ItemRef,
        /// Success rate.
        success_percent: Percent,
        /// Flat attempt fee.
        fee_zen: Zen,
        /// The fruit item created on success.
        fruit: ItemRef,
    },
    /// Devil's Eye + Devil's Key of equal level + 1 Jewel of Chaos become a
    /// Devil's Invitation at that level.
    DevilSquareTicket {
        /// Devil's Eye.
        eye: ItemRef,
        /// Devil's Key.
        key: ItemRef,
        /// Devil's Invitation created on success, at the inputs' level.
        invitation: ItemRef,
        /// Attempt fee per ticket level (level 1..=7 in entry order).
        fee_zen_by_level: [Zen; 7],
        /// Success rate per ticket level (level 1..=7 in entry order).
        success_percent_by_level: [Percent; 7],
    },
    /// Scroll of Archangel + Blood Bone of equal level + 1 Jewel of Chaos
    /// become a Cloak of Invisibility at that level.
    BloodCastleTicket {
        /// Scroll of Archangel.
        scroll: ItemRef,
        /// Blood Bone.
        bone: ItemRef,
        /// Cloak of Invisibility created on success, at the inputs' level.
        cloak: ItemRef,
        /// Attempt fee per ticket level (level 1..=8 in entry order).
        fee_zen_by_level: [Zen; 8],
        /// Success rate per ticket level (level 1..=8 in entry order).
        success_percent_by_level: [Percent; 8],
    },
}
