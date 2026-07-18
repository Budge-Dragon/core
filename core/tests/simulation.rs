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
use mu_core::components::class::CharacterClass;
use mu_core::components::combat_profile::TargetKind;
use mu_core::components::drop_claim::{DropClaim, PickerStanding};
use mu_core::components::equipment::EquipmentSlot;
use mu_core::components::inventory::{Cell, Footprint};
use mu_core::components::item_instance::{
    Durability, ItemInstance, RarityRoll, RolledNormalOption,
};
use mu_core::components::item_options::NormalOption;
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::item_ref::ItemRef;
use mu_core::components::levels::OptionLevel;
use mu_core::components::life::LifeState;
use mu_core::components::movement::{FlightChange, Movement};
use mu_core::components::party::{Leadership, MemberSlot, Membership, Vitality};
use mu_core::components::pool::Pool;
use mu_core::components::reputation::{PkStage, PlayerKillCount, Standing};
use mu_core::components::spatial::{Radius, WorldPos};
use mu_core::components::tile::{TerrainGrid, TileCoord, TileFacing};
use mu_core::components::trade_window::Side;
use mu_core::components::units::{CarriedZen, Exp, ItemLevel, Level, MapNumber, Tick, Zen};
use mu_core::components::unlocked_classes::UnlockedClasses;
use mu_core::data::common::{MonsterNumber, SkillNumber};
use mu_core::data::effects::Ailment;
use mu_core::data::gates_warps::WarpIndex;
use mu_core::data::minigame::{
    EventLevel, PlayerCount, RewardKind, RosterSlot, RosterStatus, Score, SuccessFlag, WaveNumber,
    WaveRespawn, WinnerStanding,
};
use mu_core::data::npc_shops::ShelfSlot;
use mu_core::data::spawns::SpawnPlacement;
use mu_core::entities::minigame_session::{MiniGamePhase, WaveState};
use mu_core::entities::party_session::{PartyMember, PartySession};
use mu_core::entities::spawned::Spawned;
use mu_core::entities::trade_session::TradeSession;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::account::{ClassUnlocked, CreationVerdict};
use mu_core::events::combat::AttackOutcome;
use mu_core::events::consume::{ConsumeEvent, PoolKind};
use mu_core::events::craft::MixOutcome;
use mu_core::events::death::{DeathEvent, Respawned};
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::inventory::{EquipOutcome, EquipRejection, PlaceOutcome, RemoveOutcome};
use mu_core::events::kill::KillResolution;
use mu_core::events::loot::Drop;
use mu_core::events::minigame::MiniGameEvent;
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightDenialReason, FlightOutcome, StepOutcome};
use mu_core::events::party::PartyEvent;
use mu_core::events::progression::GrowthEvent;
use mu_core::events::reputation::{PkEvent, SanctionReason};
use mu_core::events::shop::{BuyOutcome, RepairOutcome, SellOutcome};
use mu_core::events::skills::{CastRejection, SkillOutcome, TargetHit};
use mu_core::events::spawn::SpawnEvent;
use mu_core::events::trade::{CancelReason, OfferOutcome, ZenOfferOutcome};
use mu_core::events::travel::{
    EnterGateOutcome, TownPortalOutcome, WarpAvailability, WarpLockReason, WarpTravelOutcome,
};
use mu_core::services::account::{creation_verdict, unlock_classes_for_level};
use mu_core::services::effects::ApplicableBuff;
use mu_core::services::ground::DropOrigin;
use mu_core::services::inventory::{PickupOutcome, ZenPickupOutcome};
use mu_core::services::minigame::{EnterOutcome, GrantDecision};
use mu_core::services::party;
use mu_core::services::price::selling_price;
use mu_core::services::profile::{character_profile, equipped_profile};
use mu_core::services::skills::Designation;
use mu_core::services::trade::LockResult;
use mu_core::services::wear::WearEvent;

