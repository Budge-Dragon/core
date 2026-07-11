//! A running mini-game — the whole event state as one caller-owned serde
//! value, host-persisted between calls (the
//! [`crate::entities::party_session::PartySession`] treatment). All behavior
//! lives in the mini-game services; this module exposes total-structure
//! accessors and value-in/value-out data operations only. The frozen start
//! snapshot and every deadline ride the phase variant that owns them — never
//! a standalone `Option` field.

use core::num::NonZeroU16;

use serde::{Deserialize, Serialize};

use crate::components::units::{Tick, Ticks};
use crate::data::common::MonsterNumber;
use crate::data::minigame::{
    MiniGameKey, PlayerCount, RosterSlot, RosterStatus, Score, SessionMonsterId, WaveNumber,
    WinnerStanding,
};
use crate::entities::monster_instance::MonsterInstance;

/// A running mini-game — the whole event state as one caller-owned serde
/// value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MiniGameSession {
    /// The definition it runs.
    pub key: MiniGameKey,
    /// The timeline phase (carries its deadline and the frozen snapshot on
    /// the variants that have them).
    pub phase: MiniGamePhase,
    /// The entered members: slot → status + score, ascending by slot.
    pub roster: Vec<RosterMember>,
    /// The winner marker.
    pub winner: WinnerStanding,
    /// Active waves plus pending respawns (empty until Playing).
    pub waves: WaveProgress,
    /// The instanced live-set (the session's own monsters, not the map
    /// population).
    pub monsters: SessionMonsters,
}

/// The timeline state. Each deadline and the snapshot live only on the
/// variant that owns them — a live session can never hold a stray countdown,
/// and Open/Closing carry no snapshot. The min-player check runs at the
/// Open→Closing boundary, before the countdown; a failing check goes straight
/// to `Disposed` (never Closing/Playing).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MiniGamePhase {
    /// The entry window. `next_notice` is the next whole-minute
    /// closing-broadcast tick (the edge cursor, so each closing broadcast
    /// fires exactly once).
    Open {
        /// The tick entry closes.
        closes_at: Tick,
        /// The next per-minute broadcast tick.
        next_notice: Tick,
    },
    /// The fixed 30 s pre-start countdown; no re-entry. Entered only when the
    /// min-player check passed at entrance close.
    Closing {
        /// The tick the game starts (Playing begins).
        starts_at: Tick,
    },
    /// The game is running. Carries the frozen snapshot.
    Playing {
        /// The scheduled game-end tick.
        ends_at: Tick,
        /// The frozen entered count at start (dead included).
        snapshot: PlayerCount,
    },
    /// The exit/score window. Carries the snapshot and the remaining time
    /// frozen at end (`ends_at - actual_end`; 0 on timeout) — the
    /// exp-per-remaining-second basis.
    Ended {
        /// The tick the session disposes.
        disposes_at: Tick,
        /// The frozen snapshot.
        snapshot: PlayerCount,
        /// Whole-tick time remaining at end.
        remaining: Ticks,
    },
    /// Warp-out complete; terminal.
    Disposed,
}

/// One roster seat: its slot, its bare liveness, and its accumulated score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterMember {
    /// The seat.
    pub slot: RosterSlot,
    /// Alive or Dead (the death clock is `LifeState`'s, not here).
    pub status: RosterStatus,
    /// The accumulated event score.
    pub score: Score,
}

/// The wave bookkeeping: each definition wave's lifecycle track plus the
/// pending respawns scheduled by kills.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveProgress {
    /// One track per definition wave (ordered by number).
    pub waves: Vec<WaveTrack>,
    /// Scheduled respawns, each gated on its wave still `Running`.
    pub pending_respawns: Vec<PendingRespawn>,
}

/// One wave's lifecycle track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaveTrack {
    /// The wave number (joins the definition's areas/policy).
    pub number: WaveNumber,
    /// Its lifecycle state.
    pub state: WaveState,
}

/// A wave's lifecycle — a proper sum. Absolute ticks are computed at the
/// Playing transition; `Running` no longer carries `starts_at` (it has
/// fired), so "has this wave spawned" is a variant, never a bool flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WaveState {
    /// Not yet spawned; spawns at `starts_at`, closes at `ends_at`.
    Pending {
        /// Absolute wave-start tick.
        starts_at: Tick,
        /// Absolute wave-end tick.
        ends_at: Tick,
    },
    /// Spawned and live; deregisters at `ends_at`.
    Running {
        /// Absolute wave-end tick.
        ends_at: Tick,
    },
    /// Window elapsed; no more respawns.
    Closed,
}

