//! Serialized-shape drift pins (Q1).
//!
//! One canonical `assert_eq!` per host-facing wire type: a rename, field
//! reorder, or `kind`-tag change makes exactly that pin fail. These are the wire
//! contract non-Rust clients (the SpacetimeDB module, the browser wasm build,
//! the Unity FFI) read; a silent change here is a silent client break, so the
//! exact serialized string is frozen against the live serialization here.
//!
//! The canonical strings are derived from the real `serde_json` output — this
//! file asserts the shape does not drift, it does not invent one.

use mu_core::components::active_effect::{
    ActiveEffect, ActiveEffects, EffectIdentity, PoisonTicks,
};
use mu_core::components::class::CharacterClass;
use mu_core::components::collections::OneOrMore;
use mu_core::components::equipment::EquipmentSlot;
use mu_core::components::interval::Interval;
use mu_core::components::inventory::{Cell, PlacementRejection};
use mu_core::components::item_instance::{
    Durability, ExcellentArmorSet, ExcellentOptions, ExcellentWeaponSet, ItemInstance, LuckRoll,
    RarityRoll, RolledNormalOption, SkillRoll,
};
use mu_core::components::item_options::{
    AncientBonusLevel, ExcellentArmorOption, ExcellentWeaponOption, NormalOption,
};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::OptionLevel;
use mu_core::components::levels::{AmmoLevel, EnhanceLevel};
use mu_core::components::movement::{Mobility, Movement, SlowRatio};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{
    ConeHalfWidth, Facing, Fixed, Radius, Region, WorldPos, WorldRect, WorldVec,
};
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{Exp, ItemLevel, Level, MapNumber, Tick, Ticks, Zen};
use mu_core::data::common::{ItemRef, MonsterNumber};
use mu_core::data::special_drops::SpecialDrop;
use mu_core::entities::world_item::WorldItem;
use mu_core::events::combat::{AttackOutcome, Damage, DamageModifiers, Hit, HitQuality};
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::inventory::{
    EquipOutcome, EquipRejection, MoveOutcome, PlaceOutcome, RemoveOutcome, UnequipOutcome,
};
use mu_core::events::kill::KillResolution;
use mu_core::events::loot::{Drop, DropResolution};
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightDenialReason, FlightOutcome, StepOutcome, WarpOutcome};
use mu_core::events::progression::{ExpAward, LevelUp};
use mu_core::events::skills::{CastRejection, SkillOutcome, TargetHit};
use mu_core::events::spawn::SpawnEvent;

/// The canonical mobile-entity placement reused by every event that nests one.
/// Position is the centre of tile (2, 3); facing east; grounded; map 0.
fn placement() -> Placement {
    Placement {
        position: TileCoord::new(2, 3).to_world(),
        facing: Facing::POS_X,
        movement: Movement::Grounded,
        map: MapNumber(0),
    }
}

const PLACEMENT_JSON: &str =
    r#"{"position":{"x":163840,"y":229376},"facing":{"x":1,"y":0},"movement":"grounded","map":0}"#;

#[test]
fn spatial_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&Fixed::from_raw(196_736)).unwrap(),
        "196736"
    );
    assert_eq!(
        serde_json::to_string(&WorldPos::clamped(163_840, 229_376)).unwrap(),
        r#"{"x":163840,"y":229376}"#
    );
    assert_eq!(
        serde_json::to_string(&WorldVec::new(Fixed::from_raw(6), Fixed::from_raw(-12))).unwrap(),
        r#"{"x":6,"y":-12}"#
    );
    assert_eq!(
        serde_json::to_string(&Facing::POS_X).unwrap(),
        r#"{"x":1,"y":0}"#
    );
    assert_eq!(
        serde_json::to_string(&ConeHalfWidth::DEG_45).unwrap(),
        r#"{"num":1,"den":2}"#
    );
    assert_eq!(
        serde_json::to_string(
            &WorldRect::new(WorldPos::clamped(10, 10), WorldPos::clamped(20, 30)).unwrap()
        )
        .unwrap(),
        r#"{"min":{"x":10,"y":10},"max":{"x":20,"y":30}}"#
    );
    assert_eq!(
        serde_json::to_string(&Radius::new(10).unwrap()).unwrap(),
        "10"
    );
}

