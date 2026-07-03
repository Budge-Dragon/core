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

use mu_core::components::class::CharacterClass;
use mu_core::components::collections::OneOrMore;
use mu_core::components::interval::Interval;
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::{AmmoLevel, EnhanceLevel};
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{
    ConeHalfWidth, Facing, Fixed, Radius, Region, WorldPos, WorldRect, WorldVec,
};
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{Exp, ItemLevel, Level, MapNumber, Zen};
use mu_core::data::common::{ItemRef, MonsterNumber};
use mu_core::data::special_drops::SpecialDrop;
use mu_core::events::combat::{AttackOutcome, Damage, DamageModifiers, Hit, HitQuality};
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
            inflicted: None,
            displacement: None,
        }],
    };
    assert_eq!(
        serde_json::to_string(&cast).unwrap(),
        format!(
            r#"{{"kind":"cast","caster_placement":{PLACEMENT_JSON},"hits":[{{"target_index":0,"outcome":{{"kind":"landed","hit":{{"damage":7,"quality":"normal","modifiers":[]}}}},"health":{{"current":20,"max":60}},"inflicted":null,"displacement":null}}]}}"#
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
