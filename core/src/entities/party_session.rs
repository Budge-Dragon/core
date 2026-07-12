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
///
/// Private fields: construction (serde or otherwise) proves the roster/leadership
/// cross-field invariants via [`TryFrom<RawPartySession>`], the [`Character`] and
/// [`Inventory`] precedent — a forged persisted row cannot name itself leader,
/// so a held [`PartySession`] is always internally consistent.
///
/// [`Character`]: crate::entities::character::Character
/// [`Inventory`]: crate::components::inventory::Inventory
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawPartySession", into = "RawPartySession")]
pub struct PartySession {
    members: Vec<PartyMember>,
    leadership: Leadership,
}

/// Wire mirror of [`PartySession`]. The roster/leadership invariants re-prove on
/// the way in, since a persisted party loaded from a host is untrusted: a forged
/// `Led { by }` naming the forger's own seat would otherwise make them the
/// authoritative leader (the kick/invite gate is `leadership() == Led { by:
/// actor }`), a privilege escalation.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct RawPartySession {
    members: Vec<PartyMember>,
    leadership: Leadership,
}

impl TryFrom<RawPartySession> for PartySession {
    type Error = PartySessionError;

    fn try_from(raw: RawPartySession) -> Result<Self, Self::Error> {
        let RawPartySession {
            members,
            leadership,
        } = raw;
        if members.len() < PartySession::MIN_MEMBERS {
            return Err(PartySessionError::TooFewMembers {
                found: members.len(),
            });
        }
        let mut previous: Option<MemberSlot> = None;
        for member in &members {
            if member.slot.0 >= MemberSlot::CAP {
                return Err(PartySessionError::SlotOutOfRange { slot: member.slot });
            }
            if let Some(prev) = previous {
                if member.slot <= prev {
                    return Err(PartySessionError::SlotsNotAscending);
                }
            }
            previous = Some(member.slot);
        }
        match leadership {
            Leadership::Led { by } => {
                let leads = members.iter().any(|member| {
                    member.slot == by && matches!(member.membership, Membership::Active)
                });
                if !leads {
                    return Err(PartySessionError::LeaderNotActiveMember { slot: by });
                }
            }
            Leadership::Vacant => {
                let any_active = members
                    .iter()
                    .any(|member| matches!(member.membership, Membership::Active));
                if any_active {
                    return Err(PartySessionError::VacantWithActiveMember);
                }
            }
        }
        Ok(Self {
            members,
            leadership,
        })
    }
}

impl From<PartySession> for RawPartySession {
    fn from(session: PartySession) -> Self {
        Self {
            members: session.members,
            leadership: session.leadership,
        }
    }
}

/// Rejection of a party record that contradicts a roster or leadership
/// invariant, at construction or the data-load boundary — the [`CharacterError`]
/// sibling for the party aggregate.
///
/// [`CharacterError`]: crate::entities::character::CharacterError
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartySessionError {
    /// The roster held fewer than [`PartySession::MIN_MEMBERS`] members — a live
    /// party never persists below the disband threshold.
    TooFewMembers {
        /// The number of members found.
        found: usize,
    },
    /// A member's slot is outside the `0..CAP` seat range.
    SlotOutOfRange {
        /// The offending slot.
        slot: MemberSlot,
    },
    /// The roster is not strictly ascending by slot — an out-of-order or
    /// duplicate seat.
    SlotsNotAscending,
    /// `Led { by }` names a slot that is absent or not `Active` — the
    /// privilege-escalation shape a forged row would use.
    LeaderNotActiveMember {
        /// The slot the leadership named.
        slot: MemberSlot,
    },
    /// `Vacant` leadership while a member is `Active` — vacancy is the real
    /// all-`Held` state; an `Active` member must lead.
    VacantWithActiveMember,
}

impl core::fmt::Display for PartySessionError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooFewMembers { found } => {
                write!(
                    f,
                    "a party needs at least {} members, found {found}",
                    PartySession::MIN_MEMBERS
                )
            }
            Self::SlotOutOfRange { slot } => {
                write!(
                    f,
                    "member slot {} is outside 0..{}",
                    slot.0,
                    MemberSlot::CAP
                )
            }
            Self::SlotsNotAscending => {
                write!(f, "the roster is not strictly ascending by slot")
            }
            Self::LeaderNotActiveMember { slot } => {
                write!(
                    f,
                    "leadership names slot {}, which is absent or not active",
                    slot.0
                )
            }
            Self::VacantWithActiveMember => {
                write!(f, "leadership is vacant while a member is active")
            }
        }
    }
}

