//! The equip eligibility gate over the real `/data` Atlas (W-EQUIP §3.2):
//! class list-contains and the scaled wear requirement
//! `(mult × effective_drop_level × base)/100 + 20` proven against real item
//! definitions through the public `equip` port — the class refusal both ways,
//! the exact inclusive bar, the rarity/enhance drop-level surcharge, the
//! strength +4-per-option-level term, the MG cross-class lists, the raw level
//! compare, and the zero-column skip. Every expected bar is hand-derived in a
//! comment from the real definition's columns.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]`
//! body so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;

use dataset::{or_abort, real_atlas};
use mu_core::components::class::CharacterClass;
use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ExcellentOptions, ExcellentWeaponSet, ItemInstance, LuckRoll,
    RarityRoll, RolledNormalOption, SkillRoll,
};
use mu_core::components::item_options::{ExcellentWeaponOption, NormalOption};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::levels::OptionLevel;
use mu_core::components::stats::Stats;
use mu_core::components::units::{ItemLevel, Level};
use mu_core::data::atlas::Atlas;
use mu_core::events::inventory::{EquipOutcome, EquipRejection};
use mu_core::services::inventory::{Wearer, equip};

/// Short Bow — elf-only classes, wear str 20 / agi 80 at drop level 2.
const ELF_BOW: ItemRef = ItemRef {
    group: 4,
    number: 0,
};
/// Kris — wear strength 60 at drop level 3; classes include the Dark Knight.
const KRIS: ItemRef = ItemRef {
    group: 0,
    number: 1,
};
/// Short Sword — DK/BK/MG classes, wear str 80 / agi 40 at drop level 16.
const SHORT_SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};
/// Bronze Armor — DK/BK/MG classes, wear str 80 / agi 20 at drop level 18.
const BRONZE_ARMOR: ItemRef = ItemRef {
    group: 8,
    number: 0,
};
/// Wings of Satan — DK/BK/MG, wear LEVEL 180 with every stat column 0.
const WINGS_OF_SATAN: ItemRef = ItemRef {
    group: 12,
    number: 2,
};
/// Short Sword (0/1) — the Season 6 flags qualify every roster class including
/// Magic Gladiator and Dark Lord; wear strength 60 at drop level 3.
const STARTER_SHORT_SWORD: ItemRef = ItemRef {
    group: 0,
    number: 1,
};
/// Small Shield (6/0) — Season 6 `CreateShield(0, …, 1, 1, 1, 1, 1, 0, 0)`
/// qualifies DW/DK/Elf/MG/DL; wear strength 70 at drop level 3.
const STARTER_SMALL_SHIELD: ItemRef = ItemRef {
    group: 6,
    number: 0,
};
/// Skull Staff (5/0) — a group-5 staff stays wizard/MG-side; the Season 6 flags
/// mark it darkWizard + magicGladiator, never darkLord.
const SKULL_STAFF: ItemRef = ItemRef {
    group: 5,
    number: 0,
};

/// A fresh Normal instance of real item `id` at plus-`level`.
fn item_at(atlas: &Atlas, id: ItemRef, level: u8) -> ItemInstance {
    let def = or_abort(atlas.item(id).ok_or("unknown item"));
    ItemInstance {
        item: id,
        level: or_abort(ItemLevel::new(level)),
        roll: RarityRoll::Normal,
        normal_option: None,
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(def.durability),
        augment: CraftedAugment::None,
    }
}

/// A wearer view with the exact totals a hand-derived bar is compared to.
fn wearer(class: CharacterClass, level: u16, strength: u16, agility: u16) -> Wearer {
    Wearer {
        class,
        level: or_abort(Level::new(level)),
        stats: Stats::Standard {
            strength,
            agility,
            vitality: 50,
            energy: 30,
        },
    }
}

/// Drives one equip of `item` into `slot` for `who` over an empty worn set and
/// returns the outcome.
fn try_equip(atlas: &Atlas, item: ItemInstance, slot: EquipmentSlot, who: &Wearer) -> EquipOutcome {
    let def = or_abort(atlas.item(item.item).ok_or("unknown item"));
    let (_, outcome) = equip(Equipment::empty(), item, def, slot, atlas, who);
    outcome
}

/// Asserts the outcome is a rejection for `reason`.
fn assert_rejected(outcome: &EquipOutcome, reason: EquipRejection) {
    match outcome {
        EquipOutcome::Rejected { reason: got, .. } => assert_eq!(*got, reason),
        EquipOutcome::Equipped { slot } => or_abort(Err::<(), String>(format!(
            "expected {reason:?}, but the item equipped into {slot:?}"
        ))),
    }
}

/// Asserts the outcome equipped.
fn assert_equipped(outcome: &EquipOutcome) {
    match outcome {
        EquipOutcome::Equipped { .. } => {}
        EquipOutcome::Rejected { reason, .. } => or_abort(Err::<(), String>(format!(
            "expected the equip to land, got {reason:?}"
        ))),
    }
}