#[test]
fn region_wire_shapes_are_pinned() {
    let circle = Region::Circle {
        center: WorldPos::clamped(0, 0),
        radius: Radius::new(5).unwrap(),
    };
    assert_eq!(
        serde_json::to_string(&circle).unwrap(),
        r#"{"kind":"circle","center":{"x":0,"y":0},"radius":5}"#
    );

    let rect = Region::Rect {
        rect: WorldRect::new(WorldPos::clamped(10, 10), WorldPos::clamped(20, 30)).unwrap(),
    };
    assert_eq!(
        serde_json::to_string(&rect).unwrap(),
        r#"{"kind":"rect","rect":{"min":{"x":10,"y":10},"max":{"x":20,"y":30}}}"#
    );

    let cone = Region::Cone {
        apex: WorldPos::clamped(0, 0),
        facing: Facing::POS_X,
        half_width: ConeHalfWidth::DEG_45,
        range: Radius::new(10).unwrap(),
    };
    assert_eq!(
        serde_json::to_string(&cone).unwrap(),
        r#"{"kind":"cone","apex":{"x":0,"y":0},"facing":{"x":1,"y":0},"half_width":{"num":1,"den":2},"range":10}"#
    );
}

#[test]
fn vocabulary_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&CharacterClass::DarkKnight).unwrap(),
        r#""dark_knight""#
    );
    assert_eq!(
        serde_json::to_string(&ItemRarity::Excellent).unwrap(),
        r#""excellent""#
    );
    assert_eq!(serde_json::to_string(&EnhanceLevel::L7).unwrap(), "7");
    assert_eq!(serde_json::to_string(&AmmoLevel::L2).unwrap(), "2");
    assert_eq!(
        serde_json::to_string(&Level::new(150).unwrap()).unwrap(),
        "150"
    );
    assert_eq!(serde_json::to_string(&Zen(1000)).unwrap(), "1000");
    assert_eq!(serde_json::to_string(&Exp(1234)).unwrap(), "1234");
    assert_eq!(
        serde_json::to_string(&Interval::new(4u16, 11u16).unwrap()).unwrap(),
        r#"{"min":4,"max":11}"#
    );
    assert_eq!(
        serde_json::to_string(&TileCoord::new(121, 232)).unwrap(),
        r#"{"x":121,"y":232}"#
    );
}

#[test]
fn special_drop_wire_shape_is_pinned() {
    let drop = SpecialDrop::MonsterBound {
        monster: MonsterNumber(42),
        items: OneOrMore::new(vec![ItemRef {
            group: 0,
            number: 3,
        }])
        .unwrap(),
        item_level: ItemLevel::new(5).unwrap(),
    };
    assert_eq!(
        serde_json::to_string(&drop).unwrap(),
        r#"{"kind":"monster_bound","monster":42,"items":[{"group":0,"number":3}],"item_level":5}"#
    );
}

#[test]
fn combat_event_wire_shapes_are_pinned() {
    let killed = AttackOutcome::Killed {
        hit: Hit {
            damage: Damage(99),
            quality: HitQuality::Critical,
            modifiers: DamageModifiers {
                defense_ignored: true,
                doubled: false,
            },
        },
    };
    assert_eq!(
        serde_json::to_string(&killed).unwrap(),
        r#"{"kind":"killed","hit":{"damage":99,"quality":"critical","modifiers":["defense_ignored"]}}"#
    );
    assert_eq!(
        serde_json::to_string(&DamageModifiers {
            defense_ignored: true,
            doubled: true,
        })
        .unwrap(),
        r#"["defense_ignored","doubled"]"#
    );
}

