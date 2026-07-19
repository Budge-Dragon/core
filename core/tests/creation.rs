//! Character creation (W-CREATE) over the real `/data` Atlas: the core
//! [`create_character`] service exercised against the shipped class table, item
//! catalog, and terrain. Proves the class starting stats, the zero-progression
//! clean initial state, the full vitals at the class-formula maxima (and their
//! byte-parity with a level-1 respawn), the home-town walkable landing, the
//! single RNG draw and determinism, the authored worn starter kit (plain, level
//! 0, full base durability), and that each starter item matches its AUTHENTIC
//! equip-gate verdict — with the class-qualification columns sourced from the
//! Season 6 item data, every authored starter item is class-eligible for its
//! class and clears the level-1 wear bar, so the gate accepts the whole kit, the
//! same verdict the by-construction direct-seat path produces (OpenMU's
//! `AddInitialItem` plugins seat straight into the slot).
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body so
//! `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use mu_core::rand_core::RngCore;

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::class::CharacterClass;
use mu_core::components::discovered_maps::DiscoveredMaps;
use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::item_instance::{CraftedAugment, LuckRoll, RarityRoll, SkillRoll};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::life::LifeState;
use mu_core::components::movement::Movement;
use mu_core::components::reputation::Reputation;
use mu_core::components::stats::Stats;
use mu_core::components::units::{CarriedZen, Exp, ItemLevel, Level, MapNumber};
use mu_core::data::atlas::{Atlas, AtlasError};
use mu_core::entities::character::Character;
use mu_core::events::inventory::EquipOutcome;
use mu_core::services::creation::{CreatedCharacter, create_character};
use mu_core::services::death::respawn;
use mu_core::services::inventory::{Wearer, equip};
use mu_core::services::movement::resolve_spawn_gate_landing;
use mu_core::services::profile::character_profile;

use dataset::{or_abort, real_atlas, real_static_data};
use rng::TestRng;

/// An arbitrary fixed stream every scenario shares.
const SEED: u64 = 7;

/// The five classes a player can create — the roster `create_character` is
/// called for (second tiers are evolution-only and never created).
const CREATABLE: [CharacterClass; 5] = [
    CharacterClass::DarkWizard,
    CharacterClass::DarkKnight,
    CharacterClass::FairyElf,
    CharacterClass::MagicGladiator,
    CharacterClass::DarkLord,
];

/// The full eight-class roster — the crate-internal `CharacterClass::ALL` is not
/// exported, so the totality proof lists the public variants here.
const ALL_CLASSES: [CharacterClass; 8] = [
    CharacterClass::DarkWizard,
    CharacterClass::SoulMaster,
    CharacterClass::DarkKnight,
    CharacterClass::BladeKnight,
    CharacterClass::FairyElf,
    CharacterClass::MuseElf,
    CharacterClass::MagicGladiator,
    CharacterClass::DarkLord,
];

/// The twelve worn slots — the crate-internal `EquipmentSlot::ALL` is not
/// exported, so the worn-set scans list the public variants here.
const ALL_SLOTS: [EquipmentSlot; 12] = [
    EquipmentSlot::LeftHand,
    EquipmentSlot::RightHand,
    EquipmentSlot::Helm,
    EquipmentSlot::Armor,
    EquipmentSlot::Pants,
    EquipmentSlot::Gloves,
    EquipmentSlot::Boots,
    EquipmentSlot::Wings,
    EquipmentSlot::Pet,
    EquipmentSlot::Pendant,
    EquipmentSlot::Ring1,
    EquipmentSlot::Ring2,
];

/// Creates a fresh character of `class` over the real atlas on a `seed` stream.
fn create(atlas: &Atlas, class: CharacterClass, seed: u64) -> CreatedCharacter {
    create_character(class, atlas, &mut TestRng::new(seed))
}

/// The authored worn starter kit each creatable class ships with, as
/// `(item, slot)` pairs in authored order — the authentic per-class set.
fn expected_kit(class: CharacterClass) -> Vec<(ItemRef, EquipmentSlot)> {
    let small_axe = ItemRef {
        group: 1,
        number: 0,
    };
    let short_bow = ItemRef {
        group: 4,
        number: 0,
    };
    let arrows = ItemRef {
        group: 4,
        number: 15,
    };
    let short_sword = ItemRef {
        group: 0,
        number: 1,
    };
    let small_shield = ItemRef {
        group: 6,
        number: 0,
    };
    match class {
        CharacterClass::DarkWizard => Vec::new(),
        CharacterClass::DarkKnight => vec![(small_axe, EquipmentSlot::LeftHand)],
        CharacterClass::FairyElf => vec![
            (arrows, EquipmentSlot::LeftHand),
            (short_bow, EquipmentSlot::RightHand),
        ],
        CharacterClass::MagicGladiator | CharacterClass::DarkLord => vec![
            (short_sword, EquipmentSlot::LeftHand),
            (small_shield, EquipmentSlot::RightHand),
        ],
        CharacterClass::SoulMaster | CharacterClass::BladeKnight | CharacterClass::MuseElf => {
            Vec::new()
        }
    }
}