/// A scheduled respawn: which monster, from which wave, due when. Fires only
/// if its wave is still `Running` at `due`. The placement is drawn from the
/// wave's definition areas at fire time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingRespawn {
    /// The monster to re-place.
    pub monster: MonsterNumber,
    /// The wave it belongs to (gates on still-`Running`).
    pub wave: WaveNumber,
    /// The tick it becomes due (`kill_tick + respawn_ms`).
    pub due: Tick,
}

/// The session's instanced live-set — its own monsters, never the map
/// population. The id counter never reuses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionMonsters {
    /// The live instances.
    pub live: Vec<InstancedMonster>,
    /// The next id to assign.
    pub next_id: u32,
}

impl SessionMonsters {
    /// Removes and returns the instance carrying `id`; `None` when no live
    /// instance does (a stale reference). The id counter is untouched — ids
    /// never recycle.
    pub fn take(&mut self, id: SessionMonsterId) -> Option<InstancedMonster> {
        let index = self.live.iter().position(|monster| monster.id == id)?;
        Some(self.live.remove(index))
    }
}

/// One instanced monster: a session-local id, the live instance, and its
/// origin wave (so a kill knows which wave's respawn policy applies).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstancedMonster {
    /// The session-local id.
    pub id: SessionMonsterId,
    /// The live monster.
    pub instance: MonsterInstance,
    /// The wave that spawned it.
    pub origin: WaveNumber,
}

impl MiniGameSession {
    /// Opens a fresh session in `Open`, entry closing at `closes_at`, the
    /// first per-minute broadcast due immediately at `opened_at`. A real
    /// domain value, not a fabricated default.
    #[must_use]
    pub fn open(key: MiniGameKey, opened_at: Tick, closes_at: Tick) -> Self {
        Self {
            key,
            phase: MiniGamePhase::Open {
                closes_at,
                next_notice: opened_at,
            },
            roster: Vec::new(),
            winner: WinnerStanding::None,
            waves: WaveProgress {
                waves: Vec::new(),
                pending_respawns: Vec::new(),
            },
            monsters: SessionMonsters {
                live: Vec::new(),
                next_id: 0,
            },
        }
    }

    /// The frozen start snapshot — `Some(n)` once Playing has started (dead
    /// included, leavers already removed not counted), `None` in
    /// Open/Closing/Disposed. An accessor over the phase variant, not a
    /// stored `Option` field: `None` is a variant answering, never recomputed
    /// after the Playing freeze.
    #[must_use]
    pub fn start_snapshot(&self) -> Option<PlayerCount> {
        match self.phase {
            MiniGamePhase::Playing { snapshot, .. } | MiniGamePhase::Ended { snapshot, .. } => {
                Some(snapshot)
            }
            MiniGamePhase::Open { .. }
            | MiniGamePhase::Closing { .. }
            | MiniGamePhase::Disposed => None,
        }
    }

    /// The count of alive roster members — the min-player start gate's basis
    /// (asymmetric with capacity, which counts all entered).
    #[must_use]
    pub fn alive_count(&self) -> usize {
        self.roster
            .iter()
            .filter(|member| matches!(member.status, RosterStatus::Alive))
            .count()
    }

    /// The member at `slot`, or `None` when no seat holds it.
    #[must_use]
    pub fn member(&self, slot: RosterSlot) -> Option<&RosterMember> {
        self.roster.iter().find(|member| member.slot == slot)
    }

    /// The lowest `0..max` slot no member occupies, or `None` when the roster
    /// is at capacity — the seat an admitted entrant takes.
    #[must_use]
    pub fn lowest_free_slot(&self, max: NonZeroU16) -> Option<RosterSlot> {
        (0..max.get())
            .filter_map(|seat| u8::try_from(seat).ok())
            .map(RosterSlot)
            .find(|slot| self.member(*slot).is_none())
    }

    /// This session with `member` inserted in ascending slot order — the
    /// caller proves the slot free (via [`Self::lowest_free_slot`]).
    #[must_use]
    pub fn with_member(mut self, member: RosterMember) -> Self {
        let index = self
            .roster
            .partition_point(|seated| seated.slot < member.slot);
        self.roster.insert(index, member);
        self
    }

