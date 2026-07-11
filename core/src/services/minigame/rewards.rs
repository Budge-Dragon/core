//! The win/lose reward algebra: the Ended roster's finishers are ranked by
//! accumulated score descending (ties stable on slot, sequential ranks from
//! one), each reward-table row applies iff its rank filter and its whole
//! success-flag conjunction hold, and the result fans out as per-finisher
//! grant *decisions* plus the ranked score table — never as N-character
//! mutation. Resolution draws no randomness; the character-mutating
//! application runs through the existing seams — experience through
//! [`crate::services::experience::apply_experience`], money through the
//! balance-preserving credit ([`apply_money_grant`]), and an item drop
//! through loot's group roll, the instance roll, and the ground stamping
//! ([`apply_item_drop_grant`], the only grant that samples).

use rand_core::RngCore;

use crate::components::units::{Exp, Tick, TickDuration, Zen};
use crate::data::atlas::{Atlas, MiniGameHandle};
use crate::data::minigame::{Rank, RewardDropGroup, RewardKind, RosterSlot, WinnerStanding};
use crate::entities::character::Character;
use crate::entities::minigame_session::{MiniGamePhase, MiniGameSession, RosterMember};
use crate::entities::world_item::WorldItem;
use crate::events::loot::Drop;
use crate::events::minigame::{GrantRecord, MiniGameEvent, ScoreRow};
use crate::services::ground::{DropOrigin, stamp_item};
use crate::services::item_roll::roll_dropped_item;
use crate::services::loot::roll_drop_group;

/// The reward fan-out: per-finisher grant decisions the caller applies
/// through the existing seams, plus the event stream — every applied grant
/// and, last, the ranked score table. Resolution is randomness-free; only the
/// item-drop application samples, later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardOutcome {
    /// Per-finisher grant decisions, in rank order.
    pub awards: Vec<FinisherAward>,
    /// One `RewardGranted` per applied reward, then the `ScoreTable`.
    pub events: Vec<MiniGameEvent>,
}

impl RewardOutcome {
    /// The nothing-resolved outcome — a session not in its Ended phase has no
    /// finishers and grants nothing.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            awards: Vec::new(),
            events: Vec::new(),
        }
    }
}

/// One finisher's grant decisions, in reward-table order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinisherAward {
    /// The finisher.
    pub slot: RosterSlot,
    /// The character-mutating grants to apply; empty when no row matched.
    pub grants: Vec<GrantDecision>,
}

/// A single character-mutating grant decision. A score bonus is not one — it
/// mutates the table, not a character. Experience is applied through
/// [`crate::services::experience::apply_experience`], money through
/// [`apply_money_grant`], and an item drop through [`apply_item_drop_grant`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantDecision {
    /// Add experience (flat, or the per-remaining-second fold, already
    /// computed).
    Experience {
        /// The amount to grant.
        amount: Exp,
    },
    /// Credit money.
    Money {
        /// The amount to credit.
        amount: Zen,
    },
    /// Roll this group and drop the item at the finisher's feet.
    ItemDrop {
        /// The self-contained group to roll.
        group: RewardDropGroup,
    },
}

/// Marks the session's winner while the game is running — a server-computed
/// fact (a quest delivery, a last-man standing), never a client claim. The
/// next advance observes the marker and ends the game early; outside Playing
/// the request is meaningless and changes nothing.
#[must_use]
pub fn finish_event(session: MiniGameSession, winner: RosterSlot) -> MiniGameSession {
    match session.phase {
        MiniGamePhase::Playing { .. } => session.with_winner(WinnerStanding::Won { by: winner }),
        MiniGamePhase::Open { .. }
        | MiniGamePhase::Closing { .. }
        | MiniGamePhase::Ended { .. }
        | MiniGamePhase::Disposed => session,
    }
}

