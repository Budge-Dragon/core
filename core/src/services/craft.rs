//! The chaos-machine mix: one pure decision from the placed item multiset, the
//! zen balance, the [`Atlas`] recipe catalog, and injected RNG to a
//! [`MixOutcome`]. The recipe is inferred from the placed items by attempting
//! the catalog in descending authentic crafting-number order (the order the
//! Atlas retains); the first clean claim runs its family's rate → fee → roll →
//! dispositions. Every input instance reappears in exactly one outcome
//! position or is consumed by value inside this service — silent loss is
//! unrepresentable.
//!
//! Determinism rests on a fixed RNG draw order per family; a draw fires only
//! when its gate does (the `roll_dropped_item` precedent). Rates 0 and 100
//! consume no roll word. Draw order per family:
//!
//! | Family | Success draws (in order) | Fail draws (in order) |
//! |---|---|---|
//! | Chaos Weapon | roll · pick weapon · level 0–4 · luck · option-index · option · skill (if the definition has one) | roll · per sacrifice: level (skip +0) · 50% skill loss (if skilled, non-excellent) · 50% option decrement (if optioned) |
//! | First Wings | roll · pick wing · luck · option-index · option (no skill roll) | roll · chaos-weapon downgrade as above |
//! | Second Wings / Cape | roll · pick wing (cape: none) · luck · wing-option-index · wing-option · wing-bonus chance · bonus pick (if granted) | roll |
//! | Item Upgrade | roll only | roll only |
//! | Dinorant | roll · 30% first option · pick · 20% second option · pick (duplicate silently discarded) | roll |
//! | Fruits | roll · weighted level | roll |
//! | DS / BC Ticket | roll only | roll only |

use core::num::{NonZeroU8, NonZeroUsize};

use rand_core::RngCore;

use crate::components::collections::{EmptyCollection, OneOrMore};
use crate::components::item_instance::{
    AugmentSlot, CraftedAugment, DinorantOptionSet, Durability, ItemInstance, LuckRoll, RarityRoll,
    RolledNormalOption, SkillRoll,
};
use crate::components::item_options::{DinorantOption, SecondWingBonus};
use crate::components::item_ref::ItemRef;
use crate::components::levels::{EnhanceLevel, OptionLevel};
use crate::components::units::{CarriedZen, DebitOutcome, ItemLevel, Percent, Zen};
use crate::data::atlas::{Atlas, ResolvedOutput, ResolvedRecipe};
use crate::data::chaos_mixes::{
    ItemAtLevel, ItemLevelWindow, UpgradeTarget, WingEconomics, row_at,
};
use crate::data::item_definitions::{ItemDefinition, ItemKind, JewelKind};
use crate::events::craft::{Casualty, MixOutcome, RejectReason};
use crate::rng::{uniform_below, uniform_below_usize};
use crate::services::chance::{pick_one, roll_percent};
use crate::services::item_roll::eligible_normal_option;
use crate::services::item_rules::max_durability;
use crate::services::price::{buying_price, old_buying_price};
use crate::services::ratio::{nonzero, nonzero_u64, scale_ratio_u64};

// W-SRC: the chaos-weapon-family value economy — one floor division of the
// summed old NPC values by 20,000 zen per success point; the fee prices the
// final clamped rate at 10,000 zen per point.
const OLD_VALUE_ZEN_PER_POINT: u64 = 20_000;
const VALUE_FEE_ZEN_PER_POINT: u64 = 10_000;

// W-SRC: the 50% skill-loss and option-decrement chances of the downgrade.
const DOWNGRADE_STEP_PERCENT: u64 = 50;

// W-SRC: the chaos-weapon / first-wings bonus formulas off the final rate —
// luck at rate/5 + 4, the drawn option slot at rate/5 + 4·(i+1) granting
// option level 3−i, skill at rate/5 + 6.
const VALUE_LUCK_BONUS: u64 = 4;
const VALUE_SKILL_BONUS: u64 = 6;
const SACRIFICE_OPTION_STEPS: [(OptionLevel, u64); 3] = [
    (OptionLevel::L3, 1),
    (OptionLevel::L2, 2),
    (OptionLevel::L1, 3),
];

/// The chaos weapon's rolled plus-level span (+0..=+4), drawn uniformly.
const CHAOS_WEAPON_LEVELS: [EnhanceLevel; 5] = [
    EnhanceLevel::L0,
    EnhanceLevel::L1,
    EnhanceLevel::L2,
    EnhanceLevel::L3,
    EnhanceLevel::L4,
];

// W-SRC: the wing-option table — 20% at option level 1, 10% at 2, 4% at 3.
const WING_OPTION_STEPS: [(OptionLevel, u64); 3] = [
    (OptionLevel::L1, 20),
    (OptionLevel::L2, 10),
    (OptionLevel::L3, 4),
];

// W-SRC: second wings draw their bonus from the three non-Command values;
// the cape draws from all four.
const SECOND_WING_BONUS_POOL: [SecondWingBonus; 3] = [
    SecondWingBonus::MaxHealth,
    SecondWingBonus::MaxMana,
    SecondWingBonus::IgnoreDefenseChance,
];
const CAPE_BONUS_POOL: [SecondWingBonus; 4] = [
    SecondWingBonus::MaxHealth,
    SecondWingBonus::MaxMana,
    SecondWingBonus::IgnoreDefenseChance,
    SecondWingBonus::Command,
];

// W-SRC: the dinorant option chances — 30% for the first option, then 20% for
// a second uniform draw whose duplicate is silently discarded (K2, no
// re-roll).
const DINORANT_FIRST_OPTION_PERCENT: u64 = 30;
const DINORANT_SECOND_OPTION_PERCENT: u64 = 20;

// W-SRC: the fruit level weights — 30/25/20/20/5 over +0..=+4. The roll bound
// is DERIVED from the summed weights at compile time, never an assumed 100.
const FRUIT_LEVEL_WEIGHTS: [(u32, EnhanceLevel); 5] = [
    (30, EnhanceLevel::L0),
    (25, EnhanceLevel::L1),
    (20, EnhanceLevel::L2),
    (20, EnhanceLevel::L3),
    (5, EnhanceLevel::L4),
];
const FRUIT_WEIGHT_TOTAL: u32 = {
    let [(w0, _), (w1, _), (w2, _), (w3, _), (w4, _)] = FRUIT_LEVEL_WEIGHTS;
    w0 + w1 + w2 + w3 + w4
};

/// Resolves one chaos-machine mix: infers the recipe the placed items form
/// (descending crafting-number scan), computes the authoritative rate and fee,
/// charges the fee (never refunded), rolls once, and applies the family's
/// dispositions. `zen` is the character's balance; the outcome carries the new
/// balance. On [`RejectReason::InsufficientZen`] the items return in claim
/// order; on [`RejectReason::NoRecipeMatch`] in placed order.
#[must_use]
pub fn mix(
    placed: Vec<ItemInstance>,
    zen: CarriedZen,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> MixOutcome {
    let pool: Vec<PlacedItem<'_>> = placed
        .into_iter()
        .map(|instance| match atlas.item(instance.item) {
            Some(def) => PlacedItem::Resolved { instance, def },
            None => PlacedItem::Unknown { instance },
        })
        .collect();

    let matched = match scan(atlas, pool) {
        Ok(matched) => matched,
        Err(pool) => {
            return MixOutcome::Rejected {
                reason: RejectReason::NoRecipeMatch,
                items: pool.into_iter().map(PlacedItem::into_instance).collect(),
            };
        }
    };

    let rate = success_rate(&matched);
    let fee = attempt_fee(&matched, rate);
    let balance = match zen.debit(fee) {
        DebitOutcome::Debited { balance } => balance,
        DebitOutcome::Insufficient { .. } => {
            return MixOutcome::Rejected {
                reason: RejectReason::InsufficientZen,
                items: into_items(matched),
            };
        }
    };
    if roll_success(rate, rng) {
        succeed(matched, rate, fee, balance, rng)
    } else {
        fail(matched, fee, balance, rng)
    }
}

/// A placed instance with its definition resolved exactly once, or the real
/// "identity not in the catalog" value — claimable by no ingredient class, so
/// it is a machine-family leftover and a ticket-family extra.
enum PlacedItem<'a> {
    /// The instance and its resolved definition.
    Resolved {
        /// The placed instance.
        instance: ItemInstance,
        /// Its definition, resolved from the atlas.
        def: &'a ItemDefinition,
    },
    /// An instance whose identity resolves to nothing.
    Unknown {
        /// The placed instance.
        instance: ItemInstance,
    },
}

impl PlacedItem<'_> {
    fn into_instance(self) -> ItemInstance {
        match self {
            Self::Resolved { instance, .. } | Self::Unknown { instance } => instance,
        }
    }
}

/// A claimed instance carrying its resolved definition forward, so rate math
/// and dispositions never re-resolve a ref.
struct ResolvedItem<'a> {
    instance: ItemInstance,
    def: &'a ItemDefinition,
}

/// A claimed item tagged with its original window position, so a failed
/// attempt hands the pool back in placed order.
type Claim<'a> = (usize, ResolvedItem<'a>);

