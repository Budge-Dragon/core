//! Account progression (W-ACCOUNT) over the real `/data` class table: the core
//! [`unlock_classes_for_level`] and [`creation_verdict`] services proven against
//! the shipped `ClassTable`, so the 220/250 thresholds are read from data, never
//! literals. Covers the unlock crossings (below/at/multi-jump/idempotent/partial/
//! cap), the fresh-account gate over all eight roster classes, the
//! evolution-only and always-open precedence, and the wire round-trips.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body so
//! `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;

use mu_core::components::class::CharacterClass;
use mu_core::components::units::Level;
use mu_core::components::unlocked_classes::UnlockedClasses;
use mu_core::data::classes::ClassTable;
use mu_core::events::account::{ClassUnlocked, CreationVerdict};
use mu_core::services::account::{creation_verdict, unlock_classes_for_level};

use dataset::{or_abort, real_atlas};

fn level(value: u16) -> Level {
    or_abort(Level::new(value))
}

/// The real shipped class table.
fn classes() -> ClassTable {
    real_atlas().classes().clone()
}

/// Every roster class — the eight-class totality the gate must answer for.
const ROSTER: [CharacterClass; 8] = [
    CharacterClass::DarkWizard,
    CharacterClass::SoulMaster,
    CharacterClass::DarkKnight,
    CharacterClass::BladeKnight,
    CharacterClass::FairyElf,
    CharacterClass::MuseElf,
    CharacterClass::MagicGladiator,
    CharacterClass::DarkLord,
];

/// The classes announced by an unlock step, in the order emitted.
fn earned(events: &[ClassUnlocked]) -> Vec<CharacterClass> {
    events.iter().map(|event| event.class).collect()
}

// --- P2: unlock-on-level -----------------------------------------------------

#[test]
fn reaching_below_the_first_threshold_earns_nothing() {
    let classes = classes();
    let (set, events) = unlock_classes_for_level(UnlockedClasses::empty(), level(219), &classes);
    assert!(
        events.is_empty(),
        "219 is below the Magic Gladiator gate 220"
    );
    assert_eq!(set, UnlockedClasses::empty(), "the earned-set is unchanged");
}

#[test]
fn reaching_the_first_threshold_earns_magic_gladiator_only() {
    let classes = classes();
    let (set, events) = unlock_classes_for_level(UnlockedClasses::empty(), level(220), &classes);
    assert_eq!(earned(&events), vec![CharacterClass::MagicGladiator]);
    assert!(set.contains(CharacterClass::MagicGladiator));
    assert!(
        !set.contains(CharacterClass::DarkLord),
        "250 not yet reached"
    );
}

#[test]
fn reaching_the_second_threshold_earns_both_in_roster_order() {
    let classes = classes();
    let (set, events) = unlock_classes_for_level(UnlockedClasses::empty(), level(250), &classes);
    assert_eq!(
        earned(&events),
        vec![CharacterClass::MagicGladiator, CharacterClass::DarkLord],
        "roster order: Magic Gladiator precedes Dark Lord"
    );
    assert!(set.contains(CharacterClass::MagicGladiator));
    assert!(set.contains(CharacterClass::DarkLord));
}

#[test]
fn a_multi_level_jump_past_both_thresholds_earns_both_in_one_call() {
    let classes = classes();
    let (set, events) = unlock_classes_for_level(UnlockedClasses::empty(), level(251), &classes);
    assert_eq!(
        earned(&events),
        vec![CharacterClass::MagicGladiator, CharacterClass::DarkLord]
    );
    assert!(set.contains(CharacterClass::MagicGladiator));
    assert!(set.contains(CharacterClass::DarkLord));
}

#[test]
fn re_reaching_an_already_earned_threshold_announces_nothing() {
    let classes = classes();
    let held = UnlockedClasses::empty().unlocked(CharacterClass::MagicGladiator);
    let (set, events) = unlock_classes_for_level(held.clone(), level(220), &classes);
    assert!(events.is_empty(), "membership is the dedup authority");
    assert_eq!(set, held, "the earned-set is unchanged");
}

#[test]
fn crossing_the_second_threshold_with_the_first_held_earns_only_the_new_one() {
    let classes = classes();
    let held = UnlockedClasses::empty().unlocked(CharacterClass::MagicGladiator);
    let (set, events) = unlock_classes_for_level(held, level(250), &classes);
    assert_eq!(
        earned(&events),
        vec![CharacterClass::DarkLord],
        "Magic Gladiator is not announced again"
    );
    assert!(set.contains(CharacterClass::MagicGladiator));
    assert!(set.contains(CharacterClass::DarkLord));
}

#[test]
fn reaching_the_level_cap_earns_every_level_gated_class_and_no_other() {
    let atlas = real_atlas();
    let classes = atlas.classes();
    let cap = atlas.exp_curve().max_level();
    let (set, events) = unlock_classes_for_level(UnlockedClasses::empty(), cap, classes);
    // Both gated classes earned, and nothing always-open or evolution-only.
    assert_eq!(
        earned(&events),
        vec![CharacterClass::MagicGladiator, CharacterClass::DarkLord]
    );
    for class in ROSTER {
        let expected = matches!(
            class,
            CharacterClass::MagicGladiator | CharacterClass::DarkLord
        );
        assert_eq!(set.contains(class), expected, "{class:?} at the cap");
    }
}

#[test]
fn the_unlock_step_is_deterministic_and_draws_no_randomness() {
    let classes = classes();
    let (set_a, events_a) =
        unlock_classes_for_level(UnlockedClasses::empty(), level(251), &classes);
    let (set_b, events_b) =
        unlock_classes_for_level(UnlockedClasses::empty(), level(251), &classes);
    assert_eq!(set_a, set_b);
    assert_eq!(events_a, events_b);
}

