//! Party lifecycle and the two shares — pure decision functions over one
//! [`PartySession`] value, host-resolved live facts, and the injected RNG (drawn
//! only by [`distribute_kill_experience`], once per kill regardless of party
//! size). Every transition is
//! `(session, intent[, facts][, now, tick]) -> (session | terminal, outcome, events)`
//! with no clock read, no float, and no host import; the client never states a
//! result. Disband is a terminal *outcome* (`*::Disbanded`) the host acts on, not
//! a resting session variant — a sub-threshold party never crosses a port.
//!
//! Leadership is stored on the session and re-derived on every removal, hold, and
//! reconnect so `Led { by }` always names an `Active` member (the stale-master
//! bug made unrepresentable). The rule is *sticky*: it moves only when its holder
//! vacates, or when a `Vacant` party gains its first `Active` member — a lower-slot
//! reconnect never steals it.

use rand_core::RngCore;

use serde::{Deserialize, Serialize};

use crate::components::party::{Leadership, MemberSlot, Membership, Vitality};
use crate::components::spatial::{Radius, WorldPos};
use crate::components::units::{
    CarriedZen, CreditOutcome, DurationMs, Exp, Level, MapNumber, Tick, TickDuration, Zen,
};
use crate::data::atlas::Atlas;
use crate::entities::party_session::{PartyInvite, PartyMember, PartySession};
use crate::entities::world_zen::WorldZen;
use crate::events::party::{AcceptBounce, InviteRejection, MemberAward, PartyEvent};
use crate::services::experience::{draw_jitter_percent, level_ups_from, unjittered_base};
use crate::services::ratio::{nonzero_u64, scale_ratio_u64};

/// The disconnect hold window: a disconnected member's seat is reserved this long
/// before [`advance_party`] reaps it (OUR pin — 5 minutes). Held in core as a
/// [`DurationMs`], converted with the host-fed [`TickDuration`], so the rule stays
/// in core and the clock stays host-owned.
pub const PARTY_HOLD: DurationMs = DurationMs(300_000);

/// The pending-invite lease: an unanswered invite lapses this long after it is
/// sent (OUR pin — 60 seconds), reaped by [`advance_invite`].
pub const INVITE_TTL: DurationMs = DurationMs(60_000);

/// The invite and share reach — 12 tiles on the same map.
fn party_reach() -> Radius {
    Radius::from_tiles(12)
}

/// Whether two loci are on the same map and within [`party_reach`] — the folded
/// same-map-and-range gate; a cross-map pair fails it.
fn reach(a_pos: WorldPos, a_map: MapNumber, b_pos: WorldPos, b_map: MapNumber) -> bool {
    a_map == b_map && a_pos.within_range(b_pos, party_reach())
}

/// The host-resolved facts about an invite TARGET — the id-free channel, the
/// `TradeAvailability` grain generalized. Every variant is a FACT, never a
/// pre-baked decision: no `OutOfRange`/`Qualified` variant (core computes same-map
/// and range from `Available`). Self, already-partied, and pending are distinct
/// host facts, each mapping to its own named refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartyAvailability {
    /// The target resolved to the inviter's own character.
    SameActor,
    /// The target already holds a party.
    AlreadyPartied,
    /// The target already holds an outstanding invite.
    PendingInvite,
    /// The target is not alive.
    TargetDead,
    /// A live, free target at this locus.
    Available {
        /// The target's current position, checked against the reach rule.
        position: WorldPos,
        /// The target's current map, checked against the same-map rule.
        map: MapNumber,
    },
}

/// The INVITER's re-resolved presence at accept — a two-state fact, not
/// [`PartyAvailability`]: `AlreadyPartied` is meaningful-but-wrong for the inviter
/// (it means "append", carried by `inviter_party`, not "gone"). `Gone` folds
/// dead/disconnected/off-map/left into the one `InviterGone` bounce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InviterPresence {
    /// The inviter is unreachable — dead, disconnected, or off-map.
    Gone,
    /// The inviter is present at this locus.
    Present {
        /// The inviter's current position.
        position: WorldPos,
        /// The inviter's current map.
        map: MapNumber,
    },
}

/// The inviter's party context — the fusion of the party and the inviter's slot
/// in it (never a config grab-bag). Both are needed and inseparable: core checks
/// `party.leadership() == Led { by: inviter_slot }` (the leader-only rule stays in
/// core; the inviter→slot mapping stays host-owned). [`invite`] borrows it
/// read-only; [`accept_invite`] consumes and grows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InviterParty {
    /// The inviter's current party.
    pub party: PartySession,
    /// The inviter's slot in that party.
    pub inviter_slot: MemberSlot,
}

/// One member's host-resolved live facts at a kill or pickup — the id-free
/// `TradeAvailability` grain generalized to N, carrying the three live terms
/// (`vitality`, `map`, `position`) plus `level`/`experience` for the pool and the
/// per-member level-up walk. Presence (the fourth term) comes from the
/// [`PartySession`], joined by `slot`, so core applies the whole four-term
/// predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberFact {
    /// The member's slot, joined against the session's presence.
    pub slot: MemberSlot,
    /// The member's total level — feeds the average and the proportional split.
    pub level: Level,
    /// The member's current experience — the base for its own level-up walk.
    pub experience: Exp,
    /// The member's liveness — a dead member earns no share.
    pub vitality: Vitality,
    /// The member's current map.
    pub map: MapNumber,
    /// The member's current position.
    pub position: WorldPos,
}

/// A member's wallet keyed by slot — a named pair (not a bare tuple) so a
/// returned credit names its owner. As [`split_zen_pickup`] INPUT (`other_wallets`)
/// it is co-indexed with `others` (parallel slices, `other_wallets[i]` is
/// `others[i]`'s wallet); the picker's own wallet arrives separately by value. As
/// OUTPUT `credits` it carries each credited qualifier's NEW balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotWallet {
    /// The wallet's owner.
    pub slot: MemberSlot,
    /// The balance.
    pub wallet: CarriedZen,
}

/// What an invite produced, kind-tagged. `Sent` carries the [`PartyInvite`]
/// entity, so this lives in the service, not `events`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InviteOutcome {
    /// The invite went out; the owned invite carries only its expiry.
    Sent {
        /// The pending invite.
        invite: PartyInvite,
    },
    /// The invite was refused with a named reason; no invite exists.
    Rejected {
        /// Why the invite was refused.
        reason: InviteRejection,
    },
}

