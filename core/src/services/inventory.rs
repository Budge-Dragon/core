//! Container and pickup behavior: the thin `(state, intent) -> (state,
//! outcome)` adapters over the [`Inventory`] and [`Equipment`] components,
//! plus the ground-pickup pair — [`pickup`] lifts a [`WorldItem`] into the
//! inventory and [`pickup_zen`] merges a [`WorldZen`] pile into a
//! [`CarriedZen`] balance. Both pickups gate on the picker's locus first
//! (same map, within [`pickup_reach`]); item pickup then consults the drop's
//! [`DropClaim`] window before the store step. Each folds the component's
//! `Result` into a per-operation outcome enum with no `unwrap` — both arms
//! bound — and never re-implements a geometry or cap rule. The equip service
//! is the one place item-kind/slot compatibility and two-handed dual-hand
//! occupancy are decided (the component accepts any instance in any slot).
//! Container operations draw zero RNG.

use serde::{Deserialize, Serialize};

use crate::components::class::CharacterClass;
use crate::components::drop_claim::PickerStanding;
use crate::components::equipment::{Equipment, EquipmentSlot};
use crate::components::inventory::{Cell, Footprint, Inventory, PlacementRejection};
use crate::components::item_instance::{ItemInstance, RolledNormalOption};
use crate::components::spatial::{Radius, WorldPos};
use crate::components::stats::Stats;
use crate::components::units::{CarriedZen, CreditOutcome, Level, MapNumber, Tick};
use crate::data::atlas::Atlas;
use crate::data::item_definitions::{ItemDefinition, ItemKind, WeaponHandling, WearRequirements};
use crate::entities::world_item::WorldItem;
use crate::entities::world_zen::WorldZen;
use crate::events::inventory::{
    EquipOutcome, EquipRejection, MoveOutcome, PlaceOutcome, RemoveOutcome, UnequipOutcome,
};
use crate::services::item_rules::effective_drop_level;

/// The parsed request to place an item: where, how large, and the move-only
/// item itself. Built by the host from the definition's footprint (its Atlas
/// lookup), so the service never reaches into `data` for geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaceIntent {
    /// The anchor cell to place at.
    pub anchor: Cell,
    /// The item's cell footprint.
    pub footprint: Footprint,
    /// The item to place.
    pub item: ItemInstance,
}

/// Places an item into the inventory, folding the component result into a
/// [`PlaceOutcome`]. On rejection the inventory is unchanged and the item rides
/// the outcome back.
#[must_use]
pub fn place_item(inventory: Inventory, intent: PlaceIntent) -> (Inventory, PlaceOutcome) {
    let PlaceIntent {
        anchor,
        footprint,
        item,
    } = intent;
    match inventory.place(anchor, footprint, item) {
        Ok(inventory) => (inventory, PlaceOutcome::Placed { at: anchor }),
        Err((inventory, item, reason)) => (inventory, PlaceOutcome::Rejected { reason, item }),
    }
}

/// Removes the item covering `cell`, folding the component result into a
/// [`RemoveOutcome`]. On success the item rides the outcome out.
#[must_use]
pub fn remove_item(inventory: Inventory, cell: Cell) -> (Inventory, RemoveOutcome) {
    match inventory.remove(cell) {
        Ok((inventory, item)) => (inventory, RemoveOutcome::Removed { at: cell, item }),
        Err((inventory, reason)) => (inventory, RemoveOutcome::Rejected { reason }),
    }
}

/// Moves the item covering `from` so its anchor is `to`, folding the component
/// result into a [`MoveOutcome`]. No item crosses the boundary.
#[must_use]
pub fn move_item(inventory: Inventory, from: Cell, to: Cell) -> (Inventory, MoveOutcome) {
    match inventory.move_to(from, to) {
        Ok(inventory) => (inventory, MoveOutcome::Moved { from, to }),
        Err((inventory, reason)) => (inventory, MoveOutcome::Rejected { reason }),
    }
}

/// What a ground-item pickup produced, kind-tagged: the item entered the
/// inventory, the placement was rejected, the picker stood out of reach, or
/// the claim window refused a stranger — every refusal hands the untouched
/// world item back. Lives in the service (not `events`) because it carries a
/// whole [`WorldItem`] entity, and an event never imports an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PickupOutcome {
    /// The ground item was picked up and stored at `at`.
    PickedUp {
        /// The anchor cell it was stored at.
        at: Cell,
    },
    /// The inventory had no room; the untouched ground item is handed back.
    Rejected {
        /// Why the pickup was rejected.
        reason: PlacementRejection,
        /// The ground item, reassembled exactly as it was.
        item: WorldItem,
    },
    /// Off-map or beyond three tiles; the untouched ground item is handed
    /// back. Mirrors [`crate::events::shop::BuyOutcome::OutOfRange`].
    OutOfReach {
        /// The ground item, exactly as it was.
        item: WorldItem,
    },
    /// A stranger reached inside the claim window; the untouched ground item
    /// is handed back, still on the ground, still pickable once the window
    /// elapses.
    Refused {
        /// The ground item, exactly as it was.
        item: WorldItem,
    },
}

