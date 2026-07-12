//! Cross-target determinism, executed under wasm (Q4).
//!
//! This is the leg CI RUNS under `wasm32-wasip1` + wasmtime, not just compiles:
//! it asserts that a fixed RNG seed drives the core sampling seam and the chance
//! service to a FIXED, hardcoded sequence of outputs. The expected values are
//! the cross-target contract — native and wasm must both reproduce them
//! bit-for-bit, so a divergence (float creep, endian bug, width bug) is a red
//! test on whichever target drifted.
//!
//! Deliberately proptest-free: proptest pulls `wait-timeout`, which does not
//! build for wasi (see `core/Cargo.toml`), so this plain-`#[test]` file is the
//! one that runs under wasmtime while the property tests run on the native legs.

use core::num::NonZeroU32;
use std::io::Write;

use rand_core::RngCore;

use mu_core::components::interval::Interval;
use mu_core::rng::uniform_below;
use mu_core::services::chance::{
    WeightedTable, draw_cardinal, draw_heading, uniform_in_inclusive, weighted_pick,
};

/// Aborts on an impossible error (a nonzero literal failing `NonZeroU32::new`),
/// matching the integration-suite convention so no banned suppressor is needed
/// outside a `#[test]` body.
fn or_abort<T, E: std::fmt::Display>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => {
            let mut stderr = std::io::stderr();
            let _ = writeln!(stderr, "wasm_determinism: {error}");
            std::process::abort()
        }
    }
}

/// Deterministic `SplitMix64` — the same generator the in-crate tests drive, so
/// the sequence is replayable and identical on every target.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }
}

impl RngCore for SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn next_u32(&mut self) -> u32 {
        let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
        u32::from_le_bytes([b0, b1, b2, b3])
    }

    fn fill_bytes(&mut self, dst: &mut [u8]) {
        for chunk in dst.chunks_mut(8) {
            let bytes = self.next_u64().to_le_bytes();
            for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                *slot = *byte;
            }
        }
    }
}

fn nz(value: u32) -> NonZeroU32 {
    or_abort(NonZeroU32::new(value).ok_or("test bound must be nonzero"))
}

const SEED: u64 = 0x1234_5678_9ABC_DEF0;

#[test]
fn uniform_below_sequence_is_identical_across_targets() {
    let mut rng = SplitMix64::new(SEED);
    let seq: Vec<u32> = (0..12).map(|_| uniform_below(nz(1000), &mut rng)).collect();
    assert_eq!(
        seq,
        vec![272, 630, 563, 151, 47, 357, 189, 35, 643, 271, 268, 589]
    );
}

#[test]
fn uniform_in_inclusive_sequence_is_identical_across_targets() {
    let span = Interval::new(10u16, 50u16).expect("valid span");
    let mut rng = SplitMix64::new(SEED);
    let seq: Vec<u16> = (0..12)
        .map(|_| uniform_in_inclusive(span, &mut rng))
        .collect();
    assert_eq!(seq, vec![21, 35, 33, 16, 11, 24, 17, 11, 36, 21, 21, 34]);
}

#[test]
fn weighted_pick_sequence_is_identical_across_targets() {
    let table = WeightedTable::new(vec![(nz(1), 'a'), (nz(2), 'b'), (nz(3), 'c'), (nz(4), 'd')])
        .expect("nonempty weighted table");
    let mut rng = SplitMix64::new(SEED);
    let seq: String = (0..16).map(|_| *weighted_pick(&table, &mut rng)).collect();
    assert_eq!(seq, "bdcbacbadbbcacdd");
}

#[test]
fn draw_cardinal_sequence_is_identical_across_targets() {
    let mut rng = SplitMix64::new(SEED);
    let seq: Vec<(i64, i64)> = (0..12)
        .map(|_| {
            let facing = draw_cardinal(&mut rng);
            (facing.vector().x().raw(), facing.vector().y().raw())
        })
        .collect();
    assert_eq!(
        seq,
        vec![
            (1, 0),
            (-1, -1),
            (1, 1),
            (0, 1),
            (-1, 0),
            (0, 1),
            (-1, 1),
            (0, 1),
            (-1, -1),
            (1, 1),
            (1, 1),
            (-1, 0),
        ]
    );
}

#[test]
fn draw_heading_sequence_is_identical_across_targets() {
    // The Marsaglia disk-rejection heading draw consumes a variable number of
    // words per call, so this pins BOTH the accepted directions and, implicitly,
    // the exact rejection-loop word consumption: any float creep, endian, or
    // width divergence would move a drawn point across the disk boundary and
    // change the sequence.
    let mut rng = SplitMix64::new(SEED);
    let seq: Vec<(i64, i64)> = (0..12)
        .map(|_| {
            let facing = draw_heading(&mut rng);
            (facing.vector().x().raw(), facing.vector().y().raw())
        })
        .collect();
    assert_eq!(
        seq,
        vec![
            (-1862, 1069),
            (518, -2853),
            (-3704, -1169),
            (1176, -1870),
            (-1897, 730),
            (-3866, -1353),
            (2857, 2082),
            (2199, -2587),
            (-3125, -24),
            (2256, 1170),
            (-2998, 101),
            (-1566, -2310),
        ]
    );
}