/// The matched recipe: claimed instances in typed roles plus the family facts
/// and joined outputs the dispositions read. A required role is
/// [`OneOrMore`], so its emptiness is unrepresentable; exact role counts
/// remain internal builder invariants proven by the claim scan.
enum MatchedRecipe<'a> {
    /// Chaos weapon: sacrifices downgrade on failure, jewels are destroyed.
    ChaosWeapon {
        sacrifices: OneOrMore<ResolvedItem<'a>>,
        chaos_jewels: OneOrMore<ResolvedItem<'a>>,
        bless_jewels: Vec<ResolvedItem<'a>>,
        soul_jewels: Vec<ResolvedItem<'a>>,
        weapons: &'a ResolvedOutput,
    },
    /// First wings: the chaos weapon downgrades on failure; extras are pure
    /// rate fuel, destroyed on both outcomes (K3).
    FirstWings {
        chaos_weapon: ResolvedItem<'a>,
        extras: Vec<ResolvedItem<'a>>,
        chaos_jewels: OneOrMore<ResolvedItem<'a>>,
        bless_jewels: Vec<ResolvedItem<'a>>,
        soul_jewels: Vec<ResolvedItem<'a>>,
        wings: &'a ResolvedOutput,
    },
    /// Second wings: everything is destroyed on failure.
    SecondWings {
        wing: ResolvedItem<'a>,
        feather: ResolvedItem<'a>,
        chaos_jewel: ResolvedItem<'a>,
        fodder: Vec<ResolvedItem<'a>>,
        economics: WingEconomics,
        wings: &'a ResolvedOutput,
    },
    /// Cape of Lord: as second wings with the +1 crest and the 4-value pool.
    CapeOfLord {
        wing: ResolvedItem<'a>,
        crest: ResolvedItem<'a>,
        chaos_jewel: ResolvedItem<'a>,
        fodder: Vec<ResolvedItem<'a>>,
        economics: WingEconomics,
        cape: &'a ResolvedOutput,
    },
    /// In-place upgrade: the placed item levels up on success and is destroyed
    /// (not downgraded) on failure.
    ItemUpgrade {
        target_item: ResolvedItem<'a>,
        chaos_jewel: ResolvedItem<'a>,
        bless_jewels: Vec<ResolvedItem<'a>>,
        soul_jewels: Vec<ResolvedItem<'a>>,
        target: UpgradeTarget,
        base_success_percent: Percent,
        fee_zen: Zen,
    },
    /// Dinorant: every horn proven at full durability by the claim gate.
    Dinorant {
        horns: OneOrMore<ResolvedItem<'a>>,
        chaos_jewel: ResolvedItem<'a>,
        success_percent: Percent,
        fee_zen: Zen,
        dinorant: &'a ResolvedOutput,
    },
    /// Fruits: catalyst and jewel are both destroyed on failure.
    Fruits {
        catalyst: ResolvedItem<'a>,
        chaos_jewel: ResolvedItem<'a>,
        success_percent: Percent,
        fee_zen: Zen,
        fruit: &'a ResolvedOutput,
    },
    /// Devil Square ticket: extras are ignored and returned on both outcomes.
    DevilSquareTicket {
        eye: ResolvedItem<'a>,
        key: ResolvedItem<'a>,
        chaos_jewel: ResolvedItem<'a>,
        extras: Vec<ItemInstance>,
        level: ItemLevel,
        invitation: &'a ResolvedOutput,
        fee_zen_by_level: [Zen; 7],
        success_percent_by_level: [Percent; 7],
    },
    /// Blood Castle ticket: the Devil Square shape over scroll and bone.
    BloodCastleTicket {
        scroll: ResolvedItem<'a>,
        bone: ResolvedItem<'a>,
        chaos_jewel: ResolvedItem<'a>,
        extras: Vec<ItemInstance>,
        level: ItemLevel,
        cloak: &'a ResolvedOutput,
        fee_zen_by_level: [Zen; 8],
        success_percent_by_level: [Percent; 8],
    },
}

// ---------------------------------------------------------------------------
// The claim scan (§4.1 / §4.2): first clean claim in catalog order wins.
// ---------------------------------------------------------------------------

/// Attempts the catalog in the Atlas's descending crafting-number order; the
/// first clean claim wins. A failed attempt hands the pool back untouched, in
/// placed order, for the next recipe.
fn scan<'a>(
    atlas: &'a Atlas,
    mut pool: Vec<PlacedItem<'a>>,
) -> Result<MatchedRecipe<'a>, Vec<PlacedItem<'a>>> {
    for recipe in atlas.chaos_recipes() {
        match try_claim(recipe, pool) {
            Ok(matched) => return Ok(matched),
            Err(returned) => pool = returned,
        }
    }
    Err(pool)
}

/// One recipe attempt over the pool: claim-all for the machine families,
/// claim-first for the tickets, family gates included.
fn try_claim<'a>(
    recipe: &'a ResolvedRecipe,
    pool: Vec<PlacedItem<'a>>,
) -> Result<MatchedRecipe<'a>, Vec<PlacedItem<'a>>> {
    match recipe {
        ResolvedRecipe::ChaosWeapon {
            sacrifice_levels,
            weapons,
        } => claim_chaos_weapon(*sacrifice_levels, weapons, pool),
        ResolvedRecipe::FirstWings {
            chaos_weapons,
            chaos_weapon_levels,
            extra_sacrifice_levels,
            wings,
        } => claim_first_wings(
            chaos_weapons,
            *chaos_weapon_levels,
            *extra_sacrifice_levels,
            wings,
            pool,
        ),
        ResolvedRecipe::SecondWings {
            first_wings,
            wing_levels,
            excellent_levels,
            feather,
            economics,
            wings,
        } => claim_wing_tier(first_wings, *wing_levels, *excellent_levels, *feather, pool).map(
            |claim| MatchedRecipe::SecondWings {
                wing: claim.wing,
                feather: claim.catalyst,
                chaos_jewel: claim.chaos_jewel,
                fodder: claim.fodder,
                economics: *economics,
                wings,
            },
        ),
        ResolvedRecipe::CapeOfLord {
            first_wings,
            wing_levels,
            excellent_levels,
            crest,
            economics,
            cape,
        } => claim_wing_tier(first_wings, *wing_levels, *excellent_levels, *crest, pool).map(
            |claim| MatchedRecipe::CapeOfLord {
                wing: claim.wing,
                crest: claim.catalyst,
                chaos_jewel: claim.chaos_jewel,
                fodder: claim.fodder,
                economics: *economics,
                cape,
            },
        ),
        ResolvedRecipe::ItemUpgrade {
            target,
            bless,
            soul,
            base_success_percent,
            fee_zen,
        } => claim_upgrade(*target, *bless, *soul, pool).map(|claim| MatchedRecipe::ItemUpgrade {
            target_item: claim.target_item,
            chaos_jewel: claim.chaos_jewel,
            bless_jewels: claim.bless_jewels,
            soul_jewels: claim.soul_jewels,
            target: *target,
            base_success_percent: *base_success_percent,
            fee_zen: *fee_zen,
        }),
        ResolvedRecipe::Dinorant {
            horn,
            horn_count,
            success_percent,
            fee_zen,
            dinorant,
        } => claim_dinorant(*horn, *horn_count, pool).map(|claim| MatchedRecipe::Dinorant {
            horns: claim.horns,
            chaos_jewel: claim.chaos_jewel,
            success_percent: *success_percent,
            fee_zen: *fee_zen,
            dinorant,
        }),
        ResolvedRecipe::Fruits {
            catalyst,
            success_percent,
            fee_zen,
            fruit,
        } => claim_fruits(*catalyst, pool).map(|claim| MatchedRecipe::Fruits {
            catalyst: claim.catalyst,
            chaos_jewel: claim.chaos_jewel,
            success_percent: *success_percent,
            fee_zen: *fee_zen,
            fruit,
        }),
        ResolvedRecipe::DevilSquareTicket {
            eye,
            key,
            invitation,
            fee_zen_by_level,
            success_percent_by_level,
        } => claim_ticket(*eye, *key, pool).map(|claim| MatchedRecipe::DevilSquareTicket {
            eye: claim.first,
            key: claim.second,
            chaos_jewel: claim.chaos_jewel,
            extras: claim.extras,
            level: claim.level,
            invitation,
            fee_zen_by_level: *fee_zen_by_level,
            success_percent_by_level: *success_percent_by_level,
        }),
        ResolvedRecipe::BloodCastleTicket {
            scroll,
            bone,
            cloak,
            fee_zen_by_level,
            success_percent_by_level,
        } => claim_ticket(*scroll, *bone, pool).map(|claim| MatchedRecipe::BloodCastleTicket {
            scroll: claim.first,
            bone: claim.second,
            chaos_jewel: claim.chaos_jewel,
            extras: claim.extras,
            level: claim.level,
            cloak,
            fee_zen_by_level: *fee_zen_by_level,
            success_percent_by_level: *success_percent_by_level,
        }),
    }
}

fn claim_chaos_weapon<'a>(
    window: ItemLevelWindow,
    weapons: &'a ResolvedOutput,
    pool: Vec<PlacedItem<'a>>,
) -> Result<MatchedRecipe<'a>, Vec<PlacedItem<'a>>> {
    let mut sacrifices = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut bless_jewels = Vec::new();
    let mut soul_jewels = Vec::new();
    let mut leftovers = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item) if option_sacrifice(&item, window) => sacrifices.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Chaos) => chaos_jewels.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Bless) => bless_jewels.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Soul) => soul_jewels.push((position, item)),
            Ok(item) => leftovers.push((position, placed_of(item))),
            Err(placed) => leftovers.push((position, placed)),
        }
    }
    if !leftovers.is_empty() {
        return Err(hand_back(
            vec![sacrifices, chaos_jewels, bless_jewels, soul_jewels],
            leftovers,
        ));
    }
    match (many(sacrifices), many(chaos_jewels)) {
        (Ok(sacrifices), Ok(chaos_jewels)) => Ok(MatchedRecipe::ChaosWeapon {
            sacrifices: strip_many(sacrifices),
            chaos_jewels: strip_many(chaos_jewels),
            bless_jewels: strip(bless_jewels),
            soul_jewels: strip(soul_jewels),
            weapons,
        }),
        (sacrifice_result, chaos_result) => Err(hand_back(
            vec![
                undo_many(sacrifice_result),
                undo_many(chaos_result),
                bless_jewels,
                soul_jewels,
            ],
            leftovers,
        )),
    }
}

fn claim_first_wings<'a>(
    chaos_weapons: &[ItemRef; 3],
    weapon_window: ItemLevelWindow,
    extra_window: ItemLevelWindow,
    wings: &'a ResolvedOutput,
    pool: Vec<PlacedItem<'a>>,
) -> Result<MatchedRecipe<'a>, Vec<PlacedItem<'a>>> {
    let mut weapon_bucket = Vec::new();
    let mut extras = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut bless_jewels = Vec::new();
    let mut soul_jewels = Vec::new();
    let mut leftovers = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item)
                if chaos_weapons.contains(&item.def.id)
                    && option_sacrifice(&item, weapon_window) =>
            {
                weapon_bucket.push((position, item));
            }
            Ok(item) if option_sacrifice(&item, extra_window) => extras.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Chaos) => chaos_jewels.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Bless) => bless_jewels.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Soul) => soul_jewels.push((position, item)),
            Ok(item) => leftovers.push((position, placed_of(item))),
            Err(placed) => leftovers.push((position, placed)),
        }
    }
    if !leftovers.is_empty() {
        return Err(hand_back(
            vec![
                weapon_bucket,
                extras,
                chaos_jewels,
                bless_jewels,
                soul_jewels,
            ],
            leftovers,
        ));
    }
    match (one(weapon_bucket), many(chaos_jewels)) {
        (Ok((_, chaos_weapon)), Ok(chaos_jewels)) => Ok(MatchedRecipe::FirstWings {
            chaos_weapon,
            extras: strip(extras),
            chaos_jewels: strip_many(chaos_jewels),
            bless_jewels: strip(bless_jewels),
            soul_jewels: strip(soul_jewels),
            wings,
        }),
        (weapon_result, chaos_result) => Err(hand_back(
            vec![
                undo(weapon_result),
                undo_many(chaos_result),
                extras,
                bless_jewels,
                soul_jewels,
            ],
            leftovers,
        )),
    }
}

/// The shared second-wings / cape claim result.
struct WingTierClaim<'a> {
    wing: ResolvedItem<'a>,
    catalyst: ResolvedItem<'a>,
    chaos_jewel: ResolvedItem<'a>,
    fodder: Vec<ResolvedItem<'a>>,
}