/// Picks a ground item up into the inventory at `anchor`. Gate order: reach
/// first (same map AND within [`pickup_reach`] — `OutOfReach`), then the
/// drop's claim window ([`crate::components::drop_claim::DropClaim::admits`]
/// — `Refused`), then the store step. On success the world item is consumed;
/// on any refusal the inventory is unchanged and the untouched world item
/// rides the outcome back. `footprint` comes from the caller's Atlas lookup;
/// `standing` is the host-resolved kill-snapshot relation; `now` is the
/// window clock. No liveness gate — core assumes an admitted picker is alive.
/// Eight parameters, each a distinct non-bundleable domain input: the drop
/// (carrying its own locus and claim), the destination inventory, the
/// placement geometry pair, the picker locus pair, the claim relation, and
/// the clock.
#[must_use]
pub fn pickup(
    world_item: WorldItem,
    inventory: Inventory,
    anchor: Cell,
    footprint: Footprint,
    actor_pos: WorldPos,
    actor_map: MapNumber,
    standing: PickerStanding,
    now: Tick,
) -> (Inventory, PickupOutcome) {
    if !in_reach(actor_pos, actor_map, world_item.position, world_item.map) {
        return (inventory, PickupOutcome::OutOfReach { item: world_item });
    }
    if !world_item.claim.admits(standing, now) {
        return (inventory, PickupOutcome::Refused { item: world_item });
    }
    let WorldItem {
        instance,
        position,
        map,
        despawn,
        claim,
    } = world_item;
    match inventory.place(anchor, footprint, instance) {
        Ok(inventory) => (inventory, PickupOutcome::PickedUp { at: anchor }),
        Err((inventory, instance, reason)) => (
            inventory,
            PickupOutcome::Rejected {
                reason,
                item: WorldItem {
                    instance,
                    position,
                    map,
                    despawn,
                    claim,
                },
            },
        ),
    }
}

/// The pickup reach — three tiles, the `merchant_reach` convention.
#[must_use]
pub fn pickup_reach() -> Radius {
    Radius::from_tiles(3)
}

/// Same map AND within pickup reach — the folded gate, `party::reach`'s
/// shape; a cross-map pair fails it.
fn in_reach(
    actor_pos: WorldPos,
    actor_map: MapNumber,
    item_pos: WorldPos,
    item_map: MapNumber,
) -> bool {
    actor_map == item_map && actor_pos.within_range(item_pos, pickup_reach())
}

/// What a zen pickup produced, kind-tagged: the whole pile merged into the
/// balance, crediting it would overflow the carry cap, or the picker stood
/// out of reach — every refusal hands the untouched pile back, intact, still
/// on the ground, still pickable. No partial pickup exists. Zen carries no
/// claim, so there is no `Refused` arm. Lives in the service (not `events`)
/// because the rejections carry a whole [`WorldZen`] entity, and an event
/// never imports an entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZenPickupOutcome {
    /// The whole pile merged; the returned balance is the new total.
    PickedUp,
    /// The pile would overflow the carry cap; the untouched pile is handed
    /// back and the balance is unchanged.
    OverCap {
        /// The pile, exactly as it was.
        world_zen: WorldZen,
    },
    /// Off-map or beyond three tiles; the untouched pile is handed back.
    OutOfReach {
        /// The pile, exactly as it was.
        world_zen: WorldZen,
    },
}

/// Picks a whole zen pile up, merging it through [`CarriedZen::credit`]. The
/// reach gate runs first (same map AND within [`pickup_reach`] —
/// `OutOfReach`); zen carries no claim, so no window and no `now` follow. The
/// returned balance is authoritative: the new total on a pickup, the
/// unchanged balance on any refusal.
#[must_use]
pub fn pickup_zen(
    world_zen: WorldZen,
    zen: CarriedZen,
    actor_pos: WorldPos,
    actor_map: MapNumber,
) -> (CarriedZen, ZenPickupOutcome) {
    if !in_reach(actor_pos, actor_map, world_zen.position, world_zen.map) {
        return (zen, ZenPickupOutcome::OutOfReach { world_zen });
    }
    match zen.credit(world_zen.amount) {
        CreditOutcome::Credited { balance } => (balance, ZenPickupOutcome::PickedUp),
        CreditOutcome::OverCap { balance } => (balance, ZenPickupOutcome::OverCap { world_zen }),
    }
}

/// A would-be wearer's eligibility view: class, raw level, and TOTAL stats
/// (base plus any bonus from ALREADY-worn gear). Pre-S3 no worn item grants a
/// stat, so `stats` is the character's base stats today — the `total` framing
/// is the seam an ancient stat-gear wave plugs into without reshaping the
/// gate. The item being equipped never qualifies itself: the host builds this
/// from the OTHER worn pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wearer {
    /// The wearer's class, matched against the item's qualified-class list.
    pub class: CharacterClass,
    /// The wearer's raw character level (never scaled).
    pub level: Level,
    /// The wearer's TOTAL stats (base + worn-gear bonuses).
    pub stats: Stats,
}

/// Equips an item into a slot. Capability outranks transient state, and both
/// outrank eligibility: an incompatible slot is rejected before an occupied
/// one, two-handed dual-hand occupancy is checked against the paired hand
/// (resolved through the atlas), and the class/requirement gate runs last. On
/// rejection the item rides the outcome back.
#[must_use]
pub fn equip(
    equipment: Equipment,
    item: ItemInstance,
    def: &ItemDefinition,
    slot: EquipmentSlot,
    atlas: &Atlas,
    wearer: &Wearer,
) -> (Equipment, EquipOutcome) {
    if !slot_accepts(&def.kind, slot) {
        return (
            equipment,
            EquipOutcome::Rejected {
                reason: EquipRejection::IncompatibleSlot,
                item,
            },
        );
    }
    if equipment.get(slot).is_some() {
        return (
            equipment,
            EquipOutcome::Rejected {
                reason: EquipRejection::SlotOccupied,
                item,
            },
        );
    }
    if two_handed_conflict(&equipment, hand_occupation(&def.kind), slot, atlas) {
        return (
            equipment,
            EquipOutcome::Rejected {
                reason: EquipRejection::TwoHandedConflict,
                item,
            },
        );
    }
    if let Some(reason) = eligibility(wearer, &item, def) {
        return (equipment, EquipOutcome::Rejected { reason, item });
    }
    (equipment.with(slot, item), EquipOutcome::Equipped { slot })
}