/// Resolves the Ended session's rewards: ranks the finishers by accumulated
/// score descending (ties stable on slot, ranks sequential from one), applies
/// each reward-table row whose rank filter and whole flag conjunction hold,
/// folds the per-remaining-second experience through the whole-seconds basis
/// (a zero fold is skipped), accrues score bonuses onto the table's final
/// scores without re-ranking, and returns the grant decisions with the event
/// stream. Randomness-free; outside Ended it resolves nothing.
#[must_use]
pub fn resolve_rewards(
    session: &MiniGameSession,
    handle: &MiniGameHandle<'_>,
    tick: TickDuration,
) -> RewardOutcome {
    let MiniGamePhase::Ended { remaining, .. } = session.phase else {
        return RewardOutcome::empty();
    };
    let seconds = remaining.whole_seconds(tick);
    let mut ranked: Vec<&RosterMember> = session.roster.iter().collect();
    ranked.sort_by(|a, b| b.score.cmp(&a.score).then(a.slot.cmp(&b.slot)));

    let mut awards = Vec::with_capacity(ranked.len());
    let mut events = Vec::new();
    let mut rows = Vec::with_capacity(ranked.len());
    for (position, member) in ranked.iter().enumerate() {
        let rank = Rank(u16::try_from(position.saturating_add(1)).unwrap_or(u16::MAX));
        let mut grants = Vec::new();
        let mut final_score = member.score;
        let mut granted_money = Zen(0);
        let mut granted_experience = Exp(0);
        for entry in &handle.definition.reward_table {
            if !rank_matches(entry.rank, rank) {
                continue;
            }
            if !entry
                .flags
                .holds(member.status, session.winner, member.slot)
            {
                continue;
            }
            match &entry.reward {
                RewardKind::Experience { amount } => {
                    grants.push(GrantDecision::Experience { amount: *amount });
                    granted_experience = Exp(granted_experience.0.saturating_add(amount.0));
                    events.push(MiniGameEvent::RewardGranted {
                        slot: member.slot,
                        grant: GrantRecord::Experience { amount: *amount },
                    });
                }
                RewardKind::ExperiencePerRemainingSecond { amount } => {
                    let folded = Exp(seconds.saturating_mul(amount.0));
                    if folded.0 == 0 {
                        continue;
                    }
                    grants.push(GrantDecision::Experience { amount: folded });
                    granted_experience = Exp(granted_experience.0.saturating_add(folded.0));
                    events.push(MiniGameEvent::RewardGranted {
                        slot: member.slot,
                        grant: GrantRecord::Experience { amount: folded },
                    });
                }
                RewardKind::Money { amount } => {
                    grants.push(GrantDecision::Money { amount: *amount });
                    granted_money = Zen(granted_money.0.saturating_add(amount.0));
                    events.push(MiniGameEvent::RewardGranted {
                        slot: member.slot,
                        grant: GrantRecord::Money { amount: *amount },
                    });
                }
                RewardKind::ItemDrop { group } => {
                    grants.push(GrantDecision::ItemDrop {
                        group: group.clone(),
                    });
                    events.push(MiniGameEvent::RewardGranted {
                        slot: member.slot,
                        grant: GrantRecord::ItemDrop,
                    });
                }
                RewardKind::Score { amount } => {
                    final_score =
                        crate::data::minigame::Score(final_score.0.saturating_add(amount.0));
                    events.push(MiniGameEvent::RewardGranted {
                        slot: member.slot,
                        grant: GrantRecord::Score { amount: *amount },
                    });
                }
            }
        }
        rows.push(ScoreRow {
            slot: member.slot,
            rank,
            final_score,
            granted_money,
            granted_experience,
        });
        awards.push(FinisherAward {
            slot: member.slot,
            grants,
        });
    }
    events.push(MiniGameEvent::ScoreTable { rows });
    RewardOutcome { awards, events }
}