use paper_host::{
    Combatant, World, aggressive_monster, armored_monster_from, cell, dark_knight,
    dark_knight_in_band, dark_wizard, devil_square_definition, devil_square_key,
    devil_square_ticket, devil_square_ticket_ref, direct_hit_skill, earthshake_skill,
    fighting_monster_from, first_passive_monster, flame_skill, footprint_of, guard_monster,
    heal_skill, hellfire_skill, is_equippable, item_at_level, item_instance,
    lightning_direct_skill, low_level_monster, lunge_skill, magic_gladiator, monster_instance,
    none_type_skill, nova_skill, or_abort, persist, pos, pressing_monster, respawning_wave_monster,
    reward_drop_group, reward_entry, spawn_wave, tile, walkable_area, walkable_run, wearer_of,
    wire, wizardry_direct_skill, zen,
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

/// A real bow (group 4, number 0) — an elf-only weapon, used to prove the
/// LIVE class gate refuses it on a Dark Knight (the W-EQUIP Case-D flip).
const ELF_BOW: ItemRef = ItemRef {
    group: 4,
    number: 0,
};

/// A real one-handed Blade (group 0, number 5) — damage [36, 47]; at +9 its
/// scaled bars (str 171 / agi 114) sit under the reference knight's 200/120,
/// so it equips through the live gate.
const BLADE: ItemRef = ItemRef {
    group: 0,
    number: 5,
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
        let stamp = world.stamp_item_drop(DropOrigin::Ownerless, Tick(0));
        world.seat_ground_item(dropped, pos(12, 12), stamp)
    };
    let zen_stamp = world.stamp_zen_drop(DropOrigin::Ownerless, Tick(0));
    let ground_zen = world.seat_ground_zen(Zen(1007), pos(12, 12), zen_stamp);
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
            "mini_sessions",
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
        let wearer = wearer_of(world.character(killer));
        let drop = item_drops(&resolution)
            .into_iter()
            .find(|candidate| is_equippable(world.atlas(), candidate.0, &wearer));
        let Some((item, level, rarity)) = drop else {
            continue;
        };

        // V1/V7: assemble the ground item from the victim's position (returned),
        // the world map (context), and the core-stamped lifecycle clocks — a
        // monster kill at tick 0, so the drop appears one second later.
        let position = world.monster(victim).placement.position;
        let stamp = world.stamp_item_drop(DropOrigin::MonsterKill, Tick(0));
        let (ground, rolled) = world.drop_item_to_ground(item, level, rarity, position, stamp);
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

    // Pick it up into the real, half-full bag — the killer is the owner, at
    // the drop's core-stamped appearance tick.
    let appeared = world
        .stamp_item_drop(DropOrigin::MonsterKill, Tick(0))
        .appearance;
    let picked = world.pickup(killer, ground, anchor, PickerStanding::Owner, appeared);
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

    let appeared = world
        .stamp_item_drop(DropOrigin::MonsterKill, Tick(0))
        .appearance;
    let picked = world.pickup(killer, ground, anchor, PickerStanding::Owner, appeared);
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
    let appeared = world
        .stamp_item_drop(DropOrigin::MonsterKill, Tick(0))
        .appearance;
    match world.pickup(killer, ground, cell(0, 0), PickerStanding::Owner, appeared) {
        PickupOutcome::Rejected { item, .. } => {
            // The reassembled world item is byte-identical to the one on the
            // ground — nothing was dropped on the floor of the code.
            assert_eq!(
                serde_json::to_string(&persist(item)).unwrap(),
                serde_json::to_string(&before).unwrap(),
            );
        }
        PickupOutcome::PickedUp { .. }
        | PickupOutcome::OutOfReach { .. }
        | PickupOutcome::Refused { .. } => panic!("a full bag must refuse the pickup"),
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
            .terrain_grid(placement.map)
            .ok_or("the respawn map has a terrain grid"),
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
        let stamp = world.stamp_zen_drop(DropOrigin::MonsterKill, Tick(0));
        let ground = world.seat_ground_zen(Zen(amount), position, stamp);
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
    // context, despawn from the core stamping seam).
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

    let stamp = world.stamp_zen_drop(DropOrigin::Ownerless, Tick(0));
    let ground = world.seat_ground_zen(Zen(2), pos(10, 10), stamp);
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
        ZenPickupOutcome::PickedUp | ZenPickupOutcome::OutOfReach { .. } => {
            panic!("a pile one over the cap is refused whole")
        }
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

/// The Magic Gladiator second wing (12/6) — the one second-wings-mix output a
/// helper-buildable class can wear under the live equip gate.
const MG_SECOND_WING: ItemRef = ItemRef {
    group: 12,
    number: 6,
};

#[test]
fn crafted_wings_equipped_make_the_character_flight_eligible() {
    // The second-wings mix creates one of four class-locked, level-215-gated
    // wings at random, so the flyer is a level-220 Magic Gladiator and the
    // construction seed is swept until the mix succeeds with the MG wing —
    // the live equip gate then accepts it for real.
    for seed in 0u64..64 {
        let mut world = World::new(seed, MapNumber(0));
        let flyer = world.seat_character(magic_gladiator(220, tile(10, 10)));
        world.set_wallet(flyer, zen(10_000_000));

        // A Second Wings mix: a first wing + Loch's Feather + Jewel of Chaos →
        // a second wing (fee 5,000,000).
        let placed = vec![
            item_instance(world.atlas(), FAIRY_WINGS),
            item_instance(world.atlas(), LOCHS_FEATHER),
            item_instance(world.atlas(), JEWEL_OF_CHAOS),
        ];
        let wing = match world.mix(flyer, placed) {
            MixOutcome::Success { created, .. } => created,
            MixOutcome::Rejected { .. } | MixOutcome::Failed { .. } => continue,
        };
        if wing.item != MG_SECOND_WING {
            continue;
        }
        crafted_wing_lifts_the_flyer(world, flyer, wing);
        return;
    }
    panic!("no seed in 0..64 crafts the Magic Gladiator second wing");
}

/// The proven-cooperating seed's assertions: the crafted wing equips, flight
/// lifts off, and a wingless knight stays grounded.
fn crafted_wing_lifts_the_flyer(mut world: World, flyer: usize, wing: ItemInstance) {
    match world.equip_first_available(flyer, wing) {
        EquipOutcome::Equipped { slot } => assert_eq!(slot, EquipmentSlot::Wings),
        EquipOutcome::Rejected { .. } => {
            or_abort(Err::<(), &str>("the MG second wing equips on the flyer"));
        }
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
    let stamp = world.stamp_item_drop(DropOrigin::MonsterKill, Tick(0));
    let (ground, rolled) = world.drop_item_to_ground(
        DRAGON_ARMOR,
        ItemLevel::new(9).expect("level 9 is valid"),
        ItemRarity::Excellent,
        pos(10, 10),
        stamp,
    );
    assert!(
        matches!(rolled.roll, RarityRoll::Excellent { .. }),
        "the drop rolled a real excellent instance"
    );

    // Player A — the killer, so the owner — picks it up at the appearance tick.
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world
        .inventory(alpha)
        .first_fit(footprint)
        .expect("the empty bag has room");
    assert_eq!(
        world.pickup(
            alpha,
            ground,
            anchor,
            PickerStanding::Owner,
            stamp.appearance
        ),
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
    let stamp = world.stamp_item_drop(DropOrigin::Ownerless, Tick(0));
    let ground = world.seat_ground_item(armor.clone(), pos(10, 10), stamp);
    assert_eq!(world.ground_item_count(), 1);

    // A picks it up into its first-fit anchor — an ownerless drop, free to any
    // picker in reach.
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world
        .inventory(alpha)
        .first_fit(footprint)
        .expect("A's empty bag has room");
    assert_eq!(
        world.pickup(alpha, ground, anchor, PickerStanding::Stranger, Tick(0)),
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

    // 2. Advance the mob's AI two ticks (each wander draws a continuous heading,
    // a variable but deterministic-per-seed number of words).
    for now in [Tick(1_000), Tick(2_000)] {
        let intent = world.advance_monster(mob, None, now);
        trace.push(TraceStep {
            label: "ai",
            detail: wire(&intent),
        });
    }

    // 3. Step the killer two tiles along the corridor (no RNG) — into the
    // three-tile pickup reach of the mob's drop position.
    for _ in 0..2 {
        let stepped = world.step(killer, mob_tile.to_world());
        trace.push(TraceStep {
            label: "step",
            detail: wire(&stepped),
        });
    }

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
        let stamp = world.stamp_zen_drop(DropOrigin::MonsterKill, Tick(2_000));
        let ground = world.seat_ground_zen(Zen(amount), position, stamp);
        let picked = world.pickup_zen(killer, ground);
        trace.push(TraceStep {
            label: "pickup_zen",
            detail: wire(&picked),
        });
    }

    // 7. Materialise the first item drop, if any, pick it up, and sell it back.
    if let Some((item, level, rarity)) = item_drops(&resolution).into_iter().next() {
        let position = world.monster(mob).placement.position;
        let stamp = world.stamp_item_drop(DropOrigin::MonsterKill, Tick(2_000));
        let (ground, _rolled) = world.drop_item_to_ground(item, level, rarity, position, stamp);
        let footprint = footprint_of(world.atlas(), item);
        if let Some(anchor) = world.inventory(killer).first_fit(footprint) {
            let picked = world.pickup(
                killer,
                ground,
                anchor,
                PickerStanding::Owner,
                stamp.appearance,
            );
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
        let wearer = wearer_of(world.character(caster));
        let drop = item_drops(&resolution)
            .into_iter()
            .find(|candidate| is_equippable(world.atlas(), candidate.0, &wearer));
        let Some((item, level, rarity)) = drop else {
            continue;
        };
        let position = world.monster(victim).placement.position;
        let stamp = world.stamp_item_drop(DropOrigin::MonsterKill, Tick(0));
        let (ground, rolled) = world.drop_item_to_ground(item, level, rarity, position, stamp);
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
    let appeared = world
        .stamp_item_drop(DropOrigin::MonsterKill, Tick(0))
        .appearance;
    assert_eq!(
        world.pickup(caster, ground, anchor, PickerStanding::Owner, appeared),
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

// --- W-SKILLDMG standing gate: Energy-scaled wizardry drives a wizard kill. ---

/// One wizard's single cast of the shared spell at a 60-HP mob: the world, the
/// seated indices, and whether the cast landed. Both energies run the identical
/// construction under the identical seed, so the dice are shared and only the
/// Energy-derived wizardry span separates the outcomes.
fn wizard_cast(seed: u64, energy: u16) -> (World, usize, usize, bool) {
    let mut world = World::new(seed, MapNumber(0));
    let wizard = world.seat_character(dark_wizard(50, energy, tile(10, 10)));
    let spell = wizardry_direct_skill(world.atlas());
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let victim = world.seat_monster(monster_instance(number, 60, tile(11, 10)));
    let aim = pos(11, 10);
    let landed = match world.cast_damaging(wizard, spell, aim, &[victim]) {
        SkillOutcome::Cast { hits, .. } => hits
            .iter()
            .any(|hit| matches!(hit, TargetHit::Landed { .. } | TargetHit::Killed { .. })),
        SkillOutcome::Rejected { .. } => false,
    };
    (world, wizard, victim, landed)
}

#[test]
fn a_high_energy_wizard_kills_the_mob_a_low_energy_wizard_only_wounds() {
    // The wizard-kill gate: the DW multiplier is ×1, so the kill is driven
    // end-to-end (through persist) by the Energy-scaled wizardry span alone —
    // identical spell, identical mob, identical seed, only Energy differs.
    // Low energy 90 → span [10, 22] + add; high energy 900 → [100, 225] + add.
    let mut proven = false;
    for seed in 0u64..64 {
        let (low_world, _, low_victim, low_landed) = wizard_cast(seed, 90);
        let (mut high_world, high_wizard, high_victim, high_landed) = wizard_cast(seed, 900);
        if !(low_landed && high_landed) {
            continue;
        }
        // The low-Energy hit wounds: persisted health fell but stayed alive.
        let wounded = low_world.monster(low_victim).health.current();
        assert!(wounded > 0, "seed {seed}: the apprentice only wounds");
        assert!(
            wounded < 60,
            "seed {seed}: the apprentice's hit still bites"
        );
        // The high-Energy hit kills through the same persisted writeback.
        assert_eq!(
            high_world.monster(high_victim).health.current(),
            0,
            "seed {seed}: the master's identical spell kills"
        );
        // The wizard-kill produces a real reward through the same loot chain
        // the DK skill-kill uses.
        let resolution = high_world.resolve_kill_of(high_wizard, high_victim);
        assert!(
            resolution.experience.gained.0 > 0,
            "the wizard-kill grants experience"
        );
        if item_drops(&resolution).is_empty() {
            continue;
        }
        proven = true;
        break;
    }
    assert!(
        proven,
        "a seed in 0..64 lands both casts, kills on Energy, and drops an item"
    );
}

#[test]
fn a_wizardry_absent_or_none_type_cast_persists_the_scratch_never_a_weapon_hit() {
    // A DK (no wizardry interval) casting a wizardry spell, and the same DK
    // casting the None-type skill 50 (authored damage 120), both persist the
    // exact floor × multiplier scratch: max(1, 80/10) = 8, × 2030/1000 = 16 —
    // never the [50, 75] weapon span, never the authored 120.
    let mut world = World::new(77, MapNumber(0));
    let knight = world.seat_character(dark_knight(80, 300, tile(10, 10)));
    let wizardry = wizardry_direct_skill(world.atlas());
    let none_type = none_type_skill(world.atlas());
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);

    // Wizardry-absent: target ahead, aimed at its own tile.
    let far_victim = world.seat_monster(monster_instance(number, 300, tile(11, 10)));
    let mut scratched = false;
    for _ in 0..64 {
        let before = world.monster(far_victim).health.current();
        match world.cast_damaging(knight, wizardry, pos(11, 10), &[far_victim]) {
            SkillOutcome::Cast { .. } => {}
            SkillOutcome::Rejected { .. } => break,
        }
        let after = world.monster(far_victim).health.current();
        if after < before {
            assert_eq!(
                before - after,
                16,
                "the collapsed cast is the exact scratch"
            );
            scratched = true;
            break;
        }
    }
    assert!(scratched, "a wizardry-absent cast lands within 64 tries");

    // None-type: skill 50 has range 0, so the knight strikes a mob seated on
    // its own tile with a self-aimed cast.
    let near_victim = world.seat_monster(monster_instance(number, 300, tile(10, 10)));
    let mut scratched = false;
    for _ in 0..64 {
        let before = world.monster(near_victim).health.current();
        match world.cast_damaging(knight, none_type, pos(10, 10), &[near_victim]) {
            SkillOutcome::Cast { .. } => {}
            SkillOutcome::Rejected { .. } => break,
        }
        let after = world.monster(near_victim).health.current();
        if after < before {
            assert_eq!(
                before - after,
                16,
                "the None-type cast discards its authored 120 and lands the scratch"
            );
            scratched = true;
            break;
        }
    }
    assert!(scratched, "a None-type cast lands within 64 tries");
}

// --- W-AREA standing gates: authored geometry, the aim bound, the push, the ---
// --- lunge teleport, and the ≤1-tile step — all through persist. --------------

/// The first walkable / blocked / walkable horizontal triple on `map`'s real
/// terrain — a genuine one-tile wall with ground on both sides, re-found from
/// the shipped grid on every run, never a hard-coded tile. Returns the two
/// walkable flanks.
fn wall_triple(atlas: &mu_core::data::atlas::Atlas, map: MapNumber) -> (TileCoord, TileCoord) {
    let grid = or_abort(atlas.terrain_grid(map).ok_or("no terrain grid for map"));
    for y in 0u8..=u8::MAX {
        for x in 0u8..=u8::MAX - 2 {
            let near = TileCoord::new(x, y);
            let wall = TileCoord::new(x + 1, y);
            let far = TileCoord::new(x + 2, y);
            if grid.walkable(near.to_world())
                && !grid.walkable(wall.to_world())
                && grid.walkable(far.to_world())
            {
                return (near, far);
            }
        }
    }
    or_abort(Err::<(TileCoord, TileCoord), _>(
        "the map has a walkable-blocked-walkable triple",
    ))
}

#[test]
fn a_flame_cast_strikes_the_mob_inside_its_authored_circle_and_spares_the_one_outside() {
    // AOE-SIZE gate (Flame small): the authored r=1 aim circle end-to-end —
    // the mob one tile from the aim is struck and its persisted health falls;
    // the mob two tiles out is never even covered (the old range-derived r=6
    // disc swallowed both).
    let mut proven = false;
    for seed in 0u64..64 {
        let mut world = World::new(seed, MapNumber(0));
        let wizard = world.seat_character(dark_wizard(50, 400, tile(10, 10)));
        let flame = flame_skill(world.atlas());
        let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
        let near = world.seat_monster(monster_instance(number, 10_000, tile(11, 10)));
        let far = world.seat_monster(monster_instance(number, 10_000, tile(12, 10)));
        let outcome = world.cast_damaging(wizard, flame, pos(10, 10), &[near, far]);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("a funded flame over a covered mob resolves");
        };
        // Coverage is the authored size: only the near mob (batch index 0) is hit.
        assert_eq!(hits.len(), 1, "seed {seed}: exactly one mob is covered");
        assert!(
            hits.iter().all(|hit| matches!(
                hit,
                TargetHit::Missed {
                    target_index: 0,
                    ..
                } | TargetHit::Landed {
                    target_index: 0,
                    ..
                } | TargetHit::Killed {
                    target_index: 0,
                    ..
                }
            )),
            "seed {seed}: the covered mob is the near one"
        );
        assert_eq!(
            world.monster(far).health.current(),
            10_000,
            "seed {seed}: the mob two tiles out is untouched"
        );
        if world.monster(near).health.current() < 10_000 {
            proven = true;
            break;
        }
    }
    assert!(proven, "a seed in 0..64 lands the flame on the near mob");
}

#[test]
fn a_revived_hellfire_strikes_an_adjacent_mob_and_persists_its_damage() {
    // AOE-SIZE gate (Hellfire revived): the data-range-0 skill projects its
    // authored r=2 caster circle — the eruption covers the adjacent mob and its
    // persisted health falls. The range-0 dead-skill NoTargetsInRegion is gone.
    let mut proven = false;
    for seed in 0u64..64 {
        let mut world = World::new(seed, MapNumber(0));
        let wizard = world.seat_character(dark_wizard(50, 400, tile(10, 10)));
        let hellfire = hellfire_skill(world.atlas());
        let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
        let mob = world.seat_monster(monster_instance(number, 10_000, tile(12, 10)));
        let outcome = world.cast_damaging(wizard, hellfire, pos(10, 10), &[mob]);
        assert!(
            matches!(outcome, SkillOutcome::Cast { .. }),
            "seed {seed}: the revived hellfire covers its adjacent mob"
        );
        if world.monster(mob).health.current() < 10_000 {
            proven = true;
            break;
        }
    }
    assert!(
        proven,
        "a seed in 0..64 lands the hellfire on the adjacent mob"
    );
}

#[test]
fn an_aim_beyond_cast_range_is_rejected_and_no_mana_spend_persists() {
    // REJECT-FAR-AIM gate: an aim-centered area cast pointed past its cast
    // range is refused OutOfRange before any spend — the persisted mana is
    // untouched and the far mob never struck (the invariant-6 aim bound).
    let mut world = World::new(7, MapNumber(0));
    let wizard = world.seat_character(dark_wizard(50, 400, tile(10, 10)));
    let flame = flame_skill(world.atlas());
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let mob = world.seat_monster(monster_instance(number, 10_000, tile(30, 10)));
    let mana_before = world.character(wizard).vitals().mana.current();
    let outcome = world.cast_damaging(wizard, flame, pos(30, 10), &[mob]);
    assert_eq!(
        outcome,
        SkillOutcome::Rejected {
            reason: CastRejection::OutOfRange
        }
    );
    assert_eq!(
        world.character(wizard).vitals().mana.current(),
        mana_before,
        "no mana spend persists on the refused far aim"
    );
    assert_eq!(world.monster(mob).health.current(), 10_000);
}

#[test]
fn an_earthshake_scatters_struck_and_missed_mobs_and_a_killed_mob_stays_put() {
    // EARTHSHAKE-PUSH gate: the quake throws every caught mob directly away
    // from the caster — the persisted placements move exactly three tiles east
    // along the open corridor, a MISSED mob is scattered too (G2), and a killed
    // mob stays on its tile.
    let mut killed_stays = false;
    let mut missed_scattered = false;
    for seed in 0u64..128 {
        let mut world = World::new(seed, MapNumber(0));
        let run = walkable_run(world.atlas(), MapNumber(0), 8);
        let caster_tile = *or_abort(run.first().ok_or("run start"));
        let frail_tile = *or_abort(run.get(1).ok_or("run tile 1"));
        let sturdy_tile = *or_abort(run.get(2).ok_or("run tile 2"));
        let pushed_to = *or_abort(run.get(5).ok_or("run tile 5"));
        let knight = world.seat_character(dark_knight(80, 300, caster_tile));
        let quake = earthshake_skill(world.atlas());
        let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
        let frail = world.seat_monster(monster_instance(number, 1, frail_tile));
        let sturdy = world.seat_monster(monster_instance(number, 1_000_000, sturdy_tile));

        let outcome = world.cast_damaging(knight, quake, caster_tile.to_world(), &[frail, sturdy]);
        let SkillOutcome::Cast { hits, .. } = outcome else {
            panic!("seed {seed}: a funded quake over covered mobs resolves");
        };

        // The sturdy mob (batch index 1) is never killed: struck or missed, its
        // persisted placement is thrown exactly three tiles further east.
        assert_eq!(
            world.monster(sturdy).placement.position,
            pushed_to.to_world(),
            "seed {seed}: the sturdy mob is scattered three tiles away"
        );
        for hit in &hits {
            match hit {
                TargetHit::Missed {
                    target_index: 1,
                    displacement,
                    ..
                } => {
                    assert!(displacement.is_some(), "seed {seed}: a missed mob scatters");
                    missed_scattered = true;
                }
                TargetHit::Killed {
                    target_index: 0, ..
                } => {
                    // The killed frail mob stays put: its persisted placement is
                    // its seat, untouched by the push.
                    assert_eq!(
                        world.monster(frail).placement.position,
                        frail_tile.to_world(),
                        "seed {seed}: a killed mob is never pushed"
                    );
                    killed_stays = true;
                }
                TargetHit::Missed { .. } | TargetHit::Landed { .. } | TargetHit::Killed { .. } => {}
            }
        }
        if killed_stays && missed_scattered {
            break;
        }
    }
    assert!(
        killed_stays && missed_scattered,
        "the sweep reaches a killed-stays and a missed-scattered quake"
    );
}

#[test]
fn a_lunge_cast_lands_the_persisted_caster_on_its_target_across_a_wall() {
    // LUNGE-ACROSS-WALL gate: a real one-tile wall on Lorencia terrain stands
    // between the knight and its mark; the lunge teleports the caster onto the
    // target's exact cell and THAT placement persists — the classic dash,
    // proven end-to-end through the persist seam.
    let mut world = World::new(11, MapNumber(0));
    let (near, far) = wall_triple(world.atlas(), MapNumber(0));
    let knight = world.seat_character(dark_knight(80, 300, near));
    let lunge = lunge_skill(world.atlas());
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let mob = world.seat_monster(monster_instance(number, 10_000, far));
    let outcome = world.cast_damaging(knight, lunge, far.to_world(), &[mob]);
    assert!(
        matches!(outcome, SkillOutcome::Cast { .. }),
        "a funded lunge over its covered target resolves"
    );
    assert_eq!(
        world.character(knight).placement().position,
        far.to_world(),
        "the persisted caster placement is the target's tile, wall notwithstanding"
    );
}

#[test]
fn an_ordinary_step_toward_ground_across_a_wall_is_blocked_and_the_walker_stays() {
    // The ≤1-tile ordinary-step invariant through the paper host: the walker
    // faces walkable ground two tiles away with a real wall between. The step
    // drive can only hand `resolve_step` a `StepMagnitude` (whose constructors
    // bound every ordinary step to at most one tile — the deleted DASH_SPEED is
    // unrepresentable), so the step lands on the wall tile and is refused; the
    // persisted walker never tunnels.
    let mut world = World::new(3, MapNumber(0));
    let (near, far) = wall_triple(world.atlas(), MapNumber(0));
    let walker = world.seat_character(dark_knight(30, 150, near));
    assert_eq!(world.step(walker, far.to_world()), StepOutcome::Blocked);
    assert_eq!(
        world.character(walker).placement().position,
        near.to_world(),
        "the persisted walker stays on its side of the wall"
    );
}

#[test]
fn an_area_cast_clears_the_cluster_inside_its_authored_radius_and_the_outsider_survives() {
    // The golden-path area beat as its own flow (the threaded golden-path
    // identity is a Dark Knight mid-corridor; the authored-size beat needs a
    // wizard and a placed cluster, so it plays here — spec §3.3 allows the
    // dedicated scenario): one Flame cast kills the two frail mobs inside its
    // one-tile circle, the frail mob just outside survives untouched, and each
    // kill pays out its own reward.
    let mut proven = false;
    for seed in 0u64..64 {
        let mut world = World::new(seed, MapNumber(0));
        let wizard = world.seat_character(dark_wizard(50, 900, tile(10, 10)));
        let flame = flame_skill(world.atlas());
        let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
        let in_east = world.seat_monster(monster_instance(number, 1, tile(11, 10)));
        let in_south = world.seat_monster(monster_instance(number, 1, tile(10, 11)));
        let outside = world.seat_monster(monster_instance(number, 1, tile(12, 10)));
        let outcome =
            world.cast_damaging(wizard, flame, pos(10, 10), &[in_east, in_south, outside]);
        assert!(matches!(outcome, SkillOutcome::Cast { .. }));
        assert_eq!(
            world.monster(outside).health.current(),
            1,
            "seed {seed}: the mob just outside the authored radius survives"
        );
        if world.monster(in_east).health.current() != 0
            || world.monster(in_south).health.current() != 0
        {
            continue;
        }
        let reward_east = world.resolve_kill_of(wizard, in_east);
        let reward_south = world.resolve_kill_of(wizard, in_south);
        assert!(reward_east.experience.gained.0 > 0);
        assert!(reward_south.experience.gained.0 > 0);
        proven = true;
        break;
    }
    assert!(proven, "a seed in 0..64 clears the in-circle cluster");
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

    // D — the LIVE GATE (EQ-SIM-1, the W-EQUIP Case-D flip): equip now reads
    // the item's class list, so the elf-only bow is REFUSED by a Dark Knight
    // with ClassMismatch and the worn set stays byte-for-byte intact.
    let elf_seat = world.seat_character(dark_knight(80, 300, tile(11, 11)));
    let bare_worn = wire(world.equipment(elf_seat));
    let bow = item_instance(world.atlas(), ELF_BOW);
    match world.equip_into(elf_seat, bow, EquipmentSlot::RightHand) {
        EquipOutcome::Rejected { reason, item } => {
            assert_eq!(reason, EquipRejection::ClassMismatch);
            assert_eq!(item.item, ELF_BOW, "the refused bow rides the outcome back");
        }
        EquipOutcome::Equipped { .. } => {
            panic!("the class gate refuses an elf-only bow on a Dark Knight")
        }
    }
    assert_eq!(
        wire(world.equipment(elf_seat)),
        bare_worn,
        "a class-refused equip leaves the worn set untouched"
    );
}

// --- The W-EQUIP standing gates: the wear lifecycle + the fold in a fight. ---

/// Drives one strike→wear→break→repair lifecycle on construction `seed`
/// (EQ-SIM-2): a knight wearing a single one-point helm — the only
/// defensive-pool member, so every damaging bite grinds it — is bitten until
/// the helm's persisted wear ledger crosses its last point, then the broken
/// helm is repaired through the shipped W-SHOP repair service. `None` when
/// this seed never both accumulated a pre-break `Worn` event and broke inside
/// the budget — the caller sweeps. Every reached beat is a hard invariant.
fn wear_break_repair_lifecycle(seed: u64) -> Option<()> {
    let mut world = World::new(seed, MapNumber(0));
    let hero = world.seat_character(dark_knight(50, 200, tile(10, 10)));
    world.set_wallet(hero, zen(1_000_000));

    // A real helm worn through the LIVE gate, ground to its last point.
    let mut helm = item_instance(world.atlas(), HELM);
    let full_max = helm.durability.max();
    helm.durability = or_abort(Durability::new(1, full_max));
    assert!(matches!(
        world.equip_into(hero, helm, EquipmentSlot::Helm),
        EquipOutcome::Equipped { .. }
    ));

    // A pressing mob (out-rates the knight, bites for <= 70) co-located: its
    // sub-2000 bites MUST ride the persisted ledger before any point is lost.
    let mob = world.seat_monster(monster_instance(
        pressing_monster(world.atlas()),
        10_000,
        tile(10, 10),
    ));

    let mut worn_before_break = false;
    let mut broke = false;
    for _ in 0..10_000u32 {
        world.set_health(hero, Pool::full(500));
        let _ = world.player_struck_by_monster(hero, mob);
        for event in world.drain_wear_events() {
            match event {
                WearEvent::Worn { slot, durability } => {
                    assert_eq!(slot, EquipmentSlot::Helm, "the only pool member wears");
                    assert_eq!(
                        durability.current(),
                        1,
                        "the ledger accumulates without crossing before the break"
                    );
                    worn_before_break = true;
                }
                WearEvent::Broken { slot } => {
                    assert_eq!(slot, EquipmentSlot::Helm);
                    broke = true;
                }
                WearEvent::Destroyed { slot } => or_abort(Err::<(), String>(format!(
                    "wear never destroys gear, yet {slot:?} was destroyed"
                ))),
            }
        }
        if broke {
            break;
        }
    }
    if !(worn_before_break && broke) {
        return None;
    }

    // Broken: durability 0, STILL worn, the whole contribution off the fold.
    let broken_helm = or_abort(
        world
            .equipment(hero)
            .get(EquipmentSlot::Helm)
            .ok_or("the broken helm stays worn"),
    );
    assert_eq!(broken_helm.durability.current(), 0);
    let hero_char = world.character(hero).clone();
    assert_eq!(
        equipped_profile(&hero_char, world.equipment(hero), world.atlas()),
        character_profile(&hero_char).0,
        "a broken helm contributes nothing to the fold"
    );

    // A further bite finds an empty defensive pool — no wear event at all
    // (the broken-out-of-pool OUR-pin, live through the host).
    world.set_health(hero, Pool::full(500));
    let _ = world.player_struck_by_monster(hero, mob);
    assert!(
        world.drain_wear_events().is_empty(),
        "a broken piece leaves the wear pool"
    );

    // Repair through the shipped W-SHOP service: full stored max, wear
    // ledger zeroed (the full-gauge equality is ledger-inclusive), a real
    // price debited off the threaded wallet, the contribution returned.
    let wallet_before = world.character(hero).zen();
    match world.self_repair_worn(hero, EquipmentSlot::Helm) {
        RepairOutcome::Repaired { cost, balance } => {
            assert!(cost.0 > 0, "a real repair price is charged");
            assert_eq!(balance.get(), wallet_before.get() - cost.0);
            assert_eq!(world.character(hero).zen(), balance);
        }
        outcome @ (RepairOutcome::AlreadyFull
        | RepairOutcome::NotRepairableKind
        | RepairOutcome::Empty
        | RepairOutcome::OutOfRange
        | RepairOutcome::InsufficientZen) => or_abort(Err::<(), String>(format!(
            "the funded self-repair lands: {outcome:?}"
        ))),
    }
    let repaired = or_abort(
        world
            .equipment(hero)
            .get(EquipmentSlot::Helm)
            .ok_or("the repaired helm stays worn"),
    );
    assert_eq!(
        repaired.durability,
        Durability::full(full_max),
        "repair restores the stored max and zeroes the ledger"
    );
    let hero_char = world.character(hero).clone();
    assert!(
        equipped_profile(&hero_char, world.equipment(hero), world.atlas()).defense()
            > character_profile(&hero_char).0.defense(),
        "the repaired helm's contribution returns to the fold"
    );
    Some(())
}

#[test]
fn a_worn_item_wears_breaks_and_repairs_end_to_end_through_the_paper_host() {
    // EQ-SIM-2: the full strike→wear→break→repair lifecycle the
    // DURABILITY-COMBAT debt called undrivable, driven live through the paper
    // host over the real /data Atlas and the shipped W-SHOP repair.
    let proven = (0u64..16).any(|seed| wear_break_repair_lifecycle(seed).is_some());
    assert!(
        proven,
        "a seed in 0..16 drives the wear→break→repair lifecycle end to end"
    );
}

/// The strike budget both knights get against the armored mob.
const GEARED_STRIKE_BUDGET: u32 = 40;
/// The armored mob's seated health: above the bare knight's 40 × 5 = 200
/// budget ceiling, within the geared knight's ~7 landed hits.
const GEARED_MOB_HP: u32 = 300;

/// Strikes the mob up to the budget, stopping on the kill; the mob's
/// remaining health is returned.
fn fight_armored_mob(world: &mut World, hero: usize, mob: usize) -> u32 {
    for _ in 0..GEARED_STRIKE_BUDGET {
        if matches!(world.strike(hero, mob), AttackOutcome::Killed { .. }) {
            break;
        }
    }
    world.monster(mob).health.current()
}

/// EQ-SIM-3 on construction `seed`: two knights of identical class, level,
/// and stats — one bare, one wearing a real Blade +9 and helm through the
/// live gate — each fight the same armored mob under the same seed. The mob's
/// defense swallows the bare span whole, so every bare landed hit is the
/// level floor 5 and the bare knight can NEVER kill inside the budget (a hard
/// invariant on every seed); the geared knight's folded span kills. `None`
/// when this seed's geared fight ran out of budget or the bare fight never
/// landed a wound — the caller sweeps.
fn geared_kills_where_gearless_wounds(seed: u64) -> Option<()> {
    let mut bare_world = World::new(seed, MapNumber(0));
    let bare = bare_world.seat_character(dark_knight(50, 200, tile(10, 10)));
    let (number, combat) = armored_monster_from(bare_world.atlas(), 50);
    let bare_mob = bare_world.seat_monster(monster_instance(number, GEARED_MOB_HP, tile(10, 10)));

    let mut geared_world = World::new(seed, MapNumber(0));
    let geared = geared_world.seat_character(dark_knight(50, 200, tile(10, 10)));
    let geared_mob =
        geared_world.seat_monster(monster_instance(number, GEARED_MOB_HP, tile(10, 10)));
    let blade = item_at_level(geared_world.atlas(), BLADE, 9);
    assert!(matches!(
        geared_world.equip_into(geared, blade, EquipmentSlot::RightHand),
        EquipOutcome::Equipped { .. }
    ));
    let helm = item_instance(geared_world.atlas(), HELM);
    assert!(matches!(
        geared_world.equip_into(geared, helm, EquipmentSlot::Helm),
        EquipOutcome::Equipped { .. }
    ));

    // The fold is the observable cause: the geared span's MINIMUM clears the
    // bare span's MAXIMUM, and the mob's defense swallows the bare span.
    let geared_char = geared_world.character(geared).clone();
    let geared_profile = equipped_profile(
        &geared_char,
        geared_world.equipment(geared),
        geared_world.atlas(),
    );
    let bare_profile = character_profile(&geared_char).0;
    assert!(geared_profile.physical().min() > bare_profile.physical().max());
    assert!(
        combat.defense >= bare_profile.physical().max(),
        "the armored mob pins every bare landed hit to the level floor"
    );

    let bare_left = fight_armored_mob(&mut bare_world, bare, bare_mob);
    assert!(
        bare_left > 0,
        "40 floor-pinned hits cannot reach 300 — the bare knight only wounds"
    );
    let geared_left = fight_armored_mob(&mut geared_world, geared, geared_mob);
    (geared_left == 0 && bare_left < GEARED_MOB_HP).then_some(())
}

#[test]
fn a_geared_knight_kills_the_armored_mob_its_gearless_twin_only_wounds() {
    // EQ-SIM-3: end-to-end proof that the equipment fold changes a real
    // fight's outcome — the whole wave's point.
    let proven = (0u64..32).any(|seed| geared_kills_where_gearless_wounds(seed).is_some());
    assert!(
        proven,
        "a seed in 0..32 lets the geared knight kill where its bare twin wounds"
    );
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
    let stamp = world.stamp_zen_drop(DropOrigin::Ownerless, Tick(0));
    let pile = world.seat_ground_zen(Zen(1_500), pos(10, 10), stamp);
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
    let stamp = world.stamp_zen_drop(DropOrigin::Ownerless, Tick(0));
    let pile = world.seat_ground_zen(Zen(3_000), pos(10, 10), stamp);
    assert_eq!(world.pickup_zen(hero, pile), ZenPickupOutcome::PickedUp);
    match world.warp(hero, LORENCIA_WARP) {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_eq!(balance, zen(2_500), "4,500 earned minus the 2,000 fee");
            assert_eq!(world.character(hero).zen(), balance);
            assert_eq!(world.character(hero).placement(), placement);
            let grid = or_abort(
                world
                    .atlas()
                    .terrain_grid(placement.map)
                    .ok_or("the target map has a terrain grid"),
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
                    .terrain_grid(placement.map)
                    .ok_or("Lost Tower has a terrain grid"),
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
                    .terrain_grid(placement.map)
                    .ok_or("Lost Tower has a terrain grid"),
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
    // The kill's drops are stamped by the core seam: a monster drop appears a
    // beat AFTER the kill, and the item is claimed to the killer's snapshot
    // for the ownership window.
    let kill_tick = Tick(1);
    let item_stamp = world.stamp_item_drop(DropOrigin::MonsterKill, kill_tick);
    let zen_stamp = world.stamp_zen_drop(DropOrigin::MonsterKill, kill_tick);
    assert!(
        kill_tick.0 < item_stamp.appearance.0,
        "the corpse-to-loot beat stages the drop after the kill"
    );
    assert!(
        matches!(item_stamp.claim, DropClaim::Claimed { .. }),
        "a kill's item drop is claimed to the killer's snapshot"
    );

    // BEAT 6 — Pick up the money pile. The wallet is credited by EXACTLY the pile.
    assert_eq!(world.character(hero).zen(), zen(0));
    let ground_zen = world.seat_ground_zen(Zen(pile), mob_pos, zen_stamp);
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
    // any kill drop — drop_item_to_ground rolls the full instance). The owner
    // picks it inside its claim window, in reach; it leaves the ground and lands
    // in the bag once, byte-identical to the rolled instance.
    let (ground_item, rolled) = world.drop_item_to_ground(
        DRAGON_ARMOR,
        ItemLevel::ZERO,
        ItemRarity::Normal,
        mob_pos,
        item_stamp,
    );
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = world.inventory(hero).first_fit(footprint)?;
    assert_eq!(world.ground_item_count(), 1);
    assert_eq!(
        world.pickup(
            hero,
            ground_item,
            anchor,
            PickerStanding::Owner,
            item_stamp.appearance
        ),
        PickupOutcome::PickedUp { at: anchor }
    );
    assert_eq!(world.ground_item_count(), 0);

    // BEAT 7b — The leftover drop nobody picked despawns after its minute: the
    // reaper flips it off the persisted ground and reports it exactly once.
    let leftover_stamp = world.stamp_zen_drop(DropOrigin::MonsterKill, kill_tick);
    let _leftover = world.seat_ground_zen(Zen(9), mob_pos, leftover_stamp);
    assert_eq!(world.ground_zen_count(), 1);
    let despawns = world.reap_ground(leftover_stamp.despawn);
    assert_eq!(despawns.len(), 1, "one despawn event for the leftover pile");
    assert_eq!(world.ground_zen_count(), 0);
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

/// The worn plate is real (BEAT 8b): the equipment fold raises the hero's
/// defense above its gearless derivation, and a landed bite from a fresh mob
/// grinds the plate's persisted wear ledger — gear visibly changes combat and
/// visibly wears, inside the one headline run. `None` when no bite landed
/// inside the budget this seed — the caller sweeps. Every reached beat is a
/// hard invariant.
fn geared_bite(world: &mut World, hero: usize, mob_tile: TileCoord) -> Option<()> {
    let hero_char = world.character(hero).clone();
    let gearless = character_profile(&hero_char).0.defense();
    let geared = equipped_profile(&hero_char, world.equipment(hero), world.atlas()).defense();
    assert!(
        geared > gearless,
        "the worn plate raises the folded defense above the gearless derivation"
    );

    // A fresh biting mob; the hero's only worn piece is the plate, so any
    // damaging bite must grind it — the ledger advance is visible on the
    // persisted worn set's wire form.
    let (number, _combat, _resistances) = fighting_monster_from(world.atlas(), 30);
    let mob = world.seat_monster(monster_instance(number, 50, mob_tile));
    let before_bite = wire(world.equipment(hero));
    for _ in 0..64u32 {
        let _ = world.player_struck_by_monster(hero, mob);
        let worn = world
            .drain_wear_events()
            .into_iter()
            .any(|event| match event {
                WearEvent::Worn { slot, .. } | WearEvent::Broken { slot } => {
                    assert_eq!(slot, EquipmentSlot::Armor, "only the plate can wear");
                    true
                }
                WearEvent::Destroyed { slot } => or_abort(Err::<bool, String>(format!(
                    "wear never destroys gear, yet {slot:?} was destroyed"
                ))),
            });
        if worn {
            assert_ne!(
                wire(world.equipment(hero)),
                before_bite,
                "the plate's wear ledger advanced on the persisted worn set"
            );
            // The bitten hero walks on whole — the shopping beats read vitals.
            world.set_health(hero, Pool::full(500));
            return Some(());
        }
    }
    None
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
    // BEAT 8b — The worn plate folds into defense and wears under a real bite.
    geared_bite(&mut world, hero, mob_tile)?;
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
    let pile_stamp = world.stamp_zen_drop(DropOrigin::MonsterKill, Tick(0));
    let pile = WorldZen {
        amount: Zen(100_000),
        position: world.character(picker).placement().position,
        map: MapNumber(0),
        despawn: pile_stamp.despawn,
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

    // The grounded over-cap share carries the pile's own despawn tick, and the
    // reaper removes it like any other drop — no special case.
    assert_eq!(world.ground_zen(0).despawn, pile_stamp.despawn);
    let despawns = world.reap_ground(pile_stamp.despawn);
    assert_eq!(
        despawns.len(),
        1,
        "the grounded share despawns like any drop"
    );
    assert_eq!(world.ground_zen_count(), 0);

    // The solo pickup_zen path is unchanged, exercised alongside the party split.
    let stamp = world.stamp_zen_drop(DropOrigin::Ownerless, Tick(0));
    let solo_pile = world.seat_ground_zen(
        Zen(1000),
        world.character(third).placement().position,
        stamp,
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

// --- W-GROUND: the standing sixth gate (SIM-1..7) over real Lorencia. ---------

/// Whether the tile is walkable and NOT safe on `grid` — an open field tile.
fn field_tile(grid: &TerrainGrid, x: u8, y: u8) -> bool {
    let position = TileCoord::new(x, y).to_world();
    grid.walkable(position) && !grid.safe(position)
}

/// The first `(field, safe)` +X-adjacent tile pair on real Lorencia — the
/// town boundary, discovered from the shipped terrain, never hard-coded.
fn boundary_pair(grid: &TerrainGrid) -> (TileCoord, TileCoord) {
    for y in 0u8..=u8::MAX {
        for x in 0u8..u8::MAX {
            if field_tile(grid, x, y) && grid.safe(TileCoord::new(x + 1, y).to_world()) {
                return (TileCoord::new(x, y), TileCoord::new(x + 1, y));
            }
        }
    }
    or_abort(Err::<(TileCoord, TileCoord), _>(
        "Lorencia has a field tile bordering its safe core",
    ))
}

/// The first +X row lane of three field tiles followed by a safe tile — the
/// push runway into the town core, discovered from the real terrain.
fn push_lane(grid: &TerrainGrid) -> [TileCoord; 4] {
    for y in 0u8..=u8::MAX {
        for x in 0u8..u8::MAX - 3 {
            if field_tile(grid, x, y)
                && field_tile(grid, x + 1, y)
                && field_tile(grid, x + 2, y)
                && grid.safe(TileCoord::new(x + 3, y).to_world())
            {
                return [
                    TileCoord::new(x, y),
                    TileCoord::new(x + 1, y),
                    TileCoord::new(x + 2, y),
                    TileCoord::new(x + 3, y),
                ];
            }
        }
    }
    or_abort(Err::<[TileCoord; 4], _>(
        "Lorencia has a three-tile field lane into its safe core",
    ))
}

/// Lorencia's terrain grid, cloned out of the world's held atlas so the world
/// stays free to be driven mutably.
fn lorencia_grid(world: &World) -> TerrainGrid {
    or_abort(
        world
            .atlas()
            .terrain_grid(MapNumber(0))
            .ok_or("Lorencia has a terrain grid"),
    )
    .clone()
}

#[test]
fn sim_gate_a_drop_past_its_minute_flips_off_the_persisted_ground() {
    // SIM-1: a seated drop stamped 60 s off its appearance leaves the
    // persisted ground set when the world tick passes it, delivering one
    // despawn event per removed drop — and survives one tick before.
    let mut world = World::new(41, MapNumber(0));
    let armor = item_instance(world.atlas(), DRAGON_ARMOR);
    let item_stamp = world.stamp_item_drop(DropOrigin::Ownerless, Tick(0));
    let zen_stamp = world.stamp_zen_drop(DropOrigin::Ownerless, Tick(0));
    world.seat_ground_item(armor, pos(10, 10), item_stamp);
    world.seat_ground_zen(Zen(777), pos(10, 10), zen_stamp);

    let events = world.reap_ground(Tick(item_stamp.despawn.0 - 1));
    assert!(events.is_empty(), "one tick early nothing flips");
    assert_eq!(world.ground_item_count(), 1);
    assert_eq!(world.ground_zen_count(), 1);

    let events = world.reap_ground(item_stamp.despawn);
    assert_eq!(
        events.len(),
        2,
        "the item and the pile share the 60 s clock"
    );
    assert_eq!(world.ground_item_count(), 0);
    assert_eq!(world.ground_zen_count(), 0);
}

#[test]
fn sim_gate_walking_into_reach_turns_a_failed_pickup_into_a_success() {
    // SIM-2 (the S-REACH-1 mirror): eleven tiles out the pickup is refused
    // OutOfReach by core; the actor walks tile by tile — each step persisted —
    // and the retry, gated by the walked-to placement, succeeds.
    let mut world = World::new(42, MapNumber(0));
    let run = walkable_run(world.atlas(), MapNumber(0), 12);
    let start = *or_abort(run.first().ok_or("the run has a start"));
    let item_tile = *or_abort(run.get(11).ok_or("the run has an eleventh tile"));

    let alpha = world.seat_character(dark_knight(30, 150, start));
    let armor = item_instance(world.atlas(), DRAGON_ARMOR);
    let stamp = world.stamp_item_drop(DropOrigin::Ownerless, Tick(0));
    let ground = world.seat_ground_item(armor, item_tile.to_world(), stamp);

    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = or_abort(
        world
            .inventory(alpha)
            .first_fit(footprint)
            .ok_or("the empty bag has room"),
    );
    assert!(
        matches!(
            world.pickup(alpha, ground, anchor, PickerStanding::Stranger, Tick(0)),
            PickupOutcome::OutOfReach { .. }
        ),
        "eleven tiles out the pickup is out of reach"
    );

    let reach = Radius::from_tiles(3);
    while !world
        .character(alpha)
        .placement()
        .position
        .within_range(item_tile.to_world(), reach)
    {
        assert!(
            matches!(
                world.step(alpha, item_tile.to_world()),
                StepOutcome::Resolved { .. }
            ),
            "the walkable run never blocks a step"
        );
    }
    assert_eq!(
        world.pickup(alpha, ground, anchor, PickerStanding::Stranger, Tick(0)),
        PickupOutcome::PickedUp { at: anchor },
        "walked into reach, the same pickup lands"
    );
}

#[test]
fn sim_gate_the_ownership_window_refuses_at_five_seconds_and_frees_at_eleven() {
    // SIM-3: a real monster drop is claimed to the killer's kill-snapshot; a
    // stranger's pickup at appearance + 5 s is Refused and the same pickup at
    // appearance + 11 s succeeds.
    let mut world = World::new(43, MapNumber(0));
    let stranger = world.seat_character(dark_knight(30, 150, tile(10, 10)));
    let armor = item_instance(world.atlas(), DRAGON_ARMOR);
    let stamp = world.stamp_item_drop(DropOrigin::MonsterKill, Tick(0));
    let ground = world.seat_ground_item(armor, pos(10, 10), stamp);

    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = or_abort(
        world
            .inventory(stranger)
            .first_fit(footprint)
            .ok_or("the empty bag has room"),
    );
    let five_seconds_in = Tick(stamp.appearance.0 + 100);
    let eleven_seconds_in = Tick(stamp.appearance.0 + 220);

    assert!(
        matches!(
            world.pickup(
                stranger,
                ground,
                anchor,
                PickerStanding::Stranger,
                five_seconds_in
            ),
            PickupOutcome::Refused { .. }
        ),
        "inside the 10 s window a stranger is refused"
    );
    assert_eq!(world.ground_item_count(), 1, "the refused item stays put");
    assert_eq!(
        world.pickup(
            stranger,
            ground,
            anchor,
            PickerStanding::Stranger,
            eleven_seconds_in
        ),
        PickupOutcome::PickedUp { at: anchor },
        "past the window the drop is free-for-all"
    );
    assert_eq!(world.ground_item_count(), 0);
}

#[test]
fn sim_gate_a_monster_drop_is_seated_one_second_after_the_kill_and_not_before() {
    // SIM-4: a real kill at tick T; the core stamp stages the drop's
    // appearance at T + 1 s, so before that tick the host has seated nothing
    // and there is no ground drop to pick; at the appearance it is seated and
    // the pickup proceeds.
    let mut world = World::new(44, MapNumber(0));
    let killer = world.seat_character(dark_knight(80, 300, tile(10, 10)));
    let (number, combat, _resistances) = low_level_monster(world.atlas(), 20);
    let victim = world.seat_monster(monster_instance(number, combat.hp, tile(10, 10)));
    loop {
        match world.strike(killer, victim) {
            AttackOutcome::Killed { .. } => break,
            AttackOutcome::Landed { .. } | AttackOutcome::Missed => {}
        }
    }

    let kill_tick = Tick(0);
    let stamp = world.stamp_item_drop(DropOrigin::MonsterKill, kill_tick);
    assert_eq!(
        stamp.appearance.0,
        kill_tick.0 + 20,
        "the 1 s beat at the 50 ms cadence"
    );
    // Before the appearance the host has not seated the drop: nothing is on
    // the ground to reap or pick.
    assert_eq!(world.ground_item_count(), 0, "pre-beat there is no drop");

    // At the appearance the host seats it, and the killer picks it up.
    let position = world.monster(victim).placement.position;
    let (ground, _rolled) = world.drop_item_to_ground(
        DRAGON_ARMOR,
        ItemLevel::ZERO,
        ItemRarity::Normal,
        position,
        stamp,
    );
    let footprint = footprint_of(world.atlas(), DRAGON_ARMOR);
    let anchor = or_abort(
        world
            .inventory(killer)
            .first_fit(footprint)
            .ok_or("the empty bag has room"),
    );
    assert_eq!(
        world.pickup(
            killer,
            ground,
            anchor,
            PickerStanding::Owner,
            stamp.appearance
        ),
        PickupOutcome::PickedUp { at: anchor }
    );
}

#[test]
fn sim_gate_a_town_cast_is_refused_and_a_town_stander_is_untouched_by_an_aoe() {
    // SIM-5: a cast from Lorencia's safe core is rejected CasterInSafezone
    // with no persisted spend, and a field caster's area sweep leaves the
    // town-standing mob's persisted health untouched while the field mob is
    // struck.
    let mut world = World::new(45, MapNumber(0));
    let grid = lorencia_grid(&world);
    let (field, safe) = boundary_pair(&grid);

    // A funded caster parked in town: refused, nothing spent.
    let town_caster = world.seat_character(dark_wizard(50, 200, safe));
    let skill = direct_hit_skill(world.atlas());
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let prey = world.seat_monster(monster_instance(number, 1_000_000, field));
    let mana_before = world.character(town_caster).vitals().mana.current();
    assert_eq!(
        world.cast_damaging(town_caster, skill, field.to_world(), &[prey]),
        SkillOutcome::Rejected {
            reason: CastRejection::CasterInSafezone
        }
    );
    assert_eq!(
        world.character(town_caster).vitals().mana.current(),
        mana_before,
        "no persisted spend on a town-refused cast"
    );

    // A field caster's Nova covers both standers; only the field one takes
    // persisted damage.
    let field_caster_tile = TileCoord::new(field.x().saturating_sub(1), field.y());
    let field_caster = world.seat_character(dark_wizard(50, 200, field_caster_tile));
    let nova = nova_skill(world.atlas());
    let town_mob = world.seat_monster(monster_instance(number, 1_000_000, safe));
    let town_before = world.monster(town_mob).health.current();
    let outcome = world.cast_damaging(
        field_caster,
        nova,
        field_caster_tile.to_world(),
        &[town_mob, prey],
    );
    let SkillOutcome::Cast { hits, .. } = outcome else {
        panic!("a funded field cast resolves");
    };
    assert!(!hits.is_empty(), "the field mob is struck");
    assert_eq!(
        world.monster(town_mob).health.current(),
        town_before,
        "the town-standing mob takes no persisted damage"
    );
    assert!(
        world.monster(prey).health.current() < 1_000_000,
        "the field mob's persisted health fell"
    );
}

#[test]
fn sim_gate_a_member_parked_in_town_earns_no_exp_and_no_zen() {
    // SIM-6: a field killer/picker with a party member standing in Lorencia's
    // safe core — the town-stander's persisted experience and zen are both
    // unchanged (pins 1-2).
    let mut world = World::new(46, MapNumber(0));
    let grid = lorencia_grid(&world);
    let (field, safe) = boundary_pair(&grid);

    let killer = world.seat_character(dark_knight(20, 150, field)); // slot 0
    let parked = world.seat_character(dark_knight(20, 150, safe)); // slot 1
    let party = world.seat_party(PartySession::forming());

    let facts = vec![
        world.member_fact(MemberSlot(0), Vitality::Alive),
        world.member_fact(MemberSlot(1), Vitality::Alive),
    ];
    let killer_exp_before = world.character(killer).experience().0;
    let parked_exp_before = world.character(parked).experience().0;
    let awards =
        world.distribute_kill_experience(party, &facts, MemberSlot(0), or_abort(Level::new(30)));
    let slots: Vec<u8> = awards.iter().map(|(award, _events)| award.slot.0).collect();
    assert_eq!(slots, vec![0], "only the field killer earns");
    assert!(world.character(killer).experience().0 > killer_exp_before);
    assert_eq!(
        world.character(parked).experience().0,
        parked_exp_before,
        "the town-stander's persisted experience is unchanged"
    );

    // The zen split with the same seats: the picker keeps the whole pile.
    let pile_stamp = world.stamp_zen_drop(DropOrigin::MonsterKill, Tick(0));
    let pile = WorldZen {
        amount: Zen(50_001),
        position: world.character(killer).placement().position,
        map: MapNumber(0),
        despawn: pile_stamp.despawn,
    };
    let wallets = vec![
        world.slot_wallet(MemberSlot(0)),
        world.slot_wallet(MemberSlot(1)),
    ];
    let result = world.split_zen_pickup(party, &pile, &facts, MemberSlot(0), &wallets);
    assert_eq!(result.credits.len(), 1);
    assert_eq!(
        world.character(killer).zen(),
        zen(50_001),
        "the picker keeps it all"
    );
    assert_eq!(
        world.character(parked).zen(),
        zen(0),
        "the town-stander's persisted zen is unchanged"
    );
}

#[test]
fn sim_gate_push_and_jiggle_both_stop_at_the_safezone_line() {
    // SIM-7: an Earthshake pushing a real mob toward the town core parks its
    // persisted placement on the last field tile; a lightning jiggle rolled
    // toward the safe boundary never lands the persisted target on a safe
    // tile.
    let mut world = World::new(47, MapNumber(0));
    let grid = lorencia_grid(&world);
    let [caster_tile, target_tile, last_field, first_safe] = push_lane(&grid);

    let knight = world.seat_character(dark_knight(80, 300, caster_tile));
    let quake = earthshake_skill(world.atlas());
    let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
    let pushed = world.seat_monster(monster_instance(number, 1_000_000, target_tile));
    let outcome = world.cast_damaging(knight, quake, caster_tile.to_world(), &[pushed]);
    assert!(matches!(outcome, SkillOutcome::Cast { .. }));
    assert_eq!(
        world.monster(pushed).placement.position,
        last_field.to_world(),
        "the persisted push stops on the last field tile"
    );
    assert_ne!(
        world.monster(pushed).placement.position,
        first_safe.to_world()
    );

    // The jiggle: fresh wizard + mob per attempt so every cast is funded; the
    // persisted placement never reads safe, and some attempt lands a move.
    let bolt = lightning_direct_skill(world.atlas());
    let wizard_tile = TileCoord::new(target_tile.x().saturating_sub(1), target_tile.y());
    let mut saw_move = false;
    for _ in 0..48u32 {
        let wizard = world.seat_character(dark_wizard(50, 200, wizard_tile));
        let jiggled = world.seat_monster(monster_instance(number, 1_000_000, target_tile));
        let outcome = world.cast_damaging(wizard, bolt, target_tile.to_world(), &[jiggled]);
        assert!(matches!(outcome, SkillOutcome::Cast { .. }));
        let landed = world.monster(jiggled).placement.position;
        assert!(
            !grid.safe(landed),
            "a jiggled target never lands on a safe tile"
        );
        if landed != target_tile.to_world() {
            saw_move = true;
        }
    }
    assert!(saw_move, "some attempt lands a real jiggle move");
}

// --- W-PVP standing sixth gate: player-versus-player combat over real ---------
// --- Lorencia. A single-target force-attack kill routes the victim through -----
// --- the death loop with the core-computed player-kill penalty (waived), and --
// --- an area sweep spares an incidental enemy player until it is designated. ---

/// The first horizontal run of `length` consecutive open field tiles (walkable
/// and not safe) on `grid`, discovered from the real terrain — the field a
/// player-versus-player scene is staged on, never a hard-coded tile.
fn field_run(grid: &TerrainGrid, length: usize) -> Vec<TileCoord> {
    for y in 0u8..=u8::MAX {
        let mut run: Vec<TileCoord> = Vec::new();
        for x in 0u8..=u8::MAX {
            if field_tile(grid, x, y) {
                run.push(TileCoord::new(x, y));
                if run.len() == length {
                    return run;
                }
            } else {
                run.clear();
            }
        }
    }
    or_abort(Err::<Vec<TileCoord>, _>(
        "Lorencia has a run of open field tiles",
    ))
}

/// The batch positions a cast struck, in the order the outcome lists them (a
/// missed target is still in the struck set); a rejection struck nothing.
fn struck_indices(outcome: &SkillOutcome) -> Vec<usize> {
    match outcome {
        SkillOutcome::Cast { hits, .. } => hits
            .iter()
            .map(|hit| match hit {
                TargetHit::Killed { target_index, .. }
                | TargetHit::Landed { target_index, .. }
                | TargetHit::Missed { target_index, .. } => *target_index,
            })
            .collect(),
        SkillOutcome::Rejected { .. } => Vec::new(),
    }
}

#[test]
fn sim_gate_two_players_fight_outside_safezone_one_kills_the_other_penalty_free() {
    // Two players adjacent on Lorencia field tiles: the attacker force-attacks the
    // defender with a single-target skill until it is Killed (PvP overrate is
    // suppressed, so full damage lands). The host routes the Player victim to the
    // death step with the penalty the CORE rule computes from the killer's kind —
    // a player kill waives it, so the victim loses no experience and no zen — then
    // respawns it in the town safezone; the killer receives nothing. Then the same
    // two, both on a safe tile, cannot touch: the cast is refused CasterInSafezone.
    let mut world = World::new(51, MapNumber(0));
    let grid = lorencia_grid(&world);
    let field = field_run(&grid, 2);
    let safe = boundary_pair(&grid).1;

    let attacker = world.seat_character(dark_knight(150, 1500, field[0]));
    let victim = world.seat_character(dark_knight_in_band(
        world.atlas(),
        60,
        1_000_000,
        MapNumber(0),
        field[1],
    ));
    let skill = direct_hit_skill(world.atlas());
    let aim = field[1].to_world();
    let victim_exp_before = world.character(victim).experience();
    let victim_zen_before = world.character(victim).zen();

    // Force-attack the defender (batch index 0) until the single-target strike
    // kills it.
    let mut killed = false;
    for _ in 0..10_000u32 {
        let outcome = world.cast_at(
            attacker,
            skill,
            aim,
            &[Combatant::Player(victim)],
            Designation::Forced { target_index: 0 },
        );
        if let SkillOutcome::Cast { hits, .. } = &outcome {
            if hits
                .iter()
                .any(|hit| matches!(hit, TargetHit::Killed { .. }))
            {
                killed = true;
                break;
            }
        }
    }
    assert!(
        killed,
        "the force-attacked defender is beaten to a killing blow"
    );
    assert_eq!(world.character(victim).vitals().health.current(), 0);

    // The death step under the CORE-computed player-kill penalty: waived, so no
    // experience or zen is docked and the persisted totals are unchanged.
    let death_events = world.resolve_combat_death(victim, Tick(500), TargetKind::Player);
    assert!(
        !death_events.iter().any(|event| matches!(
            event,
            DeathEvent::ExperienceDocked { .. } | DeathEvent::ZenDocked { .. }
        )),
        "a player kill docks the victim no experience and no zen"
    );
    assert_eq!(
        world.character(victim).experience(),
        victim_exp_before,
        "the player-killed victim's persisted experience is unchanged"
    );
    assert_eq!(
        world.character(victim).zen(),
        victim_zen_before,
        "the player-killed victim's persisted zen is unchanged"
    );

    // Respawn seats the victim alive inside the town safezone, still penalty-free.
    let respawned = world
        .respawn_player(victim)
        .expect("a dead player respawns");
    assert_eq!(world.character(victim).life(), LifeState::Alive);
    let landing = world.character(victim).placement();
    assert_eq!(respawned.map, landing.map);
    let town = or_abort(
        world
            .atlas()
            .terrain_grid(landing.map)
            .ok_or("the respawn map has a terrain grid"),
    );
    assert!(
        town.safe(landing.position),
        "the victim respawns inside the town safezone"
    );
    assert_eq!(
        world.character(victim).experience(),
        victim_exp_before,
        "respawn restores no penalty either"
    );

    // The truce: with both standers on a safe tile the attacker's cast is refused
    // before any target is considered — players cannot fight inside the safezone.
    let safe_attacker = world.seat_character(dark_knight(150, 1500, safe));
    let safe_victim = world.seat_character(dark_knight(30, 150, safe));
    assert_eq!(
        world.cast_at(
            safe_attacker,
            skill,
            safe.to_world(),
            &[Combatant::Player(safe_victim)],
            Designation::Forced { target_index: 0 },
        ),
        SkillOutcome::Rejected {
            reason: CastRejection::CasterInSafezone
        }
    );
}

#[test]
fn sim_gate_nova_around_an_enemy_player_kills_monsters_but_not_the_player() {
    // A Nova centered on the caster covers a co-located enemy player and two
    // monsters. An Incidental sweep strikes only the two monsters — the enemy
    // player's batch index is absent from the struck set and its persisted health
    // is untouched — while a Forced designation on the player's index strikes it
    // too. The Incidental sweep replays byte-for-byte under a fixed seed. Batch
    // order pins the struck-index assertions: player at 0, monsters at 1 and 2.
    let nova_scene = |seed: u64| {
        let mut world = World::new(seed, MapNumber(0));
        let grid = lorencia_grid(&world);
        let field = field_run(&grid, 4);
        let caster = world.seat_character(dark_wizard(80, 400, field[0]));
        let player = world.seat_character(dark_knight(30, 150, field[1]));
        let (number, _combat, _resistances) = low_level_monster(world.atlas(), 20);
        let mon_a = world.seat_monster(monster_instance(number, 1_000_000, field[2]));
        let mon_b = world.seat_monster(monster_instance(number, 1_000_000, field[3]));
        let batch = vec![
            Combatant::Player(player),
            Combatant::Monster(mon_a),
            Combatant::Monster(mon_b),
        ];
        (
            world,
            caster,
            player,
            mon_a,
            mon_b,
            batch,
            field[0].to_world(),
        )
    };

    // Incidental: only the two monsters are struck; the enemy player is spared.
    let (mut world, caster, player, mon_a, mon_b, batch, aim) = nova_scene(52);
    let nova = nova_skill(world.atlas());
    let player_before = world.character(player).vitals().health.current();
    let outcome = world.cast_at(caster, nova, aim, &batch, Designation::Incidental);
    assert_eq!(
        struck_indices(&outcome),
        vec![1, 2],
        "the incidental sweep strikes only the two monsters (batch 1, 2)"
    );
    assert_eq!(
        world.character(player).vitals().health.current(),
        player_before,
        "the enemy player's persisted health is untouched by the incidental sweep"
    );
    assert!(
        world.monster(mon_a).health.current() < 1_000_000,
        "monster A took persisted nova damage"
    );
    assert!(
        world.monster(mon_b).health.current() < 1_000_000,
        "monster B took persisted nova damage"
    );

    // Byte-stable: the same seed replays the same cast bit-for-bit.
    let (mut twin, twin_caster, _tp, _ta, _tb, twin_batch, twin_aim) = nova_scene(52);
    let twin_outcome = twin.cast_at(
        twin_caster,
        nova,
        twin_aim,
        &twin_batch,
        Designation::Incidental,
    );
    assert_eq!(
        wire(&twin_outcome),
        wire(&outcome),
        "the incidental nova replays byte-for-byte under a fixed seed"
    );

    // Forced: designating the player's index strikes the player and both monsters.
    let (mut forced, forced_caster, forced_player, _fa, _fb, forced_batch, forced_aim) =
        nova_scene(52);
    let forced_before = forced.character(forced_player).vitals().health.current();
    let forced_outcome = forced.cast_at(
        forced_caster,
        nova,
        forced_aim,
        &forced_batch,
        Designation::Forced { target_index: 0 },
    );
    let mut hits = struck_indices(&forced_outcome);
    hits.sort_unstable();
    assert_eq!(
        hits,
        vec![0, 1, 2],
        "a forced designation on the player's index strikes the player and both monsters"
    );
    assert!(
        forced.character(forced_player).vitals().health.current() < forced_before,
        "the force-attacked enemy player takes persisted nova damage"
    );
}

// --- W-PK standing sixth gate: the open-world player-kill reputation lifecycle -
// --- over real Lorencia. Two innocents murdered flag a clean knight up the -----
// --- murderer ladder to the guard-huntable FirstStage; a town guard hunts the --
// --- murderer that flees into the safezone (its AiTarget built wholly from live -
// --- authoritative state); a monster kill accelerates the decay; elapsed online -
// --- time fades it back to Clean; and killing an already-flagged murderer is ----
// --- free, leaving its killer unflagged. Every reputation transition is core- ---
// --- computed and drives ZERO RNG. -------------------------------------------

/// Force-attacks the seated player at `victim` from the attacker at `attacker`
/// with the single-target `skill` aimed at `aim`, until the strike lands a
/// killing blow. Aborts if `10_000` casts do not kill it — statistically
/// impossible for a high-strength knight over a same-or-lower-level victim.
fn force_attack_to_death(
    world: &mut World,
    attacker: usize,
    victim: usize,
    skill: SkillNumber,
    aim: WorldPos,
) {
    for _ in 0..10_000u32 {
        let outcome = world.cast_at(
            attacker,
            skill,
            aim,
            &[Combatant::Player(victim)],
            Designation::Forced { target_index: 0 },
        );
        if let SkillOutcome::Cast { hits, .. } = &outcome {
            if hits
                .iter()
                .any(|hit| matches!(hit, TargetHit::Killed { .. }))
            {
                return;
            }
        }
    }
    or_abort(Err::<(), _>(
        "the force-attacked player is beaten to a killing blow",
    ));
}

/// Drives one open-world player kill end-to-end over the paper host: the killer
/// beats the victim to a killing blow, the victim is routed through the core
/// player-kill death (penalty Waived by `combat_death_penalty`), and the killer's
/// reputation is updated by the core sanction path. Returns the `PkEvent` the flag
/// path produced — a flag climb on a clean victim, a free `Sanctioned` on an
/// already-hunted one — composing the W-PVP death driver with the W-PK sanction
/// driver, both reading core-computed facts.
fn drive_player_kill(
    world: &mut World,
    killer: usize,
    victim: usize,
    aim: WorldPos,
    skill: SkillNumber,
    at: Tick,
) -> PkEvent {
    force_attack_to_death(world, killer, victim, skill, aim);
    world.resolve_combat_death(victim, at, TargetKind::Player);
    world.resolve_player_kill_of(killer, victim, at)
}

#[test]
fn sim_gate_a_murderer_is_flagged_hunted_by_guards_and_decays_back_to_clean() {
    // The full open-world PK lifecycle over real Lorencia on one threaded
    // identity: a clean knight murders two innocents on the open field (flagging
    // Warning, then the guard-huntable FirstStage), flees one tile east into the
    // town safezone, a town guard hunts it there (the guard's AiTarget built
    // wholly from the murderer's LIVE authoritative reputation and position), a
    // monster kill accelerates the decay, elapsed online time fades it back to
    // Clean, and the same guard then leaves the now-clean ex-murderer alone.
    let mut world = World::new(53, MapNumber(0));
    let grid = lorencia_grid(&world);
    // A three-tile field lane running into the safe town core: the murders happen
    // on the field, the flight ends one tile inside the safezone.
    let lane = push_lane(&grid);
    let skill = direct_hit_skill(world.atlas());

    // A would-be murderer beside two innocents piled on the neighbouring field
    // tile (positional identity is the index, not the tile, so both share it).
    let murderer = world.seat_character(dark_knight(150, 1500, lane[2]));
    let innocent_a = world.seat_character(dark_knight_in_band(
        world.atlas(),
        60,
        1_000_000,
        MapNumber(0),
        lane[1],
    ));
    let innocent_b = world.seat_character(dark_knight_in_band(
        world.atlas(),
        60,
        1_000_000,
        MapNumber(0),
        lane[1],
    ));
    assert_eq!(
        world.character(murderer).reputation().standing(),
        Standing::Clean
    );

    // First open murder: an unsanctioned kill of a clean victim flags Warning —
    // not yet hunted, not yet free-to-kill.
    let first = drive_player_kill(
        &mut world,
        murderer,
        innocent_a,
        lane[1].to_world(),
        skill,
        Tick(1000),
    );
    assert!(matches!(
        first,
        PkEvent::Flagged {
            stage: PkStage::Warning,
            ..
        }
    ));
    assert!(
        !world
            .character(murderer)
            .reputation()
            .standing()
            .is_hunted()
    );

    // Second open murder: climbs to FirstStage — now guard-huntable and itself
    // free-to-kill, with a lifetime tally of two.
    let second = drive_player_kill(
        &mut world,
        murderer,
        innocent_b,
        lane[1].to_world(),
        skill,
        Tick(2000),
    );
    assert!(matches!(
        second,
        PkEvent::Flagged {
            stage: PkStage::FirstStage,
            ..
        }
    ));
    assert!(
        world
            .character(murderer)
            .reputation()
            .standing()
            .is_hunted()
    );
    assert_eq!(
        world.character(murderer).reputation().kills(),
        PlayerKillCount(2)
    );

    // The murderer flees one tile east off the field lane into the town safezone.
    match world.step(murderer, lane[3].to_world()) {
        StepOutcome::Resolved { .. } => {}
        StepOutcome::Blocked => {
            or_abort(Err::<(), _>("the flight into town must not block"));
        }
    }
    assert!(
        grid.safe(world.character(murderer).placement().position),
        "the murderer stands inside the town safezone"
    );

    // A town guard on the field edge hunts the murderer that fled inside: the
    // AiTarget is built wholly from the murderer's live authoritative state, so the
    // guard swings only because core ruled it a hunted murderer on a safe tile.
    let guard = world.seat_monster(monster_instance(
        guard_monster(world.atlas()),
        10_000,
        lane[2],
    ));
    let hunt = world.advance_guard_against(guard, murderer, Tick(3000));
    assert_eq!(
        hunt,
        MonsterIntent::Attack {
            target: lane[3].to_world()
        }
    );

    // The murderer works the flag off by hunting: a monster kill accelerates the
    // decay (pulls the deadline earlier) without peeling the rung — still hunted.
    let (number, combat, _resistances) = low_level_monster(world.atlas(), 20);
    let prey = world.seat_monster(monster_instance(number, combat.hp, lane[1]));
    let accel = world.accelerate_decay_of(murderer, prey);
    assert!(matches!(accel, Some(PkEvent::DecayAccelerated { .. })));
    assert!(
        world
            .character(murderer)
            .reputation()
            .standing()
            .is_hunted(),
        "the accelerator pulls the deadline but never peels a rung"
    );

    // Elapsed online time decays the murderer all the way back to Clean; the
    // lifetime tally survives the fade.
    let faded = world.decay_reputation_of(murderer, Tick(10_000_000));
    assert!(matches!(
        faded,
        Some(PkEvent::Decayed {
            standing: Standing::Clean
        })
    ));
    assert_eq!(
        world.character(murderer).reputation().standing(),
        Standing::Clean
    );
    assert_eq!(
        world.character(murderer).reputation().kills(),
        PlayerKillCount(2)
    );

    // The same guard no longer hunts the now-clean ex-murderer standing in town.
    let calm = world.advance_guard_against(guard, murderer, Tick(4000));
    assert!(
        !matches!(calm, MonsterIntent::Attack { .. }),
        "a clean stander on a safe tile is no target for the guard"
    );
}

#[test]
fn sim_gate_killing_a_murderer_is_free() {
    // The victim-was-murderer carve-out over real Lorencia: a villain earns the
    // guard-huntable FirstStage by murdering two innocents, then a clean hunter
    // runs it down — killing a >=FirstStage victim is FREE (the sanction is
    // VictimWasMurderer), so the hunter never flags.
    let mut world = World::new(59, MapNumber(0));
    let grid = lorencia_grid(&world);
    let field = field_run(&grid, 2);
    let skill = direct_hit_skill(world.atlas());

    // A villain beside two innocents piled on the neighbouring field tile.
    let villain = world.seat_character(dark_knight(150, 1500, field[1]));
    let innocent_a = world.seat_character(dark_knight_in_band(
        world.atlas(),
        60,
        1_000_000,
        MapNumber(0),
        field[0],
    ));
    let innocent_b = world.seat_character(dark_knight_in_band(
        world.atlas(),
        60,
        1_000_000,
        MapNumber(0),
        field[0],
    ));

    // Two open murders earn the villain the guard-huntable FirstStage.
    drive_player_kill(
        &mut world,
        villain,
        innocent_a,
        field[0].to_world(),
        skill,
        Tick(1000),
    );
    let climbed = drive_player_kill(
        &mut world,
        villain,
        innocent_b,
        field[0].to_world(),
        skill,
        Tick(2000),
    );
    assert!(matches!(
        climbed,
        PkEvent::Flagged {
            stage: PkStage::FirstStage,
            ..
        }
    ));
    assert!(world.character(villain).reputation().standing().is_hunted());

    // A clean hunter runs the murderer down. Killing a hunted murderer is free —
    // player_kill_sanction reads the villain's AUTHORITATIVE reputation and rules
    // the kill VictimWasMurderer, so the hunter flags nothing.
    let hunter = world.seat_character(dark_knight(150, 1500, field[0]));
    assert_eq!(
        world.character(hunter).reputation().standing(),
        Standing::Clean
    );
    let free = drive_player_kill(
        &mut world,
        hunter,
        villain,
        field[1].to_world(),
        skill,
        Tick(3000),
    );
    assert!(matches!(
        free,
        PkEvent::Sanctioned {
            reason: SanctionReason::VictimWasMurderer
        }
    ));
    assert_eq!(
        world.character(hunter).reputation().standing(),
        Standing::Clean,
        "killing a murderer leaves the hunter clean"
    );
    assert_eq!(
        world.character(hunter).reputation().kills(),
        PlayerKillCount(0),
        "a free kill records no lifetime tally"
    );
}

// --- W-MINIGAME standing sixth gate: the shared event framework through the ---
// --- persist seam — a full event, its byte-for-byte replay, the min-player ----
// --- abort refund, the waived-death eject and dead-flagged finish, the --------
// --- empty-roster end, the declared-winner early end, and the overlapping -----
// --- wave schedule with wave-scoped respawn. Every session value crosses the --
// --- persist seam between calls; the host CALLS the framework seams and reads -
// --- the core-computed deadlines off the returned phase, never re-deriving ----
// --- the phase/deadline/score arithmetic inline. --------------------------------

/// The event tier every mini-game scenario authors at.
fn sim_level() -> EventLevel {
    or_abort(EventLevel::new(3))
}

/// Seats a Dark Knight at `level` carrying `wallet` zen with a `charges`-charge
/// Devil's Invitation in its bag — a ready entrant. Scenarios seat in roster
/// order, so the entrant at character index `i` is admitted to `RosterSlot(i)`
/// (the positional account↔slot convention the reward fan-out reads back).
fn seat_ticketed_entrant(world: &mut World, level: u16, wallet: u64, charges: u8) -> usize {
    let index = world.seat_character(dark_knight(level, 300, tile(10, 10)));
    world.set_wallet(index, zen(wallet));
    let ticket_ref = devil_square_ticket_ref(world.atlas());
    let ticket = devil_square_ticket(ticket_ref, charges, or_abort(ItemLevel::new(2)));
    let placed = world.place_in_bag(index, ticket, or_abort(Footprint::new(1, 1)), cell(0, 0));
    assert!(
        matches!(placed, PlaceOutcome::Placed { .. }),
        "the ticket seats into the entrant's bag: {placed:?}"
    );
    index
}

/// Advances the session at `s` through the entrance close and the 30 s countdown
/// into Playing — reading each core-computed deadline off the returned phase —
/// and returns the game-start events (the `GameStarted` freeze plus any wave that
/// fires at offset zero).
fn advance_to_playing(world: &mut World, s: usize) -> Vec<MiniGameEvent> {
    let MiniGamePhase::Open { closes_at, .. } = world.mini_session(s).phase else {
        return or_abort(Err(format!(
            "expected Open, got {:?}",
            world.mini_session(s).phase
        )));
    };
    world.advance_mini_session(s, closes_at);
    let MiniGamePhase::Closing { starts_at } = world.mini_session(s).phase else {
        return or_abort(Err(format!(
            "expected Closing, got {:?}",
            world.mini_session(s).phase
        )));
    };
    world.advance_mini_session(s, starts_at)
}

/// Advances the Playing session at `s` to its scheduled end tick (read off the
/// phase) and returns the end events.
fn advance_to_end(world: &mut World, s: usize) -> Vec<MiniGameEvent> {
    let MiniGamePhase::Playing { ends_at, .. } = world.mini_session(s).phase else {
        return or_abort(Err(format!(
            "expected Playing, got {:?}",
            world.mini_session(s).phase
        )));
    };
    world.advance_mini_session(s, ends_at)
}

/// Advances the Ended session at `s` to its dispose tick (read off the phase) and
/// returns the dispose events (the alive warp-outs, then `Disposed`).
fn advance_to_dispose(world: &mut World, s: usize) -> Vec<MiniGameEvent> {
    let MiniGamePhase::Ended { disposes_at, .. } = world.mini_session(s).phase else {
        return or_abort(Err(format!(
            "expected Ended, got {:?}",
            world.mini_session(s).phase
        )));
    };
    world.advance_mini_session(s, disposes_at)
}

/// The lifecycle state of wave `number` in the session at `s`.
fn wave_state(world: &World, s: usize, number: WaveNumber) -> WaveState {
    or_abort(
        world
            .mini_session(s)
            .waves
            .waves
            .iter()
            .find(|track| track.number == number)
            .map(|track| track.state)
            .ok_or("no such wave track"),
    )
}

/// The id of the first live monster spawned by wave `number` in the session at
/// `s` — a server-computed instance id the host references when reporting a kill.
fn first_live_of_wave(
    world: &World,
    s: usize,
    number: WaveNumber,
) -> mu_core::data::minigame::SessionMonsterId {
    or_abort(
        world
            .mini_session(s)
            .monsters
            .live
            .iter()
            .find(|instanced| instanced.origin == number)
            .map(|instanced| instanced.id)
            .ok_or("no live monster of that wave"),
    )
}

/// Drives one whole mini-game event over a single seeded stream in one fixed
/// order — enter x3, countdown into Playing (a wave spawns), scored kills, the
/// timeout end, the reward payout, and the dispose warp-out — recording each
/// step by its canonical wire form. Returns the final persisted snapshot and the
/// ordered event trace. The construction seed is the only entropy source (entry
/// landings, wave positions, and the item-drop roll all sample it), so two calls
/// with the same seed reproduce bit-for-bit. The replay twin of [`scripted_run`]
/// for the mini-game framework.
fn scripted_mini_event(seed: u64) -> (String, Vec<TraceStep>) {
    let atlas = paper_host::real_atlas();
    let monster = respawning_wave_monster(&atlas);
    let floor = walkable_area(&atlas, MapNumber(9), 8);
    let waves = vec![spawn_wave(
        1,
        0,
        240_000,
        WaveRespawn::RespawningWhileOpen,
        monster,
        4,
        floor,
    )];
    let rewards = vec![
        reward_entry(
            Some(1),
            Vec::new(),
            RewardKind::Experience { amount: Exp(6000) },
        ),
        reward_entry(Some(2), Vec::new(), RewardKind::Money { amount: Zen(500) }),
        reward_entry(
            None,
            vec![SuccessFlag::Alive],
            RewardKind::ItemDrop {
                group: reward_drop_group(SWORD, ItemLevel::ZERO),
            },
        ),
    ];
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, waves, rewards);
    let mut world = World::with_mini_games(seed, MapNumber(9), vec![definition]);
    let mut trace = Vec::new();

    let entrants = [
        seat_ticketed_entrant(&mut world, 60, 100_000, 2),
        seat_ticketed_entrant(&mut world, 60, 100_000, 2),
        seat_ticketed_entrant(&mut world, 60, 100_000, 2),
    ];
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    for entrant in entrants {
        let outcome = world.enter_mini_session(s, entrant);
        trace.push(TraceStep {
            label: "enter",
            detail: wire(&outcome),
        });
    }

    let start_events = advance_to_playing(&mut world, s);
    trace.push(TraceStep {
        label: "start",
        detail: wire(&start_events),
    });

    let MiniGamePhase::Playing { ends_at, .. } = world.mini_session(s).phase else {
        return (world.snapshot(), trace);
    };
    let kill_tick = Tick(4000);
    let ids = {
        let live = &world.mini_session(s).monsters.live;
        [
            live.first().map(|instanced| instanced.id),
            live.get(1).map(|instanced| instanced.id),
            live.get(2).map(|instanced| instanced.id),
        ]
    };
    let credits = [RosterSlot(1), RosterSlot(1), RosterSlot(0)];
    for (id, credit) in ids.into_iter().zip(credits) {
        if let Some(id) = id {
            world.report_mini_kill(s, id, credit, Score(3), kill_tick);
        }
    }
    trace.push(TraceStep {
        label: "scored",
        detail: wire(&world.mini_session(s).roster),
    });

    let end_events = advance_to_end(&mut world, s);
    trace.push(TraceStep {
        label: "end",
        detail: wire(&end_events),
    });

    let outcome = world.pay_out_mini_rewards(s, ends_at);
    trace.push(TraceStep {
        label: "rewards",
        detail: wire(&outcome.events),
    });

    let dispose_events = advance_to_dispose(&mut world, s);
    trace.push(TraceStep {
        label: "dispose",
        detail: wire(&dispose_events),
    });

    (world.snapshot(), trace)
}

#[test]
fn a_full_mini_game_event_plays_enter_to_payout_through_the_paper_host() {
    // SIM-1: the headline sixth-gate run — a test-authored event over the real
    // square, driven entirely through the framework seams and the persist seam.
    let atlas = paper_host::real_atlas();
    let monster = respawning_wave_monster(&atlas);
    let floor = walkable_area(&atlas, MapNumber(9), 8);
    let waves = vec![spawn_wave(
        1,
        0,
        240_000,
        WaveRespawn::RespawningWhileOpen,
        monster,
        4,
        floor,
    )];
    let rewards = vec![
        reward_entry(
            Some(1),
            Vec::new(),
            RewardKind::Experience { amount: Exp(6000) },
        ),
        reward_entry(Some(2), Vec::new(), RewardKind::Money { amount: Zen(500) }),
        reward_entry(Some(3), Vec::new(), RewardKind::Money { amount: Zen(250) }),
        reward_entry(
            None,
            vec![SuccessFlag::Alive],
            RewardKind::ItemDrop {
                group: reward_drop_group(SWORD, ItemLevel::ZERO),
            },
        ),
    ];
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, waves, rewards);
    let mut world = World::with_mini_games(11, MapNumber(9), vec![definition]);

    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e1 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e2 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    assert_eq!(world.mini_session_count(), 1);

    // Each entrant is admitted; the fee is debited and one ticket charge spent,
    // both surviving the persist round-trip.
    for entrant in [e0, e1, e2] {
        let outcome = world.enter_mini_session(s, entrant);
        assert!(
            matches!(outcome, EnterOutcome::Entered { .. }),
            "entrant {entrant} admitted: {outcome:?}"
        );
        assert_eq!(world.character(entrant).zen().get(), 75_000);
        let bag = world.inventory(entrant);
        let ticket = or_abort(bag.placed().first().ok_or("the ticket survives one entry"));
        assert_eq!(ticket.item.durability.current(), 1);
    }

    // The countdown starts the game and freezes the snapshot at three; the
    // zero-offset wave populates the session's own instanced live-set.
    let start_events = advance_to_playing(&mut world, s);
    assert!(start_events.contains(&MiniGameEvent::GameStarted {
        players: PlayerCount(3)
    }));
    assert!(start_events.contains(&MiniGameEvent::WaveStarted {
        number: WaveNumber(1)
    }));
    assert_eq!(world.mini_session(s).start_snapshot(), Some(PlayerCount(3)));
    assert_eq!(world.mini_session(s).monsters.live.len(), 4);

    // Server-attributed kills credit the crediting seats: slot 1 outscores slot
    // 0, slot 2 stays scoreless.
    let MiniGamePhase::Playing { ends_at, .. } = world.mini_session(s).phase else {
        panic!("expected Playing, got {:?}", world.mini_session(s).phase);
    };
    let kill_tick = Tick(4000);
    let [id0, id1, id2] = {
        let live = &world.mini_session(s).monsters.live;
        [
            or_abort(live.first().ok_or("live 0")).id,
            or_abort(live.get(1).ok_or("live 1")).id,
            or_abort(live.get(2).ok_or("live 2")).id,
        ]
    };
    world.report_mini_kill(s, id0, RosterSlot(1), Score(3), kill_tick);
    world.report_mini_kill(s, id1, RosterSlot(1), Score(3), kill_tick);
    world.report_mini_kill(s, id2, RosterSlot(0), Score(3), kill_tick);
    assert_eq!(
        or_abort(world.mini_session(s).member(RosterSlot(1)).ok_or("slot 1")).score,
        Score(6)
    );
    assert_eq!(
        or_abort(world.mini_session(s).member(RosterSlot(0)).ok_or("slot 0")).score,
        Score(3)
    );

    // The game runs to its duration; the remaining roster are the finishers.
    let end_events = advance_to_end(&mut world, s);
    assert!(end_events.contains(&MiniGameEvent::GameEnded {
        finishers: vec![RosterSlot(0), RosterSlot(1), RosterSlot(2)],
    }));

    // Rewards rank descending by score; every applied grant is persisted onto
    // its finisher through the existing per-character seams.
    let exp1_before = world.character(e1).experience();
    let ground_before = world.ground_item_count();
    let outcome = world.pay_out_mini_rewards(s, ends_at);
    let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
        panic!("the score table rides last: {:?}", outcome.events);
    };
    let ranked: Vec<(u8, u16)> = rows.iter().map(|row| (row.slot.0, row.rank.0)).collect();
    assert_eq!(ranked, vec![(1, 1), (0, 2), (2, 3)]);
    let award1 = or_abort(
        outcome
            .awards
            .iter()
            .find(|award| award.slot == RosterSlot(1))
            .ok_or("rank-1 award"),
    );
    assert!(
        award1
            .grants
            .contains(&GrantDecision::Experience { amount: Exp(6000) }),
        "the rank-1 finisher's award carries the experience grant"
    );
    assert_eq!(world.character(e1).experience().0, exp1_before.0 + 6000);
    assert_eq!(world.character(e0).zen().get(), 75_000 + 500);
    assert_eq!(world.character(e2).zen().get(), 75_000 + 250);
    // Each alive finisher's Alive-gated item drop landed a real ground item.
    assert_eq!(world.ground_item_count(), ground_before + 3);

    // The exit window warps every alive finisher to town and disposes.
    let dispose_events = advance_to_dispose(&mut world, s);
    assert_eq!(
        dispose_events
            .iter()
            .filter(|event| matches!(event, MiniGameEvent::WarpedOut { .. }))
            .count(),
        3
    );
    assert_eq!(dispose_events.last(), Some(&MiniGameEvent::Disposed));
    assert_eq!(world.mini_session(s).phase, MiniGamePhase::Disposed);
    assert!(world.mini_session(s).roster.is_empty());
}

#[test]
fn the_full_mini_game_event_replays_byte_for_byte_under_one_seed() {
    // SIM-2: same seed -> identical final snapshot AND identical ordered event
    // trace. The snapshot proves totality (nothing unpersisted drifts); the
    // trace localises any divergence to the exact step.
    let (snap_a, trace_a) = scripted_mini_event(11);
    let (snap_b, trace_b) = scripted_mini_event(11);
    assert_eq!(snap_a, snap_b, "the whole event replays byte-for-byte");
    assert_eq!(
        trace_a, trace_b,
        "the ordered event trace replays identically"
    );
    assert!(
        trace_a.len() > 4,
        "a substantive multi-step event trace, not an empty log"
    );

    // The seed is load-bearing: a different seed drives a different world — entry
    // landings, wave spawn positions, and the item-drop roll all sample it.
    let (snap_c, _) = scripted_mini_event(29);
    assert_ne!(
        snap_a, snap_c,
        "a different seed yields a different final world"
    );
}

#[test]
fn a_lone_entrants_fee_is_refunded_and_the_event_disposes_without_starting() {
    // SIM-3: the one refund path, end-to-end and AUTHENTIC — the min-player check
    // fires at the entrance close, BEFORE the countdown.
    let atlas = paper_host::real_atlas();
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, Vec::new(), Vec::new());
    let mut world = World::with_mini_games(5, MapNumber(9), vec![definition]);
    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    assert!(matches!(
        world.enter_mini_session(s, e0),
        EnterOutcome::Entered { .. }
    ));
    assert_eq!(world.character(e0).zen().get(), 75_000);

    // Advance to the entrance close: too few alive -> abort, refund, town warp,
    // dispose — no countdown, no game start.
    let MiniGamePhase::Open { closes_at, .. } = world.mini_session(s).phase else {
        panic!("expected Open, got {:?}", world.mini_session(s).phase);
    };
    let events = world.advance_mini_session(s, closes_at);
    assert!(events.iter().any(|event| matches!(
        event,
        MiniGameEvent::MinPlayersAbort {
            present: PlayerCount(1),
            required: PlayerCount(2),
        }
    )));
    // The host applies each FeeRefunded decision by crediting the member.
    for event in &events {
        if let MiniGameEvent::FeeRefunded { slot, amount } = event {
            world.refund_fee(usize::from(slot.0), *amount);
        }
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::WarpedOut { .. }))
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        MiniGameEvent::CountdownStarted { .. } | MiniGameEvent::GameStarted { .. }
    )));
    // The refund restored the fee; the session disposed without starting.
    assert_eq!(world.character(e0).zen().get(), 100_000);
    assert_eq!(world.mini_session(s).phase, MiniGamePhase::Disposed);
    assert!(world.mini_session(s).roster.is_empty());
    assert_eq!(world.mini_session(s).start_snapshot(), None);
}

#[test]
fn a_waived_death_ejected_before_the_end_leaves_the_roster_and_wins_no_grant() {
    // SIM-4: a death inside the event docks nothing (Waived), is ejected via the
    // existing respawn at its LifeState deadline, and — gone before the end —
    // finishes as no finisher and earns no grant (pins 2/3, ruling A).
    let atlas = paper_host::real_atlas();
    let rewards = vec![reward_entry(
        None,
        Vec::new(),
        RewardKind::Money { amount: Zen(1000) },
    )];
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, Vec::new(), rewards);
    let mut world = World::with_mini_games(13, MapNumber(9), vec![definition]);
    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e1 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    for entrant in [e0, e1] {
        assert!(matches!(
            world.enter_mini_session(s, entrant),
            EnterOutcome::Entered { .. }
        ));
    }
    advance_to_playing(&mut world, s);
    let zen1 = world.character(e1).zen().get();
    let exp1 = world.character(e1).experience();

    // The mini-game truce: the SAME resolve_death, penalty Waived — zero dock.
    let deaths = world.resolve_waived_death_of(e1, Tick(3300));
    assert_eq!(
        deaths.len(),
        1,
        "a waived death emits Died alone: {deaths:?}"
    );
    let LifeState::Dead { respawn_at } = world.character(e1).life() else {
        panic!("the victim is marked Dead");
    };
    assert!(
        respawn_at.0 > 3300,
        "the 3 s eject clock is set forward on LifeState, not the roster"
    );
    assert_eq!(world.character(e1).zen().get(), zen1, "no zen dock");
    assert_eq!(world.character(e1).experience(), exp1, "no exp dock");
    world.report_mini_death(s, RosterSlot(1));
    assert_eq!(
        or_abort(world.mini_session(s).member(RosterSlot(1)).ok_or("slot 1")).status,
        RosterStatus::Dead
    );

    // At the LifeState deadline the host composes the existing respawn — reviving
    // and relocating the member off the event map — then reports the exit.
    let respawned = world.respawn_player(e1);
    assert!(
        respawned.is_some(),
        "the dead member is revived by the composed respawn"
    );
    assert_ne!(
        world.character(e1).placement().map,
        MapNumber(9),
        "the ejected member is relocated off the event map"
    );
    world.report_mini_leave(s, RosterSlot(1));

    // The game ends with only the alive member as a finisher; the ejected member
    // gets no grant and keeps exactly its post-entry ledger.
    let end_events = advance_to_end(&mut world, s);
    assert!(end_events.contains(&MiniGameEvent::GameEnded {
        finishers: vec![RosterSlot(0)],
    }));
    let outcome = world.pay_out_mini_rewards(s, Tick(15_000));
    assert!(
        outcome
            .awards
            .iter()
            .all(|award| award.slot != RosterSlot(1)),
        "the ejected member is no finisher"
    );
    assert_eq!(world.character(e1).zen().get(), zen1);
    assert_eq!(world.character(e1).experience(), exp1);
}

#[test]
fn a_death_in_the_final_beat_finishes_dead_flagged_before_the_eject() {
    // DEATH-3: a member dying in the final beat, with the game ending before the
    // 3 s eject, is a Dead-classified FINISHER — eligible only for Dead-gated
    // rewards (ruling A).
    let atlas = paper_host::real_atlas();
    let rewards = vec![
        reward_entry(
            None,
            vec![SuccessFlag::Dead],
            RewardKind::Money { amount: Zen(300) },
        ),
        reward_entry(
            None,
            vec![SuccessFlag::Alive],
            RewardKind::Experience { amount: Exp(6000) },
        ),
    ];
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, Vec::new(), rewards);
    let mut world = World::with_mini_games(17, MapNumber(9), vec![definition]);
    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e1 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    for entrant in [e0, e1] {
        assert!(matches!(
            world.enter_mini_session(s, entrant),
            EnterOutcome::Entered { .. }
        ));
    }
    advance_to_playing(&mut world, s);
    let zen1 = world.character(e1).zen().get();

    // Dying in the final beat; the host does NOT compose the eject (the game ends
    // first), so the member is still present at end, classified Dead.
    let MiniGamePhase::Playing { ends_at, .. } = world.mini_session(s).phase else {
        panic!("expected Playing, got {:?}", world.mini_session(s).phase);
    };
    world.resolve_waived_death_of(e1, Tick(ends_at.0.saturating_sub(10)));
    world.report_mini_death(s, RosterSlot(1));
    let end_events = advance_to_end(&mut world, s);
    assert!(end_events.contains(&MiniGameEvent::GameEnded {
        finishers: vec![RosterSlot(0), RosterSlot(1)],
    }));

    // The dead-but-present finisher takes ONLY the Dead-gated reward; the alive
    // one ONLY the Alive-gated reward — the Dead money credited through persist.
    let outcome = world.pay_out_mini_rewards(s, Tick(15_000));
    let dead_award = or_abort(
        outcome
            .awards
            .iter()
            .find(|award| award.slot == RosterSlot(1))
            .ok_or("dead finisher award"),
    );
    assert_eq!(
        dead_award.grants,
        vec![GrantDecision::Money { amount: Zen(300) }]
    );
    let alive_award = or_abort(
        outcome
            .awards
            .iter()
            .find(|award| award.slot == RosterSlot(0))
            .ok_or("alive finisher award"),
    );
    assert_eq!(
        alive_award.grants,
        vec![GrantDecision::Experience { amount: Exp(6000) }]
    );
    assert_eq!(world.character(e1).zen().get(), zen1 + 300);
}

#[test]
fn an_emptied_roster_ends_the_event_the_instant_it_clears() {
    // DEATH-6: the game ends the instant the entered set empties after start —
    // before the game duration would elapse.
    let atlas = paper_host::real_atlas();
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, Vec::new(), Vec::new());
    let mut world = World::with_mini_games(23, MapNumber(9), vec![definition]);
    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e1 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    for entrant in [e0, e1] {
        assert!(matches!(
            world.enter_mini_session(s, entrant),
            EnterOutcome::Entered { .. }
        ));
    }
    advance_to_playing(&mut world, s);
    let MiniGamePhase::Playing { ends_at, .. } = world.mini_session(s).phase else {
        panic!("expected Playing, got {:?}", world.mini_session(s).phase);
    };

    // Both leave mid-game (fees forfeit — the abort is the only refund path).
    world.report_mini_leave(s, RosterSlot(0));
    world.report_mini_leave(s, RosterSlot(1));
    assert!(world.mini_session(s).roster.is_empty());

    // The next advance ends the game immediately, with time to spare.
    let early = Tick(ends_at.0.saturating_sub(1000));
    let events = world.advance_mini_session(s, early);
    assert!(events.contains(&MiniGameEvent::GameEnded {
        finishers: Vec::new()
    }));
    let MiniGamePhase::Ended { remaining, .. } = world.mini_session(s).phase else {
        panic!("expected Ended, got {:?}", world.mini_session(s).phase);
    };
    assert!(
        remaining.0 > 0,
        "ended early with game time still on the clock"
    );
    assert!(world.mini_session(s).monsters.live.is_empty());
}

#[test]
fn a_declared_winner_ends_the_event_early_and_takes_the_winner_reward() {
    // The framework carries the winner marker + finish_event; a server-computed
    // winner set mid-game ends the game early, and the flag algebra pays the
    // winner and losers apart.
    let atlas = paper_host::real_atlas();
    let rewards = vec![
        reward_entry(
            None,
            vec![SuccessFlag::Winner],
            RewardKind::Money {
                amount: Zen(10_000),
            },
        ),
        reward_entry(
            None,
            vec![SuccessFlag::Loser],
            RewardKind::Money { amount: Zen(300) },
        ),
    ];
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, Vec::new(), rewards);
    let mut world = World::with_mini_games(31, MapNumber(9), vec![definition]);
    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e1 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    for entrant in [e0, e1] {
        assert!(matches!(
            world.enter_mini_session(s, entrant),
            EnterOutcome::Entered { .. }
        ));
    }
    advance_to_playing(&mut world, s);
    let MiniGamePhase::Playing { ends_at, .. } = world.mini_session(s).phase else {
        panic!("expected Playing, got {:?}", world.mini_session(s).phase);
    };

    // The server-computed winner is marked mid-game; the next advance ends early.
    world.finish_mini_event(s, RosterSlot(1));
    assert_eq!(
        world.mini_session(s).winner,
        WinnerStanding::Won { by: RosterSlot(1) }
    );
    let early = Tick(ends_at.0.saturating_sub(500));
    let events = world.advance_mini_session(s, early);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::GameEnded { .. }))
    );

    let outcome = world.pay_out_mini_rewards(s, early);
    let winner_award = or_abort(
        outcome
            .awards
            .iter()
            .find(|award| award.slot == RosterSlot(1))
            .ok_or("winner award"),
    );
    assert_eq!(
        winner_award.grants,
        vec![GrantDecision::Money {
            amount: Zen(10_000)
        }]
    );
    let loser_award = or_abort(
        outcome
            .awards
            .iter()
            .find(|award| award.slot == RosterSlot(0))
            .ok_or("loser award"),
    );
    assert_eq!(
        loser_award.grants,
        vec![GrantDecision::Money { amount: Zen(300) }]
    );
    assert_eq!(world.character(e1).zen().get(), 75_000 + 10_000);
    assert_eq!(world.character(e0).zen().get(), 75_000 + 300);
}

