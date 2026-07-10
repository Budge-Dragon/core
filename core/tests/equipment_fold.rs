//! The equipment→profile fold over the real `/data` Atlas (W-EQUIP §3.2):
//! every previously-dead `item_rules` curve proven through the FOLD OUTPUT —
//! armor/shield/wing defense, shield defense-rate, the staff rise (odd and
//! even magic power), the jewelry base-1 resistance with Maximum aggregation —
//! plus the /2-once defense over a real complete suit, the double-wield span
//! over real DK weapons, the 0.95d ammunition percent, the excellent
//! implicit/per-level/×1.02 span, pet bonuses, the broken-piece gate, and the
//! empty-`Equipment` identity (E4). Every expected figure is hand-derived in a
//! comment from the spec §0.5 formulas over the real definitions.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]`
//! body so `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;

use dataset::{or_abort, real_atlas};
use mu_core::components::element::Element;
use mu_core::components::equipment::{Equipment, EquipmentSlot};
use mu_core::components::item_instance::{
    CraftedAugment, Durability, ExcellentArmorSet, ExcellentOptions, ExcellentWeaponSet,
    ItemInstance, LuckRoll, RarityRoll, RolledNormalOption, SkillRoll,
};
use mu_core::components::item_options::{
    ExcellentArmorOption, ExcellentWeaponOption, NormalOption,
};
use mu_core::components::item_ref::ItemRef;
use mu_core::components::levels::OptionLevel;
use mu_core::components::units::{ItemLevel, Percent, Resistance};
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::services::profile::{character_profile, equipped_profile};

// --- Real catalog identities (group, number). --------------------------------

/// Bronze Helm — armor family, defense 9, drop level 16.
const HELM: ItemRef = ItemRef {
    group: 7,
    number: 0,
};
/// Bronze Armor — defense 14, drop level 18.
const ARMOR: ItemRef = ItemRef {
    group: 8,
    number: 0,
};
/// Bronze Pants — defense 10.
const PANTS: ItemRef = ItemRef {
    group: 9,
    number: 0,
};
/// Bronze Gloves — defense 4.
const GLOVES: ItemRef = ItemRef {
    group: 10,
    number: 0,
};
/// Bronze Boots — defense 4.
const BOOTS: ItemRef = ItemRef {
    group: 11,
    number: 0,
};
/// Small Shield — defense 1, defense rate 3.
const SHIELD: ItemRef = ItemRef {
    group: 6,
    number: 0,
};
/// Wings of Satan — the DK first wing: defense 20, 12%/12% damage/absorb.
const WINGS_OF_SATAN: ItemRef = ItemRef {
    group: 12,
    number: 2,
};
/// Kris — width-1 one-handed dagger, damage [3, 7].
const KRIS: ItemRef = ItemRef {
    group: 0,
    number: 1,
};
/// Short Sword — width-1 one-handed sword, damage [16, 26], drop level 16.
const SHORT_SWORD: ItemRef = ItemRef {
    group: 0,
    number: 3,
};
/// Serpent Staff — EVEN magic power 34.
const SERPENT_STAFF: ItemRef = ItemRef {
    group: 5,
    number: 2,
};
/// Legendary Staff — ODD magic power 59 (the §0.5 half-point fixture).
const LEGENDARY_STAFF: ItemRef = ItemRef {
    group: 5,
    number: 5,
};
/// Ring of Ice.
const RING_OF_ICE: ItemRef = ItemRef {
    group: 13,
    number: 8,
};
/// Pendant of Lightning.
const PENDANT_OF_LIGHTNING: ItemRef = ItemRef {
    group: 13,
    number: 12,
};
/// Short Bow — the elf 0.95d weapon, damage [3, 5].
const BOW: ItemRef = ItemRef {
    group: 4,
    number: 0,
};
/// Arrows — ammunition; the item level is the ammo tier.
const ARROWS: ItemRef = ItemRef {
    group: 4,
    number: 15,
};
/// Guardian Angel — pet with data-carried `IncomingDamagePct` 20 +
/// `MaxHealth` 50.
const GUARDIAN_ANGEL: ItemRef = ItemRef {
    group: 13,
    number: 0,
};
/// Dinorant — pet with `IncomingDamagePct` 10 + `DamagePct` 15 (no combat
/// field).
const DINORANT: ItemRef = ItemRef {
    group: 13,
    number: 3,
};

