//! The ground-item lifecycle over the real `/data` Atlas (W-GROUND): a real
//! drop stamped off the atlas 60-second duration flips at exactly its despawn
//! tick, the 1-second corpse-to-loot beat anchors every clock at appearance,
//! the three-tile pickup reach gates a real item and a real zen pile, and the
//! kill-locked ownership window admits the owner, refuses the stranger, and
//! frees the drop when it elapses.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]`
//! body so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;

use dataset::{or_abort, real_atlas};
use mu_core::components::drop_claim::{DropClaim, PickerStanding};
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{ItemLevel, MapNumber, Tick, TickDuration, Zen};
use mu_core::data::atlas::Atlas;
use mu_core::entities::world_item::WorldItem;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::ground::DespawnEvent;
use mu_core::services::ground::{DropOrigin, ItemStamp, reap_ground, stamp_item, stamp_zen};
use mu_core::services::inventory::{PickupOutcome, ZenPickupOutcome, pickup, pickup_zen};

/// A real 1x3 catalog identity (Short Sword) — the ground drop every scenario
/// lays.
const SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};

/// The host's 50 ms tick cadence.
fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// A fresh instance of real item `id` at plus-level zero, full gauge.
fn instance_of(atlas: &Atlas, id: ItemRef) -> ItemInstance {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
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

/// Real item `id`'s cell footprint, read from the atlas.
fn footprint_of(atlas: &Atlas, id: ItemRef) -> Footprint {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    or_abort(Footprint::new(def.width, def.height))
}

/// A real sword laid at tile `(x, y)` on map 0 with the clocks of `stamp`.
fn ground_sword(atlas: &Atlas, x: u8, y: u8, stamp: ItemStamp) -> WorldItem {
    WorldItem {
        instance: instance_of(atlas, SWORD),
        position: TileCoord::new(x, y).to_world(),
        map: MapNumber(0),
        despawn: stamp.despawn,
        claim: stamp.claim,
    }
}

#[test]
fn a_real_drop_is_reaped_at_its_atlas_stamped_despawn_and_kept_one_tick_before() {
    // The despawn tick is the atlas 60 s duration off the drop's appearance —
    // never a fixture constant — and the flip is exact.
    let atlas = real_atlas();
    let stamp = stamp_item(
        DropOrigin::MonsterKill,
        Tick(100),
        atlas.item_drop_duration(),
        tick(),
    );
    assert_eq!(
        stamp.despawn,
        stamp.appearance + atlas.item_drop_duration().in_ticks(tick()),
        "despawn is the atlas duration off appearance"
    );
    let item = ground_sword(&atlas, 10, 10, stamp);

    let one_before = Tick(stamp.despawn.0 - 1);
    let (survivors, _, events) = reap_ground(vec![item.clone()], Vec::new(), one_before);
    assert_eq!(survivors, vec![item.clone()]);
    assert!(events.is_empty());

    let (survivors, _, events) = reap_ground(vec![item], Vec::new(), stamp.despawn);
    assert!(survivors.is_empty());
    assert_eq!(
        events,
        vec![DespawnEvent::ItemDespawned {
            position: TileCoord::new(10, 10).to_world(),
            map: MapNumber(0),
            item: SWORD,
        }]
    );
}

#[test]
fn a_monster_drop_anchors_its_clocks_at_kill_plus_one_second_a_player_drop_at_the_instant() {
    let atlas = real_atlas();
    let duration = atlas.item_drop_duration();

    let kill = stamp_item(DropOrigin::MonsterKill, Tick(500), duration, tick());
    assert_eq!(
        kill.appearance,
        Tick(500 + 20),
        "one second at 50 ms per tick"
    );
    assert_eq!(kill.despawn, kill.appearance + duration.in_ticks(tick()));
    assert_eq!(
        kill.claim,
        DropClaim::Claimed {
            until: Tick(kill.appearance.0 + 200)
        },
        "the 10 s window anchors at appearance"
    );

    let dropped = stamp_item(DropOrigin::PlayerDrop, Tick(500), duration, tick());
    assert_eq!(
        dropped.appearance,
        Tick(500),
        "a player drop appears instantly"
    );
    assert_eq!(dropped.despawn, Tick(500) + duration.in_ticks(tick()));
    assert_eq!(
        dropped.claim,
        DropClaim::Claimed { until: Tick(700) },
        "the window still anchors at the instant appearance"
    );

    // Zen shares the same clocks and carries no claim field at all.
    let pile_stamp = stamp_zen(DropOrigin::MonsterKill, Tick(500), duration, tick());
    assert_eq!(pile_stamp.appearance, kill.appearance);
    assert_eq!(pile_stamp.despawn, kill.despawn);
}

#[test]
fn a_real_ground_item_gates_on_the_three_tile_reach_and_the_same_map() {
    let atlas = real_atlas();
    let stamp = stamp_item(
        DropOrigin::Ownerless,
        Tick(0),
        atlas.item_drop_duration(),
        tick(),
    );
    let footprint = footprint_of(&atlas, SWORD);
    let anchor = Cell { row: 0, col: 0 };

    // Four tiles away on the same map: OutOfReach, the item handed back.
    let item = ground_sword(&atlas, 10, 10, stamp);
    let (_, outcome) = pickup(
        item.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        TileCoord::new(14, 10).to_world(),
        MapNumber(0),
        PickerStanding::Stranger,
        Tick(0),
    );
    assert_eq!(outcome, PickupOutcome::OutOfReach { item: item.clone() });

    // A near position on ANOTHER map: still OutOfReach (the same-map term).
    let (_, outcome) = pickup(
        item.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        TileCoord::new(10, 10).to_world(),
        MapNumber(1),
        PickerStanding::Stranger,
        Tick(0),
    );
    assert_eq!(outcome, PickupOutcome::OutOfReach { item: item.clone() });

    // Three tiles away on the same map: the gate passes and the item stores.
    let (inventory, outcome) = pickup(
        item,
        Inventory::empty(8, 8),
        anchor,
        footprint,
        TileCoord::new(13, 10).to_world(),
        MapNumber(0),
        PickerStanding::Stranger,
        Tick(0),
    );
    assert_eq!(outcome, PickupOutcome::PickedUp { at: anchor });
    assert!(inventory.occupant(anchor).is_some());
}

#[test]
fn a_real_claimed_drop_admits_the_owner_refuses_the_stranger_then_frees() {
    let atlas = real_atlas();
    let stamp = stamp_item(
        DropOrigin::MonsterKill,
        Tick(0),
        atlas.item_drop_duration(),
        tick(),
    );
    let DropClaim::Claimed { until } = stamp.claim else {
        panic!("a monster drop is claimed");
    };
    let footprint = footprint_of(&atlas, SWORD);
    let anchor = Cell { row: 0, col: 0 };
    let picker = TileCoord::new(10, 10).to_world();
    let in_window = stamp.appearance;
    let after_window = until;

    // The owner picks inside the window.
    let item = ground_sword(&atlas, 10, 10, stamp);
    let (_, outcome) = pickup(
        item.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        picker,
        MapNumber(0),
        PickerStanding::Owner,
        in_window,
    );
    assert_eq!(outcome, PickupOutcome::PickedUp { at: anchor });

    // A stranger inside the window is refused, the item handed back whole.
    let (_, outcome) = pickup(
        item.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        picker,
        MapNumber(0),
        PickerStanding::Stranger,
        in_window,
    );
    assert_eq!(outcome, PickupOutcome::Refused { item: item.clone() });

    // At the window's close the same stranger picks it up.
    let (_, outcome) = pickup(
        item,
        Inventory::empty(8, 8),
        anchor,
        footprint,
        picker,
        MapNumber(0),
        PickerStanding::Stranger,
        after_window,
    );
    assert_eq!(outcome, PickupOutcome::PickedUp { at: anchor });

    // An ownerless drop is free to the stranger at any tick.
    let free = stamp_item(
        DropOrigin::Ownerless,
        Tick(0),
        atlas.item_drop_duration(),
        tick(),
    );
    assert_eq!(free.claim, DropClaim::Unclaimed);
    let item = ground_sword(&atlas, 10, 10, free);
    let (_, outcome) = pickup(
        item,
        Inventory::empty(8, 8),
        anchor,
        footprint,
        picker,
        MapNumber(0),
        PickerStanding::Stranger,
        Tick(0),
    );
    assert_eq!(outcome, PickupOutcome::PickedUp { at: anchor });
}

#[test]
fn a_real_zen_pile_is_free_to_a_stranger_in_window_and_gates_on_the_same_reach() {
    let atlas = real_atlas();
    let stamp = stamp_zen(
        DropOrigin::MonsterKill,
        Tick(0),
        atlas.item_drop_duration(),
        tick(),
    );
    let pile = WorldZen {
        amount: Zen(1_234),
        position: TileCoord::new(10, 10).to_world(),
        map: MapNumber(0),
        despawn: stamp.despawn,
    };
    let wallet = or_abort(mu_core::components::units::CarriedZen::new(0));

    // A stranger inside what would be an item's window takes the pile whole —
    // zen carries no claim.
    let (balance, outcome) = pickup_zen(
        pile.clone(),
        wallet,
        TileCoord::new(12, 10).to_world(),
        MapNumber(0),
    );
    assert_eq!(outcome, ZenPickupOutcome::PickedUp);
    assert_eq!(
        balance,
        or_abort(mu_core::components::units::CarriedZen::new(1_234))
    );

    // Beyond three tiles it is OutOfReach, the pile handed back.
    let (balance, outcome) = pickup_zen(
        pile.clone(),
        wallet,
        TileCoord::new(14, 10).to_world(),
        MapNumber(0),
    );
    assert_eq!(outcome, ZenPickupOutcome::OutOfReach { world_zen: pile });
    assert_eq!(balance, wallet);
}