// -- A fixed item roll serializes identically on native and wasm. -------------

use mu_core::components::class::ClassSet;
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::item_ref::ItemRef;
use mu_core::components::levels::OptionLevel;
use mu_core::components::units::{ChancePer10000, ItemLevel};
use mu_core::data::common::{Provenance, SkillNumber, SourceVersion};
use mu_core::data::item_definitions::{
    ItemDefinition, ItemKind, ItemPrice, WeaponHandling, WearRequirements,
};
use mu_core::data::option_roll::OptionRollPolicy;
use mu_core::services::item_roll::roll_dropped_item;

/// A fixed weapon definition — the roll target. Hand-built so the test needs no
/// filesystem (it runs under wasmtime).
fn fixed_weapon() -> ItemDefinition {
    ItemDefinition {
        id: ItemRef {
            group: 0,
            number: 3,
        },
        provenance: Provenance {
            source_version: SourceVersion::V075,
            review: None,
        },
        width: 1,
        height: 3,
        drops_from_monsters: true,
        drop_level: 10,
        max_item_level: or_abort(ItemLevel::new(15)),
        durability: 20,
        price: ItemPrice::Formula,
        kind: ItemKind::Weapon {
            handling: WeaponHandling::OneHanded,
            min_damage: 5,
            max_damage: 12,
            attack_speed: 30,
            skill: Some(SkillNumber(19)),
            classes: ClassSet::NONE,
            wear: WearRequirements {
                level: 0,
                strength: 0,
                agility: 0,
                vitality: 0,
                energy: 0,
                command: 0,
            },
        },
    }
}

fn always() -> OptionRollPolicy {
    OptionRollPolicy {
        item_option_roll_per_10000: ChancePer10000::ALWAYS,
        luck_roll_per_10000: ChancePer10000::ALWAYS,
        extra_excellent_option_roll_per_10000: ChancePer10000::ALWAYS,
        max_excellent_options_per_drop: 3,
        max_dropped_option_level: OptionLevel::L4,
        review: None,
    }
}

// -- A fixed skill strike resolves identically on native and wasm. ------------

use mu_core::components::combat_profile::{CombatProfile, TargetKind};
use mu_core::components::pool::Pool;
use mu_core::services::combat::{ExcellentOrder, StrikeBasis, resolve_attack};

/// A hand-pinned combat profile of an explicit combat `kind` (`"player"` or
/// `"npc"`), built through the wire (the only door an external test has) so the
/// fixture needs no filesystem.
fn kinded_profile(
    kind: &str,
    level: u16,
    span: (u16, u16),
    defense: u16,
    rates: (u16, u16),
    chances: u8,
) -> CombatProfile {
    or_abort(serde_json::from_value(serde_json::json!({
        "kind": kind,
        "level": level,
        "physical": {"min": span.0, "max": span.1},
        "wizardry": null,
        "defense": defense,
        "attack_rate": rates.0,
        "defense_rate": rates.1,
        "resistances": {
            "ice": 0, "poison": 0, "lightning": 0, "fire": 0,
            "earth": 0, "wind": 0, "water": 0
        },
        "critical_chance": chances,
        "excellent_chance": chances,
        "defense_ignore_chance": chances,
        "double_damage_chance": chances,
        "incoming_damage_reduction": 0,
        "flat_damage_add": 0
    })))
}

/// A hand-pinned NPC-kind combat profile — the default fixture the pre-existing
/// draw-sequence goldens strike over.
fn fixed_profile(
    level: u16,
    span: (u16, u16),
    defense: u16,
    rates: (u16, u16),
    chances: u8,
) -> CombatProfile {
    kinded_profile("npc", level, span, defense, rates, chances)
}

#[test]
fn a_fixed_skill_strike_serializes_identically_across_targets() {
    // A wizardry-order skill basis with the DK ×2030 multiplier over a fixed
    // seed: the strike's six draws and the whole re-based fold must reproduce
    // this exact outcome on native and wasm alike.
    let attacker = fixed_profile(50, (33, 50), 0, (10_000, 0), 20);
    let target = fixed_profile(20, (1, 2), 30, (0, 0), 0);
    let basis = StrikeBasis::Skill {
        span: or_abort(Interval::new(56u16, 92u16)),
        excellent_order: ExcellentOrder::DefenseThenMultiply,
        multiplier_per_mille: 2030,
    };
    let mut rng = SplitMix64::new(SEED);
    let (health, outcome) = resolve_attack(&attacker, &target, Pool::full(500), &basis, &mut rng);
    assert_eq!(health.current(), 314);
    // Draw-by-draw under SEED: hit lands, span rolled, critical procs (and no
    // excellent), defense-ignore procs, no double — so the head is the
    // augmented max 92 with defense zeroed (the level floor 5 doesn't bind),
    // × 2030/1000 = 186.
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        r#"{"kind":"landed","hit":{"damage":186,"quality":"critical","modifiers":["defense_ignored"]}}"#
    );
}

