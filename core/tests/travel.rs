//! Travel between maps (W-WARP) over the real `/data` Atlas: the warp command
//! [`resolve_warp`], the menu projection [`warp_menu`], the enter-gate
//! traversal [`traverse_enter_gate`], and the Town Portal Scroll
//! [`use_town_portal`], applied against the shipped warp list, class table,
//! enter gates, and town gates. Proves the authentic check order (discovery →
//! level → wings → zen, the fee charged last and atomically), the class warp fraction
//! (level gate only, never the fee), discovery's lock → walk-in → unlock loop,
//! projection/command agreement from one rule, the scroll's
//! keep-everything town hop with its single-piece consume, and the one-draw
//! determinism discipline.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body
//! so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use rand_core::RngCore;
use serde_json::{Value, json};

use mu_core::components::class::CharacterClass;
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::life::LifeState;
use mu_core::components::movement::{Movement, Wings};
use mu_core::components::placement::Placement;
use mu_core::components::tile::{TileArea, TileCoord};
use mu_core::components::units::{CarriedZen, ItemLevel, Level, MapNumber, Zen};
use mu_core::data::atlas::{Atlas, EnterGateView, Landing, WarpView};
use mu_core::data::classes::WarpRequirement;
use mu_core::data::common::{GateNumber, Provenance, SourceVersion};
use mu_core::data::gates_warps::{Warp, WarpIndex};
use mu_core::entities::character::Character;
use mu_core::events::travel::{
    EnterGateOutcome, TownPortalOutcome, WarpAvailability, WarpLockReason, WarpTravelOutcome,
};
use mu_core::services::death::respawn;
use mu_core::services::travel::{resolve_warp, traverse_enter_gate, use_town_portal, warp_menu};

use dataset::{or_abort, real_atlas};
use rng::TestRng;

/// The Lost Tower entry — level 50, 5,000 zen, the deepest first drop point.
const LOST_TOWER_ENTRY: u16 = 8;
/// The Lorencia entry — level 10, 2,000 zen.
const LORENCIA_ENTRY: u16 = 2;
/// The first Tarkan entry (s6 backport, index 21) — level 140, 8,000 zen,
/// landing on the Tarkan town gate 57.
const TARKAN_ENTRY: u16 = 21;
/// The second Tarkan entry (s6 backport, index 22) — level 140, 8,500 zen,
/// landing on gate 77.
const TARKAN2_ENTRY: u16 = 22;
/// The Icarus entry (s6 backport, index 23) — level 170, 10,000 zen, the one
/// Sky destination in the list.
const ICARUS_ENTRY: u16 = 23;

/// Town Portal Scroll (group 14 number 10, durability 1 — single use).
const TOWN_PORTAL: ItemRef = ItemRef {
    group: 14,
    number: 10,
};
/// A real weapon record (Short Sword 0/3) — a non-consumable at the cell.
const SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};
/// A real recovery consumable (small HP potion) — the WRONG consumable for a
/// town portal.
const HP_SMALL: ItemRef = ItemRef {
    group: 14,
    number: 1,
};

const CELL: Cell = Cell { row: 0, col: 0 };

/// A [`TestRng`] wrapper that counts the random words drawn, so the one-draw
/// arrival discipline is asserted as an exact count, not inferred.
struct CountingRng {
    inner: TestRng,
    words: u32,
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
        self.words += 1;
        self.inner.next_u32()
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.words += 1;
        self.inner.fill_bytes(dst);
    }
}

/// A gearless hero of `class` at `lvl` carrying `zen`, standing on `map` with
/// the given discovered set — built the only way a character can be, by
/// deserialising its wire form (every parse gate re-proves on load).
fn hero(class: &str, lvl: u16, zen: u64, map: u8, discovered: &[u8]) -> Character {
    or_abort(serde_json::from_value(json!({
        "class": class,
        "level": lvl,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": zen,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": map},
        "vitals": {
            "health": {"current": 700, "max": 700},
            "mana": {"current": 200, "max": 200},
            "ability": {"current": 1, "max": 1}
        },
        "discovered": discovered,
    })))
}

/// A hero whose record omits the discovered field entirely — the fresh /
/// legacy shape whose set the parse gate seeds.
fn fresh_hero(class: &str, lvl: u16, map: u8) -> Character {
    let with_set = hero(class, lvl, 0, map, &[map]);
    let mut value = or_abort(serde_json::to_value(&with_set));
    let object = or_abort(value.as_object_mut().ok_or("character is an object"));
    object.remove("discovered");
    or_abort(serde_json::from_value(value))
}

/// The hero re-loaded with one top-level wire field replaced — the serde-only
/// mutation path the suites share (a `Character` has no setters).
fn with_field(character: &Character, field: &str, value: Value) -> Character {
    let mut wire = or_abort(serde_json::to_value(character));
    let object = or_abort(wire.as_object_mut().ok_or("character is an object"));
    object.insert(field.to_owned(), value);
    or_abort(serde_json::from_value(wire))
}

/// The real warp entry at `index`.
fn warp_view(atlas: &Atlas, index: u16) -> WarpView<'_> {
    or_abort(
        atlas
            .warp_by_index(WarpIndex(index))
            .ok_or("the real warp list carries the index"),
    )
}

/// Asserts an arrival placement sits on a walkable tile inside its own
/// landing area on the landing's map.
fn assert_seated(atlas: &Atlas, landing: &Landing, placement: Placement) {
    assert_eq!(placement.map, landing.map);
    assert!(
        landing.area.contains(placement.position),
        "the arrival tile sits inside the target gate area"
    );
    let grid = or_abort(atlas.walk_grid(placement.map).ok_or("map has a walk grid"));
    assert!(
        grid.walkable(placement.position),
        "the arrival tile is walkable"
    );
}

/// A synthetic warp entry (index 99) whose landing is the given tile area on
/// `map` — for cases the real list cannot exercise (an unseatable target, a
/// Sky destination).
fn synthetic_warp(cost: u64, min_level: u16) -> Warp {
    Warp {
        index: WarpIndex(99),
        cost_zen: Zen(cost),
        min_level: or_abort(Level::new(min_level)),
        target_gate: GateNumber(0),
        provenance: Provenance {
            source_version: SourceVersion::V075,
            review: None,
        },
    }
}

fn synthetic_view(warp: &Warp, map: u8, area: (u8, u8, u8, u8)) -> WarpView<'_> {
    WarpView {
        warp,
        landing: Landing {
            map: MapNumber(map),
            area: or_abort(TileArea::new(area.0, area.1, area.2, area.3)).to_world(),
            facing: None,
        },
    }
}

/// The first walkable tile on `map`, scanned row-major from the real grid.
fn first_walkable_tile(atlas: &Atlas, map: u8) -> (u8, u8) {
    let grid = or_abort(atlas.walk_grid(MapNumber(map)).ok_or("map has a walk grid"));
    for y in 0u8..=u8::MAX {
        for x in 0u8..=u8::MAX {
            if grid.walkable(TileCoord::new(x, y).to_world()) {
                return (x, y);
            }
        }
    }
    or_abort(Err::<(u8, u8), &str>("the map has a walkable tile"))
}

/// The enter gate whose trigger covers the given tile.
fn gate_at(atlas: &Atlas, map: u8, x: u8, y: u8) -> EnterGateView<'_> {
    or_abort(
        atlas
            .enter_gate_at(MapNumber(map), TileCoord::new(x, y).to_world())
            .ok_or("an enter gate covers the tile"),
    )
}

/// A real item instance of `id` carrying `pieces` in its gauge.
fn stack(atlas: &Atlas, id: ItemRef, pieces: u8) -> ItemInstance {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    ItemInstance {
        item: id,
        level: ItemLevel::ZERO,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: or_abort(Durability::new(pieces, def.durability)),
        augment: CraftedAugment::None,
    }
}

