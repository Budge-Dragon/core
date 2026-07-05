//! NPC-shop decisions: buy, sell, repair, and repair-all — pure functions
//! over the resolved shelf catalog, the inventory and equipment containers,
//! and a [`CarriedZen`] balance. Every service is `(state, intent) ->
//! (state, outcome)` with no RNG anywhere in the family; the client never
//! states a price, a destination, or a result. [`RepairSite`] is read by the
//! repair decisions (range rule) and projected onto the price port's rate
//! axis ([`crate::services::price::RepairRate`]). Repairability
//! is structural: only the wear-gauge kinds minus pets and ammo carry a
//! repair path, so a stack never reaches the price math.

use core::num::NonZeroU8;

use crate::components::equipment::{Equipment, EquipmentSlot};
use crate::components::inventory::{AbsorbRejection, Cell, Footprint, Inventory};
use crate::components::item_instance::{
    CraftedAugment, Durability, DurabilityError, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use crate::components::item_quality::ItemRarity;
use crate::components::item_ref::ItemRef;
use crate::components::spatial::{Radius, WorldPos};
use crate::components::units::{CarriedZen, CreditOutcome, DebitOutcome, ItemLevel, Zen};
use crate::data::atlas::{Atlas, ShelfEntryView, ShopView};
use crate::data::item_definitions::ItemKind;
use crate::data::npc_shops::{ShelfSlot, ShelfStock};
use crate::events::shop::{
    BuyOutcome, RepairAllOutcome, RepairOutcome, SellOutcome, SlotRepair, SlotRepairResult,
};
use crate::services::item_rules::max_durability;
use crate::services::price::{RepairRate, buying_price, repair_price, selling_price};

/// Where a repair happens — an enum, never a bool: the merchant position
/// exists only on the ranged variant, so a self-repair with a range check is
/// unrepresentable. Selects the range rule in the repair services (at-NPC
/// requires a merchant within reach; self-repair has no range rule) and
/// projects onto the price port's [`crate::services::price::RepairRate`]
/// (self-repair pays the classic 5/2 surcharge).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepairSite {
    /// At a merchant NPC: the base rate, gated on the buyer being in range of
    /// the merchant.
    AtNpc {
        /// The merchant's live position the range rule is checked against.
        merchant_pos: WorldPos,
    },
    /// Alone in the field: no range rule, the 5/2 price surcharge.
    SelfRepair,
}

/// The addressed item and its own container for a single repair — the
/// addressed-container sum. Fusing the address with its container makes
/// "repair a stored cell of the equipment" unrepresentable; the service hands
/// the touched container back inside the same sum, so the host persists
/// exactly what it sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairSubject {
    /// A worn item, addressed by its equipment slot.
    Equipped {
        /// The equipment set holding the item.
        equipment: Equipment,
        /// The addressed slot.
        slot: EquipmentSlot,
    },
    /// A stored item, addressed by any inventory cell its footprint covers.
    Stored {
        /// The inventory holding the item.
        inventory: Inventory,
        /// The addressed cell.
        cell: Cell,
    },
}

/// The merchant interaction reach shared by buy, sell, and at-NPC repair.
// Design pin, NOT W-SRC: classic had no range rule and OpenMU performs zero
// position validation on the whole shop path (consult §A.1.3, its own source
// calls the omission a gap); the 3-tile constant matches the era's pickup
// reach (consult §F.3.11) and is ours.
fn merchant_reach() -> Radius {
    Radius::from_tiles(3)
}

