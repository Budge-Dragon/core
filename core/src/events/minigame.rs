//! The observable transients of the mini-game framework: phase broadcasts,
//! spawn placements, the score table, and every applied grant. Kind-tagged,
//! flat, past-tense, entity-free — identity is a positional
//! [`RosterSlot`], amounts are domain newtypes; the host owns the account↔slot
//! map and event delivery.

use serde::{Deserialize, Serialize};

use crate::components::placement::Placement;
use crate::components::spatial::{Facing, WorldPos};
use crate::components::units::{Exp, Zen};
use crate::data::common::MonsterNumber;
use crate::data::minigame::{PlayerCount, Rank, RosterSlot, Score, WaveNumber};

/// One observable transient of the framework, kind-tagged. Returned by the
/// tick machine and the reward flow; the report intents return state-only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MiniGameEvent {
    /// A per-minute closing broadcast during the enter window
    /// (tick-relative).
    EntranceClosing {
        /// Whole minutes until entry closes.
        minutes_left: u8,
    },
    /// The entrance closed with enough players; the 30 s countdown began.
    CountdownStarted {
        /// The fixed countdown length, seconds.
        seconds: u16,
    },
    /// The game started; carries the frozen snapshot.
    GameStarted {
        /// The entered count at start.
        players: PlayerCount,
    },
    /// Too few alive players at entrance close; the event aborts.
    MinPlayersAbort {
        /// Alive players present.
        present: PlayerCount,
        /// The minimum required.
        required: PlayerCount,
    },
    /// A fee was refunded (the only refund path). A decision the host applies
    /// (the session owns no character).
    FeeRefunded {
        /// The refunded member.
        slot: RosterSlot,
        /// The amount refunded.
        amount: Zen,
    },
    /// A wave started.
    WaveStarted {
        /// The wave number.
        number: WaveNumber,
    },
    /// A wave/respawn placed a monster (the output of `place_spawn`'s
    /// random-cardinal draw).
    MonsterSpawned {
        /// The monster placed.
        number: MonsterNumber,
        /// Where it appeared.
        at: WorldPos,
        /// Which way it faces.
        facing: Facing,
    },
    /// The game ended; the remaining roster are the finishers.
    GameEnded {
        /// The finisher slots.
        finishers: Vec<RosterSlot>,
    },
    /// The ranked score table (one row per finisher).
    ScoreTable {
        /// The rows.
        rows: Vec<ScoreRow>,
    },
    /// One applied reward.
    RewardGranted {
        /// The finisher.
        slot: RosterSlot,
        /// What was granted.
        grant: GrantRecord,
    },
    /// An alive member was warped to town (pure movement; a decision the host
    /// applies). A dead member's eject is the host-composed respawn, not a
    /// framework event.
    WarpedOut {
        /// The warped member.
        slot: RosterSlot,
        /// The town landing.
        to: Placement,
    },
    /// The session disposed; terminal.
    Disposed,
}

/// One score-table row: the finisher's slot plus newtype values, no host id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreRow {
    /// The finisher.
    pub slot: RosterSlot,
    /// The assigned rank (1 = highest).
    pub rank: Rank,
    /// Accumulated score plus any `Score`-reward bonus.
    pub final_score: Score,
    /// Total money granted.
    pub granted_money: Zen,
    /// Total experience granted.
    pub granted_experience: Exp,
}