/// An 8×8 bag holding a `pieces`-strong stack of `id` anchored at [`CELL`].
fn bag_with(atlas: &Atlas, id: ItemRef, pieces: u8) -> Inventory {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    let footprint = or_abort(Footprint::new(def.width, def.height));
    or_abort(
        Inventory::empty(8, 8)
            .place(CELL, footprint, stack(atlas, id, pieces))
            .map_err(|(_, _, reason)| reason),
    )
}

/// The stack count at [`CELL`], or `None` when the cell is empty.
fn pieces_at(inventory: &Inventory) -> Option<u8> {
    inventory
        .occupant(CELL)
        .map(|placed| placed.item.durability.current())
}

fn zen(value: u64) -> CarriedZen {
    or_abort(CarriedZen::new(value))
}

// --- Warp check order: discovery → level → wings → zen, charged last, atomic.

#[test]
fn a_discovered_qualified_solvent_warp_arrives_and_debits_exactly_the_fee() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    assert_eq!(
        entry.landing.map,
        MapNumber(4),
        "the entry lands on Lost Tower"
    );
    assert_eq!(entry.warp.cost_zen, Zen(5_000));
    assert_eq!(entry.warp.min_level.get(), 50);

    let traveler = hero("dark_knight", 60, 10_000, 0, &[0, 4]);
    let (arrived, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_seated(&atlas, &entry.landing, placement);
            assert_eq!(balance, zen(5_000), "debited by exactly the fee");
            assert_eq!(arrived.zen(), balance);
            assert_eq!(arrived.placement(), placement);
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => panic!("a qualified warp arrives: {outcome:?}"),
    }
    // The idempotent discovery insert: Lost Tower was already a member.
    assert!(arrived.discovered().contains(MapNumber(4)));
    assert!(arrived.discovered().contains(MapNumber(0)));
}

#[test]
fn an_undiscovered_target_is_refused_not_discovered_with_the_wallet_untouched() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let traveler = hero("dark_knight", 60, 10_000, 0, &[0]);

    let (unchanged, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    assert_eq!(outcome, WarpTravelOutcome::NotDiscovered);
    assert_eq!(unchanged.zen(), zen(10_000), "nothing charged");
    assert_eq!(unchanged.placement(), traveler.placement(), "nothing moved");
    // A refused warp is not an arrival: the set is unchanged.
    assert!(!unchanged.discovered().contains(MapNumber(4)));
}

#[test]
fn an_under_level_warp_is_refused_before_zen_even_when_also_too_poor() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    // Below the level bar AND below the fee: level short-circuits first, so
    // the reject names the bar and the wallet is never touched.
    let traveler = hero("dark_knight", 40, 3_000, 0, &[0, 4]);

    let (unchanged, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    assert_eq!(outcome, WarpTravelOutcome::LevelTooLow { required: 50 });
    assert_eq!(unchanged.zen(), zen(3_000));
}

#[test]
fn an_unaffordable_warp_is_refused_atomically() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let traveler = hero("dark_knight", 60, 4_999, 0, &[0, 4]);

    let (unchanged, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    assert_eq!(
        outcome,
        WarpTravelOutcome::NotEnoughZen {
            required: Zen(5_000),
            available: zen(4_999),
        }
    );
    assert_eq!(unchanged.zen(), zen(4_999), "never partially spent");
    assert_eq!(unchanged.placement(), traveler.placement());
}

#[test]
fn a_wallet_exactly_equal_to_the_fee_warps_and_lands_at_zero() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let traveler = hero("dark_knight", 60, 5_000, 0, &[0, 4]);

    let (arrived, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        WarpTravelOutcome::Arrived { balance, .. } => {
            assert_eq!(balance, zen(0), "docked to zero, never below");
            assert_eq!(arrived.zen(), zen(0));
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => {
            panic!("the affordability edge is inclusive: {outcome:?}")
        }
    }
}

#[test]
fn a_dead_character_is_refused_not_alive_before_any_other_check() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let traveler = with_field(
        &hero("dark_knight", 60, 10_000, 0, &[0, 4]),
        "life",
        json!({"kind": "dead", "respawn_at": 903}),
    );

    let (unchanged, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    assert_eq!(outcome, WarpTravelOutcome::NotAlive);
    assert_eq!(
        or_abort(serde_json::to_string(&unchanged)),
        or_abort(serde_json::to_string(&traveler)),
        "the input is returned byte-identical"
    );
}

// --- The class warp fraction: level gate only, never the fee. ----------------

#[test]
fn the_real_class_table_pins_the_fraction_premise() {
    let atlas = real_atlas();
    // MG and DL pay 2/3 of the level requirement; every other class the full.
    for class in [CharacterClass::MagicGladiator, CharacterClass::DarkLord] {
        match atlas.classes().record(class).warp_requirement {
            WarpRequirement::Fraction {
                numerator,
                denominator,
            } => {
                assert_eq!(numerator.get(), 2, "{class:?}");
                assert_eq!(denominator.get(), 3, "{class:?}");
            }
            WarpRequirement::Full => panic!("{class:?} carries the 2/3 fraction"),
        }
    }
    assert_eq!(
        atlas
            .classes()
            .record(CharacterClass::DarkKnight)
            .warp_requirement,
        WarpRequirement::Full
    );
}

#[test]
fn a_magic_gladiator_clears_a_gate_its_plain_level_misses_and_pays_the_full_fee() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);

    // MG at 40: effective requirement floor(50 * 2/3) = 33 <= 40 — the gate
    // opens, and the fee is the full 5,000, never a fraction of it.
    let gladiator = hero("magic_gladiator", 40, 10_000, 0, &[0, 4]);
    let (arrived, outcome) =
        resolve_warp(&gladiator, entry, &atlas, Wings::None, &mut TestRng::new(7));
    match outcome {
        WarpTravelOutcome::Arrived { balance, .. } => {
            assert_eq!(balance, zen(5_000), "the FULL fee, not a fraction");
            assert_eq!(arrived.zen(), zen(5_000));
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => {
            panic!("the MG's effective 33 opens the gate: {outcome:?}")
        }
    }

    // The same level, the same entry, a Full class: the posted 50 applies
    // unreduced and the gate refuses.
    let knight = hero("dark_knight", 40, 10_000, 0, &[0, 4]);
    let (_, refused) = resolve_warp(&knight, entry, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(refused, WarpTravelOutcome::LevelTooLow { required: 50 });
}

#[test]
fn the_level_too_low_reject_carries_the_class_effective_requirement() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let gladiator = hero("magic_gladiator", 30, 10_000, 0, &[0, 4]);

    let (_, outcome) = resolve_warp(&gladiator, entry, &atlas, Wings::None, &mut TestRng::new(7));

    // floor(50 * 2/3) = 33 — the bar THIS character faces, not the posted 50.
    assert_eq!(outcome, WarpTravelOutcome::LevelTooLow { required: 33 });
}

#[test]
fn every_real_entry_warps_a_fully_qualified_hero_debiting_its_real_fee() {
    let atlas = real_atlas();
    let entries: Vec<(WarpIndex, Zen, MapNumber)> = atlas
        .warps()
        .map(|view| (view.warp.index, view.warp.cost_zen, view.landing.map))
        .collect();
    assert_eq!(entries.len(), 16, "the surviving warp list");

    for (index, cost, map) in entries {
        let traveler = hero("dark_knight", 400, 100_000, 0, &[0, 1, 2, 3, 4, 8, 10]);
        let entry = warp_view(&atlas, index.0);
        let (arrived, outcome) = resolve_warp(
            &traveler,
            entry,
            &atlas,
            Wings::Equipped,
            &mut TestRng::new(11),
        );
        match outcome {
            WarpTravelOutcome::Arrived { placement, balance } => {
                assert_seated(&atlas, &entry.landing, placement);
                assert_eq!(placement.map, map);
                assert_eq!(balance, zen(100_000 - cost.0), "warp {index:?} fee");
                assert_eq!(arrived.zen(), balance);
            }
            WarpTravelOutcome::NotAlive
            | WarpTravelOutcome::NotDiscovered
            | WarpTravelOutcome::LevelTooLow { .. }
            | WarpTravelOutcome::CannotFly
            | WarpTravelOutcome::NotEnoughZen { .. }
            | WarpTravelOutcome::NoWalkableLanding => {
                panic!("warp {index:?} arrives for a level-400 funded hero: {outcome:?}")
            }
        }
    }
}