fn claim_wing_tier<'a>(
    accepted_wings: &[ItemRef; 3],
    wing_window: ItemLevelWindow,
    fodder_window: ItemLevelWindow,
    catalyst: ItemAtLevel,
    pool: Vec<PlacedItem<'a>>,
) -> Result<WingTierClaim<'a>, Vec<PlacedItem<'a>>> {
    let mut wing_bucket = Vec::new();
    let mut catalyst_bucket = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut fodder = Vec::new();
    let mut leftovers = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item)
                if accepted_wings.contains(&item.def.id)
                    && wing_window.contains(item.instance.level) =>
            {
                wing_bucket.push((position, item));
            }
            Ok(item) if item.def.id == catalyst.item && item.instance.level == catalyst.level => {
                catalyst_bucket.push((position, item));
            }
            Ok(item) if is_jewel(item.def, JewelKind::Chaos) => chaos_jewels.push((position, item)),
            Ok(item) if excellent_fodder(&item, fodder_window) => fodder.push((position, item)),
            Ok(item) => leftovers.push((position, placed_of(item))),
            Err(placed) => leftovers.push((position, placed)),
        }
    }
    if !leftovers.is_empty() {
        return Err(hand_back(
            vec![wing_bucket, catalyst_bucket, chaos_jewels, fodder],
            leftovers,
        ));
    }
    match (one(wing_bucket), one(catalyst_bucket), one(chaos_jewels)) {
        (Ok((_, wing)), Ok((_, catalyst)), Ok((_, chaos_jewel))) => Ok(WingTierClaim {
            wing,
            catalyst,
            chaos_jewel,
            fodder: strip(fodder),
        }),
        (wing_result, catalyst_result, chaos_result) => Err(hand_back(
            vec![
                undo(wing_result),
                undo(catalyst_result),
                undo(chaos_result),
                fodder,
            ],
            leftovers,
        )),
    }
}

/// The item-upgrade claim result.
struct UpgradeClaim<'a> {
    target_item: ResolvedItem<'a>,
    chaos_jewel: ResolvedItem<'a>,
    bless_jewels: Vec<ResolvedItem<'a>>,
    soul_jewels: Vec<ResolvedItem<'a>>,
}

fn claim_upgrade(
    target: UpgradeTarget,
    bless_required: NonZeroU8,
    soul_required: NonZeroU8,
    pool: Vec<PlacedItem<'_>>,
) -> Result<UpgradeClaim<'_>, Vec<PlacedItem<'_>>> {
    let source_level = upgrade_source_level(target);
    let mut target_bucket = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut bless_jewels = Vec::new();
    let mut soul_jewels = Vec::new();
    let mut leftovers = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item) if item.instance.level == source_level => {
                target_bucket.push((position, item));
            }
            Ok(item) if is_jewel(item.def, JewelKind::Chaos) => chaos_jewels.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Bless) => bless_jewels.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Soul) => soul_jewels.push((position, item)),
            Ok(item) => leftovers.push((position, placed_of(item))),
            Err(placed) => leftovers.push((position, placed)),
        }
    }
    if bless_jewels.len() != usize::from(bless_required.get())
        || soul_jewels.len() != usize::from(soul_required.get())
        || !leftovers.is_empty()
    {
        return Err(hand_back(
            vec![target_bucket, chaos_jewels, bless_jewels, soul_jewels],
            leftovers,
        ));
    }
    match (one(target_bucket), one(chaos_jewels)) {
        (Ok((_, target_item)), Ok((_, chaos_jewel))) => Ok(UpgradeClaim {
            target_item,
            chaos_jewel,
            bless_jewels: strip(bless_jewels),
            soul_jewels: strip(soul_jewels),
        }),
        (target_result, chaos_result) => Err(hand_back(
            vec![
                undo(target_result),
                undo(chaos_result),
                bless_jewels,
                soul_jewels,
            ],
            leftovers,
        )),
    }
}

/// The dinorant claim result.
struct DinorantClaim<'a> {
    horns: OneOrMore<ResolvedItem<'a>>,
    chaos_jewel: ResolvedItem<'a>,
}

fn claim_dinorant(
    horn: ItemRef,
    horn_count: NonZeroU8,
    pool: Vec<PlacedItem<'_>>,
) -> Result<DinorantClaim<'_>, Vec<PlacedItem<'_>>> {
    let mut horns = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut leftovers = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item) if item.def.id == horn => horns.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Chaos) => chaos_jewels.push((position, item)),
            Ok(item) => leftovers.push((position, placed_of(item))),
            Err(placed) => leftovers.push((position, placed)),
        }
    }
    if !leftovers.is_empty() {
        return Err(hand_back(vec![horns, chaos_jewels], leftovers));
    }
    // The full-durability gate is part of the attempt: a worn horn fails the
    // recipe before any fee.
    match (many(horns), one(chaos_jewels)) {
        (Ok(horns), Ok((_, chaos_jewel)))
            if horns.count() == NonZeroUsize::from(horn_count)
                && horns
                    .iter()
                    .all(|(_, item)| at_full_durability(&item.instance)) =>
        {
            Ok(DinorantClaim {
                horns: strip_many(horns),
                chaos_jewel,
            })
        }
        (horns_result, chaos_result) => Err(hand_back(
            vec![undo_many(horns_result), undo(chaos_result)],
            leftovers,
        )),
    }
}

/// The fruits claim result.
struct FruitsClaim<'a> {
    catalyst: ResolvedItem<'a>,
    chaos_jewel: ResolvedItem<'a>,
}

fn claim_fruits(
    catalyst: ItemRef,
    pool: Vec<PlacedItem<'_>>,
) -> Result<FruitsClaim<'_>, Vec<PlacedItem<'_>>> {
    let mut catalysts = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut leftovers = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item) if item.def.id == catalyst => catalysts.push((position, item)),
            Ok(item) if is_jewel(item.def, JewelKind::Chaos) => chaos_jewels.push((position, item)),
            Ok(item) => leftovers.push((position, placed_of(item))),
            Err(placed) => leftovers.push((position, placed)),
        }
    }
    if !leftovers.is_empty() {
        return Err(hand_back(vec![catalysts, chaos_jewels], leftovers));
    }
    match (one(catalysts), one(chaos_jewels)) {
        (Ok((_, catalyst)), Ok((_, chaos_jewel))) => Ok(FruitsClaim {
            catalyst,
            chaos_jewel,
        }),
        (catalyst_result, chaos_result) => Err(hand_back(
            vec![undo(catalyst_result), undo(chaos_result)],
            leftovers,
        )),
    }
}

/// The shared devil-square / blood-castle claim result.
struct TicketClaim<'a> {
    first: ResolvedItem<'a>,
    second: ResolvedItem<'a>,
    chaos_jewel: ResolvedItem<'a>,
    extras: Vec<ItemInstance>,
    level: ItemLevel,
}

/// The ticket claim-first rule: each named ingredient claims the first placed
/// match (a duplicate beyond the first is simply not claimed); unclaimed items
/// are permitted extras, returned untouched. The equal-level gate is part of
/// the attempt.
fn claim_ticket(
    first_ref: ItemRef,
    second_ref: ItemRef,
    pool: Vec<PlacedItem<'_>>,
) -> Result<TicketClaim<'_>, Vec<PlacedItem<'_>>> {
    let mut firsts = Vec::new();
    let mut seconds = Vec::new();
    let mut chaos_jewels = Vec::new();
    let mut extras = Vec::new();
    for (position, placed) in pool.into_iter().enumerate() {
        match resolved(placed) {
            Ok(item) if firsts.is_empty() && item.def.id == first_ref => {
                firsts.push((position, item));
            }
            Ok(item) if seconds.is_empty() && item.def.id == second_ref => {
                seconds.push((position, item));
            }
            Ok(item) if chaos_jewels.is_empty() && is_jewel(item.def, JewelKind::Chaos) => {
                chaos_jewels.push((position, item));
            }
            Ok(item) => extras.push((position, placed_of(item))),
            Err(placed) => extras.push((position, placed)),
        }
    }
    match (one(firsts), one(seconds), one(chaos_jewels)) {
        (Ok((_, first)), Ok((_, second)), Ok((_, chaos_jewel)))
            if first.instance.level == second.instance.level =>
        {
            let level = first.instance.level;
            Ok(TicketClaim {
                first,
                second,
                chaos_jewel,
                extras: extras
                    .into_iter()
                    .map(|(_, placed)| placed.into_instance())
                    .collect(),
                level,
            })
        }
        (first_result, second_result, chaos_result) => Err(hand_back(
            vec![undo(first_result), undo(second_result), undo(chaos_result)],
            extras,
        )),
    }
}

// ---------------------------------------------------------------------------
// Claim machinery: total moves, placed-order restoration on failure.
// ---------------------------------------------------------------------------

/// Splits a placed item into its resolved pair, or hands it back whole.
fn resolved(placed: PlacedItem<'_>) -> Result<ResolvedItem<'_>, PlacedItem<'_>> {
    match placed {
        PlacedItem::Resolved { instance, def } => Ok(ResolvedItem { instance, def }),
        PlacedItem::Unknown { .. } => Err(placed),
    }
}

/// Re-wraps a claimed item as a pool entry.
fn placed_of(item: ResolvedItem<'_>) -> PlacedItem<'_> {
    PlacedItem::Resolved {
        instance: item.instance,
        def: item.def,
    }
}

/// Restores the pool from claim buckets and leftovers, sorted back into placed
/// order — a failed attempt hands every item back untouched.
fn hand_back<'a>(
    buckets: Vec<Vec<Claim<'a>>>,
    leftovers: Vec<(usize, PlacedItem<'a>)>,
) -> Vec<PlacedItem<'a>> {
    let mut entries = leftovers;
    for bucket in buckets {
        entries.extend(
            bucket
                .into_iter()
                .map(|(position, item)| (position, placed_of(item))),
        );
    }
    entries.sort_by_key(|(position, _)| *position);
    entries.into_iter().map(|(_, placed)| placed).collect()
}