/// A fresh full-gauge Normal instance of real item `id` at plus-`level`.
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

/// The same instance ground to durability 0 — broken, still wearable state.
fn broken(atlas: &Atlas, id: ItemRef, level: u8) -> ItemInstance {
    let mut instance = item_at(atlas, id, level);
    let max = instance.durability.max();
    instance.durability = or_abort(Durability::new(0, max));
    instance
}

/// A character of `class` built through the wire — the only door an external
/// test has — with the exact stats each hand-derivation reads.
fn character(class: &str, level: u16, str_agi_vit_ene: (u16, u16, u16, u16)) -> Character {
    let (strength, agility, vitality, energy) = str_agi_vit_ene;
    or_abort(serde_json::from_value(serde_json::json!({
        "class": class,
        "level": level,
        "experience": 0,
        "stats": {"kind": "standard", "strength": strength, "agility": agility, "vitality": vitality, "energy": energy},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        }
    })))
}

/// The reference Dark Knight: level 90, strength 200, agility 120 — gearless
/// physical `[33, 50]`, defense `20`, defense rate `40`.
fn knight() -> Character {
    character("dark_knight", 90, (200, 120, 100, 30))
}

/// The reference Dark Wizard: level 50, energy 100 — gearless wizardry
/// `[11, 25]`, physical `[5, 10]`.
fn wizard() -> Character {
    character("dark_wizard", 50, (40, 40, 60, 100))
}

/// The five matched Bronze suit pieces at one plus-level each.
fn bronze_suit(atlas: &Atlas, level: u8) -> Equipment {
    Equipment::empty()
        .with(EquipmentSlot::Helm, item_at(atlas, HELM, level))
        .with(EquipmentSlot::Armor, item_at(atlas, ARMOR, level))
        .with(EquipmentSlot::Pants, item_at(atlas, PANTS, level))
        .with(EquipmentSlot::Gloves, item_at(atlas, GLOVES, level))
        .with(EquipmentSlot::Boots, item_at(atlas, BOOTS, level))
}

#[test]
fn the_empty_equipment_fold_is_the_identity_per_class() {
    // E4: an empty worn set folds to the byte-identical gearless profile —
    // per class, so every gearless derivation (spans, /2-once defense,
    // rates, zero chances, zero gear magnitudes) is preserved.
    let atlas = real_atlas();
    let fixtures = [
        character("dark_wizard", 50, (40, 40, 60, 100)),
        character("dark_knight", 90, (200, 120, 100, 30)),
        character("fairy_elf", 40, (100, 180, 60, 40)),
        character("magic_gladiator", 60, (90, 60, 60, 60)),
    ];
    for fixture in fixtures {
        let (gearless, _maxima) = character_profile(&fixture);
        assert_eq!(
            equipped_profile(&fixture, &Equipment::empty(), &atlas),
            gearless,
            "the empty fold must be the identity for {:?}",
            fixture.class()
        );
    }
}

#[test]
fn a_real_armor_piece_raises_defense_through_the_armor_curve_and_the_half_once_sum() {
    // Bronze Helm +10: defense 9 + armor_defense_bonus(+10) = 31 → 40.
    // DK agility 120: raw stat term 120/3 = 40; /2 ONCE over the sum:
    // floor((40 + 40) / 2) = 40 (gearless 20).
    let atlas = real_atlas();
    let hero = knight();
    let worn = Equipment::empty().with(EquipmentSlot::Helm, item_at(&atlas, HELM, 10));
    let equipped = equipped_profile(&hero, &worn, &atlas);
    assert_eq!(equipped.defense(), 40);
    // An armor piece alone feeds neither the rate nor the span.
    let gearless = character_profile(&hero).0;
    assert_eq!(equipped.defense_rate(), gearless.defense_rate());
    assert_eq!(equipped.physical(), gearless.physical());
}