// W-SRC: the equip eligibility gate mirrors OpenMU's `CompliesRequirements`
// (GameLogic/Player.cs:926-939): every stat requirement compares the wearer's
// TOTAL attribute against the scaled value
// `(mult × effectiveDropLevel × base / 100) + 20` (ItemExtensions.cs:270-289,
// 404-414; mult 3 for strength/agility/vitality/command, 4 for energy), the
// strength requirement additionally +4 × normal-option level when the scaled
// value is > 0 (:290-297), the level requirement compares RAW (:22-29,317),
// every compare is inclusive (fails only on attribute < required, :932), and
// the class rule is list-contains over the data-materialized qualified list
// (Player.cs:938). Checked at equip only — never rechecked on stat loss.
/// The stat-requirement scaling multiplier for strength/agility/vitality/
/// command columns.
const REQUIREMENT_MULT_PHYSICAL: u32 = 3;
/// The stat-requirement scaling multiplier for the energy column.
const REQUIREMENT_MULT_ENERGY: u32 = 4;
/// The flat tail every nonzero scaled requirement lands on.
const REQUIREMENT_FLAT_ADD: u32 = 20;
/// The scaled-requirement percent divisor.
const REQUIREMENT_DIVISOR: u32 = 100;
/// The strength requirement's per-normal-option-level surcharge.
const REQUIREMENT_PER_OPTION_LEVEL: u32 = 4;

/// The first failing eligibility precondition, or `None` when the wearer may
/// equip the item — the `cast_rejection` idiom. Class first, then requirements
/// (the order is not load-bearing: one reason surfaces per equip). A
/// non-wearable kind carries no class list, which [`slot_accepts`] already
/// excluded — the `?` folds it to eligible.
fn eligibility(
    wearer: &Wearer,
    item: &ItemInstance,
    def: &ItemDefinition,
) -> Option<EquipRejection> {
    let classes = def.kind.classes()?;
    if !classes.allows(wearer.class) {
        return Some(EquipRejection::ClassMismatch);
    }
    let wear = def.kind.wear()?;
    let edl = effective_drop_level(
        def.drop_level,
        item.level.enhance_level_or_zero(),
        item.roll.rarity(),
    );
    requirements_unmet(wear, wearer, edl, item.normal_option)
        .then_some(EquipRejection::RequirementsNotMet)
}

/// Whether any scaled stat requirement, or the raw level, exceeds the wearer's
/// total. `edl` is the effective drop level (rarity/enhance surcharge already
/// in). Every compare is inclusive — a requirement fails only on
/// `total < required`.
fn requirements_unmet(
    wear: WearRequirements,
    wearer: &Wearer,
    edl: u16,
    option: Option<RolledNormalOption>,
) -> bool {
    let (strength, agility, vitality, energy, command) = totals(wearer.stats);
    let below = |base_col: u16, mult: u32, total: u16| match scaled(base_col, mult, edl) {
        // A 0 column is NO requirement — never scaled to the flat 20.
        None => false,
        Some(required) => u32::from(total) < required,
    };
    wearer.level.get() < wear.level
        || strength_below(wear.strength, strength, edl, option)
        || below(wear.agility, REQUIREMENT_MULT_PHYSICAL, agility)
        || below(wear.vitality, REQUIREMENT_MULT_PHYSICAL, vitality)
        || below(wear.energy, REQUIREMENT_MULT_ENERGY, energy)
        || below(wear.command, REQUIREMENT_MULT_PHYSICAL, command)
}

/// `(mult × edl × base_col) / 100 + 20`, or `None` when the column is 0 (no
/// requirement).
fn scaled(base_col: u16, mult: u32, edl: u16) -> Option<u32> {
    if base_col == 0 {
        return None;
    }
    Some(
        (mult
            .saturating_mul(u32::from(edl))
            .saturating_mul(u32::from(base_col))
            / REQUIREMENT_DIVISOR)
            .saturating_add(REQUIREMENT_FLAT_ADD),
    )
}

/// Whether the wearer's total strength is below the scaled strength
/// requirement, folding the normal-option surcharge: +4 per option level, only
/// when the pre-+20 scaled value is already positive.
fn strength_below(base_col: u16, total: u16, edl: u16, option: Option<RolledNormalOption>) -> bool {
    if base_col == 0 {
        return false;
    }
    let pre20 = REQUIREMENT_MULT_PHYSICAL
        .saturating_mul(u32::from(edl))
        .saturating_mul(u32::from(base_col))
        / REQUIREMENT_DIVISOR;
    let option_term = match option {
        Some(rolled) if pre20 > 0 => {
            REQUIREMENT_PER_OPTION_LEVEL.saturating_mul(u32::from(rolled.level.wire()))
        }
        Some(_) | None => 0,
    };
    let required = pre20
        .saturating_add(REQUIREMENT_FLAT_ADD)
        .saturating_add(option_term);
    u32::from(total) < required
}

/// The five trainable stat totals `(strength, agility, vitality, energy,
/// command)`; command is 0 for the four non-command classes — a real domain
/// zero (they train none), not a fabricated default.
fn totals(stats: Stats) -> (u16, u16, u16, u16, u16) {
    match stats {
        Stats::Standard {
            strength,
            agility,
            vitality,
            energy,
        } => (strength, agility, vitality, energy, 0),
        Stats::WithCommand {
            strength,
            agility,
            vitality,
            energy,
            command,
        } => (strength, agility, vitality, energy, command),
    }
}

/// Unequips a slot, folding the component result into an [`UnequipOutcome`]. On
/// success the removed item rides the outcome out.
#[must_use]
pub fn unequip(equipment: Equipment, slot: EquipmentSlot) -> (Equipment, UnequipOutcome) {
    let (equipment, taken) = equipment.without(slot);
    match taken {
        Some(item) => (equipment, UnequipOutcome::Unequipped { slot, item }),
        None => (equipment, UnequipOutcome::SlotEmpty),
    }
}