    /// This session with `slot` removed from the roster. A no-op when `slot`
    /// names no member.
    #[must_use]
    pub fn without_slot(mut self, slot: RosterSlot) -> Self {
        self.roster.retain(|member| member.slot != slot);
        self
    }

    /// This session with `slot`'s status replaced. A no-op when `slot` names
    /// no member.
    #[must_use]
    pub fn with_status(mut self, slot: RosterSlot, status: RosterStatus) -> Self {
        for member in &mut self.roster {
            if member.slot == slot {
                member.status = status;
            }
        }
        self
    }

    /// This session with `added` credited to `slot`'s accumulated score,
    /// saturating. A no-op when `slot` names no member.
    #[must_use]
    pub fn with_score_added(mut self, slot: RosterSlot, added: Score) -> Self {
        for member in &mut self.roster {
            if member.slot == slot {
                member.score = Score(member.score.0.saturating_add(added.0));
            }
        }
        self
    }

    /// This session in another phase.
    #[must_use]
    pub fn with_phase(self, phase: MiniGamePhase) -> Self {
        Self { phase, ..self }
    }

    /// This session with its winner marker replaced.
    #[must_use]
    pub fn with_winner(self, winner: WinnerStanding) -> Self {
        Self { winner, ..self }
    }

    /// This session with its wave bookkeeping replaced.
    #[must_use]
    pub fn with_waves(self, waves: WaveProgress) -> Self {
        Self { waves, ..self }
    }

    /// This session with its instanced live-set replaced.
    #[must_use]
    pub fn with_monsters(self, monsters: SessionMonsters) -> Self {
        Self { monsters, ..self }
    }