// --- Arrival landing: the shared resolve_arrival tail. -----------------------

#[test]
fn arrival_forces_the_destination_environment_movement_mode() {
    let atlas = real_atlas();

    // A synthetic entry landing on Icarus (map 10, Sky): a grounded traveler
    // arrives FLYING — the destination environment forces the mode. Winged,
    // so the Sky entry gate passes.
    let (x, y) = first_walkable_tile(&atlas, 10);
    let record = synthetic_warp(1_000, 1);
    let sky = synthetic_view(&record, 10, (x, y, x, y));
    let grounded = hero("dark_knight", 60, 10_000, 0, &[0, 10]);
    let (_, outcome) = resolve_warp(
        &grounded,
        sky,
        &atlas,
        Wings::Equipped,
        &mut TestRng::new(7),
    );
    match outcome {
        WarpTravelOutcome::Arrived { placement, .. } => {
            assert_eq!(placement.map, MapNumber(10));
            assert_eq!(placement.movement, Movement::Flying, "Sky forces flight");
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => {
            panic!("the Icarus tile is walkable: {outcome:?}")
        }
    }

    // The reverse: a flying traveler warping to a Ground target stands up.
    let flyer = with_field(
        &hero("dark_knight", 60, 10_000, 10, &[10, 4]),
        "placement",
        json!({"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "flying", "map": 10}),
    );
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let (_, outcome) = resolve_warp(&flyer, entry, &atlas, Wings::None, &mut TestRng::new(7));
    match outcome {
        WarpTravelOutcome::Arrived { placement, .. } => {
            assert_eq!(placement.movement, Movement::Grounded, "Ground grounds");
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => panic!("a qualified warp arrives: {outcome:?}"),
    }
}

#[test]
fn a_same_map_warp_is_allowed_and_charges_the_fee() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LORENCIA_ENTRY);
    assert_eq!(
        entry.landing.map,
        MapNumber(0),
        "the entry lands on Lorencia"
    );
    // Standing in Lorencia, warping to Lorencia: discovery always passes (the
    // current map is always a member), the fee is charged, the hero re-places.
    let traveler = hero("dark_knight", 60, 10_000, 0, &[0]);

    let (arrived, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_seated(&atlas, &entry.landing, placement);
            assert_eq!(balance, zen(8_000), "the fee was charged");
            assert_eq!(arrived.placement().map, MapNumber(0));
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => panic!("a same-map warp arrives: {outcome:?}"),
    }
}

#[test]
fn an_unseatable_target_folds_to_no_walkable_landing_with_the_wallet_untouched() {
    let atlas = real_atlas();
    let traveler = hero("dark_knight", 60, 10_000, 0, &[0]);

    // A real map, a degenerate landing: Lorencia's (0,0) corner is a NoMove
    // wall, so the one-tile area holds no walkable tile. No real warp target
    // is unwalkable — the fold is structural, exercised synthetically.
    let record = synthetic_warp(2_000, 1);
    let walled = synthetic_view(&record, 0, (0, 0, 0, 0));
    let mut rng = CountingRng::new(7);
    let (unchanged, outcome) = resolve_warp(&traveler, walled, &atlas, Wings::None, &mut rng);
    assert_eq!(outcome, WarpTravelOutcome::NoWalkableLanding);
    assert_eq!(unchanged.zen(), zen(10_000), "prove-then-commit: no charge");
    assert_eq!(unchanged.placement(), traveler.placement());
    assert_eq!(rng.words, 0, "an empty landing draws nothing");

    // A landing map the atlas does not carry folds to the same answer.
    let traveler = hero("dark_knight", 60, 10_000, 0, &[0]);
    let ghost_discovered = with_field(&traveler, "discovered", json!([0, 200]));
    let off_atlas = synthetic_view(&record, 200, (10, 10, 20, 20));
    let (unchanged, outcome) = resolve_warp(
        &ghost_discovered,
        off_atlas,
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    assert_eq!(outcome, WarpTravelOutcome::NoWalkableLanding);
    assert_eq!(unchanged.zen(), zen(10_000));
}

#[test]
fn buffs_and_vitals_cross_a_warp_untouched() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let buffed = with_field(
        &with_field(
            &hero("dark_knight", 60, 10_000, 0, &[0, 4]),
            "active_effects",
            json!([{"kind": "defense", "expiry": 900}]),
        ),
        "vitals",
        json!({
            "health": {"current": 350, "max": 700},
            "mana": {"current": 200, "max": 200},
            "ability": {"current": 1, "max": 1}
        }),
    );

    let (arrived, outcome) =
        resolve_warp(&buffed, entry, &atlas, Wings::None, &mut TestRng::new(7));

    assert!(matches!(outcome, WarpTravelOutcome::Arrived { .. }));
    assert_eq!(
        arrived.active_effects(),
        buffed.active_effects(),
        "no cooldown, no clear — the buff rides across"
    );
    assert_eq!(arrived.vitals(), buffed.vitals(), "no penalty, no refill");
    assert_eq!(arrived.level(), buffed.level());
    assert_eq!(arrived.experience(), buffed.experience());
    assert_eq!(arrived.life(), LifeState::Alive);
}

// --- Discovery: arrival is arrival; the menu is gated, gates are not. --------

#[test]
fn a_fresh_character_discovers_exactly_its_home_map() {
    let elf = fresh_hero("fairy_elf", 10, 3);
    assert!(elf.discovered().contains(MapNumber(3)), "Noria, its home");
    assert_eq!(elf.discovered().iter().count(), 1);

    let knight = fresh_hero("dark_knight", 10, 0);
    assert!(knight.discovered().contains(MapNumber(0)), "Lorencia");
    assert_eq!(knight.discovered().iter().count(), 1);
}

#[test]
fn walking_an_enter_gate_is_never_blocked_by_discovery_and_discovers_the_landing() {
    let atlas = real_atlas();
    // The Lorencia → Devias door (gate 18, trigger at (5,38), min level 15):
    // Devias is NOT in the set, and the door does not care.
    let gate = gate_at(&atlas, 0, 5, 38);
    assert_eq!(gate.landing.map, MapNumber(2), "the door lands on Devias");
    let traveler = hero("dark_knight", 60, 0, 0, &[0]);

    let (crossed, outcome) =
        traverse_enter_gate(&traveler, gate, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        EnterGateOutcome::Arrived { placement } => {
            assert_seated(&atlas, &gate.landing, placement);
            assert_eq!(crossed.placement(), placement);
        }
        EnterGateOutcome::NotAlive
        | EnterGateOutcome::LevelTooLow { .. }
        | EnterGateOutcome::CannotFly
        | EnterGateOutcome::NoWalkableLanding => {
            panic!("discovery never blocks a physical door: {outcome:?}")
        }
    }
    assert!(crossed.discovered().contains(MapNumber(2)), "discovered");
    assert!(crossed.discovered().contains(MapNumber(0)), "kept");
}

#[test]
fn an_enter_gate_is_gated_by_its_classic_level_rule_with_the_class_fraction() {
    let atlas = real_atlas();
    let gate = gate_at(&atlas, 0, 5, 38);

    // A level-10 knight misses the door's posted 15.
    let low_knight = hero("dark_knight", 10, 0, 0, &[0]);
    let (unchanged, outcome) =
        traverse_enter_gate(&low_knight, gate, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(outcome, EnterGateOutcome::LevelTooLow { required: 15 });
    assert_eq!(unchanged.placement(), low_knight.placement());

    // The same level-10 as a Magic Gladiator: effective floor(15 * 2/3) = 10,
    // and the door opens — the fraction applies to enter gates too.
    let low_gladiator = hero("magic_gladiator", 10, 0, 0, &[0]);
    let (_, outcome) = traverse_enter_gate(
        &low_gladiator,
        gate,
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));

    // A dead traveler cannot use a door at all.
    let dead = with_field(
        &hero("dark_knight", 60, 0, 0, &[0]),
        "life",
        json!({"kind": "dead", "respawn_at": 903}),
    );
    let (_, outcome) = traverse_enter_gate(&dead, gate, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(outcome, EnterGateOutcome::NotAlive);
}

#[test]
fn the_lock_walk_in_unlock_loop_over_real_data() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    // A qualified, funded hero who has never been to Lost Tower.
    let traveler = hero("dark_knight", 60, 20_000, 0, &[0]);

    // LOCKED: the menu names discovery as the ONLY unmet requirement, and the
    // command agrees.
    let menu = warp_menu(&traveler, &atlas, Wings::None);
    let status = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LOST_TOWER_ENTRY))
            .ok_or("the menu lists the Lost Tower entry"),
    );
    match &status.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(listed, vec![WarpLockReason::NotDiscovered]);
        }
        WarpAvailability::Available => panic!("an unvisited map is locked"),
    }
    let (_, refused) = resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(refused, WarpTravelOutcome::NotDiscovered);

    // WALK IN: Lorencia → Devias (gate 18), then Devias → Lost Tower (gate 28
    // at (2,248), min level 40) — physical doors, gated by level alone.
    let (on_devias, outcome) = traverse_enter_gate(
        &traveler,
        gate_at(&atlas, 0, 5, 38),
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));
    let tower_door = gate_at(&atlas, 2, 2, 248);
    assert_eq!(tower_door.landing.map, MapNumber(4));
    let (on_tower, outcome) = traverse_enter_gate(
        &on_devias,
        tower_door,
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));
    assert!(on_tower.discovered().contains(MapNumber(4)));

    // UNLOCKED: the menu flips and the same entry now warps, fee debited.
    let menu = warp_menu(&on_tower, &atlas, Wings::None);
    let status = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LOST_TOWER_ENTRY))
            .ok_or("the menu lists the Lost Tower entry"),
    );
    assert!(matches!(status.availability, WarpAvailability::Available));
    let (warped, outcome) =
        resolve_warp(&on_tower, entry, &atlas, Wings::None, &mut TestRng::new(7));
    match outcome {
        WarpTravelOutcome::Arrived { balance, .. } => {
            assert_eq!(balance, zen(15_000), "the 5,000 fee came off the wallet");
            assert_eq!(warped.placement().map, MapNumber(4));
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => panic!("the unlocked entry warps: {outcome:?}"),
    }
}