/// Whether an item of `kind` may be worn in `slot` — the exhaustive
/// `(ItemKind x EquipmentSlot)` compatibility rule. Non-equippable kinds accept
/// no slot; ammunition rides a hand slot beside its bow/crossbow (the fold and
/// the wear service both read it there). Two-handed dual-hand occupancy is a
/// separate rule, enforced by the equip service's two-handed check and by
/// [`reconcile_equipment`] at reload.
fn slot_accepts(kind: &ItemKind, slot: EquipmentSlot) -> bool {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. } => {
            matches!(slot, EquipmentSlot::LeftHand | EquipmentSlot::RightHand)
        }
        ItemKind::Helm { .. } => matches!(slot, EquipmentSlot::Helm),
        ItemKind::BodyArmor { .. } => matches!(slot, EquipmentSlot::Armor),
        ItemKind::Pants { .. } => matches!(slot, EquipmentSlot::Pants),
        ItemKind::Gloves { .. } => matches!(slot, EquipmentSlot::Gloves),
        ItemKind::Boots { .. } => matches!(slot, EquipmentSlot::Boots),
        ItemKind::Wings { .. } => matches!(slot, EquipmentSlot::Wings),
        ItemKind::Pet { .. } => matches!(slot, EquipmentSlot::Pet),
        ItemKind::Pendant { .. } => matches!(slot, EquipmentSlot::Pendant),
        ItemKind::Ring { .. } | ItemKind::TransformationRing { .. } => {
            matches!(slot, EquipmentSlot::Ring1 | EquipmentSlot::Ring2)
        }
        ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => false,
    }
}

/// How a hand item occupies the hand pair: one hand, both hands, a launcher
/// (both hands, except the paired hand may hold ammunition), or ammunition
/// (rides the paired hand of a launcher).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandOccupation {
    /// Claims a single hand; the paired hand stays free for another one-hander.
    OneHand,
    /// Claims both hands; the paired hand must stay empty.
    TwoHands,
    /// A bow/crossbow: two-handed, except the paired hand may hold ammunition.
    Launcher,
    /// Arrows/bolts: legal only beside a launcher (or into an empty pair,
    /// awaiting one).
    Ammunition,
}

/// How an item claims a hand slot. Total over [`ItemKind`]; non-hand kinds
/// never reach a hand slot ([`slot_accepts`] gates them), so
/// [`HandOccupation::OneHand`] is a harmless total answer for them.
// W-SRC: two-handedness is structural, not a stored flag. Melee weapons carry an
// explicit `WeaponHandling`; bows and crossbows have no handling field because
// they are two-handed by construction — except that the paired hand carries
// their ammunition (AmmunitionConsumptionRate 1,
// Version075/Items/Weapons.cs:327-330), which is what `Launcher` encodes.
// Staves are treated one-handed for want of a two-handed flag; a handling
// distinction for staves (and any two-handed staff) is a possible future
// data-model refinement, deliberately not fabricated here.
fn hand_occupation(kind: &ItemKind) -> HandOccupation {
    match kind {
        ItemKind::Weapon {
            handling: WeaponHandling::TwoHanded,
            ..
        } => HandOccupation::TwoHands,
        ItemKind::Bow { .. } | ItemKind::Crossbow { .. } => HandOccupation::Launcher,
        ItemKind::Arrows { .. } | ItemKind::Bolts { .. } => HandOccupation::Ammunition,
        ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            ..
        }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Pet { .. }
        | ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::TransformationRing { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => HandOccupation::OneHand,
    }
}

/// The paired hand of a hand slot; `None` for a non-hand slot. Only the two hand
/// slots pair — every other slot is independent.
fn paired_hand(slot: EquipmentSlot) -> Option<EquipmentSlot> {
    match slot {
        EquipmentSlot::LeftHand => Some(EquipmentSlot::RightHand),
        EquipmentSlot::RightHand => Some(EquipmentSlot::LeftHand),
        EquipmentSlot::Helm
        | EquipmentSlot::Armor
        | EquipmentSlot::Pants
        | EquipmentSlot::Gloves
        | EquipmentSlot::Boots
        | EquipmentSlot::Wings
        | EquipmentSlot::Pet
        | EquipmentSlot::Pendant
        | EquipmentSlot::Ring1
        | EquipmentSlot::Ring2 => None,
    }
}

/// A worn hand occupant's occupation, resolved through the atlas. An occupant
/// the atlas cannot identify claims one hand — a total fold of genuine
/// optionality, never a should-never-happen panic.
fn occupant_occupation(occupant: &ItemInstance, atlas: &Atlas) -> HandOccupation {
    match atlas.item(occupant.item) {
        Some(def) => hand_occupation(&def.kind),
        None => HandOccupation::OneHand,
    }
}

/// Whether wearing an `incoming`-occupancy item in `slot` would break hand-pair
/// occupancy: a two-handed melee weapon needs its paired hand empty; a launcher
/// needs it empty or holding ammunition; ammunition needs it empty or holding a
/// launcher; and a one-hander may not share the pair with a two-handed weapon,
/// a launcher, or ammunition. A non-hand slot has no paired hand, so it never
/// conflicts.
fn two_handed_conflict(
    equipment: &Equipment,
    incoming: HandOccupation,
    slot: EquipmentSlot,
    atlas: &Atlas,
) -> bool {
    let Some(paired) = paired_hand(slot) else {
        return false;
    };
    let Some(occupant) = equipment.get(paired) else {
        return false;
    };
    hand_pair_conflicts(incoming, occupant_occupation(occupant, atlas))
}

