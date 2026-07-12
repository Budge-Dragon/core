//! The monster-kill decay accelerator over the real `/data` Atlas: a flagged
//! murderer works the flag off by hunting, buying one monster-level-second of
//! decay per kill, while a clean killer or a level-less victim (a passive town
//! monster or the soccer ball) leaves the killer untouched. The victim's level
//! is read from its authoritative definition, so the pull cannot be forged.
//!
//! Runs against the real dataset because the accelerator resolves the victim's
//! level through the parsed [`Atlas`]; the shared harness (`common/dataset.rs`)
//! is the only place a real Atlas is built. Load failures route through
//! `or_abort`; every assertion is a `#[test]` body.

#[path = "common/dataset.rs"]
mod dataset;

use mu_core::components::active_effect::ActiveEffects;
use mu_core::components::movement::Movement;
use mu_core::components::placement::Placement;
use mu_core::components::pool::Pool;
use mu_core::components::reputation::{PkStage, PlayerKillCount, Reputation, Standing};
use mu_core::components::spatial::Facing;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{DurationMs, Exp, Level, MapNumber, Tick, TickDuration, Ticks};
use mu_core::data::atlas::Atlas;
use mu_core::data::common::MonsterNumber;
use mu_core::data::monster_definitions::MonsterRole;
use mu_core::entities::character::Character;
use mu_core::entities::monster_instance::MonsterInstance;
use mu_core::events::death::DeathEvent;
use mu_core::events::reputation::{PkEvent, SanctionReason};
use mu_core::services::death::{DeathPenalty, resolve_death};
use mu_core::services::reputation::{
    KillSanction, PvpContext, accelerate_reputation_decay, decay_reputation, player_kill_sanction,
    resolve_player_kill,
};

use dataset::{or_abort, real_atlas};
use serde_json::json;

/// The suite tick base: 50 ms per tick.
fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// One online hour as a tick span — the flat decay step, computed the same way
/// the service does (`PK_DECAY_STEP_MS` is private, so it is reconstructed from
/// its documented `3_600_000` ms value).
fn hour() -> Ticks {
    DurationMs(3_600_000).in_ticks(tick())
}

/// `n` whole online hours as a tick span.
fn hours(n: u64) -> Ticks {
    Ticks(hour().0 * n)
}

/// A gearless clean character — built the only way an external caller can, by
/// deserialising its wire form with an explicit clean reputation.
fn clean_char() -> Character {
    char_with_reputation(&or_abort(serde_json::to_value(Reputation::clean())))
}

/// A character flagged at `stage` with the given decay deadline and a zero kill
/// tally, seated through the character's reputation wire field.
fn flagged_char(stage: PkStage, decays_at: Tick) -> Character {
    let standing = or_abort(serde_json::to_value(Standing::Flagged { stage, decays_at }));
    char_with_reputation(&json!({ "standing": standing, "kills": 0 }))
}

/// Deserialises a gearless Dark Knight carrying the given reputation wire value.
fn char_with_reputation(reputation: &serde_json::Value) -> Character {
    let json = json!({
        "class": "dark_knight",
        "level": 50,
        "experience": 0,
        "stats": {"kind": "standard", "strength": 200, "agility": 100, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {
            "position": or_abort(serde_json::to_value(TileCoord::new(180, 120).to_world())),
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": 0
        },
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        },
        "reputation": reputation
    });
    or_abort(serde_json::from_value(json))
}

/// A live instance of the first fighting monster the dataset carries at exactly
/// `level` — the accelerator reads only its `number`, so an arbitrary valid
/// placement and health round it out.
fn monster_of_level(atlas: &Atlas, level: u16) -> MonsterInstance {
    let number = or_abort(
        atlas
            .monsters()
            .find_map(|definition| match definition.role {
                MonsterRole::Monster { combat, .. }
                | MonsterRole::Guard { combat, .. }
                | MonsterRole::Trap { combat, .. } => {
                    (combat.level.get() == level).then_some(definition.number)
                }
                MonsterRole::Npc { .. } | MonsterRole::SoccerBall => None,
            })
            .ok_or_else(|| format!("the dataset has no fighting monster at level {level}")),
    );
    instance(number)
}

/// A live instance of the first passive NPC the dataset carries — a level-less
/// victim the accelerator must treat as a no-op.
fn passive_npc(atlas: &Atlas) -> MonsterInstance {
    let number = or_abort(
        atlas
            .monsters()
            .find_map(|definition| match definition.role {
                MonsterRole::Npc { .. } => Some(definition.number),
                MonsterRole::Monster { .. }
                | MonsterRole::Guard { .. }
                | MonsterRole::Trap { .. }
                | MonsterRole::SoccerBall => None,
            })
            .ok_or("the dataset has no passive NPC"),
    );
    instance(number)
}

