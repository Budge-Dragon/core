//! End-to-end shop flows over the real regenerated `/data`: buy (new-item,
//! whole-stack merge, every failure, the space-before-zen order), sell
//! (destroy-by-value, zero-price success, wallet-full keeps the item), repair
//! (equipped and stored, the kind gate, the site multipliers, the broken
//! anchors), repair-all (classic slot order, stop-at-first-unaffordable with
//! earlier repairs kept), zen pickup at the carry-cap edges, the golden
//! per-entry shelf price sweep across all eleven merchants, and the no-RNG
//! determinism of the whole family.
//!
//! The dataset rides in through the shared `common/dataset` harness — the
//! parsed-[`Atlas`] port [`real_atlas`]; load failures route through
//! [`or_abort`] so no banned suppressor is needed outside a `#[test]` body.

#[path = "common/dataset.rs"]
mod dataset;

use dataset::{or_abort, real_atlas};

use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::spatial::WorldPos;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{CarriedZen, ItemLevel, MapNumber, Tick, Zen};
use mu_core::data::atlas::{Atlas, ShopView};
use mu_core::data::common::MonsterNumber;
use mu_core::data::npc_shops::ShelfSlot;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::shop::{
    BuyOutcome, RepairAllOutcome, RepairOutcome, SellOutcome, SlotRepair, SlotRepairResult,
};
use mu_core::services::inventory::{ZenPickupOutcome, pickup_zen};
use mu_core::services::shop::{RepairSite, RepairSubject, buy, repair, repair_all, sell};

// --- Shared fixtures. --------------------------------------------------------

/// The eleven era merchant NPC numbers.
const MERCHANTS: [u16; 11] = [242, 243, 244, 245, 246, 248, 250, 251, 253, 254, 255];

/// The classic 8-column main bag grain used by every flow here.
fn bag() -> Inventory {
    Inventory::empty(15, 8)
}

fn zen(value: u64) -> CarriedZen {
    or_abort(CarriedZen::new(value))
}

fn cell(row: u8, col: u8) -> Cell {
    Cell { row, col }
}

fn slot(byte: u8) -> ShelfSlot {
    or_abort(ShelfSlot::new(byte))
}

/// The merchant stands at tile (10, 10); 3 tiles is in reach, 4 is out.
fn merchant_pos() -> WorldPos {
    TileCoord::new(10, 10).to_world()
}

fn in_range_pos() -> WorldPos {
    TileCoord::new(10, 13).to_world()
}

fn out_of_range_pos() -> WorldPos {
    TileCoord::new(10, 14).to_world()
}

/// A bare instance of `(group, number)` at a given plus-level and gauge.
fn worn(group: u8, number: u16, level: u8, current: u8, max: u8) -> ItemInstance {
    ItemInstance {
        item: ItemRef { group, number },
        level: or_abort(ItemLevel::new(level)),
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: or_abort(Durability::new(current, max)),
        augment: CraftedAugment::None,
    }
}

fn footprint(width: u8, height: u8) -> Footprint {
    or_abort(Footprint::new(width, height))
}

fn shop_of(atlas: &Atlas, npc: u16) -> ShopView<'_> {
    match atlas.shop(MonsterNumber(npc)) {
        Some(shop) => shop,
        None => or_abort(Err(format!("merchant {npc} has no shop"))),
    }
}

// Real-data anchors used across the flows (defs verified by the catalog
// suite): Gladius (0,6) durability 30, footprint 1x3; Vine Gloves (10,10)
// base durability 22, footprint 2x2, shelved +3 at Elf Lala slot 38; small
// healing potion (14,0) stack cap 3, shelved x1 at slot 0 and x3 at slot 8;
// Arrows (4,15) durability 255, footprint 1x2, shelved at slot 23.

// --- Buy: new-item path. -----------------------------------------------------

#[test]
fn buy_places_a_fresh_full_durability_copy_at_the_first_fit_anchor() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // Vine Gloves +3 (slot 38): price 2,400, base durability 22, +3 max 25.
    let (inventory, outcome) = buy(
        bag(),
        zen(500_000),
        shop,
        slot(38),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(
        outcome,
        BuyOutcome::NewItem {
            at: cell(0, 0),
            balance: zen(497_600),
        }
    );
    let placed = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(
        placed.item.item,
        ItemRef {
            group: 10,
            number: 10
        }
    );
    assert_eq!(placed.item.level, ItemLevel::new(3).unwrap());
    // D9: full durability FOR ITS LEVEL — 25/25, strictly above the base 22.
    assert_eq!(placed.item.durability, Durability::new(25, 25).unwrap());
    assert_eq!(placed.item.luck, LuckRoll::Lucky);
}

