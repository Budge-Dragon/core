//! A live party — core's N-member session aggregate (the two-party
//! [`crate::entities::trade_session::TradeSession`] generalized). Plain serde
//! data, host-persisted between intents (the host owns the session registry;
//! core never holds it). A struct, not a phase enum: a party has one shape, and
//! disband is a terminal *outcome* ([`crate::services::party`] returns
//! `*::Disbanded`), never a resting variant. All behavior — the sticky-leadership
//! rule, the disband check, the qualification predicate — lives in
//! [`crate::services::party`]; this module exposes only total-structure
//! accessors and value-in/value-out data operations.

use serde::{Deserialize, Serialize};

use crate::components::party::{Leadership, MemberSlot, Membership};
use crate::components::units::Tick;

/// One slot's stored lifecycle data. `slot` is explicit so a member is
/// self-describing on the wire and after lowest-free-slot churn leaves the
/// roster non-contiguous.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyMember {
    /// The member's positional identity.
    pub slot: MemberSlot,
    /// The member's connection lifecycle.
    pub membership: Membership,
}

/// A live party: the roster (ascending by slot) plus its stored leadership. The
/// `len >= MIN_MEMBERS` invariant is transition-maintained — a removal that would
/// drop below it returns a terminal outcome, never a sub-threshold session — and
/// `leadership` is stored because the stickiness rule makes it path-dependent
/// (a purely-derived "earliest Active" would let a reconnecting ex-leader steal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartySession {
    members: Vec<PartyMember>,
    leadership: Leadership,
}

impl PartySession {
    /// Disband when the roster would hold fewer than this (classic §C.6).
    pub const MIN_MEMBERS: usize = 2;

    /// A freshly formed pair from an accepted solo-inviter invite: the inviter at
    /// [`MemberSlot(0)`](MemberSlot) (leader), the responder at
    /// [`MemberSlot(1)`](MemberSlot), both `Active`. A real domain value, not a
    /// fabricated default.
    #[must_use]
    pub fn forming() -> Self {
        Self {
            members: vec![
                PartyMember {
                    slot: MemberSlot(0),
                    membership: Membership::Active,
                },
                PartyMember {
                    slot: MemberSlot(1),
                    membership: Membership::Active,
                },
            ],
            leadership: Leadership::Led { by: MemberSlot(0) },
        }
    }

    /// The roster, ascending by slot.
    #[must_use]
    pub fn members(&self) -> &[PartyMember] {
        &self.members
    }

    /// The stored leadership.
    #[must_use]
    pub fn leadership(&self) -> Leadership {
        self.leadership
    }

    /// How many members the roster holds (counting `Held` seats).
    #[must_use]
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Whether the roster is empty — a transient state a service never returns
    /// (disband is a terminal below [`Self::MIN_MEMBERS`]). Present so `len` has
    /// its idiomatic twin.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }

    /// The member at `slot`, or `None` when no seat holds it.
    #[must_use]
    pub fn member(&self, slot: MemberSlot) -> Option<&PartyMember> {
        self.members.iter().find(|member| member.slot == slot)
    }

    /// Whether `slot` names a currently-`Active` member.
    #[must_use]
    pub fn is_active(&self, slot: MemberSlot) -> bool {
        matches!(
            self.member(slot),
            Some(PartyMember {
                membership: Membership::Active,
                ..
            })
        )
    }

    /// The earliest-slot `Active` member — the deterministic succession pick — or
    /// `None` when every member is `Held`.
    #[must_use]
    pub fn earliest_active_slot(&self) -> Option<MemberSlot> {
        self.members
            .iter()
            .find(|member| matches!(member.membership, Membership::Active))
            .map(|member| member.slot)
    }

    /// The lowest `0..CAP` slot no member occupies, or `None` when the roster is
    /// full — the seat an accepted join takes.
    #[must_use]
    pub fn lowest_free_slot(&self) -> Option<MemberSlot> {
        (0..MemberSlot::CAP)
            .map(MemberSlot)
            .find(|slot| self.member(*slot).is_none())
    }

    /// This roster with `member` inserted in ascending slot order — the caller
    /// proves the slot free (via [`Self::lowest_free_slot`]).
    #[must_use]
    pub fn with_member(mut self, member: PartyMember) -> Self {
        let index = self
            .members
            .partition_point(|seated| seated.slot < member.slot);
        self.members.insert(index, member);
        self
    }