/// What an accept produced, kind-tagged. `Joined` hands back a whole
/// [`PartySession`] entity, so this lives in the service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcceptOutcome {
    /// The responder joined; the returned session is new (solo inviter) or grown.
    Joined {
        /// The new or grown party.
        session: PartySession,
    },
    /// A live re-check failed; no party formed or grew.
    Bounced {
        /// Why the accept bounced.
        reason: AcceptBounce,
    },
}

/// What a kick produced, kind-tagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KickOutcome {
    /// The target was removed; the surviving party is handed back.
    Kicked {
        /// The party with the target removed.
        session: PartySession,
    },
    /// The removal dropped the roster below the minimum — terminal; the host
    /// deletes the session.
    Disbanded,
    /// The actor does not lead the party.
    NotLeader,
    /// No member occupies the target slot.
    NoSuchMember,
    /// The leader named its own slot — self-exit is `leave`, not `kick`.
    CannotKickSelf,
}

/// What a leave produced, kind-tagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LeaveOutcome {
    /// The actor left; the surviving party is handed back.
    Left {
        /// The party with the actor removed.
        session: PartySession,
    },
    /// The removal dropped the roster below the minimum — terminal.
    Disbanded,
    /// No member occupies the actor slot.
    NoSuchMember,
}

/// What a disconnect produced, kind-tagged. Disconnect never disbands (the seat
/// is held, so the count is unchanged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DisconnectOutcome {
    /// The member's seat is held; the party is handed back.
    Disconnected {
        /// The party with the seat held.
        session: PartySession,
    },
    /// No member occupies the slot.
    NoSuchMember,
}

/// What a reconnect produced, kind-tagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReconnectOutcome {
    /// The member is active again; the party is handed back.
    Reconnected {
        /// The party with the seat restored.
        session: PartySession,
    },
    /// No member occupies the slot.
    NoSuchMember,
}

/// What a per-tick held-seat sweep ([`advance_party`]) produced, kind-tagged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartyOutcome {
    /// The party continues (reaped, leadership re-derived, or unchanged).
    Continues {
        /// The surviving party.
        session: PartySession,
    },
    /// The reap dropped the roster below the minimum — terminal.
    Disbanded,
}

/// What a per-invite sweep ([`advance_invite`]) produced, kind-tagged. `Pending`
/// carries the [`PartyInvite`] entity, so this lives in the service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InviteSweep {
    /// The invite has not lapsed; it is handed back.
    Pending {
        /// The still-pending invite.
        invite: PartyInvite,
    },
    /// The invite lapsed at its expiry.
    Lapsed,
}

/// The party zen split — declared here (not `events`) because `to_ground` holds
/// [`WorldZen`] entities. Conserves every coin: `credits` (new balances the host
/// writes back) plus `to_ground` (fresh piles the host lays down) account for the
/// whole pile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZenSplitResult {
    /// Each qualifier's new balance after its share credited.
    pub credits: Vec<SlotWallet>,
    /// Over-cap shares as fresh piles the host lays down (move-only).
    pub to_ground: Vec<WorldZen>,
}

/// Sends an invite to a host-resolved target. Leader/full gates first (they hold
/// regardless of the target), then the target facts; range and same-map are
/// computed here from `Available`. A refusal names its reason — never silence.
#[must_use]
pub fn invite(
    inviter_pos: WorldPos,
    inviter_map: MapNumber,
    target: PartyAvailability,
    inviter_party: Option<&InviterParty>,
    now: Tick,
    tick: TickDuration,
) -> (InviteOutcome, Vec<PartyEvent>) {
    if let Some(context) = inviter_party {
        if context.party.leadership()
            != (Leadership::Led {
                by: context.inviter_slot,
            })
        {
            return rejected(InviteRejection::NotLeader);
        }
        if context.party.len() >= usize::from(MemberSlot::CAP) {
            return rejected(InviteRejection::PartyFull);
        }
    }
    let reason = match target {
        PartyAvailability::SameActor => InviteRejection::SameActor,
        PartyAvailability::AlreadyPartied => InviteRejection::AlreadyPartied,
        PartyAvailability::PendingInvite => InviteRejection::PendingInvite,
        PartyAvailability::TargetDead => InviteRejection::TargetDead,
        PartyAvailability::Available { position, map } => {
            if reach(inviter_pos, inviter_map, position, map) {
                let invite = PartyInvite {
                    expires: now + INVITE_TTL.in_ticks(tick),
                };
                return (
                    InviteOutcome::Sent { invite },
                    vec![PartyEvent::InviteSent, PartyEvent::InviteReceived],
                );
            }
            InviteRejection::OutOfRange
        }
    };
    rejected(reason)
}

/// Accepts an invite. Four live re-checks (inviter present, in reach, still
/// leading, party not full), each a named bounce; then joins — a solo inviter
/// forms a new party (inviter slot 0 leader, responder slot 1), an existing party
/// appends the responder at its lowest free slot. The responder's slot is
/// core-assigned, never host-supplied.
#[must_use]
pub fn accept_invite(
    inviter_presence: InviterPresence,
    inviter_party: Option<InviterParty>,
    responder_pos: WorldPos,
    responder_map: MapNumber,
) -> (AcceptOutcome, Vec<PartyEvent>) {
    let (inviter_pos, inviter_map) = match inviter_presence {
        InviterPresence::Gone => return bounced(AcceptBounce::InviterGone),
        InviterPresence::Present { position, map } => (position, map),
    };
    if !reach(inviter_pos, inviter_map, responder_pos, responder_map) {
        return bounced(AcceptBounce::OutOfRange);
    }
    match inviter_party {
        None => {
            let session = PartySession::forming();
            (
                AcceptOutcome::Joined { session },
                vec![PartyEvent::Joined {
                    slot: MemberSlot(1),
                }],
            )
        }
        Some(context) => {
            if context.party.leadership()
                != (Leadership::Led {
                    by: context.inviter_slot,
                })
            {
                return bounced(AcceptBounce::InviterNotLeader);
            }
            let Some(slot) = context.party.lowest_free_slot() else {
                return bounced(AcceptBounce::PartyFull);
            };
            let session = context.party.with_member(PartyMember {
                slot,
                membership: Membership::Active,
            });
            (
                AcceptOutcome::Joined { session },
                vec![PartyEvent::Joined { slot }],
            )
        }
    }
}

