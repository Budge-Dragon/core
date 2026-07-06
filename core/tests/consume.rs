//! Using consumables (W-CONSUME) over the real `/data` Atlas: the core
//! [`use_consumable`] service applied against the shipped group-14 records
//! (apple, HP/MP potions, antidote, alcohol, town portal). Proves the shared
//! percent-of-max + level-decaying-flat heal for every real tier, the unified
//! reject-when-no-op rule, the antidote cure, the out-of-scope refusal, the
//! stack decrement with last-piece removal, the wrong-cell / wrong-item / dead
//! refusals, and purity — each magnitude computed from the real max and the
//! authentic formula, never a hard-coded literal.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body
//! so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;

use mu_core::components::inventory::{Cell, Footprint, Inventory};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::units::ItemLevel;
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::events::consume::{ConsumeEvent, ConsumeRejection, PoolKind};
use mu_core::services::consume::use_consumable;

use dataset::{or_abort, real_atlas};

const APPLE: ItemRef = ItemRef {
    group: 14,
    number: 0,
};
const HP_SMALL: ItemRef = ItemRef {
    group: 14,
    number: 1,
};
const HP_MEDIUM: ItemRef = ItemRef {
    group: 14,
    number: 2,
};
const HP_LARGE: ItemRef = ItemRef {
    group: 14,
    number: 3,
};
const MP_SMALL: ItemRef = ItemRef {
    group: 14,
    number: 4,
};
const MP_MEDIUM: ItemRef = ItemRef {
    group: 14,
    number: 5,
};
const MP_LARGE: ItemRef = ItemRef {
    group: 14,
    number: 6,
};
const ANTIDOTE: ItemRef = ItemRef {
    group: 14,
    number: 8,
};
const ALCOHOL: ItemRef = ItemRef {
    group: 14,
    number: 9,
};
const TOWN_PORTAL: ItemRef = ItemRef {
    group: 14,
    number: 10,
};
/// A real weapon record (Short Sword 0/3) — a non-consumable at the cell.
const SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};

const CELL: Cell = Cell { row: 0, col: 0 };

/// The authentic recovery magnitude for a tier `multiplier` over the given pool
/// `max` and character `level` — the same integer formula the service computes,
/// evaluated here from the real max so no expected value is a hard-coded literal.
fn expected_recovery(max: u32, level: u16, multiplier: u32) -> u32 {
    let base_percent = multiplier * 10;
    let additional_base = (multiplier + 1) * 50;
    let scaled = max * base_percent / 100;
    let flat = additional_base.saturating_sub(u32::from(level));
    scaled + flat
}

/// A gearless Dark Knight at `level` with the given health and mana pools —
/// built the only way a character can be, by deserialising its wire form.
fn knight(level: u16, hp_current: u32, hp_max: u32, mp_current: u32, mp_max: u32) -> Character {
    or_abort(serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": level,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": hp_current, "max": hp_max},
            "mana": {"current": mp_current, "max": mp_max},
            "ability": {"current": 1, "max": 1}
        }
    })))
}

/// A level-30 Dark Knight below full health, carrying an active poison and a
/// Greater-Damage buff — the antidote-cure subject.
fn poisoned_buffed_knight() -> Character {
    or_abort(serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": 30,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 200, "max": 400},
            "mana": {"current": 200, "max": 400},
            "ability": {"current": 1, "max": 1}
        },
        "active_effects": [
            {"kind": "greater_damage", "amount": 50, "expiry": 900},
            {"kind": "poisoned", "per_tick_damage": 12, "remaining": 6, "next_tick": 60, "cadence": 60}
        ]
    })))
}

/// A dead Dark Knight awaiting respawn, health at zero.
fn dead_knight() -> Character {
    or_abort(serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": 30,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 0, "max": 400},
            "mana": {"current": 200, "max": 400},
            "ability": {"current": 1, "max": 1}
        },
        "life": {"kind": "dead", "respawn_at": 903}
    })))
}

/// A real item instance of `id` carrying `pieces` in its gauge (the stack count),
/// its ceiling the record's own durability column.
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

