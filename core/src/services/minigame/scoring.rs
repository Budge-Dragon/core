//! Instanced kill scoring: removes a slain monster from the session's own
//! live-set, credits the server-computed score to the crediting roster seat,
//! and — when the origin wave is still running and sustained — schedules the
//! monster's return after its own respawn delay. `slain`, `credit`, and
//! `score` are server-computed facts the host supplies from combat
//! attribution and the per-game score rule; the framework never invents them.
//! A kill reported outside Playing, or naming an already-removed instance,
//! changes nothing — the credit is tied to the removal, so a double report
//! cannot double-score. Draws no randomness: the respawn placement is drawn
//! later, by the tick machine.

use crate::components::units::{Tick, TickDuration};
use crate::data::atlas::MiniGameHandle;
use crate::data::common::MonsterNumber;
use crate::data::minigame::{RosterSlot, Score, SessionMonsterId, WaveNumber, WaveRespawn};
use crate::entities::minigame_session::{
    MiniGamePhase, MiniGameSession, PendingRespawn, WaveState,
};

/// Records one session kill: the slain instance leaves the live-set, `score`
/// accrues to `credit`'s seat, and a sustained still-running wave schedules
/// the monster's return at `now` plus its own respawn delay. A report outside
/// Playing or against a stale instance reference is a no-op.
#[must_use]
pub fn report_session_kill(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    slain: SessionMonsterId,
    credit: RosterSlot,
    score: Score,
    now: Tick,
    tick: TickDuration,
) -> MiniGameSession {
    let MiniGamePhase::Playing { .. } = session.phase else {
        return session;
    };
    let mut session = session;
    let Some(dead) = session.monsters.take(slain) else {
        return session;
    };
    let session = session.with_score_added(credit, score);
    match respawn_due(
        &session,
        handle,
        dead.origin,
        dead.instance.number,
        now,
        tick,
    ) {
        Some(pending) => session.with_pending_respawn(pending),
        None => session,
    }
}

/// The scheduled return of a slain sustained monster: due `respawn_ms` after
/// the kill, and only when the origin wave's track is still running, the wave
/// is sustained, and the wave still authors an area for the monster. Every
/// other case — a closed or pending window, a one-shot wave, a stale join —
/// schedules nothing.
fn respawn_due(
    session: &MiniGameSession,
    handle: &MiniGameHandle<'_>,
    origin: WaveNumber,
    monster: MonsterNumber,
    now: Tick,
    tick: TickDuration,
) -> Option<PendingRespawn> {
    let track = session
        .waves
        .waves
        .iter()
        .find(|track| track.number == origin)?;
    let WaveState::Running { .. } = track.state else {
        return None;
    };
    let wave = handle.waves.iter().find(|wave| wave.number == origin)?;
    let WaveRespawn::RespawningWhileOpen = wave.respawn else {
        return None;
    };
    let area = wave
        .areas
        .iter()
        .find(|area| area.monster.number == monster)?;
    Some(PendingRespawn {
        monster,
        wave: origin,
        due: now + area.respawn_ms.in_ticks(tick),
    })
}

#[cfg(test)]
mod tests {
    use super::super::support::{fixture, monster, open_session, resolved_wave, tick100};
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::placement::Placement;
    use crate::components::pool::Pool;
    use crate::components::spatial::Facing;
    use crate::components::tile::{TileArea, TileCoord};
    use crate::components::units::MapNumber;
    use crate::data::minigame::{PlayerCount, RosterStatus};
    use crate::entities::minigame_session::{
        InstancedMonster, RosterMember, SessionMonsters, WaveProgress, WaveTrack,
    };
    use crate::entities::monster_instance::MonsterInstance;

    fn instanced(id: u32, origin: u8) -> InstancedMonster {
        InstancedMonster {
            id: SessionMonsterId(id),
            instance: MonsterInstance {
                number: MonsterNumber(17),
                placement: Placement {
                    position: TileCoord::new(11, 11).to_world(),
                    facing: Facing::POS_Y,
                    movement: Movement::Grounded,
                    map: MapNumber(0),
                },
                health: Pool::full(60),
                anchor: TileCoord::new(11, 11).to_world(),
                next_action: Tick(0),
                active_effects: crate::components::active_effect::ActiveEffects::EMPTY,
            },
            origin: WaveNumber(origin),
        }
    }

    /// A Playing session with two live wave-1 monsters and the wave `state`.
    fn playing(state: WaveState) -> MiniGameSession {
        open_session()
            .with_member(RosterMember {
                slot: RosterSlot(1),
                status: RosterStatus::Alive,
                score: Score(0),
            })
            .with_member(RosterMember {
                slot: RosterSlot(2),
                status: RosterStatus::Alive,
                score: Score(0),
            })
            .with_phase(MiniGamePhase::Playing {
                ends_at: Tick(12_000),
                snapshot: PlayerCount(2),
            })
            .with_waves(WaveProgress {
                waves: vec![WaveTrack {
                    number: WaveNumber(1),
                    state,
                }],
                pending_respawns: Vec::new(),
            })
            .with_monsters(SessionMonsters {
                live: vec![instanced(0, 1), instanced(1, 1)],
                next_id: 2,
            })
    }

