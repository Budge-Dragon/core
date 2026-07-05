//! Player↔player trade flows end-to-end over the real regenerated `/data`:
//! the 4×8 window geometry under real item footprints, the worn-item offer as
//! host composition (unequip, then offer — the trade port never sources from
//! equipment), the second lock's full-batch prove-then-move cross of real
//! items and zen at server-chosen anchors, the bounce paths over packed real
//! bags and capped wallets, the total cancel settlement with first-fit
//! re-placement and explicit overflow, and the no-RNG determinism of the
//! whole family.
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
use mu_core::components::trade_window::Side;
use mu_core::components::units::{CarriedZen, ItemLevel, Zen};
use mu_core::data::atlas::Atlas;
use mu_core::entities::trade_session::TradeSession;
use mu_core::events::inventory::UnequipOutcome;
use mu_core::events::trade::{
    BouncedProof, CancelReason, OfferOutcome, Overflow, RearrangeOutcome, Settlement, SideFailure,
    TradeEvent, UnlockOutcome, WithdrawOutcome, ZenOfferOutcome,
};
use mu_core::services::inventory::unequip;
use mu_core::services::trade::{
    AcceptOutcome, Holdings, LockResult, RequestOutcome, TradeAvailability, accept, cancel, lock,
    offer_item, offer_zen, rearrange, request, unlock, withdraw_item,
};

// --- Shared fixtures. --------------------------------------------------------

/// Real catalog identities (footprints verified against the shipped `/data`):
/// Dragon Armor 2×3, Serpent Sword 1×3, Jewel of Bless 1×1.
const DRAGON_ARMOR: ItemRef = ItemRef {
    group: 8,
    number: 1,
};
const SERPENT_SWORD: ItemRef = ItemRef {
    group: 0,
    number: 8,
};
const JEWEL_OF_BLESS: ItemRef = ItemRef {
    group: 14,
    number: 13,
};

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

fn pos(x: u8, y: u8) -> WorldPos {
    TileCoord::new(x, y).to_world()
}

fn holdings(inventory: Inventory, wallet: u64) -> Holdings {
    Holdings {
        inventory,
        wallet: zen(wallet),
    }
}

/// A fresh instance of a real definition at plus-level zero, full gauge.
fn item(atlas: &Atlas, id: ItemRef) -> ItemInstance {
    let def = or_abort(atlas.item(id).ok_or(format!("unknown item {id:?}")));
    ItemInstance {
        item: id,
        level: ItemLevel::ZERO,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(def.durability),
        augment: CraftedAugment::None,
    }
}

/// The real definition's cell footprint.
fn footprint_of(atlas: &Atlas, id: ItemRef) -> Footprint {
    let def = or_abort(atlas.item(id).ok_or(format!("unknown item {id:?}")));
    or_abort(Footprint::new(def.width, def.height))
}

/// A 15×8 bag packed edge-to-edge with twenty real 2×3 Dragon Armors — no
/// free cell remains.
fn packed_bag(atlas: &Atlas) -> Inventory {
    let fp = footprint_of(atlas, DRAGON_ARMOR);
    let mut inventory = bag();
    for band in 0u8..5 {
        for column in 0u8..4 {
            inventory =
                match inventory.place(cell(band * 3, column * 2), fp, item(atlas, DRAGON_ARMOR)) {
                    Ok(next) => next,
                    Err((_, _, reason)) => or_abort(Err(format!("packing failed: {reason}"))),
                };
        }
    }
    inventory
}

/// An open session with `id` offered by `side` at the window origin, built
/// through the port (request → accept → offer).
fn session_with_offer(atlas: &Atlas, side: Side, id: ItemRef) -> TradeSession {
    let (outcome, _) = request(
        pos(0, 0),
        TradeAvailability::Available {
            position: pos(5, 0),
        },
    );
    let RequestOutcome::Opened { session } = outcome else {
        return or_abort(Err("request must open"));
    };
    let (outcome, _) = accept(session, Side::Partner, pos(0, 0), pos(5, 0));
    let AcceptOutcome::Accepted { session } = outcome else {
        return or_abort(Err("accept must open the windows"));
    };
    let stored = match bag().place(cell(0, 0), footprint_of(atlas, id), item(atlas, id)) {
        Ok(stored) => stored,
        Err((_, _, reason)) => return or_abort(Err(format!("seed placement failed: {reason}"))),
    };
    let (session, _, outcome, _) = offer_item(session, side, stored, cell(0, 0), cell(0, 0));
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    session
}