/// Drops the position tags off a claimed bucket, keeping placed order.
fn strip(bucket: Vec<Claim<'_>>) -> Vec<ResolvedItem<'_>> {
    bucket.into_iter().map(|(_, item)| item).collect()
}

/// The exactly-one extraction: `Ok` the single claim, `Err` the bucket back
/// untouched (zero or several claims — the claim-all bound failed).
fn one(bucket: Vec<Claim<'_>>) -> Result<Claim<'_>, Vec<Claim<'_>>> {
    match <[Claim<'_>; 1]>::try_from(bucket) {
        Ok([claim]) => Ok(claim),
        Err(bucket) => Err(bucket),
    }
}

/// The at-least-one extraction: `Ok` the bucket as a non-empty list, `Err` the
/// (empty) bucket back for the hand-back path.
fn many(bucket: Vec<Claim<'_>>) -> Result<OneOrMore<Claim<'_>>, Vec<Claim<'_>>> {
    match OneOrMore::new(bucket) {
        Ok(claims) => Ok(claims),
        Err(EmptyCollection) => Err(Vec::new()),
    }
}

/// Folds an at-least-one extraction back into a bucket for the hand-back path.
fn undo_many<'a>(result: Result<OneOrMore<Claim<'a>>, Vec<Claim<'a>>>) -> Vec<Claim<'a>> {
    match result {
        Ok(claims) => Vec::from(claims),
        Err(bucket) => bucket,
    }
}

/// Drops the position tags off a non-empty claimed bucket, keeping placed
/// order.
fn strip_many(bucket: OneOrMore<Claim<'_>>) -> OneOrMore<ResolvedItem<'_>> {
    bucket.map(|(_, item)| item)
}

/// Folds an exactly-one extraction back into a bucket for the hand-back path.
fn undo<'a>(result: Result<Claim<'a>, Vec<Claim<'a>>>) -> Vec<Claim<'a>> {
    match result {
        Ok(claim) => vec![claim],
        Err(bucket) => bucket,
    }
}

/// Every claimed instance, in claim order — the insufficient-zen return.
fn into_items(matched: MatchedRecipe<'_>) -> Vec<ItemInstance> {
    match matched {
        MatchedRecipe::ChaosWeapon {
            sacrifices,
            chaos_jewels,
            bless_jewels,
            soul_jewels,
            weapons: _,
        } => instances(sacrifices)
            .chain(instances(chaos_jewels))
            .chain(instances(bless_jewels))
            .chain(instances(soul_jewels))
            .collect(),
        MatchedRecipe::FirstWings {
            chaos_weapon,
            extras,
            chaos_jewels,
            bless_jewels,
            soul_jewels,
            wings: _,
        } => core::iter::once(chaos_weapon.instance)
            .chain(instances(extras))
            .chain(instances(chaos_jewels))
            .chain(instances(bless_jewels))
            .chain(instances(soul_jewels))
            .collect(),
        MatchedRecipe::SecondWings {
            wing,
            feather,
            chaos_jewel,
            fodder,
            ..
        } => core::iter::once(wing.instance)
            .chain(core::iter::once(feather.instance))
            .chain(core::iter::once(chaos_jewel.instance))
            .chain(instances(fodder))
            .collect(),
        MatchedRecipe::CapeOfLord {
            wing,
            crest,
            chaos_jewel,
            fodder,
            ..
        } => core::iter::once(wing.instance)
            .chain(core::iter::once(crest.instance))
            .chain(core::iter::once(chaos_jewel.instance))
            .chain(instances(fodder))
            .collect(),
        MatchedRecipe::ItemUpgrade {
            target_item,
            chaos_jewel,
            bless_jewels,
            soul_jewels,
            ..
        } => core::iter::once(target_item.instance)
            .chain(core::iter::once(chaos_jewel.instance))
            .chain(instances(bless_jewels))
            .chain(instances(soul_jewels))
            .collect(),
        MatchedRecipe::Dinorant {
            horns, chaos_jewel, ..
        } => instances(horns)
            .chain(core::iter::once(chaos_jewel.instance))
            .collect(),
        MatchedRecipe::Fruits {
            catalyst,
            chaos_jewel,
            ..
        } => vec![catalyst.instance, chaos_jewel.instance],
        MatchedRecipe::DevilSquareTicket {
            eye,
            key,
            chaos_jewel,
            extras,
            ..
        } => core::iter::once(eye.instance)
            .chain(core::iter::once(key.instance))
            .chain(core::iter::once(chaos_jewel.instance))
            .chain(extras)
            .collect(),
        MatchedRecipe::BloodCastleTicket {
            scroll,
            bone,
            chaos_jewel,
            extras,
            ..
        } => core::iter::once(scroll.instance)
            .chain(core::iter::once(bone.instance))
            .chain(core::iter::once(chaos_jewel.instance))
            .chain(extras)
            .collect(),
    }
}

/// Consumes a role bucket into its bare instances.
fn instances<'a>(
    items: impl IntoIterator<Item = ResolvedItem<'a>> + 'a,
) -> impl Iterator<Item = ItemInstance> + 'a {
    items.into_iter().map(|item| item.instance)
}

// ---------------------------------------------------------------------------
// Claim predicates.
// ---------------------------------------------------------------------------

/// The value-family sacrifice predicate: in the level window and carrying a
/// normal option (any level).
fn option_sacrifice(item: &ResolvedItem<'_>, window: ItemLevelWindow) -> bool {
    window.contains(item.instance.level) && item.instance.normal_option.is_some()
}

/// The wing-tier fodder predicate: an excellent roll in the level window.
fn excellent_fodder(item: &ResolvedItem<'_>, window: ItemLevelWindow) -> bool {
    window.contains(item.instance.level)
        && matches!(item.instance.roll, RarityRoll::Excellent { .. })
}

/// Whether a definition is the named jewel kind.
fn is_jewel(def: &ItemDefinition, kind: JewelKind) -> bool {
    matches!(def.kind, ItemKind::Jewel { jewel } if jewel == kind)
}

/// The dinorant horn gate: an unworn gauge.
fn at_full_durability(instance: &ItemInstance) -> bool {
    instance.durability.current() == instance.durability.max()
}

/// The placed level an upgrade target must sit at: exactly `target − 1`.
fn upgrade_source_level(target: UpgradeTarget) -> ItemLevel {
    match target {
        UpgradeTarget::PlusTen => ItemLevel::from(EnhanceLevel::L9),
        UpgradeTarget::PlusEleven => ItemLevel::from(EnhanceLevel::L10),
    }
}

// ---------------------------------------------------------------------------
// Rate → fee → roll (§4.3).
// ---------------------------------------------------------------------------

/// The family success rate in saturating unsigned integer math, family-capped
/// and clamped to 100 (D2 — never wraps).
fn success_rate(matched: &MatchedRecipe<'_>) -> Percent {
    match matched {
        MatchedRecipe::ChaosWeapon {
            sacrifices,
            chaos_jewels,
            bless_jewels,
            soul_jewels,
            weapons: _,
        } => old_value_rate(
            sacrifices
                .iter()
                .chain(chaos_jewels)
                .chain(bless_jewels)
                .chain(soul_jewels),
        ),
        MatchedRecipe::FirstWings {
            chaos_weapon,
            extras,
            chaos_jewels,
            bless_jewels,
            soul_jewels,
            wings: _,
        } => old_value_rate(
            core::iter::once(chaos_weapon)
                .chain(extras)
                .chain(chaos_jewels)
                .chain(bless_jewels)
                .chain(soul_jewels),
        ),
        MatchedRecipe::SecondWings {
            wing,
            fodder,
            economics,
            ..
        }
        | MatchedRecipe::CapeOfLord {
            wing,
            fodder,
            economics,
            ..
        } => wing_rate(wing, fodder, *economics),
        MatchedRecipe::ItemUpgrade {
            target_item,
            base_success_percent,
            ..
        } => {
            // W-SRC: +25 rate points when the placed item carries luck.
            let luck_bonus = match target_item.instance.luck {
                LuckRoll::Lucky => 25,
                LuckRoll::Plain => 0,
            };
            Percent::clamped(u64::from(base_success_percent.points()).saturating_add(luck_bonus))
        }
        MatchedRecipe::Dinorant {
            success_percent, ..
        }
        | MatchedRecipe::Fruits {
            success_percent, ..
        } => *success_percent,
        MatchedRecipe::DevilSquareTicket {
            success_percent_by_level,
            level,
            ..
        } => {
            let [first, rest @ ..] = success_percent_by_level;
            row_at(*first, rest, ticket_row_index(*level))
        }
        MatchedRecipe::BloodCastleTicket {
            success_percent_by_level,
            level,
            ..
        } => {
            let [first, rest @ ..] = success_percent_by_level;
            row_at(*first, rest, ticket_row_index(*level))
        }
    }
}

/// The attempt fee, priced off the final clamped rate where the family is
/// value-driven, verbatim from the record everywhere else.
fn attempt_fee(matched: &MatchedRecipe<'_>, rate: Percent) -> Zen {
    match matched {
        MatchedRecipe::ChaosWeapon { .. } | MatchedRecipe::FirstWings { .. } => {
            Zen(VALUE_FEE_ZEN_PER_POINT.saturating_mul(u64::from(rate.points())))
        }
        MatchedRecipe::SecondWings { economics, .. }
        | MatchedRecipe::CapeOfLord { economics, .. } => economics.fee_zen,
        MatchedRecipe::ItemUpgrade { fee_zen, .. }
        | MatchedRecipe::Dinorant { fee_zen, .. }
        | MatchedRecipe::Fruits { fee_zen, .. } => *fee_zen,
        MatchedRecipe::DevilSquareTicket {
            fee_zen_by_level,
            level,
            ..
        } => {
            let [first, rest @ ..] = fee_zen_by_level;
            row_at(*first, rest, ticket_row_index(*level))
        }
        MatchedRecipe::BloodCastleTicket {
            fee_zen_by_level,
            level,
            ..
        } => {
            let [first, rest @ ..] = fee_zen_by_level;
            row_at(*first, rest, ticket_row_index(*level))
        }
    }
}

/// The 0-based ticket-row key: rows are level 1..=N, so a level-0 pair reads
/// the level-1 row (K1) and a level past the table saturates to its last row.
fn ticket_row_index(level: ItemLevel) -> usize {
    usize::from(level.get()).saturating_sub(1)
}

/// The chaos-weapon-family rate: one floor division of the summed OLD values.
fn old_value_rate<'a, 'b>(items: impl Iterator<Item = &'b ResolvedItem<'a>>) -> Percent
where
    'a: 'b,
{
    let total = items.fold(0u64, |sum, item| {
        sum.saturating_add(old_buying_price(item.def, &item.instance).0)
    });
    Percent::clamped(scale_ratio_u64(
        total,
        1,
        nonzero_u64(OLD_VALUE_ZEN_PER_POINT),
    ))
}