#[test]
fn a_cross_map_respawn_discovers_the_town() {
    let atlas = real_atlas();
    // Die on Devil Square (map 9), whose town is Noria (map 3): waking up in
    // the unfamiliar town discovers it — the death penalty was the fare.
    let dead = with_field(
        &hero("dark_knight", 100, 500_000, 9, &[9]),
        "life",
        json!({"kind": "dead", "respawn_at": 1}),
    );

    let (revived, _respawned) = respawn(&dead, &atlas, &mut TestRng::new(7));

    assert_eq!(revived.placement().map, MapNumber(3));
    assert!(revived.discovered().contains(MapNumber(3)), "Noria gained");
    assert!(revived.discovered().contains(MapNumber(9)), "origin kept");
}

// --- Menu availability projection: one rule, all failures. -------------------

#[test]
fn the_projection_annotates_all_sixteen_entries_in_warp_index_order() {
    let atlas = real_atlas();
    // Discovered everywhere (all seven warp destinations), level 400 over
    // every bar including Icarus's 170, funded past the priciest fee, winged
    // for the one Sky entry — all sixteen open.
    let veteran = hero("dark_knight", 400, 200_000, 0, &[0, 1, 2, 3, 4, 8, 10]);

    let menu = warp_menu(&veteran, &atlas, Wings::Equipped);

    let expected: Vec<WarpIndex> = atlas.warps().map(|view| view.warp.index).collect();
    assert_eq!(expected.len(), 16);
    assert_eq!(
        expected,
        [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 21, 22, 23].map(WarpIndex),
        "the surviving indices: 2-14 plus the s6-backported 21-23"
    );
    assert_eq!(
        menu.iter().map(|status| status.index).collect::<Vec<_>>(),
        expected,
        "one status per entry, in warp-index order"
    );
    for status in &menu {
        assert!(
            matches!(status.availability, WarpAvailability::Available),
            "every entry is open to a discovered, leveled, funded, winged veteran: {status:?}"
        );
    }
}

#[test]
fn a_locked_entry_carries_the_complete_reason_set_in_check_order() {
    let atlas = real_atlas();
    let novice = hero("dark_knight", 15, 1_000, 0, &[0]);

    let menu = warp_menu(&novice, &atlas, Wings::None);

    // Lost Tower: undiscovered, under-leveled, unaffordable — all three, in
    // check order.
    let tower = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LOST_TOWER_ENTRY))
            .ok_or("the menu lists the Lost Tower entry"),
    );
    match &tower.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(
                listed,
                vec![
                    WarpLockReason::NotDiscovered,
                    WarpLockReason::LevelTooLow { required: 50 },
                    WarpLockReason::InsufficientZen { cost: Zen(5_000) },
                ]
            );
        }
        WarpAvailability::Available => panic!("a triply-failing entry is locked"),
    }

    // Lorencia: discovered and level-15 >= 10 — only the fee is unmet.
    let home = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LORENCIA_ENTRY))
            .ok_or("the menu lists the Lorencia entry"),
    );
    match &home.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(
                listed,
                vec![WarpLockReason::InsufficientZen { cost: Zen(2_000) }]
            );
        }
        WarpAvailability::Available => panic!("an unaffordable entry is locked"),
    }
}

#[test]
fn projection_and_command_agree_for_a_sweep_of_characters() {
    let atlas = real_atlas();
    let sweep = [
        (hero("dark_knight", 15, 1_000, 0, &[0]), Wings::None),
        (hero("dark_knight", 60, 10_000, 0, &[0, 4]), Wings::None),
        (
            hero("magic_gladiator", 40, 4_000, 0, &[0, 2, 4]),
            Wings::None,
        ),
        (
            hero("dark_knight", 400, 100_000, 0, &[0, 1, 2, 3, 4, 8, 10]),
            Wings::Equipped,
        ),
        (hero("fairy_elf", 45, 3_000, 3, &[3, 0]), Wings::None),
        // Qualified for Icarus in every axis but wings: the Sky entry locks
        // on CannotFly alone, and the command agrees.
        (hero("fairy_elf", 200, 50_000, 0, &[0, 10]), Wings::None),
    ];

    for (traveler, wings) in &sweep {
        let menu = warp_menu(traveler, &atlas, *wings);
        for status in &menu {
            let entry = warp_view(&atlas, status.index.0);
            let (_, outcome) = resolve_warp(traveler, entry, &atlas, *wings, &mut TestRng::new(3));
            match &status.availability {
                WarpAvailability::Available => {
                    // Real /data has a walkable landing for every entry, so an
                    // Available projection always executes.
                    assert!(
                        matches!(outcome, WarpTravelOutcome::Arrived { .. }),
                        "entry {:?} projected Available must arrive: {outcome:?}",
                        status.index
                    );
                }
                WarpAvailability::Locked { reasons } => {
                    // The command's first-failure reason is a member of the
                    // projected lock set — one rule, two shapes.
                    let member = match outcome {
                        WarpTravelOutcome::NotDiscovered => {
                            reasons.iter().any(|r| *r == WarpLockReason::NotDiscovered)
                        }
                        WarpTravelOutcome::LevelTooLow { required } => reasons
                            .iter()
                            .any(|r| *r == WarpLockReason::LevelTooLow { required }),
                        WarpTravelOutcome::CannotFly => {
                            reasons.iter().any(|r| *r == WarpLockReason::CannotFly)
                        }
                        WarpTravelOutcome::NotEnoughZen { required, .. } => reasons
                            .iter()
                            .any(|r| *r == WarpLockReason::InsufficientZen { cost: required }),
                        WarpTravelOutcome::Arrived { .. }
                        | WarpTravelOutcome::NotAlive
                        | WarpTravelOutcome::NoWalkableLanding => false,
                    };
                    assert!(
                        member,
                        "entry {:?}: the command's reject {outcome:?} is in the lock set {reasons:?}",
                        status.index
                    );
                }
            }
        }
    }
}

