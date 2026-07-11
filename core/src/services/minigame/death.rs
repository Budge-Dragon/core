//! In-event death and leave roster bookkeeping. A death flips the member's
//! seat to the bare `Dead` status — the eject clock lives on the character's
//! [`crate::components::life::LifeState::Dead`], set by the death service
//! under the waived penalty, never duplicated as a roster deadline — and a
//! leave (voluntary, or the host-reported exit of an ejected dead member)
//! removes the seat outright: the fee is forfeit and no reward follows. An
//! emptied roster ends the game on the next advance. Both transitions are
//! value-in/value-out, draw no randomness, and touch no experience or zen.

use crate::data::minigame::{RosterSlot, RosterStatus};
use crate::entities::minigame_session::MiniGameSession;

/// Flips `victim`'s seat to the bare `Dead` status. The dead member keeps its
/// seat (it counts toward capacity and — still present at game end — finishes
/// under the `Dead` flag); a slot naming no member changes nothing.
#[must_use]
pub fn report_death(session: MiniGameSession, victim: RosterSlot) -> MiniGameSession {
    session.with_status(victim, RosterStatus::Dead)
}

/// Removes `who`'s seat from the roster — a voluntary leave or an ejected
/// dead member's host-reported exit. The fee is forfeit (the only refund path
/// is the min-player abort) and a removed member is not a finisher; a slot
/// naming no member changes nothing. An emptied roster ends the game on the
/// next advance.
#[must_use]
pub fn report_leave(session: MiniGameSession, who: RosterSlot) -> MiniGameSession {
    session.without_slot(who)
}

#[cfg(test)]
mod tests {
    use super::super::support::open_session;
    use super::*;
    use crate::components::units::Tick;
    use crate::data::minigame::{PlayerCount, Score};
    use crate::entities::minigame_session::{MiniGamePhase, RosterMember};

    fn seated() -> MiniGameSession {
        open_session()
            .with_member(RosterMember {
                slot: RosterSlot(0),
                status: RosterStatus::Alive,
                score: Score(7),
            })
            .with_member(RosterMember {
                slot: RosterSlot(1),
                status: RosterStatus::Alive,
                score: Score(3),
            })
    }

    #[test]
    fn a_death_flips_only_the_victims_status_and_keeps_its_seat_and_score() {
        let session = report_death(seated(), RosterSlot(0));
        let victim = session.member(RosterSlot(0)).unwrap();
        assert_eq!(victim.status, RosterStatus::Dead);
        assert_eq!(victim.score, Score(7));
        assert_eq!(
            session.member(RosterSlot(1)).unwrap().status,
            RosterStatus::Alive
        );
        // The seat still counts toward capacity; only the alive count drops.
        assert_eq!(session.roster.len(), 2);
        assert_eq!(session.alive_count(), 1);
    }

    #[test]
    fn a_death_report_carries_no_roster_deadline() {
        // The Dead status is a bare discriminator: the wire form carries no
        // clock field — the respawn deadline is the character's LifeState's.
        let session = report_death(seated(), RosterSlot(0));
        let json = serde_json::to_string(&session.member(RosterSlot(0)).unwrap()).unwrap();
        assert_eq!(json, r#"{"slot":0,"status":{"kind":"dead"},"score":7}"#);
    }

    #[test]
    fn a_leave_removes_the_seat_with_no_refund_channel() {
        let session = report_leave(seated(), RosterSlot(1));
        assert!(session.member(RosterSlot(1)).is_none());
        assert_eq!(session.roster.len(), 1);
        // State-only: the session is the entire observable result — there is
        // no event stream a refund could ride on.
    }

    #[test]
    fn reports_naming_no_member_change_nothing() {
        let session = seated();
        assert_eq!(report_death(session.clone(), RosterSlot(9)), session);
        assert_eq!(report_leave(session.clone(), RosterSlot(9)), session);
    }

    #[test]
    fn a_leave_that_empties_the_roster_leaves_the_end_to_the_next_advance() {
        let playing = seated().with_phase(MiniGamePhase::Playing {
            ends_at: Tick(15_400),
            snapshot: PlayerCount(2),
        });
        let session = report_leave(report_leave(playing, RosterSlot(0)), RosterSlot(1));
        // The roster is empty but the phase is untouched: the empty-roster
        // end is the tick machine's transition, not this report's.
        assert!(session.roster.is_empty());
        assert!(matches!(session.phase, MiniGamePhase::Playing { .. }));
    }

    #[test]
    fn reports_are_deterministic_value_transitions() {
        let session = seated();
        assert_eq!(
            report_death(session.clone(), RosterSlot(0)),
            report_death(session.clone(), RosterSlot(0))
        );
        assert_eq!(
            report_leave(session.clone(), RosterSlot(0)),
            report_leave(session, RosterSlot(0))
        );
    }
}