/// The wing-tier rate: `wing value / wing rate` plus `fodder value sum /
/// fodder rate`, one floor division per term as the record states them, then
/// the record's cap.
fn wing_rate(
    wing: &ResolvedItem<'_>,
    fodder: &[ResolvedItem<'_>],
    economics: WingEconomics,
) -> Percent {
    let wing_points = scale_ratio_u64(
        buying_price(wing.def, &wing.instance).0,
        1,
        nonzero_u64(economics.wing_value_zen_per_percent.0),
    );
    let fodder_value = fodder.iter().fold(0u64, |sum, item| {
        sum.saturating_add(buying_price(item.def, &item.instance).0)
    });
    let fodder_points = scale_ratio_u64(
        fodder_value,
        1,
        nonzero_u64(economics.excellent_value_zen_per_percent.0),
    );
    let capped = wing_points
        .saturating_add(fodder_points)
        .min(u64::from(economics.max_success_percent.points()));
    Percent::clamped(capped)
}

/// One success roll. Rate 0 never succeeds and rate 100 always does, each
/// consuming no random word (D1); mid rates spend exactly one word through
/// [`roll_percent`].
fn roll_success(rate: Percent, rng: &mut impl RngCore) -> bool {
    if rate.points() == 0 {
        return false;
    }
    if rate.points() >= Percent::DENOMINATOR {
        return true;
    }
    roll_percent(rate, rng)
}

// ---------------------------------------------------------------------------
// Success dispositions and result creation (§5).
// ---------------------------------------------------------------------------

fn succeed(
    matched: MatchedRecipe<'_>,
    rate: Percent,
    fee: Zen,
    zen: CarriedZen,
    rng: &mut impl RngCore,
) -> MixOutcome {
    let (created, returned) = match matched {
        // Sacrifices and jewels are consumed by value.
        MatchedRecipe::ChaosWeapon { weapons, .. } => {
            (create_chaos_weapon(weapons, rate, rng), Vec::new())
        }
        // The chaos weapon, extras (K3), and jewels are consumed by value.
        MatchedRecipe::FirstWings { wings, .. } => {
            (create_first_wing(wings, rate, rng), Vec::new())
        }
        MatchedRecipe::SecondWings {
            wings, economics, ..
        } => {
            let [first, rest @ ..] = &SECOND_WING_BONUS_POOL;
            (
                create_crafted_wing(wings, economics, *first, rest, rng),
                Vec::new(),
            )
        }
        MatchedRecipe::CapeOfLord {
            cape, economics, ..
        } => {
            let [first, rest @ ..] = &CAPE_BONUS_POOL;
            (
                create_crafted_wing(cape, economics, *first, rest, rng),
                Vec::new(),
            )
        }
        MatchedRecipe::ItemUpgrade {
            target_item,
            target,
            ..
        } => (upgrade_in_place(target_item, target), Vec::new()),
        MatchedRecipe::Dinorant { dinorant, .. } => (create_dinorant(dinorant, rng), Vec::new()),
        MatchedRecipe::Fruits { fruit, .. } => (create_fruit(fruit, rng), Vec::new()),
        MatchedRecipe::DevilSquareTicket {
            invitation,
            level,
            extras,
            ..
        } => (create_ticket(invitation, level, rng), extras),
        MatchedRecipe::BloodCastleTicket {
            cloak,
            level,
            extras,
            ..
        } => (create_ticket(cloak, level, rng), extras),
    };
    MixOutcome::Success {
        fee,
        zen,
        created,
        returned,
    }
}

/// One catalog output: a `Choice` spends one uniform word to pick; a `Single`
/// is deterministic and spends none.
fn pick_output<'a>(output: &'a ResolvedOutput, rng: &mut impl RngCore) -> &'a ItemDefinition {
    match output {
        ResolvedOutput::Choice(candidates) => pick_one(candidates, rng),
        ResolvedOutput::Single(def) => def,
    }
}

fn create_chaos_weapon(
    output: &ResolvedOutput,
    rate: Percent,
    rng: &mut impl RngCore,
) -> ItemInstance {
    let def = pick_output(output, rng);
    let enhance = {
        let [first, rest @ ..] = &CHAOS_WEAPON_LEVELS;
        row_at(*first, rest, draw_index(CHAOS_WEAPON_LEVELS.len(), rng))
    };
    let (luck, normal_option, skill) = value_family_bonuses(def, rate, rng);
    fresh_instance(def, enhance, luck, normal_option, skill)
}

fn create_first_wing(
    output: &ResolvedOutput,
    rate: Percent,
    rng: &mut impl RngCore,
) -> ItemInstance {
    let def = pick_output(output, rng);
    let (luck, normal_option, skill) = value_family_bonuses(def, rate, rng);
    fresh_instance(def, EnhanceLevel::L0, luck, normal_option, skill)
}

/// The chaos-weapon / first-wings bonus rolls off the final rate, in draw
/// order: luck, the option pair (index then chance — only when the kind has an
/// eligible option), then skill (only when the definition carries one).
fn value_family_bonuses(
    def: &ItemDefinition,
    rate: Percent,
    rng: &mut impl RngCore,
) -> (LuckRoll, Option<RolledNormalOption>, SkillRoll) {
    let fifth = u64::from(rate.points()) / 5;
    let luck = if roll_percent(
        Percent::clamped(fifth.saturating_add(VALUE_LUCK_BONUS)),
        rng,
    ) {
        LuckRoll::Lucky
    } else {
        LuckRoll::Plain
    };
    let normal_option = match eligible_normal_option(&def.kind) {
        Some(option) => {
            let (level, step) = {
                let [first, rest @ ..] = &SACRIFICE_OPTION_STEPS;
                row_at(*first, rest, draw_index(SACRIFICE_OPTION_STEPS.len(), rng))
            };
            let chance = Percent::clamped(fifth.saturating_add(4u64.saturating_mul(step)));
            if roll_percent(chance, rng) {
                Some(RolledNormalOption { option, level })
            } else {
                None
            }
        }
        None => None,
    };
    let skill = match def.kind.skill() {
        Some(_) => {
            if roll_percent(
                Percent::clamped(fifth.saturating_add(VALUE_SKILL_BONUS)),
                rng,
            ) {
                SkillRoll::WithSkill
            } else {
                SkillRoll::NoSkill
            }
        }
        None => SkillRoll::NoSkill,
    };
    (luck, normal_option, skill)
}

/// The shared second-wings / cape creation: luck and the wing-option pair from
/// the record's economics, then at most one wing bonus from the family's pool.
fn create_crafted_wing(
    output: &ResolvedOutput,
    economics: WingEconomics,
    pool_first: SecondWingBonus,
    pool_rest: &[SecondWingBonus],
    rng: &mut impl RngCore,
) -> ItemInstance {
    let def = pick_output(output, rng);
    let luck = if roll_percent(economics.luck_chance_percent, rng) {
        LuckRoll::Lucky
    } else {
        LuckRoll::Plain
    };
    let normal_option = wing_option(def, rng);
    // The augment is derived from the definition's own augment slot — the same
    // fact reload re-proves through [`ItemInstance::reconcile`] — so a mint can
    // never carry an augment its definition forbids. Only a wing-bonus slot rolls
    // the pool; any other slot mints none.
    let augment = match def.kind.augment_slot() {
        AugmentSlot::WingBonus => {
            if roll_percent(economics.excellent_chance_percent, rng) {
                let target = draw_index(pool_rest.len().saturating_add(1), rng);
                CraftedAugment::WingBonus {
                    bonus: row_at(pool_first, pool_rest, target),
                }
            } else {
                CraftedAugment::None
            }
        }
        AugmentSlot::None | AugmentSlot::Dinorant => CraftedAugment::None,
    };
    let mut created = fresh_instance(
        def,
        EnhanceLevel::L0,
        luck,
        normal_option,
        SkillRoll::NoSkill,
    );
    created.augment = augment;
    created
}

/// The wing-option pair: one slot draw over the 20/10/4 table, then its
/// chance, granting the wing's own eligible option. No draw when the wing
/// lists no option.
fn wing_option(def: &ItemDefinition, rng: &mut impl RngCore) -> Option<RolledNormalOption> {
    match eligible_normal_option(&def.kind) {
        Some(option) => {
            let (level, chance) = {
                let [first, rest @ ..] = &WING_OPTION_STEPS;
                row_at(*first, rest, draw_index(WING_OPTION_STEPS.len(), rng))
            };
            if roll_percent(Percent::clamped(chance), rng) {
                Some(RolledNormalOption { option, level })
            } else {
                None
            }
        }
        None => None,
    }
}

/// The in-place upgrade: level to the target, durability rescaled at the new
/// level (D3). No new item is minted — the placed item IS the created item.
fn upgrade_in_place(item: ResolvedItem<'_>, target: UpgradeTarget) -> ItemInstance {
    let ResolvedItem { mut instance, def } = item;
    let upgraded = match target {
        UpgradeTarget::PlusTen => EnhanceLevel::L10,
        UpgradeTarget::PlusEleven => EnhanceLevel::L11,
    };
    instance.level = ItemLevel::from(upgraded);
    instance.durability = rescaled_durability(&instance, def);
    instance
}

fn create_dinorant(output: &ResolvedOutput, rng: &mut impl RngCore) -> ItemInstance {
    let def = pick_output(output, rng);
    // Always with skill (Fire Breath) — no draw.
    let mut created = fresh_instance(
        def,
        EnhanceLevel::L0,
        LuckRoll::Plain,
        None,
        SkillRoll::WithSkill,
    );
    // The augment is derived from the definition's own augment slot — the same
    // fact reload re-proves through [`ItemInstance::reconcile`] — so a mint can
    // never carry an augment its definition forbids. Only a dinorant slot rolls
    // the dinorant options; any other slot mints none.
    created.augment = match def.kind.augment_slot() {
        AugmentSlot::Dinorant => dinorant_augment(rng),
        AugmentSlot::None | AugmentSlot::WingBonus => CraftedAugment::None,
    };
    created
}

/// The dinorant option pair: 30% grants a first uniform option; if granted,
/// 20% draws a second uniform option whose duplicate is silently discarded
/// (K2 — the one-bit-per-slot set makes the discard structural).
fn dinorant_augment(rng: &mut impl RngCore) -> CraftedAugment {
    if !roll_percent(Percent::clamped(DINORANT_FIRST_OPTION_PERCENT), rng) {
        return CraftedAugment::None;
    }
    let first = draw_dinorant_option(rng);
    let second = if roll_percent(Percent::clamped(DINORANT_SECOND_OPTION_PERCENT), rng) {
        Some(draw_dinorant_option(rng))
    } else {
        None
    };
    CraftedAugment::Dinorant {
        options: DinorantOptionSet::with_first(first, second),
    }
}

fn draw_dinorant_option(rng: &mut impl RngCore) -> DinorantOption {
    let [first, rest @ ..] = &DinorantOptionSet::OPTIONS;
    row_at(
        *first,
        rest,
        draw_index(DinorantOptionSet::OPTIONS.len(), rng),
    )
}

fn create_fruit(output: &ResolvedOutput, rng: &mut impl RngCore) -> ItemInstance {
    let def = pick_output(output, rng);
    let level = fruit_level(rng);
    // The fruit is a flat-durability kind: the record's base count (1), no
    // wear curve.
    ItemInstance {
        item: def.id,
        level: ItemLevel::from(level),
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(def.durability),
        augment: CraftedAugment::None,
    }
}

/// The weighted fruit level: one draw over the DERIVED weight total, resolved
/// by a cumulative walk whose final bucket is held apart — no wildcard
/// fallback, no assumed 100 (C3).
fn fruit_level(rng: &mut impl RngCore) -> EnhanceLevel {
    let roll = uniform_below(nonzero(FRUIT_WEIGHT_TOTAL), rng);
    let [leading @ .., (_, last)] = &FRUIT_LEVEL_WEIGHTS;
    let mut cumulative = 0u32;
    for (weight, level) in leading {
        cumulative = cumulative.saturating_add(*weight);
        if roll < cumulative {
            return *level;
        }
    }
    *last
}