/// Declines an invite — the sole intent with no state and no decision (it always
/// succeeds and reads nothing from the invite). Kept in core so "notify both on
/// decline" is a returned domain event, not a host-invented effect.
#[must_use]
pub fn decline_invite() -> Vec<PartyEvent> {
    vec![PartyEvent::InviteDeclined]
}

/// Kicks a member. Gate order: `NotLeader` first (only the leader kicks, so the
/// leader is unkickable by others), then `CannotKickSelf` (the leader naming its
/// own slot — self-exit is `leave`), then `NoSuchMember` (a total roster over an
/// absent slot), then remove. A removal dropping below the minimum disbands.
#[must_use]
pub fn kick(
    session: PartySession,
    actor: MemberSlot,
    target: MemberSlot,
) -> (KickOutcome, Vec<PartyEvent>) {
    if session.leadership() != (Leadership::Led { by: actor }) {
        return (KickOutcome::NotLeader, Vec::new());
    }
    if target == actor {
        return (KickOutcome::CannotKickSelf, Vec::new());
    }
    if session.member(target).is_none() {
        return (KickOutcome::NoSuchMember, Vec::new());
    }
    match remove_member(session, target) {
        Removal::Disbanded => (
            KickOutcome::Disbanded,
            vec![
                PartyEvent::MemberKicked { slot: target },
                PartyEvent::Disbanded,
            ],
        ),
        Removal::Survived { session, transfer } => {
            let mut events = vec![PartyEvent::MemberKicked { slot: target }];
            events.extend(transfer);
            (KickOutcome::Kicked { session }, events)
        }
    }
}

/// Removes the actor from the party (any member may leave). A leader leaving a 3+
/// party re-derives leadership to the earliest `Active` member; a leave dropping
/// below the minimum disbands and notifies everyone, including the leaver.
#[must_use]
pub fn leave(session: PartySession, actor: MemberSlot) -> (LeaveOutcome, Vec<PartyEvent>) {
    if session.member(actor).is_none() {
        return (LeaveOutcome::NoSuchMember, Vec::new());
    }
    match remove_member(session, actor) {
        Removal::Disbanded => (
            LeaveOutcome::Disbanded,
            vec![
                PartyEvent::MemberLeft { slot: actor },
                PartyEvent::Disbanded,
            ],
        ),
        Removal::Survived { session, transfer } => {
            let mut events = vec![PartyEvent::MemberLeft { slot: actor }];
            events.extend(transfer);
            (LeaveOutcome::Left { session }, events)
        }
    }
}

/// Holds a disconnected member's seat until `now + PARTY_HOLD` (the count is
/// unchanged, so disconnect never disbands). A leader disconnect moves leadership
/// immediately to the earliest `Active` member.
#[must_use]
pub fn disconnect(
    session: PartySession,
    slot: MemberSlot,
    now: Tick,
    tick: TickDuration,
) -> (DisconnectOutcome, Vec<PartyEvent>) {
    if session.member(slot).is_none() {
        return (DisconnectOutcome::NoSuchMember, Vec::new());
    }
    let before = session.leadership();
    let expires = now + PARTY_HOLD.in_ticks(tick);
    let held = session.with_membership(slot, Membership::Held { expires });
    let after = leadership_after_hold(before, &held);
    let held = held.with_leadership(after);
    let mut events = vec![PartyEvent::MemberHeld { slot }];
    events.extend(transfer_event(before, after));
    (DisconnectOutcome::Disconnected { session: held }, events)
}

/// Restores a held member's seat as a REGULAR member — a reconnecting ex-leader
/// does not reclaim (sticky). From `Vacant` the first reconnect takes leadership.
#[must_use]
pub fn reconnect(session: PartySession, slot: MemberSlot) -> (ReconnectOutcome, Vec<PartyEvent>) {
    if session.member(slot).is_none() {
        return (ReconnectOutcome::NoSuchMember, Vec::new());
    }
    let before = session.leadership();
    let active = session.with_membership(slot, Membership::Active);
    let after = leadership_after_reconnect(before, slot);
    let active = active.with_leadership(after);
    let mut events = vec![PartyEvent::MemberReconnected { slot }];
    events.extend(transfer_event(before, after));
    (ReconnectOutcome::Reconnected { session: active }, events)
}

/// Reaps every held seat whose expiry has been reached (batch), runs the removal
/// path once, re-derives leadership once, and checks disband once. A reaped seat
/// is always `Held` — never the leader — so leadership stays sticky.
#[must_use]
pub fn advance_party(session: PartySession, now: Tick) -> (PartyOutcome, Vec<PartyEvent>) {
    let before = session.leadership();
    let expired: Vec<MemberSlot> = session
        .members()
        .iter()
        .filter(|member| {
            matches!(member.membership, Membership::Held { expires } if expires.reached(now))
        })
        .map(|member| member.slot)
        .collect();
    if expired.is_empty() {
        return (PartyOutcome::Continues { session }, Vec::new());
    }
    let mut roster = session;
    for slot in &expired {
        roster = roster.without_slot(*slot);
    }
    let mut events: Vec<PartyEvent> = expired
        .iter()
        .map(|slot| PartyEvent::MemberExpired { slot: *slot })
        .collect();
    if roster.len() < PartySession::MIN_MEMBERS {
        events.push(PartyEvent::Disbanded);
        return (PartyOutcome::Disbanded, events);
    }
    let after = leadership_after_removals(before, &roster);
    let roster = roster.with_leadership(after);
    events.extend(transfer_event(before, after));
    (PartyOutcome::Continues { session: roster }, events)
}

/// Reaps one pending invite whose lease has lapsed. The host owns the separate
/// pending-invite collection and calls this per invite.
#[must_use]
pub fn advance_invite(invite: PartyInvite, now: Tick) -> (InviteSweep, Vec<PartyEvent>) {
    if invite.expires.reached(now) {
        (InviteSweep::Lapsed, vec![PartyEvent::InviteExpired])
    } else {
        (InviteSweep::Pending { invite }, Vec::new())
    }
}