#[test]
fn buying_ammo_yields_one_full_quiver_and_never_merges() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // A quiver already stored: K2 — ammo never stack-merges.
    let stored = bag()
        .place(cell(5, 5), footprint(1, 2), worn(4, 15, 0, 255, 255))
        .unwrap();
    let (inventory, outcome) = buy(
        stored,
        zen(1_000),
        shop,
        slot(23),
        in_range_pos(),
        merchant_pos(),
    );
    // Arrows price 70; a fresh full 255-quiver lands at the first fit.
    assert_eq!(
        outcome,
        BuyOutcome::NewItem {
            at: cell(0, 0),
            balance: zen(930),
        }
    );
    let placed = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(placed.item.durability, Durability::full(255));
    assert_eq!(inventory.placed().len(), 2);
}

// --- Buy: stack-merge path. --------------------------------------------------

#[test]
fn a_whole_pack_merge_is_a_first_class_success() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // A stored small-healing stack of 1 (cap 3); the x1 shelf pack (price 20)
    // pours onto it.
    let stored = bag()
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 0, 1, 3))
        .unwrap();
    let (inventory, outcome) = buy(
        stored,
        zen(1_000),
        shop,
        slot(0),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(
        outcome,
        BuyOutcome::Merged {
            at: cell(0, 0),
            balance: zen(980),
        }
    );
    // No new slot: still one placed item, now 2 of 3 pieces.
    assert_eq!(inventory.placed().len(), 1);
    let stack = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(stack.item.durability, Durability::new(2, 3).unwrap());
}

#[test]
fn a_merge_that_would_overflow_falls_to_the_new_item_path() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // 1 stored + the x3 shelf pack (price 60) would exceed the cap of 3.
    let stored = bag()
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 0, 1, 3))
        .unwrap();
    let (inventory, outcome) = buy(
        stored,
        zen(1_000),
        shop,
        slot(8),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(
        outcome,
        BuyOutcome::NewItem {
            at: cell(0, 1),
            balance: zen(940),
        }
    );
    // The original stack is untouched; the fresh pack carries all 3 pieces.
    let original = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(original.item.durability, Durability::new(1, 3).unwrap());
    let fresh = inventory.occupant(cell(0, 1)).unwrap();
    assert_eq!(fresh.item.durability, Durability::new(3, 3).unwrap());
}

#[test]
fn a_different_plus_level_blocks_the_merge() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // A +1 stored stack never absorbs the +0 shelf pack.
    let stored = bag()
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 1, 1, 3))
        .unwrap();
    let (inventory, outcome) = buy(
        stored,
        zen(1_000),
        shop,
        slot(0),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(
        outcome,
        BuyOutcome::NewItem {
            at: cell(0, 1),
            balance: zen(980),
        }
    );
    assert_eq!(inventory.placed().len(), 2);
}

// --- Buy: failures and check order. ------------------------------------------

#[test]
fn buy_failures_leave_the_inventory_and_balance_untouched() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);

    // Out of range (4 tiles), everything else valid.
    let (inventory, outcome) = buy(
        bag(),
        zen(500_000),
        shop,
        slot(38),
        out_of_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::OutOfRange);
    assert!(inventory.placed().is_empty());

    // An empty shelf slot (19 anchors nothing at Elf Lala).
    let (_, outcome) = buy(
        bag(),
        zen(500_000),
        shop,
        slot(19),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::UnknownShelfSlot);

    // A covered NON-anchor cell of a multi-cell entry: Hanzo's Falchion
    // anchors at 76 and covers 84 and 92 — both resolve to nothing.
    let hanzo = shop_of(&atlas, 251);
    for covered in [84u8, 92] {
        let (_, outcome) = buy(
            bag(),
            zen(500_000),
            hanzo,
            slot(covered),
            in_range_pos(),
            merchant_pos(),
        );
        assert_eq!(outcome, BuyOutcome::UnknownShelfSlot, "cell {covered}");
    }

    // No fitting anchor: a 1x1 grid cannot take the 2x2 gloves.
    let (_, outcome) = buy(
        Inventory::empty(1, 1),
        zen(500_000),
        shop,
        slot(38),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::InventoryFull);

    // Room but short zen (price 2,400).
    let (_, outcome) = buy(
        bag(),
        zen(2_399),
        shop,
        slot(38),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::InsufficientZen);
}