fn create_ticket(
    output: &ResolvedOutput,
    level: ItemLevel,
    rng: &mut impl RngCore,
) -> ItemInstance {
    let def = pick_output(output, rng);
    // Tickets are flat-durability kinds: the record's base count (1).
    ItemInstance {
        item: def.id,
        level,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(def.durability),
        augment: CraftedAugment::None,
    }
}

/// A freshly created wear-kind instance at a level: full durability on the
/// enhancement curve, normal rarity, no augment.
fn fresh_instance(
    def: &ItemDefinition,
    enhance: EnhanceLevel,
    luck: LuckRoll,
    normal_option: Option<RolledNormalOption>,
    skill: SkillRoll,
) -> ItemInstance {
    ItemInstance {
        item: def.id,
        level: ItemLevel::from(enhance),
        roll: RarityRoll::Normal,
        normal_option,
        luck,
        skill,
        durability: Durability::full(max_durability(
            def.durability,
            enhance,
            crate::components::item_quality::ItemRarity::Normal,
        )),
        augment: CraftedAugment::None,
    }
}

/// A uniform index below `len` — one word. Callers pass a fixed non-empty
/// pool, and the zero-length fold to a single index is the [`nonzero`] idiom.
fn draw_index(len: usize, rng: &mut impl RngCore) -> usize {
    let bound = NonZeroUsize::MIN.saturating_add(len.saturating_sub(1));
    uniform_below_usize(bound, rng)
}

// ---------------------------------------------------------------------------
// Fail dispositions (§5 per family; §4.4 downgrade).
// ---------------------------------------------------------------------------

fn fail(
    matched: MatchedRecipe<'_>,
    fee: Zen,
    zen: CarriedZen,
    rng: &mut impl RngCore,
) -> MixOutcome {
    let casualties = match matched {
        MatchedRecipe::ChaosWeapon {
            sacrifices,
            chaos_jewels,
            bless_jewels,
            soul_jewels,
            weapons: _,
        } => {
            let mut casualties: Vec<Casualty> = sacrifices
                .into_iter()
                .map(|item| Casualty::Downgraded {
                    item: downgrade(item, rng),
                })
                .collect();
            casualties.extend(destroyed(chaos_jewels));
            casualties.extend(destroyed(bless_jewels));
            casualties.extend(destroyed(soul_jewels));
            casualties
        }
        MatchedRecipe::FirstWings {
            chaos_weapon,
            extras,
            chaos_jewels,
            bless_jewels,
            soul_jewels,
            wings: _,
        } => {
            let mut casualties = vec![Casualty::Downgraded {
                item: downgrade(chaos_weapon, rng),
            }];
            // K3: extras are pure rate fuel — destroyed on failure too.
            casualties.extend(destroyed(extras));
            casualties.extend(destroyed(chaos_jewels));
            casualties.extend(destroyed(bless_jewels));
            casualties.extend(destroyed(soul_jewels));
            casualties
        }
        MatchedRecipe::SecondWings {
            wing,
            feather,
            chaos_jewel,
            fodder,
            ..
        } => {
            let mut casualties = destroyed(vec![wing, feather, chaos_jewel]).collect::<Vec<_>>();
            casualties.extend(destroyed(fodder));
            casualties
        }
        MatchedRecipe::CapeOfLord {
            wing,
            crest,
            chaos_jewel,
            fodder,
            ..
        } => {
            let mut casualties = destroyed(vec![wing, crest, chaos_jewel]).collect::<Vec<_>>();
            casualties.extend(destroyed(fodder));
            casualties
        }
        MatchedRecipe::ItemUpgrade {
            target_item,
            chaos_jewel,
            bless_jewels,
            soul_jewels,
            ..
        } => {
            // The placed item is destroyed, not downgraded.
            let mut casualties = destroyed(vec![target_item, chaos_jewel]).collect::<Vec<_>>();
            casualties.extend(destroyed(bless_jewels));
            casualties.extend(destroyed(soul_jewels));
            casualties
        }
        MatchedRecipe::Dinorant {
            horns, chaos_jewel, ..
        } => {
            let mut casualties = destroyed(horns).collect::<Vec<_>>();
            casualties.extend(destroyed(vec![chaos_jewel]));
            casualties
        }
        MatchedRecipe::Fruits {
            catalyst,
            chaos_jewel,
            ..
        } => destroyed(vec![catalyst, chaos_jewel]).collect(),
        MatchedRecipe::DevilSquareTicket {
            eye,
            key,
            chaos_jewel,
            extras,
            ..
        } => {
            let mut casualties = destroyed(vec![eye, key, chaos_jewel]).collect::<Vec<_>>();
            casualties.extend(returned(extras));
            casualties
        }
        MatchedRecipe::BloodCastleTicket {
            scroll,
            bone,
            chaos_jewel,
            extras,
            ..
        } => {
            let mut casualties = destroyed(vec![scroll, bone, chaos_jewel]).collect::<Vec<_>>();
            casualties.extend(returned(extras));
            casualties
        }
    };
    MixOutcome::Failed {
        fee,
        zen,
        casualties,
    }
}

/// Consumes a bucket by value, reporting each identity as destroyed.
fn destroyed<'a>(
    items: impl IntoIterator<Item = ResolvedItem<'a>> + 'a,
) -> impl Iterator<Item = Casualty> + 'a {
    items.into_iter().map(|item| Casualty::Destroyed {
        item: item.instance.item,
    })
}

/// Hands ticket extras back untouched on a failed mix.
fn returned(extras: Vec<ItemInstance>) -> impl Iterator<Item = Casualty> {
    extras.into_iter().map(|item| Casualty::Returned { item })
}

/// The §4.4 downgrade, in fixed draw order: level to a uniform value below the
/// old one (a +0 item stays +0, no draw); 50% skill loss when skilled and not
/// excellent (excellent items never lose skill); 50% normal-option decrement,
/// removing the option entirely at L1; then the durability rescale at the new
/// level (D3).
fn downgrade(item: ResolvedItem<'_>, rng: &mut impl RngCore) -> ItemInstance {
    let ResolvedItem { mut instance, def } = item;
    let old_level = u32::from(instance.level.get());
    if old_level > 0 {
        let drawn = uniform_below(nonzero(old_level), rng);
        instance.level = ItemLevel::clamped(u64::from(drawn));
    }
    let excellent = match &instance.roll {
        RarityRoll::Excellent { .. } => true,
        RarityRoll::Normal | RarityRoll::Ancient { .. } => false,
    };
    if instance.skill == SkillRoll::WithSkill
        && !excellent
        && roll_percent(Percent::clamped(DOWNGRADE_STEP_PERCENT), rng)
    {
        instance.skill = SkillRoll::NoSkill;
    }
    if let Some(rolled) = instance.normal_option {
        if roll_percent(Percent::clamped(DOWNGRADE_STEP_PERCENT), rng) {
            instance.normal_option = decremented_option(rolled);
        }
    }
    instance.durability = rescaled_durability(&instance, def);
    instance
}

/// One Jewel-of-Life level down; the option disappears entirely at L1.
fn decremented_option(rolled: RolledNormalOption) -> Option<RolledNormalOption> {
    match rolled.level {
        OptionLevel::L1 => None,
        OptionLevel::L2 => Some(RolledNormalOption {
            level: OptionLevel::L1,
            ..rolled
        }),
        OptionLevel::L3 => Some(RolledNormalOption {
            level: OptionLevel::L2,
            ..rolled
        }),
        OptionLevel::L4 => Some(RolledNormalOption {
            level: OptionLevel::L3,
            ..rolled
        }),
    }
}

