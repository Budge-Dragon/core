//! The tick-driven mini-game lifecycle: one advance drives every transition
//! due at `now` — the per-minute closing broadcasts, the entrance close with
//! its min-player gate (fee refund and dispose on failure, the fixed 30 s
//! countdown on success), the game start that freezes the player-count
//! snapshot onto the phase, the wave windows and sustained respawns while
//! Playing, the three game-end conditions (empty roster, a set winner, the
//! timeout), and the exit-window dispose that warps every remaining alive
//! member to town. All deadline arithmetic saturates, and randomness is drawn
//! only where a spawn or a landing is sampled — in roster and track order —
//! so a replay under one seed is byte-identical.

use rand_core::RngCore;

use crate::components::units::{DurationMs, Tick, TickDuration, Ticks};
use crate::data::atlas::MiniGameHandle;
use crate::data::minigame::{PhaseSpan, PlayerCount, RosterSlot, RosterStatus, WinnerStanding};
use crate::entities::minigame_session::{
    MiniGamePhase, MiniGameSession, SessionMonsters, WaveProgress, WaveState, WaveTrack,
};
use crate::events::minigame::MiniGameEvent;
use crate::services::minigame::waves::{fire_respawns, spawn_wave};
use crate::services::movement::resolve_spawn_gate_landing;

/// One whole minute — the closing-broadcast cadence.
const MINUTE: DurationMs = DurationMs(60_000);

/// Advances the session to `now`, firing every due transition in order: the
/// per-minute entrance-closing broadcasts, the entrance close (min-player
/// abort with fee refunds, or the 30 s countdown), the game start freezing
/// the snapshot, the Playing wave step (window opens/closes and sustained
/// respawns), the game end (empty roster, then a set winner, then the
/// timeout), and the dispose that warps the remaining alive members to town.
/// A host that skipped ticks catches up in one call; a `Disposed` session is
/// a no-op.
#[must_use]
pub fn advance_mini_game(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    now: Tick,
    tick: TickDuration,
    rng: &mut impl RngCore,
) -> (MiniGameSession, Vec<MiniGameEvent>) {
    let mut session = session;
    let mut events = Vec::new();
    loop {
        match session.phase {
            MiniGamePhase::Open {
                closes_at,
                next_notice,
            } => {
                if next_notice.reached(now) {
                    let left = Ticks(closes_at.0.saturating_sub(next_notice.0));
                    let minutes = left.whole_minutes(tick);
                    if minutes >= 1 {
                        events.push(MiniGameEvent::EntranceClosing {
                            minutes_left: u8::try_from(minutes).unwrap_or(u8::MAX),
                        });
                        session = session.with_phase(MiniGamePhase::Open {
                            closes_at,
                            next_notice: next_notice + MINUTE.in_ticks(tick),
                        });
                        continue;
                    }
                }
                if closes_at.reached(now) {
                    session = close_entrance(session, handle, closes_at, tick, rng, &mut events);
                    continue;
                }
                break;
            }
            MiniGamePhase::Closing { starts_at } => {
                if starts_at.reached(now) {
                    session = start_game(session, handle, starts_at, tick, &mut events);
                    continue;
                }
                break;
            }
            MiniGamePhase::Playing { ends_at, snapshot } => {
                let winner_set = matches!(session.winner, WinnerStanding::Won { .. });
                if session.roster.is_empty() || winner_set || ends_at.reached(now) {
                    session = end_game(session, handle, ends_at, snapshot, now, tick, &mut events);
                    continue;
                }
                session = step_waves(session, handle, now, rng, &mut events);
                break;
            }
            MiniGamePhase::Ended { disposes_at, .. } => {
                if disposes_at.reached(now) {
                    session = dispose(session, handle, rng, &mut events);
                }
                break;
            }
            MiniGamePhase::Disposed => break,
        }
    }
    (session, events)
}