// --- HP potions: the shared percent-of-max + level-decaying flat formula. -----

#[test]
fn a_real_small_hp_potion_heals_ten_percent_of_max_plus_the_flat_term() {
    let atlas = real_atlas();
    let character = knight(30, 40, 400, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_SMALL, 3), CELL, &atlas);

    let expected = expected_recovery(400, 30, 1);
    assert_eq!(
        events,
        vec![ConsumeEvent::Recovered {
            pool: PoolKind::Health,
            restored: expected,
        }]
    );
    assert_eq!(healed.vitals().health.current(), 40 + expected);
    assert_eq!(healed.vitals().health.max(), 400);
    // The mana pool is untouched by a health potion.
    assert_eq!(healed.vitals().mana.current(), 400);
    // One piece was consumed; the rest of the stack rides back whole.
    assert_eq!(pieces_at(&bag), Some(2));
}

#[test]
fn real_medium_and_large_hp_potions_heal_by_their_tier() {
    let atlas = real_atlas();

    let character = knight(30, 100, 500, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_MEDIUM, 3), CELL, &atlas);
    let medium = expected_recovery(500, 30, 2);
    assert_eq!(
        events,
        vec![ConsumeEvent::Recovered {
            pool: PoolKind::Health,
            restored: medium
        }]
    );
    assert_eq!(healed.vitals().health.current(), 100 + medium);
    assert_eq!(pieces_at(&bag), Some(2));

    let character = knight(60, 100, 1000, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_LARGE, 3), CELL, &atlas);
    let large = expected_recovery(1000, 60, 3);
    assert_eq!(
        events,
        vec![ConsumeEvent::Recovered {
            pool: PoolKind::Health,
            restored: large
        }]
    );
    assert_eq!(healed.vitals().health.current(), 100 + large);
    assert_eq!(pieces_at(&bag), Some(2));
}

// --- MP potions: the same formula on the mana pool. ---------------------------

#[test]
fn real_mp_potions_restore_mana_and_leave_health_untouched() {
    let atlas = real_atlas();
    for (id, multiplier) in [(MP_SMALL, 1u32), (MP_MEDIUM, 2), (MP_LARGE, 3)] {
        // A deep mana pool with room to spare, so every tier's full amount lands
        // (no cap) and the asserted delta is the whole computed recovery.
        let character = knight(30, 300, 500, 10, 400);
        let (healed, bag, events) =
            use_consumable(&character, bag_with(&atlas, id, 3), CELL, &atlas);
        let expected = expected_recovery(400, 30, multiplier);
        assert_eq!(
            events,
            vec![ConsumeEvent::Recovered {
                pool: PoolKind::Mana,
                restored: expected
            }],
            "tier multiplier {multiplier} restores mana"
        );
        assert_eq!(healed.vitals().mana.current(), 10 + expected);
        // The health pool is untouched by a mana potion.
        assert_eq!(healed.vitals().health.current(), 300);
        assert_eq!(pieces_at(&bag), Some(2));
    }
}

// --- Apple: the tiny flat heal that decays to nothing. ------------------------

#[test]
fn a_real_apple_heals_only_its_flat_sip_at_a_low_level() {
    let atlas = real_atlas();
    let character = knight(10, 100, 400, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, APPLE, 3), CELL, &atlas);

    let expected = expected_recovery(400, 10, 0);
    assert_eq!(
        expected, 40,
        "an apple at level 10 heals its 40-point flat sip"
    );
    assert_eq!(
        events,
        vec![ConsumeEvent::Recovered {
            pool: PoolKind::Health,
            restored: expected
        }]
    );
    assert_eq!(healed.vitals().health.current(), 140);
    assert_eq!(pieces_at(&bag), Some(2));
}

#[test]
fn a_real_apple_past_level_fifty_heals_nothing_and_is_refused_consuming_nothing() {
    let atlas = real_atlas();
    // Not full — proves the refusal is the zero-magnitude rule, not a full pool.
    let character = knight(60, 500, 1000, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, APPLE, 3), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NoEffect
        }]
    );
    assert_eq!(healed.vitals().health.current(), 500, "health is unchanged");
    // Never invents a bigger heal and never wastes the apple.
    assert_eq!(pieces_at(&bag), Some(3));
}

