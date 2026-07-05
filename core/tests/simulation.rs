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
use mu_core::components::item_ref::ItemRef;
use mu_core::components::units::{MapNumber, Tick, Zen};
use mu_core::entities::trade_session::TradeSession;

use paper_host::{
    World, dark_knight, footprint_of, item_instance, low_level_monster, monster_instance, persist,
    pos, tile, zen,
};

/// A real 2×2 catalog identity (Dragon Armor) — footprint read from the atlas.
const DRAGON_ARMOR: ItemRef = ItemRef {
    group: 8,
    number: 1,
};

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

    // EXACTLY the five live sets, and nothing else.
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "characters",
            "ground_items",
            "ground_zen",
            "monsters",
            "sessions"
        ],
    );

    // The held Atlas, the rng's internal state, and the fixed map are excluded.
    assert!(!object.contains_key("atlas"));
    assert!(!object.contains_key("rng"));
    assert!(!object.contains_key("map"));

    // Each live set is present and carries its one seated value.
    for key in [
        "characters",
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
