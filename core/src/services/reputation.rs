//! The player-kill reputation lifecycle: the decide step that rules a kill
//! sanctioned or not, and the apply step that flags the killer onto the
//! murderer ladder. The killer-side twin of [`crate::services::death`] —
//! [`player_kill_sanction`] mirrors
//! [`crate::services::death::combat_death_penalty`] (core reads the victim's
//! authoritative reputation, never a client claim) and [`resolve_player_kill`]
//! mirrors [`crate::services::death::resolve_death`] (killer in by value, killer
//! plus one event out). Both draw **no** randomness — the ladder climb and the
//! deadline are pure tick math.

use crate::components::reputation::{PkStage, StageDrop, Standing};
use crate::components::units::{DurationMs, Tick, TickDuration, Ticks};
use crate::entities::character::Character;
use crate::events::reputation::{PkEvent, SanctionReason};

/// One online hour — the flat step every unsanctioned kill adds to the decay
/// deadline. A CMB-CONST tunable: the fade rate is balance, not a source
/// extraction, so a future server-config wave feeds it from the Atlas.
const PK_DECAY_STEP_MS: DurationMs = DurationMs(3_600_000);

/// The circumstance a player kill happened under — a transient service input
/// the host attests, never persisted and never on the wire (like
/// [`crate::services::death::DeathPenalty`]). Every non-[`PvpContext::Open`]
/// variant is a typed socket: matched today, but no host path constructs it
/// until its wave lands (`MiniGamePvp` is host session state, available now).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PvpContext {
    /// Open-world combat with no exempting circumstance — the sanction turns on
    /// the victim's own reputation.
    Open,
    /// The victim struck first; the host attests the self-defense window.
    SelfDefense,
    /// The victim belonged to a rival guild. OpenMU has TWO guild free-kill
    /// conditions — guild-war (a shared active war score) and rival-guild
    /// (`AreGuildsRival`) — collapsed here into one socket; W-GUILD will likely
    /// split this into two variants once guild state exists.
    RivalGuild,
    /// The kill happened inside a sanctioned duel the host runs.
    Duel,
    /// The kill happened inside a player-versus-player mini-game session.
    MiniGamePvp,
}

/// Whether a kill carries a sanction (a flag) or is free — the decided output
/// of [`player_kill_sanction`], fed to [`resolve_player_kill`]. A transient
/// service value, not persisted state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillSanction {
    /// The kill flags the killer — it climbs the murderer ladder.
    Unsanctioned,
    /// The kill is free for the stated reason — no flag, no state change.
    Sanctioned {
        /// Why the kill carried no sanction.
        reason: SanctionReason,
    },
}

/// Decides whether a player kill is sanctioned. The host attests the
/// circumstance it alone can know ([`PvpContext`]); core reads the victim's
/// **authoritative** reputation itself — a client cannot claim "my victim was a
/// murderer". A host exemption frees the kill; otherwise an open kill of an
/// already-hunted murderer is free, and an open kill of anyone else flags.
#[must_use]
pub fn player_kill_sanction(victim: &Character, context: PvpContext) -> KillSanction {
    match context {
        PvpContext::SelfDefense => KillSanction::Sanctioned {
            reason: SanctionReason::SelfDefense,
        },
        PvpContext::RivalGuild => KillSanction::Sanctioned {
            reason: SanctionReason::RivalGuild,
        },
        PvpContext::Duel => KillSanction::Sanctioned {
            reason: SanctionReason::Duel,
        },
        PvpContext::MiniGamePvp => KillSanction::Sanctioned {
            reason: SanctionReason::MiniGamePvp,
        },
        PvpContext::Open => {
            if victim.reputation().standing().is_hunted() {
                KillSanction::Sanctioned {
                    reason: SanctionReason::VictimWasMurderer,
                }
            } else {
                KillSanction::Unsanctioned
            }
        }
    }
}