#[test]
fn a_real_elf_bow_rejects_a_dark_knight_and_equips_a_fairy_elf() {
    // The Short Bow's generated class list admits the two elf ranks only —
    // a Dark Knight of any power is ClassMismatch; a Fairy Elf clearing the
    // scaled bars (str (3·2·20)/100+20 = 21, agi (3·2·80)/100+20 = 24)
    // equips it.
    let atlas = real_atlas();
    let knight = wearer(CharacterClass::DarkKnight, 80, 300, 300);
    assert_rejected(
        &try_equip(
            &atlas,
            item_at(&atlas, ELF_BOW, 0),
            EquipmentSlot::RightHand,
            &knight,
        ),
        EquipRejection::ClassMismatch,
    );

    let elf = wearer(CharacterClass::FairyElf, 10, 30, 80);
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, ELF_BOW, 0),
        EquipmentSlot::RightHand,
        &elf,
    ));
}

#[test]
fn a_real_weapon_gates_at_exactly_the_scaled_inclusive_bar() {
    // Kris: strength column 60 at drop level 3 →
    // (3·3·60)/100 + 20 = 5 + 20 = 25. Total 24 fails, exactly 25 passes —
    // the stated column 60 is never the compared number.
    let atlas = real_atlas();
    let weak = wearer(CharacterClass::DarkKnight, 30, 24, 60);
    assert_rejected(
        &try_equip(
            &atlas,
            item_at(&atlas, KRIS, 0),
            EquipmentSlot::RightHand,
            &weak,
        ),
        EquipRejection::RequirementsNotMet,
    );

    let exact = wearer(CharacterClass::DarkKnight, 30, 25, 60);
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, KRIS, 0),
        EquipmentSlot::RightHand,
        &exact,
    ));
}

#[test]
fn rarity_and_enhancement_raise_a_real_items_bar_through_the_surcharge() {
    // The same Kris re-rolled Excellent +5: effective drop level
    // 3 + 3·5 + 25 = 43 → (3·43·60)/100 + 20 = 77 + 20 = 97 — against the
    // plain bar of 25. 96 fails, 97 passes; rarity is not cosmetic for the
    // gate.
    let atlas = real_atlas();
    let excellent_kris = |level: u8| {
        let mut kris = item_at(&atlas, KRIS, level);
        kris.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Weapon {
                options: or_abort(ExcellentWeaponSet::from_options([
                    ExcellentWeaponOption::ManaAfterKill,
                ])),
            },
        };
        kris
    };
    let below = wearer(CharacterClass::DarkKnight, 80, 96, 60);
    assert_rejected(
        &try_equip(&atlas, excellent_kris(5), EquipmentSlot::RightHand, &below),
        EquipRejection::RequirementsNotMet,
    );
    let exact = wearer(CharacterClass::DarkKnight, 80, 97, 60);
    assert_equipped(&try_equip(
        &atlas,
        excellent_kris(5),
        EquipmentSlot::RightHand,
        &exact,
    ));

    // Enhancement alone surcharges too: Normal +5 → edl 3 + 15 = 18 →
    // (3·18·60)/100 + 20 = 32 + 20 = 52.
    let below = wearer(CharacterClass::DarkKnight, 80, 51, 60);
    assert_rejected(
        &try_equip(
            &atlas,
            item_at(&atlas, KRIS, 5),
            EquipmentSlot::RightHand,
            &below,
        ),
        EquipRejection::RequirementsNotMet,
    );
    let exact = wearer(CharacterClass::DarkKnight, 80, 52, 60);
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, KRIS, 5),
        EquipmentSlot::RightHand,
        &exact,
    ));
}

#[test]
fn the_strength_bar_gains_four_per_normal_option_level_on_real_data() {
    // Kris +0 with a +12 physical option (level 3): the pre-+20 scaled value
    // (3·3·60)/100 = 5 is positive, so the bar is 5 + 20 + 4·3 = 37.
    // 36 fails, 37 passes.
    let atlas = real_atlas();
    let optioned_kris = || {
        let mut kris = item_at(&atlas, KRIS, 0);
        kris.normal_option = Some(RolledNormalOption {
            option: NormalOption::PhysicalDamage,
            level: OptionLevel::L3,
        });
        kris
    };
    let below = wearer(CharacterClass::DarkKnight, 30, 36, 60);
    assert_rejected(
        &try_equip(&atlas, optioned_kris(), EquipmentSlot::RightHand, &below),
        EquipRejection::RequirementsNotMet,
    );
    let exact = wearer(CharacterClass::DarkKnight, 30, 37, 60);
    assert_equipped(&try_equip(
        &atlas,
        optioned_kris(),
        EquipmentSlot::RightHand,
        &exact,
    ));
}

