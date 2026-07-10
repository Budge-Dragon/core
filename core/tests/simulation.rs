//! The end-to-end "paper host" scenario harness: one deterministic, in-memory,
//! host-shaped world drives the real mu-core services over the real `/data`
//! Atlas, forcing a serde round-trip between every service call.
//!
//! This is the foundation slice — the **S-SEAM** feature only: the harness's
//! own invariants. It proves the persist seam preserves every live value
//! byte-for-byte and that a snapshot is total over the live sets and excludes
//! the held static Atlas. The cross-system seam scenarios (kill, economy,
//! growth, movement, spawn, effects, the replay capstone) are driven through
//! this same [`paper_host::World`] and its seam by the later scenario groups.
//!
//! Identity assertions are re-serialised-string equality
//! (`serde_json::to_string(a) == serde_json::to_string(b)`), never value `==`:
//! `Character` and several live types derive no `PartialEq`, and serialisation
//! is canonical here (no float, no `HashMap`, stable field order).

#[path = "common/paper_host.rs"]
mod paper_host;

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::equipment::EquipmentSlot;
use mu_core::components::inventory::{Cell, Footprint};
use mu_core::components::item_instance::{ItemInstance, RarityRoll, RolledNormalOption};
use mu_core::components::item_options::NormalOption;
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::item_ref::ItemRef;
use mu_core::components::levels::OptionLevel;
use mu_core::components::life::LifeState;
use mu_core::components::movement::{FlightChange, Movement};
use mu_core::components::party::{Leadership, MemberSlot, Membership, Vitality};
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{Radius, WorldPos};
use mu_core::components::tile::{TileCoord, TileFacing};
use mu_core::components::trade_window::Side;
use mu_core::components::units::{CarriedZen, Exp, ItemLevel, Level, MapNumber, Tick, Zen};
use mu_core::data::common::MonsterNumber;
use mu_core::data::effects::Ailment;
use mu_core::data::gates_warps::WarpIndex;
use mu_core::data::npc_shops::ShelfSlot;
use mu_core::data::spawns::SpawnPlacement;
use mu_core::entities::party_session::{PartyMember, PartySession};
use mu_core::entities::spawned::Spawned;
use mu_core::entities::trade_session::TradeSession;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::combat::AttackOutcome;
use mu_core::events::consume::{ConsumeEvent, PoolKind};
use mu_core::events::craft::MixOutcome;
use mu_core::events::death::{DeathEvent, Respawned};
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::inventory::{EquipOutcome, EquipRejection, PlaceOutcome, RemoveOutcome};
use mu_core::events::kill::KillResolution;
use mu_core::events::loot::Drop;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightDenialReason, FlightOutcome, StepOutcome};
use mu_core::events::party::PartyEvent;
use mu_core::events::progression::GrowthEvent;
use mu_core::events::shop::{BuyOutcome, SellOutcome};
use mu_core::events::skills::{SkillOutcome, TargetHit};
use mu_core::events::spawn::SpawnEvent;
use mu_core::events::trade::{CancelReason, OfferOutcome, ZenOfferOutcome};
use mu_core::events::travel::{
    EnterGateOutcome, TownPortalOutcome, WarpAvailability, WarpLockReason, WarpTravelOutcome,
};
use mu_core::services::effects::ApplicableBuff;
use mu_core::services::inventory::{PickupOutcome, ZenPickupOutcome};
use mu_core::services::party;
use mu_core::services::price::selling_price;
use mu_core::services::profile::character_profile;
use mu_core::services::trade::LockResult;

use paper_host::{
    World, aggressive_monster, cell, dark_knight, dark_knight_in_band, direct_hit_skill,
    fighting_monster_from, first_passive_monster, footprint_of, heal_skill, is_equippable,
    item_at_level, item_instance, low_level_monster, magic_gladiator, monster_instance, nova_skill,
    or_abort, persist, pos, pressing_monster, tile, walkable_run, wire, zen,
};

/// A real 2×2 catalog identity (Dragon Armor) — footprint read from the atlas.
const DRAGON_ARMOR: ItemRef = ItemRef {
    group: 8,
    number: 1,
};

/// A real 1×1 catalog identity (Jewel of Bless) — a small bag filler.
const JEWEL_OF_BLESS: ItemRef = ItemRef {
    group: 14,
    number: 13,
};

/// A real 1×3 weapon (Short Sword) — the chaos-weapon-mix sacrifice.
const SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};

/// A real helm (Bronze Helm) — the +10 upgrade-mix subject.
const HELM: ItemRef = ItemRef {
    group: 7,
    number: 0,
};

/// Jewel of Chaos — every chaos-machine recipe's catalyst.
const JEWEL_OF_CHAOS: ItemRef = ItemRef {
    group: 12,
    number: 15,
};

/// Jewel of Soul — a +10 upgrade booster.
const JEWEL_OF_SOUL: ItemRef = ItemRef {
    group: 14,
    number: 14,
};

/// Fairy Wings (a first wing) — the second-wings-mix base.
const FAIRY_WINGS: ItemRef = ItemRef {
    group: 12,
    number: 0,
};

/// Loch's Feather — the second-wings-mix reagent.
const LOCHS_FEATHER: ItemRef = ItemRef {
    group: 13,
    number: 14,
};

/// Cape of Lord — a high-value item to fund a wallet by selling.
const CAPE_OF_LORD: ItemRef = ItemRef {
    group: 13,
    number: 30,
};

/// A real shield (group 6, number 0) — the off-hand occupant that makes a
/// two-handed weapon conflict.
const SHIELD: ItemRef = ItemRef {
    group: 6,
    number: 0,
};

/// A real two-handed weapon (group 0, number 9) — claims both hands, so it
/// cannot share a hand pair with a worn shield.
const TWO_HANDED_SWORD: ItemRef = ItemRef {
    group: 0,
    number: 9,
};

/// A real bow (group 4, number 0) — an elf-only weapon, used to prove the equip
/// service ignores an item's class list (a Dark Knight wears it anyway).
const ELF_BOW: ItemRef = ItemRef {
    group: 4,
    number: 0,
};

/// Elf Lala, the potion merchant NPC (number 242); shelf slot 0 is her
/// 20-zen small-healing pack.
const ELF_LALA: MonsterNumber = MonsterNumber(242);

/// Seats one representative live value of every kind into a fresh world.
fn seated_world() -> (World, [usize; 5]) {
    let mut world = World::new(2024, MapNumber(0));
    let character = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let monster = {
        let (number, combat, _resistances) = low_level_monster(world.atlas(), 20);
        world.seat_monster(monster_instance(number, combat.hp, tile(12, 12)))
    };
    let ground_item = {
        let dropped = item_instance(world.atlas(), DRAGON_ARMOR);
        world.seat_ground_item(dropped, pos(12, 12), Tick(1200))
    };
    let ground_zen = world.seat_ground_zen(Zen(1007), pos(12, 12), Tick(1200));
    let session = world.seat_session(TradeSession::opened());
    (
        world,
        [character, monster, ground_item, ground_zen, session],
    )
}

// --- S-SEAM. The seam discipline itself. -------------------------------------

#[test]
fn every_live_value_the_world_stores_has_survived_the_persist_round_trip() {
    let (mut world, [character_ix, monster_ix, item_ix, zen_ix, session_ix]) = seated_world();

    // A real service transforms the monster; the driver writes the returned
    // health back into the world *through* the seam.
    let _outcome = world.strike(character_ix, monster_ix);

    // Every stored live value re-serialises identically to its own persist
    // round-trip — nothing leaned on in-memory identity a persisted world loses.
    let stored_character = world.character(character_ix);
    assert_eq!(
        serde_json::to_string(stored_character).unwrap(),
        serde_json::to_string(&persist(stored_character.clone())).unwrap(),
    );

    let stored_monster = world.monster(monster_ix);
    assert_eq!(
        serde_json::to_string(&stored_monster).unwrap(),
        serde_json::to_string(&persist(stored_monster)).unwrap(),
    );

    let stored_item = world.ground_item(item_ix);
    assert_eq!(
        serde_json::to_string(stored_item).unwrap(),
        serde_json::to_string(&persist(stored_item.clone())).unwrap(),
    );

    let stored_zen = world.ground_zen(zen_ix);
    assert_eq!(
        serde_json::to_string(stored_zen).unwrap(),
        serde_json::to_string(&persist(stored_zen.clone())).unwrap(),
    );

    let stored_session = world.session(session_ix);
    assert_eq!(
        serde_json::to_string(stored_session).unwrap(),
        serde_json::to_string(&persist(stored_session.clone())).unwrap(),
    );
}

#[test]
fn the_seam_preserves_the_standalone_wallet_effect_and_footprint_values() {
    let world = World::new(7, MapNumber(0));

    // CarriedZen — a wallet threaded between economy services.
    let wallet = zen(1_000_000);
    let wallet_before = serde_json::to_string(&wallet).unwrap();
    let wallet_after = serde_json::to_string(&persist(wallet)).unwrap();
    assert_eq!(wallet_before, wallet_after);

    // ActiveEffects — rides inside its owner, but survives the seam standalone.
    let effects = ActiveEffects::EMPTY;
    let effects_before = serde_json::to_string(&effects).unwrap();
    let effects_after = serde_json::to_string(&persist(effects)).unwrap();
    assert_eq!(effects_before, effects_after);

    // Footprint — the atlas lookup a host threads through a pickup.
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let footprint_before = serde_json::to_string(&footprint).unwrap();
    let footprint_after = serde_json::to_string(&persist(footprint)).unwrap();
    assert_eq!(footprint_before, footprint_after);
}

#[test]
fn the_snapshot_serialises_exactly_the_live_sets_and_never_the_atlas() {
    let (world, _indices) = seated_world();

    let snapshot = world.snapshot();
    let value: serde_json::Value = serde_json::from_str(&snapshot).unwrap();
    let object = value.as_object().expect("the snapshot is a JSON object");

    // EXACTLY the live sets, and nothing else. A character's parallel bag and
    // worn set are live persisted state too, so they belong to the snapshot's
    // totality — seat_character seats one of each alongside every character.
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "characters",
            "equipment",
            "ground_items",
            "ground_zen",
            "inventories",
            "monsters",
            "parties",
            "pending_invites",
            "sessions"
        ],
    );

    // The held Atlas, the drop policy, the rng's internal state, and the fixed
    // map are all excluded (static data or non-observable).
    assert!(!object.contains_key("atlas"));
    assert!(!object.contains_key("drop_policy"));
    assert!(!object.contains_key("rng"));
    assert!(!object.contains_key("map"));

    // Each live set is present and carries its one seated value.
    for key in [
        "characters",
        "inventories",
        "equipment",
        "monsters",
        "ground_items",
        "ground_zen",
        "sessions",
    ] {
        let set = object.get(key).and_then(serde_json::Value::as_array);
        assert_eq!(
            set.map(Vec::len),
            Some(1),
            "{key} holds its one seated value"
        );
    }
}

// --- Spawn → AI → combat: one instance identity (seam 6; V8). ----------------

#[test]
fn a_spawned_mob_is_advanced_by_its_ai_then_killed_as_one_instance() {
    let mut world = World::new(2024, MapNumber(0));
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let placement = SpawnPlacement::Fixed {
        position: tile(20, 20),
        facing: TileFacing::East,
    };

    // place_spawn returns the aggregate and its event as two ordered vecs.
    let result = world.spawn_from(number, placement);
    assert_eq!(result.spawned.len(), 1);
    assert_eq!(result.events.len(), 1);

    // V8: the Nth Spawned pairs with the Nth event by position — the delivery
    // correlation key, since there is no id. number/at/facing must agree.
    let instance = match &result.spawned[0] {
        Spawned::Mob { instance } => *instance,
        Spawned::Placed { .. } => panic!("a fighting monster spawns a mob"),
    };
    match &result.events[0] {
        SpawnEvent::MobSpawned { number, at, facing } => {
            assert_eq!(*number, instance.number);
            assert_eq!(*at, instance.placement.position);
            assert_eq!(*facing, instance.placement.facing);
        }
        SpawnEvent::ObjectPlaced { .. } => panic!("a fighting monster emits mob_spawned"),
    }

    let spawned_number = instance.number;
    let monster_ix = world.seat_monster(instance);

    // The AI advances THAT SAME instance: a target on its own tile forces a
    // deterministic Attack decision (no RNG draw, no walkable step needed).
    let intent = world.advance_monster(monster_ix, Some(instance.placement.position), Tick(1));
    assert!(matches!(intent, MonsterIntent::Attack { .. }));

    // The AI-advanced instance the world stored round-trips through persist
    // byte-for-byte — nothing leaned on in-memory identity.
    let advanced = world.monster(monster_ix);
    assert_eq!(
        serde_json::to_string(&advanced).unwrap(),
        serde_json::to_string(&persist(advanced)).unwrap(),
    );

    // A knight beats that same instance to death, then the kill composes (V3a).
    let killer = world.seat_character(dark_knight(80, 300, tile(20, 20)));
    loop {
        match world.strike(killer, monster_ix) {
            AttackOutcome::Killed { .. } => break,
            AttackOutcome::Landed { .. } | AttackOutcome::Missed => {}
        }
    }

    // One identity threaded spawn → AI → kill: the number is unchanged, and the
    // Killed outcome composed a real reward.
    assert_eq!(world.monster(monster_ix).number, spawned_number);
    assert_eq!(world.monster(monster_ix).health.current(), 0);
    let resolution = world.resolve_kill_of(killer, monster_ix);
    assert!(resolution.experience.gained.0 > 0);
}

#[test]
fn a_passive_role_placement_yields_a_placed_object_never_a_fightable_mob() {
    let mut world = World::new(3, MapNumber(0));
    let passive = first_passive_monster(world.atlas());
    let placement = SpawnPlacement::Fixed {
        position: tile(15, 15),
        facing: TileFacing::East,
    };

    let result = world.spawn_from(passive, placement);
    assert_eq!(result.spawned.len(), 1);
    match &result.spawned[0] {
        Spawned::Placed { number, .. } => assert_eq!(*number, passive),
        Spawned::Mob { .. } => panic!("a passive role places an object, not a mob"),
    }
    match &result.events[0] {
        SpawnEvent::ObjectPlaced { number, .. } => assert_eq!(*number, passive),
        SpawnEvent::MobSpawned { .. } => panic!("a passive role emits object_placed"),
    }

    // The harness routes a Placed object to NO combat — it is never seated as a
    // monster or struck. Mob vs Placed route to different host systems (duty S2).
}

// --- Kill → drop → pickup → equip (seam 1; V1/V7, V2, V3a). ------------------

/// The item drops a kill produced — the category roll and every special — as
/// `{item, level, rarity}` triples.
fn item_drops(resolution: &KillResolution) -> Vec<(ItemRef, ItemLevel, ItemRarity)> {
    let mut drops = Vec::new();
    for entry in
        core::iter::once(&resolution.drops.category).chain(resolution.drops.specials.iter())
    {
        if let Drop::Item {
            item,
            level,
            rarity,
        } = entry
        {
            drops.push((*item, *level, *rarity));
        }
    }
    drops
}

/// Drives a real kill to completion and lays the first equippable item it
/// dropped on the ground, sweeping the construction seed (outside the drive
/// loop) until a kill yields such a drop. Returns the world plus the killer,
/// victim, and ground indices, the rolled instance, and its identity. `None`
/// only if no seed in the range drops an equippable item — statistically
/// impossible at a 30% item-category rate over an equipment-heavy pool.
fn kill_with_equippable_drop() -> Option<(World, usize, usize, usize, ItemInstance, ItemRef)> {
    for seed in 0u64..64 {
        let mut world = World::new(seed, MapNumber(0));
        let killer = world.seat_character(dark_knight(80, 300, tile(10, 10)));
        let (number, combat, _resistances) = low_level_monster(world.atlas(), 20);
        let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));

        // Strike the victim to death; its health is persisted after each strike.
        loop {
            match world.strike(killer, victim) {
                AttackOutcome::Killed { .. } => break,
                AttackOutcome::Landed { .. } | AttackOutcome::Missed => {}
            }
        }

        // Route the Killed outcome into the reward pathway (V3a) and take the
        // first equippable item drop (a jewel drop is a Drop::Item too, but not
        // wearable — the equip service is the oracle).
        let resolution = world.resolve_kill_of(killer, victim);
        let drop = item_drops(&resolution)
            .into_iter()
            .find(|candidate| is_equippable(world.atlas(), candidate.0));
        let Some((item, level, rarity)) = drop else {
            continue;
        };

        // V1/V7: assemble the ground item from the victim's position (returned),
        // the world map (context), and a host-chosen despawn tick (no core
        // source — spec §5.2).
        let position = world.monster(victim).placement.position;
        let (ground, rolled) = world.drop_item_to_ground(item, level, rarity, position, Tick(1200));
        return Some((world, killer, victim, ground, rolled, item));
    }
    None
}