#[test]
fn skill_event_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&CastRejection::OutOfRange).unwrap(),
        r#""out_of_range""#
    );
    let cast = SkillOutcome::Cast {
        caster_placement: placement(),
        hits: vec![TargetHit {
            target_index: 0,
            outcome: AttackOutcome::Landed {
                hit: Hit {
                    damage: Damage(7),
                    quality: HitQuality::Normal,
                    modifiers: DamageModifiers::NONE,
                },
            },
            health: Pool::new(20, 60).unwrap(),
            active_effects: ActiveEffects::EMPTY,
            inflicted: None,
            displacement: None,
        }],
    };
    assert_eq!(
        serde_json::to_string(&cast).unwrap(),
        format!(
            r#"{{"kind":"cast","caster_placement":{PLACEMENT_JSON},"hits":[{{"target_index":0,"outcome":{{"kind":"landed","hit":{{"damage":7,"quality":"normal","modifiers":[]}}}},"health":{{"current":20,"max":60}},"active_effects":[],"inflicted":null,"displacement":null}}]}}"#
        )
    );
}

#[test]
fn loot_and_progression_event_wire_shapes_are_pinned() {
    let item = Drop::Item {
        item: ItemRef {
            group: 0,
            number: 3,
        },
        level: ItemLevel::new(2).unwrap(),
        rarity: ItemRarity::Excellent,
    };
    assert_eq!(
        serde_json::to_string(&item).unwrap(),
        r#"{"kind":"item","item":{"group":0,"number":3},"level":2,"rarity":"excellent"}"#
    );
    let resolution = DropResolution {
        category: Drop::Zen { amount: Zen(107) },
        specials: vec![Drop::Nothing],
    };
    assert_eq!(
        serde_json::to_string(&resolution).unwrap(),
        r#"{"category":{"kind":"zen","amount":107},"specials":[{"kind":"nothing"}]}"#
    );
    assert_eq!(
        serde_json::to_string(&ExpAward { gained: Exp(1234) }).unwrap(),
        r#"{"gained":1234}"#
    );
    assert_eq!(
        serde_json::to_string(&LevelUp {
            level: Level::new(51).unwrap()
        })
        .unwrap(),
        r#"{"level":51}"#
    );
    let kill = KillResolution {
        drops: DropResolution {
            category: Drop::Zen { amount: Zen(107) },
            specials: Vec::new(),
        },
        experience: ExpAward { gained: Exp(100) },
        level_ups: vec![LevelUp {
            level: Level::new(2).unwrap(),
        }],
    };
    assert_eq!(
        serde_json::to_string(&kill).unwrap(),
        r#"{"drops":{"category":{"kind":"zen","amount":107},"specials":[]},"experience":{"gained":100},"level_ups":[{"level":2}]}"#
    );
}