#[test]
fn the_new_item_path_checks_space_before_zen() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // Both preconditions fail; the classic order reports the space failure.
    let (_, outcome) = buy(
        Inventory::empty(1, 1),
        zen(0),
        shop,
        slot(38),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::InventoryFull);
}

#[test]
fn the_merge_path_reports_short_zen_as_a_typed_outcome() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    let stored = bag()
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 0, 1, 3))
        .unwrap();
    // The x1 pack costs 20; 19 is short — D4, never a silent no-op.
    let (inventory, outcome) = buy(
        stored,
        zen(19),
        shop,
        slot(0),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::InsufficientZen);
    let stack = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(stack.item.durability, Durability::new(1, 3).unwrap());
}

#[test]
fn an_overflowing_merge_with_short_zen_and_no_space_reports_the_space_verdict() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // A 1-cell grid holding a 1-of-3 stack: the x3 pack (price 60) cannot
    // merge (overflow) and no free anchor exists; 59 zen is also short. The
    // infeasible merge never gates on zen — the fall-through lands on the
    // new-item path where the pinned space-before-zen order reports the
    // space failure.
    let stored = Inventory::empty(1, 1)
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 0, 1, 3))
        .unwrap();
    let (inventory, outcome) = buy(
        stored,
        zen(59),
        shop,
        slot(8),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::InventoryFull);
    let stack = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(stack.item.durability, Durability::new(1, 3).unwrap());
}

#[test]
fn an_overflowing_merge_with_short_zen_and_free_space_gates_on_zen() {
    let atlas = real_atlas();
    let shop = shop_of(&atlas, 242);
    // The same overflowing x3 pack with room to spare: space passes on the
    // new-item path and zen (59 against 60) is the gate that fires.
    let stored = bag()
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 0, 1, 3))
        .unwrap();
    let (inventory, outcome) = buy(
        stored,
        zen(59),
        shop,
        slot(8),
        in_range_pos(),
        merchant_pos(),
    );
    assert_eq!(outcome, BuyOutcome::InsufficientZen);
    assert_eq!(inventory.placed().len(), 1);
    let stack = inventory.occupant(cell(0, 0)).unwrap();
    assert_eq!(stack.item.durability, Durability::new(1, 3).unwrap());
}

// --- Sell. --------------------------------------------------------------------

#[test]
fn sell_destroys_the_item_by_value_and_credits_the_exact_proceeds() {
    let atlas = real_atlas();
    // A full Gladius sells for 820 at any merchant; the address may be any
    // covered cell, not just the anchor.
    let stored = bag()
        .place(cell(0, 0), footprint(1, 3), worn(0, 6, 0, 30, 30))
        .unwrap();
    let (inventory, outcome) = sell(
        stored,
        zen(250_000),
        cell(1, 0),
        in_range_pos(),
        merchant_pos(),
        &atlas,
    );
    assert_eq!(
        outcome,
        SellOutcome::Sold {
            proceeds: Zen(820),
            balance: zen(250_820),
        }
    );
    assert!(inventory.placed().is_empty());
}

#[test]
fn a_zero_price_empty_quiver_still_sells_and_is_destroyed() {
    let atlas = real_atlas();
    let stored = bag()
        .place(cell(0, 0), footprint(1, 2), worn(4, 15, 0, 0, 255))
        .unwrap();
    let (inventory, outcome) = sell(
        stored,
        zen(100),
        cell(0, 0),
        in_range_pos(),
        merchant_pos(),
        &atlas,
    );
    assert_eq!(
        outcome,
        SellOutcome::Sold {
            proceeds: Zen(0),
            balance: zen(100),
        }
    );
    assert!(inventory.placed().is_empty());
}