#[test]
fn mg_auto_qualifies_for_real_dk_gear_per_the_generated_class_lists() {
    // The generated class lists carry the Magic Gladiator on the DK lines —
    // the rank-2 auto-qualification OUR-pin. Short Sword bars: str
    // (3·16·80)/100 + 20 = 58, agi (3·16·40)/100 + 20 = 39; Bronze Armor:
    // str (3·18·80)/100 + 20 = 63, agi (3·18·20)/100 + 20 = 30.
    let atlas = real_atlas();
    let gladiator = wearer(CharacterClass::MagicGladiator, 60, 100, 60);
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, SHORT_SWORD, 0),
        EquipmentSlot::RightHand,
        &gladiator,
    ));
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, BRONZE_ARMOR, 0),
        EquipmentSlot::Armor,
        &gladiator,
    ));

    // The reverse never holds through data: an elf is not on the DK line.
    let elf = wearer(CharacterClass::FairyElf, 60, 100, 60);
    assert_rejected(
        &try_equip(
            &atlas,
            item_at(&atlas, BRONZE_ARMOR, 0),
            EquipmentSlot::Armor,
            &elf,
        ),
        EquipRejection::ClassMismatch,
    );
}

#[test]
fn a_real_level_gated_wing_compares_the_character_level_raw() {
    // Wings of Satan: wear level 180 with every stat column 0 — the level
    // compares RAW (never scaled by drop level 100) and the zero columns are
    // skipped, never scaled to a phantom 20. Level 179 fails, 180 passes even
    // at minimal stats.
    let atlas = real_atlas();
    let below = wearer(CharacterClass::DarkKnight, 179, 20, 20);
    assert_rejected(
        &try_equip(
            &atlas,
            item_at(&atlas, WINGS_OF_SATAN, 0),
            EquipmentSlot::Wings,
            &below,
        ),
        EquipRejection::RequirementsNotMet,
    );
    let exact = wearer(CharacterClass::DarkKnight, 180, 20, 20);
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, WINGS_OF_SATAN, 0),
        EquipmentSlot::Wings,
        &exact,
    ));
}

#[test]
fn dark_lord_and_magic_gladiator_equip_their_authentic_starter_and_armor() {
    // Regression guard for the class-provenance fix: qualification columns are
    // sourced from the Season 6 item files, so Magic Gladiator and (especially)
    // Dark Lord are no longer under-qualified. A level-1 Dark Lord and Magic
    // Gladiator equip their authentic starter Short Sword and Small Shield and a
    // representative Bronze Armor through the live gate — none is a ClassMismatch.
    // Bars at Normal +0: Short Sword str (3·3·60)/100 + 20 = 25, Small Shield str
    // (3·3·70)/100 + 20 = 26, Bronze Armor str (3·18·80)/100 + 20 = 63 / agi
    // (3·18·20)/100 + 20 = 30 — a str-63/agi-30 wearer clears all three.
    let atlas = real_atlas();
    for class in [CharacterClass::DarkLord, CharacterClass::MagicGladiator] {
        let who = wearer(class, 1, 63, 30);
        assert_equipped(&try_equip(
            &atlas,
            item_at(&atlas, STARTER_SHORT_SWORD, 0),
            EquipmentSlot::RightHand,
            &who,
        ));
        assert_equipped(&try_equip(
            &atlas,
            item_at(&atlas, STARTER_SMALL_SHIELD, 0),
            EquipmentSlot::LeftHand,
            &who,
        ));
        assert_equipped(&try_equip(
            &atlas,
            item_at(&atlas, BRONZE_ARMOR, 0),
            EquipmentSlot::Armor,
            &who,
        ));
    }
}

#[test]
fn a_real_staff_stays_wizard_side_and_refuses_a_dark_lord() {
    // The fix is authentic per item, never a blanket Dark Lord grant: a group-5
    // staff carries no darkLord flag in the Season 6 data. A Dark Wizard wields
    // the Skull Staff (str bar (3·6·40)/100 + 20 = 27); a fully-statted Dark Lord
    // is refused with ClassMismatch — the class gate rejects before any wear bar.
    let atlas = real_atlas();
    let wizard = wearer(CharacterClass::DarkWizard, 10, 60, 60);
    assert_equipped(&try_equip(
        &atlas,
        item_at(&atlas, SKULL_STAFF, 0),
        EquipmentSlot::RightHand,
        &wizard,
    ));
    let dark_lord = wearer(CharacterClass::DarkLord, 400, 300, 300);
    assert_rejected(
        &try_equip(
            &atlas,
            item_at(&atlas, SKULL_STAFF, 0),
            EquipmentSlot::RightHand,
            &dark_lord,
        ),
        EquipRejection::ClassMismatch,
    );
}