/// What applying a money grant produced: the credit runs through the
/// balance-preserving carried-zen seam, so an over-cap credit changes nothing
/// on the character while the grant stays recorded in the resolve events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MoneyGrant {
    /// The credit fit under the carry cap; the character holds the new
    /// balance.
    Credited {
        /// The credited character.
        character: Character,
    },
    /// The credit would cross the carry cap; nothing was credited and the
    /// balance is preserved.
    OverCap {
        /// The unchanged character.
        character: Character,
    },
}

/// Applies a money grant decision to a finisher through
/// [`crate::components::units::CarriedZen::credit`].
#[must_use]
pub fn apply_money_grant(finisher: Character, amount: Zen) -> MoneyGrant {
    match finisher.zen().credit(amount) {
        crate::components::units::CreditOutcome::Credited { balance } => MoneyGrant::Credited {
            character: finisher.with_zen(balance),
        },
        crate::components::units::CreditOutcome::OverCap { balance: _ } => MoneyGrant::OverCap {
            character: finisher,
        },
    }
}

/// What applying an item-drop grant produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemDropGrant {
    /// The rolled item, stamped at the finisher's feet — the host seats it on
    /// the ground at its (instant) appearance.
    Dropped {
        /// The ground item to seat.
        item: WorldItem,
    },
    /// No placeable item — the totality fold the two matches below need. A
    /// group roll only ever yields `Drop::Item`, and every reward-group item is
    /// proven present in the atlas at parse, so neither the `Drop` match nor the
    /// shared-accessor catalog lookup can actually reach this arm; it stands to
    /// keep both exhaustive without a suppressor.
    Nothing,
}

/// Applies an item-drop grant decision: rolls the group through loot's public
/// group-roll seam, rolls the picked item into a full instance, and stamps it
/// at the finisher's feet as a player-style drop — instant appearance, the
/// ownership window opened for the finisher the host knows. The only grant
/// application that draws randomness.
#[must_use]
pub fn apply_item_drop_grant(
    finisher: &Character,
    group: &RewardDropGroup,
    atlas: &Atlas,
    now: Tick,
    tick: TickDuration,
    rng: &mut impl RngCore,
) -> ItemDropGrant {
    match roll_drop_group(&group.items, group.item_level, rng) {
        Drop::Item {
            item,
            level,
            rarity,
        } => {
            let Some(def) = atlas.item(item) else {
                return ItemDropGrant::Nothing;
            };
            let instance = roll_dropped_item(def, level, rarity, atlas.option_roll(), rng);
            let stamp = stamp_item(
                DropOrigin::PlayerDrop,
                now,
                atlas.item_drop_duration(),
                tick,
            );
            let placement = finisher.placement();
            ItemDropGrant::Dropped {
                item: WorldItem {
                    instance,
                    position: placement.position,
                    map: placement.map,
                    despawn: stamp.despawn,
                    claim: stamp.claim,
                },
            }
        }
        // A group roll yields only an item pick; the other loot categories
        // fold to the same no-drop outcome for the totality proof.
        Drop::Zen { .. } | Drop::Nothing => ItemDropGrant::Nothing,
    }
}