/// Buys the shelf entry anchored at `slot`. Check order: range → shelf
/// resolution → destination → zen, with space-before-zen on the new-item
/// path. A stack entry with a same-identity, same-level stored stack absorbs
/// the whole pack (a first-class [`BuyOutcome::Merged`] success) — the merge
/// is proven before zen is debited, so a feasible merge is gated by zen
/// alone; a merge that would overflow the stack's cap falls through to the
/// new-item path with zen untouched — never a partial merge. The price is
/// the shelf template's own [`buying_price`], no merchant markup; the
/// catalog is never mutated.
#[must_use]
pub fn buy(
    inventory: Inventory,
    zen: CarriedZen,
    shop: ShopView<'_>,
    slot: ShelfSlot,
    buyer_pos: WorldPos,
    merchant_pos: WorldPos,
) -> (Inventory, BuyOutcome) {
    if !buyer_pos.within_range(merchant_pos, merchant_reach()) {
        return (inventory, BuyOutcome::OutOfRange);
    }
    let Some(entry) = shop.entry(slot) else {
        return (inventory, BuyOutcome::UnknownShelfSlot);
    };
    let template = materialize(&entry);
    let price = buying_price(entry.def, &template);

    let inventory = if let ShelfStock::Stack { pieces } = entry.stock {
        match merge_anchor(&inventory, entry.def.id, template.level) {
            // Prove-then-debit: the absorb runs on a fork of the pre-merge
            // state, so the proof costs nothing to refuse — a zen refusal
            // hands back exactly what the buyer held.
            Some(anchor) => match inventory.clone().absorb(anchor, *pieces) {
                // The merge commits, so zen is the only gate left on this
                // path; an unpayable pack discards the merged fork.
                Ok(merged) => match zen.debit(price) {
                    DebitOutcome::Debited { balance } => {
                        return (
                            merged,
                            BuyOutcome::Merged {
                                at: anchor,
                                balance,
                            },
                        );
                    }
                    DebitOutcome::Insufficient { .. } => {
                        return (inventory, BuyOutcome::InsufficientZen);
                    }
                },
                // Either rejection is the same total reading — no merge
                // commits here: `WouldOverflow` because the whole pack must
                // fit (never split), `NoItemAtCell` because no candidate
                // covers the cell (the no-candidate continuation, same as
                // the scan finding nothing). Both fall to the new-item path
                // with zen untouched, so the pinned space-before-zen order
                // governs.
                Err((_, AbsorbRejection::NoItemAtCell | AbsorbRejection::WouldOverflow)) => {
                    inventory
                }
            },
            None => inventory,
        }
    } else {
        inventory
    };

    // New-item path: space before zen.
    let Ok(footprint) = Footprint::new(entry.def.width, entry.def.height) else {
        // A zero-dimension definition (parse-unreachable on the shipped
        // catalog) offers no placeable footprint — no fitting anchor exists.
        return (inventory, BuyOutcome::InventoryFull);
    };
    let Some(anchor) = inventory.first_fit(footprint) else {
        return (inventory, BuyOutcome::InventoryFull);
    };
    let balance = match zen.debit(price) {
        DebitOutcome::Debited { balance } => balance,
        DebitOutcome::Insufficient { .. } => return (inventory, BuyOutcome::InsufficientZen),
    };
    match inventory.place(anchor, footprint, template) {
        Ok(inventory) => (
            inventory,
            BuyOutcome::NewItem {
                at: anchor,
                balance,
            },
        ),
        // `first_fit` proved the region free; the placement's own rejection
        // re-answers the space question — total, never a panic.
        Err((inventory, _bounced, _reason)) => (inventory, BuyOutcome::InventoryFull),
    }
}

/// Materializes a fresh instance from a shelf entry — the configured facts
/// per family: gear at full durability for its level (fresh from the shop,
/// never the worn definition base), a stack at its pack's piece count, ammo
/// as one full quiver, a single at its durability-1 gauge.
fn materialize(entry: &ShelfEntryView<'_>) -> ItemInstance {
    let (luck, skill, normal_option, durability) = match entry.stock {
        ShelfStock::Gear {
            luck,
            skill,
            option,
        } => (
            *luck,
            *skill,
            *option,
            Durability::full(max_durability(
                entry.def.durability,
                entry.level,
                ItemRarity::Normal,
            )),
        ),
        ShelfStock::Stack { pieces } => (
            LuckRoll::Plain,
            SkillRoll::NoSkill,
            None,
            stack_gauge(*pieces, entry.def.durability),
        ),
        // The quiver's round count and the single's durability-1 gauge are
        // both the definition's own durability, proven at Atlas parse.
        ShelfStock::Quiver | ShelfStock::Single => (
            LuckRoll::Plain,
            SkillRoll::NoSkill,
            None,
            Durability::full(entry.def.durability),
        ),
    };
    ItemInstance {
        item: entry.def.id,
        level: ItemLevel::from(entry.level),
        roll: RarityRoll::Normal,
        normal_option,
        luck,
        skill,
        durability,
        augment: CraftedAugment::None,
    }
}

/// The shelf pack's gauge: `pieces` of the definition's stack cap. Atlas
/// parse proved `pieces <= cap`; the gauge constructor re-states the proof,
/// and the parse-unreachable overflow arm answers with the full-cap stack —
/// a defined total answer, never a panic.
fn stack_gauge(pieces: NonZeroU8, cap: u8) -> Durability {
    match Durability::new(pieces.get(), cap) {
        Ok(gauge) => gauge,
        Err(DurabilityError::CurrentExceedsMax { .. }) => Durability::full(cap),
    }
}

