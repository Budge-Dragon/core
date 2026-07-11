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
use mu_core::components::combat_profile::{CombatProfile, WeaponMode};
use mu_core::components::equipment::EquipmentSlot;
use mu_core::components::interval::Interval;
use mu_core::components::inventory::{Cell, Footprint, PlacementRejection};
use mu_core::components::item_instance::{
    CraftedAugment, DinorantOptionSet, Durability, ExcellentArmorSet, ExcellentOptions,
    ExcellentWeaponSet, ItemInstance, LuckRoll, RarityRoll, RolledNormalOption, SkillRoll,
};
use mu_core::components::item_options::{
    AncientBonusLevel, DinorantOption, ExcellentArmorOption, ExcellentWeaponOption, NormalOption,
    SecondWingBonus,
};
use mu_core::components::item_quality::ItemRarity;
use mu_core::components::levels::OptionLevel;
use mu_core::components::levels::{AmmoLevel, EnhanceLevel};
use mu_core::components::movement::{Mobility, Movement, SlowRatio};
use mu_core::components::party::{Leadership, MemberSlot, Membership, Vitality};
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::spatial::{
    ConeHalfWidth, Facing, Fixed, Radius, Region, WorldPos, WorldRect, WorldVec,
};
use mu_core::components::tile::TileCoord;
use mu_core::components::trade_window::{Side, TradeWindow};
use mu_core::components::units::{CarriedZen, Exp, ItemLevel, Level, MapNumber, Tick, Ticks, Zen};
use mu_core::data::common::{ItemRef, MonsterNumber};
use mu_core::data::gates_warps::WarpIndex;
use mu_core::data::item_definitions::{ItemPrice, PerLevelPrice};
use mu_core::data::special_drops::SpecialDrop;
use mu_core::entities::character::Character;
use mu_core::entities::party_session::{PartyInvite, PartyMember, PartySession};
use mu_core::entities::trade_session::{TradeLocks, TradeOffer, TradeOffers, TradeSession};
use mu_core::entities::world_item::WorldItem;
use mu_core::entities::world_zen::WorldZen;
use mu_core::events::combat::{AttackOutcome, Damage, DamageModifiers, Hit, HitQuality};
use mu_core::events::craft::{Casualty, MixOutcome, RejectReason};
use mu_core::events::effect::{BuffCastOutcome, EffectEvent};
use mu_core::events::inventory::{
    EquipOutcome, EquipRejection, MoveOutcome, PlaceOutcome, RemoveOutcome, UnequipOutcome,
};
use mu_core::events::kill::KillResolution;
use mu_core::events::loot::{Drop, DropResolution};
use mu_core::events::monster_ai::MonsterIntent;
use mu_core::events::movement::{FlightDenialReason, FlightOutcome, StepOutcome, WarpOutcome};
use mu_core::events::party::{AcceptBounce, InviteRejection, MemberAward, PartyEvent};
use mu_core::events::progression::{ExpAward, LevelUp};
use mu_core::events::shop::{
    BuyOutcome, RepairAllOutcome, RepairOutcome, SellOutcome, SlotRepair, SlotRepairResult,
};
use mu_core::events::skills::{CastRejection, SkillOutcome, TargetHit};
use mu_core::events::spawn::SpawnEvent;
use mu_core::events::trade::{
    BouncedProof, CancelReason, OfferOutcome, RearrangeOutcome, RequestRejection, SideFailure,
    TradeEvent, UnlockOutcome, WithdrawOutcome, ZenOfferOutcome,
};
use mu_core::events::travel::{
    EnterGateOutcome, TownPortalOutcome, WarpAvailability, WarpEntryStatus, WarpLockReason,
    WarpTravelOutcome,
};
use mu_core::services::inventory::ZenPickupOutcome;
use mu_core::services::party;
use mu_core::services::trade::{AcceptOutcome, LockResult, RequestOutcome, TradeAvailability};
use mu_core::services::wear::WearEvent;

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
    // TargetHit is kind-tagged, mirroring AttackOutcome's missed/landed/killed
    // split; the ailment/knockback fields exist only on the landed variant.
    let cast = SkillOutcome::Cast {
        caster_placement: placement(),
        hits: vec![TargetHit::Landed {
            target_index: 0,
            hit: Hit {
                damage: Damage(7),
                quality: HitQuality::Normal,
                modifiers: DamageModifiers::NONE,
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
            r#"{{"kind":"cast","caster_placement":{PLACEMENT_JSON},"hits":[{{"kind":"landed","target_index":0,"hit":{{"damage":7,"quality":"normal","modifiers":[]}},"health":{{"current":20,"max":60}},"active_effects":[],"inflicted":null,"displacement":null}}]}}"#
        )
    );
    let missed = TargetHit::Missed {
        target_index: 1,
        health: Pool::new(35, 35).unwrap(),
        active_effects: ActiveEffects::EMPTY,
        displacement: None,
    };
    assert_eq!(
        serde_json::to_string(&missed).unwrap(),
        r#"{"kind":"missed","target_index":1,"health":{"current":35,"max":35},"active_effects":[],"displacement":null}"#
    );
    let killed = TargetHit::Killed {
        target_index: 2,
        hit: Hit {
            damage: Damage(50),
            quality: HitQuality::Critical,
            modifiers: DamageModifiers::NONE,
        },
        health: Pool::new(0, 40).unwrap(),
        active_effects: ActiveEffects::EMPTY,
    };
    assert_eq!(
        serde_json::to_string(&killed).unwrap(),
        r#"{"kind":"killed","target_index":2,"hit":{"damage":50,"quality":"critical","modifiers":[]},"health":{"current":0,"max":40},"active_effects":[]}"#
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
fn target_hit_every_kind_tag_is_pinned() {
    let hit = sample_hit();
    let health = Pool::new(20, 60).unwrap();
    for target_hit in [
        TargetHit::Missed {
            target_index: 0,
            health,
            active_effects: ActiveEffects::EMPTY,
            displacement: None,
        },
        TargetHit::Landed {
            target_index: 0,
            hit,
            health,
            active_effects: ActiveEffects::EMPTY,
            inflicted: None,
            displacement: None,
        },
        TargetHit::Killed {
            target_index: 0,
            hit,
            health,
            active_effects: ActiveEffects::EMPTY,
        },
    ] {
        let expected = match &target_hit {
            TargetHit::Missed { .. } => "missed",
            TargetHit::Landed { .. } => "landed",
            TargetHit::Killed { .. } => "killed",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(target_hit).unwrap()),
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
fn warp_travel_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        WarpTravelOutcome::Arrived {
            placement: placement(),
            balance: CarriedZen::new(5000).unwrap(),
        },
        WarpTravelOutcome::NotAlive,
        WarpTravelOutcome::NotDiscovered,
        WarpTravelOutcome::LevelTooLow { required: 33 },
        WarpTravelOutcome::CannotFly,
        WarpTravelOutcome::NotEnoughZen {
            required: Zen(5000),
            available: CarriedZen::new(4999).unwrap(),
        },
        WarpTravelOutcome::NoWalkableLanding,
    ] {
        let expected = match &outcome {
            WarpTravelOutcome::Arrived { .. } => "arrived",
            WarpTravelOutcome::NotAlive => "not_alive",
            WarpTravelOutcome::NotDiscovered => "not_discovered",
            WarpTravelOutcome::LevelTooLow { .. } => "level_too_low",
            WarpTravelOutcome::CannotFly => "cannot_fly",
            WarpTravelOutcome::NotEnoughZen { .. } => "not_enough_zen",
            WarpTravelOutcome::NoWalkableLanding => "no_walkable_landing",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn warp_projection_wire_shapes_are_pinned() {
    // Every WarpLockReason and WarpAvailability discriminator, plus the flat
    // per-entry status shape the menu returns.
    for reason in [
        WarpLockReason::NotDiscovered,
        WarpLockReason::LevelTooLow { required: 50 },
        WarpLockReason::CannotFly,
        WarpLockReason::InsufficientZen { cost: Zen(5000) },
    ] {
        let expected = match &reason {
            WarpLockReason::NotDiscovered => "not_discovered",
            WarpLockReason::LevelTooLow { .. } => "level_too_low",
            WarpLockReason::CannotFly => "cannot_fly",
            WarpLockReason::InsufficientZen { .. } => "insufficient_zen",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(reason).unwrap()),
            Some(expected)
        );
    }
    for availability in [
        WarpAvailability::Available,
        WarpAvailability::Locked {
            reasons: OneOrMore::with_head(WarpLockReason::NotDiscovered, Vec::new()),
        },
    ] {
        let expected = match &availability {
            WarpAvailability::Available => "available",
            WarpAvailability::Locked { .. } => "locked",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(availability).unwrap()),
            Some(expected)
        );
    }
    assert_eq!(
        serde_json::to_string(&WarpEntryStatus {
            index: WarpIndex(8),
            availability: WarpAvailability::Locked {
                reasons: OneOrMore::with_head(
                    WarpLockReason::LevelTooLow { required: 33 },
                    vec![WarpLockReason::InsufficientZen { cost: Zen(5000) }],
                ),
            },
        })
        .unwrap(),
        r#"{"index":8,"availability":{"kind":"locked","reasons":[{"kind":"level_too_low","required":33},{"kind":"insufficient_zen","cost":5000}]}}"#
    );
}

#[test]
fn town_portal_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        TownPortalOutcome::Arrived {
            placement: placement(),
        },
        TownPortalOutcome::NotAlive,
        TownPortalOutcome::NoScroll,
    ] {
        let expected = match &outcome {
            TownPortalOutcome::Arrived { .. } => "arrived",
            TownPortalOutcome::NotAlive => "not_alive",
            TownPortalOutcome::NoScroll => "no_scroll",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn enter_gate_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        EnterGateOutcome::Arrived {
            placement: placement(),
        },
        EnterGateOutcome::NotAlive,
        EnterGateOutcome::LevelTooLow { required: 40 },
        EnterGateOutcome::CannotFly,
        EnterGateOutcome::NoWalkableLanding,
    ] {
        let expected = match &outcome {
            EnterGateOutcome::Arrived { .. } => "arrived",
            EnterGateOutcome::NotAlive => "not_alive",
            EnterGateOutcome::LevelTooLow { .. } => "level_too_low",
            EnterGateOutcome::CannotFly => "cannot_fly",
            EnterGateOutcome::NoWalkableLanding => "no_walkable_landing",
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
        augment: CraftedAugment::None,
    }
}

#[test]
fn normal_item_instance_wire_is_pinned() {
    assert_eq!(
        serde_json::to_string(&normal_instance()).unwrap(),
        r#"{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}}"#
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
        r#"{"item":{"group":0,"number":3},"level":9,"roll":{"kind":"excellent","options":{"set":"weapon","options":["mana_after_kill","attack_speed"]}},"normal_option":null,"luck":"lucky","skill":"with_skill","durability":{"current":35,"max":35},"augment":{"kind":"none"}}"#
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
        augment: CraftedAugment::None,
    };
    assert_eq!(
        serde_json::to_string(&instance).unwrap(),
        r#"{"item":{"group":7,"number":0},"level":5,"roll":{"kind":"ancient","bonus":2},"normal_option":{"option":"defense","level":4},"luck":"plain","skill":"no_skill","durability":{"current":40,"max":40},"augment":{"kind":"none"}}"#
    );
}

#[test]
fn durability_wear_ledger_wire_is_pinned() {
    // The shipped {current, max} form is byte-identical while the persisted
    // wear ledger is empty — a fresh gauge carries no third field.
    assert_eq!(
        serde_json::to_string(&Durability::full(30)).unwrap(),
        r#"{"current":30,"max":30}"#
    );
    // A sub-divisor wear advance rides the wire as `wear_progress`.
    let divisor = core::num::NonZeroU32::new(2000).unwrap();
    let worn = Durability::full(30).worn(1500, divisor);
    assert_eq!(
        serde_json::to_string(&worn).unwrap(),
        r#"{"current":30,"max":30,"wear_progress":1500}"#
    );
    // The ledger form round-trips and the next crossing lands exactly where
    // the pre-persist gauge would have: 1500 + 500 crosses one point and the
    // zeroed ledger leaves the wire again.
    let reloaded: Durability =
        serde_json::from_str(&serde_json::to_string(&worn).unwrap()).unwrap();
    assert_eq!(reloaded, worn);
    assert_eq!(
        serde_json::to_string(&reloaded.worn(500, divisor)).unwrap(),
        r#"{"current":29,"max":30}"#
    );
    // Every shipped pre-ledger persisted form still parses — to the empty
    // ledger (the two-field equality against a fresh construction proves it).
    let shipped: Durability = serde_json::from_str(r#"{"current":12,"max":30}"#).unwrap();
    assert_eq!(shipped, Durability::new(12, 30).unwrap());
}

#[test]
fn wear_event_wire_shapes_are_pinned() {
    let divisor = core::num::NonZeroU32::new(2000).unwrap();
    let worn = WearEvent::Worn {
        slot: EquipmentSlot::Helm,
        durability: Durability::full(30).worn(1500, divisor),
    };
    assert_eq!(
        serde_json::to_string(&worn).unwrap(),
        r#"{"kind":"worn","slot":"helm","durability":{"current":30,"max":30,"wear_progress":1500}}"#
    );
    assert_eq!(
        serde_json::to_string(&WearEvent::Broken {
            slot: EquipmentSlot::LeftHand
        })
        .unwrap(),
        r#"{"kind":"broken","slot":"left_hand"}"#
    );
    assert_eq!(
        serde_json::to_string(&WearEvent::Destroyed {
            slot: EquipmentSlot::Pet
        })
        .unwrap(),
        r#"{"kind":"destroyed","slot":"pet"}"#
    );
}

#[test]
fn wear_event_every_kind_tag_is_pinned() {
    for event in [
        WearEvent::Worn {
            slot: EquipmentSlot::Helm,
            durability: Durability::full(30),
        },
        WearEvent::Broken {
            slot: EquipmentSlot::Helm,
        },
        WearEvent::Destroyed {
            slot: EquipmentSlot::Helm,
        },
    ] {
        let expected = match &event {
            WearEvent::Worn { .. } => "worn",
            WearEvent::Broken { .. } => "broken",
            WearEvent::Destroyed { .. } => "destroyed",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(event).unwrap()),
            Some(expected)
        );
    }
}

/// The shipped pre-W-EQUIP `CombatProfile` wire form — no gear-magnitude
/// fields — as a persisted host row would carry it.
const GEARLESS_PROFILE_JSON: &str = r#"{"level":50,"physical":{"min":33,"max":50},"wizardry":null,"defense":20,"attack_rate":480,"defense_rate":40,"resistances":{"ice":0,"poison":0,"lightning":0,"fire":0,"earth":0,"wind":0,"water":0},"critical_chance":0,"excellent_chance":0,"defense_ignore_chance":0,"double_damage_chance":0,"incoming_damage_reduction":0,"flat_damage_add":0}"#;

#[test]
fn combat_profile_wire_carries_the_gear_magnitudes_and_parses_shipped_forms() {
    // Every shipped persisted profile (no gear fields) still parses — each
    // W-EQUIP field serde-defaults to its gearless zero/identity.
    let profile: CombatProfile = serde_json::from_str(GEARLESS_PROFILE_JSON).unwrap();
    assert_eq!(profile.wizardry_rise_x2(), 0);
    assert_eq!(profile.incoming_dd_pct().points(), 0);
    assert_eq!(profile.wing_damage_pct().points(), 0);
    assert_eq!(profile.wing_absorb_pct().points(), 0);
    assert_eq!(profile.weapon_mode(), WeaponMode::Single);
    // Contains-based (the shipped `combat_profile_wire_round_trips` idiom):
    // the re-serialized profile carries each gear magnitude under its name,
    // and the typed weapon mode rides as a snake_case string, never a bool.
    let wire = serde_json::to_string(&profile).unwrap();
    for expected in [
        r#""wizardry_rise_x2":0"#,
        r#""incoming_dd_pct":0"#,
        r#""wing_damage_pct":0"#,
        r#""wing_absorb_pct":0"#,
        r#""weapon_mode":"single""#,
    ] {
        assert!(wire.contains(expected), "missing {expected} in {wire}");
    }
}

#[test]
fn crafted_augment_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&CraftedAugment::None).unwrap(),
        r#"{"kind":"none"}"#
    );
    assert_eq!(
        serde_json::to_string(&CraftedAugment::Dinorant {
            options: DinorantOptionSet::with_first(
                DinorantOption::DamageAbsorb,
                [DinorantOption::AttackSpeed],
            ),
        })
        .unwrap(),
        r#"{"kind":"dinorant","options":["damage_absorb","attack_speed"]}"#
    );
    assert_eq!(
        serde_json::to_string(&CraftedAugment::WingBonus {
            bonus: SecondWingBonus::Command,
        })
        .unwrap(),
        r#"{"kind":"wing_bonus","bonus":"command"}"#
    );
}

#[test]
fn crafted_augment_every_kind_tag_is_pinned() {
    for augment in [
        CraftedAugment::None,
        CraftedAugment::Dinorant {
            options: DinorantOptionSet::with_first(DinorantOption::MaxAbility, []),
        },
        CraftedAugment::WingBonus {
            bonus: SecondWingBonus::MaxHealth,
        },
    ] {
        let expected = match &augment {
            CraftedAugment::None => "none",
            CraftedAugment::Dinorant { .. } => "dinorant",
            CraftedAugment::WingBonus { .. } => "wing_bonus",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(augment).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn character_wire_is_pinned() {
    let character: Character = serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": 42,
        "experience": 1234,
        "stats": {"kind": "standard", "strength": 60, "agility": 40, "vitality": 50, "energy": 30},
        "unspent_points": 15,
        "zen": 250_000,
        "placement": serde_json::from_str::<serde_json::Value>(PLACEMENT_JSON).unwrap(),
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 200, "max": 200},
            "ability": {"current": 1, "max": 1}
        },
        "active_effects": []
    }))
    .unwrap();
    assert_eq!(
        serde_json::to_string(&character).unwrap(),
        format!(
            r#"{{"class":"dark_knight","level":42,"experience":1234,"stats":{{"kind":"standard","strength":60,"agility":40,"vitality":50,"energy":30}},"unspent_points":15,"zen":250000,"placement":{PLACEMENT_JSON},"vitals":{{"health":{{"current":500,"max":500}},"mana":{{"current":200,"max":200}},"ability":{{"current":1,"max":1}}}},"active_effects":[],"life":{{"kind":"alive"}},"discovered":[0]}}"#
        )
    );
}

