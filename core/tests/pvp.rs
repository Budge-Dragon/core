//! The player-versus-player kill contract over the real `/data` Atlas: a player
//! kill runs the victim through the death -> respawn loop with the penalty core
//! computes from the killer's kind (waived — the victim loses nothing), and the
//! killer receives no reward. `resolve_kill` cannot even name a player victim
//! (it takes `&MonsterInstance`), so a player-kill reward is
//! type-unrepresentable rather than merely suppressed.
//!
//! Reuses the shared dataset/RNG harness (`common/dataset.rs`, `common/rng.rs`)
//! and mirrors the death suite's Applied-vs-Waived contrast: the same victim
//! docked under a monster killer proves the waiver is load-bearing.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use serde_json::json;

use mu_core::components::combat_profile::{CombatProfile, TargetKind};
use mu_core::components::pool::Pool;
use mu_core::components::units::{Tick, TickDuration};
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::events::combat::AttackOutcome;
use mu_core::events::death::DeathEvent;
use mu_core::services::combat::{StrikeBasis, resolve_attack};
use mu_core::services::death::{combat_death_penalty, resolve_death, respawn};
use mu_core::services::profile::character_profile;

use dataset::{or_abort, real_atlas};
use rng::TestRng;

/// The suite tick base: 50 ms, so the 3000 ms respawn delay is 60 whole ticks.
fn tick() -> TickDuration {
    or_abort(TickDuration::new(50))
}

/// Total experience the curve requires to hold `lvl`, read from the real table.
fn total(atlas: &Atlas, lvl: u16) -> u64 {
    or_abort(atlas.exp_curve().level(lvl)).total_to_hold().0
}

/// A gearless Dark Knight built the only way a character can be — by
/// deserialising its wire form (the class-stats gate re-proves on load) — at the
/// seeded level, experience, carried zen, and map, with a fixed stat block and
/// full vitals. A character-derived profile is always stamped `Player`.
fn dark_knight(level: u16, exp: u64, zen: u64, map: u8) -> Character {
    let json = json!({
        "class": "dark_knight",
        "level": level,
        "experience": exp,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 7,
        "zen": zen,
        "placement": {
            "position": {"x": 0, "y": 0},
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": map,
        },
        "vitals": {
            "health": {"current": 1000, "max": 1000},
            "mana": {"current": 500, "max": 500},
            "ability": {"current": 500, "max": 500},
        },
        "active_effects": [],
        "life": {"kind": "alive"},
    });
    or_abort(serde_json::from_value(json))
}

/// Whether a death event is one of the two experience/zen penalty docks — the
/// predicate the waiver removes and the monster-kill applies.
fn is_penalty_event(event: &DeathEvent) -> bool {
    matches!(
        event,
        DeathEvent::ExperienceDocked { .. } | DeathEvent::ZenDocked { .. }
    )
}

/// Drives the real single-strike path against a target health pool until the
/// strike lands a kill, feeding the reduced health back each swing. Bounded so a
/// non-lethal pairing fails loudly rather than looping forever.
fn strike_until_killed(
    attacker: &CombatProfile,
    victim: &CombatProfile,
    max_health: u32,
    rng: &mut TestRng,
) -> AttackOutcome {
    let mut health = Pool::full(max_health);
    let mut last = AttackOutcome::Missed;
    for _ in 0..4000 {
        let (next, outcome) =
            resolve_attack(attacker, victim, health, &StrikeBasis::PlainSwing, rng);
        health = next;
        last = outcome;
        if matches!(last, AttackOutcome::Killed { .. }) {
            return last;
        }
    }
    last
}

#[test]
fn a_player_kill_costs_the_victim_nothing_and_rewards_the_killer_nothing() {
    let atlas = real_atlas();

    // Two players outside a safezone: the victim seated at mid-band level-100
    // experience with a seven-figure balance so BOTH docks would engage under a
    // monster killer, and an overwhelming level-cap attacker that reliably kills.
    let t100 = total(&atlas, 100);
    let victim_exp = t100 + (total(&atlas, 101) - t100) / 2;
    let victim = dark_knight(100, victim_exp, 1_000_000, 3);
    let attacker = dark_knight(400, total(&atlas, 400), 0, 3);

    let (attacker_profile, _attacker_maxima) = character_profile(&attacker);
    let (victim_profile, victim_maxima) = character_profile(&victim);
    // Both derive from a Character, so both are stamped Player by construction:
    // this is a player-versus-player kill, never a claimed matchup.
    assert_eq!(attacker_profile.kind(), TargetKind::Player);
    assert_eq!(victim_profile.kind(), TargetKind::Player);

    let victim_exp_before = victim.experience();
    let victim_zen_before = victim.zen();

    // Strike to lethal through the same combat path any attacker uses.
    let outcome = strike_until_killed(
        &attacker_profile,
        &victim_profile,
        victim_maxima.max_health,
        &mut TestRng::new(7),
    );
    assert!(
        matches!(outcome, AttackOutcome::Killed { .. }),
        "the strike sequence reaches a kill"
    );

    // The host routes a Player victim to death + respawn only — never
    // `resolve_kill`, which takes `victim: &MonsterInstance` (kill.rs) and so
    // cannot name a Character: a player-kill reward is type-unrepresentable, not
    // merely skipped. The penalty is CORE-computed from the killer's kind
    // (`combat_death_penalty`), never a host literal; `resolve_death` and
    // `respawn` take the victim BY VALUE.
    let (dead, waived_events) = resolve_death(
        victim.clone(),
        Tick(500),
        tick(),
        &atlas,
        combat_death_penalty(TargetKind::Player), // == Waived, the core rule
    );
    assert!(
        !waived_events.iter().any(is_penalty_event),
        "a player kill docks neither experience nor zen"
    );

    // The SAME victim under a monster killer WOULD dock — the waiver is
    // load-bearing, not a no-op on a character with nothing to lose.
    let (_applied, applied_events) = resolve_death(
        victim,
        Tick(500),
        tick(),
        &atlas,
        combat_death_penalty(TargetKind::Npc), // == Applied
    );
    assert!(
        applied_events.iter().any(is_penalty_event),
        "a monster kill at these stats docks experience and zen"
    );

    // The victim runs the rest of the loop unchanged: respawn (by value) seats
    // it alive in a town safezone with its experience and zen intact.
    let (alive, respawned) = respawn(dead, &atlas, &mut TestRng::new(7));
    assert!(respawned.is_some(), "the dead victim respawns");
    assert_eq!(
        alive.experience(),
        victim_exp_before,
        "the player kill cost no experience"
    );
    assert_eq!(alive.zen(), victim_zen_before, "the player kill cost no zen");

    let landing = alive.placement();
    let grid = or_abort(
        atlas
            .terrain_grid(landing.map)
            .ok_or("the respawn map has a terrain grid"),
    );
    assert!(
        grid.safe(landing.position),
        "the victim respawns on a town safezone tile"
    );
}