#[test]
fn a_real_shield_raises_defense_and_defense_rate_through_the_shield_curves() {
    // Small Shield +10: defense 1 + shield_defense_bonus(+10) = 10 → 11;
    // defense = floor((40 + 11)/2) = 25. Defense rate 3 +
    // shield_defense_rate_bonus(+10) = 31 → 34 on the gearless 40 → 74 (the
    // rate never halves).
    let atlas = real_atlas();
    let hero = knight();
    let worn = Equipment::empty().with(EquipmentSlot::LeftHand, item_at(&atlas, SHIELD, 10));
    let equipped = equipped_profile(&hero, &worn, &atlas);
    assert_eq!(equipped.defense(), 25);
    assert_eq!(equipped.defense_rate(), 74);
}

#[test]
fn a_real_wing_adds_defense_and_carries_its_percents_at_their_fold_positions() {
    // Wings of Satan +2: defense 20 + wing_defense_bonus(+2) = 6 → 26;
    // defense = floor((40 + 26)/2) = 33. Damage/absorb: 12 + 2·2 = 16% each,
    // carried on the two POST-floor strike fields — never merged into
    // incoming_damage_reduction.
    let atlas = real_atlas();
    let hero = knight();
    let worn = Equipment::empty().with(EquipmentSlot::Wings, item_at(&atlas, WINGS_OF_SATAN, 2));
    let equipped = equipped_profile(&hero, &worn, &atlas);
    assert_eq!(equipped.defense(), 33);
    assert_eq!(equipped.wing_damage_pct(), or_abort(Percent::new(16)));
    assert_eq!(equipped.wing_absorb_pct(), or_abort(Percent::new(16)));
    assert_eq!(equipped.incoming_damage_reduction(), Percent::ZERO);
}

#[test]
fn a_real_staff_lifts_the_wizardry_rise_on_both_parities_dividing_once() {
    // The ×2 rise carrier: Legendary Staff (ODD magic power 59) at +1 →
    // 59 + 2·4 = 67 (rise 33.5 — the §0.5 half-point survives integrally);
    // Serpent Staff (EVEN 34) at +3 → 34 + 2·10 = 54. The gearless wizardry
    // span [11, 25] itself is untouched — the rise multiplies the whole
    // (WizBase + D) parenthesis at the skill seam, never the profile span.
    let atlas = real_atlas();
    let caster = wizard();
    let gearless = character_profile(&caster).0;

    let odd = Equipment::empty().with(
        EquipmentSlot::RightHand,
        item_at(&atlas, LEGENDARY_STAFF, 1),
    );
    let equipped = equipped_profile(&caster, &odd, &atlas);
    assert_eq!(equipped.wizardry_rise_x2(), 67);
    assert_eq!(equipped.wizardry(), gearless.wizardry());
    // The staff is also a physical weapon: [5,10] + flats [29,31] + curve 3.
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (37, 44)
    );

    let even = Equipment::empty().with(EquipmentSlot::RightHand, item_at(&atlas, SERPENT_STAFF, 3));
    let equipped = equipped_profile(&caster, &even, &atlas);
    assert_eq!(equipped.wizardry_rise_x2(), 54);
    assert_eq!(equipped.wizardry(), gearless.wizardry());
}

#[test]
fn real_jewelry_grants_base_one_plus_level_with_maximum_aggregation() {
    // EQ-JEWEL-1/2 over real rings: a +4 Ring of Ice grants 1 + 4 = 5; two
    // ice rings at +1 (2) and +3 (4) grant the MAXIMUM 4, never the sum 6;
    // a +0 Pendant of Lightning already grants 1.
    let atlas = real_atlas();
    let hero = knight();

    let one_ring = Equipment::empty().with(EquipmentSlot::Ring1, item_at(&atlas, RING_OF_ICE, 4));
    let equipped = equipped_profile(&hero, &one_ring, &atlas);
    assert_eq!(equipped.resistance(Element::Ice), Resistance(5));
    assert_eq!(equipped.resistance(Element::Fire), Resistance(0));

    let two_rings = Equipment::empty()
        .with(EquipmentSlot::Ring1, item_at(&atlas, RING_OF_ICE, 1))
        .with(EquipmentSlot::Ring2, item_at(&atlas, RING_OF_ICE, 3));
    let equipped = equipped_profile(&hero, &two_rings, &atlas);
    assert_eq!(
        equipped.resistance(Element::Ice),
        Resistance(4),
        "two same-element rings take the maximum, not the sum"
    );

    let pendant = Equipment::empty().with(
        EquipmentSlot::Pendant,
        item_at(&atlas, PENDANT_OF_LIGHTNING, 0),
    );
    let equipped = equipped_profile(&hero, &pendant, &atlas);
    assert_eq!(equipped.resistance(Element::Lightning), Resistance(1));
}