#[test]
fn a_kills_item_drop_is_rolled_grounded_picked_up_and_worn_as_one_identity() {
    let (mut world, killer, victim, ground, rolled, item) =
        kill_with_equippable_drop().expect("a seed in 0..64 yields an equippable item drop");

    // The monster's health, persisted after each strike, reached zero before the
    // reward was resolved.
    assert_eq!(world.monster(victim).health.current(), 0);

    // A partly-full bag: a 1×1 filler leaves room for a first-fit anchor.
    let filler = item_instance(world.atlas(), JEWEL_OF_BLESS);
    let filler_footprint = footprint_of(world.atlas(), JEWEL_OF_BLESS);
    world.place_in_bag(killer, filler, filler_footprint, cell(0, 0));

    let footprint = footprint_of(world.atlas(), item);
    let anchor = world
        .inventory(killer)
        .first_fit(footprint)
        .expect("the partly-full bag has a first-fit region");

    // Pick it up into the real, half-full bag.
    let picked = world.pickup(killer, ground, anchor);
    assert_eq!(picked, PickupOutcome::PickedUp { at: anchor });

    // The bagged instance equals the rolled instance byte-for-byte after persist.
    let bagged = world
        .inventory(killer)
        .occupant(anchor)
        .expect("the bag holds the picked item")
        .item
        .clone();
    assert_eq!(
        serde_json::to_string(&bagged).unwrap(),
        serde_json::to_string(&rolled).unwrap(),
    );

    // Take it into hand and wear it.
    let in_hand = match world.remove_from_bag(killer, anchor) {
        RemoveOutcome::Removed { item, .. } => item,
        RemoveOutcome::Rejected { .. } => panic!("the anchor held the picked item"),
    };
    let slot = match world.equip_first_available(killer, in_hand) {
        EquipOutcome::Equipped { slot } => slot,
        EquipOutcome::Rejected { .. } => panic!("the dropped item is equippable"),
    };

    // The worn instance equals the rolled instance byte-for-byte after every hop.
    let on_body = world
        .equipment(killer)
        .get(slot)
        .expect("the slot is filled")
        .clone();
    assert_eq!(
        serde_json::to_string(&on_body).unwrap(),
        serde_json::to_string(&rolled).unwrap(),
    );
}

#[test]
fn the_rolled_item_survives_every_hop_and_exists_in_exactly_one_place() {
    let (mut world, killer, _victim, ground, rolled, item) =
        kill_with_equippable_drop().expect("a seed in 0..64 yields an equippable item drop");

    // Pre-pickup: the ground item carries the rolled instance byte-for-byte.
    let on_ground = world.ground_item(ground).instance.clone();
    assert_eq!(
        serde_json::to_string(&on_ground).unwrap(),
        serde_json::to_string(&rolled).unwrap(),
    );

    let footprint = footprint_of(world.atlas(), item);
    let anchor = world
        .inventory(killer)
        .first_fit(footprint)
        .expect("the empty bag has a first-fit region");
    assert_eq!(world.ground_item_count(), 1);

    let picked = world.pickup(killer, ground, anchor);
    assert_eq!(picked, PickupOutcome::PickedUp { at: anchor });

    // The item exists in exactly one place: gone from the ground, present in the
    // bag, and still the identical instance — the ground is not a lossy copy.
    assert_eq!(world.ground_item_count(), 0);
    let bagged = world
        .inventory(killer)
        .occupant(anchor)
        .expect("the bag holds the picked item")
        .item
        .clone();
    assert_eq!(
        serde_json::to_string(&bagged).unwrap(),
        serde_json::to_string(&on_ground).unwrap(),
    );
}

#[test]
fn a_full_bag_refuses_the_pickup_and_the_item_stays_on_the_ground_intact() {
    let (mut world, killer, _victim, ground, _rolled, item) =
        kill_with_equippable_drop().expect("a seed in 0..64 yields an equippable item drop");

    // Fill the whole 8×8 bag so no region fits the dropped footprint.
    let filler = item_instance(world.atlas(), JEWEL_OF_BLESS);
    let full = Footprint::new(8, 8).unwrap();
    let placed = world.place_in_bag(killer, filler, full, cell(0, 0));
    assert!(matches!(placed, PlaceOutcome::Placed { .. }));

    let footprint = footprint_of(world.atlas(), item);
    assert!(world.inventory(killer).first_fit(footprint).is_none());

    // Capture the untouched ground item, then attempt the pickup.
    let before = world.ground_item(ground).clone();
    match world.pickup(killer, ground, cell(0, 0)) {
        PickupOutcome::Rejected { item, .. } => {
            // The reassembled world item is byte-identical to the one on the
            // ground — nothing was dropped on the floor of the code.
            assert_eq!(
                serde_json::to_string(&persist(item)).unwrap(),
                serde_json::to_string(&before).unwrap(),
            );
        }
        PickupOutcome::PickedUp { .. } => panic!("a full bag must refuse the pickup"),
    }

    // The ground set still holds exactly that item — still pickable.
    assert_eq!(world.ground_item_count(), 1);
    assert_eq!(
        serde_json::to_string(world.ground_item(ground)).unwrap(),
        serde_json::to_string(&before).unwrap(),
    );
}

// --- Experience → growth → combat feedback (seam 3; W-GROW leveling service). -

#[test]
fn levels_from_a_kill_are_applied_by_the_leveling_service_and_the_next_fight_is_stronger() {
    let mut world = World::new(7, MapNumber(0));
    let killer = world.seat_character(dark_knight(1, 150, tile(10, 10)));
    let (number, combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));

    let before_rate = character_profile(world.character(killer)).0.attack_rate();
    let before_level = world.character(killer).level();
    let before_points = world.character(killer).unspent_points();

    let resolution = world.resolve_kill_of(killer, victim);
    assert!(
        !resolution.level_ups.is_empty(),
        "a level-1 killer of a high-level victim crosses levels"
    );

    // The core leveling service (W-GROW) owns the growth rule — the points grant,
    // the cap clamp, and the vitals refill. The driver applies the gained
    // experience through it and returns the growth events for delivery.
    let events = world.apply_growth(killer, resolution.experience.gained);
    match events.first() {
        Some(GrowthEvent::LevelsGained {
            reached,
            points_granted,
        }) => {
            assert_eq!(*reached, world.character(killer).level());
            assert!(*points_granted > 0, "a crossing banks unspent points");
        }
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("a level-crossing kill must emit LevelsGained first")
        }
    }

    // The grown Character round-trips through persist (the class↔stats invariant
    // re-proves on load).
    let grown = world.character(killer).clone();
    assert_eq!(
        serde_json::to_string(&grown).unwrap(),
        serde_json::to_string(&persist(grown.clone())).unwrap(),
    );

    // The banked points rose and the re-derived profile is strictly stronger: the
    // level rose, so the level-scaled attack rate rose (the physical span is
    // strength-derived, so leveling's combat feedback is the higher attack rate
    // and min-damage floor).
    assert!(world.character(killer).unspent_points() > before_points);
    assert!(world.character(killer).level() > before_level);
    let after_rate = character_profile(world.character(killer)).0.attack_rate();
    assert!(after_rate > before_rate);
}

#[test]
fn a_multi_level_kill_lists_levels_ascending_and_applies_the_top_one() {
    let mut world = World::new(11, MapNumber(0));
    let killer = world.seat_character(dark_knight(1, 150, tile(10, 10)));
    let (number, combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));

    let resolution = world.resolve_kill_of(killer, victim);
    let level_ups = &resolution.level_ups;
    assert!(
        level_ups.len() >= 2,
        "a level-1 killer of a high-level victim crosses several levels"
    );

    // Ascending by level with no gap, starting one above the killer's level.
    assert_eq!(level_ups[0].level.get(), 2);
    for window in level_ups.windows(2) {
        assert_eq!(window[1].level.get(), window[0].level.get() + 1);
    }

    let top = level_ups
        .iter()
        .map(|level_up| level_up.level)
        .max()
        .expect("the list is non-empty");
    let events = world.apply_growth(killer, resolution.experience.gained);
    assert_eq!(world.character(killer).level(), top);
    // The applied top level shows in the growth event's `reached`, matching the
    // ascending delivery list's maximum.
    match events.first() {
        Some(GrowthEvent::LevelsGained { reached, .. }) => assert_eq!(*reached, top),
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("a multi-level kill must emit LevelsGained first")
        }
    }
}

#[test]
fn a_hero_levels_up_from_a_kill_banks_points_refills_and_returns_stronger() {
    let mut world = World::new(19, MapNumber(0));
    let killer = world.seat_character(dark_knight(1, 150, tile(10, 10)));
    let (number, combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));

    let before_level = world.character(killer).level();
    let before_points = world.character(killer).unspent_points();
    let before_rate = character_profile(world.character(killer)).0.attack_rate();

    let resolution = world.resolve_kill_of(killer, victim);
    let events = world.apply_growth(killer, resolution.experience.gained);

    // The kill crossed levels: the growth event reports the reached level and a
    // positive points grant, both landing on the persisted character.
    let (reached, points_granted) = match events.first() {
        Some(GrowthEvent::LevelsGained {
            reached,
            points_granted,
        }) => (*reached, *points_granted),
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("a level-1 killer of a level-30 victim crosses levels")
        }
    };
    assert!(points_granted > 0);

    // The persisted character banked the points, rose to the reached level, and
    // had all three vitals refilled to the class-formula maxima at the new level.
    let hero = world.character(killer);
    assert_eq!(hero.level(), reached);
    assert!(hero.level() > before_level);
    assert!(hero.unspent_points() > before_points);
    let (_profile, maxima) = character_profile(hero);
    assert_eq!(hero.vitals().health, Pool::full(maxima.max_health));
    assert_eq!(hero.vitals().mana, Pool::full(maxima.max_mana));
    assert_eq!(hero.vitals().ability, Pool::full(maxima.max_ability));

    // The grown character round-trips through persist (the class↔stats gate
    // re-proves on load).
    let grown = hero.clone();
    assert_eq!(
        serde_json::to_string(&grown).unwrap(),
        serde_json::to_string(&persist(grown.clone())).unwrap(),
    );

    // The re-derived profile is strictly stronger — the loop closes: level up and
    // get stronger.
    let after_rate = character_profile(&grown).0.attack_rate();
    assert!(after_rate > before_rate);
}

// --- Monster attack intent → player death (seam 7; V6; death boundary). ------

#[test]
fn a_monsters_attack_intent_forwarded_into_combat_drains_a_player_to_death() {
    let mut world = World::new(42, MapNumber(0));
    let player = world.seat_character(dark_knight(6, 100, tile(10, 10)));
    let (number, combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let monster = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));

    // The AI, seeing the player in range, returns an Attack intent aimed at it.
    let player_pos = world.character(player).placement().position;
    let intent = world.advance_monster(monster, Some(player_pos), Tick(1));
    assert!(matches!(intent, MonsterIntent::Attack { .. }));

    // V6: forward the intent into combat with monster_profile as attacker, the
    // player's character_profile as target, and the player's Pool as the health.
    // resolve_attack is symmetric — the same service kills a player.
    let start = world.character(player).vitals().health.current();
    assert!(start > 0);

    let mut previous = start;
    let mut decreased = false;
    let mut killed = false;
    for _ in 0..10_000u32 {
        let outcome = world.player_struck_by_monster(player, monster);
        let current = world.character(player).vitals().health.current();
        assert!(current <= previous, "health never rises under attack");
        if current < previous {
            decreased = true;
        }
        previous = current;
        if matches!(outcome, AttackOutcome::Killed { .. }) {
            killed = true;
            break;
        }
    }

    assert!(killed, "the player is drained to a killing blow");
    assert!(decreased, "the drain strictly reduced the player's health");
    assert_eq!(world.character(player).vitals().health.current(), 0);
}

/// The experience a death step docked — the `(lost, remaining)` an
/// `ExperienceDocked` event carries, or `None` when nothing was docked.
fn experience_docked(events: &[DeathEvent]) -> Option<(Exp, Exp)> {
    events.iter().find_map(|event| match event {
        DeathEvent::ExperienceDocked { lost, remaining } => Some((*lost, *remaining)),
        DeathEvent::Died { .. } | DeathEvent::ZenDocked { .. } => None,
    })
}

/// The zen a death step docked — the `(lost, remaining)` a `ZenDocked` event
/// carries, or `None` when nothing was docked.
fn zen_docked(events: &[DeathEvent]) -> Option<(Zen, CarriedZen)> {
    events.iter().find_map(|event| match event {
        DeathEvent::ZenDocked { lost, remaining } => Some((*lost, *remaining)),
        DeathEvent::Died { .. } | DeathEvent::ExperienceDocked { .. } => None,
    })
}

#[test]
fn a_player_death_docks_penalty_then_respawns_in_town_closing_the_loop() {
    let mut world = World::new(42, MapNumber(3));

    // A level-100 Dark Knight seeded mid-band and carrying zen, seated on map 3
    // (which owns a spawn gate). A knight's defense is agility-derived, so this
    // level-100 knight drains to zero exactly as the level-6 sibling does — the
    // death is combat-driven, never fabricated.
    let hero = dark_knight_in_band(world.atlas(), 100, 1_000_000, MapNumber(3), tile(10, 10));
    let player = world.seat_character(hero);
    let (number, combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let monster = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));

    // A poison is active at the instant of death; it must ride through the dead
    // beat and clear only on respawn.
    world.apply_ailment_to(player, Ailment::Poisoned, 30, Tick(0));
    assert!(
        world.character(player).active_effects().poison().is_some(),
        "the hero is poisoned before it dies"
    );

    // The real 1% of the level-100 band, read from the shipped curve — not a
    // literal, so the dock is proven against real data.
    let expected_exp_loss = {
        let curve = world.atlas().exp_curve();
        let band = or_abort(curve.level(101)).total_to_hold().0
            - or_abort(curve.level(100)).total_to_hold().0;
        band / 100
    };
    let before_exp = world.character(player).experience();
    let before_zen = world.character(player).zen();

    // The monster drains the player to a killing blow.
    let mut killed = false;
    for _ in 0..10_000u32 {
        if matches!(
            world.player_struck_by_monster(player, monster),
            AttackOutcome::Killed { .. }
        ) {
            killed = true;
            break;
        }
    }
    assert!(killed, "the monster drains the player to a killing blow");
    assert_eq!(world.character(player).vitals().health.current(), 0);

    // The death step: the paper host calls the real resolve_death and persists the
    // returned dead character. No penalty, mark, or clear logic lives host-side.
    let death_events = world.resolve_player_death(player, Tick(500));

    let respawn_at = match world.character(player).life() {
        LifeState::Dead { respawn_at } => respawn_at,
        LifeState::Alive => panic!("resolve_death marks the killed player Dead"),
    };
    assert!(respawn_at > Tick(500), "the dead beat is tick-delayed");
    assert!(
        death_events.contains(&DeathEvent::Died { respawn_at }),
        "the death events carry the scheduled respawn"
    );

    // Experience docked the real 1% band, floored so the level never drops.
    let after_exp = world.character(player).experience();
    let (exp_lost, exp_remaining) =
        experience_docked(&death_events).expect("a mid-band level-100 death docks experience");
    assert_eq!(exp_lost.0, expected_exp_loss, "docked the real 1% band");
    assert_eq!(exp_remaining, after_exp);
    assert_eq!(after_exp.0, before_exp.0 - expected_exp_loss);
    assert_eq!(
        world.character(player).level().get(),
        100,
        "the floor never de-levels"
    );

    // Zen docked its bracket percentage (1% at level 100).
    let after_zen = world.character(player).zen();
    let (zen_lost, zen_remaining) = zen_docked(&death_events).expect("a level-100 death docks zen");
    assert_eq!(zen_remaining, after_zen);
    assert_eq!(zen_lost.0, before_zen.get() - after_zen.get());
    assert!(
        zen_lost.0 > 0,
        "the bracket percentage docked a real amount"
    );

    // resolve_death heals nothing and clears nothing: vitals stay at zero and the
    // poison rides through the dead beat, uncleared.
    assert_eq!(
        world.character(player).vitals().health.current(),
        0,
        "resolve_death does not heal"
    );
    assert!(
        world.character(player).active_effects().poison().is_some(),
        "the poison survives the dead beat"
    );

    // Advance to the respawn beat and respawn — the paper host calls the real
    // respawn and persists the revived character.
    let respawned = world
        .respawn_player(player)
        .expect("a dead player respawns");

    // Alive, seated on the walkable town tile the Respawned carries.
    assert_eq!(world.character(player).life(), LifeState::Alive);
    let placement = world.character(player).placement();
    assert_eq!(
        respawned,
        Respawned {
            map: placement.map,
            position: placement.position,
            facing: placement.facing,
        },
        "the Respawned mirrors where the player now stands"
    );
    let grid = or_abort(
        world
            .atlas()
            .walk_grid(placement.map)
            .ok_or("the respawn map has a walk grid"),
    );
    assert!(
        grid.walkable(placement.position),
        "respawn lands on a walkable town tile"
    );

    // All three vitals refilled to the class-formula maxima; every effect cleared.
    let revived = world.character(player);
    let (_profile, maxima) = character_profile(revived);
    assert_eq!(revived.vitals().health, Pool::full(maxima.max_health));
    assert_eq!(revived.vitals().mana, Pool::full(maxima.max_mana));
    assert_eq!(revived.vitals().ability, Pool::full(maxima.max_ability));
    assert_eq!(revived.active_effects(), ActiveEffects::EMPTY);

    // The loop closes. The respawned player round-trips persist (the class↔stats
    // gate re-proves on load)...
    let revived = revived.clone();
    assert_eq!(
        serde_json::to_string(&revived).unwrap(),
        serde_json::to_string(&persist(revived.clone())).unwrap(),
    );

    // ...and death is no longer terminal: a fresh strike lands on the alive player
    // and drains it below full — the field is open again.
    let full_health = world.character(player).vitals().health.current();
    assert!(
        full_health > 0,
        "the respawned player stands at full health"
    );
    let mut struck = false;
    for _ in 0..10_000u32 {
        let outcome = world.player_struck_by_monster(player, monster);
        if world.character(player).vitals().health.current() < full_health
            || matches!(outcome, AttackOutcome::Killed { .. })
        {
            struck = true;
            break;
        }
    }
    assert!(
        struck,
        "the respawned player takes fresh combat damage — death did not end the road"
    );
}