#[test]
fn a_fixed_item_roll_serializes_identically_across_targets() {
    let mut rng = SplitMix64::new(SEED);
    let instance = roll_dropped_item(
        &fixed_weapon(),
        or_abort(ItemLevel::new(9)),
        ItemRarity::Excellent,
        &always(),
        &mut rng,
    );
    let serialized = or_abort(serde_json::to_string(&instance));
    assert_eq!(
        serialized,
        r#"{"item":{"group":0,"number":3},"level":9,"roll":{"kind":"excellent","options":{"set":"weapon","options":["health_after_kill","damage_per_level","excellent_damage_chance"]}},"normal_option":{"option":"physical_damage","level":3},"luck":"lucky","skill":"with_skill","durability":{"current":49,"max":49},"augment":{"kind":"none"}}"#
    );
}

// -- W-AREA: geometry, push, and jiggle replay across targets. -----------------

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::combat_profile::CombatTarget;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::spatial::Facing;
use mu_core::components::tile::{TerrainGrid, TileCoord};
use mu_core::components::units::MapNumber;
use mu_core::data::skills::Skill;
use mu_core::entities::character::Character;
use mu_core::services::skills::{DamagingSkillRef, Designation, SkillRouting, cast, route};

/// A hand-pinned level-50 Dark Knight caster at tile (10, 10) facing +X, built
/// through the wire (the only door an external test has) so the fixture needs
/// no filesystem.
fn fixed_caster() -> Character {
    or_abort(serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": 50,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 200, "agility": 100, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {
            "position": {"x": 10 * 65_536 + 32_768, "y": 10 * 65_536 + 32_768},
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": 0
        },
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    })))
}

/// A hand-pinned deep-health target seated at `tile`, wearing the fixed
/// defender profile.
fn fixed_target(tile: (u8, u8)) -> CombatTarget {
    CombatTarget::new(
        fixed_profile(20, (1, 2), 0, (0, 0), 0),
        Pool::full(100_000),
        Placement {
            position: TileCoord::new(tile.0, tile.1).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        },
        ActiveEffects::EMPTY,
    )
}

/// The damaging reference of a hand-built skill; a non-damaging shape aborts.
fn fixed_damaging(skill: &Skill) -> DamagingSkillRef<'_> {
    match route(skill) {
        SkillRouting::Damaging(reference) => reference,
        SkillRouting::Buff(_) | SkillRouting::Heal(_) | SkillRouting::Deferred => {
            or_abort(Err::<DamagingSkillRef<'_>, _>("expected a damaging skill"))
        }
    }
}

/// An Earthshake-shaped area skill (caster circle r=5, directional push, the
/// inert lightning tag), hand-built through the wire.
fn fixed_earthshake() -> Skill {
    or_abort(serde_json::from_value(serde_json::json!({
        "number": 62,
        "source_version": "075",
        "attack_damage": 150,
        "damage_type": "physical",
        "element": "lightning",
        "range": 10,
        "shape": {
            "kind": "area",
            "geometry": {"kind": "caster_circle", "radius_x2": 10},
            "displacement": "directional_push"
        },
        "cost": {"mana": 0, "ability": 50},
        "learn": {"level": 0, "energy": 0, "command": 0},
        "classes": []
    })))
}

/// A lunge-shaped weapon skill, hand-built through the wire.
fn fixed_lunge() -> Skill {
    or_abort(serde_json::from_value(serde_json::json!({
        "number": 19,
        "source_version": "075",
        "attack_damage": 0,
        "damage_type": "physical",
        "range": 3,
        "shape": {"kind": "lunge"},
        "cost": {"mana": 9, "ability": 0},
        "learn": {"level": 0, "energy": 0, "command": 0},
        "classes": []
    })))
}

fn open_ground() -> TerrainGrid {
    TerrainGrid::from_words([u64::MAX; 1024])
}

