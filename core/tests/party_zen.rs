//! The party zen split over the real wallet cap: the equal division with the
//! remainder to the picker, an at-cap member's share grounding as a fresh pile
//! (conserved against the real 2,000,000,000 cap), the shared qualification
//! exclusions, and the untouched solo `pickup_zen` path beside it.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body.

#[path = "common/dataset.rs"]
mod dataset;

use mu_core::components::party::{MemberSlot, Membership, Vitality};
use mu_core::components::spatial::WorldPos;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{CarriedZen, Exp, Level, MapNumber, Tick, Zen};
use mu_core::entities::party_session::{PartyMember, PartySession};
use mu_core::entities::world_zen::WorldZen;
use mu_core::services::inventory::{ZenPickupOutcome, pickup_zen};
use mu_core::services::party::{MemberFact, SlotWallet, split_zen_pickup};

use dataset::{or_abort, real_atlas};

fn pos(x: u8, y: u8) -> WorldPos {
    TileCoord::new(x, y).to_world()
}

fn map0() -> MapNumber {
    MapNumber(0)
}

fn wallet(value: u64) -> CarriedZen {
    or_abort(CarriedZen::new(value))
}

fn active(slot: u8) -> PartyMember {
    PartyMember {
        slot: MemberSlot(slot),
        membership: Membership::Active,
    }
}

fn trio() -> PartySession {
    PartySession::forming().with_member(active(2))
}

fn fact(slot: u8) -> MemberFact {
    MemberFact {
        slot: MemberSlot(slot),
        level: or_abort(Level::new(30)),
        experience: Exp(0),
        vitality: Vitality::Alive,
        map: map0(),
        position: pos(0, 0),
    }
}

fn wallets(balances: [u64; 3]) -> Vec<SlotWallet> {
    balances
        .into_iter()
        .enumerate()
        .map(|(index, balance)| SlotWallet {
            slot: MemberSlot(or_abort(u8::try_from(index))),
            wallet: wallet(balance),
        })
        .collect()
}

fn pile(amount: u64) -> WorldZen {
    WorldZen {
        amount: Zen(amount),
        position: pos(0, 0),
        map: map0(),
        despawn: Tick(9999),
    }
}