/// The anchor of the first stored stack matching the shelf entry's identity
/// and plus-level — the buy merge candidate, scanned in placement order.
fn merge_anchor(inventory: &Inventory, item: ItemRef, level: ItemLevel) -> Option<Cell> {
    inventory
        .placed()
        .iter()
        .find(|placed| placed.item.item == item && placed.item.level == level)
        .map(|placed| placed.anchor)
}

/// Sells the item covering `cell` to a merchant: the item is priced by
/// [`selling_price`], destroyed by value, and the proceeds credited.
/// Merchants never resell and any of them is a sink; zero-price sales
/// succeed and still destroy. Equipped items are structurally unsellable —
/// the address is an inventory cell and equipment has no cell, so no guard
/// exists. A credit past the carry cap is [`SellOutcome::WalletFull`]: the
/// item is kept and the inventory untouched (the occupant is priced without
/// removal and removed only on a successful credit).
#[must_use]
pub fn sell(
    inventory: Inventory,
    zen: CarriedZen,
    cell: Cell,
    buyer_pos: WorldPos,
    merchant_pos: WorldPos,
    atlas: &Atlas,
) -> (Inventory, SellOutcome) {
    if !buyer_pos.within_range(merchant_pos, merchant_reach()) {
        return (inventory, SellOutcome::OutOfRange);
    }
    let Some(occupant) = inventory.occupant(cell) else {
        return (inventory, SellOutcome::NoItemAtCell);
    };
    // An occupant the atlas cannot identify is nothing a sale can price —
    // the equip service's unknown-occupant genuine-optionality fold, a
    // no-action refusal that keeps the item.
    let Some(def) = atlas.item(occupant.item.item) else {
        return (inventory, SellOutcome::NoItemAtCell);
    };
    let proceeds = selling_price(def, &occupant.item);
    let balance = match zen.credit(proceeds) {
        CreditOutcome::Credited { balance } => balance,
        CreditOutcome::OverCap { .. } => return (inventory, SellOutcome::WalletFull),
    };
    match inventory.remove(cell) {
        // The removed instance is consumed by value here — no buyback exists.
        Ok((inventory, _destroyed)) => (inventory, SellOutcome::Sold { proceeds, balance }),
        // `occupant` proved the cell covered; the removal's own rejection
        // re-answers the presence question — total, never a panic.
        Err((inventory, _reason)) => (inventory, SellOutcome::NoItemAtCell),
    }
}

/// Repairs the addressed item to full and debits the price. The kind gate is
/// what a spare potion stack, quiver, or pet dies on; a full gauge is a
/// typed no-op with no charge. [`RepairSite::AtNpc`] requires the merchant in
/// reach; [`RepairSite::SelfRepair`] needs neither and prices at the 5/2
/// surcharge. The touched container threads back inside the returned
/// [`RepairSubject`].
#[must_use]
pub fn repair(
    subject: RepairSubject,
    zen: CarriedZen,
    site: RepairSite,
    buyer_pos: WorldPos,
    atlas: &Atlas,
) -> (RepairSubject, RepairOutcome) {
    if let RepairSite::AtNpc { merchant_pos } = site {
        if !buyer_pos.within_range(merchant_pos, merchant_reach()) {
            return (subject, RepairOutcome::OutOfRange);
        }
    }
    match subject {
        RepairSubject::Equipped { equipment, slot } => {
            repair_equipped(equipment, slot, zen, site, atlas)
        }
        RepairSubject::Stored { inventory, cell } => {
            repair_stored(inventory, cell, zen, site, atlas)
        }
    }
}

/// The single repair of a worn slot: one lookup takes the item out, every
/// verdict puts it back — repaired or untouched.
fn repair_equipped(
    equipment: Equipment,
    slot: EquipmentSlot,
    zen: CarriedZen,
    site: RepairSite,
    atlas: &Atlas,
) -> (RepairSubject, RepairOutcome) {
    let (stripped, taken) = equipment.without(slot);
    let Some(item) = taken else {
        return (
            RepairSubject::Equipped {
                equipment: stripped,
                slot,
            },
            RepairOutcome::Empty,
        );
    };
    let (item, outcome) = match charge(&item, zen, site, atlas) {
        ChargeDecision::Charged { cost, balance, .. } => {
            let mut item = item;
            item.durability = Durability::full(item.durability.max());
            (item, RepairOutcome::Repaired { cost, balance })
        }
        ChargeDecision::Unaffordable { .. } => (item, RepairOutcome::InsufficientZen),
        ChargeDecision::AlreadyFull => (item, RepairOutcome::AlreadyFull),
        ChargeDecision::NotRepairable => (item, RepairOutcome::NotRepairableKind),
    };
    (
        RepairSubject::Equipped {
            equipment: stripped.with(slot, item),
            slot,
        },
        outcome,
    )
}