// --- Zen as one economy: earn then spend one purse (seam 2; V1/V7). ----------

/// The first money `Drop::Zen` amount a kill produced — the category roll or a
/// special; `None` when the kill dropped no money.
fn zen_drop(resolution: &KillResolution) -> Option<u64> {
    core::iter::once(&resolution.drops.category)
        .chain(resolution.drops.specials.iter())
        .find_map(|drop| match drop {
            Drop::Zen { amount } => Some(amount.0),
            Drop::Item { .. } | Drop::Nothing => None,
        })
}

/// Drives a real kill to completion and lays the money it dropped on the ground,
/// sweeping the construction seed (outside the drive loop) until a kill yields a
/// `Drop::Zen` of at least the 20-zen cost of the cheapest shelf entry. Returns
/// the world plus the killer index, the ground-zen index, and the pile amount.
/// `None` only if no seed in the range dropped enough money — statistically
/// impossible over a money-weighted drop pool.
fn kill_with_zen_drop() -> Option<(World, usize, usize, u64)> {
    for seed in 0u64..256 {
        let mut world = World::new(seed, MapNumber(0));
        let killer = world.seat_character(dark_knight(30, 250, tile(10, 10)));
        let (number, combat, _resistances) = fighting_monster_from(world.atlas(), 30);
        let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));
        loop {
            match world.strike(killer, victim) {
                AttackOutcome::Killed { .. } => break,
                AttackOutcome::Landed { .. } | AttackOutcome::Missed => {}
            }
        }
        let resolution = world.resolve_kill_of(killer, victim);
        let Some(amount) = zen_drop(&resolution).filter(|amount| *amount >= 20) else {
            continue;
        };
        let position = world.monster(victim).placement.position;
        let ground = world.seat_ground_zen(Zen(amount), position, Tick(1200));
        return Some((world, killer, ground, amount));
    }
    None
}

#[test]
fn kill_money_picked_up_off_the_ground_is_the_money_spent_at_the_merchant() {
    let (mut world, killer, ground, amount) =
        kill_with_zen_drop().expect("a seed in 0..256 drops at least 20 zen");

    // The wallet starts empty; the kill pile is its only funding.
    assert_eq!(world.character(killer).zen(), zen(0));

    let picked = world.pickup_zen(killer, ground);
    assert_eq!(picked, ZenPickupOutcome::PickedUp);
    // Credited from the pile, cap-checked: the wallet is exactly the drop (V1/V7
    // mirror S-KILL for WorldZen — position from victim placement, map from
    // context, despawn a host-policy tick).
    assert_eq!(world.character(killer).zen(), zen(amount));

    // Buy the 20-zen potion from Elf Lala, standing on the merchant's tile.
    let merchant = world.character(killer).placement().position;
    let slot = ShelfSlot::new(0).expect("shelf slot 0 is valid");
    match world.buy(killer, ELF_LALA, slot, merchant) {
        BuyOutcome::NewItem { balance, .. } | BuyOutcome::Merged { balance, .. } => {
            // Balance = earned − cost, and the wallet threaded pickup → buy is
            // that same CarriedZen value across the persist round-trip. No
            // fixture wallet: the balance spent is provably the balance earned.
            assert_eq!(balance, zen(amount - 20));
            assert_eq!(world.character(killer).zen(), zen(amount - 20));
        }
        BuyOutcome::OutOfRange
        | BuyOutcome::UnknownShelfSlot
        | BuyOutcome::InventoryFull
        | BuyOutcome::InsufficientZen => panic!("in range with the earned zen, the buy lands"),
    }
}

#[test]
fn sale_proceeds_fund_a_chaos_mix_fee_across_one_wallet() {
    let mut world = World::new(24, MapNumber(0));
    let knight = world.seat_character(dark_knight(30, 150, tile(10, 10)));

    // The wallet is empty; selling a Cape of Lord funds it.
    let cape = item_instance(world.atlas(), CAPE_OF_LORD);
    let footprint = footprint_of(world.atlas(), CAPE_OF_LORD);
    let anchor = world
        .inventory(knight)
        .first_fit(footprint)
        .expect("the empty bag has room for the cape");
    assert!(matches!(
        world.place_in_bag(knight, cape, footprint, anchor),
        PlaceOutcome::Placed { .. }
    ));
    let merchant = world.character(knight).placement().position;
    let post_sale = match world.sell(knight, anchor, merchant) {
        SellOutcome::Sold { balance, .. } => balance,
        SellOutcome::OutOfRange | SellOutcome::NoItemAtCell | SellOutcome::WalletFull => {
            panic!("a merchant on the tile buys the cape")
        }
    };
    assert_eq!(world.character(knight).zen(), post_sale);

    // The same wallet now pays a chaos-weapon mix fee (an option-bearing sword +
    // Jewel of Chaos → a chaos-weapon sacrifice).
    let sword = {
        let mut sword = item_at_level(world.atlas(), SWORD, 6);
        sword.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L1,
        });
        sword
    };
    let placed = vec![sword, item_instance(world.atlas(), JEWEL_OF_CHAOS)];
    let (fee, after) = match world.mix(knight, placed) {
        MixOutcome::Success { fee, zen, .. } | MixOutcome::Failed { fee, zen, .. } => (fee, zen),
        MixOutcome::Rejected { .. } => panic!("a funded chaos-weapon window is a real recipe"),
    };
    // The fee was charged against exactly the post-sale balance, and the mix's
    // reported balance is that minus the fee — sell earns, mix spends, no fixture
    // wallet in between.
    assert_eq!(after, zen(post_sale.get() - fee.0));
    assert_eq!(world.character(knight).zen(), after);
}

#[test]
fn a_pile_one_over_the_carry_cap_is_refused_and_stays_whole_on_the_ground() {
    let mut world = World::new(9, MapNumber(0));
    let knight = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    world.set_wallet(knight, zen(1_999_999_999));

    let ground = world.seat_ground_zen(Zen(2), pos(10, 10), Tick(1200));
    let before = world.ground_zen(ground).clone();

    match world.pickup_zen(knight, ground) {
        ZenPickupOutcome::OverCap { world_zen } => {
            // The handed-back pile is byte-identical to the one on the ground —
            // never clamped or split (P3: the returned balance is authoritative).
            assert_eq!(
                serde_json::to_string(&persist(world_zen)).unwrap(),
                serde_json::to_string(&before).unwrap(),
            );
        }
        ZenPickupOutcome::PickedUp => panic!("a pile one over the cap is refused whole"),
    }

    // The wallet is unchanged and the untouched pile still lies on the ground.
    assert_eq!(world.character(knight).zen(), zen(1_999_999_999));
    assert_eq!(
        serde_json::to_string(world.ground_zen(ground)).unwrap(),
        serde_json::to_string(&before).unwrap(),
    );
}

// --- Movement into/out of range-gated services (seam 4). ---------------------

#[test]
fn a_buy_that_failed_out_of_range_succeeds_after_walking_into_merchant_reach() {
    let mut world = World::new(2024, MapNumber(0));
    // A straight walkable corridor on real Lorencia terrain: the knight starts at
    // one end, the merchant stands eleven tiles away at the far end.
    let run = walkable_run(world.atlas(), MapNumber(0), 12);
    let start = run[0];
    let merchant = run[11].to_world();
    let knight = world.seat_character(dark_knight(30, 150, start));
    world.set_wallet(knight, zen(500_000));
    let slot = ShelfSlot::new(0).expect("shelf slot 0 is valid");

    // Eleven tiles out (past the 3-tile reach): the buy fails, nothing changes.
    assert_eq!(
        world.buy(knight, ELF_LALA, slot, merchant),
        BuyOutcome::OutOfRange
    );

    // Walk the knight toward the merchant, persisting each Placement, until in
    // the 3-tile reach — every step lands on the walkable run, never Blocked.
    let reach = Radius::from_tiles(3);
    while !world
        .character(knight)
        .placement()
        .position
        .within_range(merchant, reach)
    {
        match world.step(knight, merchant) {
            StepOutcome::Resolved { .. } => {}
            StepOutcome::Blocked => panic!("the walkable run must not block a step"),
        }
    }

    // The buy now succeeds — gated by the post-step Placement, not a fixture.
    match world.buy(knight, ELF_LALA, slot, merchant) {
        BuyOutcome::NewItem { .. } | BuyOutcome::Merged { .. } => {}
        BuyOutcome::OutOfRange
        | BuyOutcome::UnknownShelfSlot
        | BuyOutcome::InventoryFull
        | BuyOutcome::InsufficientZen => panic!("in reach with zen, the buy lands"),
    }
    assert!(
        world
            .character(knight)
            .placement()
            .position
            .within_range(merchant, reach),
        "the position that gated the buy is the walked-to placement"
    );
}

#[test]
fn walking_a_trader_out_of_range_mid_session_does_not_cancel_the_trade() {
    let mut world = World::new(7, MapNumber(0));
    let run = walkable_run(world.atlas(), MapNumber(0), 15);
    let home = run[0];
    let requester = world.seat_character(dark_knight(30, 150, home));
    let partner = world.seat_character(dark_knight(30, 150, home));
    let partner_pos = world.character(partner).placement().position;

    // The requester holds an item to offer.
    let armor = item_instance(world.atlas(), DRAGON_ARMOR);
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world
        .inventory(requester)
        .first_fit(footprint)
        .expect("the empty bag has room for the armor");
    world.place_in_bag(requester, armor.clone(), footprint, anchor);

    // Open + accept while co-located (well within the 12-tile trade reach).
    let session = world.open_and_accept_trade(requester, partner);

    // Walk the requester past 12 tiles from the partner, persisting each step.
    let far = run[14].to_world();
    let reach = Radius::from_tiles(12);
    while world
        .character(requester)
        .placement()
        .position
        .within_range(partner_pos, reach)
    {
        match world.step(requester, far) {
            StepOutcome::Resolved { .. } => {}
            StepOutcome::Blocked => panic!("the walkable run must not block a step"),
        }
    }

    // Post-accept, offer_item consults no position: it succeeds out of range,
    // and the session stays open with the offer escrowed — walking away cancels
    // nothing (only request/accept gate on range).
    assert_eq!(
        world.offer_item_to_trade(session, requester, Side::Requester, anchor, cell(0, 0)),
        OfferOutcome::Offered { at: cell(0, 0) }
    );
    assert!(matches!(world.session(session), TradeSession::Open { .. }));

    // And the escrow still settles cleanly on an explicit cancel: the armor
    // returns to the walked-away trader's own bag byte-for-byte, no overflow.
    let settlement = world.cancel_trade(session, CancelReason::Explicit, requester, partner);
    assert!(settlement.requester.overflow.items.is_empty());
    let returned = world.inventory(requester).placed();
    assert_eq!(returned.len(), 1);
    assert_eq!(
        serde_json::to_string(&returned[0].item).unwrap(),
        serde_json::to_string(&armor).unwrap(),
    );
}

// --- Craft output flows into the rest of the world (seam 8; V5). -------------

#[test]
fn a_chaos_mix_created_item_is_landed_in_the_bag_then_worn() {
    let mut world = World::new(24, MapNumber(0));
    let knight = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    world.set_wallet(knight, zen(5_000_000));

    // A +10 upgrade mix: the placed +9 helm levels in place (fee 2,000,000).
    let placed = vec![
        item_at_level(world.atlas(), HELM, 9),
        item_instance(world.atlas(), JEWEL_OF_CHAOS),
        item_instance(world.atlas(), JEWEL_OF_BLESS),
        item_instance(world.atlas(), JEWEL_OF_SOUL),
    ];
    let created = match world.mix(knight, placed) {
        MixOutcome::Success { created, .. } => created,
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed 24 passes the +10 rate")
        }
    };

    // Land the created item in the bag at a first-fit anchor (V5: place_item),
    // then take it into hand and wear it.
    let footprint = footprint_of(world.atlas(), created.item);
    let anchor = world
        .inventory(knight)
        .first_fit(footprint)
        .expect("the empty bag has room for the created item");
    assert!(matches!(
        world.place_in_bag(knight, created.clone(), footprint, anchor),
        PlaceOutcome::Placed { .. }
    ));
    let in_hand = match world.remove_from_bag(knight, anchor) {
        RemoveOutcome::Removed { item, .. } => item,
        RemoveOutcome::Rejected { .. } => panic!("the anchor holds the created item"),
    };
    let slot = match world.equip_first_available(knight, in_hand) {
        EquipOutcome::Equipped { slot } => slot,
        EquipOutcome::Rejected { .. } => panic!("the created helm is equippable"),
    };

    // The worn instance equals the created instance byte-for-byte after every hop.
    let worn = world
        .equipment(knight)
        .get(slot)
        .expect("the slot is filled")
        .clone();
    assert_eq!(
        serde_json::to_string(&worn).unwrap(),
        serde_json::to_string(&created).unwrap(),
    );
}

#[test]
fn crafted_wings_equipped_make_the_character_flight_eligible() {
    let mut world = World::new(24, MapNumber(0));
    let flyer = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    world.set_wallet(flyer, zen(10_000_000));

    // A Second Wings mix: a first wing + Loch's Feather + Jewel of Chaos → a
    // second wing (fee 5,000,000).
    let placed = vec![
        item_instance(world.atlas(), FAIRY_WINGS),
        item_instance(world.atlas(), LOCHS_FEATHER),
        item_instance(world.atlas(), JEWEL_OF_CHAOS),
    ];
    let wing = match world.mix(flyer, placed) {
        MixOutcome::Success { created, .. } => created,
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed 24 passes the second-wings base rate")
        }
    };
    match world.equip_first_available(flyer, wing) {
        EquipOutcome::Equipped { slot } => assert_eq!(slot, EquipmentSlot::Wings),
        EquipOutcome::Rejected { .. } => panic!("a crafted wing is wearable"),
    }

    // With the crafted wing worn, change_flight lifts off; the Movement persists
    // airborne (the host derives Wings::Equipped from the worn wing slot).
    assert!(matches!(
        world
            .change_flight(flyer, FlightChange::EnableFlight)
            .as_slice(),
        [FlightOutcome::ModeChanged {
            mode: Movement::Flying
        }]
    ));
    assert_eq!(
        world.character(flyer).placement().movement,
        Movement::Flying
    );

    // A wingless knight is denied the same change and stays grounded.
    let grounded = world.seat_character(dark_knight(30, 150, tile(11, 11)));
    assert!(matches!(
        world
            .change_flight(grounded, FlightChange::EnableFlight)
            .as_slice(),
        [FlightOutcome::Denied {
            reason: FlightDenialReason::NoWings
        }]
    ));
    assert_eq!(
        world.character(grounded).placement().movement,
        Movement::Grounded
    );
}

#[test]
fn a_crafted_item_is_sold_back_and_priced_from_its_own_instance() {
    let mut world = World::new(24, MapNumber(0));
    let knight = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    world.set_wallet(knight, zen(5_000_000));

    let placed = vec![
        item_at_level(world.atlas(), HELM, 9),
        item_instance(world.atlas(), JEWEL_OF_CHAOS),
        item_instance(world.atlas(), JEWEL_OF_BLESS),
        item_instance(world.atlas(), JEWEL_OF_SOUL),
    ];
    let created = match world.mix(knight, placed) {
        MixOutcome::Success { created, .. } => created,
        MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => {
            panic!("seed 24 passes the +10 rate")
        }
    };

    // The sale is priced from the created instance itself.
    let expected = {
        let def = world.atlas().item(created.item).expect("a real item");
        selling_price(def, &created)
    };
    let footprint = footprint_of(world.atlas(), created.item);
    let anchor = world
        .inventory(knight)
        .first_fit(footprint)
        .expect("the empty bag has room");
    world.place_in_bag(knight, created.clone(), footprint, anchor);

    let merchant = world.character(knight).placement().position;
    match world.sell(knight, anchor, merchant) {
        SellOutcome::Sold { proceeds, balance } => {
            assert_eq!(proceeds, expected, "priced from the crafted instance");
            assert_eq!(world.character(knight).zen(), balance);
        }
        SellOutcome::OutOfRange | SellOutcome::NoItemAtCell | SellOutcome::WalletFull => {
            panic!("a merchant on the tile buys the crafted item")
        }
    }
    // Destroyed by value: no retained copy remains in the bag.
    assert!(world.inventory(knight).placed().is_empty());
}