/// Whether a reward-table row's rank filter admits `rank` — an absent filter
/// admits any rank.
fn rank_matches(filter: Option<Rank>, rank: Rank) -> bool {
    match filter {
        Some(required) => required == rank,
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::super::support::{character, fixture, open_session, tick100};
    use super::*;
    use crate::components::class::CharacterClass;
    use crate::components::collections::OneOrMore;
    use crate::components::units::{ItemLevel, Ticks};
    use crate::data::common::ItemRef;
    use crate::data::minigame::{
        PlayerCount, RewardEntry, RosterStatus, Score, SuccessFlag, SuccessFlags,
    };
    use crate::entities::minigame_session::RosterMember;

    fn ended(members: &[(u8, RosterStatus, u32)], remaining: Ticks) -> MiniGameSession {
        let mut session = open_session();
        for (slot, status, score) in members {
            session = session.with_member(RosterMember {
                slot: RosterSlot(*slot),
                status: *status,
                score: Score(*score),
            });
        }
        session.with_phase(MiniGamePhase::Ended {
            disposes_at: Tick(17_200),
            snapshot: PlayerCount(u16::try_from(members.len()).unwrap()),
            remaining,
        })
    }

    fn entry(rank: Option<u16>, flags: Vec<SuccessFlag>, reward: RewardKind) -> RewardEntry {
        RewardEntry {
            rank: rank.map(Rank),
            flags: SuccessFlags::new(flags).unwrap(),
            reward,
        }
    }

    #[test]
    fn finishers_rank_by_score_descending_with_slot_stable_ties() {
        let holder = fixture();
        let handle = holder.handle();
        let session = ended(
            &[
                (0, RosterStatus::Alive, 40),
                (1, RosterStatus::Alive, 90),
                (2, RosterStatus::Alive, 10),
                (3, RosterStatus::Alive, 40),
            ],
            Ticks(0),
        );
        let outcome = resolve_rewards(&session, &handle, tick100());
        let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
            panic!("the score table rides last");
        };
        let ranked: Vec<(u8, u16)> = rows.iter().map(|row| (row.slot.0, row.rank.0)).collect();
        // 90 -> rank 1; the tied 40s keep slot order; ranks are sequential.
        assert_eq!(ranked, vec![(1, 1), (0, 2), (3, 3), (2, 4)]);
    }

    #[test]
    fn a_rank_gated_reward_applies_only_at_its_rank() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![
            entry(
                Some(1),
                vec![],
                RewardKind::Experience { amount: Exp(6000) },
            ),
            entry(
                Some(2),
                vec![],
                RewardKind::Experience { amount: Exp(4000) },
            ),
        ];
        let handle = holder.handle();
        let session = ended(
            &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Alive, 90)],
            Ticks(0),
        );
        let outcome = resolve_rewards(&session, &handle, tick100());
        // Slot 1 is rank 1 (6000); slot 0 is rank 2 (4000); no crossover.
        assert_eq!(
            outcome.awards,
            vec![
                FinisherAward {
                    slot: RosterSlot(1),
                    grants: vec![GrantDecision::Experience { amount: Exp(6000) }],
                },
                FinisherAward {
                    slot: RosterSlot(0),
                    grants: vec![GrantDecision::Experience { amount: Exp(4000) }],
                },
            ]
        );
    }

    #[test]
    fn alive_and_dead_flags_gate_by_finisher_status() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![
            entry(
                None,
                vec![SuccessFlag::Alive],
                RewardKind::ItemDrop {
                    group: RewardDropGroup {
                        items: OneOrMore::new(vec![ItemRef {
                            group: 12,
                            number: 15,
                        }])
                        .unwrap(),
                        item_level: ItemLevel::ZERO,
                    },
                },
            ),
            entry(
                None,
                vec![SuccessFlag::Dead],
                RewardKind::Money { amount: Zen(300) },
            ),
        ];
        let handle = holder.handle();
        let session = ended(
            &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Dead, 10)],
            Ticks(0),
        );
        let outcome = resolve_rewards(&session, &handle, tick100());
        assert_eq!(outcome.awards.len(), 2);
        assert!(matches!(
            outcome.awards[0].grants.as_slice(),
            [GrantDecision::ItemDrop { .. }]
        ));
        assert_eq!(outcome.awards[0].slot, RosterSlot(0));
        assert_eq!(
            outcome.awards[1].grants,
            vec![GrantDecision::Money { amount: Zen(300) }]
        );
        assert_eq!(outcome.awards[1].slot, RosterSlot(1));
    }

    #[test]
    fn the_winner_flags_resolve_against_the_marker() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![
            entry(
                None,
                vec![SuccessFlag::Winner],
                RewardKind::Money { amount: Zen(1000) },
            ),
            entry(
                None,
                vec![SuccessFlag::Loser],
                RewardKind::Money { amount: Zen(10) },
            ),
            entry(
                None,
                vec![SuccessFlag::WinnerExists],
                RewardKind::Money { amount: Zen(1) },
            ),
        ];
        let handle = holder.handle();
        let session = ended(
            &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Alive, 90)],
            Ticks(0),
        )
        .with_winner(WinnerStanding::Won { by: RosterSlot(1) });
        let outcome = resolve_rewards(&session, &handle, tick100());
        // Rank 1 is slot 1 — the winner: Winner + WinnerExists.
        assert_eq!(
            outcome.awards[0].grants,
            vec![
                GrantDecision::Money { amount: Zen(1000) },
                GrantDecision::Money { amount: Zen(1) },
            ]
        );
        // Slot 0 — the loser: Loser + WinnerExists.
        assert_eq!(
            outcome.awards[1].grants,
            vec![
                GrantDecision::Money { amount: Zen(10) },
                GrantDecision::Money { amount: Zen(1) },
            ]
        );
    }

    #[test]
    fn with_no_winner_only_the_winner_not_exists_rewards_apply() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![
            entry(
                None,
                vec![SuccessFlag::Winner],
                RewardKind::Money { amount: Zen(1000) },
            ),
            entry(
                None,
                vec![SuccessFlag::WinnerNotExists],
                RewardKind::Money { amount: Zen(50) },
            ),
        ];
        let handle = holder.handle();
        let session = ended(
            &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Alive, 90)],
            Ticks(0),
        );
        let outcome = resolve_rewards(&session, &handle, tick100());
        for award in &outcome.awards {
            assert_eq!(award.grants, vec![GrantDecision::Money { amount: Zen(50) }]);
        }
    }

    #[test]
    fn per_remaining_second_experience_floors_seconds_and_a_timeout_grants_nothing() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![entry(
            None,
            vec![],
            RewardKind::ExperiencePerRemainingSecond { amount: Exp(160) },
        )];
        let handle = holder.handle();
        // 905 ticks at 100 ms = 90.5 s -> floors to 90 whole seconds.
        let early = ended(&[(0, RosterStatus::Alive, 40)], Ticks(905));
        let outcome = resolve_rewards(&early, &handle, tick100());
        assert_eq!(
            outcome.awards[0].grants,
            vec![GrantDecision::Experience {
                amount: Exp(90 * 160)
            }]
        );
        // A timeout has zero remaining: the reward is skipped outright.
        let timeout = ended(&[(0, RosterStatus::Alive, 40)], Ticks(0));
        let outcome = resolve_rewards(&timeout, &handle, tick100());
        assert!(outcome.awards[0].grants.is_empty());
        assert!(
            !outcome
                .events
                .iter()
                .any(|event| matches!(event, MiniGameEvent::RewardGranted { .. }))
        );
    }

    #[test]
    fn a_score_bonus_raises_the_final_score_without_re_ranking() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![entry(
            None,
            vec![SuccessFlag::Alive],
            RewardKind::Score { amount: Score(600) },
        )];
        let handle = holder.handle();
        let session = ended(
            &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Dead, 90)],
            Ticks(0),
        );
        let outcome = resolve_rewards(&session, &handle, tick100());
        let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
            panic!("the score table rides last");
        };
        // Ranks follow the accumulated (pre-bonus) scores: 90 stays rank 1
        // even though the alive 40 finishes with a larger final score.
        assert_eq!(rows[0].slot, RosterSlot(1));
        assert_eq!(rows[0].rank, Rank(1));
        assert_eq!(rows[0].final_score, Score(90));
        assert_eq!(rows[1].slot, RosterSlot(0));
        assert_eq!(rows[1].rank, Rank(2));
        assert_eq!(rows[1].final_score, Score(640));
        // A score bonus is table-only: no character-mutating decision rides.
        assert!(outcome.awards.iter().all(|award| award.grants.is_empty()));
        // But the grant is still reported.
        assert!(outcome.events.iter().any(|event| matches!(
            event,
            MiniGameEvent::RewardGranted {
                grant: GrantRecord::Score { .. },
                ..
            }
        )));
    }

    #[test]
    fn the_table_totals_money_and_experience_per_finisher() {
        let mut holder = fixture();
        holder.definition.reward_table = vec![
            entry(None, vec![], RewardKind::Experience { amount: Exp(6000) }),
            entry(None, vec![], RewardKind::Money { amount: Zen(300) }),
            entry(None, vec![], RewardKind::Money { amount: Zen(200) }),
        ];
        let handle = holder.handle();
        let session = ended(&[(0, RosterStatus::Alive, 40)], Ticks(0));
        let outcome = resolve_rewards(&session, &handle, tick100());
        let Some(MiniGameEvent::ScoreTable { rows }) = outcome.events.last() else {
            panic!("the score table rides last");
        };
        assert_eq!(rows[0].granted_experience, Exp(6000));
        assert_eq!(rows[0].granted_money, Zen(500));
        // Grants ride first, the table last.
        assert_eq!(outcome.events.len(), 4);
    }

    #[test]
    fn resolving_outside_the_ended_phase_grants_nothing() {
        let holder = fixture();
        let handle = holder.handle();
        let playing = open_session()
            .with_member(RosterMember {
                slot: RosterSlot(0),
                status: RosterStatus::Alive,
                score: Score(40),
            })
            .with_phase(MiniGamePhase::Playing {
                ends_at: Tick(15_400),
                snapshot: PlayerCount(1),
            });
        assert_eq!(
            resolve_rewards(&playing, &handle, tick100()),
            RewardOutcome::empty()
        );
    }

    #[test]
    fn finish_event_marks_the_winner_only_while_playing() {
        let playing = open_session().with_phase(MiniGamePhase::Playing {
            ends_at: Tick(15_400),
            snapshot: PlayerCount(2),
        });
        let finished = finish_event(playing, RosterSlot(1));
        assert_eq!(finished.winner, WinnerStanding::Won { by: RosterSlot(1) });
        // Outside Playing the request changes nothing.
        let open = open_session();
        assert_eq!(finish_event(open.clone(), RosterSlot(1)), open);
        let ended = open_session().with_phase(MiniGamePhase::Ended {
            disposes_at: Tick(17_200),
            snapshot: PlayerCount(2),
            remaining: Ticks(0),
        });
        assert_eq!(finish_event(ended.clone(), RosterSlot(1)), ended);
    }

    #[test]
    fn a_money_grant_credits_through_the_carried_zen_seam() {
        let finisher = character(CharacterClass::DarkKnight, 60, 100_000);
        let MoneyGrant::Credited { character } = apply_money_grant(finisher, Zen(500_000)) else {
            panic!("a covered credit lands");
        };
        assert_eq!(character.zen().get(), 600_000);
    }

    #[test]
    fn an_over_cap_money_grant_is_reported_and_credits_nothing() {
        let finisher = character(CharacterClass::DarkKnight, 60, 1_999_999_900);
        let MoneyGrant::OverCap { character } = apply_money_grant(finisher, Zen(500_000)) else {
            panic!("an over-cap credit is reported");
        };
        assert_eq!(character.zen().get(), 1_999_999_900);
    }

    #[test]
    fn resolution_is_deterministic_and_draws_no_randomness() {
        let holder = fixture();
        let handle = holder.handle();
        let session = ended(
            &[(0, RosterStatus::Alive, 40), (1, RosterStatus::Dead, 90)],
            Ticks(300),
        );
        let first = resolve_rewards(&session, &handle, tick100());
        let second = resolve_rewards(&session, &handle, tick100());
        assert_eq!(first, second);
    }
}