/// What a `RewardGranted` reports — the applied reward's payload, kind-tagged.
/// The item drop names no specific item: the item is rolled and stamped at
/// application and rides the ground drop's own event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GrantRecord {
    /// Experience granted (flat or per-second, already computed).
    Experience {
        /// The amount.
        amount: Exp,
    },
    /// Money granted.
    Money {
        /// The amount.
        amount: Zen,
    },
    /// A bonus score added to the final table score.
    Score {
        /// The amount.
        amount: Score,
    },
    /// An item-drop reward was granted (the item rides the ground drop
    /// event).
    ItemDrop,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;

    fn every_variant() -> Vec<MiniGameEvent> {
        vec![
            MiniGameEvent::EntranceClosing { minutes_left: 5 },
            MiniGameEvent::CountdownStarted { seconds: 30 },
            MiniGameEvent::GameStarted {
                players: PlayerCount(4),
            },
            MiniGameEvent::MinPlayersAbort {
                present: PlayerCount(1),
                required: PlayerCount(2),
            },
            MiniGameEvent::FeeRefunded {
                slot: RosterSlot(0),
                amount: Zen(25_000),
            },
            MiniGameEvent::WaveStarted {
                number: WaveNumber(1),
            },
            MiniGameEvent::MonsterSpawned {
                number: MonsterNumber(17),
                at: TileCoord::new(2, 3).to_world(),
                facing: Facing::POS_X_POS_Y,
            },
            MiniGameEvent::GameEnded {
                finishers: vec![RosterSlot(0), RosterSlot(2)],
            },
            MiniGameEvent::ScoreTable {
                rows: vec![ScoreRow {
                    slot: RosterSlot(0),
                    rank: Rank(1),
                    final_score: Score(42),
                    granted_money: Zen(30_000),
                    granted_experience: Exp(6000),
                }],
            },
            MiniGameEvent::RewardGranted {
                slot: RosterSlot(0),
                grant: GrantRecord::Experience { amount: Exp(6000) },
            },
            MiniGameEvent::WarpedOut {
                slot: RosterSlot(2),
                to: Placement {
                    position: TileCoord::new(2, 3).to_world(),
                    facing: Facing::POS_Y,
                    movement: Movement::Grounded,
                    map: MapNumber(0),
                },
            },
            MiniGameEvent::Disposed,
        ]
    }

    #[test]
    fn every_variant_round_trips_with_a_snake_case_kind_tag() {
        for event in every_variant() {
            let json = serde_json::to_string(&event).unwrap();
            assert!(json.starts_with(r#"{"kind":""#), "missing tag: {json}");
            assert_eq!(
                serde_json::from_str::<MiniGameEvent>(&json).unwrap(),
                event,
                "round-trip failed: {json}"
            );
        }
    }

    #[test]
    fn game_started_wire_carries_the_frozen_snapshot_flat() {
        let event = MiniGameEvent::GameStarted {
            players: PlayerCount(4),
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"kind":"game_started","players":4}"#
        );
    }

    #[test]
    fn monster_spawned_wire_mirrors_the_spawn_event_shape() {
        let event = MiniGameEvent::MonsterSpawned {
            number: MonsterNumber(17),
            at: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_X_POS_Y,
        };
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"kind":"monster_spawned","number":17,"at":{"x":163840,"y":229376},"facing":{"x":1,"y":1}}"#
        );
    }

    #[test]
    fn score_row_identifies_the_member_by_slot_and_amounts_by_newtypes() {
        let row = ScoreRow {
            slot: RosterSlot(0),
            rank: Rank(1),
            final_score: Score(42),
            granted_money: Zen(30_000),
            granted_experience: Exp(6000),
        };
        let json = serde_json::to_string(&row).unwrap();
        assert_eq!(
            json,
            r#"{"slot":0,"rank":1,"final_score":42,"granted_money":30000,"granted_experience":6000}"#
        );
        assert_eq!(serde_json::from_str::<ScoreRow>(&json).unwrap(), row);
    }

    #[test]
    fn grant_record_variants_round_trip_and_item_drop_names_no_item() {
        let grants = [
            GrantRecord::Experience { amount: Exp(6000) },
            GrantRecord::Money { amount: Zen(300) },
            GrantRecord::Score { amount: Score(600) },
            GrantRecord::ItemDrop,
        ];
        for grant in grants {
            let json = serde_json::to_string(&grant).unwrap();
            assert_eq!(serde_json::from_str::<GrantRecord>(&json).unwrap(), grant);
        }
        assert_eq!(
            serde_json::to_string(&GrantRecord::ItemDrop).unwrap(),
            r#"{"kind":"item_drop"}"#
        );
    }
}