/// The single repair of a stored cell: the occupant is gated and priced by
/// borrow, and the gauge refill is the stack-absorb primitive raising the
/// gauge by exactly its own shortfall.
fn repair_stored(
    inventory: Inventory,
    cell: Cell,
    zen: CarriedZen,
    site: RepairSite,
    atlas: &Atlas,
) -> (RepairSubject, RepairOutcome) {
    let Some(occupant) = inventory.occupant(cell) else {
        return (
            RepairSubject::Stored { inventory, cell },
            RepairOutcome::Empty,
        );
    };
    let decision = charge(&occupant.item, zen, site, atlas);
    let (inventory, outcome) = match decision {
        ChargeDecision::Charged {
            missing,
            cost,
            balance,
        } => match inventory.absorb(cell, missing) {
            Ok(inventory) => (inventory, RepairOutcome::Repaired { cost, balance }),
            // The occupant was just located and `missing` is its own gauge
            // shortfall; each rejection re-answers its own question — total,
            // never a panic.
            Err((inventory, AbsorbRejection::NoItemAtCell)) => (inventory, RepairOutcome::Empty),
            Err((inventory, AbsorbRejection::WouldOverflow)) => {
                (inventory, RepairOutcome::AlreadyFull)
            }
        },
        ChargeDecision::Unaffordable { .. } => (inventory, RepairOutcome::InsufficientZen),
        ChargeDecision::AlreadyFull => (inventory, RepairOutcome::AlreadyFull),
        ChargeDecision::NotRepairable => (inventory, RepairOutcome::NotRepairableKind),
    };
    (RepairSubject::Stored { inventory, cell }, outcome)
}

/// The classic repair-all slot walk, in order. `Pet` is deliberately absent:
/// a pet's gauge is its life, never mended for zen.
// W-SRC: the era 0–11 equipment-slot walk of the repair-all path (consult
// §D.1.2), the pet slot skipped by the kind rule.
const REPAIR_ALL_ORDER: [EquipmentSlot; 11] = [
    EquipmentSlot::LeftHand,
    EquipmentSlot::RightHand,
    EquipmentSlot::Helm,
    EquipmentSlot::Armor,
    EquipmentSlot::Pants,
    EquipmentSlot::Gloves,
    EquipmentSlot::Boots,
    EquipmentSlot::Wings,
    EquipmentSlot::Pendant,
    EquipmentSlot::Ring1,
    EquipmentSlot::Ring2,
];

/// Repairs every worn slot in the classic order, pricing and paying each
/// individually; the first unaffordable slot stops the walk with every
/// earlier repair kept and paid. Range is checked once before the walk
/// ([`RepairSite::AtNpc`] only). The outcome reports every walked slot.
#[must_use]
pub fn repair_all(
    equipment: Equipment,
    zen: CarriedZen,
    site: RepairSite,
    buyer_pos: WorldPos,
    atlas: &Atlas,
) -> (Equipment, RepairAllOutcome) {
    if let RepairSite::AtNpc { merchant_pos } = site {
        if !buyer_pos.within_range(merchant_pos, merchant_reach()) {
            return (equipment, RepairAllOutcome::OutOfRange);
        }
    }
    let mut equipment = equipment;
    let mut balance = zen;
    let mut slots = Vec::new();
    for slot in REPAIR_ALL_ORDER {
        let (stripped, taken) = equipment.without(slot);
        let Some(item) = taken else {
            equipment = stripped;
            slots.push(SlotRepair {
                slot,
                result: SlotRepairResult::Empty,
            });
            continue;
        };
        match charge(&item, balance, site, atlas) {
            ChargeDecision::Charged {
                cost,
                balance: paid,
                ..
            } => {
                let mut item = item;
                item.durability = Durability::full(item.durability.max());
                equipment = stripped.with(slot, item);
                balance = paid;
                slots.push(SlotRepair {
                    slot,
                    result: SlotRepairResult::Repaired { cost },
                });
            }
            ChargeDecision::Unaffordable { cost } => {
                equipment = stripped.with(slot, item);
                slots.push(SlotRepair {
                    slot,
                    result: SlotRepairResult::Unaffordable { cost },
                });
                // The first unaffordable slot stops the walk; earlier slots
                // stay repaired and paid.
                return (equipment, RepairAllOutcome::Walked { slots, balance });
            }
            ChargeDecision::AlreadyFull => {
                equipment = stripped.with(slot, item);
                slots.push(SlotRepair {
                    slot,
                    result: SlotRepairResult::AlreadyFull,
                });
            }
            // A worn occupant is structurally repairable (the pet slot is
            // not walked; ammo cannot be worn) — only an occupant the atlas
            // cannot identify lands here, nothing the walk can price, folded
            // to the no-charge empty report (the equip-service
            // unknown-occupant precedent).
            ChargeDecision::NotRepairable => {
                equipment = stripped.with(slot, item);
                slots.push(SlotRepair {
                    slot,
                    result: SlotRepairResult::Empty,
                });
            }
        }
    }
    (equipment, RepairAllOutcome::Walked { slots, balance })
}