    /// This roster with `slot` removed. Leadership is left untouched — the
    /// service re-derives it.
    #[must_use]
    pub fn without_slot(mut self, slot: MemberSlot) -> Self {
        self.members.retain(|member| member.slot != slot);
        self
    }

    /// This roster with `slot`'s membership replaced. A no-op when `slot` names
    /// no member.
    #[must_use]
    pub fn with_membership(mut self, slot: MemberSlot, membership: Membership) -> Self {
        for member in &mut self.members {
            if member.slot == slot {
                member.membership = membership;
            }
        }
        self
    }

    /// This party with its stored leadership replaced.
    #[must_use]
    pub fn with_leadership(self, leadership: Leadership) -> Self {
        Self { leadership, ..self }
    }
}

/// A pending party invite — owned data the host persists (the trade
/// `Requested`-phase grain, taken further: an invite can exist before any
/// party). Core holds only `expires`; the host owns the inviter↔target pairing
/// and the dialog. No host id, no structurally-undefined slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyInvite {
    /// The tick the invite lapses at.
    pub expires: Tick,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active(slot: u8) -> PartyMember {
        PartyMember {
            slot: MemberSlot(slot),
            membership: Membership::Active,
        }
    }

    fn held(slot: u8, expires: u64) -> PartyMember {
        PartyMember {
            slot: MemberSlot(slot),
            membership: Membership::Held {
                expires: Tick(expires),
            },
        }
    }

    #[test]
    fn forming_seats_inviter_slot_zero_leader_and_responder_slot_one() {
        let party = PartySession::forming();
        assert_eq!(party.len(), 2);
        assert_eq!(party.leadership(), Leadership::Led { by: MemberSlot(0) });
        assert_eq!(party.members(), &[active(0), active(1)]);
    }

    #[test]
    fn with_member_appends_in_ascending_slot_order() {
        let party = PartySession::forming().with_member(active(2));
        assert_eq!(party.members(), &[active(0), active(1), active(2)]);
        // Even inserted out of order, the roster stays ascending by slot.
        let churned = PartySession::forming()
            .without_slot(MemberSlot(1))
            .with_member(active(3))
            .with_member(active(1));
        let slots: Vec<u8> = churned.members().iter().map(|m| m.slot.0).collect();
        assert_eq!(slots, vec![0, 1, 3]);
    }

    #[test]
    fn lowest_free_slot_is_none_only_when_full() {
        let five = PartySession::forming()
            .with_member(active(2))
            .with_member(active(3))
            .with_member(active(4));
        assert_eq!(five.len(), 5);
        assert_eq!(five.lowest_free_slot(), None);
        // A gap left by churn is the lowest free slot.
        let gapped = five.without_slot(MemberSlot(1));
        assert_eq!(gapped.lowest_free_slot(), Some(MemberSlot(1)));
    }

    #[test]
    fn earliest_active_slot_picks_earliest_active_and_is_none_when_all_held() {
        let party = PartySession::forming()
            .with_member(active(2))
            .with_membership(MemberSlot(0), Membership::Held { expires: Tick(1) });
        assert_eq!(party.earliest_active_slot(), Some(MemberSlot(1)));
        let all_held = party
            .with_membership(MemberSlot(1), Membership::Held { expires: Tick(1) })
            .with_membership(MemberSlot(2), Membership::Held { expires: Tick(1) });
        assert_eq!(all_held.earliest_active_slot(), None);
    }

    #[test]
    fn a_live_session_round_trips_mid_lifecycle_with_active_and_held_members() {
        let party = PartySession::forming().with_member(held(2, 1300));
        assert_eq!(
            serde_json::to_string(&party).unwrap(),
            concat!(
                r#"{"members":[{"slot":0,"membership":{"kind":"active"}},"#,
                r#"{"slot":1,"membership":{"kind":"active"}},"#,
                r#"{"slot":2,"membership":{"kind":"held","expires":1300}}],"#,
                r#""leadership":{"kind":"led","by":0}}"#,
            )
        );
        let json = serde_json::to_string(&party).unwrap();
        assert_eq!(serde_json::from_str::<PartySession>(&json).unwrap(), party);
    }

    #[test]
    fn party_invite_carries_only_its_expiry() {
        let invite = PartyInvite { expires: Tick(660) };
        assert_eq!(
            serde_json::to_string(&invite).unwrap(),
            r#"{"expires":660}"#
        );
        assert_eq!(
            serde_json::from_str::<PartyInvite>(r#"{"expires":660}"#).unwrap(),
            invite
        );
    }
}
