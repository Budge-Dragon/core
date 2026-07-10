//! Combat durability wear over the real `/data` Atlas (W-EQUIP): the
//! defensive/offensive/pet/ammunition pools, the pendant's offensive-path
//! exemption, the broken-out-of-pool OUR-pin, the persisted scaled-integer
//! ledger riding the returned equipment across a serde round trip, the
//! fixed RNG draw order (defender pool, then attacker pool, AFTER the
//! unchanged strike), and the empty-pool zero-draw rule — all proven through
//! the public `wear_from_strike` / `resolve_strike_with_wear` ports.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]`
//! body so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use rand_core::RngCore;

use dataset::{or_abort, real_atlas};
use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::pool::Pool;
use mu_core::components::units::ItemLevel;
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::events::combat::{AttackOutcome, Damage, DamageModifiers, Hit, HitQuality};
use mu_core::services::combat::{StrikeBasis, resolve_attack};
use mu_core::services::profile::character_profile;
use mu_core::services::wear::{WearEvent, resolve_strike_with_wear, wear_from_strike};
use rng::TestRng;

// --- Real catalog identities (group, number). --------------------------------

const HELM: ItemRef = ItemRef {
    group: 7,
    number: 0,
};
const ARMOR: ItemRef = ItemRef {
    group: 8,
    number: 0,
};
const PANTS: ItemRef = ItemRef {
    group: 9,
    number: 0,
};
const GLOVES: ItemRef = ItemRef {
    group: 10,
    number: 0,
};
const BOOTS: ItemRef = ItemRef {
    group: 11,
    number: 0,
};
const WINGS: ItemRef = ItemRef {
    group: 12,
    number: 0,
};
const RING_OF_ICE: ItemRef = ItemRef {
    group: 13,
    number: 8,
};
const RING_OF_POISON: ItemRef = ItemRef {
    group: 13,
    number: 9,
};
const PENDANT_OF_LIGHTNING: ItemRef = ItemRef {
    group: 13,
    number: 12,
};
const SHIELD: ItemRef = ItemRef {
    group: 6,
    number: 0,
};
const GUARDIAN_ANGEL: ItemRef = ItemRef {
    group: 13,
    number: 0,
};
const KRIS: ItemRef = ItemRef {
    group: 0,
    number: 1,
};
const BOW: ItemRef = ItemRef {
    group: 4,
    number: 0,
};
const ARROWS: ItemRef = ItemRef {
    group: 4,
    number: 15,
};