#[test]
fn character_multi_map_discovered_set_round_trips_as_bare_map_numbers() {
    // A traveled character's persisted set is a flat, sorted array of bare map
    // numbers, and the record re-loads through the current-map parse gate.
    let character: Character = serde_json::from_value(serde_json::json!({
        "class": "dark_knight",
        "level": 60,
        "experience": 1234,
        "stats": {"kind": "standard", "strength": 60, "agility": 40, "vitality": 50, "energy": 30},
        "unspent_points": 0,
        "zen": 10_000,
        "placement": serde_json::from_str::<serde_json::Value>(PLACEMENT_JSON).unwrap(),
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 200, "max": 200},
            "ability": {"current": 1, "max": 1}
        },
        "discovered": [4, 0, 2]
    }))
    .unwrap();
    let json = serde_json::to_string(&character).unwrap();
    assert!(json.ends_with(r#""discovered":[0,2,4]}"#), "{json}");
    assert_eq!(serde_json::from_str::<Character>(&json).unwrap(), character);
}

#[test]
fn item_price_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&ItemPrice::Fixed { zen: Zen(810_000) }).unwrap(),
        r#"{"kind":"fixed","zen":810000}"#
    );
    assert_eq!(
        serde_json::to_string(&ItemPrice::PerLevel {
            zen_by_level: PerLevelPrice::try_from(vec![Zen(180_000), Zen(7_500_000)]).unwrap(),
        })
        .unwrap(),
        r#"{"kind":"per_level","zen_by_level":[180000,7500000]}"#
    );
    assert_eq!(
        serde_json::to_string(&ItemPrice::Formula).unwrap(),
        r#"{"kind":"formula"}"#
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
        r#"{"instance":{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}},"position":{"x":163840,"y":229376},"map":0,"despawn":1200}"#
    );
}