#[test]
fn a_fixed_earthshake_cast_serializes_identically_across_targets() {
    // The authored caster-circle geometry read, the strike, and the
    // away-vector push (target two tiles east -> thrown to (15,10), zero
    // displacement words) under SEED must reproduce this exact outcome on
    // native and wasm alike.
    let caster = fixed_caster();
    let profile = fixed_profile(50, (33, 50), 0, (10_000, 0), 0);
    let skill = fixed_earthshake();
    let targets = [fixed_target((12, 10))];
    let aim = TileCoord::new(10, 10).to_world();
    let mut rng = SplitMix64::new(SEED);
    let (vitals, outcome) = cast(
        &caster,
        &profile,
        fixed_damaging(&skill).locate(aim, Designation::Incidental),
        &targets,
        &open_ground(),
        &mut rng,
    );
    assert_eq!(vitals.ability.current(), 350, "the quake's 50 AG is spent");
    // Draw-by-draw under SEED: the strike lands normal for 489 (the [33,50]
    // span augmented by D=150 to [183,275], ×2030/1000); the inert lightning
    // tag rolls no element word; the push draws nothing and throws the (12,10)
    // target due east to (15,10).
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        r#"{"kind":"cast","caster_placement":{"position":{"x":688128,"y":688128},"facing":{"x":1,"y":0},"movement":"grounded","map":0},"hits":[{"kind":"landed","target_index":0,"hit":{"damage":489,"quality":"normal","modifiers":[]},"health":{"current":99511,"max":100000},"active_effects":[],"inflicted":null,"displacement":{"position":{"x":1015808,"y":688128},"facing":{"x":65536,"y":0},"movement":"grounded","map":0}}]}"#
    );
}

#[test]
fn a_fixed_diagonal_earthshake_cast_serializes_identically_across_targets() {
    // The continuous swept knockback along a true 45° away-vector: the caster at
    // (10,10) throws the target at (13,13) three tiles straight-line to
    // (1_023_759,1_023_759) — ~2.12 tiles on each axis, NOT the whole-tile
    // (16,16) an octant snap would give, and NOT an axis-aligned (16,13). Zero
    // displacement words. This exact serialization is the cross-target contract:
    // native and wasm must reproduce the diagonal push bit-for-bit.
    let caster = fixed_caster();
    let profile = fixed_profile(50, (33, 50), 0, (10_000, 0), 0);
    let skill = fixed_earthshake();
    let targets = [fixed_target((13, 13))];
    let aim = TileCoord::new(10, 10).to_world();
    let mut rng = SplitMix64::new(SEED);
    let (vitals, outcome) = cast(
        &caster,
        &profile,
        fixed_damaging(&skill).locate(aim, Designation::Incidental),
        &targets,
        &open_ground(),
        &mut rng,
    );
    assert_eq!(vitals.ability.current(), 350, "the quake's 50 AG is spent");
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        r#"{"kind":"cast","caster_placement":{"position":{"x":688128,"y":688128},"facing":{"x":1,"y":0},"movement":"grounded","map":0},"hits":[{"kind":"landed","target_index":0,"hit":{"damage":489,"quality":"normal","modifiers":[]},"health":{"current":99511,"max":100000},"active_effects":[],"inflicted":null,"displacement":{"position":{"x":1023759,"y":1023759},"facing":{"x":46341,"y":46341},"movement":"grounded","map":0}}]}"#
    );
}

#[test]
fn a_fixed_lunge_cast_serializes_identically_across_targets() {
    // The lunge's caster teleport (no draw) and its MovesTarget jiggle (two
    // words) under SEED must reproduce this exact outcome — placement and
    // displacement alike — on native and wasm.
    let caster = fixed_caster();
    let profile = fixed_profile(50, (33, 50), 0, (10_000, 0), 0);
    let skill = fixed_lunge();
    let targets = [fixed_target((12, 10))];
    let aim = TileCoord::new(12, 10).to_world();
    let mut rng = SplitMix64::new(SEED);
    let (vitals, outcome) = cast(
        &caster,
        &profile,
        fixed_damaging(&skill).locate(aim, Designation::Forced { target_index: 0 }),
        &targets,
        &open_ground(),
        &mut rng,
    );
    assert_eq!(vitals.mana.current(), 391, "the lunge's 9 mana is spent");
    // Draw-by-draw under SEED: the strike lands normal for 89 (the bare
    // [33,50] span ×2030/1000); the caster teleports onto the target's (12,10)
    // cell facing east; the victim's continuous jiggle draws a free heading and
    // nudges it ~one tile along it, to (854089,632649) — this exact continuous
    // displacement is the cross-target contract, reproduced bit-for-bit on
    // native and wasm.
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        r#"{"kind":"cast","caster_placement":{"position":{"x":819200,"y":688128},"facing":{"x":131072,"y":0},"movement":"grounded","map":0},"hits":[{"kind":"landed","target_index":0,"hit":{"damage":89,"quality":"normal","modifiers":[]},"health":{"current":99911,"max":100000},"active_effects":[],"inflicted":null,"displacement":{"position":{"x":854089,"y":632649},"facing":{"x":34889,"y":-55479},"movement":"grounded","map":0}}]}"#
    );
}