/// The exhaustive hand-pair legality rule over both occupations. The only
/// legal occupied pairs are one-hander beside one-hander and launcher beside
/// ammunition (either order).
fn hand_pair_conflicts(incoming: HandOccupation, paired: HandOccupation) -> bool {
    match (incoming, paired) {
        (HandOccupation::OneHand, HandOccupation::OneHand)
        | (HandOccupation::Launcher, HandOccupation::Ammunition)
        | (HandOccupation::Ammunition, HandOccupation::Launcher) => false,
        (
            HandOccupation::OneHand,
            HandOccupation::TwoHands | HandOccupation::Launcher | HandOccupation::Ammunition,
        )
        | (
            HandOccupation::TwoHands,
            HandOccupation::OneHand
            | HandOccupation::TwoHands
            | HandOccupation::Launcher
            | HandOccupation::Ammunition,
        )
        | (
            HandOccupation::Launcher,
            HandOccupation::OneHand | HandOccupation::TwoHands | HandOccupation::Launcher,
        )
        | (
            HandOccupation::Ammunition,
            HandOccupation::OneHand | HandOccupation::TwoHands | HandOccupation::Ammunition,
        ) => true,
    }
}

/// Re-proves hand-pair occupancy at the reload boundary — the
/// instance×definition cross-reference the [`Equipment`] component cannot hold
/// alone (a slot carries an [`ItemInstance`], whose handedness lives in the
/// definition). A hand wearing a two-handed weapon requires the other hand
/// empty; a launcher admits only ammunition beside it.
///
/// # Errors
/// Returns [`EquipmentConflict::TwoHandedWithOffhand`] when both hands are
/// occupied by an illegal pairing.
pub fn reconcile_equipment(equipment: &Equipment, atlas: &Atlas) -> Result<(), EquipmentConflict> {
    match (
        equipment.get(EquipmentSlot::LeftHand),
        equipment.get(EquipmentSlot::RightHand),
    ) {
        (Some(left), Some(right)) => {
            if hand_pair_conflicts(
                occupant_occupation(left, atlas),
                occupant_occupation(right, atlas),
            ) {
                Err(EquipmentConflict::TwoHandedWithOffhand)
            } else {
                Ok(())
            }
        }
        (Some(_) | None, None) | (None, Some(_)) => Ok(()),
    }
}

/// Why a reloaded equipment set violates hand-pair occupancy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EquipmentConflict {
    /// Both hands are occupied by an illegal pairing: a two-handed weapon
    /// beside anything, a launcher beside a non-ammunition item, or ammunition
    /// beside a non-launcher item.
    TwoHandedWithOffhand,
}

impl core::fmt::Display for EquipmentConflict {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TwoHandedWithOffhand => write!(
                f,
                "a two-handed weapon is worn while the other hand is occupied"
            ),
        }
    }
}

impl core::error::Error for EquipmentConflict {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::spatial::WorldPos;
    use crate::components::units::{ItemLevel, MapNumber, Tick, Zen};

    // The equip service now resolves a paired-hand occupant through the Atlas, so
    // its two-handed rule and `reconcile_equipment` are exercised against the real
    // dataset in `core/tests/item_roll_integration.rs`. These inline tests cover
    // the Atlas-free container geometry and the unequip fold.

    fn item(number: u16) -> ItemInstance {
        ItemInstance {
            item: ItemRef { group: 0, number },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
            augment: CraftedAugment::None,
        }
    }

    fn footprint(width: u8, height: u8) -> Footprint {
        Footprint::new(width, height).unwrap()
    }

    fn cell(row: u8, col: u8) -> Cell {
        Cell { row, col }
    }