// --- Cap-at-max: the near-full delta reaches exactly full. --------------------

#[test]
fn a_real_potion_near_full_caps_at_max_and_reports_only_the_delta() {
    let atlas = real_atlas();
    let character = knight(30, 395, 400, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_MEDIUM, 3), CELL, &atlas);

    // The computed amount over-caps; the event carries the actual gain to full.
    assert_eq!(
        events,
        vec![ConsumeEvent::Recovered {
            pool: PoolKind::Health,
            restored: 5
        }]
    );
    assert_eq!(
        healed.vitals().health.current(),
        400,
        "reaches exactly full"
    );
    assert_eq!(pieces_at(&bag), Some(2));
}

// --- Reject-when-no-op: a full pool. ------------------------------------------

#[test]
fn a_real_potion_at_full_hp_is_refused_consuming_nothing() {
    let atlas = real_atlas();
    let character = knight(30, 400, 400, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_SMALL, 3), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NoEffect
        }]
    );
    assert_eq!(healed.vitals().health.current(), 400);
    assert_eq!(pieces_at(&bag), Some(3), "nothing consumed");
}

#[test]
fn a_real_mp_potion_at_full_mana_is_refused_even_with_low_health() {
    let atlas = real_atlas();
    // Full mana, health well below full — an MP potion reads only the mana pool.
    let character = knight(30, 40, 400, 200, 200);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, MP_MEDIUM, 3), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NoEffect
        }]
    );
    assert_eq!(healed.vitals().mana.current(), 200);
    assert_eq!(healed.vitals().health.current(), 40);
    assert_eq!(pieces_at(&bag), Some(3));
}

// --- Antidote: cure only. -----------------------------------------------------

#[test]
fn a_real_antidote_cures_poison_and_leaves_health_and_other_effects_intact() {
    let atlas = real_atlas();
    let character = poisoned_buffed_knight();
    let (cured, bag, events) =
        use_consumable(&character, bag_with(&atlas, ANTIDOTE, 3), CELL, &atlas);

    assert_eq!(events, vec![ConsumeEvent::PoisonCured]);
    assert!(
        cured.active_effects().poison().is_none(),
        "the poison is cleared"
    );
    assert!(
        cured.active_effects().greater_damage().is_some(),
        "the Greater-Damage buff still stands"
    );
    // An antidote is not a heal.
    assert_eq!(cured.vitals().health.current(), 200);
    assert_eq!(pieces_at(&bag), Some(2));
}

#[test]
fn a_real_antidote_with_no_poison_is_refused_consuming_nothing() {
    let atlas = real_atlas();
    let character = knight(30, 200, 400, 400, 400);
    let (cured, bag, events) =
        use_consumable(&character, bag_with(&atlas, ANTIDOTE, 3), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NoEffect
        }]
    );
    assert_eq!(cured.active_effects(), character.active_effects());
    assert_eq!(pieces_at(&bag), Some(3));
}

// --- Out-of-scope consumables: alcohol and town portal. -----------------------

#[test]
fn real_out_of_scope_consumables_are_refused_not_recoverable_consuming_nothing() {
    let atlas = real_atlas();
    for id in [ALCOHOL, TOWN_PORTAL] {
        let character = knight(30, 40, 400, 40, 400);
        let (unchanged, bag, events) =
            use_consumable(&character, bag_with(&atlas, id, 1), CELL, &atlas);
        assert_eq!(
            events,
            vec![ConsumeEvent::Rejected {
                reason: ConsumeRejection::NotRecoverable
            }],
            "item {id:?} is out of this service's recovery/cure scope"
        );
        assert_eq!(unchanged.vitals().health.current(), 40);
        assert_eq!(pieces_at(&bag), Some(1), "nothing consumed");
    }
}

// --- Wrong item / wrong cell / dead. ------------------------------------------