#[test]
fn a_wallet_overflowing_sale_keeps_the_item_and_the_balance() {
    let atlas = real_atlas();
    let stored = bag()
        .place(cell(0, 0), footprint(1, 3), worn(0, 6, 0, 30, 30))
        .unwrap();
    // The 820 proceeds would push the purse one over the cap.
    let (inventory, outcome) = sell(
        stored,
        zen(CarriedZen::CAP - 819),
        cell(0, 0),
        in_range_pos(),
        merchant_pos(),
        &atlas,
    );
    assert_eq!(outcome, SellOutcome::WalletFull);
    assert_eq!(inventory.placed().len(), 1);
    // An exact-fit sale still succeeds at the cap edge.
    let (inventory, outcome) = sell(
        inventory,
        zen(CarriedZen::CAP - 820),
        cell(0, 0),
        in_range_pos(),
        merchant_pos(),
        &atlas,
    );
    assert_eq!(
        outcome,
        SellOutcome::Sold {
            proceeds: Zen(820),
            balance: zen(CarriedZen::CAP),
        }
    );
    assert!(inventory.placed().is_empty());
}

#[test]
fn sell_refusals_report_range_and_empty_cells() {
    let atlas = real_atlas();
    let stored = bag()
        .place(cell(0, 0), footprint(1, 3), worn(0, 6, 0, 30, 30))
        .unwrap();
    let (inventory, outcome) = sell(
        stored,
        zen(0),
        cell(0, 0),
        out_of_range_pos(),
        merchant_pos(),
        &atlas,
    );
    assert_eq!(outcome, SellOutcome::OutOfRange);
    assert_eq!(inventory.placed().len(), 1);

    let (_, outcome) = sell(
        inventory,
        zen(0),
        cell(7, 7),
        in_range_pos(),
        merchant_pos(),
        &atlas,
    );
    assert_eq!(outcome, SellOutcome::NoItemAtCell);
}

// --- Repair: single item. -----------------------------------------------------

fn at_npc() -> RepairSite {
    RepairSite::AtNpc {
        merchant_pos: merchant_pos(),
    }
}