// --- A rolled item keeps its identity through a live journey (seam 10). -------

#[test]
fn an_excellent_rolled_item_survives_the_journey_into_a_partners_bag() {
    let mut world = World::new(24, MapNumber(0));
    let alpha = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let beta = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    world.set_wallet(alpha, zen(500_000));

    // An excellent Dragon Armor rolled as a kill's Drop::Item, laid on the ground.
    let (ground, rolled) = world.drop_item_to_ground(
        DRAGON_ARMOR,
        ItemLevel::new(9).expect("level 9 is valid"),
        ItemRarity::Excellent,
        pos(10, 10),
        Tick(1200),
    );
    assert!(
        matches!(rolled.roll, RarityRoll::Excellent { .. }),
        "the drop rolled a real excellent instance"
    );

    // Player A picks it up.
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world
        .inventory(alpha)
        .first_fit(footprint)
        .expect("the empty bag has room");
    assert_eq!(
        world.pickup(alpha, ground, anchor),
        PickupOutcome::PickedUp { at: anchor }
    );

    // A opens a trade with B, offers the item and some zen, and both lock.
    let session = world.open_and_accept_trade(alpha, beta);
    assert_eq!(
        world.offer_item_to_trade(session, alpha, Side::Requester, anchor, cell(0, 0)),
        OfferOutcome::Offered { at: cell(0, 0) }
    );
    assert!(matches!(
        world.offer_zen_to_trade(session, alpha, Side::Requester, Zen(100_000)),
        ZenOfferOutcome::Offered { .. }
    ));
    assert!(matches!(
        world.lock_trade(session, alpha, beta, Side::Partner),
        LockResult::Locked { .. }
    ));
    assert_eq!(
        world.lock_trade(session, alpha, beta, Side::Requester),
        LockResult::Completed
    );

    // The excellent item now lives in B's bag, byte-for-byte the rolled instance
    // — its excellent set, skill, augment, and durability all intact through
    // roll → pickup → trade → the partner's bag.
    let landed = world.inventory(beta).placed();
    assert_eq!(landed.len(), 1);
    assert_eq!(
        serde_json::to_string(&landed[0].item).unwrap(),
        serde_json::to_string(&rolled).unwrap(),
    );
    // The offered zen crossed too; A kept the remainder — one settled ledger.
    assert_eq!(world.character(beta).zen(), zen(100_000));
    assert_eq!(world.character(alpha).zen(), zen(400_000));
}

// --- Effects are orthogonal to trade; heal reconstruction (seam 9; V4). -------

#[test]
fn a_poisoned_and_buffed_character_trades_exactly_as_a_healthy_one() {
    // The trade window is effect-blind. The identical full trade lifecycle, run
    // once effect-free and once on a requester carrying BOTH a poison ailment and
    // a defense buff, produces the identical outcome sequence — and the effects
    // thread every persist-seam step unchanged (each wallet write re-serialises
    // the whole character, effects included). Trade reads no effect state.
    let lifecycle = |effects: bool| {
        let mut world = World::new(2024, MapNumber(0));
        let requester = world.seat_character(dark_knight(30, 150, tile(10, 10)));
        let partner = world.seat_character(dark_knight(30, 150, tile(10, 10)));
        world.set_wallet(requester, zen(200_000));

        let applied = effects.then(|| {
            let ailment = world.apply_ailment_to(requester, Ailment::Poisoned, 120, Tick(0));
            let buff = world.apply_buff_to(requester, ApplicableBuff::Defense, 120, Tick(0));
            (ailment, buff)
        });

        // The requester holds an item to offer, then the pair completes a trade.
        let armor = item_instance(world.atlas(), DRAGON_ARMOR);
        let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
        let anchor = world
            .inventory(requester)
            .first_fit(footprint)
            .expect("the empty bag has room for the armor");
        world.place_in_bag(requester, armor, footprint, anchor);

        let session = world.open_and_accept_trade(requester, partner);
        let offer =
            world.offer_item_to_trade(session, requester, Side::Requester, anchor, cell(0, 0));
        let zen_offer = world.offer_zen_to_trade(session, requester, Side::Requester, Zen(50_000));
        let partner_lock = world.lock_trade(session, requester, partner, Side::Partner);
        let requester_lock = world.lock_trade(session, requester, partner, Side::Requester);

        // The requester's effect store after the whole lifecycle (post-persist).
        let surviving = world.character(requester).active_effects();
        (
            offer,
            zen_offer,
            partner_lock,
            requester_lock,
            applied,
            surviving,
        )
    };

    let (co, cz, cpl, crl, control_applied, control_surviving) = lifecycle(false);
    let (to, tz, tpl, trl, treatment_applied, treatment_surviving) = lifecycle(true);

    // Effect-blind: every trade outcome is identical across the two runs, and the
    // trade really completed.
    assert_eq!(crl, LockResult::Completed);
    assert_eq!((co, cz, cpl, crl), (to, tz, tpl, trl));

    // The control carried nothing; the treatment carried both a poison ailment and
    // a defense buff — and both survived the whole trade untouched, since the
    // window never reads or writes the effect store.
    assert_eq!(control_applied, None);
    assert_eq!(control_surviving, ActiveEffects::EMPTY);
    let (ailment, buff) = treatment_applied.expect("the treatment run applied both effects");
    assert_eq!(
        treatment_surviving,
        ActiveEffects::EMPTY.with(ailment).with(buff)
    );
}

#[test]
fn poison_can_reach_a_killing_tick_but_no_reward_pathway_exists() {
    // A poisoned character's health drains on the effect tick alone. A lethal tick
    // returns EffectEvent::PoisonKilled — but mu-core has NO poison-kill reward
    // pathway: no resolve_kill overload a PoisonKilled routes into, so it awards no
    // exp and no drops (spec §5.2, V3b — a GENUINE GAP). The harness drives to the
    // killing tick and STOPS, asserting the absence rather than faking a reward. A
    // host ends any open trade on this death via cancel(CancelReason::Died), which
    // S-REACH-2 already proves settles totally.
    let mut world = World::new(9, MapNumber(0));
    let knight = world.seat_character(dark_knight(30, 150, tile(10, 10)));

    // A low-health knight — a saved character loaded near death (the persist seam,
    // load direction) — with its reward baseline captured.
    world.set_health(knight, Pool::new(10, 500).unwrap());
    let start_exp = world.character(knight).experience();
    let start_zen = world.character(knight).zen();

    // Poison scaled off a strong caster: 1 + 120/9 = 14 per tick, lethal on the
    // first tick against 10 health.
    world.apply_ailment_to(knight, Ailment::Poisoned, 120, Tick(0));

    // Advance the effect store past the first scheduled tick — it fires and kills.
    let events = world.advance_effects_on(knight, Tick(1_000));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, EffectEvent::PoisonKilled { .. })),
        "the poison drained the last of the knight's health"
    );

    // The knight is dead: health zeroed by the effect tick, the store cleared.
    assert_eq!(world.character(knight).vitals().health.current(), 0);
    assert_eq!(
        world.character(knight).active_effects(),
        ActiveEffects::EMPTY
    );

    // The end of the road — NO service turned PoisonKilled into a reward. Nothing
    // was awarded and nothing dropped: exp and zen are as created, the ground is
    // bare. Only the killed character's persist round-trip is left to prove.
    assert_eq!(world.character(knight).experience(), start_exp);
    assert_eq!(world.character(knight).zen(), start_zen);
    assert_eq!(world.ground_item_count(), 0);
    let dead = world.character(knight).clone();
    assert_eq!(
        serde_json::to_string(&dead).unwrap(),
        serde_json::to_string(&persist(dead.clone())).unwrap(),
    );
}

#[test]
fn a_heal_reconstructs_the_receivers_health_from_the_returned_pre_clamped_amount() {
    // V4: cast_heal returns only the pre-clamped `amount` restored, never the
    // post-heal pool. The host reconstructs it by crediting `amount` to the SAME
    // Pool it passed in as receiver_health — and current + amount <= max holds only
    // against that very pool (the amount is pre-clamped against it).
    let mut world = World::new(1, MapNumber(0));
    let caster = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let receiver = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let heal = heal_skill(world.atlas());

    // A Dark Knight's energy is 30, so a heal restores 5 + 30/5 = 11.
    // Below max: the full 11 lands, no clamp.
    world.set_health(receiver, Pool::new(100, 500).unwrap());
    let (below, reconstructed) = world.cast_heal_on(caster, receiver, heal);
    let below_amount = match below {
        BuffCastOutcome::Healed { amount } => amount,
        BuffCastOutcome::Rejected { .. } | BuffCastOutcome::Applied { .. } => {
            panic!("an affordable heal restores health")
        }
    };
    assert_eq!(below_amount, 11);
    assert_eq!(reconstructed.current(), 100 + below_amount);
    assert!(reconstructed.current() <= reconstructed.max());
    // The reconstructed pool is what the receiver now carries (post-persist).
    assert_eq!(
        world.character(receiver).vitals().health.current(),
        reconstructed.current()
    );
    // The caster spent the heal's 20 mana (K1), persisted through the seam.
    assert_eq!(world.character(caster).vitals().mana.current(), 380);

    // Near max: only 3 room, so the 11 clamps to 3 and current + amount == max.
    world.set_health(receiver, Pool::new(497, 500).unwrap());
    let (near, clamped) = world.cast_heal_on(caster, receiver, heal);
    let near_amount = match near {
        BuffCastOutcome::Healed { amount } => amount,
        BuffCastOutcome::Rejected { .. } | BuffCastOutcome::Applied { .. } => {
            panic!("an affordable heal restores health")
        }
    };
    assert_eq!(near_amount, 3);
    assert_eq!(clamped.current(), 497 + near_amount);
    assert_eq!(clamped.current(), clamped.max());
}

// --- Two actors contend for one world (seam 11). -----------------------------

#[test]
fn the_first_actors_pickup_consumes_the_only_ground_item_the_second_finds_nothing() {
    // Two actors on one map, one ground item. The move-only WorldItem makes
    // double-pickup unrepresentable at the world level — A's pickup consumes it
    // (the driver removes it from the ground set), so B finds nothing there. The
    // item exists in exactly one place: moved, never copied.
    let mut world = World::new(2024, MapNumber(0));
    let alpha = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let beta = world.seat_character(dark_knight(30, 150, tile(10, 10)));

    // Exactly one item on the ground.
    let armor = item_instance(world.atlas(), DRAGON_ARMOR);
    let ground = world.seat_ground_item(armor.clone(), pos(10, 10), Tick(1200));
    assert_eq!(world.ground_item_count(), 1);

    // A picks it up into its first-fit anchor.
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world
        .inventory(alpha)
        .first_fit(footprint)
        .expect("A's empty bag has room");
    assert_eq!(
        world.pickup(alpha, ground, anchor),
        PickupOutcome::PickedUp { at: anchor }
    );

    // Gone from the ground (B finds nothing), present exactly once in A's bag, and
    // byte-for-byte the seated instance; B's bag is empty.
    assert_eq!(world.ground_item_count(), 0);
    let a_placed = world.inventory(alpha).placed();
    assert_eq!(a_placed.len(), 1);
    assert_eq!(
        serde_json::to_string(&a_placed[0].item).unwrap(),
        serde_json::to_string(&armor).unwrap(),
    );
    assert!(world.inventory(beta).placed().is_empty());
}

#[test]
fn a_mob_chasing_one_actor_retargets_the_other_when_it_becomes_the_target() {
    // AI targeting is a host decision fed as input. A mob whose attack decision was
    // keyed to A's position this tick retargets to B's when B is the supplied
    // target next tick — the same instance, the Attack intent's `target` flipping
    // verbatim from A to B.
    let mut world = World::new(7, MapNumber(0));
    let number = aggressive_monster(world.atlas());
    let mob = world.seat_monster(monster_instance(number, 100, tile(10, 10)));

    // Two actors cardinal-adjacent to the mob (both within any attack range >= 1).
    let alpha = pos(11, 10);
    let beta = pos(9, 10);

    // Tick one: aimed at A -> Attack targeting A.
    let first = world.advance_monster(mob, Some(alpha), Tick(1));
    assert_eq!(first, MonsterIntent::Attack { target: alpha });

    // The advanced instance round-trips through persist, still the same number.
    let after_first = world.monster(mob);
    assert_eq!(
        serde_json::to_string(&after_first).unwrap(),
        serde_json::to_string(&persist(after_first)).unwrap(),
    );
    assert_eq!(after_first.number, number);

    // Tick two: the very same instance, now aimed at B -> Attack targeting B.
    let second = world.advance_monster(mob, Some(beta), Tick(100_000));
    assert_eq!(second, MonsterIntent::Attack { target: beta });

    // The target flipped from A to B; one identity threaded both ticks.
    assert_ne!(first, second);
    assert_eq!(world.monster(mob).number, number);
}

// --- The capstone: a whole run replays identically within a target (seam 5). --

/// One ordered step of the composed run, recorded by its canonical wire form — a
/// derived-`PartialEq` element (the ambient `Frame` pattern), so a replay
/// divergence localises to the exact step rather than hiding in a monolithic
/// snapshot diff.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TraceStep {
    label: &'static str,
    detail: String,
}

/// Drives one whole multi-system run over a single seeded stream in one fixed
/// order — spawn -> AI -> step -> strike -> kill -> drop -> pickup -> sell —
/// reusing the cluster A/B drive methods and threading the world's one `TestRng`
/// through every randomised call. Returns the final live-state snapshot and the
/// ordered event trace. The construction seed is the only entropy source and is
/// chosen by the caller OUTSIDE this driver (never re-drawn mid-run), so two calls
/// with the same seed reproduce bit-for-bit. This is a WITHIN-TARGET determinism
/// driver — cross-target (native vs wasm) reproduction is `wasm_determinism.rs`'s
/// job, and it cannot join this real-`/data` run.
fn scripted_run(seed: u64) -> (String, Vec<TraceStep>) {
    let mut world = World::new(seed, MapNumber(0));
    let mut trace = Vec::new();

    // A straight walkable corridor so the step lands (never Blocked).
    let run = walkable_run(world.atlas(), MapNumber(0), 6);
    let (Some(start), Some(mob_tile)) = (run.first().copied(), run.get(5).copied()) else {
        return (world.snapshot(), trace);
    };

    let killer = world.seat_character(dark_knight(80, 300, start));

    // 1. Spawn a fighting mob (RNG on the stream).
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let placement = SpawnPlacement::Fixed {
        position: mob_tile,
        facing: TileFacing::East,
    };
    let spawn = world.spawn_from(number, placement);
    trace.push(TraceStep {
        label: "spawn",
        detail: wire(&spawn.events),
    });
    let Some(Spawned::Mob { instance }) = spawn.spawned.first() else {
        return (world.snapshot(), trace);
    };
    let mob = world.seat_monster(*instance);

    // 2. Advance the mob's AI two ticks (wander draws one heading word each).
    for now in [Tick(1_000), Tick(2_000)] {
        let intent = world.advance_monster(mob, None, now);
        trace.push(TraceStep {
            label: "ai",
            detail: wire(&intent),
        });
    }

    // 3. Step the killer one tile along the corridor (no RNG).
    let stepped = world.step(killer, mob_tile.to_world());
    trace.push(TraceStep {
        label: "step",
        detail: wire(&stepped),
    });

    // 4. Strike the mob to death (each hit rolls the stream).
    loop {
        let outcome = world.strike(killer, mob);
        let done = matches!(outcome, AttackOutcome::Killed { .. });
        trace.push(TraceStep {
            label: "strike",
            detail: wire(&outcome),
        });
        if done {
            break;
        }
    }

    // 5. Resolve the kill reward (exp jitter + drop rolls on the stream).
    let resolution = world.resolve_kill_of(killer, mob);
    trace.push(TraceStep {
        label: "kill",
        detail: wire(&resolution),
    });

    // 6. Materialise the money drop, if any, and pick it up.
    if let Some(amount) = zen_drop(&resolution) {
        let position = world.monster(mob).placement.position;
        let ground = world.seat_ground_zen(Zen(amount), position, Tick(6_000));
        let picked = world.pickup_zen(killer, ground);
        trace.push(TraceStep {
            label: "pickup_zen",
            detail: wire(&picked),
        });
    }

    // 7. Materialise the first item drop, if any, pick it up, and sell it back.
    if let Some((item, level, rarity)) = item_drops(&resolution).into_iter().next() {
        let position = world.monster(mob).placement.position;
        let (ground, _rolled) =
            world.drop_item_to_ground(item, level, rarity, position, Tick(6_000));
        let footprint = footprint_of(world.atlas(), item);
        if let Some(anchor) = world.inventory(killer).first_fit(footprint) {
            let picked = world.pickup(killer, ground, anchor);
            let landed = matches!(picked, PickupOutcome::PickedUp { .. });
            trace.push(TraceStep {
                label: "pickup",
                detail: wire(&picked),
            });
            if landed {
                let merchant = world.character(killer).placement().position;
                let sold = world.sell(killer, anchor, merchant);
                trace.push(TraceStep {
                    label: "sell",
                    detail: wire(&sold),
                });
            }
        }
    }

    (world.snapshot(), trace)
}