/// The authentic home town of each creatable class (Fairy Elf on Noria, the
/// rest on Lorencia).
fn home_map(class: CharacterClass) -> MapNumber {
    match class {
        CharacterClass::FairyElf => MapNumber(3),
        CharacterClass::DarkWizard
        | CharacterClass::DarkKnight
        | CharacterClass::MagicGladiator
        | CharacterClass::DarkLord
        | CharacterClass::SoulMaster
        | CharacterClass::BladeKnight
        | CharacterClass::MuseElf => MapNumber(0),
    }
}

#[test]
fn each_creatable_class_carries_its_record_starting_stats() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let record = atlas.classes().record(class);
        assert_eq!(
            created.character.stats(),
            Stats::from(record.starting_stats),
            "{class:?} stats"
        );
        assert_eq!(created.character.class(), class, "{class:?} class");
    }
}

#[test]
fn only_dark_lord_gets_with_command_stats_every_other_gets_standard() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let is_command = matches!(created.character.stats(), Stats::WithCommand { .. });
        assert_eq!(
            is_command,
            class == CharacterClass::DarkLord,
            "{class:?} command-ness"
        );
    }
}

#[test]
fn a_fresh_character_has_no_progression_and_a_clean_initial_state() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let c = &created.character;
        assert_eq!(c.level(), Level::MIN, "{class:?} level");
        assert_eq!(c.experience(), Exp::ZERO, "{class:?} exp");
        assert_eq!(c.unspent_points(), 0, "{class:?} points");
        assert_eq!(c.zen(), CarriedZen::ZERO, "{class:?} zen");
        assert_eq!(
            c.active_effects(),
            ActiveEffects::EMPTY,
            "{class:?} effects"
        );
        assert_eq!(c.life(), LifeState::Alive, "{class:?} life");
        assert_eq!(c.reputation(), Reputation::clean(), "{class:?} reputation");
        assert_eq!(
            *c.discovered(),
            DiscoveredMaps::single(home_map(class)),
            "{class:?} discovered is exactly the home map"
        );
    }
}

#[test]
fn vitals_are_full_pools_at_the_class_formula_maxima() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let (_profile, maxima) = character_profile(&created.character);
        let vitals = created.character.vitals();
        // Each pool is full (current == max) at the class-formula capacity.
        assert_eq!(
            vitals.health.current(),
            vitals.health.max(),
            "{class:?} hp full"
        );
        assert_eq!(
            vitals.mana.current(),
            vitals.mana.max(),
            "{class:?} mp full"
        );
        assert_eq!(
            vitals.ability.current(),
            vitals.ability.max(),
            "{class:?} ag full"
        );
        assert_eq!(
            vitals.health.max(),
            maxima.max_health,
            "{class:?} hp maxima"
        );
        assert_eq!(vitals.mana.max(), maxima.max_mana, "{class:?} mp maxima");
        assert_eq!(
            vitals.ability.max(),
            maxima.max_ability,
            "{class:?} ag maxima"
        );
    }
}

#[test]
fn fresh_vitals_are_byte_identical_to_a_level_one_respawn() {
    // The single-source-of-truth guarantee: creation and respawn both seed
    // Pool::full at the character_profile maxima. A fresh character marked Dead
    // and respawned lands on the exact same vitals.
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let mut value = or_abort(serde_json::to_value(&created.character));
        value["life"] = serde_json::json!({"kind": "dead", "respawn_at": 0});
        let dead: Character = or_abort(serde_json::from_value(value));
        let (revived, _) = respawn(dead, &atlas, &mut TestRng::new(99));
        assert_eq!(
            revived.vitals(),
            created.character.vitals(),
            "{class:?} respawn vitals parity"
        );
    }
}