// --- Window geometry over real footprints (4×8). ------------------------------

#[test]
fn a_real_dragon_armor_fits_the_window_at_the_top_and_leaves_it_at_the_bottom() {
    let atlas = real_atlas();
    let stored = bag()
        .place(
            cell(0, 0),
            footprint_of(&atlas, DRAGON_ARMOR),
            item(&atlas, DRAGON_ARMOR),
        )
        .unwrap();
    let (session, inventory, outcome, _) = offer_item(
        TradeSession::opened(),
        Side::Requester,
        stored,
        cell(0, 0),
        cell(0, 0),
    );
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    assert!(inventory.occupant(cell(0, 0)).is_none());
    // The 2×3 armor anchored at row 2 would cover rows 2..5, past the 4-row
    // window.
    let (session, _, outcome, _) = withdraw_item(session, Side::Requester, cell(0, 0), inventory);
    assert_eq!(outcome, WithdrawOutcome::Withdrawn { at: cell(0, 0) });
    let stored = bag()
        .place(
            cell(0, 0),
            footprint_of(&atlas, DRAGON_ARMOR),
            item(&atlas, DRAGON_ARMOR),
        )
        .unwrap();
    let (_, inventory, outcome, _) =
        offer_item(session, Side::Requester, stored, cell(0, 0), cell(2, 0));
    assert_eq!(outcome, OfferOutcome::WindowOutOfBounds);
    assert!(inventory.occupant(cell(0, 0)).is_some());
}

#[test]
fn real_items_pack_the_window_until_offers_are_refused() {
    let atlas = real_atlas();
    let armor_fp = footprint_of(&atlas, DRAGON_ARMOR);
    let sword_fp = footprint_of(&atlas, SERPENT_SWORD);
    let stored = bag()
        .place(cell(0, 0), armor_fp, item(&atlas, DRAGON_ARMOR))
        .unwrap()
        .place(cell(0, 2), sword_fp, item(&atlas, SERPENT_SWORD))
        .unwrap()
        .place(cell(0, 3), armor_fp, item(&atlas, DRAGON_ARMOR))
        .unwrap();
    let (session, inventory, outcome, _) = offer_item(
        TradeSession::opened(),
        Side::Requester,
        stored,
        cell(0, 0),
        cell(0, 0),
    );
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    let (session, inventory, outcome, _) =
        offer_item(session, Side::Requester, inventory, cell(0, 2), cell(0, 2));
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 2) });
    // Overlapping the armor's 2×3 rectangle is refused, the item kept.
    let (session, inventory, outcome, _) =
        offer_item(session, Side::Requester, inventory, cell(0, 3), cell(1, 1));
    assert_eq!(outcome, OfferOutcome::WindowCellsOccupied);
    assert!(inventory.occupant(cell(0, 3)).is_some());
    // A 1×3 sword anchored on the bottom row leaves the window.
    let (session, inventory, outcome, _) =
        offer_item(session, Side::Requester, inventory, cell(0, 3), cell(3, 7));
    assert_eq!(outcome, OfferOutcome::WindowOutOfBounds);
    // Sliding the sword within the packed window still works and swaps
    // nothing.
    let (_, outcome, _) = rearrange(session, Side::Requester, cell(0, 2), cell(0, 7));
    assert_eq!(
        outcome,
        RearrangeOutcome::Rearranged {
            from: cell(0, 2),
            to: cell(0, 7),
        }
    );
    let _ = inventory;
}

#[test]
fn a_worn_item_is_offered_by_host_composition_unequip_then_offer() {
    let atlas = real_atlas();
    let worn = Equipment::empty().with(EquipmentSlot::Armor, item(&atlas, DRAGON_ARMOR));
    // Host composition step 1: the existing inventory service takes the item
    // off.
    let (equipment, outcome) = unequip(worn, EquipmentSlot::Armor);
    let UnequipOutcome::Unequipped { item: taken, .. } = outcome else {
        panic!("the armor was worn");
    };
    assert!(equipment.get(EquipmentSlot::Armor).is_none());
    // Host composition step 2: the item lands in the bag, then the trade port
    // offers it from there — no equipment-source path exists on the port.
    let stored = bag()
        .place(cell(0, 0), footprint_of(&atlas, DRAGON_ARMOR), taken)
        .unwrap();
    let (session, _, outcome, _) = offer_item(
        TradeSession::opened(),
        Side::Requester,
        stored,
        cell(0, 0),
        cell(0, 0),
    );
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    let TradeSession::Open { .. } = session else {
        panic!("still open");
    };
}