#[test]
fn a_real_bow_with_ammo_multiplies_the_physical_span_by_the_ammo_tier() {
    // EQ-AMMO-1 over real data: Fairy Elf (str 100, agi 180) gearless
    // [(280/7), (280/4)] = [40, 70]; Short Bow flats [3, 5] → [43, 75];
    // level-1 arrows → ×103/100 → [floor(44.29), floor(77.25)] = [44, 77].
    let atlas = real_atlas();
    let elf = character("fairy_elf", 40, (100, 180, 60, 40));

    let bow_and_quiver = Equipment::empty()
        .with(EquipmentSlot::RightHand, item_at(&atlas, BOW, 0))
        .with(EquipmentSlot::LeftHand, item_at(&atlas, ARROWS, 1));
    let equipped = equipped_profile(&elf, &bow_and_quiver, &atlas);
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (44, 77)
    );

    // The bow alone: no ammo, no multiplier — the flats stand as summed.
    let bow_only = Equipment::empty().with(EquipmentSlot::RightHand, item_at(&atlas, BOW, 0));
    let equipped = equipped_profile(&elf, &bow_only, &atlas);
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (43, 75)
    );

    // Ammunition alone: no bow, no percent, and arrows carry no weapon flats.
    let quiver_only = Equipment::empty().with(EquipmentSlot::LeftHand, item_at(&atlas, ARROWS, 1));
    let equipped = equipped_profile(&elf, &quiver_only, &atlas);
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (40, 70)
    );
}

#[test]
fn a_complete_real_suit_multiplies_defense_rate_and_uniform_level_ten_defense() {
    // The five Bronze pieces at uniform +10 contribute
    // (9+31)+(14+31)+(10+31)+(4+31)+(4+31) = 196; DK raw stat term 40.
    // Uniform level 10 ⇒ ×(100 + (10−9)·5)/100 and the /2, ONE floor:
    // defense = floor(236 · 105 / 200) = floor(123.9) = 123.
    // Complete-suit rate ×11/10 (any level): floor(40 · 11/10) = 44.
    let atlas = real_atlas();
    let hero = knight();
    let uniform = bronze_suit(&atlas, 10);
    let equipped = equipped_profile(&hero, &uniform, &atlas);
    assert_eq!(equipped.defense(), 123);
    assert_eq!(equipped.defense_rate(), 44);

    // Mixed levels (boots +9): still a complete suit — the rate ×11/10
    // holds — but the level-scaled defense multiplier needs a UNIFORM level:
    // boots 4+27 = 31, sum 192, defense = floor((40+192)/2) = 116.
    let (mixed, _) = bronze_suit(&atlas, 10).without(EquipmentSlot::Boots);
    let mixed = mixed.with(EquipmentSlot::Boots, item_at(&atlas, BOOTS, 9));
    let equipped = equipped_profile(&hero, &mixed, &atlas);
    assert_eq!(equipped.defense(), 116);
    assert_eq!(equipped.defense_rate(), 44);

    // A broken piece (durability 0) drops out of the sum AND the suit:
    // sum 196 − 35 = 161, defense = floor((40+161)/2) = 100, rate back to
    // the gearless 40 — while the boots stay in their slot.
    let (broken_suit, _) = bronze_suit(&atlas, 10).without(EquipmentSlot::Boots);
    let broken_suit = broken_suit.with(EquipmentSlot::Boots, broken(&atlas, BOOTS, 10));
    let equipped = equipped_profile(&hero, &broken_suit, &atlas);
    assert_eq!(equipped.defense(), 100);
    assert_eq!(equipped.defense_rate(), 40);
    assert!(broken_suit.get(EquipmentSlot::Boots).is_some());
}