#[test]
fn a_fresh_character_lands_on_its_home_town_walkable_gate_grounded() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let placement = created.character.placement();
        assert_eq!(placement.map, home_map(class), "{class:?} home map");
        assert_eq!(placement.movement, Movement::Grounded, "{class:?} grounded");

        // The landing is one of the home town gate's parse-proven walkable tiles.
        let (gate, _env) = or_abort(
            atlas
                .town_gate_for_map(home_map(class))
                .ok_or("home map owns a town gate"),
        );
        assert!(
            gate.landing.iter().any(|&tile| tile == placement.position),
            "{class:?} landed inside the gate's walkable set"
        );
        // The gate's authored facing (or the movement default) is what it wears.
        if let Some(authored) = gate.facing {
            assert_eq!(placement.facing, authored, "{class:?} gate facing");
        }
    }
}

#[test]
fn create_character_draws_exactly_one_random_word() {
    // The landing pick is the only randomness — the respawn/spawn single-draw
    // contract. Running create on a fresh stream and taking the same landing
    // directly on a twin stream leaves both at the same position.
    let atlas = real_atlas();
    let mut created_stream = TestRng::new(SEED);
    let created = create_character(CharacterClass::DarkKnight, &atlas, &mut created_stream);

    let mut manual_stream = TestRng::new(SEED);
    let record = atlas.classes().record(CharacterClass::DarkKnight);
    let (gate, env) = match atlas.town_gate_for_map(record.home_map) {
        Some(destination) => destination,
        None => atlas.fallback_town_gate(),
    };
    let manual = resolve_spawn_gate_landing(gate, env, &mut manual_stream);

    assert_eq!(created.character.placement(), manual, "same landing");
    // Both streams stand at the same position afterward — one word each.
    assert_eq!(
        created_stream.next_u64(),
        manual_stream.next_u64(),
        "create advanced the stream by exactly one landing draw"
    );
}

#[test]
fn the_same_seed_produces_an_identical_bundle() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let first = create(&atlas, class, SEED);
        let second = create(&atlas, class, SEED);
        assert_eq!(first, second, "{class:?} determinism");
    }
}

#[test]
fn two_seeds_land_a_dark_knight_on_walkable_lorencia_tiles() {
    let atlas = real_atlas();
    let a = create(&atlas, CharacterClass::DarkKnight, SEED);
    let b = create(&atlas, CharacterClass::DarkKnight, 999);
    assert_eq!(a.character.placement().map, MapNumber(0));
    assert_eq!(b.character.placement().map, MapNumber(0));
    let (gate, _env) = or_abort(
        atlas
            .town_gate_for_map(MapNumber(0))
            .ok_or("lorencia owns a town gate"),
    );
    for created in [&a, &b] {
        assert!(
            gate.landing
                .iter()
                .any(|&tile| tile == created.character.placement().position),
        );
    }
}

#[test]
fn each_class_wears_its_authored_starter_kit() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let worn: Vec<(ItemRef, EquipmentSlot)> = ALL_SLOTS
            .iter()
            .filter_map(|&slot| created.equipment.get(slot).map(|item| (item.item, slot)))
            .collect();
        assert_eq!(worn, expected_kit(class), "{class:?} worn kit");
    }
}

#[test]
fn a_fresh_dark_wizard_wears_nothing() {
    let atlas = real_atlas();
    let created = create(&atlas, CharacterClass::DarkWizard, SEED);
    for slot in ALL_SLOTS {
        assert!(created.equipment.get(slot).is_none(), "{slot:?} is empty");
    }
}

#[test]
fn every_worn_starter_item_is_plain_level_zero_and_at_full_base_durability() {
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        for (item, slot) in expected_kit(class) {
            let worn = or_abort(created.equipment.get(slot).ok_or("worn slot occupied"));
            let def = or_abort(atlas.item(item).ok_or("kit item defined"));
            assert_eq!(worn.item, item, "{class:?} {slot:?} identity");
            assert_eq!(worn.level, ItemLevel::ZERO, "{class:?} {slot:?} level 0");
            assert!(
                matches!(worn.roll, RarityRoll::Normal),
                "{class:?} plain roll"
            );
            assert!(worn.normal_option.is_none(), "{class:?} no option");
            assert!(matches!(worn.luck, LuckRoll::Plain), "{class:?} no luck");
            assert!(
                matches!(worn.skill, SkillRoll::NoSkill),
                "{class:?} no skill"
            );
            assert!(
                matches!(worn.augment, CraftedAugment::None),
                "{class:?} no augment"
            );
            assert_eq!(
                worn.durability.current(),
                worn.durability.max(),
                "{class:?} {slot:?} full gauge"
            );
            assert_eq!(
                worn.durability.max(),
                def.durability,
                "{class:?} {slot:?} at its own definition's durability"
            );
        }
    }
}