// --- Completion over real receivers. -------------------------------------------

#[test]
fn the_second_lock_crosses_real_items_at_server_chosen_anchors() {
    let atlas = real_atlas();
    let session = session_with_offer(&atlas, Side::Requester, DRAGON_ARMOR);
    let stored = bag()
        .place(
            cell(0, 0),
            footprint_of(&atlas, SERPENT_SWORD),
            item(&atlas, SERPENT_SWORD),
        )
        .unwrap();
    let (session, _, outcome, _) =
        offer_item(session, Side::Partner, stored, cell(0, 0), cell(0, 0));
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    let (_, _, result, _) = lock(
        session,
        Side::Requester,
        holdings(bag(), 0),
        holdings(bag(), 0),
    );
    let LockResult::Locked { session } = result else {
        panic!("the first lock must not complete");
    };
    let (requester, partner, result, events) = lock(
        session,
        Side::Partner,
        holdings(bag(), 0),
        holdings(bag(), 0),
    );
    assert_eq!(result, LockResult::Completed);
    assert_eq!(events, vec![TradeEvent::Completed]);
    // Server-chosen anchors: first_fit on the empty receivers lands at the
    // origin.
    assert_eq!(
        requester.inventory.occupant(cell(0, 0)).unwrap().item.item,
        SERPENT_SWORD
    );
    assert_eq!(
        partner.inventory.occupant(cell(0, 0)).unwrap().item.item,
        DRAGON_ARMOR
    );
}

#[test]
fn a_receiver_packed_with_real_items_bounces_the_second_lock() {
    let atlas = real_atlas();
    let session = session_with_offer(&atlas, Side::Partner, JEWEL_OF_BLESS);
    let (_, _, result, _) = lock(
        session,
        Side::Partner,
        holdings(packed_bag(&atlas), 0),
        holdings(bag(), 0),
    );
    let LockResult::Locked { session } = result else {
        panic!("the first lock must not complete");
    };
    let requester_in = holdings(packed_bag(&atlas), 0);
    let partner_in = holdings(bag(), 0);
    let (requester, partner, result, _) = lock(
        session,
        Side::Requester,
        requester_in.clone(),
        partner_in.clone(),
    );
    let LockResult::Bounced { session, proof } = result else {
        panic!("expected a bounce");
    };
    assert_eq!(
        proof,
        BouncedProof::Requester {
            failure: SideFailure::ItemsDoNotFit
        }
    );
    // Nothing crossed: holdings byte-identical, escrow intact, back to open.
    assert_eq!(requester, requester_in);
    assert_eq!(partner, partner_in);
    let TradeSession::Open { offers, .. } = session else {
        panic!("bounced back to open");
    };
    assert_eq!(offers.get(Side::Partner).window().placed().len(), 1);
}

#[test]
fn real_jewels_and_zen_cross_together_with_cap_checked_balances() {
    let atlas = real_atlas();
    let session = session_with_offer(&atlas, Side::Requester, JEWEL_OF_BLESS);
    let (session, wallet, outcome, _) =
        offer_zen(session, Side::Requester, zen(300_000), Zen(250_000));
    assert_eq!(
        outcome,
        ZenOfferOutcome::Offered {
            escrowed: Zen(250_000),
            wallet: zen(50_000),
        }
    );
    let (_, _, result, _) = lock(
        session,
        Side::Requester,
        holdings(bag(), wallet.get()),
        holdings(bag(), 1_999_750_000),
    );
    let LockResult::Locked { session } = result else {
        panic!("the first lock must not complete");
    };
    let (requester, partner, result, _) = lock(
        session,
        Side::Partner,
        holdings(bag(), wallet.get()),
        holdings(bag(), 1_999_750_000),
    );
    assert_eq!(result, LockResult::Completed);
    // The jewel and the zen crossed together; the credit lands exactly on the
    // carry cap — inclusive, so it completes.
    assert_eq!(
        partner.inventory.occupant(cell(0, 0)).unwrap().item.item,
        JEWEL_OF_BLESS
    );
    assert_eq!(partner.wallet, zen(2_000_000_000));
    assert_eq!(requester.wallet, zen(50_000));
    assert!(requester.inventory.placed().is_empty());
}