/// A live monster instance for `number` with an arbitrary valid placement — the
/// accelerator reads only `number`, so the rest is filler.
fn instance(number: MonsterNumber) -> MonsterInstance {
    MonsterInstance {
        number,
        placement: Placement {
            position: TileCoord::new(20, 20).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        },
        health: Pool::full(60),
        anchor: TileCoord::new(20, 20).to_world(),
        next_action: Tick(0),
        active_effects: ActiveEffects::EMPTY,
    }
}

/// A gearless Dark Knight seeded mid-band in `level`'s experience band over the
/// real curve — so a monster death docks a real, non-zero percentage of the band
/// — carrying the given reputation wire value. The mid-band seed keeps the dock
/// well inside the band, so the level floor never bites (no de-level).
fn banded_char(atlas: &Atlas, level: u16, reputation: &serde_json::Value) -> Character {
    let curve = atlas.exp_curve();
    let floor = or_abort(curve.level(level)).total_to_hold().0;
    let next = or_abort(curve.level(level + 1)).total_to_hold().0;
    let experience = floor + (next - floor) / 2;
    let json = json!({
        "class": "dark_knight",
        "level": level,
        "experience": experience,
        "stats": {"kind": "standard", "strength": 200, "agility": 100, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {
            "position": or_abort(serde_json::to_value(TileCoord::new(180, 120).to_world())),
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": 0
        },
        "vitals": {
            "health": {"current": 500, "max": 500},
            "mana": {"current": 400, "max": 400},
            "ability": {"current": 400, "max": 400}
        },
        "reputation": reputation
    });
    or_abort(serde_json::from_value(json))
}

/// The reputation wire value of a clean character.
fn clean_reputation() -> serde_json::Value {
    or_abort(serde_json::to_value(Reputation::clean()))
}

/// The reputation wire value of a character flagged at `stage` (the deadline is
/// irrelevant to the exp-loss fork, which reads only the stage and level band).
fn flagged_reputation(stage: PkStage) -> serde_json::Value {
    let standing = or_abort(serde_json::to_value(Standing::Flagged {
        stage,
        decays_at: Tick(1),
    }));
    json!({ "standing": standing, "kills": 0 })
}

/// The absolute decay deadline of a flagged standing, aborting on a clean one.
fn deadline(standing: Standing) -> Tick {
    match standing {
        Standing::Flagged { decays_at, .. } => decays_at,
        Standing::Clean => or_abort(Err::<Tick, _>("expected a flagged standing")),
    }
}

/// The experience a death docked — the `lost` an `ExperienceDocked` carries,
/// aborting when the death docked no experience (the callers seed a mid-band
/// level so a dock is always produced).
fn exp_docked(events: &[DeathEvent]) -> Exp {
    or_abort(
        events
            .iter()
            .find_map(|event| match event {
                DeathEvent::ExperienceDocked { lost, .. } => Some(*lost),
                DeathEvent::Died { .. } | DeathEvent::ZenDocked { .. } => None,
            })
            .ok_or("the mid-band death docks experience"),
    )
}

#[test]
fn accelerator_pulls_the_deadline_in_by_the_monster_level_without_peeling() {
    let atlas = real_atlas();
    let victim = monster_of_level(&atlas, 40);
    let killer = flagged_char(PkStage::FirstStage, Tick(0) + hours(5));
    let (killer, ev) = accelerate_reputation_decay(killer, &victim, &atlas, tick());
    let reduced = DurationMs(40 * 1000).in_ticks(tick());
    assert_eq!(
        killer.reputation().standing(),
        Standing::Flagged {
            stage: PkStage::FirstStage,
            decays_at: (Tick(0) + hours(5)) - reduced
        }
    );
    assert!(matches!(
        ev,
        Some(PkEvent::DecayAccelerated { reduced_by, .. }) if reduced_by == reduced
    ));
}

#[test]
fn accelerator_is_a_noop_on_a_clean_killer_or_a_levelless_victim() {
    let atlas = real_atlas();
    let (clean, ev) =
        accelerate_reputation_decay(clean_char(), &monster_of_level(&atlas, 40), &atlas, tick());
    assert_eq!(clean.reputation(), Reputation::clean());
    assert!(ev.is_none());

    let flagged = flagged_char(PkStage::Warning, Tick(9));
    let (after, ev) = accelerate_reputation_decay(flagged, &passive_npc(&atlas), &atlas, tick());
    assert_eq!(
        after.reputation().standing(),
        Standing::Flagged {
            stage: PkStage::Warning,
            decays_at: Tick(9)
        }
    );
    assert!(ev.is_none());
}

#[test]
fn a_murderer_flags_climbs_accelerates_decays_and_a_hunted_victim_is_free_to_kill() {
    // The whole open-world murderer lifecycle wired across the reputation
    // services over the real dataset: flag, climb, the free-kill carve-out, the
    // monster-kill accelerator, and the tick-driven decay back to clean — every
    // sanction core-computed from the victim's authoritative reputation.
    let atlas = real_atlas();

    // A clean player commits an unsanctioned open kill of a clean victim: it
    // flags Warning, not yet hunted, with the lifetime tally at one.
    let killer = clean_char();
    let sanction = player_kill_sanction(&clean_char(), PvpContext::Open);
    assert!(matches!(sanction, KillSanction::Unsanctioned));
    let (killer, ev) = resolve_player_kill(killer, sanction, Tick(1000), tick());
    assert!(matches!(
        ev,
        PkEvent::Flagged {
            stage: PkStage::Warning,
            ..
        }
    ));
    assert!(!killer.reputation().standing().is_hunted());
    assert_eq!(killer.reputation().kills(), PlayerKillCount(1));

    // A second open kill climbs to FirstStage — now hunted (guard-huntable) and
    // itself free-to-kill; the lifetime tally is two.
    let (killer, ev) = resolve_player_kill(
        killer,
        player_kill_sanction(&clean_char(), PvpContext::Open),
        Tick(2000),
        tick(),
    );
    assert!(matches!(
        ev,
        PkEvent::Flagged {
            stage: PkStage::FirstStage,
            ..
        }
    ));
    assert!(killer.reputation().standing().is_hunted());
    assert_eq!(killer.reputation().kills(), PlayerKillCount(2));

    // Killing a >=FirstStage victim is free — this murderer is a free target, so
    // its killer would flag nothing.
    assert!(matches!(
        player_kill_sanction(&killer, PvpContext::Open),
        KillSanction::Sanctioned {
            reason: SanctionReason::VictimWasMurderer
        }
    ));

    // The murderer works the flag off by hunting: a monster kill accelerates the
    // decay (pulls the deadline earlier) without peeling the rung.
    let before = deadline(killer.reputation().standing());
    let (killer, ev) =
        accelerate_reputation_decay(killer, &monster_of_level(&atlas, 40), &atlas, tick());
    assert!(matches!(ev, Some(PkEvent::DecayAccelerated { .. })));
    assert!(
        deadline(killer.reputation().standing()) < before,
        "the monster kill pulls the deadline earlier"
    );
    assert!(
        killer.reputation().standing().is_hunted(),
        "the accelerator never peels a rung"
    );

    // Elapsed online time decays the murderer all the way back to Clean; the
    // lifetime tally survives the fade to clean.
    let (killer, ev) = decay_reputation(killer, Tick(10_000_000), tick());
    assert!(matches!(
        ev,
        Some(PkEvent::Decayed {
            standing: Standing::Clean
        })
    ));
    assert_eq!(killer.reputation().standing(), Standing::Clean);
    assert_eq!(killer.reputation().kills(), PlayerKillCount(2));
}

#[test]
fn a_flagged_murderers_monster_death_exp_loss_is_heavier_and_never_de_levels() {
    // The exp fork over the real curve: at one mid-band level, a flagged murderer
    // loses strictly more experience on a monster death than a clean character,
    // and the loss is floored so neither ever de-levels.
    let atlas = real_atlas();
    let level = 100u16;
    let expected = or_abort(Level::new(level));

    let clean = banded_char(&atlas, level, &clean_reputation());
    let flagged = banded_char(&atlas, level, &flagged_reputation(PkStage::SecondStage));

    let (clean_dead, clean_events) =
        resolve_death(clean, Tick(500), tick(), &atlas, DeathPenalty::Applied);
    let (flagged_dead, flagged_events) =
        resolve_death(flagged, Tick(500), tick(), &atlas, DeathPenalty::Applied);

    let clean_lost = exp_docked(&clean_events);
    let flagged_lost = exp_docked(&flagged_events);
    assert!(clean_lost.0 > 0, "a mid-band clean death docks a real 1%");
    assert!(
        flagged_lost.0 > clean_lost.0,
        "the flagged murderer's death costs strictly more experience ({} vs {})",
        flagged_lost.0,
        clean_lost.0
    );

    // The floor never de-levels: both stay at the level they died on.
    assert_eq!(clean_dead.level(), expected);
    assert_eq!(flagged_dead.level(), expected);
}