#[test]
fn a_real_non_consumable_is_refused_not_consumable() {
    let atlas = real_atlas();
    let character = knight(30, 40, 400, 400, 400);
    let (unchanged, bag, events) =
        use_consumable(&character, bag_with(&atlas, SWORD, 1), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NotConsumable
        }]
    );
    assert_eq!(unchanged.vitals().health.current(), 40);
    // The sword rides back whole.
    assert!(bag.occupant(CELL).is_some());
}

#[test]
fn an_empty_cell_is_refused_no_item() {
    let atlas = real_atlas();
    let character = knight(30, 40, 400, 400, 400);
    let (unchanged, bag, events) = use_consumable(&character, Inventory::empty(8, 8), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NoItem
        }]
    );
    assert_eq!(unchanged.vitals().health.current(), 40);
    assert!(bag.placed().is_empty());
}

#[test]
fn a_dead_character_is_refused_not_alive_over_real_data() {
    let atlas = real_atlas();
    let character = dead_knight();
    let (unchanged, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_SMALL, 3), CELL, &atlas);

    assert_eq!(
        events,
        vec![ConsumeEvent::Rejected {
            reason: ConsumeRejection::NotAlive
        }]
    );
    assert_eq!(
        unchanged.vitals().health.current(),
        0,
        "no heal on a corpse"
    );
    assert_eq!(pieces_at(&bag), Some(3), "nothing consumed");
}

// --- Consume-one: decrement to empty across repeated drinks. ------------------

#[test]
fn a_real_three_stack_decrements_to_two_then_one_then_empties_on_the_last_drink() {
    let atlas = real_atlas();
    // A low current health so every drink heals (never a full-pool refusal).
    let mut character = knight(30, 40, 4000, 400, 400);
    let mut bag = bag_with(&atlas, HP_SMALL, 3);

    for expected_left in [2u8, 1, 0] {
        let (healed, next_bag, events) = use_consumable(&character, bag, CELL, &atlas);
        assert!(
            matches!(
                events.as_slice(),
                [ConsumeEvent::Recovered {
                    pool: PoolKind::Health,
                    ..
                }]
            ),
            "each drink recovers health"
        );
        character = healed;
        bag = next_bag;
        match expected_left {
            0 => assert_eq!(
                pieces_at(&bag),
                None,
                "the last drink empties the cell — no ghost"
            ),
            left => assert_eq!(pieces_at(&bag), Some(left)),
        }
    }
    assert!(bag.placed().is_empty(), "no zero-count stack lingers");
}

// --- The full loop + persist round-trip. --------------------------------------

#[test]
fn a_hurt_knight_drinks_a_real_potion_heals_and_the_character_round_trips() {
    let atlas = real_atlas();
    let character = knight(30, 40, 400, 400, 400);
    let (healed, bag, events) =
        use_consumable(&character, bag_with(&atlas, HP_SMALL, 3), CELL, &atlas);

    let expected = expected_recovery(400, 30, 1);
    assert_eq!(
        events,
        vec![ConsumeEvent::Recovered {
            pool: PoolKind::Health,
            restored: expected
        }]
    );
    assert!(healed.vitals().health.current() > 40, "health rose");
    assert_eq!(pieces_at(&bag), Some(2), "one potion left the stack");
    // The healed character re-parses to itself — the class↔stats gate re-proves.
    let wire = or_abort(serde_json::to_string(&healed));
    assert_eq!(or_abort(serde_json::from_str::<Character>(&wire)), healed);
}

// --- Purity / determinism: no RNG, no in-place edit. --------------------------

#[test]
fn use_consumable_is_a_pure_deterministic_transition() {
    let atlas = real_atlas();
    let character = knight(30, 40, 400, 400, 400);
    let before = character.clone();

    let first = use_consumable(&character, bag_with(&atlas, HP_SMALL, 3), CELL, &atlas);
    let second = use_consumable(&character, bag_with(&atlas, HP_SMALL, 3), CELL, &atlas);

    assert_eq!(first, second, "identical inputs yield an identical triple");
    // The borrowed character is never mutated in place.
    assert_eq!(character, before);
}