#[test]
fn two_real_items_that_each_fit_alone_bounce_together_when_the_batch_does_not() {
    let atlas = real_atlas();
    let armor_fp = footprint_of(&atlas, DRAGON_ARMOR);
    // Exactly one 2×3 hole in an otherwise packed real bag.
    let (one_hole, _) = packed_bag(&atlas).remove(cell(0, 0)).unwrap();
    let stored = bag()
        .place(cell(0, 0), armor_fp, item(&atlas, DRAGON_ARMOR))
        .unwrap()
        .place(cell(0, 2), armor_fp, item(&atlas, DRAGON_ARMOR))
        .unwrap();
    let (session, inventory, outcome, _) = offer_item(
        TradeSession::opened(),
        Side::Partner,
        stored,
        cell(0, 0),
        cell(0, 0),
    );
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    let (session, _, outcome, _) =
        offer_item(session, Side::Partner, inventory, cell(0, 2), cell(0, 2));
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 2) });
    let (_, _, result, _) = lock(
        session,
        Side::Partner,
        holdings(one_hole.clone(), 0),
        holdings(bag(), 0),
    );
    let LockResult::Locked { session } = result else {
        panic!("the first lock must not complete");
    };
    // Either armor alone fits the single hole; the full batch must fail as
    // one vote — never a partial commit.
    assert!(one_hole.first_fit(armor_fp).is_some());
    let (requester, _, result, _) = lock(
        session,
        Side::Requester,
        holdings(one_hole.clone(), 0),
        holdings(bag(), 0),
    );
    let LockResult::Bounced { proof, .. } = result else {
        panic!("expected a bounce");
    };
    assert_eq!(
        proof,
        BouncedProof::Requester {
            failure: SideFailure::ItemsDoNotFit
        }
    );
    assert_eq!(requester.inventory, one_hole);
}

// --- Full lifecycle e2e. ---------------------------------------------------------

#[test]
fn a_full_lifecycle_ends_with_both_sides_holding_the_swapped_goods() {
    let atlas = real_atlas();
    let (outcome, _) = request(
        pos(10, 10),
        TradeAvailability::Available {
            position: pos(10, 20),
        },
    );
    let RequestOutcome::Opened { session } = outcome else {
        panic!("request must open");
    };
    let (outcome, _) = accept(session, Side::Partner, pos(10, 10), pos(10, 20));
    let AcceptOutcome::Accepted { session } = outcome else {
        panic!("accept must open the windows");
    };
    // The requester offers a Dragon Armor and 400,000 zen.
    let requester_bag = bag()
        .place(
            cell(0, 0),
            footprint_of(&atlas, DRAGON_ARMOR),
            item(&atlas, DRAGON_ARMOR),
        )
        .unwrap();
    let (session, requester_bag, outcome, _) = offer_item(
        session,
        Side::Requester,
        requester_bag,
        cell(0, 0),
        cell(0, 0),
    );
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    let (session, requester_wallet, outcome, _) =
        offer_zen(session, Side::Requester, zen(1_000_000), Zen(400_000));
    assert_eq!(
        outcome,
        ZenOfferOutcome::Offered {
            escrowed: Zen(400_000),
            wallet: zen(600_000),
        }
    );
    // The partner offers a Serpent Sword and 100,000 zen.
    let partner_bag = bag()
        .place(
            cell(0, 0),
            footprint_of(&atlas, SERPENT_SWORD),
            item(&atlas, SERPENT_SWORD),
        )
        .unwrap();
    let (session, partner_bag, outcome, _) =
        offer_item(session, Side::Partner, partner_bag, cell(0, 0), cell(0, 0));
    assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
    let (session, partner_wallet, outcome, _) =
        offer_zen(session, Side::Partner, zen(1_000_000), Zen(100_000));
    assert_eq!(
        outcome,
        ZenOfferOutcome::Offered {
            escrowed: Zen(100_000),
            wallet: zen(900_000),
        }
    );
    // Lock, lock: the second lock completes the deal.
    let (_, _, result, _) = lock(
        session,
        Side::Partner,
        holdings(requester_bag.clone(), requester_wallet.get()),
        holdings(partner_bag.clone(), partner_wallet.get()),
    );
    let LockResult::Locked { session } = result else {
        panic!("the first lock must not complete");
    };
    let (requester, partner, result, _) = lock(
        session,
        Side::Requester,
        holdings(requester_bag, requester_wallet.get()),
        holdings(partner_bag, partner_wallet.get()),
    );
    assert_eq!(result, LockResult::Completed);
    assert_eq!(
        requester.inventory.occupant(cell(0, 0)).unwrap().item.item,
        SERPENT_SWORD
    );
    assert_eq!(requester.wallet, zen(700_000));
    assert_eq!(
        partner.inventory.occupant(cell(0, 0)).unwrap().item.item,
        DRAGON_ARMOR
    );
    assert_eq!(partner.wallet, zen(1_300_000));
}