/// A half-worn Gladius (15/30): at-NPC 210, self 520; broken (0/30): at-NPC
/// 580, self 1,400 — the real-data pins of the b-anchor curve.
#[test]
fn a_worn_equipped_item_repairs_to_full_at_the_pinned_price() {
    let atlas = real_atlas();
    let equipment = Equipment::empty().with(EquipmentSlot::LeftHand, worn(0, 6, 0, 15, 30));
    let (subject, outcome) = repair(
        RepairSubject::Equipped {
            equipment,
            slot: EquipmentSlot::LeftHand,
        },
        zen(1_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(
        outcome,
        RepairOutcome::Repaired {
            cost: Zen(210),
            balance: zen(790),
        }
    );
    match subject {
        RepairSubject::Equipped { equipment, slot } => {
            assert_eq!(slot, EquipmentSlot::LeftHand);
            let item = equipment.get(EquipmentSlot::LeftHand).unwrap();
            assert_eq!(item.durability, Durability::full(30));
        }
        RepairSubject::Stored { .. } => panic!("the equipped subject threads back"),
    }
}

#[test]
fn a_stored_item_repairs_in_place_through_any_covered_cell() {
    let atlas = real_atlas();
    let stored = bag()
        .place(cell(0, 0), footprint(1, 3), worn(0, 6, 0, 15, 30))
        .unwrap();
    let (subject, outcome) = repair(
        RepairSubject::Stored {
            inventory: stored,
            cell: cell(2, 0),
        },
        zen(1_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(
        outcome,
        RepairOutcome::Repaired {
            cost: Zen(210),
            balance: zen(790),
        }
    );
    match subject {
        RepairSubject::Stored {
            inventory,
            cell: at,
        } => {
            assert_eq!(at, cell(2, 0));
            assert_eq!(inventory.placed().len(), 1);
            let placed = inventory.occupant(cell(0, 0)).unwrap();
            assert_eq!(placed.anchor, cell(0, 0));
            assert_eq!(placed.item.durability, Durability::full(30));
        }
        RepairSubject::Equipped { .. } => panic!("the stored subject threads back"),
    }
}

#[test]
fn repair_prices_the_broken_and_self_multipliers_on_real_data() {
    let atlas = real_atlas();
    let cases: [(u8, RepairSite, u64); 4] = [
        (15, at_npc(), 210),
        (15, RepairSite::SelfRepair, 520),
        (0, at_npc(), 580),
        (0, RepairSite::SelfRepair, 1_400),
    ];
    for (current, site, cost) in cases {
        let equipment =
            Equipment::empty().with(EquipmentSlot::LeftHand, worn(0, 6, 0, current, 30));
        let (_, outcome) = repair(
            RepairSubject::Equipped {
                equipment,
                slot: EquipmentSlot::LeftHand,
            },
            zen(10_000),
            site,
            in_range_pos(),
            &atlas,
        );
        assert_eq!(
            outcome,
            RepairOutcome::Repaired {
                cost: Zen(cost),
                balance: zen(10_000 - cost),
            },
            "current {current}"
        );
    }
}

#[test]
fn a_full_gauge_is_a_no_op_with_no_charge() {
    let atlas = real_atlas();
    let equipment = Equipment::empty().with(EquipmentSlot::LeftHand, worn(0, 6, 0, 30, 30));
    let (subject, outcome) = repair(
        RepairSubject::Equipped {
            equipment,
            slot: EquipmentSlot::LeftHand,
        },
        zen(1_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::AlreadyFull);
    match subject {
        RepairSubject::Equipped { equipment, .. } => {
            let item = equipment.get(EquipmentSlot::LeftHand).unwrap();
            assert_eq!(item.durability, Durability::full(30));
        }
        RepairSubject::Stored { .. } => panic!("the equipped subject threads back"),
    }
}

#[test]
fn the_kind_gate_refuses_a_potion_stack_and_a_quiver() {
    let atlas = real_atlas();
    // A potion stack in a cell (D7) — ample zen changes nothing.
    let potions = bag()
        .place(cell(0, 0), footprint(1, 1), worn(14, 0, 0, 1, 3))
        .unwrap();
    let (subject, outcome) = repair(
        RepairSubject::Stored {
            inventory: potions,
            cell: cell(0, 0),
        },
        zen(1_000_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::NotRepairableKind);
    match subject {
        RepairSubject::Stored { inventory, .. } => {
            let placed = inventory.occupant(cell(0, 0)).unwrap();
            assert_eq!(placed.item.durability, Durability::new(1, 3).unwrap());
        }
        RepairSubject::Equipped { .. } => panic!("the stored subject threads back"),
    }
    // A part-quiver is refused identically: ammo is bought, not mended.
    let quiver = bag()
        .place(cell(0, 0), footprint(1, 2), worn(4, 15, 0, 100, 255))
        .unwrap();
    let (_, outcome) = repair(
        RepairSubject::Stored {
            inventory: quiver,
            cell: cell(0, 0),
        },
        zen(1_000_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::NotRepairableKind);
}

#[test]
fn at_npc_repair_is_range_gated_and_self_repair_never_is() {
    let atlas = real_atlas();
    let equipment = Equipment::empty().with(EquipmentSlot::LeftHand, worn(0, 6, 0, 15, 30));
    let (subject, outcome) = repair(
        RepairSubject::Equipped {
            equipment,
            slot: EquipmentSlot::LeftHand,
        },
        zen(1_000),
        at_npc(),
        out_of_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::OutOfRange);
    // The same position self-repairs fine — at the 5/2 rate.
    let (_, outcome) = repair(
        subject,
        zen(1_000),
        RepairSite::SelfRepair,
        out_of_range_pos(),
        &atlas,
    );
    assert_eq!(
        outcome,
        RepairOutcome::Repaired {
            cost: Zen(520),
            balance: zen(480),
        }
    );
}

#[test]
fn short_zen_and_empty_addresses_refuse_without_change() {
    let atlas = real_atlas();
    let equipment = Equipment::empty().with(EquipmentSlot::LeftHand, worn(0, 6, 0, 15, 30));
    // 209 is one short of the 210 price.
    let (subject, outcome) = repair(
        RepairSubject::Equipped {
            equipment,
            slot: EquipmentSlot::LeftHand,
        },
        zen(209),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::InsufficientZen);
    match &subject {
        RepairSubject::Equipped { equipment, .. } => {
            let item = equipment.get(EquipmentSlot::LeftHand).unwrap();
            assert_eq!(item.durability, Durability::new(15, 30).unwrap());
        }
        RepairSubject::Stored { .. } => panic!("the equipped subject threads back"),
    }
    // An empty slot and an empty cell both report Empty, no charge.
    let (_, outcome) = repair(
        RepairSubject::Equipped {
            equipment: Equipment::empty(),
            slot: EquipmentSlot::Helm,
        },
        zen(1_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::Empty);
    let (_, outcome) = repair(
        RepairSubject::Stored {
            inventory: bag(),
            cell: cell(3, 3),
        },
        zen(1_000),
        at_npc(),
        in_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairOutcome::Empty);
}

// --- Repair-all. ---------------------------------------------------------------

/// The classic walk order the report must follow.
const WALK_ORDER: [EquipmentSlot; 11] = [
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

/// Damaged Gladius in both hands (210 and 280), a damaged Vine Helm (50), and
/// full-durability Vine Boots; every other slot empty.
fn worn_set() -> Equipment {
    Equipment::empty()
        .with(EquipmentSlot::LeftHand, worn(0, 6, 0, 15, 30))
        .with(EquipmentSlot::RightHand, worn(0, 6, 0, 10, 30))
        .with(EquipmentSlot::Helm, worn(7, 10, 0, 10, 22))
        .with(EquipmentSlot::Boots, worn(11, 10, 0, 22, 22))
}

#[test]
fn repair_all_walks_every_slot_in_the_classic_order() {
    let atlas = real_atlas();
    let (equipment, outcome) =
        repair_all(worn_set(), zen(10_000), at_npc(), in_range_pos(), &atlas);
    let RepairAllOutcome::Walked { slots, balance } = outcome else {
        panic!("in range: the walk runs");
    };
    // 210 + 280 + 50 debited across the three damaged slots.
    assert_eq!(balance, zen(9_460));
    let expected: Vec<SlotRepair> = WALK_ORDER
        .iter()
        .map(|&slot| SlotRepair {
            slot,
            result: match slot {
                EquipmentSlot::LeftHand => SlotRepairResult::Repaired { cost: Zen(210) },
                EquipmentSlot::RightHand => SlotRepairResult::Repaired { cost: Zen(280) },
                EquipmentSlot::Helm => SlotRepairResult::Repaired { cost: Zen(50) },
                EquipmentSlot::Boots => SlotRepairResult::AlreadyFull,
                EquipmentSlot::Armor
                | EquipmentSlot::Pants
                | EquipmentSlot::Gloves
                | EquipmentSlot::Wings
                | EquipmentSlot::Pet
                | EquipmentSlot::Pendant
                | EquipmentSlot::Ring1
                | EquipmentSlot::Ring2 => SlotRepairResult::Empty,
            },
        })
        .collect();
    assert_eq!(slots, expected);
    for slot in [
        EquipmentSlot::LeftHand,
        EquipmentSlot::RightHand,
        EquipmentSlot::Helm,
    ] {
        let item = equipment.get(slot).unwrap();
        assert_eq!(item.durability.current(), item.durability.max(), "{slot:?}");
    }
}

#[test]
fn repair_all_stops_at_the_first_unaffordable_slot_with_earlier_repairs_kept() {
    let atlas = real_atlas();
    // 539 covers LeftHand (210) and RightHand (280) but not the Helm's 50.
    let (equipment, outcome) = repair_all(worn_set(), zen(539), at_npc(), in_range_pos(), &atlas);
    let RepairAllOutcome::Walked { slots, balance } = outcome else {
        panic!("in range: the walk runs");
    };
    assert_eq!(balance, zen(49));
    assert_eq!(
        slots,
        vec![
            SlotRepair {
                slot: EquipmentSlot::LeftHand,
                result: SlotRepairResult::Repaired { cost: Zen(210) },
            },
            SlotRepair {
                slot: EquipmentSlot::RightHand,
                result: SlotRepairResult::Repaired { cost: Zen(280) },
            },
            SlotRepair {
                slot: EquipmentSlot::Helm,
                result: SlotRepairResult::Unaffordable { cost: Zen(50) },
            },
        ]
    );
    // The paid slots stay repaired; the stopped slot stays worn.
    assert_eq!(
        equipment.get(EquipmentSlot::LeftHand).unwrap().durability,
        Durability::full(30)
    );
    assert_eq!(
        equipment.get(EquipmentSlot::RightHand).unwrap().durability,
        Durability::full(30)
    );
    assert_eq!(
        equipment.get(EquipmentSlot::Helm).unwrap().durability,
        Durability::new(10, 22).unwrap()
    );
}

#[test]
fn repair_all_is_range_gated_at_npc_and_priced_5_over_2_in_the_field() {
    let atlas = real_atlas();
    let (_, outcome) = repair_all(
        worn_set(),
        zen(10_000),
        at_npc(),
        out_of_range_pos(),
        &atlas,
    );
    assert_eq!(outcome, RepairAllOutcome::OutOfRange);

    // Self-repair needs no merchant and multiplies each slot by 5/2.
    let (_, outcome) = repair_all(
        worn_set(),
        zen(10_000),
        RepairSite::SelfRepair,
        out_of_range_pos(),
        &atlas,
    );
    let RepairAllOutcome::Walked { slots, .. } = outcome else {
        panic!("self-repair never range-gates");
    };
    let left = slots.first().unwrap();
    assert_eq!(
        *left,
        SlotRepair {
            slot: EquipmentSlot::LeftHand,
            result: SlotRepairResult::Repaired { cost: Zen(520) },
        }
    );
}

// --- Zen pickup at the cap edges. ----------------------------------------------

fn pile(amount: u64) -> WorldZen {
    WorldZen {
        amount: Zen(amount),
        position: TileCoord::new(10, 10).to_world(),
        map: MapNumber(0),
        despawn: Tick(1_200),
    }
}

/// The picker's locus for the cap-edge pickups — standing on the pile's tile.
fn picker_pos() -> WorldPos {
    TileCoord::new(10, 10).to_world()
}

#[test]
fn an_exact_fit_pickup_reaches_the_cap_edge() {
    let (balance, outcome) = pickup_zen(pile(1), zen(1_999_999_999), picker_pos(), MapNumber(0));
    assert_eq!(balance, zen(CarriedZen::CAP));
    assert_eq!(outcome, ZenPickupOutcome::PickedUp);
}

#[test]
fn a_one_over_pickup_hands_the_same_world_zen_back() {
    let original = pile(2);
    let (balance, outcome) = pickup_zen(
        original.clone(),
        zen(1_999_999_999),
        picker_pos(),
        MapNumber(0),
    );
    assert_eq!(balance, zen(1_999_999_999));
    assert_eq!(
        outcome,
        ZenPickupOutcome::OverCap {
            world_zen: original
        }
    );
}

// --- Golden per-entry shelf prices. ---------------------------------------------

/// Buys the entry at `$at` into an empty bag and yields the debited price.
/// A macro (the catalog suite's `load!` precedent) so its `panic!` expands
/// inside the `#[test]` functions where `clippy.toml` permits it.
macro_rules! bought_price {
    ($shop:expr, $at:expr) => {{
        let (_, outcome) = buy(
            bag(),
            zen(CarriedZen::CAP),
            $shop,
            $at,
            in_range_pos(),
            merchant_pos(),
        );
        match outcome {
            BuyOutcome::NewItem { balance, .. } => CarriedZen::CAP - balance.get(),
            BuyOutcome::Merged { .. }
            | BuyOutcome::OutOfRange
            | BuyOutcome::UnknownShelfSlot
            | BuyOutcome::InventoryFull
            | BuyOutcome::InsufficientZen => {
                panic!("an empty bag buys every entry fresh: {outcome:?}")
            }
        }
    }};
}

#[test]
fn every_shelf_entry_across_all_eleven_merchants_prices_through_the_buy_service() {
    let atlas = real_atlas();
    let mut entries = 0usize;
    let mut total = 0u64;
    for npc in MERCHANTS {
        let shop = shop_of(&atlas, npc);
        for (at, _entry) in shop.entries() {
            entries += 1;
            total += bought_price!(shop, at);
        }
    }
    // The whole-catalog golden checksum: 231 entries, 8,075,710 zen.
    assert_eq!(entries, 231);
    assert_eq!(total, 8_075_710);
}

#[test]
fn the_scenario_named_entries_price_at_their_golden_values() {
    let atlas = real_atlas();
    // (npc, slot, price): the curated Hanzo fixes (D14 Gladius/Falchion on
    // distinct anchors, D15 the actual Mace at +2 and Morning Star at +3),
    // Elf Lala's verbatim mixed-level Vine set (K4), the two option-2
    // shields (K6, Elven at Eo and Legendary at Izabel), the potion packs,
    // and the two ammo quivers.
    let golden: [(u16, u8, u64); 18] = [
        (251, 50, 1_900),
        (251, 54, 15_400),
        (251, 73, 29_400),
        (251, 76, 40_100),
        (242, 32, 610),
        (242, 34, 1_400),
        (242, 36, 960),
        (242, 38, 2_400),
        (242, 48, 2_800),
        (243, 78, 19_100),
        (245, 52, 94_700),
        (242, 0, 20),
        (242, 8, 60),
        (242, 21, 750),
        (242, 23, 70),
        (242, 22, 100),
        (242, 24, 150),
        (242, 28, 150),
    ];
    for (npc, at, price) in golden {
        let shop = shop_of(&atlas, npc);
        assert_eq!(bought_price!(shop, slot(at)), price, "npc {npc} slot {at}");
    }
}

// --- Determinism: no RNG anywhere in the family (L7). ---------------------------

#[test]
fn shop_services_are_bit_identical_on_identical_inputs_with_no_seed() {
    // The signatures are the compile-time proof: no `RngCore` parameter
    // exists anywhere in the wave — binding each service to a plain fn
    // pointer type admits no generic RNG slot. Every double-call below runs
    // through the pinned pointer.
    type BuyPort<'a> = fn(
        Inventory,
        CarriedZen,
        ShopView<'a>,
        ShelfSlot,
        WorldPos,
        WorldPos,
    ) -> (Inventory, BuyOutcome);
    type SellPort<'a> =
        fn(Inventory, CarriedZen, Cell, WorldPos, WorldPos, &'a Atlas) -> (Inventory, SellOutcome);
    type RepairPort<'a> = fn(
        RepairSubject,
        CarriedZen,
        RepairSite,
        WorldPos,
        &'a Atlas,
    ) -> (RepairSubject, RepairOutcome);
    type RepairAllPort<'a> =
        fn(Equipment, CarriedZen, RepairSite, WorldPos, &'a Atlas) -> (Equipment, RepairAllOutcome);
    type PickupZenPort =
        fn(WorldZen, CarriedZen, WorldPos, MapNumber) -> (CarriedZen, ZenPickupOutcome);
    let buy_port: BuyPort<'_> = buy;
    let sell_port: SellPort<'_> = sell;
    let repair_port: RepairPort<'_> = repair;
    let repair_all_port: RepairAllPort<'_> = repair_all;
    let pickup_zen_port: PickupZenPort = pickup_zen;

    let atlas = real_atlas();
    let shop = shop_of(&atlas, 251);

    let run_buy = || {
        buy_port(
            bag(),
            zen(500_000),
            shop,
            slot(76),
            in_range_pos(),
            merchant_pos(),
        )
    };
    assert_eq!(run_buy(), run_buy());

    let run_sell = || {
        let stored = bag()
            .place(cell(0, 0), footprint(1, 3), worn(0, 6, 0, 30, 30))
            .unwrap();
        sell_port(
            stored,
            zen(250_000),
            cell(0, 0),
            in_range_pos(),
            merchant_pos(),
            &atlas,
        )
    };
    assert_eq!(run_sell(), run_sell());

    let run_repair = || {
        repair_port(
            RepairSubject::Stored {
                inventory: bag()
                    .place(cell(0, 0), footprint(1, 3), worn(0, 6, 0, 15, 30))
                    .unwrap(),
                cell: cell(0, 0),
            },
            zen(1_000),
            at_npc(),
            in_range_pos(),
            &atlas,
        )
    };
    assert_eq!(run_repair(), run_repair());

    let run_repair_all =
        || repair_all_port(worn_set(), zen(10_000), at_npc(), in_range_pos(), &atlas);
    assert_eq!(run_repair_all(), run_repair_all());

    let run_pickup = || pickup_zen_port(pile(40_000), zen(250_000), picker_pos(), MapNumber(0));
    assert_eq!(run_pickup(), run_pickup());
}

// --- D2: intents name their target; no session pointer exists. -------------------

#[test]
fn each_buy_depends_only_on_its_named_shop_view() {
    let atlas = real_atlas();
    // The same buyer state against two different merchants' shelves: each
    // result reads only the passed view — core holds no opened-NPC state.
    let lala = shop_of(&atlas, 242);
    let hanzo = shop_of(&atlas, 251);
    let (_, from_lala) = buy(
        bag(),
        zen(500_000),
        lala,
        slot(0),
        in_range_pos(),
        merchant_pos(),
    );
    let (_, from_hanzo) = buy(
        bag(),
        zen(500_000),
        hanzo,
        slot(0),
        in_range_pos(),
        merchant_pos(),
    );
    // Elf Lala's slot 0 is the 20-zen potion pack; Hanzo's is the 230-zen
    // Kris — same intent byte, different named target, different result.
    assert_eq!(
        from_lala,
        BuyOutcome::NewItem {
            at: cell(0, 0),
            balance: zen(499_980),
        }
    );
    assert_eq!(
        from_hanzo,
        BuyOutcome::NewItem {
            at: cell(0, 0),
            balance: zen(499_770),
        }
    );
}