/// The entrance close: the min-player gate runs here, before the countdown.
/// Enough alive players enter the fixed 30 s countdown; too few abort — the
/// only fee-refund path — with every still-entered member refunded, every
/// alive member warped to town, and the session disposed without a countdown
/// or a game start.
fn close_entrance(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    closes_at: Tick,
    tick: TickDuration,
    rng: &mut impl RngCore,
    events: &mut Vec<MiniGameEvent>,
) -> MiniGameSession {
    let required = handle.definition.players.min();
    let alive = session.alive_count();
    if alive >= usize::from(required.get()) {
        events.push(MiniGameEvent::CountdownStarted {
            seconds: countdown_seconds(),
        });
        return session.with_phase(MiniGamePhase::Closing {
            starts_at: closes_at + PhaseSpan::COUNTDOWN.in_ticks(tick),
        });
    }
    events.push(MiniGameEvent::MinPlayersAbort {
        present: PlayerCount(u16::try_from(alive).unwrap_or(u16::MAX)),
        required: PlayerCount(required.get()),
    });
    for member in &session.roster {
        events.push(MiniGameEvent::FeeRefunded {
            slot: member.slot,
            amount: handle.definition.entrance_fee,
        });
    }
    warp_out_alive(&session, handle, rng, events);
    MiniGameSession {
        roster: Vec::new(),
        phase: MiniGamePhase::Disposed,
        ..session
    }
}

/// The game start: freezes the entered count (dead included) onto the Playing
/// phase and seats one pending track per resolved wave, its window converted
/// to absolute ticks off the start.
fn start_game(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    starts_at: Tick,
    tick: TickDuration,
    events: &mut Vec<MiniGameEvent>,
) -> MiniGameSession {
    let snapshot = PlayerCount(u16::try_from(session.roster.len()).unwrap_or(u16::MAX));
    events.push(MiniGameEvent::GameStarted { players: snapshot });
    let tracks = handle
        .waves
        .iter()
        .map(|wave| WaveTrack {
            number: wave.number,
            state: WaveState::Pending {
                starts_at: starts_at + wave.window.min().in_ticks(tick),
                ends_at: starts_at + wave.window.max().in_ticks(tick),
            },
        })
        .collect();
    session
        .with_phase(MiniGamePhase::Playing {
            ends_at: starts_at + handle.definition.game_duration.get().in_ticks(tick),
            snapshot,
        })
        .with_waves(WaveProgress {
            waves: tracks,
            pending_respawns: Vec::new(),
        })
}

/// The game end: the remaining roster are the finishers, the remaining time
/// is frozen for the per-second reward basis (zero at or past the timeout),
/// and the exit window opens off `now`. An end on an emptied roster also
/// clears the wave bookkeeping and the live-set (the id counter never
/// rewinds).
fn end_game(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    ends_at: Tick,
    snapshot: PlayerCount,
    now: Tick,
    tick: TickDuration,
    events: &mut Vec<MiniGameEvent>,
) -> MiniGameSession {
    let finishers: Vec<RosterSlot> = session.roster.iter().map(|member| member.slot).collect();
    let emptied = finishers.is_empty();
    events.push(MiniGameEvent::GameEnded { finishers });
    let exit = handle.definition.exit_duration.get().in_ticks(tick);
    let session = session.with_phase(MiniGamePhase::Ended {
        disposes_at: now + exit + PhaseSpan::COUNTDOWN.in_ticks(tick),
        snapshot,
        remaining: Ticks(ends_at.0.saturating_sub(now.0)),
    });
    if emptied {
        let next_id = session.monsters.next_id;
        return session
            .with_waves(WaveProgress {
                waves: Vec::new(),
                pending_respawns: Vec::new(),
            })
            .with_monsters(SessionMonsters {
                live: Vec::new(),
                next_id,
            });
    }
    session
}

/// The dispose: every remaining alive member is warped to town (a dead
/// member's relocation is the host-composed respawn's), the roster clears,
/// and the session reports itself disposed — terminal.
fn dispose(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    rng: &mut impl RngCore,
    events: &mut Vec<MiniGameEvent>,
) -> MiniGameSession {
    warp_out_alive(&session, handle, rng, events);
    events.push(MiniGameEvent::Disposed);
    MiniGameSession {
        roster: Vec::new(),
        phase: MiniGamePhase::Disposed,
        ..session
    }
}