#[test]
fn one_composed_run_replays_byte_for_byte_on_the_same_construction_seed() {
    // WITHIN-TARGET determinism: interleaving many RNG consumers (spawn, AI,
    // combat, kill, item roll) on ONE seeded stream is stable across a replay from
    // the same construction seed — the one property no per-system replay can catch.
    // Cross-target (native vs wasm) is a different proof, carried per-seam by
    // wasm_determinism.rs, which is filesystem-free and cannot join this run.
    let (first, _) = scripted_run(2024);
    let (second, _) = scripted_run(2024);
    assert_eq!(
        first, second,
        "same construction seed replays byte-for-byte"
    );

    // The seed is load-bearing, not inert: a different construction seed drives a
    // different run (guards against a degenerate all-same-output stream).
    let (other, _) = scripted_run(7);
    assert_ne!(
        first, other,
        "a different seed yields a different final world"
    );
}

#[test]
fn the_ordered_event_trace_is_identical_across_same_seed_replays() {
    // The trace is the PRIMARY diagnostic — a Vec equality localises any drift to
    // the exact step, where the monolithic snapshot-string diff of the sibling
    // scenario is near-unreadable. Both are kept: the snapshot for totality, the
    // trace for locality.
    let (_, first) = scripted_run(2024);
    let (_, second) = scripted_run(2024);
    assert_eq!(first, second, "the ordered event trace replays identically");

    // And it is a real, non-trivial multi-system trace, not an empty log.
    assert!(
        first.len() > 3,
        "the composed run produced a substantive trace"
    );
}

// --- Damaging-skill kill chain (seam 9; V4 offence twin). --------------------

/// Everything a skill-kill loot chain produced: the world after the kill, the
/// actor/target/ground indices, the rolled drop and its identity, the caster's
/// mana across the fight, and the experience the kill granted.
struct SkillKill {
    world: World,
    caster: usize,
    victim: usize,
    ground: usize,
    rolled: ItemInstance,
    item: ItemRef,
    mana_before: u32,
    mana_after: u32,
    gained: u64,
}

/// Casts a real DAMAGING skill in a loop until it lands the killing blow, then
/// resolves the reward and grounds the first equippable item it dropped —
/// sweeping the construction seed (outside the drive loop) until a skill-kill
/// yields such a drop. `None` only if no seed in the range drops an equippable
/// item — statistically impossible over an equipment-heavy pool.
fn skill_kill_with_item_drop() -> Option<SkillKill> {
    for seed in 0u64..128 {
        let mut world = World::new(seed, MapNumber(0));
        let caster = world.seat_character(dark_knight(80, 300, tile(10, 10)));
        let skill = direct_hit_skill(world.atlas());
        let (number, combat, _resistances) = low_level_monster(world.atlas(), 20);
        let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));
        let aim = world.character(caster).placement().position;
        let mana_before = world.character(caster).vitals().mana.current();

        // Cast the damaging skill onto the mob until a Killed hit lands — each
        // cast spends the skill's mana and drains the target through the writeback.
        let mut killed = false;
        for _ in 0..10_000u32 {
            match world.cast_damaging(caster, skill, aim, &[victim]) {
                SkillOutcome::Cast { hits, .. } => {
                    if hits
                        .iter()
                        .any(|hit| matches!(hit, TargetHit::Killed { .. }))
                    {
                        killed = true;
                        break;
                    }
                }
                SkillOutcome::Rejected { .. } => break,
            }
        }
        if !killed {
            continue;
        }

        let mana_after = world.character(caster).vitals().mana.current();
        let resolution = world.resolve_kill_of(caster, victim);
        let gained = resolution.experience.gained.0;
        let drop = item_drops(&resolution)
            .into_iter()
            .find(|candidate| is_equippable(world.atlas(), candidate.0));
        let Some((item, level, rarity)) = drop else {
            continue;
        };
        let position = world.monster(victim).placement.position;
        let (ground, rolled) = world.drop_item_to_ground(item, level, rarity, position, Tick(1200));
        return Some(SkillKill {
            world,
            caster,
            victim,
            ground,
            rolled,
            item,
            mana_before,
            mana_after,
            gained,
        });
    }
    None
}

#[test]
fn a_damaging_skill_cast_kills_a_mob_pays_mana_and_its_loot_is_picked_up() {
    let SkillKill {
        mut world,
        caster,
        victim,
        ground,
        rolled,
        item,
        mana_before,
        mana_after,
        gained,
    } = skill_kill_with_item_drop()
        .expect("a seed in 0..128 lands a skill-kill that drops an item");

    // Mana paid: each damaging cast spent the skill's mana, so the caster's mana
    // strictly fell across the fight.
    assert!(
        mana_after < mana_before,
        "the damaging casts spent the caster's mana"
    );

    // Killed BY THE SKILL: the victim's health, persisted after each cast's
    // writeback, reached zero — the basic-attack drive method was never called.
    assert_eq!(world.monster(victim).health.current(), 0);

    // The skill-kill produced a real reward: experience and at least one drop.
    assert!(gained > 0, "the skill-kill grants experience");

    // The dropped item is picked up into the bag, byte-identical to the rolled
    // instance — the skill-kill loot chain closes.
    let footprint = footprint_of(world.atlas(), item);
    let anchor = world
        .inventory(caster)
        .first_fit(footprint)
        .expect("the empty bag has room");
    assert_eq!(world.ground_item_count(), 1);
    assert_eq!(
        world.pickup(caster, ground, anchor),
        PickupOutcome::PickedUp { at: anchor }
    );
    assert_eq!(world.ground_item_count(), 0);
    let bagged = world
        .inventory(caster)
        .occupant(anchor)
        .expect("the bag holds the picked item")
        .item
        .clone();
    assert_eq!(wire(&bagged), wire(&rolled));
}

#[test]
fn a_nova_cast_kills_a_cluster_and_each_kill_pays_out_its_own_reward() {
    // One area cast over two seated mobs kills both — multi-kill loot no other
    // test touches. Swept over the construction seed until one nova lands a lethal
    // hit on both frail mobs (a miss on either just tries the next seed).
    let mut proven = false;
    for seed in 0u64..64 {
        let mut world = World::new(seed, MapNumber(0));
        let caster = world.seat_character(dark_knight(80, 300, tile(10, 10)));
        let nova = nova_skill(world.atlas());
        let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
        // Two frail mobs flanking the caster, both inside the caster-centred disc.
        let mob_a = world.seat_monster(monster_instance(number, 1, tile(11, 10)));
        let mob_b = world.seat_monster(monster_instance(number, 1, tile(10, 11)));
        let aim = world.character(caster).placement().position;
        let mana_before = world.character(caster).vitals().mana.current();

        let outcome = world.cast_damaging(caster, nova, aim, &[mob_a, mob_b]);
        assert!(
            matches!(outcome, SkillOutcome::Cast { .. }),
            "a funded nova over two mobs resolves"
        );

        // Both frail mobs must be dead for this seed to prove the multi-kill.
        if world.monster(mob_a).health.current() != 0 || world.monster(mob_b).health.current() != 0
        {
            continue;
        }

        // The one cast paid its mana once, and each kill pays out its own reward.
        assert!(
            world.character(caster).vitals().mana.current() < mana_before,
            "the nova cast spent mana"
        );
        let reward_a = world.resolve_kill_of(caster, mob_a);
        let reward_b = world.resolve_kill_of(caster, mob_b);
        assert!(
            reward_a.experience.gained.0 > 0,
            "the first kill pays experience"
        );
        assert!(
            reward_b.experience.gained.0 > 0,
            "the second kill pays experience"
        );
        proven = true;
        break;
    }
    assert!(proven, "a seed in 0..64 lands a lethal nova on both mobs");
}

// --- Equip gating: the refused branch W-SIM never asserted (seam 1). ---------

#[test]
fn equip_is_gated_by_kind_and_occupancy_and_a_refusal_leaves_the_worn_set_intact() {
    let mut world = World::new(2024, MapNumber(0));
    let knight = world.seat_character(dark_knight(80, 300, tile(10, 10)));

    // A — IncompatibleSlot: a non-wearable jewel is refused by a real slot, and
    // the empty worn set is left byte-for-byte unchanged.
    let empty_worn = wire(world.equipment(knight));
    let jewel = item_instance(world.atlas(), JEWEL_OF_BLESS);
    match world.equip_into(knight, jewel, EquipmentSlot::Helm) {
        EquipOutcome::Rejected { reason, .. } => {
            assert_eq!(reason, EquipRejection::IncompatibleSlot);
        }
        EquipOutcome::Equipped { .. } => panic!("a jewel is wearable in no slot"),
    }
    assert_eq!(
        wire(world.equipment(knight)),
        empty_worn,
        "a refused incompatible equip leaves the worn set untouched"
    );

    // B — SlotOccupied: a helm worn, a second helm refused from the same slot,
    // and the one-helm worn set left unchanged.
    let helm = item_instance(world.atlas(), HELM);
    assert!(matches!(
        world.equip_into(knight, helm, EquipmentSlot::Helm),
        EquipOutcome::Equipped {
            slot: EquipmentSlot::Helm
        }
    ));
    let helm_worn = wire(world.equipment(knight));
    let second_helm = item_instance(world.atlas(), HELM);
    match world.equip_into(knight, second_helm, EquipmentSlot::Helm) {
        EquipOutcome::Rejected { reason, .. } => assert_eq!(reason, EquipRejection::SlotOccupied),
        EquipOutcome::Equipped { .. } => panic!("the helm slot is already worn"),
    }
    assert_eq!(
        wire(world.equipment(knight)),
        helm_worn,
        "a refused second helm leaves the occupied slot unchanged"
    );

    // C — TwoHandedConflict (the 2h-while-shield conflict): a shield worn in one
    // hand, a two-hander refused from the paired hand, worn set left unchanged.
    let shield = item_instance(world.atlas(), SHIELD);
    assert!(matches!(
        world.equip_into(knight, shield, EquipmentSlot::LeftHand),
        EquipOutcome::Equipped {
            slot: EquipmentSlot::LeftHand
        }
    ));
    let shielded_worn = wire(world.equipment(knight));
    let two_hander = item_instance(world.atlas(), TWO_HANDED_SWORD);
    match world.equip_into(knight, two_hander, EquipmentSlot::RightHand) {
        EquipOutcome::Rejected { reason, .. } => {
            assert_eq!(reason, EquipRejection::TwoHandedConflict);
        }
        EquipOutcome::Equipped { .. } => {
            panic!("a two-hander cannot share a hand pair with a worn shield")
        }
    }
    assert_eq!(
        wire(world.equipment(knight)),
        shielded_worn,
        "a refused two-hander leaves the shield-in-hand set unchanged"
    );

    // D — the DOCUMENTED GAP: core's equip gates on kind->slot and two-handed
    // occupancy ONLY — never on an item's class list or its wear (stat/level)
    // requirements, which have no equip consumer in core. A class-mismatched item
    // is therefore ACCEPTED: an elf-only bow is worn by a Dark Knight.
    let elf_seat = world.seat_character(dark_knight(80, 300, tile(11, 11)));
    let bow = item_instance(world.atlas(), ELF_BOW);
    match world.equip_into(elf_seat, bow, EquipmentSlot::RightHand) {
        EquipOutcome::Equipped { slot } => assert_eq!(
            slot,
            EquipmentSlot::RightHand,
            "equip ignores the class list — the elf-only bow is worn by the Dark Knight"
        ),
        EquipOutcome::Rejected { reason, .. } => {
            panic!("equip does not gate on class, yet refused with {reason:?}")
        }
    }
}

// --- Heal as a survivability tool inside a fight (seam 9; V4). ---------------

/// Fights one mob while the player self-heals between blows, on construction
/// `seed`. `None` when this seed's exchange did not show a real dip-and-recover
/// win (the mob out-raced the heals, or never bit before dying) — the caller
/// sweeps the seed. Every proven-cooperating seed asserts the win, the survival,
/// and the mana spend as hard invariants.
fn heal_carries_the_player_through_a_fight(seed: u64) -> Option<()> {
    let mut world = World::new(seed, MapNumber(0));
    // A low-level knight — its defense is out-rated by the pressing mob, so the
    // bites land — but a high strength, so it kills a low-HP mob fast, before the
    // wounds outrun a heal-per-bite.
    let player = world.seat_character(dark_knight(20, 250, tile(10, 10)));
    let heal = heal_skill(world.atlas());
    // A pressing mob co-located, seated with a low HP pool so the knight fells it
    // in a handful of blows; its attack rate out-rates the knight, so it draws
    // real blood the self-heal must mend.
    let mob = world.seat_monster(monster_instance(
        pressing_monster(world.atlas()),
        200,
        tile(10, 10),
    ));

    // Start the player wounded so the mob's bite and the heal are both visible.
    world.set_health(player, or_abort(Pool::new(450, 500)));
    let mana_before = world.character(player).vitals().mana.current();

    let mut bitten = false;
    let mut recovered = false;
    let mut won = false;
    for _ in 0..10_000u32 {
        // The mob bites the player; a lethal bite means it out-raced the heals
        // this seed — try another.
        let before_bite = world.character(player).vitals().health.current();
        if matches!(
            world.player_struck_by_monster(player, mob),
            AttackOutcome::Killed { .. }
        ) {
            return None;
        }
        let wounded = world.character(player).vitals().health.current();

        // The player heals only the wounds it actually takes — a landed bite is
        // mended by a self-heal cast between blows.
        if wounded < before_bite {
            bitten = true;
            match world.cast_heal_on(player, player, heal).0 {
                BuffCastOutcome::Healed { .. } => {
                    if world.character(player).vitals().health.current() > wounded {
                        recovered = true;
                    }
                }
                // Mana ran dry before the mob died — this seed can't prove the
                // survival loop; try another.
                BuffCastOutcome::Rejected { .. } | BuffCastOutcome::Applied { .. } => {
                    return None;
                }
            }
        }

        // The player strikes back.
        if matches!(world.strike(player, mob), AttackOutcome::Killed { .. }) {
            won = true;
            break;
        }
    }

    if !(won && bitten && recovered) {
        return None;
    }

    // The fight was won alive: the mob drew blood, a self-heal mended it, the mob
    // is dead, the player still breathes, and the heals spent mana.
    assert!(
        world.character(player).vitals().health.current() > 0,
        "the player survived the exchange"
    );
    assert_eq!(world.monster(mob).health.current(), 0, "the mob is dead");
    assert!(
        world.character(player).vitals().mana.current() < mana_before,
        "the self-heals spent mana"
    );
    Some(())
}

#[test]
fn a_self_heal_between_blows_carries_the_player_through_a_winning_fight() {
    let carried = (0u64..64).any(|seed| heal_carries_the_player_through_a_fight(seed).is_some());
    assert!(
        carried,
        "a seed in 0..64 lets the healed player win the fight alive"
    );
}

#[test]
fn a_bought_potion_is_drunk_to_heal_after_a_monster_bite_closing_the_consume_loop() {
    // The CONSUMABLE-USE loop the town shops opened: buy → hurt → drink → heal,
    // on one threaded identity over the real /data record. No heal rule lives
    // host-side — the paper host reads live state, calls the real service, and
    // persists what it returns.
    let mut world = World::new(7, MapNumber(0));
    // A low-level knight, whose defense the pressing mob out-rates so the bites
    // land and draw the blood a potion must mend.
    let player = world.seat_character(dark_knight(20, 250, tile(10, 10)));
    // Fund the wallet so the small-HP-potion (83 zen) buy lands.
    world.set_wallet(player, zen(1_000));

    // Buy the small HP potion from Elf Lala. Slot 0 is her 20-zen apple; the
    // small HP potion is shelf slot 1 (the classic small-healing pack).
    let merchant = world.character(player).placement().position;
    let slot = ShelfSlot::new(1).expect("shelf slot 1 is valid");
    let potion_cell = match world.buy(player, ELF_LALA, slot, merchant) {
        BuyOutcome::NewItem { at, .. } => at,
        BuyOutcome::Merged { .. }
        | BuyOutcome::OutOfRange
        | BuyOutcome::UnknownShelfSlot
        | BuyOutcome::InventoryFull
        | BuyOutcome::InsufficientZen => panic!("the funded, in-range buy lands a fresh potion"),
    };

    // A pressing mob co-located draws blood — the hero drops below full health.
    let mob = world.seat_monster(monster_instance(
        pressing_monster(world.atlas()),
        500,
        tile(10, 10),
    ));
    let full = world.character(player).vitals().health.max();
    let mut wounded = false;
    for _ in 0..10_000u32 {
        assert!(
            !matches!(
                world.player_struck_by_monster(player, mob),
                AttackOutcome::Killed { .. }
            ),
            "a single bite cannot fell a full-health level-20 knight"
        );
        if world.character(player).vitals().health.current() < full {
            wounded = true;
            break;
        }
    }
    assert!(wounded, "the pressing mob drew blood");
    let hurt = world.character(player).vitals().health.current();

    // Drink the potion: health rises off the real record (never a host-invented
    // amount), and the single-piece stack empties the cell.
    match world.use_consumable(player, potion_cell).as_slice() {
        [
            ConsumeEvent::Recovered {
                pool: PoolKind::Health,
                restored,
            },
        ] => {
            assert!(*restored > 0, "a hurt hero's HP potion restores health");
            assert_eq!(
                world.character(player).vitals().health.current(),
                hurt + restored,
                "health rose by exactly the restored delta"
            );
        }
        other => panic!("a hurt hero drinking an HP potion recovers health: {other:?}"),
    }
    assert!(
        world.character(player).vitals().health.current() > hurt,
        "the drink carried the hero back up"
    );
    assert!(
        world.inventory(player).occupant(potion_cell).is_none(),
        "the single potion left the bag — no zero-count ghost"
    );

    // The healed character survived the persist seam byte-for-byte.
    let stored = world.character(player);
    assert_eq!(
        serde_json::to_string(stored).unwrap(),
        serde_json::to_string(&persist(stored.clone())).unwrap(),
    );
}