#[test]
fn movement_event_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&FlightOutcome::Denied {
            reason: FlightDenialReason::NoWings
        })
        .unwrap(),
        r#"{"kind":"denied","reason":"no_wings"}"#
    );
    assert_eq!(
        serde_json::to_string(&StepOutcome::Resolved {
            placement: placement()
        })
        .unwrap(),
        format!(r#"{{"kind":"resolved","placement":{PLACEMENT_JSON}}}"#)
    );
    assert_eq!(
        serde_json::to_string(&WarpOutcome::Arrived {
            placement: placement()
        })
        .unwrap(),
        format!(r#"{{"kind":"arrived","placement":{PLACEMENT_JSON}}}"#)
    );
}

#[test]
fn monster_ai_and_spawn_event_wire_shapes_are_pinned() {
    let wander = MonsterIntent::Wander {
        to: TileCoord::new(2, 3).to_world(),
        facing: Facing::POS_X,
    };
    assert_eq!(
        serde_json::to_string(&wander).unwrap(),
        r#"{"kind":"wander","to":{"x":163840,"y":229376},"facing":{"x":1,"y":0}}"#
    );
    let spawned = SpawnEvent::MobSpawned {
        number: MonsterNumber(7),
        at: TileCoord::new(2, 3).to_world(),
        facing: Facing::POS_X_POS_Y,
    };
    assert_eq!(
        serde_json::to_string(&spawned).unwrap(),
        r#"{"kind":"mob_spawned","number":7,"at":{"x":163840,"y":229376},"facing":{"x":1,"y":1}}"#
    );
}

// -- Exhaustive `kind`-tag coverage for the host-facing event enums. ----------
//
// The single-value pins above freeze one representative serialization per type.
// These blocks additionally pin EVERY variant's discriminator, so an unpinned
// variant's `kind` tag cannot drift silently. Each block constructs every
// variant and asserts its live tag against an inline exhaustive `match` — a new
// variant reds the build here (the match is not wildcarded) until its tag is
// pinned too.

/// The internally-tagged `kind` discriminator of a serialized value, if present.
fn kind_tag(value: &serde_json::Value) -> Option<&str> {
    value.get("kind").and_then(serde_json::Value::as_str)
}

/// The canonical resolved hit reused by the combat/skill variant constructors.
fn sample_hit() -> Hit {
    Hit {
        damage: Damage(1),
        quality: HitQuality::Normal,
        modifiers: DamageModifiers::NONE,
    }
}

#[test]
fn attack_outcome_every_kind_tag_is_pinned() {
    let hit = sample_hit();
    for outcome in [
        AttackOutcome::Missed,
        AttackOutcome::Landed { hit },
        AttackOutcome::Killed { hit },
    ] {
        let expected = match &outcome {
            AttackOutcome::Missed => "missed",
            AttackOutcome::Landed { .. } => "landed",
            AttackOutcome::Killed { .. } => "killed",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn skill_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        SkillOutcome::Rejected {
            reason: CastRejection::OutOfRange,
        },
        SkillOutcome::Cast {
            caster_placement: placement(),
            hits: Vec::new(),
        },
    ] {
        let expected = match &outcome {
            SkillOutcome::Rejected { .. } => "rejected",
            SkillOutcome::Cast { .. } => "cast",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn cast_rejection_every_wire_string_is_pinned() {
    for reason in [
        CastRejection::InsufficientMana,
        CastRejection::InsufficientAbility,
        CastRejection::OutOfRange,
        CastRejection::NoTargetsInRegion,
    ] {
        let actual = serde_json::to_string(&reason).unwrap();
        let expected = match &reason {
            CastRejection::InsufficientMana => r#""insufficient_mana""#,
            CastRejection::InsufficientAbility => r#""insufficient_ability""#,
            CastRejection::OutOfRange => r#""out_of_range""#,
            CastRejection::NoTargetsInRegion => r#""no_targets_in_region""#,
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn drop_every_kind_tag_is_pinned() {
    for drop in [
        Drop::Zen { amount: Zen(1) },
        Drop::Item {
            item: ItemRef {
                group: 0,
                number: 3,
            },
            level: ItemLevel::new(2).unwrap(),
            rarity: ItemRarity::Excellent,
        },
        Drop::Nothing,
    ] {
        let expected = match &drop {
            Drop::Zen { .. } => "zen",
            Drop::Item { .. } => "item",
            Drop::Nothing => "nothing",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(drop).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn monster_intent_every_kind_tag_is_pinned() {
    let to = TileCoord::new(2, 3).to_world();
    for intent in [
        MonsterIntent::Idle,
        MonsterIntent::Wander {
            to,
            facing: Facing::POS_X,
        },
        MonsterIntent::Chase {
            to,
            facing: Facing::POS_X,
        },
        MonsterIntent::LeashReturn {
            to,
            facing: Facing::POS_X,
        },
        MonsterIntent::Attack { target: to },
    ] {
        let expected = match &intent {
            MonsterIntent::Idle => "idle",
            MonsterIntent::Wander { .. } => "wander",
            MonsterIntent::Chase { .. } => "chase",
            MonsterIntent::LeashReturn { .. } => "leash_return",
            MonsterIntent::Attack { .. } => "attack",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(intent).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn flight_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        FlightOutcome::ModeChanged {
            mode: Movement::Flying,
        },
        FlightOutcome::Denied {
            reason: FlightDenialReason::NoWings,
        },
    ] {
        let expected = match &outcome {
            FlightOutcome::ModeChanged { .. } => "mode_changed",
            FlightOutcome::Denied { .. } => "denied",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn step_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        StepOutcome::Resolved {
            placement: placement(),
        },
        StepOutcome::Blocked,
    ] {
        let expected = match &outcome {
            StepOutcome::Resolved { .. } => "resolved",
            StepOutcome::Blocked => "blocked",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn warp_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        WarpOutcome::Arrived {
            placement: placement(),
        },
        WarpOutcome::NoWalkableLanding,
    ] {
        let expected = match &outcome {
            WarpOutcome::Arrived { .. } => "arrived",
            WarpOutcome::NoWalkableLanding => "no_walkable_landing",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn spawn_event_every_kind_tag_is_pinned() {
    let at = TileCoord::new(2, 3).to_world();
    for event in [
        SpawnEvent::MobSpawned {
            number: MonsterNumber(7),
            at,
            facing: Facing::POS_X,
        },
        SpawnEvent::ObjectPlaced {
            number: MonsterNumber(248),
            at,
            facing: Facing::POS_Y,
        },
    ] {
        let expected = match &event {
            SpawnEvent::MobSpawned { .. } => "mob_spawned",
            SpawnEvent::ObjectPlaced { .. } => "object_placed",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(event).unwrap()),
            Some(expected)
        );
    }
}

// -- Item-instance, container, and world-item wire pins. ----------------------

/// A bare normal instance of item (0, 3) at +0, full durability 30.
fn normal_instance() -> ItemInstance {
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
    }
}

#[test]
fn normal_item_instance_wire_is_pinned() {
    assert_eq!(
        serde_json::to_string(&normal_instance()).unwrap(),
        r#"{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30}}"#
    );
}

#[test]
fn excellent_item_instance_wire_is_pinned() {
    let mut instance = normal_instance();
    instance.level = ItemLevel::new(9).unwrap();
    instance.roll = RarityRoll::Excellent {
        options: ExcellentOptions::Weapon {
            options: ExcellentWeaponSet::with_first(
                ExcellentWeaponOption::ManaAfterKill,
                [ExcellentWeaponOption::AttackSpeed],
            ),
        },
    };
    instance.luck = LuckRoll::Lucky;
    instance.skill = SkillRoll::WithSkill;
    instance.durability = Durability::full(35);
    assert_eq!(
        serde_json::to_string(&instance).unwrap(),
        r#"{"item":{"group":0,"number":3},"level":9,"roll":{"kind":"excellent","options":{"set":"weapon","options":["mana_after_kill","attack_speed"]}},"normal_option":null,"luck":"lucky","skill":"with_skill","durability":{"current":35,"max":35}}"#
    );
}

#[test]
fn ancient_item_instance_wire_is_pinned() {
    let instance = ItemInstance {
        item: ItemRef {
            group: 7,
            number: 0,
        },
        level: ItemLevel::new(5).unwrap(),
        roll: RarityRoll::Ancient {
            bonus: AncientBonusLevel::Two,
        },
        normal_option: Some(RolledNormalOption {
            option: NormalOption::Defense,
            level: OptionLevel::L4,
        }),
        luck: LuckRoll::Plain,
        skill: SkillRoll::NoSkill,
        durability: Durability::full(40),
    };
    assert_eq!(
        serde_json::to_string(&instance).unwrap(),
        r#"{"item":{"group":7,"number":0},"level":5,"roll":{"kind":"ancient","bonus":2},"normal_option":{"option":"defense","level":4},"luck":"plain","skill":"no_skill","durability":{"current":40,"max":40}}"#
    );
}

#[test]
fn excellent_armor_set_wire_is_a_slot_sorted_name_array() {
    let set = ExcellentArmorSet::with_first(
        ExcellentArmorOption::MaxHealth,
        [ExcellentArmorOption::ZenGain],
    );
    assert_eq!(
        serde_json::to_string(&set).unwrap(),
        r#"["zen_gain","max_health"]"#
    );
}

#[test]
fn world_item_wire_is_pinned() {
    let world_item = WorldItem {
        instance: normal_instance(),
        position: WorldPos::clamped(163_840, 229_376),
        map: MapNumber(0),
        despawn: Tick(1200),
    };
    assert_eq!(
        serde_json::to_string(&world_item).unwrap(),
        r#"{"instance":{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30}},"position":{"x":163840,"y":229376},"map":0,"despawn":1200}"#
    );
}

#[test]
fn container_outcome_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&PlaceOutcome::Placed {
            at: Cell { row: 1, col: 2 }
        })
        .unwrap(),
        r#"{"kind":"placed","at":{"row":1,"col":2}}"#
    );
    assert_eq!(
        serde_json::to_string(&PlaceOutcome::Rejected {
            reason: PlacementRejection::CellsOccupied,
            item: normal_instance(),
        })
        .unwrap(),
        r#"{"kind":"rejected","reason":{"kind":"cells_occupied"},"item":{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30}}}"#
    );
    assert_eq!(
        serde_json::to_string(&MoveOutcome::Moved {
            from: Cell { row: 0, col: 0 },
            to: Cell { row: 3, col: 4 }
        })
        .unwrap(),
        r#"{"kind":"moved","from":{"row":0,"col":0},"to":{"row":3,"col":4}}"#
    );
    assert_eq!(
        serde_json::to_string(&EquipOutcome::Equipped {
            slot: EquipmentSlot::Helm
        })
        .unwrap(),
        r#"{"kind":"equipped","slot":"helm"}"#
    );
    assert_eq!(
        serde_json::to_string(&UnequipOutcome::SlotEmpty).unwrap(),
        r#"{"kind":"slot_empty"}"#
    );
    assert_eq!(
        serde_json::to_string(&EquipRejection::IncompatibleSlot).unwrap(),
        r#""incompatible_slot""#
    );
    assert_eq!(
        serde_json::to_string(&EquipRejection::TwoHandedConflict).unwrap(),
        r#""two_handed_conflict""#
    );
    assert_eq!(
        serde_json::to_string(&PlacementRejection::NoItemAtCell).unwrap(),
        r#"{"kind":"no_item_at_cell"}"#
    );
}

#[test]
fn container_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        RemoveOutcome::Removed {
            at: Cell { row: 0, col: 0 },
            item: normal_instance(),
        },
        RemoveOutcome::Rejected {
            reason: PlacementRejection::NoItemAtCell,
        },
    ] {
        let expected = match &outcome {
            RemoveOutcome::Removed { .. } => "removed",
            RemoveOutcome::Rejected { .. } => "rejected",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
    for outcome in [
        EquipOutcome::Equipped {
            slot: EquipmentSlot::Ring1,
        },
        EquipOutcome::Rejected {
            reason: EquipRejection::SlotOccupied,
            item: normal_instance(),
        },
    ] {
        let expected = match &outcome {
            EquipOutcome::Equipped { .. } => "equipped",
            EquipOutcome::Rejected { .. } => "rejected",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

// -- Timed-effect, mobility, and effect-event wire pins. ----------------------

#[test]
fn active_effect_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&ActiveEffect::Defense { expiry: Tick(80) }).unwrap(),
        r#"{"kind":"defense","expiry":80}"#
    );
    assert_eq!(
        serde_json::to_string(&ActiveEffect::GreaterDamage {
            amount: 13,
            expiry: Tick(1200)
        })
        .unwrap(),
        r#"{"kind":"greater_damage","amount":13,"expiry":1200}"#
    );
    assert_eq!(
        serde_json::to_string(&ActiveEffect::Poisoned {
            per_tick_damage: 12,
            remaining: PoisonTicks::INITIAL,
            next_tick: Tick(60),
            cadence: Ticks(60),
        })
        .unwrap(),
        r#"{"kind":"poisoned","per_tick_damage":12,"remaining":6,"next_tick":60,"cadence":60}"#
    );
    assert_eq!(
        serde_json::to_string(&EffectIdentity::DefenseReduction).unwrap(),
        r#""defense_reduction""#
    );
    // ActiveEffects is a Vec<ActiveEffect> on the wire; empty is the empty array.
    let store = ActiveEffects::EMPTY.with(ActiveEffect::Iced { expiry: Tick(40) });
    assert_eq!(
        serde_json::to_string(&store).unwrap(),
        r#"[{"kind":"iced","expiry":40}]"#
    );
    assert_eq!(serde_json::to_string(&ActiveEffects::EMPTY).unwrap(), "[]");
}

#[test]
fn mobility_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&Mobility::Free).unwrap(),
        r#"{"kind":"free"}"#
    );
    assert_eq!(
        serde_json::to_string(&Mobility::Slowed {
            ratio: SlowRatio::HALVED
        })
        .unwrap(),
        r#"{"kind":"slowed","ratio":{"num":1,"den":2}}"#
    );
    assert_eq!(
        serde_json::to_string(&Mobility::Immobilized).unwrap(),
        r#"{"kind":"immobilized"}"#
    );
}

#[test]
fn effect_event_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&EffectEvent::EffectApplied {
            effect: ActiveEffect::Defense { expiry: Tick(80) }
        })
        .unwrap(),
        r#"{"kind":"effect_applied","effect":{"kind":"defense","expiry":80}}"#
    );
    assert_eq!(
        serde_json::to_string(&EffectEvent::PoisonTick { damage: Damage(9) }).unwrap(),
        r#"{"kind":"poison_tick","damage":9}"#
    );
    assert_eq!(
        serde_json::to_string(&EffectEvent::EffectExpired {
            effect: EffectIdentity::Poisoned
        })
        .unwrap(),
        r#"{"kind":"effect_expired","effect":"poisoned"}"#
    );
    assert_eq!(
        serde_json::to_string(&BuffCastOutcome::Healed { amount: 20 }).unwrap(),
        r#"{"kind":"healed","amount":20}"#
    );
}

#[test]
fn active_effect_every_kind_tag_is_pinned() {
    for effect in [
        ActiveEffect::Defense { expiry: Tick(1) },
        ActiveEffect::GreaterDamage {
            amount: 1,
            expiry: Tick(1),
        },
        ActiveEffect::GreaterDefense {
            amount: 1,
            expiry: Tick(1),
        },
        ActiveEffect::Poisoned {
            per_tick_damage: 1,
            remaining: PoisonTicks::INITIAL,
            next_tick: Tick(1),
            cadence: Ticks(1),
        },
        ActiveEffect::Iced { expiry: Tick(1) },
        ActiveEffect::Frozen { expiry: Tick(1) },
        ActiveEffect::DefenseReduction { expiry: Tick(1) },
    ] {
        let expected = match &effect {
            ActiveEffect::Defense { .. } => "defense",
            ActiveEffect::GreaterDamage { .. } => "greater_damage",
            ActiveEffect::GreaterDefense { .. } => "greater_defense",
            ActiveEffect::Poisoned { .. } => "poisoned",
            ActiveEffect::Iced { .. } => "iced",
            ActiveEffect::Frozen { .. } => "frozen",
            ActiveEffect::DefenseReduction { .. } => "defense_reduction",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(effect).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn effect_event_every_kind_tag_is_pinned() {
    for event in [
        EffectEvent::EffectApplied {
            effect: ActiveEffect::Defense { expiry: Tick(1) },
        },
        EffectEvent::PoisonTick { damage: Damage(1) },
        EffectEvent::PoisonKilled { damage: Damage(1) },
        EffectEvent::EffectExpired {
            effect: EffectIdentity::Iced,
        },
        EffectEvent::Healed { amount: 1 },
    ] {
        let expected = match &event {
            EffectEvent::EffectApplied { .. } => "effect_applied",
            EffectEvent::PoisonTick { .. } => "poison_tick",
            EffectEvent::PoisonKilled { .. } => "poison_killed",
            EffectEvent::EffectExpired { .. } => "effect_expired",
            EffectEvent::Healed { .. } => "healed",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(event).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn buff_cast_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        BuffCastOutcome::Rejected {
            reason: CastRejection::OutOfRange,
        },
        BuffCastOutcome::Applied {
            effect: ActiveEffect::Defense { expiry: Tick(1) },
        },
        BuffCastOutcome::Healed { amount: 1 },
    ] {
        let expected = match &outcome {
            BuffCastOutcome::Rejected { .. } => "rejected",
            BuffCastOutcome::Applied { .. } => "applied",
            BuffCastOutcome::Healed { .. } => "healed",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn mobility_every_kind_tag_is_pinned() {
    for mobility in [
        Mobility::Free,
        Mobility::Slowed {
            ratio: SlowRatio::HALVED,
        },
        Mobility::Immobilized,
    ] {
        let expected = match &mobility {
            Mobility::Free => "free",
            Mobility::Slowed { .. } => "slowed",
            Mobility::Immobilized => "immobilized",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(mobility).unwrap()),
            Some(expected)
        );
    }
}