/// Emits a town warp-out for every alive roster member, in roster order — one
/// landing draw each through the shared spawn-gate primitive. A dead member
/// is not warped: its relocation is the host-composed respawn's.
fn warp_out_alive(
    session: &MiniGameSession,
    handle: &MiniGameHandle<'_>,
    rng: &mut impl RngCore,
    events: &mut Vec<MiniGameEvent>,
) {
    for member in &session.roster {
        match member.status {
            RosterStatus::Alive => {
                let to = resolve_spawn_gate_landing(handle.town, handle.town_env, rng);
                events.push(MiniGameEvent::WarpedOut {
                    slot: member.slot,
                    to,
                });
            }
            RosterStatus::Dead => {}
        }
    }
}

/// The Playing wave step: opens every due window (spawning its areas), closes
/// every elapsed one, then fires the due sustained respawns — window
/// transitions first, so a respawn due at a window's end tick finds the wave
/// closed and is dropped.
fn step_waves(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    now: Tick,
    rng: &mut impl RngCore,
    events: &mut Vec<MiniGameEvent>,
) -> MiniGameSession {
    let mut session = session;
    let mut tracks = core::mem::take(&mut session.waves.waves);
    for track in &mut tracks {
        track.state = match track.state {
            WaveState::Pending { starts_at, ends_at } if starts_at.reached(now) => {
                // The tracks were seated from the handle's waves at the game
                // start, so the join is present for any well-formed session;
                // a stale number simply has no areas to fire.
                if let Some(wave) = handle.waves.iter().find(|wave| wave.number == track.number) {
                    events.extend(spawn_wave(&mut session.monsters, wave, handle, rng));
                }
                if ends_at.reached(now) {
                    WaveState::Closed
                } else {
                    WaveState::Running { ends_at }
                }
            }
            WaveState::Pending { starts_at, ends_at } => WaveState::Pending { starts_at, ends_at },
            WaveState::Running { ends_at } if ends_at.reached(now) => WaveState::Closed,
            WaveState::Running { ends_at } => WaveState::Running { ends_at },
            WaveState::Closed => WaveState::Closed,
        };
    }
    session.waves.waves = tracks;
    events.extend(fire_respawns(&mut session, handle, now, rng));
    session
}