/// The shared gate-and-price verdict of the repair paths over a borrowed
/// occupant: resolve, kind-gate, already-full short-circuit, price, debit.
enum ChargeDecision {
    /// The repair is payable: the gauge shortfall, its price, and the balance
    /// after the debit.
    Charged {
        /// How far the gauge is from full — always nonzero here.
        missing: NonZeroU8,
        /// The debited repair price.
        cost: Zen,
        /// The balance after the debit.
        balance: CarriedZen,
    },
    /// The price exceeds the balance; nothing changed.
    Unaffordable {
        /// The unpayable repair price.
        cost: Zen,
    },
    /// The gauge is already full — no charge, nothing to do.
    AlreadyFull,
    /// No repair path: a non-repairable kind, or an occupant the atlas
    /// cannot identify (unknown is not known-repairable — the
    /// genuine-optionality fold).
    NotRepairable,
}

/// Gates and prices one repair candidate. The kind gate runs first (a full
/// potion stack is still not repairable), the full-gauge short-circuit
/// second (pricing only ever sees a damaged item), the typed debit last.
fn charge(item: &ItemInstance, zen: CarriedZen, site: RepairSite, atlas: &Atlas) -> ChargeDecision {
    let Some(def) = atlas.item(item.item) else {
        return ChargeDecision::NotRepairable;
    };
    if !is_repairable_kind(&def.kind) {
        return ChargeDecision::NotRepairable;
    }
    let shortfall = item
        .durability
        .max()
        .saturating_sub(item.durability.current());
    let Some(missing) = NonZeroU8::new(shortfall) else {
        return ChargeDecision::AlreadyFull;
    };
    let rate = match site {
        RepairSite::AtNpc { .. } => RepairRate::AtNpc,
        RepairSite::SelfRepair => RepairRate::SelfRepair,
    };
    let cost = repair_price(def, item, rate);
    match zen.debit(cost) {
        DebitOutcome::Debited { balance } => ChargeDecision::Charged {
            missing,
            cost,
            balance,
        },
        DebitOutcome::Insufficient { .. } => ChargeDecision::Unaffordable { cost },
    }
}

/// Whether a kind carries a repair path: the wear-gauge kinds minus `Pet`
/// (its gauge is its life), `Arrows`, and `Bolts` (ammo is bought, not
/// mended). Total over [`ItemKind`] — stacks, jewels, and inert kinds never
/// reach the price math, which kills the negative-price stack exploit by
/// shape rather than by guard.
fn is_repairable_kind(kind: &ItemKind) -> bool {
    match kind {
        ItemKind::Weapon { .. }
        | ItemKind::Bow { .. }
        | ItemKind::Crossbow { .. }
        | ItemKind::Staff { .. }
        | ItemKind::Shield { .. }
        | ItemKind::Helm { .. }
        | ItemKind::BodyArmor { .. }
        | ItemKind::Pants { .. }
        | ItemKind::Gloves { .. }
        | ItemKind::Boots { .. }
        | ItemKind::Wings { .. }
        | ItemKind::Ring { .. }
        | ItemKind::Pendant { .. }
        | ItemKind::TransformationRing { .. } => true,
        ItemKind::Pet { .. }
        | ItemKind::Arrows { .. }
        | ItemKind::Bolts { .. }
        | ItemKind::Orb { .. }
        | ItemKind::SkillScroll { .. }
        | ItemKind::Jewel { .. }
        | ItemKind::Consumable { .. }
        | ItemKind::LuckyBox
        | ItemKind::EventTicket { .. }
        | ItemKind::MixMaterial
        | ItemKind::StatFruit => false,
    }
}