    #[test]
    fn place_then_reject_hands_the_item_back() {
        let inventory = Inventory::empty(8, 8);
        let (inventory, outcome) = place_item(
            inventory,
            PlaceIntent {
                anchor: cell(0, 0),
                footprint: footprint(2, 2),
                item: item(1),
            },
        );
        assert_eq!(outcome, PlaceOutcome::Placed { at: cell(0, 0) });
        let (inventory, outcome) = place_item(
            inventory,
            PlaceIntent {
                anchor: cell(1, 1),
                footprint: footprint(2, 2),
                item: item(2),
            },
        );
        match outcome {
            PlaceOutcome::Rejected { reason, item } => {
                assert_eq!(reason, PlacementRejection::CellsOccupied);
                assert_eq!(item.item.number, 2);
            }
            PlaceOutcome::Placed { .. } => panic!("overlap should reject"),
        }
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn remove_and_move_fold_the_component_result() {
        let (inventory, _) = place_item(
            Inventory::empty(8, 8),
            PlaceIntent {
                anchor: cell(0, 0),
                footprint: footprint(1, 1),
                item: item(5),
            },
        );
        let (inventory, outcome) = move_item(inventory, cell(0, 0), cell(3, 3));
        assert_eq!(
            outcome,
            MoveOutcome::Moved {
                from: cell(0, 0),
                to: cell(3, 3)
            }
        );
        let (inventory, outcome) = remove_item(inventory, cell(3, 3));
        match outcome {
            RemoveOutcome::Removed { at, item } => {
                assert_eq!(at, cell(3, 3));
                assert_eq!(item.item.number, 5);
            }
            RemoveOutcome::Rejected { .. } => panic!("item is present"),
        }
        assert!(inventory.placed().is_empty());
    }

    use crate::components::drop_claim::DropClaim;
    use crate::components::spatial::UNITS_PER_TILE;

    /// A world position at whole-tile coordinates.
    fn tile_pos(x: i64, y: i64) -> WorldPos {
        WorldPos::clamped(x * UNITS_PER_TILE, y * UNITS_PER_TILE)
    }

    fn ground_item(claim: DropClaim, position: WorldPos, map: MapNumber) -> WorldItem {
        WorldItem {
            instance: item(9),
            position,
            map,
            despawn: Tick(1200),
            claim,
        }
    }

    /// The stock drop for the pickup tests: unclaimed, at tile (10, 10) on
    /// map 3, so a same-position picker is trivially in reach.
    fn free_drop() -> WorldItem {
        ground_item(DropClaim::Unclaimed, tile_pos(10, 10), MapNumber(3))
    }

    #[test]
    fn pickup_reject_reassembles_the_untouched_world_item() {
        let occupied = place_item(
            Inventory::empty(4, 4),
            PlaceIntent {
                anchor: cell(0, 0),
                footprint: footprint(4, 4),
                item: item(1),
            },
        )
        .0;
        let world_item = free_drop();
        let (inventory, outcome) = pickup(
            world_item,
            occupied,
            cell(0, 0),
            footprint(2, 2),
            tile_pos(10, 10),
            MapNumber(3),
            PickerStanding::Stranger,
            Tick(0),
        );
        match outcome {
            PickupOutcome::Rejected { reason, item } => {
                assert_eq!(reason, PlacementRejection::CellsOccupied);
                assert_eq!(item, free_drop());
            }
            PickupOutcome::PickedUp { .. }
            | PickupOutcome::OutOfReach { .. }
            | PickupOutcome::Refused { .. } => panic!("full grid should reject"),
        }
        // The inventory is unchanged (still just the 4x4 filler).
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn pickup_success_consumes_the_world_item() {
        let (inventory, outcome) = pickup(
            free_drop(),
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(10, 10),
            MapNumber(3),
            PickerStanding::Stranger,
            Tick(0),
        );
        assert_eq!(outcome, PickupOutcome::PickedUp { at: cell(0, 0) });
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn pickup_beyond_three_tiles_is_out_of_reach_with_the_item_handed_back() {
        let (inventory, outcome) = pickup(
            free_drop(),
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(14, 10),
            MapNumber(3),
            PickerStanding::Stranger,
            Tick(0),
        );
        assert_eq!(outcome, PickupOutcome::OutOfReach { item: free_drop() });
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn pickup_at_three_tiles_or_less_proceeds_to_place() {
        for actor_pos in [tile_pos(13, 10), tile_pos(12, 10), tile_pos(10, 10)] {
            let (inventory, outcome) = pickup(
                free_drop(),
                Inventory::empty(8, 8),
                cell(0, 0),
                footprint(1, 1),
                actor_pos,
                MapNumber(3),
                PickerStanding::Stranger,
                Tick(0),
            );
            assert_eq!(outcome, PickupOutcome::PickedUp { at: cell(0, 0) });
            assert_eq!(inventory.placed().len(), 1);
        }
    }

    #[test]
    fn pickup_on_another_map_is_out_of_reach() {
        let (_, outcome) = pickup(
            free_drop(),
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(10, 10),
            MapNumber(0),
            PickerStanding::Stranger,
            Tick(0),
        );
        assert_eq!(outcome, PickupOutcome::OutOfReach { item: free_drop() });
    }

    #[test]
    fn reach_is_gated_before_the_claim_window() {
        let claimed = ground_item(
            DropClaim::Claimed { until: Tick(200) },
            tile_pos(10, 10),
            MapNumber(3),
        );
        let (_, outcome) = pickup(
            claimed.clone(),
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(14, 10),
            MapNumber(3),
            PickerStanding::Stranger,
            Tick(0),
        );
        assert_eq!(outcome, PickupOutcome::OutOfReach { item: claimed });
    }

    #[test]
    fn an_owner_picks_a_claimed_drop_inside_the_window() {
        let claimed = ground_item(
            DropClaim::Claimed { until: Tick(200) },
            tile_pos(10, 10),
            MapNumber(3),
        );
        let (inventory, outcome) = pickup(
            claimed,
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(10, 10),
            MapNumber(3),
            PickerStanding::Owner,
            Tick(0),
        );
        assert_eq!(outcome, PickupOutcome::PickedUp { at: cell(0, 0) });
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn a_stranger_is_refused_inside_the_window_with_the_item_handed_back() {
        let claimed = ground_item(
            DropClaim::Claimed { until: Tick(200) },
            tile_pos(10, 10),
            MapNumber(3),
        );
        let (inventory, outcome) = pickup(
            claimed.clone(),
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(10, 10),
            MapNumber(3),
            PickerStanding::Stranger,
            Tick(199),
        );
        assert_eq!(outcome, PickupOutcome::Refused { item: claimed });
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn anyone_picks_a_claimed_drop_once_the_window_elapsed() {
        let claimed = ground_item(
            DropClaim::Claimed { until: Tick(200) },
            tile_pos(10, 10),
            MapNumber(3),
        );
        let (_, outcome) = pickup(
            claimed,
            Inventory::empty(8, 8),
            cell(0, 0),
            footprint(1, 1),
            tile_pos(10, 10),
            MapNumber(3),
            PickerStanding::Stranger,
            Tick(200),
        );
        assert_eq!(outcome, PickupOutcome::PickedUp { at: cell(0, 0) });
    }

    #[test]
    fn an_unclaimed_drop_is_picked_up_by_anyone_at_any_tick() {
        for standing in [PickerStanding::Owner, PickerStanding::Stranger] {
            let (_, outcome) = pickup(
                free_drop(),
                Inventory::empty(8, 8),
                cell(0, 0),
                footprint(1, 1),
                tile_pos(10, 10),
                MapNumber(3),
                standing,
                Tick(0),
            );
            assert_eq!(outcome, PickupOutcome::PickedUp { at: cell(0, 0) });
        }
    }

    #[test]
    fn pickup_reach_is_three_tiles() {
        assert_eq!(pickup_reach(), Radius::from_tiles(3));
    }

    fn pile(amount: u64) -> WorldZen {
        WorldZen {
            amount: Zen(amount),
            position: tile_pos(10, 10),
            map: MapNumber(3),
            despawn: Tick(1200),
        }
    }

    #[test]
    fn pickup_zen_merges_the_whole_pile() {
        let (balance, outcome) = pickup_zen(
            pile(40_000),
            CarriedZen::new(250_000).unwrap(),
            tile_pos(12, 10),
            MapNumber(3),
        );
        assert_eq!(balance, CarriedZen::new(290_000).unwrap());
        assert_eq!(outcome, ZenPickupOutcome::PickedUp);
    }

    #[test]
    fn pickup_zen_over_cap_hands_the_untouched_pile_back() {
        let (balance, outcome) = pickup_zen(
            pile(2),
            CarriedZen::new(1_999_999_999).unwrap(),
            tile_pos(10, 10),
            MapNumber(3),
        );
        assert_eq!(balance, CarriedZen::new(1_999_999_999).unwrap());
        assert_eq!(outcome, ZenPickupOutcome::OverCap { world_zen: pile(2) });
    }

    #[test]
    fn pickup_zen_beyond_three_tiles_or_off_map_is_out_of_reach() {
        let balance = CarriedZen::new(250_000).unwrap();
        for (actor_pos, actor_map) in [
            (tile_pos(14, 10), MapNumber(3)),
            (tile_pos(10, 10), MapNumber(0)),
        ] {
            let (unchanged, outcome) = pickup_zen(pile(40_000), balance, actor_pos, actor_map);
            assert_eq!(unchanged, balance);
            assert_eq!(
                outcome,
                ZenPickupOutcome::OutOfReach {
                    world_zen: pile(40_000)
                }
            );
        }
    }

    #[test]
    fn zen_pickup_outcome_wire_round_trips() {
        assert_eq!(
            serde_json::to_string(&ZenPickupOutcome::PickedUp).unwrap(),
            r#"{"kind":"picked_up"}"#
        );
        let rejected = ZenPickupOutcome::OverCap { world_zen: pile(7) };
        let json = serde_json::to_string(&rejected).unwrap();
        assert_eq!(
            serde_json::from_str::<ZenPickupOutcome>(&json).unwrap(),
            rejected
        );
        let out_of_reach = ZenPickupOutcome::OutOfReach { world_zen: pile(7) };
        let json = serde_json::to_string(&out_of_reach).unwrap();
        assert!(json.starts_with(r#"{"kind":"out_of_reach""#));
        assert_eq!(
            serde_json::from_str::<ZenPickupOutcome>(&json).unwrap(),
            out_of_reach
        );
    }

    #[test]
    fn pickup_outcome_new_arms_wire_round_trip() {
        let out_of_reach = PickupOutcome::OutOfReach { item: free_drop() };
        let json = serde_json::to_string(&out_of_reach).unwrap();
        assert!(json.starts_with(r#"{"kind":"out_of_reach""#));
        assert_eq!(
            serde_json::from_str::<PickupOutcome>(&json).unwrap(),
            out_of_reach
        );
        let refused = PickupOutcome::Refused { item: free_drop() };
        let json = serde_json::to_string(&refused).unwrap();
        assert!(json.starts_with(r#"{"kind":"refused""#));
        assert_eq!(
            serde_json::from_str::<PickupOutcome>(&json).unwrap(),
            refused
        );
    }

    #[test]
    fn unequip_empty_slot_reports_slot_empty() {
        let (equipment, outcome) = unequip(Equipment::empty(), EquipmentSlot::Boots);
        assert_eq!(outcome, UnequipOutcome::SlotEmpty);
        assert!(equipment.get(EquipmentSlot::Boots).is_none());
    }

    #[test]
    fn unequip_hands_the_item_out() {
        let equipment = Equipment::empty().with(EquipmentSlot::Helm, item(7));
        let (equipment, outcome) = unequip(equipment, EquipmentSlot::Helm);
        match outcome {
            UnequipOutcome::Unequipped { slot, item } => {
                assert_eq!(slot, EquipmentSlot::Helm);
                assert_eq!(item.item.number, 7);
            }
            UnequipOutcome::SlotEmpty => panic!("the slot held an item"),
        }
        assert!(equipment.get(EquipmentSlot::Helm).is_none());
    }

    // --- The eligibility gate (white-box over hand-built wearer + wear
    // columns; hand-derived numbers from the equipment spec §0.5). -----------

    use crate::components::class::ClassSet;
    use crate::components::item_instance::{ExcellentOptions, ExcellentWeaponSet};
    use crate::components::item_options::{ExcellentWeaponOption, NormalOption};
    use crate::components::levels::OptionLevel;
    use crate::data::common::Provenance;
    use crate::data::common::SourceVersion;
    use crate::data::item_definitions::ItemPrice;

    fn dk_only() -> ClassSet {
        ClassSet {
            dark_knight: true,
            ..ClassSet::NONE
        }
    }

    fn wear_columns(level: u16, strength: u16, energy: u16) -> WearRequirements {
        WearRequirements {
            level,
            strength,
            agility: 0,
            vitality: 0,
            energy,
            command: 0,
        }
    }

    fn weapon_definition(drop_level: u8, wear: WearRequirements) -> ItemDefinition {
        ItemDefinition {
            id: ItemRef {
                group: 0,
                number: 1,
            },
            provenance: Provenance {
                source_version: SourceVersion::V075,
                review: None,
            },
            width: 1,
            height: 3,
            drops_from_monsters: true,
            drop_level,
            max_item_level: ItemLevel::new(11).unwrap(),
            durability: 30,
            price: ItemPrice::Formula,
            kind: ItemKind::Weapon {
                handling: WeaponHandling::OneHanded,
                min_damage: 10,
                max_damage: 20,
                attack_speed: 0,
                skill: None,
                classes: dk_only(),
                wear,
            },
        }
    }

    fn knight(level: u16, strength: u16, energy: u16) -> Wearer {
        Wearer {
            class: CharacterClass::DarkKnight,
            level: Level::new(level).unwrap(),
            stats: Stats::Standard {
                strength,
                agility: 0,
                vitality: 0,
                energy,
            },
        }
    }

    #[test]
    fn a_class_outside_the_items_list_is_rejected_class_mismatch() {
        let def = weapon_definition(20, wear_columns(0, 0, 0));
        let elf = Wearer {
            class: CharacterClass::FairyElf,
            ..knight(50, 200, 0)
        };
        assert_eq!(
            eligibility(&elf, &item(1), &def),
            Some(EquipRejection::ClassMismatch)
        );
        assert_eq!(eligibility(&knight(50, 200, 0), &item(1), &def), None);
    }

    #[test]
    fn the_scaled_strength_requirement_is_inclusive() {
        // EQ-GATE-2: base 120 at edl 20 → (3·20·120)/100 + 20 = 92.
        let def = weapon_definition(20, wear_columns(0, 120, 0));
        assert_eq!(
            eligibility(&knight(50, 91, 0), &item(1), &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        assert_eq!(eligibility(&knight(50, 92, 0), &item(1), &def), None);
    }

    #[test]
    fn the_plus_twenty_floor_makes_the_stated_column_never_the_compared_number() {
        // EQ-GATE-3: base 20 at edl 6 → (3·6·20)/100 + 20 = 23.
        let def = weapon_definition(6, wear_columns(0, 20, 0));
        assert_eq!(
            eligibility(&knight(50, 22, 0), &item(1), &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        assert_eq!(eligibility(&knight(50, 23, 0), &item(1), &def), None);
    }

    #[test]
    fn rarity_and_enhancement_raise_the_requirement_through_the_drop_level() {
        // EQ-GATE-4: the 92-requirement weapon re-rolled Excellent +5 →
        // edl 20 + 15 + 25 = 60 → (3·60·120)/100 + 20 = 236.
        let def = weapon_definition(20, wear_columns(0, 120, 0));
        let mut excellent = item(1);
        excellent.level = ItemLevel::new(5).unwrap();
        excellent.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Weapon {
                options: ExcellentWeaponSet::with_first(ExcellentWeaponOption::DamagePct, []),
            },
        };
        assert_eq!(
            eligibility(&knight(50, 235, 0), &excellent, &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        assert_eq!(eligibility(&knight(50, 236, 0), &excellent, &def), None);
    }

    #[test]
    fn the_strength_requirement_gains_four_per_normal_option_level() {
        // EQ-GATE-5: pre-+20 scaled 72 > 0, option level 3 → 72 + 12 + 20 = 104.
        let def = weapon_definition(20, wear_columns(0, 120, 0));
        let mut optioned = item(1);
        optioned.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L3,
        });
        assert_eq!(
            eligibility(&knight(50, 103, 0), &optioned, &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        assert_eq!(eligibility(&knight(50, 104, 0), &optioned, &def), None);
        // The +4·level term never fires when the strength column is 0.
        let no_strength = weapon_definition(20, wear_columns(0, 0, 0));
        let mut optioned = item(1);
        optioned.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L3,
        });
        assert_eq!(eligibility(&knight(1, 0, 0), &optioned, &no_strength), None);
    }

    #[test]
    fn a_zero_column_is_no_requirement_never_scaled_to_twenty() {
        // EQ-GATE-6: an all-0-column item equips for a level-1 minimal wearer.
        let def = weapon_definition(20, wear_columns(0, 0, 0));
        assert_eq!(eligibility(&knight(1, 0, 0), &item(1), &def), None);
    }

    #[test]
    fn the_gate_reads_total_stats_so_worn_gear_qualifies_further_gear() {
        // EQ-GATE-7: a base-90 wearer with a +2 strength ring presents total
        // 92 — a synthetic total-stats wearer, the seam ancient stat gear
        // fills; the gate itself never aggregates.
        let def = weapon_definition(20, wear_columns(0, 120, 0));
        let base_only = knight(50, 90, 0);
        assert_eq!(
            eligibility(&base_only, &item(1), &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        let with_ring_total = knight(50, 92, 0);
        assert_eq!(eligibility(&with_ring_total, &item(1), &def), None);
    }

    #[test]
    fn the_level_requirement_compares_raw_and_inclusive() {
        // EQ-GATE-8: level 180 wings — 179 rejected, 180 equipped; the level
        // column never scales with drop level or rarity.
        let def = weapon_definition(100, wear_columns(180, 0, 0));
        assert_eq!(
            eligibility(&knight(179, 0, 0), &item(1), &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        assert_eq!(eligibility(&knight(180, 0, 0), &item(1), &def), None);
    }

    #[test]
    fn the_energy_column_scales_with_multiplier_four() {
        // §0.5 A6: base energy 40 at edl 20 → (4·20·40)/100 + 20 = 52.
        let def = weapon_definition(20, wear_columns(0, 0, 40));
        assert_eq!(
            eligibility(&knight(50, 0, 51), &item(1), &def),
            Some(EquipRejection::RequirementsNotMet)
        );
        assert_eq!(eligibility(&knight(50, 0, 52), &item(1), &def), None);
    }

    #[test]
    fn hand_pair_legality_admits_only_one_handers_and_launcher_with_ammo() {
        use HandOccupation::{Ammunition, Launcher, OneHand, TwoHands};
        assert!(!hand_pair_conflicts(OneHand, OneHand));
        assert!(!hand_pair_conflicts(Launcher, Ammunition));
        assert!(!hand_pair_conflicts(Ammunition, Launcher));
        // A sword cannot sit beside arrows, arrows cannot pair with arrows,
        // and a launcher still conflicts with everything but ammunition.
        assert!(hand_pair_conflicts(OneHand, Ammunition));
        assert!(hand_pair_conflicts(Ammunition, OneHand));
        assert!(hand_pair_conflicts(Ammunition, Ammunition));
        assert!(hand_pair_conflicts(Launcher, OneHand));
        assert!(hand_pair_conflicts(Launcher, Launcher));
        assert!(hand_pair_conflicts(TwoHands, Ammunition));
        assert!(hand_pair_conflicts(TwoHands, OneHand));
        assert!(hand_pair_conflicts(OneHand, TwoHands));
    }
}