#[test]
fn the_starter_arrows_carry_the_full_ammunition_round_count() {
    let atlas = real_atlas();
    let created = create(&atlas, CharacterClass::FairyElf, SEED);
    let arrows = or_abort(
        created
            .equipment
            .get(EquipmentSlot::LeftHand)
            .ok_or("elf carries arrows in the left hand"),
    );
    assert_eq!(
        arrows.item,
        ItemRef {
            group: 4,
            number: 15
        }
    );
    // def.durability for arrows is the round count (255).
    assert_eq!(arrows.durability.current(), 255);
    assert_eq!(arrows.durability.max(), 255);
}

#[test]
fn each_starter_kit_matches_the_authentic_equip_gate_verdict() {
    // Creation seats the kit BY CONSTRUCTION, never through the equip gate —
    // exactly as OpenMU's `AddInitialItem` plugins seat starter gear straight into
    // the inventory slot, bypassing `QualifiedCharacters`. This is the independent
    // proof that the direct-seat path and the equip-gate path agree on the
    // AUTHENTIC verdict: with class qualification sourced from the Season 6 item
    // files, every class's whole authored kit is class-eligible AND clears the
    // level-1 wear bar, so the gate accepts every item. In particular the Season 6
    // Small Shield (`CreateShield(0, …, 1, 1, 1, 1, 1, 0, 0)`) admits Magic
    // Gladiator and Dark Lord, and the Short Sword (`…, 1, 1, 1, 1, 1, …`) admits
    // Dark Lord — so those seatings, once thought force-seated past a mismatch, are
    // genuinely gate-eligible. A mis-authored kit — an item a level-1 character
    // could not wield — flips a verdict and fails here.
    let atlas = real_atlas();
    for class in CREATABLE {
        let created = create(&atlas, class, SEED);
        let wearer = Wearer {
            class,
            level: created.character.level(),
            stats: created.character.stats(),
        };
        for entry in atlas.starting_kit(class).iter() {
            let def = or_abort(
                atlas
                    .item(entry.item_instance.item)
                    .ok_or("resolved kit item is defined"),
            );
            let (_worn, outcome) = equip(
                Equipment::empty(),
                entry.item_instance.clone(),
                def,
                entry.slot,
                &atlas,
                &wearer,
            );
            assert!(
                matches!(outcome, EquipOutcome::Equipped { .. }),
                "{class:?} {:?} is eligible on a fresh level-1 character, got {outcome:?}",
                entry.slot
            );
        }
    }
}

#[test]
fn the_shipped_class_records_carry_the_authored_starting_kit() {
    // The authored data — not a fabricated fixture — is what ships.
    let atlas = real_atlas();
    for class in CREATABLE {
        let record = atlas.classes().record(class);
        let authored: Vec<(ItemRef, EquipmentSlot)> = record
            .starting_kit
            .iter()
            .map(|entry| (entry.item, entry.slot))
            .collect();
        assert_eq!(authored, expected_kit(class), "{class:?} record kit");
        // Every authored entry is a plus-0 item.
        for entry in record.starting_kit.iter() {
            assert_eq!(
                entry.item_level,
                ItemLevel::ZERO,
                "{class:?} authored level 0"
            );
        }
    }
}

#[test]
fn a_starter_kit_naming_no_item_fails_atlas_load() {
    // The FK-resolution proof: a kit reference to a nonexistent item is a load
    // failure, never a runtime absence.
    let mut data = real_static_data();
    let mut value = or_abort(serde_json::to_value(&data.classes));
    // Corrupt the Dark Knight record (index 2 in roster order) to name an item
    // that no definition carries.
    value["records"][2]["starting_kit"] = serde_json::json!([{
        "item": {"group": 99, "number": 999},
        "item_level": 0,
        "slot": "left_hand"
    }]);
    data.classes = or_abort(serde_json::from_value(value));

    let err = Atlas::parse(data).unwrap_err();
    assert!(
        matches!(
            err,
            AtlasError::StartingKitItemMissing {
                class: CharacterClass::DarkKnight,
                item: ItemRef {
                    group: 99,
                    number: 999
                }
            }
        ),
        "expected StartingKitItemMissing, got {err:?}"
    );
}

#[test]
fn the_real_atlas_resolves_every_class_starter_kit() {
    // The positive of the FK check: the shipped dataset's kits all resolve, so
    // every per-class accessor is total.
    let atlas = real_atlas();
    for class in ALL_CLASSES {
        // A total accessor — never panics, never an Option.
        let kit = atlas.starting_kit(class);
        for entry in kit.iter() {
            assert!(
                atlas.item(entry.item_instance.item).is_some(),
                "{class:?} kit item resolves"
            );
        }
    }
}