#[test]
fn the_class_effective_requirement_is_identical_in_projection_and_command() {
    let atlas = real_atlas();
    let gladiator = hero("magic_gladiator", 30, 10_000, 0, &[0, 4]);

    let menu = warp_menu(&gladiator, &atlas, Wings::None);
    let tower = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LOST_TOWER_ENTRY))
            .ok_or("the menu lists the Lost Tower entry"),
    );
    match &tower.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(listed, vec![WarpLockReason::LevelTooLow { required: 33 }]);
        }
        WarpAvailability::Available => panic!("level 30 misses the effective 33"),
    }
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let (_, outcome) = resolve_warp(&gladiator, entry, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(outcome, WarpTravelOutcome::LevelTooLow { required: 33 });

    // A level-40 MG sees the same entry Available — the projection carries
    // the fraction exactly as the command applies it.
    let older = hero("magic_gladiator", 40, 10_000, 0, &[0, 4]);
    let menu = warp_menu(&older, &atlas, Wings::None);
    let tower = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LOST_TOWER_ENTRY))
            .ok_or("the menu lists the Lost Tower entry"),
    );
    assert!(matches!(tower.availability, WarpAvailability::Available));
}

#[test]
fn a_stale_menu_cannot_buy_a_warp_the_character_no_longer_qualifies_for() {
    let atlas = real_atlas();
    let funded = hero("dark_knight", 60, 10_000, 0, &[0, 4]);

    // The snapshot says Available...
    let menu = warp_menu(&funded, &atlas, Wings::None);
    let tower = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(LOST_TOWER_ENTRY))
            .ok_or("the menu lists the Lost Tower entry"),
    );
    assert!(matches!(tower.availability, WarpAvailability::Available));

    // ...the wallet is spent down elsewhere, and the command re-evaluates
    // live: the stale menu buys nothing.
    let spent = with_field(&funded, "zen", json!(4_000));
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let (unchanged, outcome) =
        resolve_warp(&spent, entry, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(
        outcome,
        WarpTravelOutcome::NotEnoughZen {
            required: Zen(5_000),
            available: zen(4_000),
        }
    );
    assert_eq!(unchanged.zen(), zen(4_000));
}

// --- Town Portal Scroll: travel service, single charge, keeps everything. ----

#[test]
fn a_field_scroll_lands_in_the_towns_gate_alive_with_vitals_and_buffs_intact() {
    let atlas = real_atlas();
    // Deep in the Dungeon (map 1), hurt and buffed: the scroll burns away and
    // the hero is home in Lorencia (Dungeon's town), everything kept.
    let traveler = with_field(
        &with_field(
            &hero("dark_knight", 60, 10_000, 1, &[1]),
            "active_effects",
            json!([{"kind": "defense", "expiry": 900}]),
        ),
        "vitals",
        json!({
            "health": {"current": 350, "max": 700},
            "mana": {"current": 200, "max": 200},
            "ability": {"current": 1, "max": 1}
        }),
    );
    let bag = bag_with(&atlas, TOWN_PORTAL, 1);

    let (home, bag, outcome) = use_town_portal(&traveler, bag, CELL, &atlas, &mut TestRng::new(7));

    match outcome {
        TownPortalOutcome::Arrived { placement } => {
            assert_eq!(placement.map, MapNumber(0), "Lorencia, Dungeon's town");
            let (gate, _env) = or_abort(
                atlas
                    .town_gate_for_map(MapNumber(1))
                    .ok_or("Dungeon resolves a town gate"),
            );
            assert!(
                gate.landing.iter().any(|&tile| tile == placement.position),
                "seated on the town gate's retained walkable set"
            );
            assert_eq!(home.placement(), placement);
        }
        TownPortalOutcome::NotAlive | TownPortalOutcome::NoScroll => {
            panic!("a held scroll teleports: {outcome:?}")
        }
    }
    assert_eq!(home.life(), LifeState::Alive);
    assert_eq!(home.vitals(), traveler.vitals(), "no refill, no penalty");
    assert_eq!(
        home.active_effects(),
        traveler.active_effects(),
        "the buff rides home"
    );
    assert_eq!(home.zen(), traveler.zen(), "a scroll is not a fee");
    assert!(home.discovered().contains(MapNumber(0)), "town discovered");
    assert!(home.discovered().contains(MapNumber(1)), "origin kept");
    assert_eq!(pieces_at(&bag), None, "the single scroll left the bag");
}

#[test]
fn an_in_town_scroll_re_seats_at_the_towns_own_gate() {
    let atlas = real_atlas();
    let townsfolk = hero("dark_knight", 60, 0, 0, &[0]);
    let bag = bag_with(&atlas, TOWN_PORTAL, 1);

    let (reseated, bag, outcome) =
        use_town_portal(&townsfolk, bag, CELL, &atlas, &mut TestRng::new(7));

    match outcome {
        TownPortalOutcome::Arrived { placement } => {
            assert_eq!(placement.map, MapNumber(0), "a town is its own town");
        }
        TownPortalOutcome::NotAlive | TownPortalOutcome::NoScroll => {
            panic!("an in-town read re-seats: {outcome:?}")
        }
    }
    assert_eq!(
        reseated.discovered(),
        townsfolk.discovered(),
        "nothing new to discover"
    );
    assert_eq!(pieces_at(&bag), None, "one scroll consumed all the same");
}