/// A fresh full-gauge instance of real item `id`.
fn item(atlas: &Atlas, id: ItemRef) -> ItemInstance {
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

/// The same instance at a chosen gauge.
fn item_at(atlas: &Atlas, id: ItemRef, current: u8) -> ItemInstance {
    let mut instance = item(atlas, id);
    let max = instance.durability.max();
    instance.durability = or_abort(Durability::new(current, max));
    instance
}

/// The full defensive wardrobe: five armor pieces, wings, both rings, a
/// shield in the left hand — plus the pendant, which must NEVER wear here.
fn defensive_set(atlas: &Atlas) -> Equipment {
    Equipment::empty()
        .with(EquipmentSlot::Helm, item(atlas, HELM))
        .with(EquipmentSlot::Armor, item(atlas, ARMOR))
        .with(EquipmentSlot::Pants, item(atlas, PANTS))
        .with(EquipmentSlot::Gloves, item(atlas, GLOVES))
        .with(EquipmentSlot::Boots, item(atlas, BOOTS))
        .with(EquipmentSlot::Wings, item(atlas, WINGS))
        .with(EquipmentSlot::Ring1, item(atlas, RING_OF_ICE))
        .with(EquipmentSlot::Ring2, item(atlas, RING_OF_POISON))
        .with(EquipmentSlot::LeftHand, item(atlas, SHIELD))
        .with(EquipmentSlot::Pendant, item(atlas, PENDANT_OF_LIGHTNING))
}

/// A landed hit carrying `damage` health damage — the typed outcome the wear
/// service trusts; no strike resolution is needed to drive the pools.
fn landed(damage: u32) -> AttackOutcome {
    AttackOutcome::Landed {
        hit: Hit {
            damage: Damage(damage),
            quality: HitQuality::Normal,
            modifiers: DamageModifiers::NONE,
        },
    }
}

/// The slot a single wear event names, with its post-wear gauge when worn.
fn worn_slot(event: &WearEvent) -> (EquipmentSlot, Option<Durability>) {
    match *event {
        WearEvent::Worn { slot, durability } => (slot, Some(durability)),
        WearEvent::Broken { slot } | WearEvent::Destroyed { slot } => (slot, None),
    }
}

#[test]
fn a_damaging_hit_wears_exactly_one_defensive_item_and_never_the_pendant() {
    // EQ-WEAR-1: 2000 HealthDamage = exactly one durability point on exactly
    // one random defensive-pool member; the pendant is exempt (offensive
    // path). Swept across seeds so the whole pool is exercised.
    let atlas = real_atlas();
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..256u64 {
        let mut rng = TestRng::new(seed);
        let wear = wear_from_strike(
            &landed(2000),
            Equipment::empty(),
            defensive_set(&atlas),
            &atlas,
            &mut rng,
        );
        assert!(wear.attacker_events.is_empty(), "the attacker is naked");
        assert_eq!(wear.defender_events.len(), 1, "exactly one defensive pick");
        let (slot, gauge) = worn_slot(&wear.defender_events[0]);
        assert_ne!(slot, EquipmentSlot::Pendant, "the pendant never wears here");
        assert_ne!(slot, EquipmentSlot::Pet, "no pet is worn");
        let after = or_abort(wear.defender_worn.get(slot).ok_or("still worn"));
        let full = or_abort(atlas.item(after.item).ok_or("known item")).durability;
        assert_eq!(after.durability.current(), full - 1, "one point lost");
        assert_eq!(gauge, Some(after.durability));
        seen.insert(format!("{slot:?}"));
    }
    assert_eq!(
        seen.len(),
        9,
        "every defensive-pool member (5 armor + wings + 2 rings + shield) is reachable: {seen:?}"
    );
}

#[test]
fn a_landing_attacker_wears_one_of_weapon_or_pendant_at_the_flat_rate() {
    // EQ-WEAR-2/5: the offensive pool is the weapon hand plus the PENDANT;
    // the flat 1/10000 advances the persisted counter without flooring away —
    // durability is unchanged on hit 1, and the ledger rides the returned set.
    let atlas = real_atlas();
    let attacker = Equipment::empty()
        .with(EquipmentSlot::RightHand, item(&atlas, KRIS))
        .with(EquipmentSlot::Pendant, item(&atlas, PENDANT_OF_LIGHTNING));
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..64u64 {
        let mut rng = TestRng::new(seed);
        let wear = wear_from_strike(
            &landed(500),
            attacker.clone(),
            Equipment::empty(),
            &atlas,
            &mut rng,
        );
        assert_eq!(wear.attacker_events.len(), 1, "exactly one offensive pick");
        let (slot, gauge) = worn_slot(&wear.attacker_events[0]);
        assert!(
            matches!(slot, EquipmentSlot::RightHand | EquipmentSlot::Pendant),
            "the offensive pool is weapon + pendant, got {slot:?}"
        );
        let gauge = or_abort(gauge.ok_or("hit 1 never breaks a full item"));
        let full = if slot == EquipmentSlot::RightHand {
            item(&atlas, KRIS).durability
        } else {
            item(&atlas, PENDANT_OF_LIGHTNING).durability
        };
        assert_eq!(gauge.current(), full.current(), "1/10000 crosses no point");
        assert_ne!(gauge, full, "the hit counter advanced on the ledger");
        seen.insert(format!("{slot:?}"));
    }
    assert_eq!(
        seen.len(),
        2,
        "both offensive members are reachable: {seen:?}"
    );
}

#[test]
fn a_shield_or_ammo_hand_never_joins_the_offensive_pick() {
    // A bow + quiver attacker: the quiver only consumes; the bow is the only
    // offensive pick — the ammo hand never wears the flat rate.
    let atlas = real_atlas();
    let attacker = Equipment::empty()
        .with(EquipmentSlot::RightHand, item(&atlas, BOW))
        .with(EquipmentSlot::LeftHand, item(&atlas, ARROWS));
    for seed in 0..32u64 {
        let mut rng = TestRng::new(seed);
        let wear = wear_from_strike(
            &landed(500),
            attacker.clone(),
            Equipment::empty(),
            &atlas,
            &mut rng,
        );
        // Two attacker events: the ammo consumption, then the weapon pick.
        assert_eq!(wear.attacker_events.len(), 2);
        let (ammo_slot, _) = worn_slot(&wear.attacker_events[0]);
        let (pick_slot, _) = worn_slot(&wear.attacker_events[1]);
        assert_eq!(ammo_slot, EquipmentSlot::LeftHand, "the quiver spent first");
        assert_eq!(pick_slot, EquipmentSlot::RightHand, "only the bow wears");
    }
}

#[test]
fn a_miss_wears_nothing_but_ammunition_and_draws_zero_words() {
    let atlas = real_atlas();
    // Gearless both sides, a miss: no events, and the RNG is untouched — a
    // twin stream drawn after the call matches a fresh one word-for-word.
    let mut used = TestRng::new(99);
    let wear = wear_from_strike(
        &AttackOutcome::Missed,
        Equipment::empty(),
        Equipment::empty(),
        &atlas,
        &mut used,
    );
    assert!(wear.attacker_events.is_empty());
    assert!(wear.defender_events.is_empty());
    let mut fresh = TestRng::new(99);
    assert_eq!(used.next_u64(), fresh.next_u64(), "a miss draws zero words");

    // A bow-wielder's miss still spends one round (hit-or-miss), still no RNG.
    let attacker = Equipment::empty()
        .with(EquipmentSlot::RightHand, item(&atlas, BOW))
        .with(EquipmentSlot::LeftHand, item(&atlas, ARROWS));
    let mut used = TestRng::new(7);
    let wear = wear_from_strike(
        &AttackOutcome::Missed,
        attacker,
        defensive_set(&atlas),
        &atlas,
        &mut used,
    );
    assert_eq!(
        wear.attacker_events.len(),
        1,
        "the round is spent on a miss"
    );
    let (slot, _) = worn_slot(&wear.attacker_events[0]);
    assert_eq!(slot, EquipmentSlot::LeftHand);
    assert!(wear.defender_events.is_empty(), "a miss wears no gear");
    let mut fresh = TestRng::new(7);
    assert_eq!(
        used.next_u64(),
        fresh.next_u64(),
        "ammo consumption is RNG-free"
    );
}

#[test]
fn a_zero_damage_hit_wears_no_gear() {
    // EQ-WEAR-3: a fully-reduced landed hit (0 HealthDamage) folds to no wear
    // structurally — same as a miss.
    let atlas = real_atlas();
    let mut rng = TestRng::new(3);
    let wear = wear_from_strike(
        &landed(0),
        Equipment::empty().with(EquipmentSlot::RightHand, item(&atlas, KRIS)),
        defensive_set(&atlas),
        &atlas,
        &mut rng,
    );
    assert!(wear.attacker_events.is_empty());
    assert!(wear.defender_events.is_empty());
    let mut fresh = TestRng::new(3);
    assert_eq!(
        rng.next_u64(),
        fresh.next_u64(),
        "zero damage draws no words"
    );
}

#[test]
fn an_all_broken_pool_is_empty_and_draws_zero_words() {
    // EQ-BROKEN-3 (OUR-pin): broken items leave the pool; a defender wearing
    // only broken pieces takes no wear and costs no RNG word.
    let atlas = real_atlas();
    let broken_set = Equipment::empty()
        .with(EquipmentSlot::Helm, item_at(&atlas, HELM, 0))
        .with(EquipmentSlot::Armor, item_at(&atlas, ARMOR, 0));
    let mut used = TestRng::new(11);
    let wear = wear_from_strike(
        &landed(4000),
        Equipment::empty(),
        broken_set,
        &atlas,
        &mut used,
    );
    assert!(
        wear.defender_events.is_empty(),
        "broken items never re-wear"
    );
    let mut fresh = TestRng::new(11);
    assert_eq!(
        used.next_u64(),
        fresh.next_u64(),
        "an empty pool draws nothing"
    );

    // A broken piece beside an intact one: only the intact piece can wear.
    let mixed = Equipment::empty()
        .with(EquipmentSlot::Helm, item_at(&atlas, HELM, 0))
        .with(EquipmentSlot::Armor, item(&atlas, ARMOR));
    for seed in 0..32u64 {
        let mut rng = TestRng::new(seed);
        let wear = wear_from_strike(
            &landed(2000),
            Equipment::empty(),
            mixed.clone(),
            &atlas,
            &mut rng,
        );
        let (slot, _) = worn_slot(&wear.defender_events[0]);
        assert_eq!(
            slot,
            EquipmentSlot::Armor,
            "only the intact piece is in the pool"
        );
    }
}

#[test]
fn the_ledger_rides_the_returned_equipment_across_a_serde_round_trip() {
    // EQ-WEAR-4: 1500 then 1500 on the SAME (single-member pool) item — the
    // first call crosses nothing, the remainder persists through a wire
    // round trip, and the second call crosses exactly one point.
    let atlas = real_atlas();
    let solo = Equipment::empty().with(EquipmentSlot::Helm, item(&atlas, HELM));
    let full = item(&atlas, HELM).durability.current();

    let mut rng = TestRng::new(1);
    let first = wear_from_strike(&landed(1500), Equipment::empty(), solo, &atlas, &mut rng);
    let after_first = or_abort(first.defender_worn.get(EquipmentSlot::Helm).ok_or("worn"));
    assert_eq!(
        after_first.durability.current(),
        full,
        "1500 < 2000: no point"
    );

    // Persist seam: serialize/deserialize the worn set mid-sequence.
    let json = or_abort(serde_json::to_string(&first.defender_worn));
    let reloaded: Equipment = or_abort(serde_json::from_str(&json));

    let mut rng = TestRng::new(2);
    let second = wear_from_strike(
        &landed(1500),
        Equipment::empty(),
        reloaded,
        &atlas,
        &mut rng,
    );
    let after_second = or_abort(second.defender_worn.get(EquipmentSlot::Helm).ok_or("worn"));
    assert_eq!(
        after_second.durability.current(),
        full - 1,
        "3000 accumulated crosses one point after the round trip"
    );
}

#[test]
fn the_last_point_breaks_the_item_which_stays_worn() {
    // EQ-BROKEN-1: the broken-keep seam — durability 0, Broken event, the
    // item still occupies its slot (repairable), never removed.
    let atlas = real_atlas();
    let solo = Equipment::empty().with(EquipmentSlot::Ring1, item_at(&atlas, RING_OF_ICE, 1));
    let mut rng = TestRng::new(1);
    let wear = wear_from_strike(&landed(2000), Equipment::empty(), solo, &atlas, &mut rng);
    assert_eq!(
        wear.defender_events,
        vec![WearEvent::Broken {
            slot: EquipmentSlot::Ring1
        }]
    );
    let broken = or_abort(wear.defender_worn.get(EquipmentSlot::Ring1).ok_or("worn"));
    assert_eq!(broken.durability.current(), 0, "broken, not destroyed");
}

#[test]
fn the_pet_wears_additionally_and_is_destroyed_at_zero() {
    // EQ-PET-2: the pet is outside the random pool (both events fire on one
    // hit) at /100000; a non-trainable pet ground to 0 is Destroyed — removed.
    let atlas = real_atlas();
    let set = Equipment::empty()
        .with(EquipmentSlot::Helm, item(&atlas, HELM))
        .with(EquipmentSlot::Pet, item_at(&atlas, GUARDIAN_ANGEL, 1));
    let mut rng = TestRng::new(1);
    let wear = wear_from_strike(&landed(100_000), Equipment::empty(), set, &atlas, &mut rng);
    assert_eq!(wear.defender_events.len(), 2, "pool pick + additional pet");
    assert_eq!(
        wear.defender_events[1],
        WearEvent::Destroyed {
            slot: EquipmentSlot::Pet
        }
    );
    assert!(
        wear.defender_worn.get(EquipmentSlot::Pet).is_none(),
        "a destroyed pet leaves its slot"
    );

    // Below the crossing the pet only accumulates (Worn, gauge unchanged).
    let set = Equipment::empty().with(EquipmentSlot::Pet, item(&atlas, GUARDIAN_ANGEL));
    let mut rng = TestRng::new(2);
    let wear = wear_from_strike(&landed(2000), Equipment::empty(), set, &atlas, &mut rng);
    let (slot, gauge) = worn_slot(&wear.defender_events[0]);
    assert_eq!(slot, EquipmentSlot::Pet);
    let gauge = or_abort(gauge.ok_or("the pet accumulated without crossing"));
    assert_eq!(
        gauge.current(),
        item(&atlas, GUARDIAN_ANGEL).durability.current()
    );
}

#[test]
fn ammunition_is_consumed_per_swing_and_destroyed_at_zero() {
    // EQ-AMMO-2: durability IS the round count; one round per swing hit or
    // miss; the last round destroys the quiver and empties the hand.
    let atlas = real_atlas();
    let attacker = Equipment::empty()
        .with(EquipmentSlot::RightHand, item(&atlas, BOW))
        .with(EquipmentSlot::LeftHand, item_at(&atlas, ARROWS, 2));

    let mut rng = TestRng::new(1);
    let first = wear_from_strike(
        &AttackOutcome::Missed,
        attacker,
        Equipment::empty(),
        &atlas,
        &mut rng,
    );
    let quiver = or_abort(
        first
            .attacker_worn
            .get(EquipmentSlot::LeftHand)
            .ok_or("held"),
    );
    assert_eq!(
        quiver.durability.current(),
        1,
        "one round spent on the miss"
    );

    let second = wear_from_strike(
        &landed(100),
        first.attacker_worn,
        Equipment::empty(),
        &atlas,
        &mut rng,
    );
    assert_eq!(
        second.attacker_events[0],
        WearEvent::Destroyed {
            slot: EquipmentSlot::LeftHand
        }
    );
    assert!(
        second.attacker_worn.get(EquipmentSlot::LeftHand).is_none(),
        "the emptied quiver leaves the hand"
    );
}

#[test]
fn the_draw_order_is_defender_pool_then_attacker_pool() {
    // The fixed order pin: with both pools live, the defender pick consumes
    // the FIRST word — a twin stream replaying only the defender pick lands
    // on the same slot; the attacker pick consumes the second.
    let atlas = real_atlas();
    let attacker = Equipment::empty()
        .with(EquipmentSlot::RightHand, item(&atlas, KRIS))
        .with(EquipmentSlot::Pendant, item(&atlas, PENDANT_OF_LIGHTNING));
    for seed in 0..32u64 {
        let mut composed = TestRng::new(seed);
        let wear = wear_from_strike(
            &landed(2000),
            attacker.clone(),
            defensive_set(&atlas),
            &atlas,
            &mut composed,
        );
        // Replay: defender-only run on a twin stream picks the same slot.
        let mut twin = TestRng::new(seed);
        let defender_only = wear_from_strike(
            &landed(2000),
            Equipment::empty(),
            defensive_set(&atlas),
            &atlas,
            &mut twin,
        );
        assert_eq!(
            worn_slot(&wear.defender_events[0]).0,
            worn_slot(&defender_only.defender_events[0]).0,
            "the defender pick is the first word"
        );
    }
}

#[test]
fn same_seed_and_worn_sets_yield_byte_identical_wear() {
    // EQ-DET-2: replay bit-identity of the whole StrikeWear value.
    let atlas = real_atlas();
    let attacker = Equipment::empty()
        .with(EquipmentSlot::RightHand, item(&atlas, KRIS))
        .with(EquipmentSlot::Pendant, item(&atlas, PENDANT_OF_LIGHTNING));
    let mut first_rng = TestRng::new(42);
    let mut second_rng = TestRng::new(42);
    let first = wear_from_strike(
        &landed(3131),
        attacker.clone(),
        defensive_set(&atlas),
        &atlas,
        &mut first_rng,
    );
    let second = wear_from_strike(
        &landed(3131),
        attacker,
        defensive_set(&atlas),
        &atlas,
        &mut second_rng,
    );
    assert_eq!(first, second);
}

#[test]
fn resolve_strike_with_wear_composes_the_strike_then_the_wear() {
    // The core compose contract: one call == resolve_attack then
    // wear_from_strike on the SAME stream, in that order — so no host can
    // diverge the RNG by re-ordering the steps.
    let atlas = real_atlas();
    let attacker: Character = or_abort(serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": 50,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 200, "agility": 100, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    })));
    let defender: Character = or_abort(serde_json::from_value(serde_json::json!({
        "class": "dark_wizard",
        "level": 40,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 40, "agility": 40, "vitality": 60, "energy": 120},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    })));
    let attacker_profile = character_profile(&attacker).0;
    let defender_profile = character_profile(&defender).0;
    let attacker_worn = Equipment::empty().with(EquipmentSlot::RightHand, item(&atlas, KRIS));
    for seed in 0..64u64 {
        let mut composed = TestRng::new(seed);
        let (health, outcome, wear) = resolve_strike_with_wear(
            &attacker_profile,
            attacker_worn.clone(),
            &defender_profile,
            defensive_set(&atlas),
            Pool::full(1_000),
            &StrikeBasis::PlainSwing,
            &atlas,
            &mut composed,
        );
        let mut twin = TestRng::new(seed);
        let (twin_health, twin_outcome) = resolve_attack(
            &attacker_profile,
            &defender_profile,
            Pool::full(1_000),
            &StrikeBasis::PlainSwing,
            &mut twin,
        );
        let twin_wear = wear_from_strike(
            &twin_outcome,
            attacker_worn.clone(),
            defensive_set(&atlas),
            &atlas,
            &mut twin,
        );
        assert_eq!(health, twin_health);
        assert_eq!(outcome, twin_outcome);
        assert_eq!(wear, twin_wear);
    }
}