#[test]
fn overlapping_waves_and_wave_scoped_respawn_hold_through_the_persist_seam() {
    // SIM-5 / WAVE gate: overlapping windows run concurrently; a kill inside a
    // window schedules the monster's OWN respawn_ms; the respawn stops the moment
    // its wave closes — all persisted, all reading core-computed deadlines.
    let atlas = paper_host::real_atlas();
    let monster = respawning_wave_monster(&atlas);
    let floor = walkable_area(&atlas, MapNumber(9), 8);
    // Wave 1 over [0, 240 s], wave 2 over [60 s, 270 s] — overlapping between 60
    // and 240 s, both well inside the 5-minute game, both RespawningWhileOpen.
    let waves = vec![
        spawn_wave(
            1,
            0,
            240_000,
            WaveRespawn::RespawningWhileOpen,
            monster,
            2,
            floor,
        ),
        spawn_wave(
            2,
            60_000,
            270_000,
            WaveRespawn::RespawningWhileOpen,
            monster,
            2,
            floor,
        ),
    ];
    let definition = devil_square_definition(&atlas, sim_level(), 2, 3, waves, Vec::new());
    let mut world = World::with_mini_games(41, MapNumber(9), vec![definition]);
    let e0 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let e1 = seat_ticketed_entrant(&mut world, 60, 100_000, 2);
    let s = world.open_mini_session(devil_square_key(sim_level()), Tick(0));
    for entrant in [e0, e1] {
        assert!(matches!(
            world.enter_mini_session(s, entrant),
            EnterOutcome::Entered { .. }
        ));
    }

    // Wave 1 spawns at game start.
    advance_to_playing(&mut world, s);
    assert_eq!(world.mini_session(s).monsters.live.len(), 2);

    // Advance to wave 2's absolute start (read off its Pending track): both waves
    // now populate the live-set at once.
    let WaveState::Pending { starts_at, .. } = wave_state(&world, s, WaveNumber(2)) else {
        panic!("wave 2 is pending before its window");
    };
    world.advance_mini_session(s, starts_at);
    assert_eq!(
        world.mini_session(s).monsters.live.len(),
        4,
        "both windows are live simultaneously"
    );
    assert!(matches!(
        wave_state(&world, s, WaveNumber(1)),
        WaveState::Running { .. }
    ));
    assert!(matches!(
        wave_state(&world, s, WaveNumber(2)),
        WaveState::Running { .. }
    ));

    // A wave-1 kill inside its window schedules the monster's own respawn_ms;
    // advancing to the core-computed due tick re-places it (ids never recycle).
    let WaveState::Running { ends_at: wave1_end } = wave_state(&world, s, WaveNumber(1)) else {
        panic!("wave 1 is running");
    };
    let slain = first_live_of_wave(&world, s, WaveNumber(1));
    world.report_mini_kill(s, slain, RosterSlot(0), Score(1), starts_at);
    assert_eq!(world.mini_session(s).monsters.live.len(), 3);
    let due = or_abort(
        world
            .mini_session(s)
            .waves
            .pending_respawns
            .first()
            .ok_or("a scheduled respawn"),
    )
    .due;
    assert!(
        due.0 < wave1_end.0,
        "the respawn falls due before wave 1 closes"
    );
    let events = world.advance_mini_session(s, due);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. })),
        "the respawn fires inside the open window"
    );
    assert_eq!(world.mini_session(s).monsters.live.len(), 4);
    assert!(world.mini_session(s).waves.pending_respawns.is_empty());

    // A wave-1 kill one tick before its window closes: the respawn is due AFTER
    // the window, so it is dropped when the wave closes — no respawn fires.
    let slain = first_live_of_wave(&world, s, WaveNumber(1));
    world.report_mini_kill(
        s,
        slain,
        RosterSlot(0),
        Score(1),
        Tick(wave1_end.0.saturating_sub(1)),
    );
    let due = or_abort(
        world
            .mini_session(s)
            .waves
            .pending_respawns
            .first()
            .ok_or("a scheduled respawn"),
    )
    .due;
    assert!(
        due.0 > wave1_end.0,
        "the respawn falls due after wave 1 closes"
    );
    let events = world.advance_mini_session(s, due);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. })),
        "no respawn fires after the window has closed"
    );
    assert!(
        world.mini_session(s).waves.pending_respawns.is_empty(),
        "the pending respawn was dropped at the window's close"
    );
    assert!(matches!(
        wave_state(&world, s, WaveNumber(1)),
        WaveState::Closed
    ));
}