#[test]
fn a_class_unlocked_event_round_trips_its_wire_form() {
    let event = ClassUnlocked {
        class: CharacterClass::DarkLord,
    };
    let wire = or_abort(serde_json::to_string(&event));
    assert_eq!(wire, r#"{"class":"dark_lord"}"#);
    assert_eq!(
        or_abort(serde_json::from_str::<ClassUnlocked>(&wire)),
        event
    );
}

// --- P3: the authoritative creation gate --------------------------------------

#[test]
fn a_fresh_account_may_create_only_the_always_open_classes() {
    let classes = classes();
    let fresh = UnlockedClasses::empty();
    assert_eq!(
        creation_verdict(CharacterClass::DarkWizard, &fresh, &classes),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkKnight, &fresh, &classes),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::FairyElf, &fresh, &classes),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::MagicGladiator, &fresh, &classes),
        CreationVerdict::Locked {
            required: level(220)
        }
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkLord, &fresh, &classes),
        CreationVerdict::Locked {
            required: level(250)
        }
    );
    for class in [
        CharacterClass::SoulMaster,
        CharacterClass::BladeKnight,
        CharacterClass::MuseElf,
    ] {
        assert_eq!(
            creation_verdict(class, &fresh, &classes),
            CreationVerdict::EvolutionOnly,
            "{class:?} is a second tier"
        );
    }
}

#[test]
fn the_gate_gives_a_total_verdict_for_every_roster_class() {
    let classes = classes();
    let fresh = UnlockedClasses::empty();
    // Every one of the eight classes gets a well-defined verdict.
    for class in ROSTER {
        let verdict = creation_verdict(class, &fresh, &classes);
        let expected = match class {
            CharacterClass::DarkWizard | CharacterClass::DarkKnight | CharacterClass::FairyElf => {
                CreationVerdict::Creatable
            }
            CharacterClass::SoulMaster | CharacterClass::BladeKnight | CharacterClass::MuseElf => {
                CreationVerdict::EvolutionOnly
            }
            CharacterClass::MagicGladiator => CreationVerdict::Locked {
                required: level(220),
            },
            CharacterClass::DarkLord => CreationVerdict::Locked {
                required: level(250),
            },
        };
        assert_eq!(verdict, expected, "{class:?}");
    }
}

#[test]
fn an_earned_level_gated_class_becomes_creatable() {
    let classes = classes();
    let held = UnlockedClasses::empty().unlocked(CharacterClass::MagicGladiator);
    assert_eq!(
        creation_verdict(CharacterClass::MagicGladiator, &held, &classes),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkLord, &held, &classes),
        CreationVerdict::Locked {
            required: level(250)
        },
        "Dark Lord stays locked until earned"
    );
}

#[test]
fn both_earned_means_both_creatable() {
    let classes = classes();
    let held = UnlockedClasses::empty()
        .unlocked(CharacterClass::MagicGladiator)
        .unlocked(CharacterClass::DarkLord);
    assert_eq!(
        creation_verdict(CharacterClass::MagicGladiator, &held, &classes),
        CreationVerdict::Creatable
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkLord, &held, &classes),
        CreationVerdict::Creatable
    );
}

#[test]
fn evolution_only_wins_over_any_earned_set_membership() {
    let classes = classes();
    // A stray/corrupt persisted membership names an evolution-only class; the
    // CreationGate is the primary discriminator, so the set is never consulted.
    let stray = UnlockedClasses::empty().unlocked(CharacterClass::SoulMaster);
    assert_eq!(
        creation_verdict(CharacterClass::SoulMaster, &stray, &classes),
        CreationVerdict::EvolutionOnly,
        "evolution-only is never creatable, whatever the set holds"
    );
}

#[test]
fn always_open_ignores_the_earned_set_entirely() {
    let classes = classes();
    let empty = UnlockedClasses::empty();
    assert_eq!(
        creation_verdict(CharacterClass::DarkWizard, &empty, &classes),
        CreationVerdict::Creatable
    );
    // An earned-set holding every roster class leaves an always-open verdict
    // unchanged — the gate never consults the set for an Always class.
    let mut full = UnlockedClasses::empty();
    for class in ROSTER {
        full = full.unlocked(class);
    }
    assert_eq!(
        creation_verdict(CharacterClass::DarkWizard, &full, &classes),
        CreationVerdict::Creatable
    );
}

#[test]
fn a_locked_verdict_surfaces_the_exact_required_level_from_data() {
    let classes = classes();
    let fresh = UnlockedClasses::empty();
    // The 220/250 thresholds are read from the shipped table, not literals here.
    assert_eq!(
        creation_verdict(CharacterClass::MagicGladiator, &fresh, &classes),
        CreationVerdict::Locked {
            required: level(220)
        }
    );
    assert_eq!(
        creation_verdict(CharacterClass::DarkLord, &fresh, &classes),
        CreationVerdict::Locked {
            required: level(250)
        }
    );
}

#[test]
fn each_creation_verdict_shape_round_trips_its_wire_form() {
    let cases = [
        (CreationVerdict::Creatable, r#"{"kind":"creatable"}"#),
        (
            CreationVerdict::Locked {
                required: level(220),
            },
            r#"{"kind":"locked","required":220}"#,
        ),
        (
            CreationVerdict::EvolutionOnly,
            r#"{"kind":"evolution_only"}"#,
        ),
    ];
    for (verdict, wire) in cases {
        assert_eq!(or_abort(serde_json::to_string(&verdict)), wire);
        assert_eq!(
            or_abort(serde_json::from_str::<CreationVerdict>(wire)),
            verdict
        );
    }
}