// --- W-GROUND: the RNG-free ground lifecycle, pickup gates, firewall, and ---
// --- party fifth term reproduce byte-identically across targets (DET-2). ---

use mu_core::components::drop_claim::{DropClaim, PickerStanding};
use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::party::{MemberSlot, Membership, Vitality};
use mu_core::components::units::{CarriedZen, DurationMs, Exp, Level, Tick, TickDuration, Zen};
use mu_core::entities::party_session::{PartyMember, PartySession};
use mu_core::entities::world_item::WorldItem;
use mu_core::entities::world_zen::WorldZen;
use mu_core::services::ground::{DropOrigin, reap_ground, stamp_item, stamp_zen};
use mu_core::services::inventory::{PickupOutcome, ZenPickupOutcome, pickup, pickup_zen};
use mu_core::services::party::{MemberFact, SlotWallet, split_zen_pickup};

/// A fully walkable grid whose safe set is exactly the listed tiles — the
/// pocket the firewall and fifth-term pins stand a subject on.
fn safe_pocket(tiles: &[(u8, u8)]) -> TerrainGrid {
    let mut safe = [0u64; 1024];
    for &(x, y) in tiles {
        let bit = (usize::from(y) << 8) | usize::from(x);
        let word = or_abort(safe.get_mut(bit >> 6).ok_or("tile bit within the grid"));
        *word |= 1u64 << (bit & 63);
    }
    TerrainGrid::from_bitsets([u64::MAX; 1024], safe)
}

/// A hand-pinned sword instance — the ground item the lifecycle pins carry.
fn fixed_instance() -> ItemInstance {
    ItemInstance {
        item: ItemRef {
            group: 0,
            number: 3,
        },
        level: ItemLevel::ZERO,
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(30),
        augment: CraftedAugment::None,
    }
}

#[test]
fn fixed_ground_stamps_and_the_reaper_are_identical_across_targets() {
    // A monster kill at tick 100 on the 50 ms cadence with the authentic
    // 60 s duration: appearance 120 (the 1 s beat), despawn 1320, the claim
    // window closing at 320 — pure integer arithmetic, no draw.
    let tick = or_abort(TickDuration::new(50));
    let stamp = stamp_item(DropOrigin::MonsterKill, Tick(100), DurationMs(60_000), tick);
    assert_eq!(stamp.appearance, Tick(120));
    assert_eq!(stamp.despawn, Tick(1320));
    assert_eq!(stamp.claim, DropClaim::Claimed { until: Tick(320) });
    let zen_stamp = stamp_zen(DropOrigin::MonsterKill, Tick(100), DurationMs(60_000), tick);
    assert_eq!(zen_stamp.despawn, stamp.despawn);

    let position = TileCoord::new(10, 10).to_world();
    let item = WorldItem {
        instance: fixed_instance(),
        position,
        map: MapNumber(0),
        despawn: stamp.despawn,
        claim: stamp.claim,
    };
    let pile = WorldZen {
        amount: Zen(40_000),
        position,
        map: MapNumber(0),
        despawn: zen_stamp.despawn,
    };

    let (kept_items, kept_zen, events) =
        reap_ground(vec![item.clone()], vec![pile.clone()], Tick(1319));
    assert_eq!(kept_items, vec![item.clone()]);
    assert_eq!(kept_zen, vec![pile.clone()]);
    assert!(events.is_empty());

    let (gone_items, gone_zen, events) = reap_ground(vec![item], vec![pile], Tick(1320));
    assert!(gone_items.is_empty());
    assert!(gone_zen.is_empty());
    assert_eq!(
        or_abort(serde_json::to_string(&events)),
        r#"[{"kind":"item_despawned","position":{"x":688128,"y":688128},"map":0,"item":{"group":0,"number":3}},{"kind":"zen_despawned","position":{"x":688128,"y":688128},"map":0,"amount":40000}]"#
    );
}