    /// This session with a respawn scheduled.
    #[must_use]
    pub fn with_pending_respawn(mut self, pending: PendingRespawn) -> Self {
        self.waves.pending_respawns.push(pending);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::placement::Placement;
    use crate::components::pool::Pool;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;
    use crate::data::minigame::{EventLevel, MiniGameKind};

    fn key() -> MiniGameKey {
        MiniGameKey {
            kind: MiniGameKind::DevilSquare,
            level: EventLevel::new(3).unwrap(),
        }
    }

    fn member(slot: u8, status: RosterStatus, score: u32) -> RosterMember {
        RosterMember {
            slot: RosterSlot(slot),
            status,
            score: Score(score),
        }
    }

    fn instanced(id: u32, origin: u8) -> InstancedMonster {
        InstancedMonster {
            id: SessionMonsterId(id),
            instance: MonsterInstance {
                number: MonsterNumber(17),
                placement: Placement {
                    position: TileCoord::new(2, 3).to_world(),
                    facing: Facing::POS_Y,
                    movement: Movement::Grounded,
                    map: MapNumber(0),
                },
                health: Pool::full(60),
                anchor: TileCoord::new(2, 3).to_world(),
                next_action: Tick(0),
                active_effects: crate::components::active_effect::ActiveEffects::EMPTY,
            },
            origin: WaveNumber(origin),
        }
    }

    #[test]
    fn open_seats_no_one_with_the_first_notice_due_immediately() {
        let session = MiniGameSession::open(key(), Tick(100), Tick(3100));
        assert_eq!(
            session.phase,
            MiniGamePhase::Open {
                closes_at: Tick(3100),
                next_notice: Tick(100),
            }
        );
        assert!(session.roster.is_empty());
        assert_eq!(session.winner, WinnerStanding::None);
        assert!(session.waves.waves.is_empty());
        assert!(session.waves.pending_respawns.is_empty());
        assert!(session.monsters.live.is_empty());
        assert_eq!(session.monsters.next_id, 0);
    }

    #[test]
    fn start_snapshot_rides_only_the_playing_and_ended_variants() {
        let session = MiniGameSession::open(key(), Tick(0), Tick(100));
        assert_eq!(session.start_snapshot(), None);
        let closing = session.clone().with_phase(MiniGamePhase::Closing {
            starts_at: Tick(400),
        });
        assert_eq!(closing.start_snapshot(), None);
        let playing = session.clone().with_phase(MiniGamePhase::Playing {
            ends_at: Tick(12_400),
            snapshot: PlayerCount(4),
        });
        assert_eq!(playing.start_snapshot(), Some(PlayerCount(4)));
        let ended = session.clone().with_phase(MiniGamePhase::Ended {
            disposes_at: Tick(14_200),
            snapshot: PlayerCount(4),
            remaining: Ticks(900),
        });
        assert_eq!(ended.start_snapshot(), Some(PlayerCount(4)));
        let disposed = session.with_phase(MiniGamePhase::Disposed);
        assert_eq!(disposed.start_snapshot(), None);
    }

    #[test]
    fn alive_count_excludes_dead_members_while_the_roster_keeps_them() {
        let session = MiniGameSession::open(key(), Tick(0), Tick(100))
            .with_member(member(0, RosterStatus::Alive, 0))
            .with_member(member(1, RosterStatus::Dead, 5))
            .with_member(member(2, RosterStatus::Alive, 0));
        assert_eq!(session.alive_count(), 2);
        assert_eq!(session.roster.len(), 3);
    }

    #[test]
    fn lowest_free_slot_fills_gaps_and_is_none_at_capacity() {
        let max = NonZeroU16::new(3).unwrap();
        let session = MiniGameSession::open(key(), Tick(0), Tick(100));
        assert_eq!(session.lowest_free_slot(max), Some(RosterSlot(0)));
        let session = session
            .with_member(member(0, RosterStatus::Alive, 0))
            .with_member(member(1, RosterStatus::Dead, 0))
            .with_member(member(2, RosterStatus::Alive, 0));
        assert_eq!(session.lowest_free_slot(max), None);
        // A gap left by a leave is the lowest free slot again.
        let gapped = session.without_slot(RosterSlot(1));
        assert_eq!(gapped.lowest_free_slot(max), Some(RosterSlot(1)));
    }

    #[test]
    fn with_member_keeps_the_roster_ascending_by_slot() {
        let session = MiniGameSession::open(key(), Tick(0), Tick(100))
            .with_member(member(2, RosterStatus::Alive, 0))
            .with_member(member(0, RosterStatus::Alive, 0))
            .with_member(member(1, RosterStatus::Alive, 0));
        let slots: Vec<u8> = session.roster.iter().map(|seated| seated.slot.0).collect();
        assert_eq!(slots, vec![0, 1, 2]);
    }

    #[test]
    fn with_score_added_accumulates_on_the_named_slot_and_saturates() {
        let session = MiniGameSession::open(key(), Tick(0), Tick(100))
            .with_member(member(0, RosterStatus::Alive, 0))
            .with_member(member(1, RosterStatus::Alive, 0))
            .with_score_added(RosterSlot(1), Score(5))
            .with_score_added(RosterSlot(1), Score(5));
        assert_eq!(session.member(RosterSlot(1)).unwrap().score, Score(10));
        assert_eq!(session.member(RosterSlot(0)).unwrap().score, Score(0));
        let saturated = session.with_score_added(RosterSlot(1), Score(u32::MAX));
        assert_eq!(
            saturated.member(RosterSlot(1)).unwrap().score,
            Score(u32::MAX)
        );
    }

    #[test]
    fn with_status_flips_only_the_named_slot() {
        let session = MiniGameSession::open(key(), Tick(0), Tick(100))
            .with_member(member(0, RosterStatus::Alive, 0))
            .with_member(member(1, RosterStatus::Alive, 0))
            .with_status(RosterSlot(0), RosterStatus::Dead);
        assert_eq!(
            session.member(RosterSlot(0)).unwrap().status,
            RosterStatus::Dead
        );
        assert_eq!(
            session.member(RosterSlot(1)).unwrap().status,
            RosterStatus::Alive
        );
    }

    #[test]
    fn take_removes_by_id_and_never_recycles_the_counter() {
        let mut monsters = SessionMonsters {
            live: vec![instanced(0, 1), instanced(1, 1)],
            next_id: 2,
        };
        let taken = monsters.take(SessionMonsterId(0)).unwrap();
        assert_eq!(taken.id, SessionMonsterId(0));
        assert_eq!(monsters.live.len(), 1);
        assert_eq!(monsters.next_id, 2);
        // A stale reference removes nothing.
        assert_eq!(monsters.take(SessionMonsterId(0)), None);
        assert_eq!(monsters.live.len(), 1);
    }

    #[test]
    fn wave_state_wire_forms_are_kind_tagged() {
        assert_eq!(
            serde_json::to_string(&WaveState::Pending {
                starts_at: Tick(100),
                ends_at: Tick(4300),
            })
            .unwrap(),
            r#"{"kind":"pending","starts_at":100,"ends_at":4300}"#
        );
        assert_eq!(
            serde_json::to_string(&WaveState::Running {
                ends_at: Tick(4300)
            })
            .unwrap(),
            r#"{"kind":"running","ends_at":4300}"#
        );
        assert_eq!(
            serde_json::to_string(&WaveState::Closed).unwrap(),
            r#"{"kind":"closed"}"#
        );
    }

    #[test]
    fn phase_wire_carries_each_deadline_and_snapshot_on_its_own_variant() {
        assert_eq!(
            serde_json::to_string(&MiniGamePhase::Open {
                closes_at: Tick(3100),
                next_notice: Tick(100),
            })
            .unwrap(),
            r#"{"kind":"open","closes_at":3100,"next_notice":100}"#
        );
        assert_eq!(
            serde_json::to_string(&MiniGamePhase::Closing {
                starts_at: Tick(3400)
            })
            .unwrap(),
            r#"{"kind":"closing","starts_at":3400}"#
        );
        assert_eq!(
            serde_json::to_string(&MiniGamePhase::Playing {
                ends_at: Tick(15_400),
                snapshot: PlayerCount(4),
            })
            .unwrap(),
            r#"{"kind":"playing","ends_at":15400,"snapshot":4}"#
        );
        assert_eq!(
            serde_json::to_string(&MiniGamePhase::Ended {
                disposes_at: Tick(17_200),
                snapshot: PlayerCount(4),
                remaining: Ticks(900),
            })
            .unwrap(),
            r#"{"kind":"ended","disposes_at":17200,"snapshot":4,"remaining":900}"#
        );
        assert_eq!(
            serde_json::to_string(&MiniGamePhase::Disposed).unwrap(),
            r#"{"kind":"disposed"}"#
        );
    }

    #[test]
    fn a_mid_playing_session_round_trips_byte_identically() {
        let session = MiniGameSession::open(key(), Tick(0), Tick(3000))
            .with_member(member(0, RosterStatus::Alive, 12))
            .with_member(member(1, RosterStatus::Dead, 4))
            .with_phase(MiniGamePhase::Playing {
                ends_at: Tick(15_400),
                snapshot: PlayerCount(2),
            })
            .with_winner(WinnerStanding::Won { by: RosterSlot(0) })
            .with_waves(WaveProgress {
                waves: vec![
                    WaveTrack {
                        number: WaveNumber(1),
                        state: WaveState::Running {
                            ends_at: Tick(4300),
                        },
                    },
                    WaveTrack {
                        number: WaveNumber(2),
                        state: WaveState::Pending {
                            starts_at: Tick(3100),
                            ends_at: Tick(8500),
                        },
                    },
                ],
                pending_respawns: vec![PendingRespawn {
                    monster: MonsterNumber(17),
                    wave: WaveNumber(1),
                    due: Tick(2100),
                }],
            })
            .with_monsters(SessionMonsters {
                live: vec![instanced(0, 1)],
                next_id: 1,
            });
        let json = serde_json::to_string(&session).unwrap();
        let reparsed: MiniGameSession = serde_json::from_str(&json).unwrap();
        assert_eq!(reparsed, session);
        assert_eq!(serde_json::to_string(&reparsed).unwrap(), json);
    }

    #[test]
    fn a_fresh_open_session_has_an_exact_wire_form() {
        let session = MiniGameSession::open(key(), Tick(100), Tick(3100)).with_member(member(
            0,
            RosterStatus::Alive,
            0,
        ));
        assert_eq!(
            serde_json::to_string(&session).unwrap(),
            concat!(
                r#"{"key":{"kind":"devil_square","level":3},"#,
                r#""phase":{"kind":"open","closes_at":3100,"next_notice":100},"#,
                r#""roster":[{"slot":0,"status":{"kind":"alive"},"score":0}],"#,
                r#""winner":{"kind":"none"},"#,
                r#""waves":{"waves":[],"pending_respawns":[]},"#,
                r#""monsters":{"live":[],"next_id":0}}"#,
            )
        );
    }
}
