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