/// Applies a decided sanction to the killer. A sanctioned kill returns the
/// killer byte-identical with a [`PkEvent::Sanctioned`] (no state change). An
/// unsanctioned kill climbs the ladder — clean starts at
/// [`PkStage::Warning`], a flagged killer climbs one rung (saturating at the
/// cap) — stacks a flat online hour onto the later of the standing deadline and
/// `at`, and records one more lifetime kill. A pure deterministic transition:
/// killer in by value, killer plus event out, no RNG.
#[must_use]
pub fn resolve_player_kill(
    killer: Character,
    sanction: KillSanction,
    at: Tick,
    tick: TickDuration,
) -> (Character, PkEvent) {
    match sanction {
        KillSanction::Sanctioned { reason } => (killer, PkEvent::Sanctioned { reason }),
        KillSanction::Unsanctioned => {
            let (stage, base) = match killer.reputation().standing() {
                Standing::Clean => (PkStage::Warning, at),
                Standing::Flagged { stage, decays_at } => (stage.climbed(), decays_at.max(at)),
            };
            let decays_at = base + PK_DECAY_STEP_MS.in_ticks(tick);
            let reputation = killer
                .reputation()
                .with_standing(Standing::Flagged { stage, decays_at })
                .with_recorded_kill();
            (
                killer.with_reputation(reputation),
                PkEvent::Flagged {
                    stage,
                    decays_at,
                    lifetime_kills: reputation.kills(),
                },
            )
        }
    }
}

/// Peels the murderer ladder as online time reaches the decay deadline — the
/// **sole** stage-lowering path (the monster-kill accelerator only pulls the
/// deadline earlier, it never peels). A clean killer is a no-op; a flagged one
/// drops one rung per crossed boundary, re-arming a flat online hour at each
/// drop, so the first peel waits out the whole accumulated total, every later
/// rung a flat hour, and a large elapsed multi-peels toward clean in a single
/// call (call-frequency-independent). Killer in by value, killer plus at most
/// one [`PkEvent::Decayed`] out; no randomness.
#[must_use]
pub fn decay_reputation(
    character: Character,
    now: Tick,
    tick: TickDuration,
) -> (Character, Option<PkEvent>) {
    let Standing::Flagged { stage, decays_at } = character.reputation().standing() else {
        return (character, None);
    };
    match peel(stage, decays_at, now, PK_DECAY_STEP_MS.in_ticks(tick)) {
        Peeled::None => (character, None),
        Peeled::To(standing) => {
            let reputation = character.reputation().with_standing(standing);
            (
                character.with_reputation(reputation),
                Some(PkEvent::Decayed { standing }),
            )
        }
    }
}

/// The outcome of peeling: nothing has crossed the deadline yet, or the standing
/// the killer lands on once every crossed boundary is applied (a lower rung, or
/// clean off the bottom of the ladder).
enum Peeled {
    /// The deadline is still ahead of `now` — no rung peels.
    None,
    /// The standing after every crossed boundary is applied.
    To(Standing),
}