/// The fixed pre-start countdown in whole seconds, derived from the shared
/// 30 s floor constant.
fn countdown_seconds() -> u16 {
    u16::try_from(PhaseSpan::COUNTDOWN.0 / 1000).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::super::death::report_leave;
    use super::super::support::{TestRng, fixture, open_session, tick100};
    use super::*;
    use crate::data::minigame::Score;

    fn seated(statuses: &[RosterStatus]) -> MiniGameSession {
        let mut session = open_session();
        for (seat, status) in statuses.iter().enumerate() {
            session = session.with_member(crate::entities::minigame_session::RosterMember {
                slot: RosterSlot(u8::try_from(seat).unwrap()),
                status: *status,
                score: Score(0),
            });
        }
        session
    }

    #[test]
    fn the_enter_window_broadcasts_each_remaining_minute_then_closes() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        // Opened at 100, closes at 3100 (5 minutes at 100 ms/tick).
        let mut session = seated(&[RosterStatus::Alive, RosterStatus::Alive]);
        let mut notices = Vec::new();
        for step in 0..=30 {
            let now = Tick(100 + step * 100);
            let (next, events) = advance_mini_game(session, &handle, now, tick100(), &mut rng);
            session = next;
            for event in events {
                if let MiniGameEvent::EntranceClosing { minutes_left } = event {
                    notices.push(minutes_left);
                }
            }
        }
        assert_eq!(notices, vec![5, 4, 3, 2, 1]);
        assert!(matches!(session.phase, MiniGamePhase::Closing { .. }));
    }

    #[test]
    fn a_late_advance_catches_up_notices_close_and_start_in_one_call() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session = seated(&[RosterStatus::Alive, RosterStatus::Alive]);
        // Far past the close AND the countdown: one call drives it all.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(3_400), tick100(), &mut rng);
        let kinds: Vec<&MiniGameEvent> = events.iter().collect();
        assert_eq!(kinds.len(), 7);
        assert!(matches!(
            events.first(),
            Some(MiniGameEvent::EntranceClosing { minutes_left: 5 })
        ));
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::CountdownStarted { seconds: 30 }))
        );
        assert!(events.iter().any(|event| matches!(
            event,
            MiniGameEvent::GameStarted {
                players: PlayerCount(2)
            }
        )));
        assert!(matches!(session.phase, MiniGamePhase::Playing { .. }));
    }

    #[test]
    fn the_countdown_is_exactly_thirty_seconds_and_freezes_the_snapshot_dead_included() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session = seated(&[RosterStatus::Alive, RosterStatus::Alive, RosterStatus::Dead]);
        // Close the entrance at 3100: two alive >= min 2 -> Closing.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(3_100), tick100(), &mut rng);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::CountdownStarted { seconds: 30 }))
        );
        assert_eq!(
            session.phase,
            MiniGamePhase::Closing {
                starts_at: Tick(3_400)
            }
        );
        // One tick before the 30 s elapse: still Closing.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(3_399), tick100(), &mut rng);
        assert!(events.is_empty());
        assert!(matches!(session.phase, MiniGamePhase::Closing { .. }));
        // At the deadline: Playing, snapshot 3 (the dead entrant counts).
        let (session, events) =
            advance_mini_game(session, &handle, Tick(3_400), tick100(), &mut rng);
        assert!(events.iter().any(|event| matches!(
            event,
            MiniGameEvent::GameStarted {
                players: PlayerCount(3)
            }
        )));
        assert_eq!(session.start_snapshot(), Some(PlayerCount(3)));
        // 20-minute game: ends at 3400 + 12000.
        assert_eq!(
            session.phase,
            MiniGamePhase::Playing {
                ends_at: Tick(15_400),
                snapshot: PlayerCount(3)
            }
        );
    }

    #[test]
    fn too_few_alive_at_entrance_close_aborts_with_refunds_before_any_countdown() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        // Two entered, only one alive; the minimum is two ALIVE players.
        let session = seated(&[RosterStatus::Alive, RosterStatus::Dead]);
        let (session, events) =
            advance_mini_game(session, &handle, Tick(3_100), tick100(), &mut rng);
        assert!(events.iter().any(|event| matches!(
            event,
            MiniGameEvent::MinPlayersAbort {
                present: PlayerCount(1),
                required: PlayerCount(2)
            }
        )));
        // EVERY still-entered member is refunded, dead included.
        let refunds: Vec<RosterSlot> = events
            .iter()
            .filter_map(|event| match event {
                MiniGameEvent::FeeRefunded { slot, amount } => {
                    assert_eq!(amount.0, 25_000);
                    Some(*slot)
                }
                MiniGameEvent::EntranceClosing { .. }
                | MiniGameEvent::CountdownStarted { .. }
                | MiniGameEvent::GameStarted { .. }
                | MiniGameEvent::MinPlayersAbort { .. }
                | MiniGameEvent::WaveStarted { .. }
                | MiniGameEvent::MonsterSpawned { .. }
                | MiniGameEvent::GameEnded { .. }
                | MiniGameEvent::ScoreTable { .. }
                | MiniGameEvent::RewardGranted { .. }
                | MiniGameEvent::WarpedOut { .. }
                | MiniGameEvent::Disposed => None,
            })
            .collect();
        assert_eq!(refunds, vec![RosterSlot(0), RosterSlot(1)]);
        // Only the alive member is warped out; the session never counts down.
        let warped: Vec<RosterSlot> = events
            .iter()
            .filter_map(|event| match event {
                MiniGameEvent::WarpedOut { slot, .. } => Some(*slot),
                MiniGameEvent::EntranceClosing { .. }
                | MiniGameEvent::CountdownStarted { .. }
                | MiniGameEvent::GameStarted { .. }
                | MiniGameEvent::MinPlayersAbort { .. }
                | MiniGameEvent::FeeRefunded { .. }
                | MiniGameEvent::WaveStarted { .. }
                | MiniGameEvent::MonsterSpawned { .. }
                | MiniGameEvent::GameEnded { .. }
                | MiniGameEvent::ScoreTable { .. }
                | MiniGameEvent::RewardGranted { .. }
                | MiniGameEvent::Disposed => None,
            })
            .collect();
        assert_eq!(warped, vec![RosterSlot(0)]);
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::CountdownStarted { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::GameStarted { .. }))
        );
        assert_eq!(session.phase, MiniGamePhase::Disposed);
        assert!(session.roster.is_empty());
    }

    #[test]
    fn the_game_runs_to_its_duration_and_the_remaining_roster_finish() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session = seated(&[
            RosterStatus::Alive,
            RosterStatus::Alive,
            RosterStatus::Alive,
        ])
        .with_phase(MiniGamePhase::Playing {
            ends_at: Tick(15_400),
            snapshot: PlayerCount(3),
        });
        // One tick early: still Playing.
        let (session, _) = advance_mini_game(session, &handle, Tick(15_399), tick100(), &mut rng);
        assert!(matches!(session.phase, MiniGamePhase::Playing { .. }));
        // At the timeout: Ended with zero remaining and every seat a finisher.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(15_400), tick100(), &mut rng);
        assert!(events.iter().any(|event| matches!(
            event,
            MiniGameEvent::GameEnded { finishers } if *finishers == vec![RosterSlot(0), RosterSlot(1), RosterSlot(2)]
        )));
        // Exit 2 min 30 s (1500) + 30 s (300) off the end tick.
        assert_eq!(
            session.phase,
            MiniGamePhase::Ended {
                disposes_at: Tick(15_400 + 1_500 + 300),
                snapshot: PlayerCount(3),
                remaining: Ticks(0),
            }
        );
    }

    #[test]
    fn an_emptied_roster_ends_the_game_immediately_and_clears_the_battlefield() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session = seated(&[RosterStatus::Alive]).with_phase(MiniGamePhase::Playing {
            ends_at: Tick(15_400),
            snapshot: PlayerCount(1),
        });
        let session = report_leave(session, RosterSlot(0));
        let (session, events) =
            advance_mini_game(session, &handle, Tick(5_000), tick100(), &mut rng);
        assert!(events.iter().any(|event| matches!(
            event,
            MiniGameEvent::GameEnded { finishers } if finishers.is_empty()
        )));
        let MiniGamePhase::Ended { remaining, .. } = session.phase else {
            panic!("expected Ended, got {:?}", session.phase);
        };
        assert_eq!(remaining, Ticks(15_400 - 5_000));
        assert!(session.waves.waves.is_empty());
        assert!(session.monsters.live.is_empty());
    }

    #[test]
    fn a_set_winner_ends_the_game_early() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session = seated(&[RosterStatus::Alive, RosterStatus::Alive])
            .with_phase(MiniGamePhase::Playing {
                ends_at: Tick(15_400),
                snapshot: PlayerCount(2),
            })
            .with_winner(WinnerStanding::Won { by: RosterSlot(1) });
        let (session, events) =
            advance_mini_game(session, &handle, Tick(6_400), tick100(), &mut rng);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::GameEnded { .. }))
        );
        let MiniGamePhase::Ended { remaining, .. } = session.phase else {
            panic!("expected Ended, got {:?}", session.phase);
        };
        assert_eq!(remaining, Ticks(9_000));
        assert_eq!(session.winner, WinnerStanding::Won { by: RosterSlot(1) });
    }

    #[test]
    fn dispose_warps_only_the_alive_members_and_clears_the_roster() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session =
            seated(&[RosterStatus::Alive, RosterStatus::Dead]).with_phase(MiniGamePhase::Ended {
                disposes_at: Tick(17_200),
                snapshot: PlayerCount(2),
                remaining: Ticks(0),
            });
        // One tick early: nothing happens.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(17_199), tick100(), &mut rng);
        assert!(events.is_empty());
        // At the deadline: the alive member warps to the town landing, the
        // dead one is left to its host-composed respawn, the roster clears.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(17_200), tick100(), &mut rng);
        let warped: Vec<RosterSlot> = events
            .iter()
            .filter_map(|event| match event {
                MiniGameEvent::WarpedOut { slot, to } => {
                    assert_eq!(to.map, crate::components::units::MapNumber(0));
                    Some(*slot)
                }
                MiniGameEvent::EntranceClosing { .. }
                | MiniGameEvent::CountdownStarted { .. }
                | MiniGameEvent::GameStarted { .. }
                | MiniGameEvent::MinPlayersAbort { .. }
                | MiniGameEvent::FeeRefunded { .. }
                | MiniGameEvent::WaveStarted { .. }
                | MiniGameEvent::MonsterSpawned { .. }
                | MiniGameEvent::GameEnded { .. }
                | MiniGameEvent::ScoreTable { .. }
                | MiniGameEvent::RewardGranted { .. }
                | MiniGameEvent::Disposed => None,
            })
            .collect();
        assert_eq!(warped, vec![RosterSlot(0)]);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::Disposed))
        );
        assert_eq!(session.phase, MiniGamePhase::Disposed);
        assert!(session.roster.is_empty());
        // A disposed session is inert.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(20_000), tick100(), &mut rng);
        assert!(events.is_empty());
        assert_eq!(session.phase, MiniGamePhase::Disposed);
    }

    #[test]
    fn deadline_arithmetic_saturates_at_the_timeline_ceiling() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        let session = seated(&[RosterStatus::Alive, RosterStatus::Alive]).with_phase(
            MiniGamePhase::Playing {
                ends_at: Tick(u64::MAX - 5),
                snapshot: PlayerCount(2),
            },
        );
        // The timeout at the ceiling: now + exit + countdown saturates
        // instead of wrapping — a wrapped deadline would land in the past and
        // dispose immediately.
        let (session, _) =
            advance_mini_game(session, &handle, Tick(u64::MAX - 5), tick100(), &mut rng);
        assert_eq!(
            session.phase,
            MiniGamePhase::Ended {
                disposes_at: Tick(u64::MAX),
                snapshot: PlayerCount(2),
                remaining: Ticks(0),
            }
        );
        // The saturated deadline still fires, in order, at the ceiling.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(u64::MAX), tick100(), &mut rng);
        assert_eq!(session.phase, MiniGamePhase::Disposed);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::Disposed))
        );
    }

    #[test]
    fn the_min_player_abort_is_the_only_refund_path() {
        let holder = fixture();
        let handle = holder.handle();
        let mut rng = TestRng::new(1);
        // Three enter; one leaves during Open (forfeits); the rest play to the
        // end and dispose. No FeeRefunded fires anywhere.
        let session = seated(&[
            RosterStatus::Alive,
            RosterStatus::Alive,
            RosterStatus::Alive,
        ]);
        let session = report_leave(session, RosterSlot(2));
        let mut all_events = Vec::new();
        let mut session = session;
        for now in [3_100u64, 3_400, 9_000, 15_400, 17_200] {
            let (next, events) =
                advance_mini_game(session, &handle, Tick(now), tick100(), &mut rng);
            session = next;
            all_events.extend(events);
        }
        assert_eq!(session.phase, MiniGamePhase::Disposed);
        assert!(
            !all_events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::FeeRefunded { .. }))
        );
    }

    #[test]
    fn advancing_is_deterministic_for_one_seed() {
        let holder = fixture();
        let handle = holder.handle();
        let session = seated(&[RosterStatus::Alive, RosterStatus::Alive]);
        let mut first_rng = TestRng::new(11);
        let mut second_rng = TestRng::new(11);
        let first = advance_mini_game(
            session.clone(),
            &handle,
            Tick(3_400),
            tick100(),
            &mut first_rng,
        );
        let second = advance_mini_game(session, &handle, Tick(3_400), tick100(), &mut second_rng);
        assert_eq!(first, second);
    }
}