#[test]
fn a_sky_scroll_lands_grounded_in_lost_tower_and_discovers_it() {
    let atlas = real_atlas();
    // Flying over Icarus (map 10, Sky), whose town table points at Lost Tower
    // (map 4, Ground): the flyer comes down to earth. Also the WF-TP-1
    // regression pin: use_town_portal takes NO wings fact and checks no wings
    // gate — leaving Icarus is authentically ungated, and the hero here is
    // gearless (wingless) by construction.
    let flyer = with_field(
        &hero("fairy_elf", 90, 0, 10, &[10]),
        "placement",
        json!({"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "flying", "map": 10}),
    );
    let bag = bag_with(&atlas, TOWN_PORTAL, 1);

    let (landed, bag, outcome) = use_town_portal(&flyer, bag, CELL, &atlas, &mut TestRng::new(7));

    match outcome {
        TownPortalOutcome::Arrived { placement } => {
            assert_eq!(placement.map, MapNumber(4), "Lost Tower, Icarus's town");
            assert_eq!(
                placement.movement,
                Movement::Grounded,
                "the destination environment drops the traveler from flight"
            );
        }
        TownPortalOutcome::NotAlive | TownPortalOutcome::NoScroll => {
            panic!("a held scroll teleports: {outcome:?}")
        }
    }
    assert_eq!(landed.life(), LifeState::Alive);
    assert_eq!(landed.vitals(), flyer.vitals());
    assert!(landed.discovered().contains(MapNumber(4)), "discovered");
    assert!(landed.discovered().contains(MapNumber(10)), "kept");
    assert_eq!(pieces_at(&bag), None, "one scroll consumed");
}

#[test]
fn a_dead_characters_scroll_does_nothing_and_is_not_consumed() {
    let atlas = real_atlas();
    let dead = with_field(
        &hero("dark_knight", 60, 0, 1, &[1]),
        "life",
        json!({"kind": "dead", "respawn_at": 903}),
    );
    let bag = bag_with(&atlas, TOWN_PORTAL, 1);

    let (unchanged, bag, outcome) = use_town_portal(&dead, bag, CELL, &atlas, &mut TestRng::new(7));

    assert_eq!(outcome, TownPortalOutcome::NotAlive);
    assert_eq!(
        or_abort(serde_json::to_string(&unchanged)),
        or_abort(serde_json::to_string(&dead)),
        "the input is returned byte-identical"
    );
    assert_eq!(pieces_at(&bag), Some(1), "the scroll stack is whole");
}

#[test]
fn an_empty_cell_or_wrong_item_is_refused_no_scroll_consuming_nothing() {
    let atlas = real_atlas();
    let traveler = hero("dark_knight", 60, 0, 1, &[1]);

    // An empty cell.
    let (unchanged, bag, outcome) = use_town_portal(
        &traveler,
        Inventory::empty(8, 8),
        CELL,
        &atlas,
        &mut TestRng::new(7),
    );
    assert_eq!(outcome, TownPortalOutcome::NoScroll);
    assert_eq!(unchanged.placement(), traveler.placement());
    assert!(bag.placed().is_empty());

    // A non-consumable at the cell.
    let (_, bag, outcome) = use_town_portal(
        &traveler,
        bag_with(&atlas, SWORD, 1),
        CELL,
        &atlas,
        &mut TestRng::new(7),
    );
    assert_eq!(outcome, TownPortalOutcome::NoScroll);
    assert!(bag.occupant(CELL).is_some(), "the sword rides back whole");

    // The WRONG consumable — a potion is not a scroll.
    let (_, bag, outcome) = use_town_portal(
        &traveler,
        bag_with(&atlas, HP_SMALL, 3),
        CELL,
        &atlas,
        &mut TestRng::new(7),
    );
    assert_eq!(outcome, TownPortalOutcome::NoScroll);
    assert_eq!(pieces_at(&bag), Some(3), "nothing consumed");
}

#[test]
fn the_town_gate_arrival_is_total_for_every_real_map_and_the_fallback() {
    let atlas = real_atlas();
    // Every one of the 11 real maps — and an off-roster map that takes the
    // Lorencia fallback — seats the scroll arrival; no landing failure is
    // representable in the outcome.
    for map in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 200] {
        let traveler = hero("dark_knight", 60, 0, map, &[map]);
        let bag = bag_with(&atlas, TOWN_PORTAL, 1);
        let (landed, _, outcome) =
            use_town_portal(&traveler, bag, CELL, &atlas, &mut TestRng::new(map.into()));
        match outcome {
            TownPortalOutcome::Arrived { placement } => {
                let grid = or_abort(
                    atlas
                        .walk_grid(placement.map)
                        .ok_or("the town map has a walk grid"),
                );
                assert!(grid.walkable(placement.position), "map {map}");
                assert_eq!(landed.placement(), placement);
            }
            TownPortalOutcome::NotAlive | TownPortalOutcome::NoScroll => {
                panic!("map {map}: a held scroll on a live hero always arrives")
            }
        }
    }
}

// --- Determinism: one draw per arrival, zero per rejection, pure menu. -------

#[test]
fn warp_draws_exactly_one_word_on_arrival_and_zero_on_every_rejection() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);

    // Arrived: exactly one word — the landing pick.
    let qualified = hero("dark_knight", 60, 10_000, 0, &[0, 4]);
    let mut rng = CountingRng::new(7);
    let (_, outcome) = resolve_warp(&qualified, entry, &atlas, Wings::None, &mut rng);
    assert!(matches!(outcome, WarpTravelOutcome::Arrived { .. }));
    assert_eq!(rng.words, 1);

    // Every rejection: zero words.
    let rejections = [
        with_field(
            &qualified,
            "life",
            json!({"kind": "dead", "respawn_at": 903}),
        ),
        hero("dark_knight", 60, 10_000, 0, &[0]),
        hero("dark_knight", 40, 10_000, 0, &[0, 4]),
        hero("dark_knight", 60, 4_999, 0, &[0, 4]),
    ];
    for traveler in &rejections {
        let mut rng = CountingRng::new(7);
        let (_, outcome) = resolve_warp(traveler, entry, &atlas, Wings::None, &mut rng);
        assert!(
            !matches!(outcome, WarpTravelOutcome::Arrived { .. }),
            "{outcome:?}"
        );
        assert_eq!(rng.words, 0, "a rejection never touches the stream");
    }

    // A wingless Icarus attempt joins the zero-draw set: the wings gate sits
    // before the landing draw, so CannotFly never touches the stream.
    let wingless = hero("fairy_elf", 200, 20_000, 0, &[0, 10]);
    let mut rng = CountingRng::new(7);
    let (_, outcome) = resolve_warp(
        &wingless,
        warp_view(&atlas, ICARUS_ENTRY),
        &atlas,
        Wings::None,
        &mut rng,
    );
    assert_eq!(outcome, WarpTravelOutcome::CannotFly);
    assert_eq!(rng.words, 0, "a wings refusal never touches the stream");
}

#[test]
fn portal_and_gate_draw_exactly_one_word_on_arrival_and_zero_on_rejection() {
    let atlas = real_atlas();

    // Scroll arrival: one word (the town-gate landing pick).
    let traveler = hero("dark_knight", 60, 0, 1, &[1]);
    let mut rng = CountingRng::new(7);
    let (_, _, outcome) = use_town_portal(
        &traveler,
        bag_with(&atlas, TOWN_PORTAL, 1),
        CELL,
        &atlas,
        &mut rng,
    );
    assert!(matches!(outcome, TownPortalOutcome::Arrived { .. }));
    assert_eq!(rng.words, 1);

    // Scroll rejections: zero.
    let mut rng = CountingRng::new(7);
    let (_, _, outcome) =
        use_town_portal(&traveler, Inventory::empty(8, 8), CELL, &atlas, &mut rng);
    assert_eq!(outcome, TownPortalOutcome::NoScroll);
    assert_eq!(rng.words, 0);

    // Enter-gate traversal: one on arrival, zero on a level refusal.
    let gate = gate_at(&atlas, 0, 5, 38);
    let mut rng = CountingRng::new(7);
    let (_, outcome) = traverse_enter_gate(
        &hero("dark_knight", 60, 0, 0, &[0]),
        gate,
        &atlas,
        Wings::None,
        &mut rng,
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));
    assert_eq!(rng.words, 1);
    let mut rng = CountingRng::new(7);
    let (_, outcome) = traverse_enter_gate(
        &hero("dark_knight", 10, 0, 0, &[0]),
        gate,
        &atlas,
        Wings::None,
        &mut rng,
    );
    assert_eq!(outcome, EnterGateOutcome::LevelTooLow { required: 15 });
    assert_eq!(rng.words, 0);
}

#[test]
fn identical_inputs_and_seed_produce_a_byte_identical_character() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, LOST_TOWER_ENTRY);
    let traveler = hero("dark_knight", 60, 10_000, 0, &[0, 4]);

    let (a, outcome_a) = resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(21));
    let (b, outcome_b) = resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(21));
    assert_eq!(outcome_a, outcome_b);
    assert_eq!(
        or_abort(serde_json::to_string(&a)),
        or_abort(serde_json::to_string(&b)),
        "same placement, same wallet, same discovered set"
    );

    let scroll_reader = hero("dark_knight", 60, 0, 1, &[1]);
    let (a, _, outcome_a) = use_town_portal(
        &scroll_reader,
        bag_with(&atlas, TOWN_PORTAL, 1),
        CELL,
        &atlas,
        &mut TestRng::new(21),
    );
    let (b, _, outcome_b) = use_town_portal(
        &scroll_reader,
        bag_with(&atlas, TOWN_PORTAL, 1),
        CELL,
        &atlas,
        &mut TestRng::new(21),
    );
    assert_eq!(outcome_a, outcome_b);
    assert_eq!(
        or_abort(serde_json::to_string(&a)),
        or_abort(serde_json::to_string(&b))
    );
}