// --- Warp / travel: menu, discovery, fees, and the scroll (seam W-WARP). -----

/// The Lorencia warp entry (Move.txt index 2): level 10, 2,000 zen, map 0.
const LORENCIA_WARP: WarpIndex = WarpIndex(2);

/// The first Lost Tower warp entry (Move.txt index 8): level 50, 5,000 zen,
/// map 4.
const LOST_TOWER_WARP: WarpIndex = WarpIndex(8);

/// Town Portal Scroll (group 14 number 10, durability 1) — Elf Lala shelves
/// it at slot 21 for 750 zen.
const TOWN_PORTAL: ItemRef = ItemRef {
    group: 14,
    number: 10,
};

/// The persisted menu's Lost Tower annotation for the character at `hero`.
fn lost_tower_status(world: &World, hero: usize) -> WarpAvailability {
    or_abort(
        world
            .warp_menu(hero)
            .into_iter()
            .find(|status| status.index == LOST_TOWER_WARP)
            .ok_or("the menu lists the Lost Tower entry"),
    )
    .availability
}

#[test]
fn a_hero_earns_attempts_a_warp_too_poor_then_earns_enough_and_warps() {
    // The WARP-COMMAND debt record's pre-written scenario, driven end-to-end:
    // earn → invoke a warp too poor (rejected, wallet intact) → earn enough →
    // warp (fee debited, arrived on a walkable tile of the target map).
    let mut world = World::new(31, MapNumber(0));
    let hero = world.seat_character(dark_knight(60, 150, tile(10, 10)));

    // Earn: a money pile picked off the ground — below the 2,000-zen fee.
    let pile = world.seat_ground_zen(Zen(1_500), pos(10, 10), Tick(1_000));
    assert_eq!(world.pickup_zen(hero, pile), ZenPickupOutcome::PickedUp);
    assert_eq!(world.character(hero).zen(), zen(1_500));

    // Too poor: refused with the authoritative numbers, the persisted wallet
    // intact, the hero unmoved.
    assert_eq!(
        world.warp(hero, LORENCIA_WARP),
        WarpTravelOutcome::NotEnoughZen {
            required: Zen(2_000),
            available: zen(1_500),
        }
    );
    assert_eq!(
        world.character(hero).zen(),
        zen(1_500),
        "never partially spent"
    );

    // Earn past the fee and warp: the fee comes off the persisted wallet and
    // the hero stands on a walkable tile of the target map.
    let pile = world.seat_ground_zen(Zen(3_000), pos(10, 10), Tick(1_000));
    assert_eq!(world.pickup_zen(hero, pile), ZenPickupOutcome::PickedUp);
    match world.warp(hero, LORENCIA_WARP) {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_eq!(balance, zen(2_500), "4,500 earned minus the 2,000 fee");
            assert_eq!(world.character(hero).zen(), balance);
            assert_eq!(world.character(hero).placement(), placement);
            let grid = or_abort(
                world
                    .atlas()
                    .walk_grid(placement.map)
                    .ok_or("the target map has a walk grid"),
            );
            assert!(grid.walkable(placement.position), "a walkable landing");
        }
        outcome @ (WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding) => panic!("the funded warp lands: {outcome:?}"),
    }
}

#[test]
fn discovery_locks_the_menu_until_a_walk_in_and_the_menu_warp_returns() {
    let mut world = World::new(32, MapNumber(0));
    // Seated on the Lorencia → Devias door's trigger (gate 18 at (5,38)).
    let hero = world.seat_character(dark_knight(60, 150, tile(5, 38)));
    world.set_wallet(hero, zen(20_000));

    // LOCKED: unvisited Lost Tower is closed for exactly one reason —
    // discovery — and the command agrees without touching the wallet.
    match lost_tower_status(&world, hero) {
        WarpAvailability::Locked { reasons } => {
            assert_eq!(
                reasons.iter().copied().collect::<Vec<_>>(),
                vec![WarpLockReason::NotDiscovered]
            );
        }
        WarpAvailability::Available => panic!("an unvisited map is locked"),
    }
    assert_eq!(
        world.warp(hero, LOST_TOWER_WARP),
        WarpTravelOutcome::NotDiscovered
    );
    assert_eq!(world.character(hero).zen(), zen(20_000));

    // WALK IN: through the world's own doors — Lorencia → Devias, then the
    // Devias → Lost Tower door at (2,248). Discovery never blocks a door.
    assert!(matches!(
        world.traverse_gate(hero),
        EnterGateOutcome::Arrived { .. }
    ));
    assert_eq!(world.character(hero).placement().map, MapNumber(2));
    world.place_at(hero, tile(2, 248));
    assert!(matches!(
        world.traverse_gate(hero),
        EnterGateOutcome::Arrived { .. }
    ));
    assert_eq!(world.character(hero).placement().map, MapNumber(4));
    assert!(world.character(hero).discovered().contains(MapNumber(4)));

    // UNLOCKED: the persisted discovered set flips the menu; the warp home
    // and the menu warp back both execute, each fee off the same wallet.
    assert!(matches!(
        lost_tower_status(&world, hero),
        WarpAvailability::Available
    ));
    match world.warp(hero, LORENCIA_WARP) {
        WarpTravelOutcome::Arrived { balance, .. } => assert_eq!(balance, zen(18_000)),
        outcome @ (WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding) => panic!("the home warp lands: {outcome:?}"),
    }
    assert_eq!(world.character(hero).placement().map, MapNumber(0));
    match world.warp(hero, LOST_TOWER_WARP) {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_eq!(balance, zen(13_000), "the 5,000 fee off the wallet");
            assert_eq!(placement.map, MapNumber(4));
            let grid = or_abort(
                world
                    .atlas()
                    .walk_grid(placement.map)
                    .ok_or("Lost Tower has a walk grid"),
            );
            assert!(grid.walkable(placement.position));
        }
        outcome @ (WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding) => {
            panic!("the unlocked menu entry warps: {outcome:?}")
        }
    }

    // The traveled character — multi-map discovered set and all — survives
    // the persist round-trip byte-for-byte and re-drives cleanly.
    let stored = world.character(hero);
    assert_eq!(
        serde_json::to_string(stored).unwrap(),
        serde_json::to_string(&persist(stored.clone())).unwrap(),
    );
}

#[test]
fn a_magic_gladiator_warps_a_gate_its_plain_level_misses_and_pays_the_full_fee() {
    let mut world = World::new(33, MapNumber(0));
    let gladiator = world.seat_character(magic_gladiator(40, tile(5, 38)));
    world.set_wallet(gladiator, zen(10_000));
    let knight = world.seat_character(dark_knight(40, 150, tile(5, 38)));
    world.set_wallet(knight, zen(10_000));

    // Both walk the same physical chain to Lost Tower — the doors' posted 15
    // and 40 are met by both (the MG through its 2/3 fraction, the DK exactly).
    for hero in [gladiator, knight] {
        assert!(matches!(
            world.traverse_gate(hero),
            EnterGateOutcome::Arrived { .. }
        ));
        world.place_at(hero, tile(2, 248));
        assert!(matches!(
            world.traverse_gate(hero),
            EnterGateOutcome::Arrived { .. }
        ));
        assert_eq!(world.character(hero).placement().map, MapNumber(4));
    }

    // The MG's effective floor(50·2/3) = 33 opens the level-50 menu entry —
    // and the persisted wallet shows the FULL, un-reduced fee.
    match world.warp(gladiator, LOST_TOWER_WARP) {
        WarpTravelOutcome::Arrived { balance, .. } => {
            assert_eq!(balance, zen(5_000), "10,000 minus the whole 5,000 fee");
            assert_eq!(world.character(gladiator).zen(), balance);
        }
        outcome @ (WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding) => {
            panic!("the MG's fraction opens the gate: {outcome:?}")
        }
    }
    // The Dark Knight at the very same level faces the posted 50 and is
    // refused, its persisted wallet intact.
    assert_eq!(
        world.warp(knight, LOST_TOWER_WARP),
        WarpTravelOutcome::LevelTooLow { required: 50 }
    );
    assert_eq!(world.character(knight).zen(), zen(10_000));
}

#[test]
fn a_bought_town_portal_scroll_carries_the_hero_home_with_everything_kept() {
    let mut world = World::new(34, MapNumber(0));
    // Seated on the Lorencia → Dungeon door's trigger (gate 1 at (121,232)).
    let hero = world.seat_character(dark_knight(60, 150, tile(121, 232)));
    world.set_wallet(hero, zen(1_000));

    // Buy the scroll from Elf Lala (shelf slot 21, 750 zen) before leaving.
    let merchant = world.character(hero).placement().position;
    let slot = ShelfSlot::new(21).expect("shelf slot 21 is valid");
    let scroll_cell = match world.buy(hero, ELF_LALA, slot, merchant) {
        BuyOutcome::NewItem { at, .. } => at,
        outcome @ (BuyOutcome::Merged { .. }
        | BuyOutcome::OutOfRange
        | BuyOutcome::UnknownShelfSlot
        | BuyOutcome::InventoryFull
        | BuyOutcome::InsufficientZen) => {
            panic!("the funded, in-range scroll buy lands: {outcome:?}")
        }
    };
    assert_eq!(world.character(hero).zen(), zen(250));
    let bought = or_abort(
        world
            .inventory(hero)
            .occupant(scroll_cell)
            .ok_or("the bag holds the bought scroll"),
    );
    assert_eq!(
        bought.item.item, TOWN_PORTAL,
        "the shelf sold a real scroll"
    );

    // Walk into the Dungeon (the door's posted 20 is met) and buff up below.
    assert!(matches!(
        world.traverse_gate(hero),
        EnterGateOutcome::Arrived { .. }
    ));
    assert_eq!(world.character(hero).placement().map, MapNumber(1));
    let _buff = world.apply_buff_to(hero, ApplicableBuff::Defense, 30, Tick(10));
    let vitals_before = world.character(hero).vitals();

    // Read the scroll: home in Lorencia (the Dungeon's town), alive, buff and
    // vitals intact, exactly one scroll consumed, the town in the persisted
    // discovered set.
    match world.use_town_portal(hero, scroll_cell) {
        TownPortalOutcome::Arrived { placement } => {
            assert_eq!(placement.map, MapNumber(0), "Lorencia, the Dungeon's town");
            assert_eq!(world.character(hero).placement(), placement);
        }
        outcome @ (TownPortalOutcome::NotAlive | TownPortalOutcome::NoScroll) => {
            panic!("a held scroll teleports: {outcome:?}")
        }
    }
    let home = world.character(hero);
    assert_eq!(home.life(), LifeState::Alive);
    assert_eq!(home.vitals(), vitals_before, "no refill, no penalty");
    assert!(
        home.active_effects().defense().is_some(),
        "the buff rode home"
    );
    assert!(home.discovered().contains(MapNumber(1)), "the field kept");
    assert!(home.discovered().contains(MapNumber(0)), "the town held");
    assert!(
        world.inventory(hero).occupant(scroll_cell).is_none(),
        "the single-use scroll left the bag — no zero-count ghost"
    );
}

#[test]
fn scroll_rejections_consume_nothing() {
    let mut world = World::new(35, MapNumber(0));
    let hero = world.seat_character(dark_knight(60, 150, tile(10, 10)));

    // An empty cell is refused NoScroll.
    assert_eq!(
        world.use_town_portal(hero, cell(0, 0)),
        TownPortalOutcome::NoScroll
    );

    // Hold a scroll, then die: the dead hero's scroll does nothing and the
    // persisted stack is whole.
    let scroll = item_instance(world.atlas(), TOWN_PORTAL);
    let footprint = footprint_of(world.atlas(), TOWN_PORTAL);
    let anchor = or_abort(
        world
            .inventory(hero)
            .first_fit(footprint)
            .ok_or("the empty bag fits a scroll"),
    );
    assert!(matches!(
        world.place_in_bag(hero, scroll, footprint, anchor),
        PlaceOutcome::Placed { .. }
    ));
    let _events = world.resolve_player_death(hero, Tick(100));
    assert!(matches!(
        world.character(hero).life(),
        LifeState::Dead { .. }
    ));
    assert_eq!(
        world.use_town_portal(hero, anchor),
        TownPortalOutcome::NotAlive
    );
    assert!(
        world.inventory(hero).occupant(anchor).is_some(),
        "the scroll is whole"
    );
}

#[test]
fn the_menu_and_the_command_agree_for_the_persisted_character() {
    let mut world = World::new(36, MapNumber(0));
    // A novice with a thin wallet: every one of the 16 entries is locked for
    // at least one reason, so sweeping the whole menu through the command
    // leaves the world unmoved while proving the two readers share one rule.
    let hero = world.seat_character(dark_knight(15, 150, tile(10, 10)));
    world.set_wallet(hero, zen(100));

    let menu = world.warp_menu(hero);
    assert_eq!(menu.len(), 16, "one status per surviving entry");
    for status in menu {
        let reasons = match status.availability {
            WarpAvailability::Locked { reasons } => reasons,
            WarpAvailability::Available => panic!("a broke novice opens no entry"),
        };
        let outcome = world.warp(hero, status.index);
        let member = match outcome {
            WarpTravelOutcome::NotDiscovered => {
                reasons.iter().any(|r| *r == WarpLockReason::NotDiscovered)
            }
            WarpTravelOutcome::LevelTooLow { required } => reasons
                .iter()
                .any(|r| *r == WarpLockReason::LevelTooLow { required }),
            WarpTravelOutcome::CannotFly => reasons.iter().any(|r| *r == WarpLockReason::CannotFly),
            WarpTravelOutcome::NotEnoughZen { required, .. } => reasons
                .iter()
                .any(|r| *r == WarpLockReason::InsufficientZen { cost: required }),
            WarpTravelOutcome::Arrived { .. }
            | WarpTravelOutcome::NotAlive
            | WarpTravelOutcome::NoWalkableLanding => false,
        };
        assert!(
            member,
            "entry {:?}: the command's {outcome:?} is in the projected lock set",
            status.index
        );
    }
    assert_eq!(
        world.character(hero).zen(),
        zen(100),
        "no locked entry charged the persisted wallet"
    );
}

// --- Icarus / wings: the fly-less bounce → equip → enter → warp-back leg. ----

/// The Icarus warp entry (s6 backport, index 23): level 170, 10,000 zen, the
/// one Sky destination — its entry doors are wings-gated.
const ICARUS_WARP: WarpIndex = WarpIndex(23);

/// Wings of Satan (group 12 number 2, a knight's first wing) — worn into the
/// wings slot, it flips the host-derived `Wings` fact to `Equipped`.
const WINGS_OF_SATAN: ItemRef = ItemRef {
    group: 12,
    number: 2,
};

/// The persisted menu's Icarus annotation for the character at `hero`.
fn icarus_status(world: &World, hero: usize) -> WarpAvailability {
    or_abort(
        world
            .warp_menu(hero)
            .into_iter()
            .find(|status| status.index == ICARUS_WARP)
            .ok_or("the menu lists the Icarus entry"),
    )
    .availability
}