#[test]
fn an_explicit_cancel_after_offers_returns_every_real_item_and_coin() {
    let atlas = real_atlas();
    let session = session_with_offer(&atlas, Side::Requester, DRAGON_ARMOR);
    let (session, requester_wallet, _, _) =
        offer_zen(session, Side::Requester, zen(1_000_000), Zen(400_000));
    let (settlement, events) = cancel(
        session,
        CancelReason::Explicit,
        holdings(bag(), requester_wallet.get()),
        holdings(bag(), 500_000),
    );
    assert_eq!(
        settlement
            .requester
            .inventory
            .occupant(cell(0, 0))
            .unwrap()
            .item
            .item,
        DRAGON_ARMOR
    );
    assert_eq!(settlement.requester.wallet, zen(1_000_000));
    assert_eq!(settlement.requester.overflow, Overflow::empty());
    assert_eq!(settlement.partner.wallet, zen(500_000));
    assert_eq!(settlement.partner.overflow, Overflow::empty());
    assert_eq!(
        events,
        vec![TradeEvent::Cancelled {
            reason: CancelReason::Explicit
        }]
    );
}

#[test]
fn a_both_fail_second_lock_names_each_side_over_real_data() {
    let atlas = real_atlas();
    // The partner offers a real armor the requester has no room for; the
    // requester offers zen the partner has no headroom for.
    let session = session_with_offer(&atlas, Side::Partner, DRAGON_ARMOR);
    let (session, _, outcome, _) = offer_zen(
        session,
        Side::Requester,
        zen(1_000_000_000),
        Zen(1_000_000_000),
    );
    assert_eq!(
        outcome,
        ZenOfferOutcome::Offered {
            escrowed: Zen(1_000_000_000),
            wallet: zen(0),
        }
    );
    let (_, _, result, _) = lock(
        session,
        Side::Partner,
        holdings(packed_bag(&atlas), 0),
        holdings(bag(), 1_500_000_000),
    );
    let LockResult::Locked { session } = result else {
        panic!("the first lock must not complete");
    };
    let (_, _, result, _) = lock(
        session,
        Side::Requester,
        holdings(packed_bag(&atlas), 0),
        holdings(bag(), 1_500_000_000),
    );
    let LockResult::Bounced { proof, .. } = result else {
        panic!("expected a bounce");
    };
    assert_eq!(
        proof,
        BouncedProof::Both {
            requester: SideFailure::ItemsDoNotFit,
            partner: SideFailure::WalletWouldOverflow,
        }
    );
}

#[test]
fn cancel_after_a_mid_trade_pickup_overflows_the_unfittable_real_item() {
    let atlas = real_atlas();
    let session = session_with_offer(&atlas, Side::Requester, DRAGON_ARMOR);
    // Mid-trade the requester's bag filled completely — the escrowed armor's
    // hole is gone.
    let (settlement, _) = cancel(
        session,
        CancelReason::Explicit,
        holdings(packed_bag(&atlas), 0),
        holdings(bag(), 0),
    );
    assert_eq!(settlement.requester.overflow.items.len(), 1);
    assert_eq!(
        settlement.requester.overflow.items.first().unwrap().item,
        DRAGON_ARMOR
    );
    assert_eq!(settlement.requester.overflow.zen, Zen(0));
    // Twenty armors still packed, none evicted; the escrowed one rides the
    // overflow for the host to ground-drop — never lost.
    assert_eq!(settlement.requester.inventory.placed().len(), 20);
}

// --- Determinism: no RNG anywhere in the family. -----------------------------------