// --- W-ACCOUNT standing sixth gate: level-up → unlock → create, through the ---
// --- persist seam. A grind to 220 earns Magic Gladiator for the account; the --
// --- earned-set survives the persist seam; the authoritative gate then flips --
// --- Magic Gladiator to Creatable while Dark Lord stays Locked at 250. --------

#[test]
fn sim_gate_a_grind_to_220_lets_the_account_create_a_magic_gladiator() {
    let mut world = World::new(2024, MapNumber(0));

    // A Dark Knight on the account, seated one level below the Magic Gladiator
    // gate with a consistent mid-band experience read from the real curve.
    let player = world.seat_character(dark_knight_in_band(
        world.atlas(),
        219,
        0,
        MapNumber(0),
        tile(10, 10),
    ));
    assert_eq!(world.character(player).level().get(), 219);

    // Award exactly enough to cross to level 220 — the reached level is the
    // server-decided output of the leveling service, never a client claim.
    let total_for_220 = or_abort(world.atlas().exp_curve().level(220))
        .total_to_hold()
        .0;
    let gained = Exp(total_for_220 - world.character(player).experience().0);
    let growth = world.apply_growth(player, gained);
    let reached = match growth.first() {
        Some(GrowthEvent::LevelsGained { reached, .. }) => *reached,
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("the award crosses into level 220")
        }
    };
    assert_eq!(reached.get(), 220);

    // The host composes the account unlock off the LevelsGained level and the
    // account's (empty) earned-set — one Magic Gladiator unlock is announced.
    let (earned, unlocks) =
        unlock_classes_for_level(UnlockedClasses::empty(), reached, world.atlas().classes());
    assert_eq!(
        unlocks,
        vec![ClassUnlocked {
            class: CharacterClass::MagicGladiator,
        }]
    );

    // The earned-set survives the paper-host persist seam byte-for-byte.
    let before = or_abort(serde_json::to_string(&earned));
    let reloaded = persist(earned);
    assert_eq!(before, or_abort(serde_json::to_string(&reloaded)));

    // The authoritative creation gate reads the reloaded earned-set: Magic
    // Gladiator is now Creatable; Dark Lord stays Locked at its data threshold.
    assert_eq!(
        creation_verdict(
            CharacterClass::MagicGladiator,
            &reloaded,
            world.atlas().classes()
        ),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkLord, &reloaded, world.atlas().classes()),
        CreationVerdict::Locked {
            required: or_abort(Level::new(250))
        }
    );
}