#[test]
fn a_wingless_hero_is_bounced_at_both_icarus_doors_until_a_wing_is_worn() {
    let mut world = World::new(37, MapNumber(4));

    // A qualified-but-wingless hero standing on the Lost Tower → Icarus
    // door's trigger (gate 62 at (17,250), min level 160).
    let character = dark_knight_in_band(world.atlas(), 200, 50_000, MapNumber(4), tile(17, 250));
    let hero = world.seat_character(character);

    // BOUNCE 1 — the walk-in door: CannotFly, the persisted placement is
    // unmoved and the persisted discovered set still lacks Icarus.
    assert_eq!(world.traverse_gate(hero), EnterGateOutcome::CannotFly);
    assert_eq!(world.character(hero).placement().map, MapNumber(4));
    assert!(!world.character(hero).discovered().contains(MapNumber(10)));

    // BOUNCE 2 — the menu warp, driven for a host-loaded past visitor whose
    // discovered set already carries Icarus (and Tarkan): with discovery and
    // level met, the wingless refusal is CannotFly and the persisted wallet
    // and placement are untouched.
    let mut wire = or_abort(serde_json::to_value(dark_knight_in_band(
        world.atlas(),
        200,
        50_000,
        MapNumber(4),
        tile(17, 250),
    )));
    let object = or_abort(wire.as_object_mut().ok_or("character is an object"));
    object.insert("discovered".to_owned(), serde_json::json!([4, 8, 10]));
    let veteran = world.seat_character(or_abort(serde_json::from_value(wire)));
    assert_eq!(
        world.warp(veteran, ICARUS_WARP),
        WarpTravelOutcome::CannotFly
    );
    assert_eq!(world.character(veteran).zen(), zen(50_000));
    assert_eq!(world.character(veteran).placement().map, MapNumber(4));

    // Before the wing, the hero's Icarus entry is locked with CannotFly among
    // the reasons (alongside the undiscovered lock).
    match icarus_status(&world, hero) {
        WarpAvailability::Locked { reasons } => {
            assert!(
                reasons.iter().any(|r| *r == WarpLockReason::CannotFly),
                "the wingless lock set names the wings gate: {reasons:?}"
            );
        }
        WarpAvailability::Available => panic!("a wingless hero cannot open the Sky entry"),
    }

    // EQUIP — a wing worn into the wings slot flips the host-derived fact.
    let wing = item_instance(world.atlas(), WINGS_OF_SATAN);
    assert!(matches!(
        world.equip_into(hero, wing, EquipmentSlot::Wings),
        EquipOutcome::Equipped { .. }
    ));

    // ENTER — the same door now admits: the persisted placement is Flying on
    // Icarus and the persisted discovered set gains it.
    match world.traverse_gate(hero) {
        EnterGateOutcome::Arrived { placement } => {
            assert_eq!(placement.map, MapNumber(10));
            assert_eq!(placement.movement, Movement::Flying, "Sky forces flight");
            assert_eq!(world.character(hero).placement(), placement);
        }
        outcome @ (EnterGateOutcome::NotAlive
        | EnterGateOutcome::LevelTooLow { .. }
        | EnterGateOutcome::CannotFly
        | EnterGateOutcome::NoWalkableLanding) => {
            panic!("the winged hero steps through: {outcome:?}")
        }
    }
    assert!(world.character(hero).discovered().contains(MapNumber(10)));

    // The menu flips: the Icarus entry is Available where CannotFly locked it.
    assert!(matches!(
        icarus_status(&world, hero),
        WarpAvailability::Available
    ));

    // RETURN — the sixteen-entry list and the shared rule survive the persist
    // seam: the hero menu-warps back to a Ground map, the fee off the wallet.
    assert_eq!(world.warp_menu(hero).len(), 16);
    match world.warp(hero, LOST_TOWER_WARP) {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_eq!(placement.map, MapNumber(4));
            // 50,000 seeded minus the one 5,000 fee — the earlier CannotFly
            // bounce charged nothing.
            assert_eq!(balance, zen(45_000), "the 5,000 fee off the wallet");
            assert_eq!(world.character(hero).zen(), balance);
            let grid = or_abort(
                world
                    .atlas()
                    .walk_grid(placement.map)
                    .ok_or("Lost Tower has a walk grid"),
            );
            assert!(grid.walkable(placement.position), "a walkable landing");
        }
        outcome @ (WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding) => panic!("the winged return lands: {outcome:?}"),
    }
    // Characters carrying the backported maps in their discovered sets — the
    // traveler ({4,10}) and the loaded veteran ({4,8,10}) — survive the serde
    // round-trip byte-for-byte and re-drive cleanly.
    for index in [hero, veteran] {
        let stored = world.character(index);
        assert_eq!(
            serde_json::to_string(stored).unwrap(),
            serde_json::to_string(&persist(stored.clone())).unwrap(),
        );
        let _menu = world.warp_menu(index);
    }
}

// --- The canonical golden path: one identity + one wallet, front to back. ----

/// Farms the first kill of the golden path (BEATS 1-5): spawns a fightable mob at
/// `mob_tile`, advances its AI to a real intent, walks the `hero` to it along the
/// real terrain, strikes it dead, and resolves the reward. Returns the money the
/// kill dropped (gated at the 20-zen potion cost) and the victim's position, or
/// `None` when this seed's money drop did not clear the gate. Every combat beat
/// is a hard invariant.
fn farm_first_kill(world: &mut World, hero: usize, mob_tile: TileCoord) -> Option<(u64, WorldPos)> {
    // BEAT 1 — Spawn a fightable mob. A combat monster is seated; its number is stable.
    let (number, _combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let placement = SpawnPlacement::Fixed {
        position: mob_tile,
        facing: TileFacing::East,
    };
    let spawn = world.spawn_from(number, placement);
    let instance = match spawn.spawned.first()? {
        Spawned::Mob { instance } => *instance,
        Spawned::Placed { .. } => return None,
    };
    assert_eq!(
        instance.number, number,
        "the spawned mob is the requested combat monster"
    );
    let mob = world.seat_monster(instance);

    // BEAT 2 — Advance its AI. Aimed at its own tile it decides a real Attack intent.
    let intent = world.advance_monster(mob, Some(instance.placement.position), Tick(1));
    assert!(
        matches!(intent, MonsterIntent::Attack { .. }),
        "the AI returns a real intent, not inert"
    );

    // BEAT 3 — Walk the hero to the mob. Position strictly advances; never a false Block.
    let start_x = world.character(hero).placement().position.x().raw();
    let touch = Radius::from_tiles(1);
    while !world
        .character(hero)
        .placement()
        .position
        .within_range(mob_tile.to_world(), touch)
    {
        assert!(
            matches!(
                world.step(hero, mob_tile.to_world()),
                StepOutcome::Resolved { .. }
            ),
            "the walkable corridor must never block a step"
        );
    }
    assert!(
        world.character(hero).placement().position.x().raw() > start_x,
        "the hero advanced toward the mob"
    );

    // BEAT 4 — Strike the mob to death. HP falls monotonically; the last blow is Killed.
    let mut previous = world.monster(mob).health.current();
    let mut killed = false;
    for _ in 0..10_000u32 {
        let outcome = world.strike(hero, mob);
        let current = world.monster(mob).health.current();
        assert!(
            current <= previous,
            "the mob's health never rises under attack"
        );
        previous = current;
        if matches!(outcome, AttackOutcome::Killed { .. }) {
            killed = true;
            break;
        }
    }
    assert!(killed, "the mob is beaten to a killing blow");
    assert_eq!(world.monster(mob).health.current(), 0);

    // BEAT 5 — Resolve the kill. Experience is granted and drops are rolled.
    let resolution = world.resolve_kill_of(hero, mob);
    assert!(
        resolution.experience.gained.0 > 0,
        "the kill grants experience"
    );
    // The kill's money funds the potion — required this seed (>= the 20-zen cost).
    let pile = zen_drop(&resolution).filter(|amount| *amount >= 20)?;
    Some((pile, world.monster(mob).placement.position))
}

/// Picks up the kill's money and item drops and gears up (BEATS 6-8): credits the
/// wallet by exactly the pile, lands the item drop in the bag byte-identical to
/// the roll, and wears it. `None` on the empty-bag first-fit gate (never reached).
/// Every value beat is a hard invariant.
fn loot_and_gear(world: &mut World, hero: usize, pile: u64, mob_pos: WorldPos) -> Option<()> {
    // BEAT 6 — Pick up the money pile. The wallet is credited by EXACTLY the pile.
    assert_eq!(world.character(hero).zen(), zen(0));
    let ground_zen = world.seat_ground_zen(Zen(pile), mob_pos, Tick(6_000));
    assert_eq!(
        world.pickup_zen(hero, ground_zen),
        ZenPickupOutcome::PickedUp
    );
    assert_eq!(
        world.character(hero).zen(),
        zen(pile),
        "the wallet holds exactly the picked-up pile"
    );

    // BEAT 7 — Pick up the kill's item drop (materialised the way the harness lays
    // any kill drop — drop_item_to_ground rolls the full instance). It leaves the
    // ground and lands in the bag once, byte-identical to the rolled instance.
    let (ground_item, rolled) = world.drop_item_to_ground(
        DRAGON_ARMOR,
        ItemLevel::ZERO,
        ItemRarity::Normal,
        mob_pos,
        Tick(6_000),
    );
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world.inventory(hero).first_fit(footprint)?;
    assert_eq!(world.ground_item_count(), 1);
    assert_eq!(
        world.pickup(hero, ground_item, anchor),
        PickupOutcome::PickedUp { at: anchor }
    );
    assert_eq!(world.ground_item_count(), 0);
    let bagged = or_abort(
        world
            .inventory(hero)
            .occupant(anchor)
            .ok_or("the bag holds the picked item"),
    )
    .item
    .clone();
    assert_eq!(
        wire(&bagged),
        wire(&rolled),
        "the bagged item is byte-identical to the rolled drop"
    );

    // BEAT 8 — Equip it. The worn item is byte-identical to the rolled instance.
    let in_hand = match world.remove_from_bag(hero, anchor) {
        RemoveOutcome::Removed { item, .. } => item,
        RemoveOutcome::Rejected { .. } => {
            or_abort(Err::<_, &str>("the anchor held the picked item"))
        }
    };
    let slot = match world.equip_first_available(hero, in_hand) {
        EquipOutcome::Equipped { slot } => slot,
        EquipOutcome::Rejected { .. } => {
            or_abort(Err::<_, &str>("the dropped armor is equippable"))
        }
    };
    let worn = or_abort(world.equipment(hero).get(slot).ok_or("the slot is filled")).clone();
    assert_eq!(
        wire(&worn),
        wire(&rolled),
        "the worn item is byte-identical to the rolled drop"
    );
    Some(())
}

/// Walks to the merchant, buys, sells, and crafts (BEATS 9-11): the potion cost is
/// debited from the threaded wallet, the cape's proceeds credited, and the chaos
/// mix's fee charged against the same wallet. Returns the created item and its bag
/// anchor for the trade beat; `None` when the mix's success roll did not fire this
/// seed. Every non-probabilistic beat is a hard invariant.
fn shop_and_craft(
    world: &mut World,
    hero: usize,
    merchant_tile: TileCoord,
) -> Option<(ItemInstance, Cell)> {
    let merchant = merchant_tile.to_world();

    // BEAT 9 — Walk to the merchant and buy a potion. Balance = wallet - cost.
    assert!(
        matches!(world.step(hero, merchant), StepOutcome::Resolved { .. }),
        "the corridor must never block the walk to the merchant"
    );
    let before_buy = world.character(hero).zen();
    let slot0 = or_abort(ShelfSlot::new(0));
    let after_buy = match world.buy(hero, ELF_LALA, slot0, merchant) {
        BuyOutcome::NewItem { balance, .. } | BuyOutcome::Merged { balance, .. } => balance,
        BuyOutcome::OutOfRange
        | BuyOutcome::UnknownShelfSlot
        | BuyOutcome::InventoryFull
        | BuyOutcome::InsufficientZen => or_abort(Err::<_, &str>(
            "in reach with the earned zen, the potion buy lands",
        )),
    };
    assert_eq!(
        after_buy,
        zen(before_buy.get() - 20),
        "the potion cost is debited from the threaded wallet"
    );
    assert_eq!(world.character(hero).zen(), after_buy);

    // BEAT 10 — Sell a Cape of Lord (funds the crafting fee). Proceeds credited; slot freed.
    let cape = item_instance(world.atlas(), CAPE_OF_LORD);
    let cape_footprint = footprint_of(world.atlas(), CAPE_OF_LORD);
    let cape_anchor = world.inventory(hero).first_fit(cape_footprint)?;
    assert!(matches!(
        world.place_in_bag(hero, cape, cape_footprint, cape_anchor),
        PlaceOutcome::Placed { .. }
    ));
    let before_sale = world.character(hero).zen();
    let after_sale = match world.sell(hero, cape_anchor, merchant) {
        SellOutcome::Sold { proceeds, balance } => {
            assert!(proceeds.0 > 0, "the cape sale credits real proceeds");
            assert_eq!(
                balance,
                zen(before_sale.get() + proceeds.0),
                "proceeds add to the threaded wallet"
            );
            balance
        }
        SellOutcome::OutOfRange | SellOutcome::NoItemAtCell | SellOutcome::WalletFull => {
            or_abort(Err::<_, &str>("a merchant in reach buys the cape"))
        }
    };
    assert_eq!(world.character(hero).zen(), after_sale);
    assert!(
        world.inventory(hero).occupant(cape_anchor).is_none(),
        "the sold item's slot is freed"
    );

    // BEAT 11 — Craft at the chaos machine. A result is created and lands in the
    // bag; the fee is charged against the threaded wallet. A failed roll just
    // tries the next seed (the created-item assertion needs a success).
    let sword = {
        let mut sword = item_at_level(world.atlas(), SWORD, 6);
        sword.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L1,
        });
        sword
    };
    let placed = vec![sword, item_instance(world.atlas(), JEWEL_OF_CHAOS)];
    let before_mix = world.character(hero).zen();
    let (created, after_mix) = match world.mix(hero, placed) {
        MixOutcome::Success {
            created,
            fee,
            zen: balance,
            ..
        } => {
            assert_eq!(
                balance,
                zen(before_mix.get() - fee.0),
                "the mix fee is charged off the real wallet"
            );
            (created, balance)
        }
        MixOutcome::Failed { .. } | MixOutcome::Rejected { .. } => return None,
    };
    assert_eq!(world.character(hero).zen(), after_mix);
    let created_footprint = footprint_of(world.atlas(), created.item);
    let created_anchor = world.inventory(hero).first_fit(created_footprint)?;
    assert!(matches!(
        world.place_in_bag(hero, created.clone(), created_footprint, created_anchor),
        PlaceOutcome::Placed { .. }
    ));
    let created_bagged = or_abort(
        world
            .inventory(hero)
            .occupant(created_anchor)
            .ok_or("the bag holds the created item"),
    )
    .item
    .clone();
    assert_eq!(
        wire(&created_bagged),
        wire(&created),
        "the created item is in the bag byte-for-byte"
    );
    Some((created, created_anchor))
}

/// Trades the crafted item to a second actor (BEAT 12): the item crosses to the
/// partner's bag byte-for-byte and the zen ledger balances. Every beat is a hard
/// invariant.
fn trade_created_item(
    world: &mut World,
    hero: usize,
    merchant_tile: TileCoord,
    created: &ItemInstance,
    created_anchor: Cell,
) {
    let partner = world.seat_character(dark_knight(80, 300, merchant_tile));
    let session = world.open_and_accept_trade(hero, partner);
    assert_eq!(
        world.offer_item_to_trade(session, hero, Side::Requester, created_anchor, cell(0, 0)),
        OfferOutcome::Offered { at: cell(0, 0) }
    );
    let hero_before_trade = world.character(hero).zen();
    let offered = 1_000u64;
    assert!(matches!(
        world.offer_zen_to_trade(session, hero, Side::Requester, Zen(offered)),
        ZenOfferOutcome::Offered { .. }
    ));
    assert!(matches!(
        world.lock_trade(session, hero, partner, Side::Partner),
        LockResult::Locked { .. }
    ));
    assert_eq!(
        world.lock_trade(session, hero, partner, Side::Requester),
        LockResult::Completed
    );

    let landed = world.inventory(partner).placed();
    assert_eq!(landed.len(), 1);
    let crossed = or_abort(
        landed
            .first()
            .ok_or("the partner's bag holds the traded item"),
    );
    assert_eq!(
        wire(&crossed.item),
        wire(created),
        "the created item crossed to the partner byte-for-byte"
    );
    assert_eq!(
        world.character(partner).zen(),
        zen(offered),
        "the partner gained exactly the offered zen"
    );
    assert_eq!(
        world.character(hero).zen(),
        zen(hero_before_trade.get() - offered),
        "the hero kept the remainder"
    );
}

/// Plays one full session front-to-back on construction `seed`, threading one hero
/// identity and one wallet through every beat. `None` when this seed's
/// probabilistic beats (the kill's money drop, the chaos mix's success roll) do
/// not cooperate — the caller sweeps the seed OUTSIDE the run so the single stream
/// stays deterministic.
fn play_full_session(seed: u64) -> Option<()> {
    let mut world = World::new(seed, MapNumber(0));

    // A straight walkable corridor on real Lorencia terrain — the hero walks it to
    // the mob and on to the merchant.
    let run = walkable_run(world.atlas(), MapNumber(0), 8);
    let start = *run.first()?;
    let mob_tile = *run.get(5)?;
    let merchant_tile = *run.get(6)?;

    // ONE hero identity and ONE wallet (starting empty), threaded through every beat.
    let hero = world.seat_character(dark_knight(80, 300, start));

    // BEATS 1-5 — Spawn -> AI -> walk -> strike -> kill, earning the pile.
    let (pile, mob_pos) = farm_first_kill(&mut world, hero, mob_tile)?;
    // BEATS 6-8 — Pick up the money and item, and gear up.
    loot_and_gear(&mut world, hero, pile, mob_pos)?;
    // BEATS 9-11 — Walk to the merchant, buy, sell, and craft.
    let (created, created_anchor) = shop_and_craft(&mut world, hero, merchant_tile)?;
    // BEAT 12 — Trade the crafted item to a second actor.
    trade_created_item(&mut world, hero, merchant_tile, &created, created_anchor);
    Some(())
}