// --- Wings and the s6 backport: Tarkan/Icarus reachable, Sky entry gated. ----

#[test]
fn a_wingless_qualified_hero_warps_to_tarkan_debiting_the_fee() {
    // WF-TARK-1: Tarkan is Ground, so the wings fact is immaterial.
    let atlas = real_atlas();
    let entry = warp_view(&atlas, TARKAN_ENTRY);
    assert_eq!(entry.landing.map, MapNumber(8), "the entry lands on Tarkan");
    assert_eq!(entry.warp.cost_zen, Zen(8_000));
    assert_eq!(entry.warp.min_level.get(), 140);

    let traveler = hero("dark_knight", 150, 20_000, 0, &[0, 8]);
    let (arrived, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_seated(&atlas, &entry.landing, placement);
            assert_eq!(balance, zen(12_000), "debited by exactly the fee");
            assert_eq!(arrived.zen(), balance);
            assert_eq!(placement.movement, Movement::Grounded, "Tarkan is Ground");
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => {
            panic!("a wingless hero enters a Ground map: {outcome:?}")
        }
    }
}

#[test]
fn the_second_tarkan_entry_lands_at_its_own_gate_for_its_own_fee() {
    // WF-TARK-2: index 22 targets gate 77's rect (91,160)-(93,161) at 8,500.
    let atlas = real_atlas();
    let entry = warp_view(&atlas, TARKAN2_ENTRY);
    assert_eq!(entry.landing.map, MapNumber(8));
    assert_eq!(entry.warp.cost_zen, Zen(8_500));

    let traveler = hero("dark_knight", 150, 20_000, 0, &[0, 8]);
    let (arrived, outcome) =
        resolve_warp(&traveler, entry, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_seated(&atlas, &entry.landing, placement);
            assert_eq!(balance, zen(11_500));
            assert_eq!(arrived.zen(), balance);
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => {
            panic!("the second Tarkan entry arrives: {outcome:?}")
        }
    }
}

#[test]
fn a_wingless_qualified_hero_is_refused_icarus_with_the_wallet_untouched() {
    // WF-ICA-1: discovery, level, and zen are all met; only wings is unmet —
    // and a wings failure costs nothing and draws nothing.
    let atlas = real_atlas();
    let entry = warp_view(&atlas, ICARUS_ENTRY);
    assert_eq!(
        entry.landing.map,
        MapNumber(10),
        "the entry lands on Icarus"
    );
    assert_eq!(entry.warp.cost_zen, Zen(10_000));
    assert_eq!(entry.warp.min_level.get(), 170);

    let traveler = hero("fairy_elf", 200, 20_000, 0, &[0, 10]);
    let mut rng = CountingRng::new(7);
    let (unchanged, outcome) = resolve_warp(&traveler, entry, &atlas, Wings::None, &mut rng);

    assert_eq!(outcome, WarpTravelOutcome::CannotFly);
    assert_eq!(unchanged.zen(), zen(20_000), "nothing charged");
    assert_eq!(unchanged.placement(), traveler.placement(), "nothing moved");
    assert_eq!(rng.words, 0, "the wings gate sits before the landing draw");
}

#[test]
fn the_same_hero_with_wings_warps_to_icarus_lands_flying_and_pays() {
    // WF-ICA-2: wings flip the one unmet gate; the Sky arrival forces Flying.
    let atlas = real_atlas();
    let entry = warp_view(&atlas, ICARUS_ENTRY);
    let traveler = hero("fairy_elf", 200, 20_000, 0, &[0, 10]);

    let mut rng = CountingRng::new(7);
    let (arrived, outcome) = resolve_warp(&traveler, entry, &atlas, Wings::Equipped, &mut rng);

    match outcome {
        WarpTravelOutcome::Arrived { placement, balance } => {
            assert_seated(&atlas, &entry.landing, placement);
            assert_eq!(balance, zen(10_000), "debited by exactly the fee");
            assert_eq!(arrived.zen(), balance);
            assert_eq!(placement.movement, Movement::Flying, "Sky forces flight");
        }
        WarpTravelOutcome::NotAlive
        | WarpTravelOutcome::NotDiscovered
        | WarpTravelOutcome::LevelTooLow { .. }
        | WarpTravelOutcome::CannotFly
        | WarpTravelOutcome::NotEnoughZen { .. }
        | WarpTravelOutcome::NoWalkableLanding => {
            panic!("the winged hero arrives on Icarus: {outcome:?}")
        }
    }
    assert_eq!(rng.words, 1, "exactly the landing pick");
}

#[test]
fn discovery_and_level_short_circuit_before_wings_on_icarus() {
    let atlas = real_atlas();
    let entry = warp_view(&atlas, ICARUS_ENTRY);

    // WF-ICA-3: an undiscovered Icarus is NotDiscovered, not CannotFly.
    let stranger = hero("fairy_elf", 200, 20_000, 0, &[0]);
    let mut rng = CountingRng::new(7);
    let (unchanged, outcome) = resolve_warp(&stranger, entry, &atlas, Wings::None, &mut rng);
    assert_eq!(outcome, WarpTravelOutcome::NotDiscovered);
    assert_eq!(unchanged.zen(), zen(20_000), "nothing charged");
    assert_eq!(rng.words, 0);

    // WF-ICA-4: an under-level wingless hero is LevelTooLow, not CannotFly.
    let low = hero("fairy_elf", 100, 20_000, 0, &[0, 10]);
    let (unchanged, outcome) = resolve_warp(&low, entry, &atlas, Wings::None, &mut TestRng::new(7));
    assert_eq!(outcome, WarpTravelOutcome::LevelTooLow { required: 170 });
    assert_eq!(unchanged.zen(), zen(20_000), "nothing charged");
}

#[test]
fn the_lost_tower_icarus_door_bars_a_wingless_hero_without_moving_him() {
    // WF-GATE-1: gate 62 (trigger (17,250)-(19,250) on Lost Tower, min 160)
    // lands on Sky — CannotFly for bare shoulders, unmoved, undiscovered.
    let atlas = real_atlas();
    let door = gate_at(&atlas, 4, 17, 250);
    assert_eq!(door.landing.map, MapNumber(10), "the door lands on Icarus");
    let traveler = hero("dark_knight", 200, 0, 4, &[4]);

    let mut rng = CountingRng::new(7);
    let (unchanged, outcome) = traverse_enter_gate(&traveler, door, &atlas, Wings::None, &mut rng);

    assert_eq!(outcome, EnterGateOutcome::CannotFly);
    assert_eq!(unchanged.placement(), traveler.placement(), "not moved");
    assert!(
        !unchanged.discovered().contains(MapNumber(10)),
        "a refused door discovers nothing"
    );
    assert_eq!(rng.words, 0, "the wings gate sits before the landing draw");
}

#[test]
fn the_same_door_with_wings_admits_lands_flying_and_discovers_icarus() {
    // WF-GATE-2.
    let atlas = real_atlas();
    let door = gate_at(&atlas, 4, 17, 250);
    let traveler = hero("dark_knight", 200, 0, 4, &[4]);

    let (crossed, outcome) = traverse_enter_gate(
        &traveler,
        door,
        &atlas,
        Wings::Equipped,
        &mut TestRng::new(7),
    );

    match outcome {
        EnterGateOutcome::Arrived { placement } => {
            assert_seated(&atlas, &door.landing, placement);
            assert_eq!(placement.movement, Movement::Flying, "Sky forces flight");
            assert_eq!(crossed.placement(), placement);
        }
        EnterGateOutcome::NotAlive
        | EnterGateOutcome::LevelTooLow { .. }
        | EnterGateOutcome::CannotFly
        | EnterGateOutcome::NoWalkableLanding => {
            panic!("the winged hero steps through: {outcome:?}")
        }
    }
    assert!(crossed.discovered().contains(MapNumber(10)), "discovered");
    assert!(crossed.discovered().contains(MapNumber(4)), "kept");
}