#[test]
fn trade_services_are_bit_identical_on_identical_inputs_with_no_seed() {
    // The signatures are the compile-time proof: no `RngCore` parameter
    // exists anywhere in the wave — binding each service to a plain fn
    // pointer type admits no generic RNG slot. Every double-call below runs
    // through the pinned pointer.
    type RequestPort = fn(WorldPos, TradeAvailability) -> (RequestOutcome, Vec<TradeEvent>);
    type AcceptPort =
        fn(TradeSession, Side, WorldPos, WorldPos) -> (AcceptOutcome, Vec<TradeEvent>);
    type OfferItemPort = fn(
        TradeSession,
        Side,
        Inventory,
        Cell,
        Cell,
    ) -> (TradeSession, Inventory, OfferOutcome, Vec<TradeEvent>);
    type WithdrawPort = fn(
        TradeSession,
        Side,
        Cell,
        Inventory,
    ) -> (TradeSession, Inventory, WithdrawOutcome, Vec<TradeEvent>);
    type OfferZenPort = fn(
        TradeSession,
        Side,
        CarriedZen,
        Zen,
    ) -> (TradeSession, CarriedZen, ZenOfferOutcome, Vec<TradeEvent>);
    type RearrangePort =
        fn(TradeSession, Side, Cell, Cell) -> (TradeSession, RearrangeOutcome, Vec<TradeEvent>);
    type LockPort = fn(
        TradeSession,
        Side,
        Holdings,
        Holdings,
    ) -> (Holdings, Holdings, LockResult, Vec<TradeEvent>);
    type UnlockPort = fn(TradeSession, Side) -> (TradeSession, UnlockOutcome, Vec<TradeEvent>);
    type CancelPort =
        fn(TradeSession, CancelReason, Holdings, Holdings) -> (Settlement, Vec<TradeEvent>);
    let request_port: RequestPort = request;
    let accept_port: AcceptPort = accept;
    let offer_item_port: OfferItemPort = offer_item;
    let withdraw_port: WithdrawPort = withdraw_item;
    let offer_zen_port: OfferZenPort = offer_zen;
    let rearrange_port: RearrangePort = rearrange;
    let lock_port: LockPort = lock;
    let unlock_port: UnlockPort = unlock;
    let cancel_port: CancelPort = cancel;

    let atlas = real_atlas();
    let run_request = || {
        request_port(
            pos(0, 0),
            TradeAvailability::Available {
                position: pos(12, 0),
            },
        )
    };
    assert_eq!(run_request(), run_request());

    let run_accept = || accept_port(TradeSession::Requested, Side::Partner, pos(0, 0), pos(5, 0));
    assert_eq!(run_accept(), run_accept());

    let run_offer = || {
        let stored = bag()
            .place(
                cell(0, 0),
                footprint_of(&atlas, DRAGON_ARMOR),
                item(&atlas, DRAGON_ARMOR),
            )
            .unwrap();
        offer_item_port(
            TradeSession::opened(),
            Side::Requester,
            stored,
            cell(0, 0),
            cell(0, 0),
        )
    };
    assert_eq!(run_offer(), run_offer());

    let run_withdraw = || {
        let (session, inventory, _, _) = run_offer();
        withdraw_port(session, Side::Requester, cell(0, 0), inventory)
    };
    assert_eq!(run_withdraw(), run_withdraw());

    let run_zen = || {
        offer_zen_port(
            TradeSession::opened(),
            Side::Requester,
            zen(1_000_000),
            Zen(250_000),
        )
    };
    assert_eq!(run_zen(), run_zen());

    let run_rearrange = || {
        let (session, _, _, _) = run_offer();
        rearrange_port(session, Side::Requester, cell(0, 0), cell(0, 4))
    };
    assert_eq!(run_rearrange(), run_rearrange());

    let run_lock = || {
        let (session, inventory, _, _) = run_offer();
        let (_, _, result, _) = lock_port(
            session,
            Side::Partner,
            holdings(inventory.clone(), 100),
            holdings(bag(), 200),
        );
        let LockResult::Locked { session } = result else {
            panic!("the first lock must not complete");
        };
        lock_port(
            session,
            Side::Requester,
            holdings(inventory, 100),
            holdings(bag(), 200),
        )
    };
    assert_eq!(run_lock(), run_lock());

    let run_unlock = || {
        let (session, _, _, _) = run_offer();
        unlock_port(session, Side::Requester)
    };
    assert_eq!(run_unlock(), run_unlock());

    let run_cancel = || {
        let (session, inventory, _, _) = run_offer();
        cancel_port(
            session,
            CancelReason::Explicit,
            holdings(inventory, 100),
            holdings(bag(), 200),
        )
    };
    assert_eq!(run_cancel(), run_cancel());
}