/// Distributes one kill's experience across the qualifying party — the only
/// RNG-drawing service (one jitter word per kill). The killer's fact arrives by
/// value and seeds the qualifying set `Q` unconditionally — it is definitionally a
/// participant (its own map, in range of itself, alive, present) — while also
/// anchoring the share predicate the `others` are tested against. Each qualifier's
/// pooled, level-proportional share is computed, the split remainder lands on the
/// killer, and each award carries its own level-up walk. Degenerates to the
/// byte-identical solo award at `|Q| = 1`.
#[must_use]
pub fn distribute_kill_experience(
    party: &PartySession,
    killer: MemberFact,
    others: &[MemberFact],
    victim_level: Level,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> Vec<MemberAward> {
    let mut qualifiers: Vec<MemberFact> = Vec::with_capacity(others.len() + 1);
    qualifiers.push(killer);
    qualifiers.extend(
        others
            .iter()
            .copied()
            .filter(|fact| qualifies(party, &killer, fact)),
    );
    let count = saturating_u64(qualifiers.len());
    let level_sum: u64 = qualifiers
        .iter()
        .map(|fact| u64::from(fact.level.get()))
        .sum();
    let average = Level::clamped(level_sum / nonzero_u64(count).get());

    let base = unjittered_base(average, victim_level);
    let (bonus_num, bonus_den) = party_bonus(count);
    let pool_pre = scale_ratio_u64(
        u64::from(base).saturating_mul(count),
        bonus_num,
        nonzero_u64(bonus_den),
    );
    let percent = draw_jitter_percent(atlas, rng);
    let pool = scale_ratio_u64(pool_pre, u64::from(percent), nonzero_u64(100));

    let denominator = nonzero_u64(level_sum);
    let mut awarded: u64 = 0;
    let mut shares: Vec<(MemberFact, u64)> = Vec::with_capacity(qualifiers.len());
    for fact in qualifiers {
        let share = scale_ratio_u64(pool, u64::from(fact.level.get()), denominator);
        awarded = awarded.saturating_add(share);
        shares.push((fact, share));
    }
    let remainder = pool.saturating_sub(awarded);

    shares
        .into_iter()
        .map(|(fact, share)| {
            let gained = if fact.slot == killer.slot {
                share.saturating_add(remainder)
            } else {
                share
            };
            MemberAward {
                slot: fact.slot,
                gained: Exp(gained),
                level_ups: level_ups_from(
                    fact.level,
                    Exp(fact.experience.0.saturating_add(gained)),
                    atlas,
                ),
            }
        })
        .collect()
}

/// Splits one zen pile equally across the qualifying party — no RNG. The picker's
/// fact and wallet arrive by value and seed the qualifying set `Q` unconditionally
/// (the picker is definitionally a participant, so the divisor is never zero),
/// while the picker also anchors the same four-term predicate the `others` are
/// tested against. The division remainder lands on the picker, and an at-cap
/// member's share grounds as a fresh pile rather than being destroyed. Conserves
/// every coin — `credits` deltas plus `to_ground` amounts sum to `pile.amount`.
/// `other_wallets` is co-indexed with `others` (parallel slices).
#[must_use]
pub fn split_zen_pickup(
    pile: &WorldZen,
    party: &PartySession,
    picker: MemberFact,
    picker_wallet: CarriedZen,
    others: &[MemberFact],
    other_wallets: &[SlotWallet],
) -> ZenSplitResult {
    let mut qualifiers: Vec<(MemberFact, CarriedZen)> = Vec::with_capacity(others.len() + 1);
    qualifiers.push((picker, picker_wallet));
    qualifiers.extend(
        others
            .iter()
            .zip(other_wallets)
            .filter(|(fact, _)| qualifies(party, &picker, fact))
            .map(|(fact, slot_wallet)| (*fact, slot_wallet.wallet)),
    );
    let count = saturating_u64(qualifiers.len());
    let share = pile.amount.0 / nonzero_u64(count).get();
    let remainder = pile.amount.0.saturating_sub(share.saturating_mul(count));

    let mut credits = Vec::new();
    let mut to_ground = Vec::new();
    for (fact, wallet) in qualifiers {
        let amount = if fact.slot == picker.slot {
            share.saturating_add(remainder)
        } else {
            share
        };
        match wallet.credit(Zen(amount)) {
            CreditOutcome::Credited { balance } => credits.push(SlotWallet {
                slot: fact.slot,
                wallet: balance,
            }),
            CreditOutcome::OverCap { .. } => to_ground.push(WorldZen {
                amount: Zen(amount),
                position: pile.position,
                map: pile.map,
                despawn: pile.despawn,
            }),
        }
    }
    ZenSplitResult { credits, to_ground }
}

/// The outcome of removing one member: disband (terminal) or a survived party
/// plus the leadership-transfer event, if any.
enum Removal {
    /// The removal dropped the roster below the minimum.
    Disbanded,
    /// The party survives, with a possible leadership transfer.
    Survived {
        /// The surviving party, leadership re-derived.
        session: PartySession,
        /// The transfer event when leadership moved.
        transfer: Option<PartyEvent>,
    },
}

/// Removes `slot` (the caller proved it a member), re-derives leadership, and
/// decides disband vs survival. The removal event is prepended by the caller.
fn remove_member(session: PartySession, slot: MemberSlot) -> Removal {
    let before = session.leadership();
    let new_roster = session.without_slot(slot);
    if new_roster.len() < PartySession::MIN_MEMBERS {
        return Removal::Disbanded;
    }
    let after = leadership_after_removals(before, &new_roster);
    Removal::Survived {
        session: new_roster.with_leadership(after),
        transfer: transfer_event(before, after),
    }
}

/// Leadership after member(s) left the roster: the holder keeps it while still
/// present (sticky), otherwise it re-derives to the earliest `Active` member — or
/// `Vacant` when none remains.
fn leadership_after_removals(leadership: Leadership, new_roster: &PartySession) -> Leadership {
    match leadership {
        Leadership::Led { by } if new_roster.member(by).is_some() => Leadership::Led { by },
        Leadership::Led { .. } | Leadership::Vacant => earliest(new_roster),
    }
}

/// Leadership after a seat went `Held`: the leader keeps it only while still
/// `Active`; a leader that just went `Held` hands off to the earliest remaining
/// `Active` member (or `Vacant`).
fn leadership_after_hold(leadership: Leadership, held_roster: &PartySession) -> Leadership {
    match leadership {
        Leadership::Led { by } if held_roster.is_active(by) => Leadership::Led { by },
        Leadership::Led { .. } | Leadership::Vacant => earliest(held_roster),
    }
}

/// Leadership after `slot` reconnected to `Active`: `Vacant` yields it to the
/// first returner; an established leader keeps it (sticky — a lower-slot reconnect
/// never steals).
fn leadership_after_reconnect(leadership: Leadership, slot: MemberSlot) -> Leadership {
    match leadership {
        Leadership::Vacant => Leadership::Led { by: slot },
        Leadership::Led { by } => Leadership::Led { by },
    }
}

/// The earliest-slot `Active` member as a leadership, or `Vacant` when none is.
fn earliest(roster: &PartySession) -> Leadership {
    match roster.earliest_active_slot() {
        Some(slot) => Leadership::Led { by: slot },
        None => Leadership::Vacant,
    }
}

/// A [`PartyEvent::LeadershipTransferred`] iff leadership moved to a new leader.
fn transfer_event(before: Leadership, after: Leadership) -> Option<PartyEvent> {
    match after {
        Leadership::Led { by } if before != after => {
            Some(PartyEvent::LeadershipTransferred { to: by })
        }
        Leadership::Led { .. } | Leadership::Vacant => None,
    }
}

/// The shared four-term share predicate: present (`Active` in the session), alive,
/// on the anchor's map, and within [`party_reach`] of the anchor.
fn qualifies(party: &PartySession, anchor: &MemberFact, fact: &MemberFact) -> bool {
    party.is_active(fact.slot)
        && matches!(fact.vitality, Vitality::Alive)
        && reach(anchor.position, anchor.map, fact.position, fact.map)
}

/// The classic `1.05^(n-1)` grouping bonus as an exact rational `(num, den)` for a
/// qualifier count `n`, pinned over the bounded domain (cap 5). The trailing `_`
/// is the `n >= 5` count cap — a match on a primitive count, not a domain-enum
/// wildcard.
fn party_bonus(n: u64) -> (u64, u64) {
    match n {
        0 | 1 => (100, 100),
        2 => (105, 100),
        3 => (11_025, 10_000),
        4 => (1_157_625, 1_000_000),
        _ => (121_550_625, 100_000_000),
    }
}

/// A count widened to `u64`, saturating at the boundary — the qualifier count is
/// at most `CAP`, far below the ceiling; the saturating narrow keeps the
/// conversion total without an `unwrap`.
fn saturating_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// An invite refusal with no events.
fn rejected(reason: InviteRejection) -> (InviteOutcome, Vec<PartyEvent>) {
    (InviteOutcome::Rejected { reason }, Vec::new())
}

/// An accept bounce mirrored to both sides.
fn bounced(reason: AcceptBounce) -> (AcceptOutcome, Vec<PartyEvent>) {
    (AcceptOutcome::Bounced { reason }, Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::spatial::WorldPos;
    use crate::components::tile::TileCoord;

    fn pos(x: u8, y: u8) -> WorldPos {
        TileCoord::new(x, y).to_world()
    }

    fn map0() -> MapNumber {
        MapNumber(0)
    }

    fn tick50() -> TickDuration {
        TickDuration::new(50).unwrap()
    }

    fn active(slot: u8) -> PartyMember {
        PartyMember {
            slot: MemberSlot(slot),
            membership: Membership::Active,
        }
    }

    /// A three-active-member party led by slot 0.
    fn trio() -> PartySession {
        PartySession::forming().with_member(active(2))
    }

    fn slot(n: u8) -> MemberSlot {
        MemberSlot(n)
    }

    // --- Invite gates. -------------------------------------------------------

    #[test]
    fn a_solo_inviter_reaching_an_available_target_sends_the_invite() {
        let (outcome, events) = invite(
            pos(0, 0),
            map0(),
            PartyAvailability::Available {
                position: pos(12, 0),
                map: map0(),
            },
            None,
            Tick(600),
            tick50(),
        );
        let InviteOutcome::Sent { invite } = outcome else {
            panic!("expected Sent");
        };
        // 60s / 50ms = 1200 ticks after now=600.
        assert_eq!(invite.expires, Tick(600 + 1200));
        assert_eq!(
            events,
            vec![PartyEvent::InviteSent, PartyEvent::InviteReceived]
        );
    }

    #[test]
    fn the_reach_edge_is_inclusive_at_twelve_and_out_at_thirteen() {
        let sent = invite(
            pos(0, 0),
            map0(),
            PartyAvailability::Available {
                position: pos(12, 0),
                map: map0(),
            },
            None,
            Tick(0),
            tick50(),
        );
        assert!(matches!(sent.0, InviteOutcome::Sent { .. }));
        let out = invite(
            pos(0, 0),
            map0(),
            PartyAvailability::Available {
                position: pos(13, 0),
                map: map0(),
            },
            None,
            Tick(0),
            tick50(),
        );
        assert_eq!(
            out.0,
            InviteOutcome::Rejected {
                reason: InviteRejection::OutOfRange
            }
        );
    }

    #[test]
    fn a_cross_map_target_is_out_of_range() {
        let (outcome, _) = invite(
            pos(0, 0),
            map0(),
            PartyAvailability::Available {
                position: pos(1, 0),
                map: MapNumber(3),
            },
            None,
            Tick(0),
            tick50(),
        );
        assert_eq!(
            outcome,
            InviteOutcome::Rejected {
                reason: InviteRejection::OutOfRange
            }
        );
    }

    #[test]
    fn each_host_fact_maps_to_its_own_named_refusal() {
        for (target, reason) in [
            (PartyAvailability::SameActor, InviteRejection::SameActor),
            (
                PartyAvailability::AlreadyPartied,
                InviteRejection::AlreadyPartied,
            ),
            (
                PartyAvailability::PendingInvite,
                InviteRejection::PendingInvite,
            ),
            (PartyAvailability::TargetDead, InviteRejection::TargetDead),
        ] {
            let (outcome, events) = invite(pos(0, 0), map0(), target, None, Tick(0), tick50());
            assert_eq!(outcome, InviteOutcome::Rejected { reason });
            assert!(events.is_empty());
        }
    }

    #[test]
    fn a_non_leader_inviter_is_refused_and_full_before_the_target_facts() {
        let non_leader = Some(InviterParty {
            party: trio(),
            inviter_slot: slot(1),
        });
        // Even a same-actor target loses to the leader gate.
        let (outcome, _) = invite(
            pos(0, 0),
            map0(),
            PartyAvailability::SameActor,
            non_leader.as_ref(),
            Tick(0),
            tick50(),
        );
        assert_eq!(
            outcome,
            InviteOutcome::Rejected {
                reason: InviteRejection::NotLeader
            }
        );

        let full = PartySession::forming()
            .with_member(active(2))
            .with_member(active(3))
            .with_member(active(4));
        let leader_of_full = Some(InviterParty {
            party: full,
            inviter_slot: slot(0),
        });
        let (outcome, _) = invite(
            pos(0, 0),
            map0(),
            PartyAvailability::Available {
                position: pos(1, 0),
                map: map0(),
            },
            leader_of_full.as_ref(),
            Tick(0),
            tick50(),
        );
        assert_eq!(
            outcome,
            InviteOutcome::Rejected {
                reason: InviteRejection::PartyFull
            }
        );
    }

    // --- Accept / decline. ---------------------------------------------------

    #[test]
    fn accepting_a_solo_inviter_forms_a_new_party_with_the_inviter_leading() {
        let (outcome, events) = accept_invite(
            InviterPresence::Present {
                position: pos(0, 0),
                map: map0(),
            },
            None,
            pos(5, 0),
            map0(),
        );
        let AcceptOutcome::Joined { session } = outcome else {
            panic!("expected Joined");
        };
        assert_eq!(session.leadership(), Leadership::Led { by: slot(0) });
        assert!(session.is_active(slot(0)) && session.is_active(slot(1)));
        assert_eq!(events, vec![PartyEvent::Joined { slot: slot(1) }]);
    }

    #[test]
    fn accepting_an_existing_leaders_invite_appends_the_responder() {
        let context = Some(InviterParty {
            party: PartySession::forming(),
            inviter_slot: slot(0),
        });
        let (outcome, events) = accept_invite(
            InviterPresence::Present {
                position: pos(0, 0),
                map: map0(),
            },
            context,
            pos(1, 0),
            map0(),
        );
        let AcceptOutcome::Joined { session } = outcome else {
            panic!("expected Joined");
        };
        assert_eq!(session.len(), 3);
        assert_eq!(session.leadership(), Leadership::Led { by: slot(0) });
        assert_eq!(events, vec![PartyEvent::Joined { slot: slot(2) }]);
    }

    #[test]
    fn every_live_recheck_bounces_with_its_named_reason() {
        // Inviter gone.
        assert_eq!(
            accept_invite(InviterPresence::Gone, None, pos(0, 0), map0()).0,
            AcceptOutcome::Bounced {
                reason: AcceptBounce::InviterGone
            }
        );
        // Out of range.
        assert_eq!(
            accept_invite(
                InviterPresence::Present {
                    position: pos(0, 0),
                    map: map0()
                },
                None,
                pos(13, 0),
                map0(),
            )
            .0,
            AcceptOutcome::Bounced {
                reason: AcceptBounce::OutOfRange
            }
        );
        // Inviter no longer leads.
        let not_leader = Some(InviterParty {
            party: trio(),
            inviter_slot: slot(1),
        });
        assert_eq!(
            accept_invite(
                InviterPresence::Present {
                    position: pos(0, 0),
                    map: map0()
                },
                not_leader,
                pos(1, 0),
                map0(),
            )
            .0,
            AcceptOutcome::Bounced {
                reason: AcceptBounce::InviterNotLeader
            }
        );
        // Party filled meanwhile.
        let full = PartySession::forming()
            .with_member(active(2))
            .with_member(active(3))
            .with_member(active(4));
        let leader_of_full = Some(InviterParty {
            party: full,
            inviter_slot: slot(0),
        });
        assert_eq!(
            accept_invite(
                InviterPresence::Present {
                    position: pos(0, 0),
                    map: map0()
                },
                leader_of_full,
                pos(1, 0),
                map0(),
            )
            .0,
            AcceptOutcome::Bounced {
                reason: AcceptBounce::PartyFull
            }
        );
    }

    #[test]
    fn decline_notifies_both_sides() {
        assert_eq!(decline_invite(), vec![PartyEvent::InviteDeclined]);
    }

    // --- Kick / leave / disband totality. ------------------------------------

    #[test]
    fn kick_totality_names_every_refusal_and_never_panics() {
        // Non-leader kick.
        assert_eq!(kick(trio(), slot(1), slot(2)).0, KickOutcome::NotLeader);
        // A non-leader targeting the leader is still NotLeader (leader unkickable).
        assert_eq!(kick(trio(), slot(1), slot(0)).0, KickOutcome::NotLeader);
        // Leader naming its own slot.
        assert_eq!(
            kick(trio(), slot(0), slot(0)).0,
            KickOutcome::CannotKickSelf
        );
        // Absent slot.
        assert_eq!(kick(trio(), slot(0), slot(4)).0, KickOutcome::NoSuchMember);
        // A real kick shrinks the roster, leader unchanged.
        let (outcome, events) = kick(trio(), slot(0), slot(2));
        let KickOutcome::Kicked { session } = outcome else {
            panic!("expected Kicked");
        };
        assert_eq!(session.len(), 2);
        assert_eq!(session.leadership(), Leadership::Led { by: slot(0) });
        assert_eq!(events, vec![PartyEvent::MemberKicked { slot: slot(2) }]);
    }

    #[test]
    fn a_leader_leaving_a_trio_transfers_to_the_earliest_active_member() {
        let (outcome, events) = leave(trio(), slot(0));
        let LeaveOutcome::Left { session } = outcome else {
            panic!("expected Left");
        };
        assert_eq!(session.leadership(), Leadership::Led { by: slot(1) });
        assert_eq!(
            events,
            vec![
                PartyEvent::MemberLeft { slot: slot(0) },
                PartyEvent::LeadershipTransferred { to: slot(1) },
            ]
        );
    }

    #[test]
    fn a_removal_dropping_below_two_disbands_and_notifies_everyone() {
        let (outcome, events) = leave(PartySession::forming(), slot(1));
        assert_eq!(outcome, LeaveOutcome::Disbanded);
        assert_eq!(
            events,
            vec![
                PartyEvent::MemberLeft { slot: slot(1) },
                PartyEvent::Disbanded
            ]
        );
        let (kout, kevents) = kick(PartySession::forming(), slot(0), slot(1));
        assert_eq!(kout, KickOutcome::Disbanded);
        assert_eq!(
            kevents,
            vec![
                PartyEvent::MemberKicked { slot: slot(1) },
                PartyEvent::Disbanded
            ]
        );
    }

    #[test]
    fn leave_over_an_absent_slot_is_no_such_member() {
        assert_eq!(leave(trio(), slot(3)).0, LeaveOutcome::NoSuchMember);
    }

    // --- Disconnect / reconnect / advance. -----------------------------------

    #[test]
    fn disconnect_holds_the_slot_with_its_expiry_and_keeps_the_count() {
        let (outcome, events) = disconnect(trio(), slot(2), Tick(1000), tick50());
        let DisconnectOutcome::Disconnected { session } = outcome else {
            panic!("expected Disconnected");
        };
        // 5 min / 50 ms = 6000 ticks.
        assert_eq!(
            session.member(slot(2)).unwrap().membership,
            Membership::Held {
                expires: Tick(1000 + 6000)
            }
        );
        assert_eq!(session.len(), 3);
        assert_eq!(session.leadership(), Leadership::Led { by: slot(0) });
        assert_eq!(events, vec![PartyEvent::MemberHeld { slot: slot(2) }]);
    }

    #[test]
    fn a_leader_disconnect_moves_leadership_and_a_reconnecting_ex_leader_does_not_reclaim() {
        let (outcome, events) = disconnect(trio(), slot(0), Tick(0), tick50());
        let DisconnectOutcome::Disconnected { session } = outcome else {
            panic!("expected Disconnected");
        };
        assert_eq!(session.leadership(), Leadership::Led { by: slot(1) });
        assert!(events.contains(&PartyEvent::LeadershipTransferred { to: slot(1) }));
        // Reconnect: back as a regular member, leadership sticky at slot 1.
        let (rout, revents) = reconnect(session, slot(0));
        let ReconnectOutcome::Reconnected { session } = rout else {
            panic!("expected Reconnected");
        };
        assert!(session.is_active(slot(0)));
        assert_eq!(session.leadership(), Leadership::Led { by: slot(1) });
        assert_eq!(
            revents,
            vec![PartyEvent::MemberReconnected { slot: slot(0) }]
        );
    }

    #[test]
    fn from_vacant_the_first_reconnect_takes_leadership() {
        let all_held = trio()
            .with_membership(slot(0), Membership::Held { expires: Tick(1) })
            .with_membership(slot(1), Membership::Held { expires: Tick(1) })
            .with_membership(slot(2), Membership::Held { expires: Tick(1) })
            .with_leadership(Leadership::Vacant);
        let (outcome, events) = reconnect(all_held, slot(2));
        let ReconnectOutcome::Reconnected { session } = outcome else {
            panic!("expected Reconnected");
        };
        assert_eq!(session.leadership(), Leadership::Led { by: slot(2) });
        assert!(events.contains(&PartyEvent::LeadershipTransferred { to: slot(2) }));
    }

    #[test]
    fn advance_party_reaps_an_expired_held_seat_like_a_leave() {
        // Slot 0 held with an expiry in the past; slots 1,2 active; leadership
        // already moved to slot 1 by the earlier disconnect.
        let session = trio()
            .with_membership(slot(0), Membership::Held { expires: Tick(900) })
            .with_leadership(Leadership::Led { by: slot(1) });
        let (outcome, events) = advance_party(session, Tick(1000));
        let PartyOutcome::Continues { session } = outcome else {
            panic!("expected Continues");
        };
        assert_eq!(session.len(), 2);
        assert!(session.member(slot(0)).is_none());
        assert_eq!(session.leadership(), Leadership::Led { by: slot(1) });
        assert_eq!(events, vec![PartyEvent::MemberExpired { slot: slot(0) }]);
    }

    #[test]
    fn advance_party_with_nothing_expired_continues_unchanged() {
        let session = trio();
        let (outcome, events) = advance_party(session.clone(), Tick(1000));
        assert_eq!(outcome, PartyOutcome::Continues { session });
        assert!(events.is_empty());
    }

    #[test]
    fn advance_party_reaping_below_two_disbands() {
        let session = PartySession::forming()
            .with_membership(slot(1), Membership::Held { expires: Tick(1) })
            .with_leadership(Leadership::Led { by: slot(0) });
        let (outcome, events) = advance_party(session, Tick(1000));
        assert_eq!(outcome, PartyOutcome::Disbanded);
        assert_eq!(
            events,
            vec![
                PartyEvent::MemberExpired { slot: slot(1) },
                PartyEvent::Disbanded
            ]
        );
    }

    #[test]
    fn advance_invite_reaps_past_expiry_and_holds_a_live_one() {
        assert_eq!(
            advance_invite(PartyInvite { expires: Tick(500) }, Tick(1000)),
            (InviteSweep::Lapsed, vec![PartyEvent::InviteExpired])
        );
        let live = PartyInvite {
            expires: Tick(2000),
        };
        assert_eq!(
            advance_invite(live, Tick(1000)),
            (InviteSweep::Pending { invite: live }, Vec::new())
        );
    }

    // --- Experience split. ---------------------------------------------------

    /// Levels 10/20/30 at slots 0/1/2 with current experience `exp`.
    fn facts_10_20_30(exp: [u64; 3]) -> Vec<MemberFact> {
        [10u16, 20, 30]
            .into_iter()
            .enumerate()
            .map(|(index, level)| MemberFact {
                slot: MemberSlot(u8::try_from(index).unwrap()),
                level: Level::new(level).unwrap(),
                experience: Exp(exp[index]),
                vitality: Vitality::Alive,
                map: map0(),
                position: pos(0, 0),
            })
            .collect()
    }

    /// A stand-in atlas is unavailable inline; the numeric split example is proven
    /// against the real Atlas in `core/tests/party_experience.rs`. Here the pure
    /// helpers are proven — the bonus table and the qualification predicate.
    #[test]
    fn party_bonus_is_the_exact_rational_table() {
        assert_eq!(party_bonus(0), (100, 100));
        assert_eq!(party_bonus(1), (100, 100));
        assert_eq!(party_bonus(2), (105, 100));
        assert_eq!(party_bonus(3), (11_025, 10_000));
        assert_eq!(party_bonus(4), (1_157_625, 1_000_000));
        assert_eq!(party_bonus(5), (121_550_625, 100_000_000));
        // n >= 5 caps at the 5-member rational.
        assert_eq!(party_bonus(9), (121_550_625, 100_000_000));
    }

    #[test]
    fn the_share_predicate_excludes_held_dead_offmap_and_out_of_range() {
        let party = trio();
        let facts = facts_10_20_30([0, 0, 0]);
        let anchor = facts.first().unwrap();
        // Slot 2 is present, alive, same-map, in-range -> qualifies.
        assert!(qualifies(&party, anchor, facts.get(2).unwrap()));
        // Held excludes.
        let held = party
            .clone()
            .with_membership(slot(2), Membership::Held { expires: Tick(1) });
        assert!(!qualifies(&held, anchor, facts.get(2).unwrap()));
        // Dead excludes.
        let mut dead = *facts.get(2).unwrap();
        dead.vitality = Vitality::Dead;
        assert!(!qualifies(&party, anchor, &dead));
        // Off-map excludes.
        let mut offmap = *facts.get(2).unwrap();
        offmap.map = MapNumber(7);
        assert!(!qualifies(&party, anchor, &offmap));
        // Out of range (13 tiles) excludes; 12 tiles includes.
        let mut far = *facts.get(2).unwrap();
        far.position = pos(13, 0);
        assert!(!qualifies(&party, anchor, &far));
        let mut edge = *facts.get(2).unwrap();
        edge.position = pos(12, 0);
        assert!(qualifies(&party, anchor, &edge));
    }

    // --- Zen split. ----------------------------------------------------------

    fn wallets_for(facts: &[MemberFact], balances: &[u64]) -> Vec<SlotWallet> {
        facts
            .iter()
            .zip(balances)
            .map(|(fact, balance)| SlotWallet {
                slot: fact.slot,
                wallet: CarriedZen::new(*balance).unwrap(),
            })
            .collect()
    }

    fn pile(amount: u64) -> WorldZen {
        WorldZen {
            amount: Zen(amount),
            position: pos(0, 0),
            map: map0(),
            despawn: Tick(9999),
        }
    }

    #[test]
    fn zen_splits_equally_with_the_remainder_to_the_picker() {
        let party = trio();
        let facts = facts_10_20_30([0, 0, 0]);
        let wallets = wallets_for(&facts, &[0, 0, 0]);
        let (picker, others) = facts.split_first().unwrap();
        let (picker_wallet, other_wallets) = wallets.split_first().unwrap();
        let result = split_zen_pickup(
            &pile(100_000),
            &party,
            *picker,
            picker_wallet.wallet,
            others,
            other_wallets,
        );
        assert!(result.to_ground.is_empty());
        // 33333 each, remainder 1 to picker slot 0.
        let credited: Vec<(u8, u64)> = result
            .credits
            .iter()
            .map(|c| (c.slot.0, c.wallet.get()))
            .collect();
        assert_eq!(credited, vec![(0, 33_334), (1, 33_333), (2, 33_333)]);
        let total: u64 = result.credits.iter().map(|c| c.wallet.get()).sum();
        assert_eq!(total, 100_000);
    }

    #[test]
    fn an_at_cap_members_share_grounds_and_conserves_the_pile() {
        let party = trio();
        let facts = facts_10_20_30([0, 0, 0]);
        // Slot 1 at 1_999_999_999: crediting 33333 over-caps.
        let wallets = wallets_for(&facts, &[0, 1_999_999_999, 0]);
        let (picker, others) = facts.split_first().unwrap();
        let (picker_wallet, other_wallets) = wallets.split_first().unwrap();
        let result = split_zen_pickup(
            &pile(100_000),
            &party,
            *picker,
            picker_wallet.wallet,
            others,
            other_wallets,
        );
        assert_eq!(result.to_ground.len(), 1);
        let grounded = result.to_ground.first().unwrap();
        assert_eq!(grounded.amount, Zen(33_333));
        assert_eq!(grounded.position, pos(0, 0));
        // Slot 1 got no credit; slots 0 and 2 did.
        let slots: Vec<u8> = result.credits.iter().map(|c| c.slot.0).collect();
        assert_eq!(slots, vec![0, 2]);
        let credited: u64 = result.credits.iter().map(|c| c.wallet.get()).sum();
        // 33334 (picker) + 33333 (slot 2) + 33333 (grounded) = 100000.
        assert_eq!(credited + grounded.amount.0, 100_000);
    }

    #[test]
    fn a_dead_member_is_dropped_from_the_zen_divisor() {
        let party = trio();
        let mut facts = facts_10_20_30([0, 0, 0]);
        facts.get_mut(2).unwrap().vitality = Vitality::Dead;
        let wallets = wallets_for(&facts, &[0, 0, 0]);
        let (picker, others) = facts.split_first().unwrap();
        let (picker_wallet, other_wallets) = wallets.split_first().unwrap();
        let result = split_zen_pickup(
            &pile(90_000),
            &party,
            *picker,
            picker_wallet.wallet,
            others,
            other_wallets,
        );
        // Divisor 2: 45000 each.
        let credited: Vec<(u8, u64)> = result
            .credits
            .iter()
            .map(|c| (c.slot.0, c.wallet.get()))
            .collect();
        assert_eq!(credited, vec![(0, 45_000), (1, 45_000)]);
        assert!(result.to_ground.is_empty());
    }

    #[test]
    fn a_deterministic_split_uses_no_rng_and_repeats() {
        // split_zen_pickup takes no RngCore by signature; two calls agree.
        let party = trio();
        let facts = facts_10_20_30([0, 0, 0]);
        let wallets = wallets_for(&facts, &[0, 0, 0]);
        let (picker, others) = facts.split_first().unwrap();
        let (picker_wallet, other_wallets) = wallets.split_first().unwrap();
        let a = split_zen_pickup(
            &pile(100_000),
            &party,
            *picker,
            picker_wallet.wallet,
            others,
            other_wallets,
        );
        let b = split_zen_pickup(
            &pile(100_000),
            &party,
            *picker,
            picker_wallet.wallet,
            others,
            other_wallets,
        );
        assert_eq!(a, b);
    }
}