#[test]
fn fixed_pickup_gates_are_identical_across_targets() {
    // The reach gate, the claim window, and the store step — all RNG-free.
    let footprint = or_abort(Footprint::new(1, 3));
    let anchor = Cell { row: 0, col: 0 };
    let item_pos = TileCoord::new(10, 10).to_world();
    let ground = WorldItem {
        instance: fixed_instance(),
        position: item_pos,
        map: MapNumber(0),
        despawn: Tick(1320),
        claim: DropClaim::Claimed { until: Tick(320) },
    };

    // Four tiles away: OutOfReach, the untouched item handed back.
    let far = TileCoord::new(14, 10).to_world();
    let (_, outcome) = pickup(
        ground.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        far,
        MapNumber(0),
        PickerStanding::Owner,
        Tick(120),
    );
    assert_eq!(
        outcome,
        PickupOutcome::OutOfReach {
            item: ground.clone()
        }
    );

    // In reach, a stranger inside the window: Refused.
    let near = TileCoord::new(12, 10).to_world();
    let (_, outcome) = pickup(
        ground.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        near,
        MapNumber(0),
        PickerStanding::Stranger,
        Tick(120),
    );
    assert_eq!(
        outcome,
        PickupOutcome::Refused {
            item: ground.clone()
        }
    );

    // The owner inside the window stores it.
    let (_, outcome) = pickup(
        ground.clone(),
        Inventory::empty(8, 8),
        anchor,
        footprint,
        near,
        MapNumber(0),
        PickerStanding::Owner,
        Tick(120),
    );
    assert_eq!(outcome, PickupOutcome::PickedUp { at: anchor });

    // Zen gates on the same reach with no window.
    let pile = WorldZen {
        amount: Zen(500),
        position: item_pos,
        map: MapNumber(0),
        despawn: Tick(1320),
    };
    let (balance, outcome) = pickup_zen(
        pile.clone(),
        or_abort(CarriedZen::new(0)),
        far,
        MapNumber(0),
    );
    assert_eq!(outcome, ZenPickupOutcome::OutOfReach { world_zen: pile });
    assert_eq!(balance, or_abort(CarriedZen::new(0)));
}

#[test]
fn a_fixed_cast_from_a_safe_tile_is_rejected_identically_across_targets() {
    // The firewall's caster gate: the fixed caster stands at (10, 10); with
    // that tile safe the cast is refused before any spend, and no RNG word is
    // drawn — SEED replays untouched.
    let caster = fixed_caster();
    let profile = fixed_profile(50, (33, 50), 0, (10_000, 0), 0);
    let skill = fixed_earthshake();
    let targets = [fixed_target((12, 10))];
    let aim = TileCoord::new(10, 10).to_world();
    let grid = safe_pocket(&[(10, 10)]);
    let mut rng = SplitMix64::new(SEED);
    let (vitals, outcome) = cast(
        &caster,
        &profile,
        fixed_damaging(&skill).locate(aim, Designation::Incidental),
        &targets,
        &grid,
        &mut rng,
    );
    assert_eq!(vitals.ability.current(), 400, "nothing spent");
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        r#"{"kind":"rejected","reason":"caster_in_safezone"}"#
    );
    assert_eq!(rng.next_u64(), SplitMix64::new(SEED).next_u64());
}

#[test]
fn a_fixed_zen_split_excludes_the_safe_stander_identically_across_targets() {
    // The party fifth term, RNG-free: slot 1 stands on the safe tile and is
    // dropped from the divisor; the 100_000 pile splits 50_000/50_000 between
    // the picker (slot 0) and the field member (slot 2).
    let party = PartySession::forming().with_member(PartyMember {
        slot: MemberSlot(2),
        membership: Membership::Active,
    });
    let fact = |slot: u8, tile: (u8, u8)| MemberFact {
        slot: MemberSlot(slot),
        level: or_abort(Level::new(30)),
        experience: Exp(0),
        vitality: Vitality::Alive,
        map: MapNumber(0),
        position: TileCoord::new(tile.0, tile.1).to_world(),
    };
    let pile = WorldZen {
        amount: Zen(100_000),
        position: TileCoord::new(10, 10).to_world(),
        map: MapNumber(0),
        despawn: Tick(1320),
    };
    let grid = safe_pocket(&[(11, 10)]);
    let others = [fact(1, (11, 10)), fact(2, (12, 10))];
    let other_wallets = [
        SlotWallet {
            slot: MemberSlot(1),
            wallet: or_abort(CarriedZen::new(0)),
        },
        SlotWallet {
            slot: MemberSlot(2),
            wallet: or_abort(CarriedZen::new(0)),
        },
    ];
    let result = split_zen_pickup(
        &pile,
        &party,
        fact(0, (10, 10)),
        or_abort(CarriedZen::new(0)),
        &others,
        &other_wallets,
        &grid,
    );
    assert_eq!(
        or_abort(serde_json::to_string(&result.credits)),
        r#"[{"slot":0,"wallet":50000},{"slot":2,"wallet":50000}]"#
    );
    assert!(result.to_ground.is_empty());
}

// --- W-PVP: the target-kind wire tag, the all-NPC area struck set, and the ----
// --- PvP/PvM draw-sequence identity reproduce byte-identically across targets. -

use mu_core::events::combat::AttackOutcome;
use mu_core::events::skills::SkillOutcome;

/// The damage a landed or lethal strike dealt, or `None` for a miss.
fn landed_damage(outcome: &AttackOutcome) -> Option<u32> {
    match outcome {
        AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => Some(hit.damage.0),
        AttackOutcome::Missed => None,
    }
}

