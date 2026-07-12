//! The player-kill reputation lifecycle: the decide step that rules a kill
//! sanctioned or not, and the apply step that flags the killer onto the
//! murderer ladder. The killer-side twin of [`crate::services::death`] —
//! [`player_kill_sanction`] mirrors
//! [`crate::services::death::combat_death_penalty`] (core reads the victim's
//! authoritative reputation, never a client claim) and [`resolve_player_kill`]
//! mirrors [`crate::services::death::resolve_death`] (killer in by value, killer
//! plus one event out). Both draw **no** randomness — the ladder climb and the
//! deadline are pure tick math.

use crate::components::reputation::{PkStage, Standing};
use crate::components::units::{DurationMs, Tick, TickDuration};
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
}