#[test]
fn world_zen_wire_is_pinned() {
    let world_zen = WorldZen {
        amount: Zen(40_000),
        position: WorldPos::clamped(163_840, 229_376),
        map: MapNumber(0),
        despawn: Tick(1200),
    };
    assert_eq!(
        serde_json::to_string(&world_zen).unwrap(),
        r#"{"amount":40000,"position":{"x":163840,"y":229376},"map":0,"despawn":1200}"#
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
        r#"{"kind":"rejected","reason":{"kind":"cells_occupied"},"item":{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}}}"#
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

// -- NPC-shop outcome and zen-pickup wire pins. --------------------------------

#[test]
fn shop_outcome_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&BuyOutcome::NewItem {
            at: Cell { row: 0, col: 0 },
            balance: CarriedZen::new(497_600).unwrap(),
        })
        .unwrap(),
        r#"{"kind":"new_item","at":{"row":0,"col":0},"balance":497600}"#
    );
    assert_eq!(
        serde_json::to_string(&BuyOutcome::Merged {
            at: Cell { row: 2, col: 5 },
            balance: CarriedZen::new(980).unwrap(),
        })
        .unwrap(),
        r#"{"kind":"merged","at":{"row":2,"col":5},"balance":980}"#
    );
    assert_eq!(
        serde_json::to_string(&SellOutcome::Sold {
            proceeds: Zen(820),
            balance: CarriedZen::new(250_820).unwrap(),
        })
        .unwrap(),
        r#"{"kind":"sold","proceeds":820,"balance":250820}"#
    );
    assert_eq!(
        serde_json::to_string(&RepairOutcome::Repaired {
            cost: Zen(210),
            balance: CarriedZen::new(790).unwrap(),
        })
        .unwrap(),
        r#"{"kind":"repaired","cost":210,"balance":790}"#
    );
    assert_eq!(
        serde_json::to_string(&RepairAllOutcome::Walked {
            slots: vec![SlotRepair {
                slot: EquipmentSlot::LeftHand,
                result: SlotRepairResult::Unaffordable { cost: Zen(50) },
            }],
            balance: CarriedZen::new(49).unwrap(),
        })
        .unwrap(),
        r#"{"kind":"walked","slots":[{"slot":"left_hand","result":{"kind":"unaffordable","cost":50}}],"balance":49}"#
    );
    assert_eq!(
        serde_json::to_string(&ZenPickupOutcome::PickedUp).unwrap(),
        r#"{"kind":"picked_up"}"#
    );
    let over_cap = ZenPickupOutcome::OverCap {
        world_zen: WorldZen {
            amount: Zen(40_000),
            position: WorldPos::clamped(163_840, 229_376),
            map: MapNumber(0),
            despawn: Tick(1200),
        },
    };
    assert_eq!(
        serde_json::to_string(&over_cap).unwrap(),
        r#"{"kind":"over_cap","world_zen":{"amount":40000,"position":{"x":163840,"y":229376},"map":0,"despawn":1200}}"#
    );
}