/// The wear gauge at the instance's (already updated) level: the new maximum
/// from [`max_durability`] (the base count when the level is a box tier
/// outside the curve), the floor rescale (D3) owned by
/// [`Durability::rescaled`].
fn rescaled_durability(instance: &ItemInstance, def: &ItemDefinition) -> Durability {
    let new_max = match instance.level.enhance_level() {
        Some(enhance) => max_durability(def.durability, enhance, instance.roll.rarity()),
        None => def.durability,
    };
    instance.durability.rescaled(new_max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::class::ClassSet;
    use crate::components::item_instance::{AugmentSlot, ExcellentArmorSet, ExcellentOptions};
    use crate::components::item_options::ExcellentArmorOption;
    use crate::data::common::{Provenance, SourceVersion};
    use crate::data::item_definitions::{
        ItemPrice, PerLevelPrice, WeaponHandling, WearRequirements,
    };

    /// Deterministic `SplitMix64` for replayable tests.
    struct TestRng {
        state: u64,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }
    }

    impl RngCore for TestRng {
        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            for chunk in dst.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                    *slot = *byte;
                }
            }
        }
    }

    /// Wraps the test stream and counts the words drawn, so no-draw and
    /// exact-draw contracts are observable.
    struct CountingRng {
        inner: TestRng,
        words: u64,
    }

    impl CountingRng {
        fn new(seed: u64) -> Self {
            Self {
                inner: TestRng::new(seed),
                words: 0,
            }
        }
    }

    impl RngCore for CountingRng {
        fn next_u64(&mut self) -> u64 {
            self.words += 1;
            self.inner.next_u64()
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            self.inner.fill_bytes(dst);
        }
    }

    fn provenance() -> Provenance {
        Provenance {
            source_version: SourceVersion::V075,
            review: None,
        }
    }

    fn wear() -> WearRequirements {
        WearRequirements {
            level: 0,
            strength: 0,
            agility: 0,
            vitality: 0,
            energy: 0,
            command: 0,
        }
    }

    fn def(id: ItemRef, durability: u8, price: ItemPrice, kind: ItemKind) -> ItemDefinition {
        ItemDefinition {
            id,
            provenance: provenance(),
            width: 2,
            height: 1,
            drops_from_monsters: false,
            drop_level: 10,
            max_item_level: ItemLevel::new(15).unwrap(),
            durability,
            price,
            kind,
        }
    }

    /// A flat-priced material worth exactly `zen` at every level.
    fn material_def(number: u16, zen: u64) -> ItemDefinition {
        def(
            ItemRef { group: 13, number },
            1,
            ItemPrice::PerLevel {
                zen_by_level: PerLevelPrice::try_from(vec![Zen(zen)]).unwrap(),
            },
            ItemKind::MixMaterial,
        )
    }

    fn jewel_def(jewel: JewelKind) -> ItemDefinition {
        def(
            ItemRef {
                group: 14,
                number: 13,
            },
            1,
            ItemPrice::Fixed { zen: Zen(1) },
            ItemKind::Jewel { jewel },
        )
    }

    fn weapon_def(number: u16, skill: Option<crate::data::common::SkillNumber>) -> ItemDefinition {
        def(
            ItemRef { group: 0, number },
            20,
            ItemPrice::Formula,
            ItemKind::Weapon {
                handling: WeaponHandling::OneHanded,
                min_damage: 1,
                max_damage: 2,
                attack_speed: 20,
                skill,
                classes: ClassSet::NONE,
                wear: wear(),
            },
        )
    }

    fn wing_def(drop_level: u8) -> ItemDefinition {
        let mut wing = def(
            ItemRef {
                group: 12,
                number: 0,
            },
            200,
            ItemPrice::Formula,
            ItemKind::Wings {
                tier: crate::data::item_definitions::WingTier::First,
                defense: 10,
                absorb_percent: 12,
                damage_percent: 12,
                jol_options: vec![crate::components::item_options::NormalOption::HealthRecoveryPct],
                augment: AugmentSlot::None,
                classes: ClassSet::NONE,
                wear: wear(),
            },
        );
        wing.drop_level = drop_level;
        wing
    }

    fn instance(def: &ItemDefinition, level: u8) -> ItemInstance {
        ItemInstance {
            item: def.id,
            level: ItemLevel::new(level).unwrap(),
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(def.durability),
            augment: CraftedAugment::None,
        }
    }

    fn resolved_item(def: &ItemDefinition, level: u8) -> ResolvedItem<'_> {
        ResolvedItem {
            instance: instance(def, level),
            def,
        }
    }

    fn economics() -> WingEconomics {
        WingEconomics {
            fee_zen: Zen(5_000_000),
            max_success_percent: Percent::new(90).unwrap(),
            wing_value_zen_per_percent: Zen(4_000_000),
            excellent_value_zen_per_percent: Zen(40_000),
            luck_chance_percent: Percent::new(20).unwrap(),
            excellent_chance_percent: Percent::new(20).unwrap(),
        }
    }

    fn choice_output(defs: Vec<ItemDefinition>) -> ResolvedOutput {
        ResolvedOutput::Choice(OneOrMore::new(defs).unwrap())
    }

    fn chaos_weapon_bundle<'a>(
        sacrifices: Vec<ResolvedItem<'a>>,
        chaos_jewels: Vec<ResolvedItem<'a>>,
        bless_jewels: Vec<ResolvedItem<'a>>,
        weapons: &'a ResolvedOutput,
    ) -> MatchedRecipe<'a> {
        MatchedRecipe::ChaosWeapon {
            sacrifices: OneOrMore::new(sacrifices).unwrap(),
            chaos_jewels: OneOrMore::new(chaos_jewels).unwrap(),
            bless_jewels,
            soul_jewels: Vec::new(),
            weapons,
        }
    }

    // -- rate math ----------------------------------------------------------

    #[test]
    fn chaos_weapon_rate_is_one_pooled_floor_division_by_20_000() {
        let material = material_def(20, 30_000);
        let chaos = jewel_def(JewelKind::Chaos);
        let weapons = choice_output(vec![weapon_def(1, None)]);
        let bundle = chaos_weapon_bundle(
            vec![resolved_item(&material, 0), resolved_item(&material, 0)],
            vec![resolved_item(&chaos, 0)],
            Vec::new(),
            &weapons,
        );
        // Pooled: (30,000 + 30,000 + 40,000) / 20,000 = 5. Per-term floors
        // would give 1 + 1 + 2 = 4.
        assert_eq!(success_rate(&bundle), Percent::new(5).unwrap());
        // The fee prices the final rate at 10,000 per point.
        assert_eq!(attempt_fee(&bundle, success_rate(&bundle)), Zen(50_000));
    }

    #[test]
    fn chaos_weapon_rate_saturates_to_100_never_wrapping() {
        let material = material_def(20, 30_000);
        let chaos = jewel_def(JewelKind::Chaos);
        let bless = jewel_def(JewelKind::Bless);
        let weapons = choice_output(vec![weapon_def(1, None)]);
        let boosters: Vec<ResolvedItem<'_>> = (0..60).map(|_| resolved_item(&bless, 0)).collect();
        let bundle = chaos_weapon_bundle(
            vec![resolved_item(&material, 0)],
            vec![resolved_item(&chaos, 0)],
            boosters,
            &weapons,
        );
        // 30,000 + 40,000 + 60·100,000 = 6,070,000 → 303 points, clamped to
        // 100 (D2).
        assert_eq!(success_rate(&bundle), Percent::new(100).unwrap());
        assert_eq!(
            attempt_fee(&bundle, Percent::new(100).unwrap()),
            Zen(1_000_000)
        );
    }

    #[test]
    fn chaos_weapon_rate_floors_the_pooled_division() {
        let cheap = material_def(20, 19_900);
        let chaos = jewel_def(JewelKind::Chaos);
        let weapons = choice_output(vec![weapon_def(1, None)]);
        let bundle = chaos_weapon_bundle(
            vec![resolved_item(&cheap, 0)],
            vec![resolved_item(&chaos, 0)],
            Vec::new(),
            &weapons,
        );
        // (19,900 + 40,000) / 20,000 = 2.995 floors to 2, never rounds up.
        assert_eq!(success_rate(&bundle), Percent::new(2).unwrap());
        assert_eq!(attempt_fee(&bundle, success_rate(&bundle)), Zen(20_000));
    }

    #[test]
    fn old_jewel_values_feed_the_chaos_weapon_rate() {
        // Bless 100,000 and Soul 70,000 old values: (100k + 70k + 40k) / 20k = 10
        // with a worthless sacrifice.
        let material = material_def(20, 0);
        let chaos = jewel_def(JewelKind::Chaos);
        let bless = jewel_def(JewelKind::Bless);
        let soul = jewel_def(JewelKind::Soul);
        let weapons = choice_output(vec![weapon_def(1, None)]);
        let bundle = MatchedRecipe::ChaosWeapon {
            sacrifices: OneOrMore::with_head(resolved_item(&material, 0), Vec::new()),
            chaos_jewels: OneOrMore::with_head(resolved_item(&chaos, 0), Vec::new()),
            bless_jewels: vec![resolved_item(&bless, 0)],
            soul_jewels: vec![resolved_item(&soul, 0)],
            weapons: &weapons,
        };
        assert_eq!(success_rate(&bundle), Percent::new(10).unwrap());
    }

    #[test]
    fn second_wings_rate_is_one_floor_per_term_with_the_13_point_anchor() {
        let wing = wing_def(100);
        let feather = material_def(14, 180_000);
        let chaos = jewel_def(JewelKind::Chaos);
        let fodder_a = material_def(21, 50_000);
        let fodder_b = material_def(22, 30_000);
        let wings = choice_output(vec![wing_def(150)]);
        let bundle = MatchedRecipe::SecondWings {
            wing: resolved_item(&wing, 0),
            feather: resolved_item(&feather, 0),
            chaos_jewel: resolved_item(&chaos, 0),
            fodder: vec![resolved_item(&fodder_a, 0), resolved_item(&fodder_b, 0)],
            economics: economics(),
            wings: &wings,
        };
        // Wing term: 55,400,000 / 4,000,000 = 13 (the pinned anchor).
        // Fodder term: (50,000 + 30,000) / 40,000 = 2 — one division over the
        // summed fodder value, not per item.
        assert_eq!(success_rate(&bundle), Percent::new(15).unwrap());
        // The fee is flat, rate-independent.
        assert_eq!(attempt_fee(&bundle, success_rate(&bundle)), Zen(5_000_000));
    }

    #[test]
    fn second_wings_rate_caps_at_90_before_the_clamp() {
        let wing = wing_def(100);
        let feather = material_def(14, 180_000);
        let chaos = jewel_def(JewelKind::Chaos);
        let rich = material_def(23, 40_000_000);
        let wings = choice_output(vec![wing_def(150)]);
        let bundle = MatchedRecipe::SecondWings {
            wing: resolved_item(&wing, 0),
            feather: resolved_item(&feather, 0),
            chaos_jewel: resolved_item(&chaos, 0),
            fodder: vec![resolved_item(&rich, 0)],
            economics: economics(),
            wings: &wings,
        };
        // 13 + 1000 points, capped at the record's 90.
        assert_eq!(success_rate(&bundle), Percent::new(90).unwrap());
    }

    #[test]
    fn upgrade_rate_adds_25_points_exactly_when_the_item_carries_luck() {
        let helm = weapon_def(5, None);
        let chaos = jewel_def(JewelKind::Chaos);
        let mut bundle = MatchedRecipe::ItemUpgrade {
            target_item: resolved_item(&helm, 9),
            chaos_jewel: resolved_item(&chaos, 0),
            bless_jewels: Vec::new(),
            soul_jewels: Vec::new(),
            target: UpgradeTarget::PlusTen,
            base_success_percent: Percent::new(50).unwrap(),
            fee_zen: Zen(2_000_000),
        };
        assert_eq!(success_rate(&bundle), Percent::new(50).unwrap());
        if let MatchedRecipe::ItemUpgrade { target_item, .. } = &mut bundle {
            target_item.instance.luck = LuckRoll::Lucky;
        }
        assert_eq!(success_rate(&bundle), Percent::new(75).unwrap());
        assert_eq!(attempt_fee(&bundle, success_rate(&bundle)), Zen(2_000_000));
    }

    #[test]
    fn plus_eleven_base_rate_is_45_without_luck() {
        let helm = weapon_def(5, None);
        let chaos = jewel_def(JewelKind::Chaos);
        let bundle = MatchedRecipe::ItemUpgrade {
            target_item: resolved_item(&helm, 10),
            chaos_jewel: resolved_item(&chaos, 0),
            bless_jewels: Vec::new(),
            soul_jewels: Vec::new(),
            target: UpgradeTarget::PlusEleven,
            base_success_percent: Percent::new(45).unwrap(),
            fee_zen: Zen(4_000_000),
        };
        assert_eq!(success_rate(&bundle), Percent::new(45).unwrap());
        assert_eq!(attempt_fee(&bundle, success_rate(&bundle)), Zen(4_000_000));
    }

    #[test]
    fn flat_family_rates_and_fees_pass_through_verbatim() {
        let horn = material_def(2, 0);
        let chaos = jewel_def(JewelKind::Chaos);
        let output = ResolvedOutput::Single(material_def(3, 0));
        let dinorant = MatchedRecipe::Dinorant {
            horns: OneOrMore::with_head(resolved_item(&horn, 0), Vec::new()),
            chaos_jewel: resolved_item(&chaos, 0),
            success_percent: Percent::new(70).unwrap(),
            fee_zen: Zen(250_000),
            dinorant: &output,
        };
        assert_eq!(success_rate(&dinorant), Percent::new(70).unwrap());
        assert_eq!(
            attempt_fee(&dinorant, success_rate(&dinorant)),
            Zen(250_000)
        );

        let creation = jewel_def(JewelKind::Creation);
        let fruit_output = ResolvedOutput::Single(material_def(15, 0));
        let fruits = MatchedRecipe::Fruits {
            catalyst: resolved_item(&creation, 0),
            chaos_jewel: resolved_item(&chaos, 0),
            success_percent: Percent::new(90).unwrap(),
            fee_zen: Zen(3_000_000),
            fruit: &fruit_output,
        };
        assert_eq!(success_rate(&fruits), Percent::new(90).unwrap());
        assert_eq!(attempt_fee(&fruits, success_rate(&fruits)), Zen(3_000_000));
    }

    #[test]
    fn ticket_rows_index_by_level_with_k1_and_saturation() {
        let eye = material_def(17, 0);
        let key = material_def(18, 0);
        let chaos = jewel_def(JewelKind::Chaos);
        let output = ResolvedOutput::Single(material_def(19, 0));
        let fees = [
            Zen(100_000),
            Zen(200_000),
            Zen(400_000),
            Zen(700_000),
            Zen(1_100_000),
            Zen(1_600_000),
            Zen(2_000_000),
        ];
        let rates = [
            Percent::new(80).unwrap(),
            Percent::new(80).unwrap(),
            Percent::new(80).unwrap(),
            Percent::new(80).unwrap(),
            Percent::new(70).unwrap(),
            Percent::new(70).unwrap(),
            Percent::new(70).unwrap(),
        ];
        let at = |level: u8| MatchedRecipe::DevilSquareTicket {
            eye: resolved_item(&eye, level),
            key: resolved_item(&key, level),
            chaos_jewel: resolved_item(&chaos, 0),
            extras: Vec::new(),
            level: ItemLevel::new(level).unwrap(),
            invitation: &output,
            fee_zen_by_level: fees,
            success_percent_by_level: rates,
        };
        // Level 3 reads row 3.
        assert_eq!(attempt_fee(&at(3), Percent::ZERO), Zen(400_000));
        assert_eq!(success_rate(&at(3)), Percent::new(80).unwrap());
        // Level 5 crosses into the 70% band.
        assert_eq!(success_rate(&at(5)), Percent::new(70).unwrap());
        // K1: a level-0 pair reads the level-1 row.
        assert_eq!(attempt_fee(&at(0), Percent::ZERO), Zen(100_000));
        assert_eq!(success_rate(&at(0)), Percent::new(80).unwrap());
        // A level past the table saturates to the last row.
        assert_eq!(attempt_fee(&at(15), Percent::ZERO), Zen(2_000_000));
    }

    // -- the roll (D1) ------------------------------------------------------

    #[test]
    fn rate_extremes_consume_no_random_word() {
        let mut rng = CountingRng::new(7);
        assert!(!roll_success(Percent::ZERO, &mut rng));
        assert!(roll_success(Percent::new(100).unwrap(), &mut rng));
        assert_eq!(rng.words, 0);
        assert_eq!(
            roll_success(Percent::new(50).unwrap(), &mut rng),
            roll_percent(Percent::new(50).unwrap(), &mut TestRng::new(7))
        );
        assert_eq!(rng.words, 1);
    }

    // -- the downgrade (§4.4) -----------------------------------------------

    #[test]
    fn downgrade_draws_a_level_strictly_below_the_old_one() {
        let weapon = weapon_def(1, None);
        let mut seen = [false; 6];
        for seed in 0..200 {
            let mut rng = TestRng::new(seed);
            let mut item = resolved_item(&weapon, 6);
            item.instance.normal_option = None;
            let downgraded = downgrade(item, &mut rng);
            let level = usize::from(downgraded.level.get());
            assert!(level <= 5, "level {level} not below +6");
            seen[level] = true;
        }
        assert!(
            seen.iter().all(|&hit| hit),
            "every level below +6 reachable"
        );
    }

    #[test]
    fn a_plus_zero_bare_item_downgrades_with_no_draw_and_stays_plus_zero() {
        let weapon = weapon_def(1, None);
        let mut rng = CountingRng::new(3);
        let downgraded = downgrade(resolved_item(&weapon, 0), &mut rng);
        assert_eq!(downgraded.level, ItemLevel::ZERO);
        assert_eq!(rng.words, 0);
    }

    #[test]
    fn an_excellent_item_never_loses_its_skill() {
        let weapon = weapon_def(1, Some(crate::data::common::SkillNumber(19)));
        for seed in 0..100 {
            let mut rng = TestRng::new(seed);
            let mut item = resolved_item(&weapon, 6);
            item.instance.skill = SkillRoll::WithSkill;
            item.instance.roll = RarityRoll::Excellent {
                options: ExcellentOptions::Armor {
                    options: ExcellentArmorSet::with_first(ExcellentArmorOption::MaxHealth, []),
                },
            };
            // Keep the gauge legal for the excellent max at +6.
            item.instance.durability = Durability::full(20);
            assert_eq!(downgrade(item, &mut rng).skill, SkillRoll::WithSkill);
        }
    }

    #[test]
    fn a_skilled_normal_item_loses_its_skill_on_the_50_percent_branch() {
        let weapon = weapon_def(1, Some(crate::data::common::SkillNumber(19)));
        let mut kept = false;
        let mut lost = false;
        for seed in 0..100 {
            let mut rng = TestRng::new(seed);
            let mut item = resolved_item(&weapon, 6);
            item.instance.skill = SkillRoll::WithSkill;
            match downgrade(item, &mut rng).skill {
                SkillRoll::WithSkill => kept = true,
                SkillRoll::NoSkill => lost = true,
            }
        }
        assert!(kept && lost, "both 50% branches must be reachable");
    }

    #[test]
    fn option_decrement_steps_one_level_and_removes_at_l1() {
        let weapon = weapon_def(1, None);
        let mut removed = false;
        let mut kept_l1 = false;
        let mut stepped_to_l1 = false;
        for seed in 0..100 {
            let mut rng = TestRng::new(seed);
            let mut item = resolved_item(&weapon, 6);
            item.instance.normal_option = Some(RolledNormalOption {
                option: crate::components::item_options::NormalOption::PhysicalDamage,
                level: OptionLevel::L1,
            });
            match downgrade(item, &mut rng).normal_option {
                None => removed = true,
                Some(rolled) => {
                    assert_eq!(rolled.level, OptionLevel::L1);
                    kept_l1 = true;
                }
            }
            let mut two = resolved_item(&weapon, 6);
            two.instance.normal_option = Some(RolledNormalOption {
                option: crate::components::item_options::NormalOption::PhysicalDamage,
                level: OptionLevel::L2,
            });
            let mut rng2 = TestRng::new(seed);
            if let Some(rolled) = downgrade(two, &mut rng2).normal_option {
                if rolled.level == OptionLevel::L1 {
                    stepped_to_l1 = true;
                }
            }
        }
        assert!(removed && kept_l1 && stepped_to_l1);
    }

    #[test]
    fn downgrade_rescales_durability_by_integer_floor() {
        // A +1 item downgrades to +0 deterministically (0..=0). Base 20 →
        // +1 max 21; gauge 10/21 → new max 20 → floor(20·10/21) = 9.
        let weapon = weapon_def(1, None);
        let mut item = resolved_item(&weapon, 1);
        item.instance.durability = Durability::new(10, 21).unwrap();
        let mut rng = TestRng::new(11);
        let downgraded = downgrade(item, &mut rng);
        assert_eq!(downgraded.level, ItemLevel::ZERO);
        assert_eq!(downgraded.durability.current(), 9);
        assert_eq!(downgraded.durability.max(), 20);
    }

    #[test]
    fn upgrade_rescale_uses_the_new_level_maximum() {
        // +9 → +10 in place: max 20+12=32 → 20+17=37; gauge 16/32 →
        // floor(37·16/32) = 18.
        let weapon = weapon_def(1, None);
        let mut item = resolved_item(&weapon, 9);
        item.instance.durability = Durability::new(16, 32).unwrap();
        let upgraded = upgrade_in_place(item, UpgradeTarget::PlusTen);
        assert_eq!(upgraded.level.get(), 10);
        assert_eq!(upgraded.durability.current(), 18);
        assert_eq!(upgraded.durability.max(), 37);
    }

    // -- dinorant options (K2) and fruit levels (C3) --------------------------

    #[test]
    fn dinorant_options_are_at_most_two_and_k2_discards_the_duplicate() {
        let mut none_one_word = false;
        let mut single_three_words = false;
        let mut single_four_words = false; // the K2 duplicate discard
        let mut pair_four_words = false;
        for seed in 0..2000 {
            let mut rng = CountingRng::new(seed);
            match dinorant_augment(&mut rng) {
                CraftedAugment::None => {
                    assert_eq!(rng.words, 1);
                    none_one_word = true;
                }
                CraftedAugment::Dinorant { options } => match (options.count(), rng.words) {
                    (1, 3) => single_three_words = true,
                    (1, 4) => single_four_words = true,
                    (2, 4) => pair_four_words = true,
                    (count, words) => {
                        panic!("unexpected dinorant draw shape: {count} options, {words} words")
                    }
                },
                CraftedAugment::WingBonus { .. } => panic!("a dinorant never rolls a wing bonus"),
            }
        }
        assert!(none_one_word, "the 70% no-option branch must appear");
        assert!(single_three_words, "a lone first option must appear");
        assert!(
            single_four_words,
            "K2: a discarded duplicate second draw must appear"
        );
        assert!(pair_four_words, "two distinct options must appear");
    }

    #[test]
    fn fruit_level_is_one_draw_over_the_derived_weight_total() {
        assert_eq!(FRUIT_WEIGHT_TOTAL, 100);
        let mut counts = [0u32; 5];
        for seed in 0..4000 {
            let mut rng = CountingRng::new(seed);
            let level = fruit_level(&mut rng);
            assert_eq!(rng.words, 1, "the fruit level is a single draw");
            counts[usize::from(level.wire())] += 1;
        }
        assert!(
            counts.iter().all(|&count| count > 0),
            "all five levels roll"
        );
        assert!(
            counts[0] > counts[4],
            "weight 30 must outdraw weight 5 over a large sample"
        );
    }

    // -- bonus-roll word gating ----------------------------------------------

    #[test]
    fn value_bonuses_draw_only_their_gated_words() {
        // A skilled weapon: luck, option index, option chance, skill → 4 words.
        let skilled = weapon_def(1, Some(crate::data::common::SkillNumber(19)));
        let mut rng = CountingRng::new(5);
        let _ = value_family_bonuses(&skilled, Percent::new(50).unwrap(), &mut rng);
        assert_eq!(rng.words, 4);
        // A skill-less weapon skips the skill word.
        let plain = weapon_def(1, None);
        let mut rng = CountingRng::new(5);
        let _ = value_family_bonuses(&plain, Percent::new(50).unwrap(), &mut rng);
        assert_eq!(rng.words, 3);
        // A wing with no listed option draws luck only.
        let mut bare_wing = wing_def(100);
        if let ItemKind::Wings { jol_options, .. } = &mut bare_wing.kind {
            jol_options.clear();
        }
        let mut rng = CountingRng::new(5);
        let _ = value_family_bonuses(&bare_wing, Percent::new(50).unwrap(), &mut rng);
        assert_eq!(rng.words, 1);
    }
}