/// Drops one rung per boundary the online-time `now` has crossed, re-arming a
/// flat `step` at each drop. Terminates in at most three iterations — the ladder
/// height — because every pass either returns or lowers `stage` toward clean.
fn peel(mut stage: PkStage, mut decays_at: Tick, now: Tick, step: Ticks) -> Peeled {
    if !decays_at.reached(now) {
        return Peeled::None;
    }
    loop {
        match stage.dropped() {
            StageDrop::ToClean => return Peeled::To(Standing::Clean),
            StageDrop::To(lower) => {
                let rearmed = decays_at + step;
                stage = lower;
                decays_at = rearmed;
                if !rearmed.reached(now) {
                    return Peeled::To(Standing::Flagged { stage, decays_at });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::reputation::{PlayerKillCount, Reputation};
    use crate::components::tile::TileCoord;
    use crate::components::units::Ticks;

    fn tick() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    fn hour() -> Ticks {
        PK_DECAY_STEP_MS.in_ticks(tick())
    }

    /// `n` whole online hours as a tick span — the accumulated decay total a
    /// flagged killer must wait out.
    fn hours(n: u64) -> Ticks {
        Ticks(hour().0 * n)
    }

    /// A gearless clean character — built the only way a character can be, by
    /// deserialising its wire form; `reputation` seeds clean by serde default.
    fn clean_char() -> Character {
        let json = serde_json::json!({
            "class": "dark_knight",
            "level": 50,
            "experience": 0,
            "stats": {"kind": "standard", "strength": 200, "agility": 100, "vitality": 100, "energy": 30},
            "unspent_points": 0,
            "zen": 0,
            "placement": {
                "position": serde_json::to_value(TileCoord::new(180, 120).to_world()).unwrap(),
                "facing": {"x": 1, "y": 0},
                "movement": "grounded",
                "map": 0
            },
            "vitals": {
                "health": {"current": 500, "max": 500},
                "mana": {"current": 400, "max": 400},
                "ability": {"current": 400, "max": 400}
            }
        });
        serde_json::from_value(json).unwrap()
    }

    /// A clean character flagged at `stage` with the given deadline and a zero
    /// kill tally — the standing seated through the character's reputation gate.
    fn flagged(stage: PkStage, decays_at: Tick) -> Character {
        clean_char().with_reputation(
            Reputation::clean().with_standing(Standing::Flagged { stage, decays_at }),
        )
    }

    #[test]
    fn open_kill_of_a_clean_victim_flags_the_killer_one_stage_and_stacks_the_timer() {
        let killer = clean_char();
        let victim = clean_char();
        let s = player_kill_sanction(&victim, PvpContext::Open);
        assert!(matches!(s, KillSanction::Unsanctioned));
        let (killer, ev) = resolve_player_kill(killer, s, Tick(1000), tick());
        assert_eq!(
            killer.reputation().standing(),
            Standing::Flagged {
                stage: PkStage::Warning,
                decays_at: Tick(1000) + hour()
            }
        );
        assert_eq!(killer.reputation().kills(), PlayerKillCount(1));
        assert!(matches!(
            ev,
            PkEvent::Flagged {
                stage: PkStage::Warning,
                ..
            }
        ));
    }

    #[test]
    fn a_second_open_kill_climbs_and_accumulates() {
        let killer = flagged(PkStage::Warning, Tick(5000)); // decays_at ahead of `at`
        let (killer, _) = resolve_player_kill(killer, KillSanction::Unsanctioned, Tick(1000), tick());
        assert_eq!(
            killer.reputation().standing(),
            Standing::Flagged {
                stage: PkStage::FirstStage,
                decays_at: Tick(5000) + hour()
            }
        );
        assert_eq!(killer.reputation().kills(), PlayerKillCount(1)); // fixture started at 0
    }

    #[test]
    fn killing_a_first_stage_victim_is_free_but_a_warning_victim_is_not() {
        assert!(matches!(
            player_kill_sanction(&flagged(PkStage::FirstStage, Tick(1)), PvpContext::Open),
            KillSanction::Sanctioned {
                reason: SanctionReason::VictimWasMurderer
            }
        ));
        assert!(matches!(
            player_kill_sanction(&flagged(PkStage::Warning, Tick(1)), PvpContext::Open),
            KillSanction::Unsanctioned
        ));
    }

    #[test]
    fn host_exemptions_are_free_and_do_not_change_the_killer() {
        for (ctx, reason) in [
            (PvpContext::SelfDefense, SanctionReason::SelfDefense),
            (PvpContext::RivalGuild, SanctionReason::RivalGuild),
            (PvpContext::Duel, SanctionReason::Duel),
            (PvpContext::MiniGamePvp, SanctionReason::MiniGamePvp),
        ] {
            let s = player_kill_sanction(&clean_char(), ctx);
            assert!(matches!(s, KillSanction::Sanctioned { reason: r } if r == reason));
            let before = flagged(PkStage::Warning, Tick(9));
            let (after, ev) = resolve_player_kill(before.clone(), s, Tick(1000), tick());
            assert_eq!(after, before, "a sanctioned kill leaves the killer byte-identical");
            assert!(matches!(ev, PkEvent::Sanctioned { .. }));
        }
    }

    #[test]
    fn cap_stacks_timer_and_count_but_not_stage() {
        let killer = flagged(PkStage::SecondStage, Tick(5000));
        let (killer, _) = resolve_player_kill(killer, KillSanction::Unsanctioned, Tick(1000), tick());
        assert_eq!(
            killer.reputation().standing(),
            Standing::Flagged {
                stage: PkStage::SecondStage,
                decays_at: Tick(5000) + hour()
            }
        );
    }

    #[test]
    fn time_decay_peels_one_stage_per_boundary_accumulate_then_reset() {
        // Three rapid kills from clean stack the deadline to t0 + 3h at SecondStage;
        // the first peel waits out the whole accumulated total, each later rung a
        // flat hour.
        let c = flagged(PkStage::SecondStage, Tick(0) + hours(3));
        let (c, ev) = decay_reputation(c, Tick(0) + hours(3), tick());
        assert_eq!(
            c.reputation().standing(),
            Standing::Flagged {
                stage: PkStage::FirstStage,
                decays_at: Tick(0) + hours(4)
            }
        );
        assert!(matches!(ev, Some(PkEvent::Decayed { .. })));
        let (c, _) = decay_reputation(c, Tick(0) + hours(4), tick());
        assert_eq!(
            c.reputation().standing(),
            Standing::Flagged {
                stage: PkStage::Warning,
                decays_at: Tick(0) + hours(5)
            }
        );
        let (c, _) = decay_reputation(c, Tick(0) + hours(5), tick());
        assert_eq!(c.reputation().standing(), Standing::Clean);
    }

    #[test]
    fn a_large_elapsed_multi_peels_to_clean_in_one_call() {
        let c = flagged(PkStage::SecondStage, Tick(0) + hours(3));
        let (c, ev) = decay_reputation(c, Tick(0) + hours(10), tick());
        assert_eq!(c.reputation().standing(), Standing::Clean);
        assert!(matches!(
            ev,
            Some(PkEvent::Decayed {
                standing: Standing::Clean
            })
        ));
    }

    #[test]
    fn decay_before_the_deadline_or_on_clean_is_a_noop() {
        let c = flagged(PkStage::Warning, Tick(0) + hours(1));
        let rep = c.reputation();
        let (c, ev) = decay_reputation(c, (Tick(0) + hours(1)) - Ticks(1), tick());
        assert_eq!(c.reputation(), rep);
        assert!(ev.is_none());
        let (clean, ev) = decay_reputation(clean_char(), Tick(9_999), tick());
        assert_eq!(clean.reputation(), Reputation::clean());
        assert!(ev.is_none());
    }

    #[test]
    fn decay_never_grows_the_timer_property() {
        // Sweep (start rung, now): before the deadline the standing is untouched;
        // once reached it lands strictly lower on the ladder (or clean), never a
        // higher rung — OpenMU's decay sign bug is structurally unrepresentable.
        fn rank(standing: Standing) -> u8 {
            match standing {
                Standing::Clean => 0,
                Standing::Flagged { stage, .. } => match stage {
                    PkStage::Warning => 1,
                    PkStage::FirstStage => 2,
                    PkStage::SecondStage => 3,
                },
            }
        }
        let deadline = Tick(0) + hours(3);
        for start in [PkStage::Warning, PkStage::FirstStage, PkStage::SecondStage] {
            let before = flagged(start, deadline);
            let before_rank = rank(before.reputation().standing());
            for now in [
                Tick(0),
                deadline - Ticks(1),
                deadline,
                deadline + hours(1),
                deadline + hours(9),
            ] {
                let (after, ev) = decay_reputation(before.clone(), now, tick());
                if deadline.reached(now) {
                    assert!(
                        rank(after.reputation().standing()) < before_rank,
                        "a reached deadline strictly lowers the rung, never raises it"
                    );
                    assert!(ev.is_some());
                } else {
                    assert_eq!(after.reputation(), before.reputation());
                    assert!(ev.is_none());
                }
            }
        }
    }
}