#[test]
fn a_full_session_plays_start_to_finish_with_sensible_state_at_every_beat() {
    // The single "the game works end-to-end" artifact: one hero identity and one
    // wallet threaded through every beat — spawn, AI, walk, kill, loot, gear,
    // shop, craft, trade — each asserting a game-sensible value (not wire
    // equality). The run plays on the first construction seed whose probabilistic
    // beats (the kill's money drop and the chaos mix's success roll) cooperate;
    // every other beat is a hard invariant. Sweeping the seed OUTSIDE the run
    // keeps the single stream deterministic — the harness idiom.
    //
    // Grouping does not compose onto this single-identity spine (a party needs two
    // live heroes and a shared kill); the full party flow — form, share one kill's
    // exp, disband — is proven front-to-back in
    // `a_full_party_flow_forms_shares_a_kill_and_disbands` in the Party section below.
    // Travel likewise does not compose here (warping the hero off Lorencia would
    // strand it away from the mob corridor and the merchant that the later beats
    // thread through); the full travel flow — discovery lock, the walk-in unlock,
    // the fee-debited menu warp, and the scroll home — is proven front-to-back in
    // `discovery_locks_the_menu_until_a_walk_in_and_the_menu_warp_returns` and
    // `a_bought_town_portal_scroll_carries_the_hero_home_with_everything_kept`
    // in the Warp/travel section above.
    let played = (0u64..1024).any(|seed| play_full_session(seed).is_some());
    assert!(
        played,
        "a construction seed in 0..1024 plays the whole session start to finish"
    );
}

// --- Party: form, roster, shares, disband (seam D; W-PARTY). -----------------
//
// A party's `MemberSlot(i)` binds to the character seated at index `i` (the paper
// host's account↔slot map). Each scenario reads live values, drives a pure party
// service through the host, and asserts the persisted world — the standing sixth
// gate for grouping. `distribute_kill_experience` is the only party beat that
// draws RNG; the roster, invite, and zen beats are pure value functions.

/// An `Active` member seat at `slot` — the roster ingredient scenarios pre-seat.
fn active_member(slot: u8) -> PartyMember {
    PartyMember {
        slot: MemberSlot(slot),
        membership: Membership::Active,
    }
}

/// A trio led by slot 0, all `Active` (slots 0, 1, 2).
fn seated_trio() -> PartySession {
    PartySession::forming().with_member(active_member(2))
}

/// The host-resolved availability of the character at `char_index` as an invite
/// target — read from its own live placement.
fn available_target(world: &World, char_index: usize) -> party::PartyAvailability {
    let placement = world.character(char_index).placement();
    party::PartyAvailability::Available {
        position: placement.position,
        map: placement.map,
    }
}

#[test]
fn a_solo_hero_invites_a_second_who_accepts_and_the_persisted_party_seats_both() {
    let mut world = World::new(1, MapNumber(0));
    let a = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let b = world.seat_character(dark_knight(30, 150, tile(11, 10)));

    let target = available_target(&world, b);
    assert!(matches!(
        world.invite(a, target, Tick(0)),
        party::InviteOutcome::Sent { .. }
    ));
    assert_eq!(world.pending_invite_count(), 1);

    assert!(matches!(
        world.accept_invite(0, a, b),
        party::AcceptOutcome::Joined { .. }
    ));
    assert_eq!(
        world.pending_invite_count(),
        0,
        "the accepted invite is reaped"
    );
    assert_eq!(world.party_count(), 1);
    let party = world.party(0);
    assert_eq!(party.len(), 2);
    assert_eq!(party.leadership(), Leadership::Led { by: MemberSlot(0) });
    assert!(party.is_active(MemberSlot(0)) && party.is_active(MemberSlot(1)));
}

#[test]
fn a_declined_invite_notifies_both_and_leaves_no_party() {
    let mut world = World::new(2, MapNumber(0));
    let a = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let b = world.seat_character(dark_knight(30, 150, tile(11, 10)));

    let target = available_target(&world, b);
    world.invite(a, target, Tick(0));
    assert_eq!(world.pending_invite_count(), 1);

    assert_eq!(world.decline_invite(0), vec![PartyEvent::InviteDeclined]);
    assert_eq!(world.pending_invite_count(), 0);
    assert_eq!(world.party_count(), 0);
}

#[test]
fn a_pending_invite_lapses_at_its_ttl_and_is_reaped() {
    let mut world = World::new(3, MapNumber(0));
    let a = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let b = world.seat_character(dark_knight(30, 150, tile(11, 10)));

    let target = available_target(&world, b);
    world.invite(a, target, Tick(1000));
    let expires = world.pending_invite(0).expires;

    // Before expiry: still pending, still stored.
    assert!(matches!(
        world.advance_invite(0, Tick(0)),
        party::InviteSweep::Pending { .. }
    ));
    assert_eq!(world.pending_invite_count(), 1);

    // At its lease end: lapsed and reaped.
    assert_eq!(world.advance_invite(0, expires), party::InviteSweep::Lapsed);
    assert_eq!(world.pending_invite_count(), 0);
}

#[test]
fn a_leader_leaving_a_trio_transfers_leadership_in_the_persisted_party() {
    let mut world = World::new(4, MapNumber(0));
    for _ in 0..3 {
        world.seat_character(dark_knight(30, 150, tile(10, 10)));
    }
    let party = world.seat_party(seated_trio());

    assert!(matches!(
        world.leave(party, MemberSlot(0)),
        party::LeaveOutcome::Left { .. }
    ));
    let stored = world.party(party);
    assert_eq!(stored.leadership(), Leadership::Led { by: MemberSlot(1) });
    assert!(stored.member(MemberSlot(0)).is_none());
    assert_eq!(stored.len(), 2);
}

#[test]
fn a_leader_disconnect_moves_leadership_and_the_reconnecting_ex_leader_is_regular() {
    let mut world = World::new(5, MapNumber(0));
    for _ in 0..3 {
        world.seat_character(dark_knight(30, 150, tile(10, 10)));
    }
    let party = world.seat_party(seated_trio());

    assert!(matches!(
        world.disconnect(party, MemberSlot(0), Tick(0)),
        party::DisconnectOutcome::Disconnected { .. }
    ));
    assert_eq!(
        world.party(party).leadership(),
        Leadership::Led { by: MemberSlot(1) },
        "the offline leader never freezes the party"
    );

    assert!(matches!(
        world.reconnect(party, MemberSlot(0)),
        party::ReconnectOutcome::Reconnected { .. }
    ));
    let stored = world.party(party);
    assert!(stored.is_active(MemberSlot(0)));
    assert_eq!(
        stored.leadership(),
        Leadership::Led { by: MemberSlot(1) },
        "a reconnecting ex-leader does not reclaim (sticky)"
    );
}

#[test]
fn a_leader_kicks_a_member_and_named_refusals_never_panic() {
    let mut world = World::new(6, MapNumber(0));
    for _ in 0..3 {
        world.seat_character(dark_knight(30, 150, tile(10, 10)));
    }
    let party = world.seat_party(seated_trio());

    // A non-leader's kick is a named refusal, roster untouched.
    assert_eq!(
        world.kick(party, MemberSlot(1), MemberSlot(2)),
        party::KickOutcome::NotLeader
    );
    assert_eq!(world.party(party).len(), 3);
    // A kick at an unoccupied slot is a named refusal, never a panic.
    assert_eq!(
        world.kick(party, MemberSlot(0), MemberSlot(4)),
        party::KickOutcome::NoSuchMember
    );
    // The real kick shrinks the roster.
    assert!(matches!(
        world.kick(party, MemberSlot(0), MemberSlot(2)),
        party::KickOutcome::Kicked { .. }
    ));
    assert_eq!(world.party(party).len(), 2);
    assert!(world.party(party).member(MemberSlot(2)).is_none());
}

#[test]
fn a_leave_from_a_two_member_party_disbands_and_deletes_the_persisted_session() {
    let mut world = World::new(7, MapNumber(0));
    world.seat_character(dark_knight(30, 150, tile(10, 10)));
    world.seat_character(dark_knight(30, 150, tile(11, 10)));
    let party = world.seat_party(PartySession::forming());
    assert_eq!(world.party_count(), 1);

    assert_eq!(
        world.leave(party, MemberSlot(1)),
        party::LeaveOutcome::Disbanded
    );
    assert_eq!(world.party_count(), 0, "a one-member party never exists");
}

#[test]
fn a_disconnected_member_is_reaped_at_hold_expiry_and_the_party_shrinks() {
    let mut world = World::new(8, MapNumber(0));
    for _ in 0..3 {
        world.seat_character(dark_knight(30, 150, tile(10, 10)));
    }
    let party = world.seat_party(seated_trio());

    world.disconnect(party, MemberSlot(2), Tick(0));
    assert_eq!(world.party(party).len(), 3, "a held seat still counts");

    // Before the hold lapses the party continues unchanged.
    assert!(matches!(
        world.advance_party(party, Tick(0)),
        party::PartyOutcome::Continues { .. }
    ));
    assert_eq!(world.party(party).len(), 3);

    // Well past the 5-minute hold, the seat is reaped like a leave.
    assert!(matches!(
        world.advance_party(party, Tick(1_000_000)),
        party::PartyOutcome::Continues { .. }
    ));
    assert_eq!(world.party(party).len(), 2);
    assert!(world.party(party).member(MemberSlot(2)).is_none());
}

#[test]
fn a_party_kill_fans_exp_to_the_qualifiers_and_dead_or_out_of_range_members_get_nothing() {
    let mut world = World::new(11, MapNumber(0));
    let killer = world.seat_character(dark_knight(20, 150, tile(10, 10))); // slot 0
    let helper = world.seat_character(dark_knight(20, 150, tile(11, 10))); // slot 1
    let corpse = world.seat_character(dark_knight(20, 150, tile(12, 10))); // slot 2 (dead)
    let stray = world.seat_character(dark_knight(20, 150, tile(40, 10))); // slot 3 (out of range)
    let party = world.seat_party(
        PartySession::forming()
            .with_member(active_member(2))
            .with_member(active_member(3)),
    );

    let facts = vec![
        world.member_fact(MemberSlot(0), Vitality::Alive),
        world.member_fact(MemberSlot(1), Vitality::Alive),
        world.member_fact(MemberSlot(2), Vitality::Dead),
        world.member_fact(MemberSlot(3), Vitality::Alive),
    ];
    let before_killer = world.character(killer).experience().0;
    let before_corpse = world.character(corpse).experience().0;
    let before_stray = world.character(stray).experience().0;

    let awards =
        world.distribute_kill_experience(party, &facts, MemberSlot(0), or_abort(Level::new(30)));

    let slots: Vec<u8> = awards.iter().map(|(award, _events)| award.slot.0).collect();
    assert_eq!(
        slots,
        vec![0, 1],
        "only the two present, alive, in-range members earn"
    );
    assert!(world.character(killer).experience().0 > before_killer);
    assert!(world.character(helper).experience().0 > 0);
    assert_eq!(
        world.character(corpse).experience().0,
        before_corpse,
        "a dead member earns nothing"
    );
    assert_eq!(
        world.character(stray).experience().0,
        before_stray,
        "an out-of-range member earns nothing"
    );
    // Each grown character survives the persist round-trip.
    let grown = world.character(killer).clone();
    assert_eq!(wire(&grown), wire(&persist(grown.clone())));
}

#[test]
fn a_party_kill_surfaces_growth_events_only_for_the_member_that_crosses_a_level() {
    let mut world = World::new(23, MapNumber(0));
    // Slot 0 is a level-1 killer: any positive share carries it into level 2.
    // Slot 1 is a level-5 member: its larger share stays well short of the
    // level-6 threshold, so it crosses nothing — across the whole jitter band.
    world.seat_character(dark_knight(1, 150, tile(10, 10))); // slot 0
    world.seat_character(dark_knight(5, 150, tile(11, 10))); // slot 1
    let party = world.seat_party(PartySession::forming());

    let facts = vec![
        world.member_fact(MemberSlot(0), Vitality::Alive),
        world.member_fact(MemberSlot(1), Vitality::Alive),
    ];

    let awards =
        world.distribute_kill_experience(party, &facts, MemberSlot(0), or_abort(Level::new(30)));

    // The crossing killer surfaces LevelsGained with a positive point grant.
    let killer_award = or_abort(
        awards
            .iter()
            .find(|entry| entry.0.slot == MemberSlot(0))
            .ok_or("slot 0 award"),
    );
    match killer_award.1.first() {
        Some(GrowthEvent::LevelsGained {
            reached,
            points_granted,
        }) => {
            assert!(reached.get() > 1, "the level-1 killer climbed a level");
            assert!(*points_granted > 0, "a crossing banks unspent points");
        }
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("the crossing member must surface LevelsGained first")
        }
    }

    // The non-crossing member surfaces no growth event at all.
    let member_award = or_abort(
        awards
            .iter()
            .find(|entry| entry.0.slot == MemberSlot(1))
            .ok_or("slot 1 award"),
    );
    assert_eq!(
        member_award.1,
        vec![],
        "a member that crosses no level surfaces no growth event"
    );
}

#[test]
fn a_party_zen_pile_splits_with_remainder_to_picker_and_an_at_cap_share_grounds() {
    let mut world = World::new(9, MapNumber(0));
    let picker = world.seat_character(dark_knight(30, 150, tile(10, 10))); // slot 0
    let rich = world.seat_character(dark_knight(30, 150, tile(11, 10))); // slot 1 (at cap)
    let third = world.seat_character(dark_knight(30, 150, tile(12, 10))); // slot 2
    world.set_wallet(rich, zen(1_999_999_999));
    let party = world.seat_party(seated_trio());

    let facts = vec![
        world.member_fact(MemberSlot(0), Vitality::Alive),
        world.member_fact(MemberSlot(1), Vitality::Alive),
        world.member_fact(MemberSlot(2), Vitality::Alive),
    ];
    let wallets = vec![
        world.slot_wallet(MemberSlot(0)),
        world.slot_wallet(MemberSlot(1)),
        world.slot_wallet(MemberSlot(2)),
    ];
    let pile = WorldZen {
        amount: Zen(100_000),
        position: world.character(picker).placement().position,
        map: MapNumber(0),
        despawn: Tick(9999),
    };

    let result = world.split_zen_pickup(party, &pile, &facts, MemberSlot(0), &wallets);

    // Picker keeps the odd coin; the at-cap member's share grounds, never lost.
    assert_eq!(world.character(picker).zen(), zen(33_334));
    assert_eq!(world.character(third).zen(), zen(33_333));
    assert_eq!(
        world.character(rich).zen(),
        zen(1_999_999_999),
        "the at-cap wallet is not clamped"
    );
    assert_eq!(result.to_ground.len(), 1);
    assert_eq!(world.ground_zen(0).amount, Zen(33_333));

    // The solo pickup_zen path is unchanged, exercised alongside the party split.
    let solo_pile = world.seat_ground_zen(
        Zen(1000),
        world.character(third).placement().position,
        Tick(9999),
    );
    assert_eq!(
        world.pickup_zen(third, solo_pile),
        ZenPickupOutcome::PickedUp
    );
    assert_eq!(world.character(third).zen(), zen(34_333));
}

#[test]
fn a_full_party_flow_forms_shares_a_kill_and_disbands() {
    let mut world = World::new(3, MapNumber(0));
    let a = world.seat_character(dark_knight(20, 150, tile(10, 10)));
    let b = world.seat_character(dark_knight(20, 150, tile(11, 10)));

    // Form the party.
    let target = available_target(&world, b);
    assert!(matches!(
        world.invite(a, target, Tick(0)),
        party::InviteOutcome::Sent { .. }
    ));
    assert!(matches!(
        world.accept_invite(0, a, b),
        party::AcceptOutcome::Joined { .. }
    ));
    assert_eq!(world.party_count(), 1);

    // Share one kill's experience across both members.
    let facts = vec![
        world.member_fact(MemberSlot(0), Vitality::Alive),
        world.member_fact(MemberSlot(1), Vitality::Alive),
    ];
    let before_a = world.character(a).experience().0;
    let before_b = world.character(b).experience().0;
    let awards =
        world.distribute_kill_experience(0, &facts, MemberSlot(0), or_abort(Level::new(30)));
    assert_eq!(awards.len(), 2);
    assert!(world.character(a).experience().0 > before_a);
    assert!(world.character(b).experience().0 > before_b);

    // A leave dropping below two disbands the whole party.
    assert_eq!(
        world.leave(0, MemberSlot(1)),
        party::LeaveOutcome::Disbanded
    );
    assert_eq!(world.party_count(), 0);
}