#[test]
fn a_real_weapon_with_option_and_luck_widens_the_span_and_the_critical_chance() {
    // Short Sword +3 with a +12 physical option, lucky: DK [33, 50] + flats
    // [16, 26] + curve 9 (both ends) + option 12 (both) = [70, 97]; luck
    // adds the flat 5% critical chance.
    let atlas = real_atlas();
    let hero = knight();
    let mut sword = item_at(&atlas, SHORT_SWORD, 3);
    sword.normal_option = Some(RolledNormalOption {
        option: NormalOption::PhysicalDamage,
        level: OptionLevel::L3,
    });
    sword.luck = LuckRoll::Lucky;
    let worn = Equipment::empty().with(EquipmentSlot::RightHand, sword);
    let equipped = equipped_profile(&hero, &worn, &atlas);
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (70, 97)
    );
    assert_eq!(equipped.critical_chance(), or_abort(Percent::new(5)));
}

#[test]
fn an_excellent_real_weapon_folds_implicit_per_level_and_the_damage_multiplier() {
    // Excellent Short Sword +3 on a level-90 knight, options
    // {DamagePct, DamagePerLevel}: additive [33+16+9, 50+26+9] = [58, 85],
    // + implicit 16·25/16 + 5 = 30 (both ends) → [88, 115],
    // + TotalLevel/20 = 4 (both) → [92, 119], then ×102/100 LAST →
    // [floor(93.84), floor(121.38)] = [93, 121].
    let atlas = real_atlas();
    let hero = knight();
    let mut sword = item_at(&atlas, SHORT_SWORD, 3);
    sword.roll = RarityRoll::Excellent {
        options: ExcellentOptions::Weapon {
            options: or_abort(ExcellentWeaponSet::from_options([
                ExcellentWeaponOption::DamagePct,
                ExcellentWeaponOption::DamagePerLevel,
            ])),
        },
    };
    let worn = Equipment::empty().with(EquipmentSlot::RightHand, sword);
    let equipped = equipped_profile(&hero, &worn, &atlas);
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (93, 121)
    );

    // The ExcellentDamageChance option raises the excellent chance +10%.
    let mut chance_sword = item_at(&atlas, SHORT_SWORD, 0);
    chance_sword.roll = RarityRoll::Excellent {
        options: ExcellentOptions::Weapon {
            options: or_abort(ExcellentWeaponSet::from_options([
                ExcellentWeaponOption::ExcellentDamageChance,
            ])),
        },
    };
    let worn = Equipment::empty().with(EquipmentSlot::RightHand, chance_sword);
    let equipped = equipped_profile(&hero, &worn, &atlas);
    assert_eq!(equipped.excellent_chance(), or_abort(Percent::new(10)));
}

#[test]
fn excellent_armor_damage_decrease_folds_to_the_pre_floor_field() {
    // Each excellent DamageDecrease armor piece adds 4% on the PRE-floor
    // incoming_dd_pct field — distinct from incoming_damage_reduction and
    // from the wing absorb, which sit at other positions.
    let atlas = real_atlas();
    let hero = knight();
    let dd = |id: ItemRef| {
        let mut piece = item_at(&atlas, id, 0);
        piece.roll = RarityRoll::Excellent {
            options: ExcellentOptions::Armor {
                options: or_abort(ExcellentArmorSet::from_options([
                    ExcellentArmorOption::DamageDecrease,
                ])),
            },
        };
        piece
    };
    let one = Equipment::empty().with(EquipmentSlot::Armor, dd(ARMOR));
    let equipped = equipped_profile(&hero, &one, &atlas);
    assert_eq!(equipped.incoming_dd_pct(), or_abort(Percent::new(4)));
    assert_eq!(equipped.incoming_damage_reduction(), Percent::ZERO);

    let two = Equipment::empty()
        .with(EquipmentSlot::Armor, dd(ARMOR))
        .with(EquipmentSlot::Helm, dd(HELM));
    let equipped = equipped_profile(&hero, &two, &atlas);
    assert_eq!(equipped.incoming_dd_pct(), or_abort(Percent::new(8)));
}