impl core::error::Error for PartySessionError {}

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
    fn the_wire_gate_rejects_a_forged_leader_naming_a_held_or_absent_slot() {
        // Led { by } must name an Active member — the kick/invite gate is
        // `leadership() == Led { by: actor }`, so a forged leader is a
        // privilege escalation.
        let held_leader = RawPartySession {
            members: vec![held(0, 9), active(1)],
            leadership: Leadership::Led { by: MemberSlot(0) },
        };
        assert_eq!(
            PartySession::try_from(held_leader),
            Err(PartySessionError::LeaderNotActiveMember {
                slot: MemberSlot(0)
            })
        );
        let absent_leader = RawPartySession {
            members: vec![active(0), active(1)],
            leadership: Leadership::Led { by: MemberSlot(4) },
        };
        assert_eq!(
            PartySession::try_from(absent_leader),
            Err(PartySessionError::LeaderNotActiveMember {
                slot: MemberSlot(4)
            })
        );
        // The same forged bytes are rejected on the real deserialize path.
        let forged = concat!(
            r#"{"members":[{"slot":0,"membership":{"kind":"active"}},"#,
            r#"{"slot":1,"membership":{"kind":"active"}}],"#,
            r#""leadership":{"kind":"led","by":4}}"#,
        );
        assert!(serde_json::from_str::<PartySession>(forged).is_err());
    }

    #[test]
    fn the_wire_gate_rejects_a_non_ascending_duplicate_or_out_of_range_roster() {
        let descending = RawPartySession {
            members: vec![active(1), active(0)],
            leadership: Leadership::Led { by: MemberSlot(1) },
        };
        assert_eq!(
            PartySession::try_from(descending),
            Err(PartySessionError::SlotsNotAscending)
        );
        let duplicate = RawPartySession {
            members: vec![active(0), active(0)],
            leadership: Leadership::Led { by: MemberSlot(0) },
        };
        assert_eq!(
            PartySession::try_from(duplicate),
            Err(PartySessionError::SlotsNotAscending)
        );
        let out_of_range = RawPartySession {
            members: vec![active(0), active(MemberSlot::CAP)],
            leadership: Leadership::Led { by: MemberSlot(0) },
        };
        assert_eq!(
            PartySession::try_from(out_of_range),
            Err(PartySessionError::SlotOutOfRange {
                slot: MemberSlot(MemberSlot::CAP)
            })
        );
    }

    #[test]
    fn the_wire_gate_rejects_a_sub_threshold_or_empty_roster() {
        let solo = RawPartySession {
            members: vec![active(0)],
            leadership: Leadership::Led { by: MemberSlot(0) },
        };
        assert_eq!(
            PartySession::try_from(solo),
            Err(PartySessionError::TooFewMembers { found: 1 })
        );
        let empty = RawPartySession {
            members: Vec::new(),
            leadership: Leadership::Vacant,
        };
        assert_eq!(
            PartySession::try_from(empty),
            Err(PartySessionError::TooFewMembers { found: 0 })
        );
    }

    #[test]
    fn the_wire_gate_rejects_vacant_leadership_while_a_member_is_active() {
        let active_but_vacant = RawPartySession {
            members: vec![active(0), held(1, 9)],
            leadership: Leadership::Vacant,
        };
        assert_eq!(
            PartySession::try_from(active_but_vacant),
            Err(PartySessionError::VacantWithActiveMember)
        );
        // The legitimate all-held vacancy is accepted.
        let all_held = RawPartySession {
            members: vec![held(0, 9), held(1, 9)],
            leadership: Leadership::Vacant,
        };
        assert!(PartySession::try_from(all_held).is_ok());
    }

    #[test]
    fn a_valid_forming_pair_round_trips_through_the_gate() {
        let party = PartySession::forming().with_member(held(2, 1300));
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