    fn sustained_fixture() -> super::super::support::HandleFixture {
        let mut holder = fixture();
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            crate::data::minigame::WaveRespawn::RespawningWhileOpen,
            monster(17, 10_000),
            TileArea::new(10, 10, 12, 12).unwrap(),
            2,
        )];
        holder
    }

    #[test]
    fn a_kill_credits_the_score_and_removes_the_instance() {
        let holder = sustained_fixture();
        let handle = holder.handle();
        let session = playing(WaveState::Running {
            ends_at: Tick(4_200),
        });
        let session = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(2),
            Score(3),
            Tick(1_800),
            tick100(),
        );
        assert_eq!(session.member(RosterSlot(2)).unwrap().score, Score(3));
        assert_eq!(session.member(RosterSlot(1)).unwrap().score, Score(0));
        assert_eq!(session.monsters.live.len(), 1);
        // Sustained + running: the return is scheduled at the monster's own
        // delay, with no placement drawn yet.
        assert_eq!(
            session.waves.pending_respawns,
            vec![PendingRespawn {
                monster: MonsterNumber(17),
                wave: WaveNumber(1),
                due: Tick(1_900),
            }]
        );
    }

    #[test]
    fn a_kill_reported_after_the_game_ended_scores_nothing() {
        let holder = sustained_fixture();
        let handle = holder.handle();
        let session = playing(WaveState::Running {
            ends_at: Tick(4_200),
        })
        .with_phase(MiniGamePhase::Ended {
            disposes_at: Tick(14_000),
            snapshot: PlayerCount(2),
            remaining: crate::components::units::Ticks(0),
        });
        let before = session.clone();
        let after = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(2),
            Score(3),
            Tick(12_100),
            tick100(),
        );
        assert_eq!(after, before);
    }

    #[test]
    fn repeated_kills_accumulate_on_the_credited_seat() {
        let holder = sustained_fixture();
        let handle = holder.handle();
        let session = playing(WaveState::Running {
            ends_at: Tick(4_200),
        });
        let session = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(1_000),
            tick100(),
        );
        let session = report_session_kill(
            session,
            &handle,
            SessionMonsterId(1),
            RosterSlot(1),
            Score(5),
            Tick(1_001),
            tick100(),
        );
        assert_eq!(session.member(RosterSlot(1)).unwrap().score, Score(10));
        assert!(session.monsters.live.is_empty());
    }

    #[test]
    fn a_stale_reference_neither_scores_nor_schedules() {
        let holder = sustained_fixture();
        let handle = holder.handle();
        let session = playing(WaveState::Running {
            ends_at: Tick(4_200),
        });
        let session = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(1_000),
            tick100(),
        );
        let before = session.clone();
        // The same id reported again: already removed, nothing changes.
        let after = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(1_001),
            tick100(),
        );
        assert_eq!(after, before);
    }

    #[test]
    fn no_respawn_is_scheduled_off_a_closed_wave_or_a_one_shot_wave() {
        let holder = sustained_fixture();
        let handle = holder.handle();
        // Closed window: removal + credit, no schedule.
        let session = playing(WaveState::Closed);
        let session = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(4_300),
            tick100(),
        );
        assert_eq!(session.member(RosterSlot(1)).unwrap().score, Score(5));
        assert!(session.waves.pending_respawns.is_empty());

        // One-shot wave: running window, still no schedule.
        let mut holder = fixture();
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            crate::data::minigame::WaveRespawn::OnceAtWaveStart,
            monster(17, 10_000),
            TileArea::new(10, 10, 12, 12).unwrap(),
            2,
        )];
        let handle = holder.handle();
        let session = playing(WaveState::Running {
            ends_at: Tick(4_200),
        });
        let session = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(1_000),
            tick100(),
        );
        assert!(session.waves.pending_respawns.is_empty());
    }

    #[test]
    fn reporting_a_kill_is_deterministic_for_identical_inputs() {
        let holder = sustained_fixture();
        let handle = holder.handle();
        let session = playing(WaveState::Running {
            ends_at: Tick(4_200),
        });
        let first = report_session_kill(
            session.clone(),
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(1_000),
            tick100(),
        );
        let second = report_session_kill(
            session,
            &handle,
            SessionMonsterId(0),
            RosterSlot(1),
            Score(5),
            Tick(1_000),
            tick100(),
        );
        assert_eq!(first, second);
    }
}