#[test]
fn double_wield_over_real_dk_weapons_halves_the_span_and_flags_the_mode() {
    // EQ-DW-1 span side over real weapons: Kris [3, 7] + Short Sword
    // [16, 26] on the DK [33, 50] sum to [52, 83]; the double-wield ×55/100
    // lands span-side → [floor(28.6), floor(45.65)] = [28, 45]; the typed
    // mode carries the strike head's pre-defense ×2 (net 110%).
    let atlas = real_atlas();
    let hero = knight();
    let both_hands = Equipment::empty()
        .with(EquipmentSlot::LeftHand, item_at(&atlas, KRIS, 0))
        .with(EquipmentSlot::RightHand, item_at(&atlas, SHORT_SWORD, 0));
    let equipped = equipped_profile(&hero, &both_hands, &atlas);
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (28, 45)
    );
    assert_eq!(
        equipped.weapon_mode(),
        mu_core::components::combat_profile::WeaponMode::DoubleWield
    );

    // A non-DK/MG class never double-wields: a wizard with two krises keeps
    // Single and the plainly-summed span [5+3+3, 10+7+7] = [11, 24].
    let caster = wizard();
    let both_hands = Equipment::empty()
        .with(EquipmentSlot::LeftHand, item_at(&atlas, KRIS, 0))
        .with(EquipmentSlot::RightHand, item_at(&atlas, KRIS, 0));
    let equipped = equipped_profile(&caster, &both_hands, &atlas);
    assert_eq!(
        equipped.weapon_mode(),
        mu_core::components::combat_profile::WeaponMode::Single
    );
    assert_eq!(
        (equipped.physical().min(), equipped.physical().max()),
        (11, 24)
    );
}

#[test]
fn a_worn_pet_folds_its_data_carried_bonuses() {
    // EQ-PET-1 over real data: the Guardian Angel's data-carried
    // IncomingDamagePct 20 folds into the general reduction through the
    // shipped CombatBonus seam; its MaxHealth 50 has no combat field. The
    // Dinorant's 10% folds the same way and its DamagePct 15 has no combat
    // consumer — the span stays gearless.
    let atlas = real_atlas();
    let hero = knight();
    let gearless = character_profile(&hero).0;

    let angel = Equipment::empty().with(EquipmentSlot::Pet, item_at(&atlas, GUARDIAN_ANGEL, 0));
    let equipped = equipped_profile(&hero, &angel, &atlas);
    assert_eq!(
        equipped.incoming_damage_reduction(),
        or_abort(Percent::new(20))
    );
    assert_eq!(equipped.physical(), gearless.physical());
    assert_eq!(equipped.defense(), gearless.defense());

    let dinorant = Equipment::empty().with(EquipmentSlot::Pet, item_at(&atlas, DINORANT, 0));
    let equipped = equipped_profile(&hero, &dinorant, &atlas);
    assert_eq!(
        equipped.incoming_damage_reduction(),
        or_abort(Percent::new(10))
    );
    assert_eq!(equipped.physical(), gearless.physical());
}

#[test]
fn a_broken_worn_piece_contributes_nothing_while_staying_worn() {
    // EQ-BROKEN-1's fold half: a durability-0 helm folds to the byte-exact
    // gearless profile, and a durability-0 ring grants no resistance — the
    // whole contribution switches off while the item stays in its slot.
    let atlas = real_atlas();
    let hero = knight();
    let gearless = character_profile(&hero).0;

    let dead_helm = Equipment::empty().with(EquipmentSlot::Helm, broken(&atlas, HELM, 10));
    assert_eq!(equipped_profile(&hero, &dead_helm, &atlas), gearless);
    assert!(dead_helm.get(EquipmentSlot::Helm).is_some());

    let dead_ring = Equipment::empty().with(EquipmentSlot::Ring1, broken(&atlas, RING_OF_ICE, 4));
    let equipped = equipped_profile(&hero, &dead_ring, &atlas);
    assert_eq!(equipped.resistance(Element::Ice), Resistance(0));
}