// A single large award can vault past several unlock thresholds at once: the
// real leveling service collapses the multi-level jump into one reached level,
// and the account earns every gated class at or below it in one composed step —
// here Magic Gladiator (220) and Dark Lord (250) together. This exercises the
// real `apply_growth` → multi-threshold-unlock composition the single-gate flow
// above does not, and confirms the creation gate reads only the account's
// earned-set (never the roster), so the unlock cannot be revoked by any later
// change to the earning character (the permanent-unlock pin, 2026-07-19).
#[test]
fn sim_gate_a_multi_level_award_earns_both_gated_classes_in_one_step() {
    let mut world = World::new(2024, MapNumber(0));

    // The account's only character: a Dark Knight one level below the first gate.
    let knight = world.seat_character(dark_knight_in_band(
        world.atlas(),
        219,
        0,
        MapNumber(0),
        tile(10, 10),
    ));

    // One award large enough to vault from 219 past the Dark Lord gate at 250.
    // `apply_growth` returns a single `LevelsGained` carrying the top level
    // reached — the server-decided output, never a client claim.
    let total_for_255 = or_abort(world.atlas().exp_curve().level(255))
        .total_to_hold()
        .0;
    let gained = Exp(total_for_255 - world.character(knight).experience().0);
    let growth = world.apply_growth(knight, gained);
    let reached = match growth.first() {
        Some(GrowthEvent::LevelsGained { reached, .. }) => *reached,
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("the award crosses past level 250")
        }
    };
    assert_eq!(reached.get(), 255);

    // Both gated classes are earned in this one composed step, announced in
    // roster order (Magic Gladiator @220 before Dark Lord @250).
    let (earned, unlocks) =
        unlock_classes_for_level(UnlockedClasses::empty(), reached, world.atlas().classes());
    assert_eq!(
        unlocks,
        vec![
            ClassUnlocked {
                class: CharacterClass::MagicGladiator,
            },
            ClassUnlocked {
                class: CharacterClass::DarkLord,
            },
        ]
    );

    // The earned-set survives the paper-host persist seam, and the gate — a
    // pure function of the earned-set and class data, never of any character —
    // opens both classes on reload.
    let reloaded = persist(earned);
    assert_eq!(
        creation_verdict(
            CharacterClass::MagicGladiator,
            &reloaded,
            world.atlas().classes()
        ),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkLord, &reloaded, world.atlas().classes()),
        CreationVerdict::Creatable
    );
}