#[test]
fn a_real_hundred_thousand_pile_splits_equally_with_the_remainder_to_the_picker() {
    let _atlas = real_atlas();
    let facts = [fact(0), fact(1), fact(2)];
    let slot_wallets = wallets([0, 0, 0]);
    let (picker, others) = or_abort(facts.split_first().ok_or("nonempty facts"));
    let (picker_wallet, other_wallets) =
        or_abort(slot_wallets.split_first().ok_or("nonempty wallets"));
    let result = split_zen_pickup(
        &pile(100_000),
        &trio(),
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    assert!(result.to_ground.is_empty());
    let credited: Vec<(u8, u64)> = result
        .credits
        .iter()
        .map(|c| (c.slot.0, c.wallet.get()))
        .collect();
    assert_eq!(credited, vec![(0, 33_334), (1, 33_333), (2, 33_333)]);
    let total: u64 = result.credits.iter().map(|c| c.wallet.get()).sum();
    assert_eq!(total, 100_000, "every coin is accounted for");
}

#[test]
fn an_at_cap_wallet_grounds_its_share_and_conservation_holds_against_the_real_cap() {
    // The game_config zen cap is the real 2e9 the wallet type enforces.
    let atlas = real_atlas();
    let config_cap = atlas.progression(); // touch the parsed atlas
    let _ = config_cap;
    assert_eq!(CarriedZen::CAP, 2_000_000_000);

    let facts = [fact(0), fact(1), fact(2)];
    // Slot 1 one below the cap: crediting 33,333 over-caps, so its share grounds.
    let slot_wallets = wallets([0, 1_999_999_999, 0]);
    let (picker, others) = or_abort(facts.split_first().ok_or("nonempty facts"));
    let (picker_wallet, other_wallets) =
        or_abort(slot_wallets.split_first().ok_or("nonempty wallets"));
    let result = split_zen_pickup(
        &pile(100_000),
        &trio(),
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    assert_eq!(result.to_ground.len(), 1);
    let grounded = or_abort(result.to_ground.first().ok_or("one grounded pile"));
    assert_eq!(grounded.amount, Zen(33_333));
    assert_eq!(grounded.position, pos(0, 0));
    assert_eq!(grounded.map, map0());

    let slots: Vec<u8> = result.credits.iter().map(|c| c.slot.0).collect();
    assert_eq!(slots, vec![0, 2], "slot 1 receives no credit");
    // Conservation: 33,334 (picker) + 33,333 (slot 2) + 33,333 (grounded) = 100,000.
    let credited_delta: u64 = result.credits.iter().map(|c| c.wallet.get()).sum();
    assert_eq!(credited_delta + grounded.amount.0, 100_000);
}

#[test]
fn a_dead_held_or_out_of_range_member_is_dropped_from_the_divisor() {
    let _atlas = real_atlas();
    let slot_wallets = wallets([0, 0, 0]);
    let (picker_wallet, other_wallets) =
        or_abort(slot_wallets.split_first().ok_or("nonempty wallets"));

    // Dead slot 2 -> divisor 2, 45,000 each.
    let mut dead = [fact(0), fact(1), fact(2)];
    dead[2].vitality = Vitality::Dead;
    let (picker, others) = or_abort(dead.split_first().ok_or("nonempty facts"));
    let result = split_zen_pickup(
        &pile(90_000),
        &trio(),
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    let credited: Vec<(u8, u64)> = result
        .credits
        .iter()
        .map(|c| (c.slot.0, c.wallet.get()))
        .collect();
    assert_eq!(credited, vec![(0, 45_000), (1, 45_000)]);

    // Held slot 2 -> divisor 2.
    let held = trio().with_membership(MemberSlot(2), Membership::Held { expires: Tick(1) });
    let facts = [fact(0), fact(1), fact(2)];
    let (picker, others) = or_abort(facts.split_first().ok_or("nonempty facts"));
    let result = split_zen_pickup(
        &pile(90_000),
        &held,
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    assert_eq!(result.credits.len(), 2);
    assert!(result.credits.iter().all(|c| c.slot != MemberSlot(2)));

    // Out of range slot 2 -> divisor 2; at 12 tiles -> divisor 3 (inclusive edge).
    let mut far = [fact(0), fact(1), fact(2)];
    far[2].position = pos(13, 0);
    let (picker, others) = or_abort(far.split_first().ok_or("nonempty facts"));
    let result = split_zen_pickup(
        &pile(90_000),
        &trio(),
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    assert_eq!(result.credits.len(), 2);

    let mut edge = [fact(0), fact(1), fact(2)];
    edge[2].position = pos(12, 0);
    let (picker, others) = or_abort(edge.split_first().ok_or("nonempty facts"));
    let result = split_zen_pickup(
        &pile(90_000),
        &trio(),
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    assert_eq!(result.credits.len(), 3);
}

#[test]
fn the_picker_always_qualifies_so_the_divisor_is_never_zero_and_the_pile_is_never_lost() {
    let _atlas = real_atlas();
    // Every other member is dead; only the picker qualifies -> the whole pile
    // credits to the picker, nothing lost.
    let mut facts = [fact(0), fact(1), fact(2)];
    facts[1].vitality = Vitality::Dead;
    facts[2].vitality = Vitality::Dead;
    let slot_wallets = wallets([0, 0, 0]);
    let (picker, others) = or_abort(facts.split_first().ok_or("nonempty facts"));
    let (picker_wallet, other_wallets) =
        or_abort(slot_wallets.split_first().ok_or("nonempty wallets"));
    let result = split_zen_pickup(
        &pile(70_000),
        &trio(),
        *picker,
        picker_wallet.wallet,
        others,
        other_wallets,
    );
    assert!(result.to_ground.is_empty());
    assert_eq!(result.credits.len(), 1);
    assert_eq!(result.credits.first().map(|c| c.wallet.get()), Some(70_000));
}

#[test]
fn a_solo_picker_still_uses_pickup_zen_untouched() {
    let _atlas = real_atlas();
    // The solo path merges the whole pile.
    let (balance, outcome) = pickup_zen(pile(40_000), wallet(250_000));
    assert_eq!(balance, wallet(290_000));
    assert_eq!(outcome, ZenPickupOutcome::PickedUp);

    // Over-cap hands the pile back whole — the solo path is unchanged by W-PARTY.
    let (balance, outcome) = pickup_zen(pile(2), wallet(1_999_999_999));
    assert_eq!(balance, wallet(1_999_999_999));
    assert_eq!(outcome, ZenPickupOutcome::OverCap { world_zen: pile(2) });
}