#[test]
fn target_kind_serializes_to_its_snake_case_tag_across_targets() {
    // The combat category rides the wire as a bare snake_case string, identical on
    // native and wasm, and round-trips on every variant.
    assert_eq!(
        or_abort(serde_json::to_string(&TargetKind::Player)),
        r#""player""#
    );
    assert_eq!(
        or_abort(serde_json::to_string(&TargetKind::Npc)),
        r#""npc""#
    );
    for kind in [TargetKind::Player, TargetKind::Npc] {
        let wire = or_abort(serde_json::to_string(&kind));
        assert_eq!(or_abort(serde_json::from_str::<TargetKind>(&wire)), kind);
    }
}

#[test]
fn an_all_npc_area_cast_strikes_the_npc_and_replays_byte_for_byte() {
    // An incidental area sweep over a lone NPC strikes it — an area cast hits every
    // incidental NPC, so the struck set is unchanged from before the player/npc
    // split — and replays byte-for-byte under a fixed seed on native and wasm.
    let caster = fixed_caster();
    let profile = fixed_profile(50, (33, 50), 0, (10_000, 0), 0);
    let skill = fixed_earthshake();
    let targets = [fixed_target((12, 10))];
    let aim = TileCoord::new(10, 10).to_world();
    let run = |seed: u64| {
        let mut rng = SplitMix64::new(seed);
        cast(
            &caster,
            &profile,
            fixed_damaging(&skill).locate(aim, Designation::Incidental),
            &targets,
            &open_ground(),
            &mut rng,
        )
        .1
    };
    let outcome = run(SEED);
    match &outcome {
        SkillOutcome::Cast { hits, .. } => {
            assert_eq!(
                hits.len(),
                1,
                "the lone NPC is struck by the incidental sweep"
            );
        }
        SkillOutcome::Rejected { .. } => {
            or_abort(Err::<(), _>("the funded field cast resolves"));
        }
    }
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        or_abort(serde_json::to_string(&run(SEED))),
        "the incidental all-NPC area cast replays byte-for-byte under a fixed seed"
    );
}

#[test]
fn a_pvp_strike_draws_the_same_sequence_as_a_pvm_strike_and_only_the_overrate_differs() {
    // Identical attacker/defender stats and identical seed: the player-versus-
    // player strike and the player-versus-monster strike draw the SAME RNG
    // sequence — the matchup only re-scales the final damage post-draw. The
    // defender out-rates the attacker, so the monster strike crushes the overrate
    // to 3/10 while the player strike keeps full damage; the draw sequence is
    // byte-identical either way, proved by both streams sitting at the same word
    // afterward. The out-rated hit floors at 3%, so a landed pair is swept for.
    let attacker = kinded_profile("player", 10, (100, 100), 0, (100, 0), 0);
    let monster_defender = kinded_profile("npc", 10, (0, 0), 0, (0, 200), 0);
    let player_defender = kinded_profile("player", 10, (0, 0), 0, (0, 200), 0);
    let mut landed = 0u32;
    for seed in 0u64..256 {
        let mut monster_stream = SplitMix64::new(seed);
        let mut player_stream = SplitMix64::new(seed);
        let (_, versus_monster) = resolve_attack(
            &attacker,
            &monster_defender,
            Pool::full(500),
            &StrikeBasis::PlainSwing,
            &mut monster_stream,
        );
        let (_, versus_player) = resolve_attack(
            &attacker,
            &player_defender,
            Pool::full(500),
            &StrikeBasis::PlainSwing,
            &mut player_stream,
        );
        // Same draw sequence at every seed: after each strike both streams sit at
        // the same next word (equal seed + equal draw count => equal state).
        assert_eq!(
            monster_stream.next_u64(),
            player_stream.next_u64(),
            "seed {seed}: the two matchups draw an identical sequence"
        );
        if let (Some(against_monster), Some(against_player)) = (
            landed_damage(&versus_monster),
            landed_damage(&versus_player),
        ) {
            assert!(
                against_player > against_monster,
                "seed {seed}: the player strike keeps full damage {against_player}; the monster strike is overrate-crushed to {against_monster}"
            );
            landed += 1;
        }
    }
    assert!(landed > 0, "an out-rated hit lands within 256 seeds");
}

// --- W-PK: the player-kill reputation wire forms are byte-identical across -----
// --- targets, and every reputation transition draws ZERO RNG (their signatures -
// --- carry no generator), so a combat kill's stream is unchanged whether or not -
// --- the killer is flagged and decayed after it. -----------------------------

use mu_core::components::reputation::{PkStage, PlayerKillCount, Reputation, Standing};
use mu_core::components::units::Ticks;
use mu_core::events::reputation::{PkEvent, SanctionReason};
use mu_core::services::reputation::{
    PvpContext, decay_reputation, player_kill_sanction, resolve_player_kill,
};

