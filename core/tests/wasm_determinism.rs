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
    WeightedTable, draw_cardinal, uniform_in_inclusive, weighted_pick,
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

use mu_core::components::combat_profile::CombatProfile;
use mu_core::components::pool::Pool;
use mu_core::services::combat::{ExcellentOrder, StrikeBasis, resolve_attack};

/// A hand-pinned combat profile, built through the wire (the only door an
/// external test has) so the fixture needs no filesystem.
fn fixed_profile(
    level: u16,
    span: (u16, u16),
    defense: u16,
    rates: (u16, u16),
    chances: u8,
) -> CombatProfile {
    or_abort(serde_json::from_value(serde_json::json!({
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
use mu_core::components::spatial::{Facing, Fixed, TileDelta, TileOffset, WorldVec};
use mu_core::components::tile::{TileCoord, WalkGrid};
use mu_core::components::units::MapNumber;
use mu_core::data::skills::Skill;
use mu_core::entities::character::Character;
use mu_core::services::chance::draw_jiggle_offset;
use mu_core::services::skills::{DamagingSkillRef, SkillRouting, cast, route};

#[test]
fn draw_jiggle_offset_sequence_is_identical_across_targets() {
    // The ±1 jiggle draw (dx then dy, two `uniform_below(3)` words per call)
    // under SEED: the nine-outcome offsets below are the cross-target contract.
    let mut rng = SplitMix64::new(SEED);
    let seq: Vec<TileOffset> = (0..12).map(|_| draw_jiggle_offset(&mut rng)).collect();
    let offset = |dx: TileDelta, dy: TileDelta| TileOffset::new(dx, dy);
    assert_eq!(
        seq,
        vec![
            offset(TileDelta::Neg, TileDelta::Zero),
            offset(TileDelta::Zero, TileDelta::Neg),
            offset(TileDelta::Neg, TileDelta::Zero),
            offset(TileDelta::Neg, TileDelta::Neg),
            offset(TileDelta::Zero, TileDelta::Neg),
            offset(TileDelta::Neg, TileDelta::Zero),
            offset(TileDelta::Neg, TileDelta::Zero),
            offset(TileDelta::Pos, TileDelta::Pos),
            offset(TileDelta::Pos, TileDelta::Neg),
            offset(TileDelta::Neg, TileDelta::Pos),
            offset(TileDelta::Neg, TileDelta::Zero),
            offset(TileDelta::Pos, TileDelta::Zero),
        ]
    );
}

#[test]
fn the_octant_quantizer_is_identical_across_targets() {
    // The 8-way away-vector quantizer is pure i128 arithmetic over the pinned
    // 408/985 convergent of tan 22.5° — no RNG, no float — so its sector
    // decisions (including the boundary-exact-to-diagonal tie rule) must agree
    // bit-for-bit on every target. The vectors are the design doc's worked
    // boundaries.
    let vec = |x: i64, y: i64| WorldVec::new(Fixed::from_raw(x), Fixed::from_raw(y));
    let offset = |dx: TileDelta, dy: TileDelta| TileOffset::new(dx, dy);
    // Just inside the East sector.
    assert_eq!(
        vec(985, 407).octant(),
        Some(offset(TileDelta::Pos, TileDelta::Zero))
    );
    // Exactly on the 22.5° boundary: the pinned tie rule resolves diagonal.
    assert_eq!(
        vec(985, 408).octant(),
        Some(offset(TileDelta::Pos, TileDelta::Pos))
    );
    // Straight north; a clear diagonal; the mirrored west flank; the zero vector.
    assert_eq!(
        vec(0, 65_536).octant(),
        Some(offset(TileDelta::Zero, TileDelta::Pos))
    );
    assert_eq!(
        vec(131_072, 65_536).octant(),
        Some(offset(TileDelta::Pos, TileDelta::Pos))
    );
    assert_eq!(
        vec(-985, -407).octant(),
        Some(offset(TileDelta::Neg, TileDelta::Zero))
    );
    assert_eq!(WorldVec::ZERO.octant(), None);
}

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

fn open_ground() -> WalkGrid {
    WalkGrid::from_words([u64::MAX; 1024])
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
        fixed_damaging(&skill).locate(aim),
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
        fixed_damaging(&skill).locate(aim),
        &targets,
        &open_ground(),
        &mut rng,
    );
    assert_eq!(vitals.mana.current(), 391, "the lunge's 9 mana is spent");
    // Draw-by-draw under SEED: the strike lands normal for 89 (the bare
    // [33,50] span ×2030/1000); the caster teleports onto the target's (12,10)
    // cell facing east; the victim's two jiggle words land the (−1,−1)
    // diagonal, one tile toward the origin corner at (11,9).
    assert_eq!(
        or_abort(serde_json::to_string(&outcome)),
        r#"{"kind":"cast","caster_placement":{"position":{"x":819200,"y":688128},"facing":{"x":131072,"y":0},"movement":"grounded","map":0},"hits":[{"kind":"landed","target_index":0,"hit":{"damage":89,"quality":"normal","modifiers":[]},"health":{"current":99911,"max":100000},"active_effects":[],"inflicted":null,"displacement":{"position":{"x":753664,"y":622592},"facing":{"x":-65536,"y":-65536},"movement":"grounded","map":0}}]}"#
    );
}
