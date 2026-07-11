//! The wave spawner: fires a wave's authored areas into the session's own
//! instanced live-set at window start, and re-places killed sustained
//! monsters after their own respawn delay while — and only while — their wave
//! window is open. Placement reuses the shared spawn primitive, so an area of
//! only-unwalkable tiles places zero instances, and every landing/facing draw
//! comes from the injected RNG in area order.

use rand_core::RngCore;

use crate::components::units::Tick;
use crate::data::atlas::{MiniGameHandle, ResolvedWave, ResolvedWaveArea};
use crate::data::common::MonsterNumber;
use crate::data::minigame::{SessionMonsterId, WaveNumber};
use crate::data::spawns::SpawnPlacement;
use crate::entities::minigame_session::{
    InstancedMonster, MiniGameSession, SessionMonsters, WaveState,
};
use crate::entities::spawned::Spawned;
use crate::events::minigame::MiniGameEvent;
use crate::services::spawn::{SpawnResult, place_spawn};

/// Fires one wave at its window start: announces the wave, then places every
/// area's quantity over the entrance terrain into the session's live-set —
/// never the map population — reporting each placement.
pub(super) fn spawn_wave(
    monsters: &mut SessionMonsters,
    wave: &ResolvedWave,
    handle: &MiniGameHandle<'_>,
    rng: &mut impl RngCore,
) -> Vec<MiniGameEvent> {
    let mut events = vec![MiniGameEvent::WaveStarted {
        number: wave.number,
    }];
    for area in &wave.areas {
        let placement = SpawnPlacement::Area {
            area: area.area,
            quantity: area.quantity.get(),
        };
        let result = place_spawn(
            &area.monster,
            &placement,
            handle.terrain,
            handle.definition.entrance.map,
            rng,
        );
        admit(monsters, result, wave.number, &mut events);
    }
    events
}

/// Fires every due pending respawn whose wave is still running: one instance
/// re-placed over the wave's first area spawning that monster (deterministic,
/// order-stable). A due respawn whose wave is no longer running — or whose
/// wave or area no longer resolves against the handle — is dropped;
/// not-yet-due respawns are kept.
pub(super) fn fire_respawns(
    session: &mut MiniGameSession,
    handle: &MiniGameHandle<'_>,
    now: Tick,
    rng: &mut impl RngCore,
) -> Vec<MiniGameEvent> {
    let mut events = Vec::new();
    let pending = core::mem::take(&mut session.waves.pending_respawns);
    let mut kept = Vec::with_capacity(pending.len());
    for respawn in pending {
        if !respawn.due.reached(now) {
            kept.push(respawn);
            continue;
        }
        if !wave_running(session, respawn.wave) {
            continue;
        }
        let Some(area) = first_area(handle, respawn.wave, respawn.monster) else {
            continue;
        };
        let placement = SpawnPlacement::Area {
            area: area.area,
            quantity: 1,
        };
        let result = place_spawn(
            &area.monster,
            &placement,
            handle.terrain,
            handle.definition.entrance.map,
            rng,
        );
        admit(&mut session.monsters, result, respawn.wave, &mut events);
    }
    session.waves.pending_respawns = kept;
    events
}

/// Whether `wave`'s track is currently running.
fn wave_running(session: &MiniGameSession, wave: WaveNumber) -> bool {
    session
        .waves
        .waves
        .iter()
        .any(|track| track.number == wave && matches!(track.state, WaveState::Running { .. }))
}

/// The first definition area of `wave` spawning `monster` — the deterministic
/// re-placement source when a sustained monster appears in more than one
/// area.
fn first_area<'a>(
    handle: &'a MiniGameHandle<'_>,
    wave: WaveNumber,
    monster: MonsterNumber,
) -> Option<&'a ResolvedWaveArea> {
    handle
        .waves
        .iter()
        .find(|resolved| resolved.number == wave)?
        .areas
        .iter()
        .find(|area| area.monster.number == monster)
}