/// The suite tick base: 50 ms per tick — the same cadence the reputation
/// transitions convert their online-hour step against.
fn pk_tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

#[test]
fn pk_stage_serializes_to_its_snake_case_tag_across_targets() {
    for (stage, wire) in [
        (PkStage::Warning, r#""warning""#),
        (PkStage::FirstStage, r#""first_stage""#),
        (PkStage::SecondStage, r#""second_stage""#),
    ] {
        assert_eq!(or_abort(serde_json::to_string(&stage)), wire);
        assert_eq!(or_abort(serde_json::from_str::<PkStage>(wire)), stage);
    }
}

#[test]
fn standing_and_reputation_wire_forms_are_identical_across_targets() {
    assert_eq!(
        or_abort(serde_json::to_string(&Standing::Clean)),
        r#"{"kind":"clean"}"#
    );
    assert_eq!(
        or_abort(serde_json::to_string(&Standing::Flagged {
            stage: PkStage::FirstStage,
            decays_at: Tick(903),
        })),
        r#"{"kind":"flagged","stage":"first_stage","decays_at":903}"#
    );

    // A clean reputation is the flat standing-plus-tally pair.
    assert_eq!(
        or_abort(serde_json::to_string(&Reputation::clean())),
        r#"{"standing":{"kind":"clean"},"kills":0}"#
    );
    // A flagged reputation carrying a lifetime tally round-trips byte-for-byte.
    let flagged =
        r#"{"standing":{"kind":"flagged","stage":"second_stage","decays_at":903},"kills":2}"#;
    let reputation = or_abort(serde_json::from_str::<Reputation>(flagged));
    assert_eq!(or_abort(serde_json::to_string(&reputation)), flagged);
}

#[test]
fn pk_event_wire_forms_are_identical_across_targets() {
    let cases = [
        (
            PkEvent::Flagged {
                stage: PkStage::FirstStage,
                decays_at: Tick(903),
                lifetime_kills: PlayerKillCount(2),
            },
            r#"{"kind":"flagged","stage":"first_stage","decays_at":903,"lifetime_kills":2}"#,
        ),
        (
            PkEvent::Sanctioned {
                reason: SanctionReason::VictimWasMurderer,
            },
            r#"{"kind":"sanctioned","reason":"victim_was_murderer"}"#,
        ),
        (
            PkEvent::Decayed {
                standing: Standing::Clean,
            },
            r#"{"kind":"decayed","standing":{"kind":"clean"}}"#,
        ),
        (
            PkEvent::DecayAccelerated {
                decays_at: Tick(903),
                reduced_by: Ticks(80),
            },
            r#"{"kind":"decay_accelerated","decays_at":903,"reduced_by":80}"#,
        ),
    ];
    for (event, wire) in cases {
        assert_eq!(or_abort(serde_json::to_string(&event)), wire);
        assert_eq!(or_abort(serde_json::from_str::<PkEvent>(wire)), event);
    }
}

#[test]
fn pk_transitions_draw_no_rng_so_a_kill_stream_is_identical_across_targets() {
    // A fixed lethal strike over the seed, then the same strike over the same seed
    // followed by the whole flag + decay path. None of the reputation transitions
    // takes an RNG (their signatures carry no generator), so both streams sit at
    // the same next word: a combat kill's RNG sequence is byte-identical whether or
    // not the killer is flagged and decayed after it — the cross-target contract.
    let attacker = fixed_profile(50, (100, 100), 0, (10_000, 0), 20);
    let target = fixed_profile(20, (1, 2), 0, (0, 0), 0);

    let mut bare = SplitMix64::new(SEED);
    let (_health, bare_outcome) = resolve_attack(
        &attacker,
        &target,
        Pool::full(50),
        &StrikeBasis::PlainSwing,
        &mut bare,
    );
    assert!(
        matches!(bare_outcome, AttackOutcome::Killed { .. }),
        "the fixed strike is a combat kill"
    );

    let mut with_pk = SplitMix64::new(SEED);
    let (_health, _outcome) = resolve_attack(
        &attacker,
        &target,
        Pool::full(50),
        &StrikeBasis::PlainSwing,
        &mut with_pk,
    );
    // The killer-bump: a clean victim flags the killer up the ladder.
    let victim = fixed_caster();
    let killer = fixed_caster();
    let sanction = player_kill_sanction(&victim, PvpContext::Open);
    let (flagged, _flag_event) = resolve_player_kill(killer, sanction, Tick(1000), pk_tick());
    // Then the tick-driven decay peels it all the way back to clean.
    let (_decayed, _decay_event) = decay_reputation(flagged, Tick(10_000_000), pk_tick());

    // Neither transition advanced the generator: the same next word as the bare
    // strike stream, on native and wasm alike.
    assert_eq!(bare.next_u64(), with_pk.next_u64());
}