#[test]
fn buy_outcome_every_kind_tag_is_pinned() {
    let balance = CarriedZen::new(0).unwrap();
    let at = Cell { row: 0, col: 0 };
    for outcome in [
        BuyOutcome::NewItem { at, balance },
        BuyOutcome::Merged { at, balance },
        BuyOutcome::OutOfRange,
        BuyOutcome::UnknownShelfSlot,
        BuyOutcome::InventoryFull,
        BuyOutcome::InsufficientZen,
    ] {
        let expected = match &outcome {
            BuyOutcome::NewItem { .. } => "new_item",
            BuyOutcome::Merged { .. } => "merged",
            BuyOutcome::OutOfRange => "out_of_range",
            BuyOutcome::UnknownShelfSlot => "unknown_shelf_slot",
            BuyOutcome::InventoryFull => "inventory_full",
            BuyOutcome::InsufficientZen => "insufficient_zen",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn sell_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        SellOutcome::Sold {
            proceeds: Zen(0),
            balance: CarriedZen::new(0).unwrap(),
        },
        SellOutcome::OutOfRange,
        SellOutcome::NoItemAtCell,
        SellOutcome::WalletFull,
    ] {
        let expected = match &outcome {
            SellOutcome::Sold { .. } => "sold",
            SellOutcome::OutOfRange => "out_of_range",
            SellOutcome::NoItemAtCell => "no_item_at_cell",
            SellOutcome::WalletFull => "wallet_full",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn repair_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        RepairOutcome::Repaired {
            cost: Zen(1),
            balance: CarriedZen::new(0).unwrap(),
        },
        RepairOutcome::AlreadyFull,
        RepairOutcome::NotRepairableKind,
        RepairOutcome::Empty,
        RepairOutcome::OutOfRange,
        RepairOutcome::InsufficientZen,
    ] {
        let expected = match &outcome {
            RepairOutcome::Repaired { .. } => "repaired",
            RepairOutcome::AlreadyFull => "already_full",
            RepairOutcome::NotRepairableKind => "not_repairable_kind",
            RepairOutcome::Empty => "empty",
            RepairOutcome::OutOfRange => "out_of_range",
            RepairOutcome::InsufficientZen => "insufficient_zen",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn repair_all_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        RepairAllOutcome::OutOfRange,
        RepairAllOutcome::Walked {
            slots: Vec::new(),
            balance: CarriedZen::new(0).unwrap(),
        },
    ] {
        let expected = match &outcome {
            RepairAllOutcome::OutOfRange => "out_of_range",
            RepairAllOutcome::Walked { .. } => "walked",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
    for result in [
        SlotRepairResult::Repaired { cost: Zen(1) },
        SlotRepairResult::AlreadyFull,
        SlotRepairResult::Empty,
        SlotRepairResult::Unaffordable { cost: Zen(1) },
    ] {
        let expected = match &result {
            SlotRepairResult::Repaired { .. } => "repaired",
            SlotRepairResult::AlreadyFull => "already_full",
            SlotRepairResult::Empty => "empty",
            SlotRepairResult::Unaffordable { .. } => "unaffordable",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(result).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn zen_pickup_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        ZenPickupOutcome::PickedUp,
        ZenPickupOutcome::OverCap {
            world_zen: WorldZen {
                amount: Zen(1),
                position: WorldPos::clamped(0, 0),
                map: MapNumber(0),
                despawn: Tick(0),
            },
        },
    ] {
        let expected = match &outcome {
            ZenPickupOutcome::PickedUp => "picked_up",
            ZenPickupOutcome::OverCap { .. } => "over_cap",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

// -- Chaos-machine mix outcome wire pins. --------------------------------------

#[test]
fn mix_outcome_wire_shapes_are_pinned() {
    assert_eq!(
        serde_json::to_string(&MixOutcome::Rejected {
            reason: RejectReason::InsufficientZen,
            items: vec![normal_instance()],
        })
        .unwrap(),
        r#"{"kind":"rejected","reason":"insufficient_zen","items":[{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}}]}"#
    );
    assert_eq!(
        serde_json::to_string(&MixOutcome::Failed {
            fee: Zen(250_000),
            zen: CarriedZen::new(750_000).unwrap(),
            casualties: vec![Casualty::Destroyed {
                item: ItemRef {
                    group: 12,
                    number: 15,
                },
            }],
        })
        .unwrap(),
        r#"{"kind":"failed","fee":250000,"zen":750000,"casualties":[{"kind":"destroyed","item":{"group":12,"number":15}}]}"#
    );
    assert_eq!(
        serde_json::to_string(&MixOutcome::Success {
            fee: Zen(5_000_000),
            zen: CarriedZen::new(0).unwrap(),
            created: normal_instance(),
            returned: Vec::new(),
        })
        .unwrap(),
        r#"{"kind":"success","fee":5000000,"zen":0,"created":{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}},"returned":[]}"#
    );
}

#[test]
fn mix_outcome_every_kind_tag_is_pinned() {
    for outcome in [
        MixOutcome::Rejected {
            reason: RejectReason::NoRecipeMatch,
            items: Vec::new(),
        },
        MixOutcome::Failed {
            fee: Zen(1),
            zen: CarriedZen::new(0).unwrap(),
            casualties: Vec::new(),
        },
        MixOutcome::Success {
            fee: Zen(1),
            zen: CarriedZen::new(0).unwrap(),
            created: normal_instance(),
            returned: Vec::new(),
        },
    ] {
        let expected = match &outcome {
            MixOutcome::Rejected { .. } => "rejected",
            MixOutcome::Failed { .. } => "failed",
            MixOutcome::Success { .. } => "success",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(outcome).unwrap()),
            Some(expected)
        );
    }
}

#[test]
fn reject_reason_every_wire_string_is_pinned() {
    for reason in [RejectReason::NoRecipeMatch, RejectReason::InsufficientZen] {
        let actual = serde_json::to_string(&reason).unwrap();
        let expected = match &reason {
            RejectReason::NoRecipeMatch => r#""no_recipe_match""#,
            RejectReason::InsufficientZen => r#""insufficient_zen""#,
        };
        assert_eq!(actual, expected);
    }
}

#[test]
fn casualty_every_kind_tag_is_pinned() {
    for casualty in [
        Casualty::Destroyed {
            item: ItemRef {
                group: 12,
                number: 15,
            },
        },
        Casualty::Downgraded {
            item: normal_instance(),
        },
        Casualty::Returned {
            item: normal_instance(),
        },
    ] {
        let expected = match &casualty {
            Casualty::Destroyed { .. } => "destroyed",
            Casualty::Downgraded { .. } => "downgraded",
            Casualty::Returned { .. } => "returned",
        };
        assert_eq!(
            kind_tag(&serde_json::to_value(casualty).unwrap()),
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

// -- Player-trade wire pins. -----------------------------------------------------

#[test]
fn trade_session_wire_is_pinned_with_windows_and_escrow_as_bare_integers() {
    assert_eq!(
        serde_json::to_string(&TradeSession::Requested).unwrap(),
        r#"{"kind":"requested"}"#
    );
    let session = TradeSession::Open {
        offers: TradeOffers::empty().with(
            Side::Requester,
            TradeOffer::empty()
                .with_window(
                    TradeWindow::empty()
                        .place(
                            Cell { row: 0, col: 0 },
                            Footprint::new(1, 1).unwrap(),
                            normal_instance(),
                        )
                        .unwrap(),
                )
                .with_escrow_zen(Zen(400_000)),
        ),
        locks: TradeLocks::NeitherLocked,
    };
    assert_eq!(
        serde_json::to_string(&session).unwrap(),
        concat!(
            r#"{"kind":"open","offers":{"requester":{"window":{"rows":4,"cols":8,"placed":"#,
            r#"[{"anchor":{"row":0,"col":0},"footprint":{"width":1,"height":1},"item":"#,
            r#"{"item":{"group":0,"number":3},"level":0,"roll":{"kind":"normal"},"normal_option":null,"#,
            r#""luck":"plain","skill":"no_skill","durability":{"current":30,"max":30},"augment":{"kind":"none"}}}]},"#,
            r#""escrow_zen":400000},"partner":{"window":{"rows":4,"cols":8,"placed":[]},"escrow_zen":0}},"#,
            r#""locks":{"kind":"neither_locked"}}"#,
        )
    );
    // The escrowed instance survives the wire unchanged — no augment or
    // durability drift across escrow.
    let reparsed: TradeSession =
        serde_json::from_str(&serde_json::to_string(&session).unwrap()).unwrap();
    let TradeSession::Open { offers, .. } = reparsed else {
        panic!("still open");
    };
    let placed = offers.get(Side::Requester).window().placed();
    assert_eq!(placed.first().unwrap().item, normal_instance());
}

#[test]
fn trade_locks_and_bounced_proof_wire_pins() {
    assert_eq!(
        serde_json::to_string(&TradeLocks::NeitherLocked).unwrap(),
        r#"{"kind":"neither_locked"}"#
    );
    assert_eq!(
        serde_json::to_string(&TradeLocks::OneLocked {
            side: Side::Requester
        })
        .unwrap(),
        r#"{"kind":"one_locked","side":"requester"}"#
    );
    assert_eq!(
        serde_json::to_string(&BouncedProof::Both {
            requester: SideFailure::ItemsDoNotFit,
            partner: SideFailure::WalletWouldOverflow,
        })
        .unwrap(),
        r#"{"kind":"both","requester":"items_do_not_fit","partner":"wallet_would_overflow"}"#
    );
}

#[test]
fn trade_outcome_every_kind_tag_is_pinned() {
    let at = Cell { row: 0, col: 0 };
    for (value, expected) in [
        (
            serde_json::to_value(OfferOutcome::Offered { at }).unwrap(),
            "offered",
        ),
        (
            serde_json::to_value(OfferOutcome::NotOpen).unwrap(),
            "not_open",
        ),
        (
            serde_json::to_value(OfferOutcome::SideLocked).unwrap(),
            "side_locked",
        ),
        (
            serde_json::to_value(OfferOutcome::NoItemAtSource).unwrap(),
            "no_item_at_source",
        ),
        (
            serde_json::to_value(OfferOutcome::WindowCellsOccupied).unwrap(),
            "window_cells_occupied",
        ),
        (
            serde_json::to_value(OfferOutcome::WindowOutOfBounds).unwrap(),
            "window_out_of_bounds",
        ),
        (
            serde_json::to_value(WithdrawOutcome::Withdrawn { at }).unwrap(),
            "withdrawn",
        ),
        (
            serde_json::to_value(WithdrawOutcome::NoItemAtWindowCell).unwrap(),
            "no_item_at_window_cell",
        ),
        (
            serde_json::to_value(WithdrawOutcome::InventoryFull).unwrap(),
            "inventory_full",
        ),
        (
            serde_json::to_value(ZenOfferOutcome::Offered {
                escrowed: Zen(1),
                wallet: CarriedZen::new(0).unwrap(),
            })
            .unwrap(),
            "offered",
        ),
        (
            serde_json::to_value(ZenOfferOutcome::Unaffordable {
                wallet: CarriedZen::new(0).unwrap(),
            })
            .unwrap(),
            "unaffordable",
        ),
        (
            serde_json::to_value(ZenOfferOutcome::WalletFull).unwrap(),
            "wallet_full",
        ),
        (
            serde_json::to_value(RearrangeOutcome::Rearranged { from: at, to: at }).unwrap(),
            "rearranged",
        ),
        (
            serde_json::to_value(UnlockOutcome::Unlocked).unwrap(),
            "unlocked",
        ),
        (
            serde_json::to_value(UnlockOutcome::AlreadyUnlocked).unwrap(),
            "already_unlocked",
        ),
    ] {
        assert_eq!(kind_tag(&value), Some(expected));
    }
}

#[test]
fn trade_session_outcome_every_kind_tag_is_pinned() {
    for (value, expected) in [
        (
            serde_json::to_value(RequestOutcome::Opened {
                session: TradeSession::Requested,
            })
            .unwrap(),
            "opened",
        ),
        (
            serde_json::to_value(RequestOutcome::Rejected {
                reason: RequestRejection::SelfTrade,
            })
            .unwrap(),
            "rejected",
        ),
        (
            serde_json::to_value(AcceptOutcome::Accepted {
                session: TradeSession::opened(),
            })
            .unwrap(),
            "accepted",
        ),
        (
            serde_json::to_value(AcceptOutcome::WrongSide {
                session: TradeSession::Requested,
            })
            .unwrap(),
            "wrong_side",
        ),
        (
            serde_json::to_value(AcceptOutcome::NotRequested {
                session: TradeSession::opened(),
            })
            .unwrap(),
            "not_requested",
        ),
        (
            serde_json::to_value(LockResult::Locked {
                session: TradeSession::opened(),
            })
            .unwrap(),
            "locked",
        ),
        (
            serde_json::to_value(LockResult::AlreadyLocked {
                session: TradeSession::opened(),
            })
            .unwrap(),
            "already_locked",
        ),
        (
            serde_json::to_value(LockResult::Bounced {
                session: TradeSession::opened(),
                proof: BouncedProof::Requester {
                    failure: SideFailure::ItemsAndWallet,
                },
            })
            .unwrap(),
            "bounced",
        ),
        (
            serde_json::to_value(LockResult::Completed).unwrap(),
            "completed",
        ),
        (
            serde_json::to_value(TradeAvailability::SameCharacter).unwrap(),
            "same_character",
        ),
        (
            serde_json::to_value(TradeAvailability::Busy).unwrap(),
            "busy",
        ),
        (
            serde_json::to_value(TradeAvailability::Dead).unwrap(),
            "dead",
        ),
    ] {
        assert_eq!(kind_tag(&value), Some(expected));
    }
}

#[test]
fn trade_event_every_kind_tag_is_pinned() {
    let at = Cell { row: 1, col: 2 };
    for (event, expected) in [
        (TradeEvent::Opened, "opened"),
        (TradeEvent::Accepted, "accepted"),
        (
            TradeEvent::ItemOffered {
                by: Side::Requester,
                at,
            },
            "item_offered",
        ),
        (
            TradeEvent::ItemWithdrawn {
                by: Side::Partner,
                at,
            },
            "item_withdrawn",
        ),
        (
            TradeEvent::ItemRearranged {
                by: Side::Requester,
                from: at,
                to: at,
            },
            "item_rearranged",
        ),
        (
            TradeEvent::ZenOffered {
                by: Side::Partner,
                amount: Zen(9),
            },
            "zen_offered",
        ),
        (
            TradeEvent::Locked {
                by: Side::Requester,
            },
            "locked",
        ),
        (
            TradeEvent::Unlocked {
                by: Side::Requester,
            },
            "unlocked",
        ),
        (
            TradeEvent::DealChanged { by: Side::Partner },
            "deal_changed",
        ),
        (TradeEvent::Completed, "completed"),
        (
            TradeEvent::Cancelled {
                reason: CancelReason::Declined,
            },
            "cancelled",
        ),
    ] {
        assert_eq!(
            kind_tag(&serde_json::to_value(event).unwrap()),
            Some(expected)
        );
    }
    assert_eq!(
        serde_json::to_string(&TradeEvent::Cancelled {
            reason: CancelReason::Declined,
        })
        .unwrap(),
        r#"{"kind":"cancelled","reason":"declined"}"#
    );
}

fn party_of_three_with_a_held_seat() -> PartySession {
    PartySession::forming().with_member(PartyMember {
        slot: MemberSlot(2),
        membership: Membership::Held {
            expires: Tick(1300),
        },
    })
}

#[test]
fn party_membership_leadership_and_session_wire_is_pinned() {
    // Positional identity is a bare integer.
    assert_eq!(serde_json::to_string(&MemberSlot(2)).unwrap(), "2");

    assert_eq!(
        serde_json::to_string(&Membership::Active).unwrap(),
        r#"{"kind":"active"}"#
    );
    assert_eq!(
        serde_json::to_string(&Membership::Held {
            expires: Tick(1300)
        })
        .unwrap(),
        r#"{"kind":"held","expires":1300}"#
    );
    assert_eq!(
        serde_json::to_string(&Leadership::Led { by: MemberSlot(0) }).unwrap(),
        r#"{"kind":"led","by":0}"#
    );
    assert_eq!(
        serde_json::to_string(&Leadership::Vacant).unwrap(),
        r#"{"kind":"vacant"}"#
    );
    assert_eq!(
        serde_json::to_string(&Vitality::Alive).unwrap(),
        r#"{"kind":"alive"}"#
    );
    assert_eq!(
        serde_json::to_string(&Vitality::Dead).unwrap(),
        r#"{"kind":"dead"}"#
    );

    let session = party_of_three_with_a_held_seat();
    assert_eq!(
        serde_json::to_string(&session).unwrap(),
        concat!(
            r#"{"members":[{"slot":0,"membership":{"kind":"active"}},"#,
            r#"{"slot":1,"membership":{"kind":"active"}},"#,
            r#"{"slot":2,"membership":{"kind":"held","expires":1300}}],"#,
            r#""leadership":{"kind":"led","by":0}}"#,
        )
    );
    // The live session survives the wire mid-lifecycle unchanged.
    let json = serde_json::to_string(&session).unwrap();
    assert_eq!(
        serde_json::from_str::<PartySession>(&json).unwrap(),
        session
    );

    assert_eq!(
        serde_json::to_string(&PartyInvite { expires: Tick(660) }).unwrap(),
        r#"{"expires":660}"#
    );
}

#[test]
fn party_outcome_every_kind_tag_is_pinned() {
    let invite = PartyInvite { expires: Tick(660) };
    let session = party_of_three_with_a_held_seat();
    for (value, expected) in [
        (
            serde_json::to_value(party::InviteOutcome::Sent { invite }).unwrap(),
            "sent",
        ),
        (
            serde_json::to_value(party::InviteOutcome::Rejected {
                reason: InviteRejection::PartyFull,
            })
            .unwrap(),
            "rejected",
        ),
        (
            serde_json::to_value(party::AcceptOutcome::Joined {
                session: session.clone(),
            })
            .unwrap(),
            "joined",
        ),
        (
            serde_json::to_value(party::AcceptOutcome::Bounced {
                reason: AcceptBounce::InviterGone,
            })
            .unwrap(),
            "bounced",
        ),
        (
            serde_json::to_value(party::KickOutcome::Kicked {
                session: session.clone(),
            })
            .unwrap(),
            "kicked",
        ),
        (
            serde_json::to_value(party::KickOutcome::Disbanded).unwrap(),
            "disbanded",
        ),
        (
            serde_json::to_value(party::KickOutcome::NotLeader).unwrap(),
            "not_leader",
        ),
        (
            serde_json::to_value(party::KickOutcome::NoSuchMember).unwrap(),
            "no_such_member",
        ),
        (
            serde_json::to_value(party::KickOutcome::CannotKickSelf).unwrap(),
            "cannot_kick_self",
        ),
        (
            serde_json::to_value(party::LeaveOutcome::Left {
                session: session.clone(),
            })
            .unwrap(),
            "left",
        ),
        (
            serde_json::to_value(party::LeaveOutcome::Disbanded).unwrap(),
            "disbanded",
        ),
        (
            serde_json::to_value(party::DisconnectOutcome::Disconnected {
                session: session.clone(),
            })
            .unwrap(),
            "disconnected",
        ),
        (
            serde_json::to_value(party::ReconnectOutcome::Reconnected { session }).unwrap(),
            "reconnected",
        ),
        (
            serde_json::to_value(party::PartyOutcome::Disbanded).unwrap(),
            "disbanded",
        ),
        (
            serde_json::to_value(party::InviteSweep::Pending { invite }).unwrap(),
            "pending",
        ),
        (
            serde_json::to_value(party::InviteSweep::Lapsed).unwrap(),
            "lapsed",
        ),
    ] {
        assert_eq!(kind_tag(&value), Some(expected));
    }
}

#[test]
fn party_event_award_and_refusal_wire_is_pinned() {
    assert_eq!(
        serde_json::to_string(&PartyEvent::Joined {
            slot: MemberSlot(1)
        })
        .unwrap(),
        r#"{"kind":"joined","slot":1}"#
    );
    for (event, expected) in [
        (PartyEvent::InviteSent, "invite_sent"),
        (PartyEvent::InviteReceived, "invite_received"),
        (PartyEvent::InviteDeclined, "invite_declined"),
        (PartyEvent::InviteExpired, "invite_expired"),
        (
            PartyEvent::Joined {
                slot: MemberSlot(1),
            },
            "joined",
        ),
        (
            PartyEvent::MemberKicked {
                slot: MemberSlot(2),
            },
            "member_kicked",
        ),
        (
            PartyEvent::MemberLeft {
                slot: MemberSlot(0),
            },
            "member_left",
        ),
        (
            PartyEvent::MemberHeld {
                slot: MemberSlot(3),
            },
            "member_held",
        ),
        (
            PartyEvent::MemberReconnected {
                slot: MemberSlot(3),
            },
            "member_reconnected",
        ),
        (
            PartyEvent::LeadershipTransferred { to: MemberSlot(1) },
            "leadership_transferred",
        ),
        (
            PartyEvent::MemberExpired {
                slot: MemberSlot(4),
            },
            "member_expired",
        ),
        (PartyEvent::Disbanded, "disbanded"),
    ] {
        assert_eq!(
            kind_tag(&serde_json::to_value(event).unwrap()),
            Some(expected)
        );
    }

    // MemberAward carries only components — a flat slot / gained / level-ups record.
    assert_eq!(
        serde_json::to_string(&MemberAward {
            slot: MemberSlot(2),
            gained: Exp(1137),
            level_ups: vec![LevelUp {
                level: Level::new(31).unwrap(),
            }],
        })
        .unwrap(),
        r#"{"slot":2,"gained":1137,"level_ups":[{"level":31}]}"#
    );

    // Named refusals are bare snake_case strings.
    assert_eq!(
        serde_json::to_string(&InviteRejection::OutOfRange).unwrap(),
        r#""out_of_range""#
    );
    assert_eq!(
        serde_json::to_string(&AcceptBounce::InviterNotLeader).unwrap(),
        r#""inviter_not_leader""#
    );
}