#[test]
fn level_short_circuits_before_wings_at_the_enter_gate_too() {
    // WF-GATE-3: the door's order is level → wings.
    let atlas = real_atlas();
    let door = gate_at(&atlas, 4, 17, 250);
    let low = hero("dark_knight", 100, 0, 4, &[4]);

    let (_, outcome) = traverse_enter_gate(&low, door, &atlas, Wings::None, &mut TestRng::new(7));

    assert_eq!(outcome, EnterGateOutcome::LevelTooLow { required: 160 });
}

#[test]
fn leaving_icarus_needs_no_wings_because_the_exit_lands_on_ground() {
    // WF-GATE-4: gate 64 (trigger (14,12)-(16,12) on Icarus, min 50) lands on
    // Lost Tower — the wings gate keys off the DESTINATION, so exit is free.
    let atlas = real_atlas();
    let door = gate_at(&atlas, 10, 14, 12);
    assert_eq!(
        door.landing.map,
        MapNumber(4),
        "the door lands on Lost Tower"
    );
    let elf = hero("fairy_elf", 90, 0, 10, &[10]);

    let (crossed, outcome) =
        traverse_enter_gate(&elf, door, &atlas, Wings::None, &mut TestRng::new(7));

    match outcome {
        EnterGateOutcome::Arrived { placement } => {
            assert_seated(&atlas, &door.landing, placement);
            assert_eq!(placement.movement, Movement::Grounded, "Ground grounds");
        }
        EnterGateOutcome::NotAlive
        | EnterGateOutcome::LevelTooLow { .. }
        | EnterGateOutcome::CannotFly
        | EnterGateOutcome::NoWalkableLanding => {
            panic!("a wingless exit is never gated: {outcome:?}")
        }
    }
    assert!(crossed.discovered().contains(MapNumber(4)), "discovered");
    assert!(crossed.discovered().contains(MapNumber(10)), "kept");
}

#[test]
fn a_quadruply_failing_icarus_entry_lists_all_four_reasons_in_check_order() {
    // WF-PROJ-1: the complete ordered lock set — discovery → level → wings →
    // zen — pinned as values and as the exact wire shape.
    let atlas = real_atlas();
    let novice = hero("dark_knight", 100, 5_000, 0, &[0]);

    let menu = warp_menu(&novice, &atlas, Wings::None);
    let icarus = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(ICARUS_ENTRY))
            .ok_or("the menu lists the Icarus entry"),
    );
    match &icarus.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(
                listed,
                vec![
                    WarpLockReason::NotDiscovered,
                    WarpLockReason::LevelTooLow { required: 170 },
                    WarpLockReason::CannotFly,
                    WarpLockReason::InsufficientZen { cost: Zen(10_000) },
                ]
            );
        }
        WarpAvailability::Available => panic!("a quadruply-failing entry is locked"),
    }
    assert_eq!(
        or_abort(serde_json::to_string(&icarus.availability)),
        r#"{"kind":"locked","reasons":[{"kind":"not_discovered"},{"kind":"level_too_low","required":170},{"kind":"cannot_fly"},{"kind":"insufficient_zen","cost":10000}]}"#
    );

    // WF-PROJ-3: the same hero's Tarkan entry never carries CannotFly — a
    // Ground destination has no wings reason.
    let tarkan = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(TARKAN_ENTRY))
            .ok_or("the menu lists the Tarkan entry"),
    );
    match &tarkan.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(
                listed,
                vec![
                    WarpLockReason::NotDiscovered,
                    WarpLockReason::LevelTooLow { required: 140 },
                    WarpLockReason::InsufficientZen { cost: Zen(8_000) },
                ],
                "no CannotFly on a Ground destination"
            );
        }
        WarpAvailability::Available => panic!("a triply-failing entry is locked"),
    }
}

#[test]
fn cannot_fly_is_the_sole_lock_for_a_qualified_wingless_hero_and_wings_flip_it() {
    // WF-PROJ-2: wings is the only gate; equipping flips exactly that reason.
    let atlas = real_atlas();
    let elf = hero("fairy_elf", 200, 50_000, 0, &[0, 10]);

    let menu = warp_menu(&elf, &atlas, Wings::None);
    let icarus = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(ICARUS_ENTRY))
            .ok_or("the menu lists the Icarus entry"),
    );
    match &icarus.availability {
        WarpAvailability::Locked { reasons } => {
            let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
            assert_eq!(listed, vec![WarpLockReason::CannotFly]);
        }
        WarpAvailability::Available => panic!("a wingless hero cannot open the Sky entry"),
    }

    let menu = warp_menu(&elf, &atlas, Wings::Equipped);
    let icarus = or_abort(
        menu.iter()
            .find(|status| status.index == WarpIndex(ICARUS_ENTRY))
            .ok_or("the menu lists the Icarus entry"),
    );
    assert!(matches!(icarus.availability, WarpAvailability::Available));
}

#[test]
fn walking_atlans_to_tarkan_and_back_discovers_tarkan_and_keeps_atlans() {
    // WF-DISC-1: the s6 doors 53 (Atlans→Tarkan) and 55 (Tarkan→Atlans).
    let atlas = real_atlas();
    let out_door = gate_at(&atlas, 7, 14, 225);
    assert_eq!(out_door.landing.map, MapNumber(8));
    let traveler = hero("dark_knight", 150, 0, 7, &[7]);

    let (on_tarkan, outcome) = traverse_enter_gate(
        &traveler,
        out_door,
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));
    assert!(on_tarkan.discovered().contains(MapNumber(8)), "discovered");
    assert!(on_tarkan.discovered().contains(MapNumber(7)), "kept");

    let back_door = gate_at(&atlas, 8, 246, 40);
    assert_eq!(back_door.landing.map, MapNumber(7));
    let (home, outcome) = traverse_enter_gate(
        &on_tarkan,
        back_door,
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));
    assert_eq!(home.placement().map, MapNumber(7));
    assert!(home.discovered().contains(MapNumber(7)));
    assert!(home.discovered().contains(MapNumber(8)), "Tarkan retained");
}

#[test]
fn a_winged_descent_to_icarus_discovers_it_and_the_wingless_return_is_free() {
    // WF-DISC-2: down through gate 62 with wings, back out gate 64 without.
    let atlas = real_atlas();
    let down_door = gate_at(&atlas, 4, 17, 250);
    let traveler = hero("dark_knight", 200, 0, 4, &[4]);

    let (on_icarus, outcome) = traverse_enter_gate(
        &traveler,
        down_door,
        &atlas,
        Wings::Equipped,
        &mut TestRng::new(7),
    );
    assert!(matches!(outcome, EnterGateOutcome::Arrived { .. }));
    assert!(on_icarus.discovered().contains(MapNumber(10)));

    // Wings come off; the exit door is destination-Ground, so it opens anyway.
    let up_door = gate_at(&atlas, 10, 14, 12);
    let (home, outcome) = traverse_enter_gate(
        &on_icarus,
        up_door,
        &atlas,
        Wings::None,
        &mut TestRng::new(7),
    );
    match outcome {
        EnterGateOutcome::Arrived { placement } => {
            assert_eq!(placement.map, MapNumber(4));
        }
        EnterGateOutcome::NotAlive
        | EnterGateOutcome::LevelTooLow { .. }
        | EnterGateOutcome::CannotFly
        | EnterGateOutcome::NoWalkableLanding => {
            panic!("the wingless return exits freely: {outcome:?}")
        }
    }
    assert!(home.discovered().contains(MapNumber(4)));
    assert!(
        home.discovered().contains(MapNumber(10)),
        "first footfall kept"
    );
}

#[test]
fn the_menu_projection_is_a_pure_query() {
    let atlas = real_atlas();
    let traveler = hero("magic_gladiator", 40, 4_000, 0, &[0, 2, 4]);
    // No RngCore parameter exists to pass; reading twice yields identical
    // projections.
    assert_eq!(
        warp_menu(&traveler, &atlas, Wings::None),
        warp_menu(&traveler, &atlas, Wings::None)
    );
}