/// Folds placed spawns into the live-set: each mob takes the next
/// session-local id (never recycled) and remembers its origin wave, and its
/// placement is reported. The passive arm is enumerated for the totality
/// proof — a wave-area monster is parse-proven to be a fighting kind, so it
/// never places a passive object.
fn admit(
    monsters: &mut SessionMonsters,
    result: SpawnResult,
    origin: WaveNumber,
    events: &mut Vec<MiniGameEvent>,
) {
    for spawned in result.spawned {
        match spawned {
            Spawned::Mob { instance } => {
                events.push(MiniGameEvent::MonsterSpawned {
                    number: instance.number,
                    at: instance.placement.position,
                    facing: instance.placement.facing,
                });
                monsters.live.push(InstancedMonster {
                    id: SessionMonsterId(monsters.next_id),
                    instance,
                    origin,
                });
                monsters.next_id = monsters.next_id.saturating_add(1);
            }
            Spawned::Placed { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lifecycle::advance_mini_game;
    use super::super::scoring::report_session_kill;
    use super::super::support::{TestRng, fixture, monster, resolved_wave, tick100};
    use super::*;
    use crate::components::tile::TerrainGrid;
    use crate::components::tile::TileArea;
    use crate::components::units::TickDuration;
    use crate::data::minigame::{
        PlayerCount, RosterSlot, RosterStatus, Score, WaveNumber, WaveRespawn,
    };
    use crate::entities::minigame_session::{
        MiniGamePhase, MiniGameSession, RosterMember, WaveProgress, WaveTrack,
    };

    /// A Playing session that started at tick 0 (ends at 12,000 = 20 min) with
    /// one alive member, its tracks built from `waves` game-relative windows.
    fn playing(waves: &[ResolvedWave], tick: TickDuration) -> MiniGameSession {
        let tracks = waves
            .iter()
            .map(|wave| WaveTrack {
                number: wave.number,
                state: WaveState::Pending {
                    starts_at: Tick(0) + wave.window.min().in_ticks(tick),
                    ends_at: Tick(0) + wave.window.max().in_ticks(tick),
                },
            })
            .collect();
        super::super::support::open_session()
            .with_member(RosterMember {
                slot: RosterSlot(0),
                status: RosterStatus::Alive,
                score: Score(0),
            })
            .with_phase(MiniGamePhase::Playing {
                ends_at: Tick(12_000),
                snapshot: PlayerCount(1),
            })
            .with_waves(WaveProgress {
                waves: tracks,
                pending_respawns: Vec::new(),
            })
    }

    #[test]
    fn a_wave_start_spawns_its_areas_and_announces_each_placement() {
        let mut holder = fixture();
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            monster(17, 10_000),
            TileArea::new(10, 10, 20, 20).unwrap(),
            35,
        )];
        let handle = holder.handle();
        let session = playing(&holder.waves, tick100());
        let mut rng = TestRng::new(3);
        let (session, events) = advance_mini_game(session, &handle, Tick(0), tick100(), &mut rng);

        assert_eq!(session.monsters.live.len(), 35);
        assert!(
            session
                .monsters
                .live
                .iter()
                .all(|instanced| instanced.origin == WaveNumber(1))
        );
        // Session-local ids are sequential and never reused.
        let ids: Vec<u32> = session.monsters.live.iter().map(|m| m.id.0).collect();
        assert_eq!(ids, (0..35).collect::<Vec<u32>>());
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, MiniGameEvent::WaveStarted { .. }))
                .count(),
            1
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
                .count(),
            35
        );
        assert!(matches!(
            session.waves.waves.first().map(|track| track.state),
            Some(WaveState::Running { .. })
        ));
    }

    #[test]
    fn overlapping_windows_leave_two_waves_running_at_once() {
        let mut holder = fixture();
        holder.waves = vec![
            resolved_wave(
                1,
                0,
                420_000,
                WaveRespawn::RespawningWhileOpen,
                monster(17, 10_000),
                TileArea::new(10, 10, 12, 12).unwrap(),
                2,
            ),
            resolved_wave(
                2,
                300_000,
                840_000,
                WaveRespawn::RespawningWhileOpen,
                monster(18, 10_000),
                TileArea::new(14, 14, 16, 16).unwrap(),
                2,
            ),
        ];
        let handle = holder.handle();
        let session = playing(&holder.waves, tick100());
        let mut rng = TestRng::new(3);
        // 6 minutes in: wave 1 (0..7 min) and wave 2 (5..14 min) both live.
        let (session, _) = advance_mini_game(session, &handle, Tick(3_600), tick100(), &mut rng);
        let states: Vec<(WaveNumber, bool)> = session
            .waves
            .waves
            .iter()
            .map(|track| {
                (
                    track.number,
                    matches!(track.state, WaveState::Running { .. }),
                )
            })
            .collect();
        assert_eq!(states, vec![(WaveNumber(1), true), (WaveNumber(2), true)]);
        assert_eq!(session.monsters.live.len(), 4);
    }

    #[test]
    fn a_sustained_kill_respawns_after_the_monsters_own_delay() {
        let mut holder = fixture();
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            monster(17, 10_000),
            TileArea::new(10, 10, 12, 12).unwrap(),
            2,
        )];
        let handle = holder.handle();
        let session = playing(&holder.waves, tick100());
        let mut rng = TestRng::new(3);
        let (session, _) = advance_mini_game(session, &handle, Tick(0), tick100(), &mut rng);
        assert_eq!(session.monsters.live.len(), 2);

        // Killed at 3 minutes (tick 1800): due at 1800 + 100 (10 s).
        let slain = session.monsters.live[0].id;
        let session = report_session_kill(
            session,
            &handle,
            slain,
            RosterSlot(0),
            Score(3),
            Tick(1_800),
            tick100(),
        );
        assert_eq!(session.monsters.live.len(), 1);
        assert_eq!(session.waves.pending_respawns.len(), 1);

        // One tick before the delay elapses: nothing fires.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(1_899), tick100(), &mut rng);
        assert_eq!(session.monsters.live.len(), 1);
        assert!(events.is_empty());

        // At the delay: the monster returns with a fresh id.
        let (session, events) =
            advance_mini_game(session, &handle, Tick(1_900), tick100(), &mut rng);
        assert_eq!(session.monsters.live.len(), 2);
        assert!(session.waves.pending_respawns.is_empty());
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
        );
        assert!(
            session
                .monsters
                .live
                .iter()
                .all(|instanced| instanced.id.0 != slain.0)
        );
    }

    #[test]
    fn a_respawn_due_after_the_window_closes_is_dropped() {
        let mut holder = fixture();
        // Wave 1 open 0..7 min (ticks 0..4200), respawn delay 10 s.
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            monster(17, 10_000),
            TileArea::new(10, 10, 12, 12).unwrap(),
            2,
        )];
        let handle = holder.handle();
        let session = playing(&holder.waves, tick100());
        let mut rng = TestRng::new(3);
        let (session, _) = advance_mini_game(session, &handle, Tick(0), tick100(), &mut rng);

        // Killed at 6 min 55 s (tick 4150): due at 4250, past the 4200 close.
        let slain = session.monsters.live[0].id;
        let session = report_session_kill(
            session,
            &handle,
            slain,
            RosterSlot(0),
            Score(3),
            Tick(4_150),
            tick100(),
        );
        assert_eq!(session.waves.pending_respawns.len(), 1);

        let (session, _) = advance_mini_game(session, &handle, Tick(4_250), tick100(), &mut rng);
        assert!(matches!(
            session.waves.waves.first().map(|track| track.state),
            Some(WaveState::Closed)
        ));
        assert_eq!(session.monsters.live.len(), 1);
        assert!(session.waves.pending_respawns.is_empty());
    }

    #[test]
    fn a_one_shot_wave_spawns_once_and_never_returns() {
        let mut holder = fixture();
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            WaveRespawn::OnceAtWaveStart,
            monster(17, 10_000),
            TileArea::new(10, 10, 12, 12).unwrap(),
            5,
        )];
        let handle = holder.handle();
        let session = playing(&holder.waves, tick100());
        let mut rng = TestRng::new(3);
        let (session, _) = advance_mini_game(session, &handle, Tick(0), tick100(), &mut rng);
        assert_eq!(session.monsters.live.len(), 5);

        let slain = session.monsters.live[0].id;
        let session = report_session_kill(
            session,
            &handle,
            slain,
            RosterSlot(0),
            Score(3),
            Tick(600),
            tick100(),
        );
        // No respawn is even scheduled for a one-shot wave.
        assert!(session.waves.pending_respawns.is_empty());
        let (session, _) = advance_mini_game(session, &handle, Tick(800), tick100(), &mut rng);
        assert_eq!(session.monsters.live.len(), 4);
    }

    #[test]
    fn an_unwalkable_area_spawns_zero_without_error() {
        let mut holder = fixture();
        holder.terrain = TerrainGrid::from_words([0u64; 1024]);
        holder.waves = vec![resolved_wave(
            1,
            0,
            420_000,
            WaveRespawn::RespawningWhileOpen,
            monster(17, 10_000),
            TileArea::new(10, 10, 12, 12).unwrap(),
            35,
        )];
        let handle = holder.handle();
        let session = playing(&holder.waves, tick100());
        let mut rng = TestRng::new(3);
        let (session, events) = advance_mini_game(session, &handle, Tick(0), tick100(), &mut rng);
        assert!(session.monsters.live.is_empty());
        // The wave still opens and announces itself; no placement is reported.
        assert!(
            events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::WaveStarted { .. }))
        );
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::MonsterSpawned { .. }))
        );
    }
}
