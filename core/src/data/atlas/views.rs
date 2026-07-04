//! The resolved view types the [`Atlas`](super::Atlas) hands out: landings,
//! enter-gate and warp views, spawn entries, the per-map handle, and the
//! definition-joined chaos-recipe catalog — each over resolved state whose
//! referents were proven present at parse. The backing `Resolved*` records are
//! retained by the Atlas and read back through these views; only the Atlas
//! mints them, so no view has a public fabricating constructor.

use core::num::NonZeroU8;

use crate::components::collections::OneOrMore;
use crate::components::spatial::{Facing, WorldRect};
use crate::components::tile::WalkGrid;
use crate::components::units::{Percent, Zen};
use crate::data::chaos_mixes::{ItemAtLevel, ItemLevelWindow, UpgradeTarget, WingEconomics};
use crate::data::common::{ItemRef, MapNumber};
use crate::data::gates_warps::{EnterGate, Warp};
use crate::data::item_definitions::ItemDefinition;
use crate::data::map_definitions::MapDefinition;
use crate::data::monster_definitions::MonsterDefinition;
use crate::data::spawns::Spawn;

/// The landing side of a resolved gate reference, in world space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    /// Map the traveler lands on.
    pub map: MapNumber,
    /// Landing rectangle in world space.
    pub area: WorldRect,
    /// Facing on arrival; absent = unspecified (never fabricated).
    pub facing: Option<Facing>,
}

/// An enter gate with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnterGateView<'a> {
    /// The trigger-side record.
    pub gate: &'a EnterGate,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// An enter gate with its trigger area projected to world space and its
/// landing resolved at parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedEnterGate {
    pub(super) gate: EnterGate,
    pub(super) trigger: WorldRect,
    pub(super) landing: Landing,
}

/// A warp entry with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpView<'a> {
    /// The warp-list record.
    pub warp: &'a Warp,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// A spawn record joined to the monster definition it names, retained per map
/// at parse. The join is proven total here (the monster is looked up once,
/// during resolution), so a consuming service reaches the definition without an
/// `Option` — mirroring the `warps: Vec<(Warp, Landing)>` owned-join precedent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedSpawn {
    pub(super) spawn: Spawn,
    pub(super) monster: MonsterDefinition,
}

/// A spawn record borrowed with the monster definition it resolves to — the
/// public view over the atlas's owned spawn-to-monster join, mirroring
/// [`WarpView`]/[`EnterGateView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnEntry<'a> {
    /// The spawn record.
    pub spawn: &'a Spawn,
    /// The monster definition it names — resolution proven at parse.
    pub monster: &'a MonsterDefinition,
}

/// A chaos recipe with every **output** `ItemRef` joined to its
/// [`ItemDefinition`] at parse — the [`ResolvedSpawn`] owned-join precedent
/// applied to the recipe catalog. Ingredient refs stay refs (they are matched
/// by identity against placed items, so no definition is needed on the recipe
/// side); each variant otherwise carries its record's facts verbatim. Minted
/// only by the [`Atlas`](super::Atlas) at parse, in descending authentic
/// crafting-number scan order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedRecipe {
    /// One or more option-items are sacrificed for a random chaos weapon.
    ChaosWeapon {
        /// Level window of the sacrificed option-items.
        sacrifice_levels: ItemLevelWindow,
        /// The three craftable chaos weapons, joined.
        weapons: ResolvedOutput,
    },
    /// A chaos weapon (plus optional extra sacrifices) becomes a first wing.
    FirstWings {
        /// The accepted chaos weapons (exactly one placed).
        chaos_weapons: [ItemRef; 3],
        /// Level window of the placed chaos weapon (must carry an option).
        chaos_weapon_levels: ItemLevelWindow,
        /// Level window of optional extra option-item sacrifices.
        extra_sacrifice_levels: ItemLevelWindow,
        /// The three first wings, joined.
        wings: ResolvedOutput,
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
        /// The four second wings, joined.
        wings: ResolvedOutput,
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
        /// The cape created on success, joined.
        cape: ResolvedOutput,
    },
    /// One item at `target - 1` plus jewels; upgraded in place on success —
    /// the one family with no output to join.
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
        /// The Dinorant created on success, joined.
        dinorant: ResolvedOutput,
    },
    /// A Jewel of Creation plus a Jewel of Chaos become a stat fruit.
    Fruits {
        /// Jewel of Creation.
        catalyst: ItemRef,
        /// Success rate.
        success_percent: Percent,
        /// Flat attempt fee.
        fee_zen: Zen,
        /// The fruit item created on success, joined.
        fruit: ResolvedOutput,
    },
    /// Devil's Eye + Devil's Key of equal level + 1 Jewel of Chaos become a
    /// Devil's Invitation at that level.
    DevilSquareTicket {
        /// Devil's Eye.
        eye: ItemRef,
        /// Devil's Key.
        key: ItemRef,
        /// Devil's Invitation created on success, joined.
        invitation: ResolvedOutput,
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
        /// Cloak of Invisibility created on success, joined.
        cloak: ResolvedOutput,
        /// Attempt fee per ticket level (level 1..=8 in entry order).
        fee_zen_by_level: [Zen; 8],
        /// Success rate per ticket level (level 1..=8 in entry order).
        success_percent_by_level: [Percent; 8],
    },
}

/// A recipe's joined output definitions. Two variants because the RNG draw
/// order differs: [`Self::Choice`] spends one uniform word to pick;
/// [`Self::Single`] is deterministic and spends none. `uniform_below(1)` DOES
/// consume a word, so a degenerate one-element `Choice` would shift the RNG
/// stream — hence `Single` is a distinct variant, never a one-element
/// `Choice`. (The in-place upgrade family carries no output at all.)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedOutput {
    /// Several candidate results; the mix picks one uniformly (chaos weapons,
    /// first and second wings).
    Choice(OneOrMore<ItemDefinition>),
    /// Exactly one result, no pick draw (cape, dinorant, fruit, tickets).
    Single(ItemDefinition),
}

/// A proven-present view of one map: its definition, its walk grid, and its
/// spawns joined to their monster definitions. Minted only by the [`Atlas`](super::Atlas)
/// from resolved state — there is no public fabricating constructor — so its
/// walk grid and spawns are total, never `Option`.
#[derive(Debug, Clone, Copy)]
pub struct MapHandle<'a> {
    pub(super) definition: &'a MapDefinition,
    pub(super) walk_grid: &'a WalkGrid,
    pub(super) spawns: &'a [ResolvedSpawn],
}

impl<'a> MapHandle<'a> {
    /// The map's definition — its environment and, on Arena, its soccer pitch.
    #[must_use]
    pub fn definition(&self) -> &'a MapDefinition {
        self.definition
    }

    /// The map's walk grid. Total — presence was proven at parse.
    #[must_use]
    pub fn walk_grid(&self) -> &'a WalkGrid {
        self.walk_grid
    }

    /// The map's spawn entries, each already joined to its monster definition.
    pub fn spawns(&self) -> impl Iterator<Item = SpawnEntry<'a>> {
        self.spawns.iter().map(|resolved| SpawnEntry {
            spawn: &resolved.spawn,
            monster: &resolved.monster,
        })
    }
}
