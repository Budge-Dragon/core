//! The component-only outcome values of the party services: one qualifier's
//! kill award ([`MemberAward`]), the named invite/accept refusals
//! ([`InviteRejection`]/[`AcceptBounce`]), and the returned domain events
//! ([`PartyEvent`]). Only outcomes that carry no entity live here — the
//! session-carrying and pile-carrying outcomes (`InviteOutcome`, `AcceptOutcome`,
//! `ZenSplitResult`, …) ride in [`crate::services::party`], since an event never
//! imports an entity. Every refusal is named — no silent drop.

use serde::{Deserialize, Serialize};

use crate::components::party::MemberSlot;
use crate::components::units::Exp;
use crate::events::progression::LevelUp;

/// One qualifier's kill award — the fan-out of the solo `(gained, level_ups)`.
/// Carries no entity (only [`MemberSlot`]/[`Exp`]/[`LevelUp`]), so it lives here.
/// The host applies each via the existing growth seam, N-fanned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemberAward {
    /// The awarded member's slot.
    pub slot: MemberSlot,
    /// The experience this member gained (the killer's carries the split
    /// remainder).
    pub gained: Exp,
    /// The levels this member crossed, ascending, from its own new total.
    pub level_ups: Vec<LevelUp>,
}

/// Every named invite refusal — bare-string `snake_case` (the
/// [`crate::events::trade::RequestRejection`] grain). One distinct reason per
/// host-resolved target fact and per party gate; no silent drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InviteRejection {
    /// The target resolved to the inviter's own character.
    SameActor,
    /// The target already holds a party.
    AlreadyPartied,
    /// The target already holds an outstanding invite.
    PendingInvite,
    /// The target is not alive.
    TargetDead,
    /// The target stands off-map or outside the invite reach.
    OutOfRange,
    /// The inviter is a member but not the leader of its party.
    NotLeader,
    /// The inviter's party is already full.
    PartyFull,
}

/// Every named accept bounce — the four live re-checks at accept time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptBounce {
    /// The inviter is dead, disconnected, off-map, or otherwise unreachable.
    InviterGone,
    /// The inviter and responder now stand out of reach.
    OutOfRange,
    /// The inviter lost leadership between invite and accept.
    InviterNotLeader,
    /// The inviter's party filled meanwhile.
    PartyFull,
}

/// The party's returned domain events — values, never side effects. Each names
/// the [`MemberSlot`] it concerns; the host routes delivery and owns the id map.
/// Past-tense, minimum payload, no envelope or wire version (host concerns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PartyEvent {
    /// To the inviter: the invite went out.
    InviteSent,
    /// To the target: an invite arrived.
    InviteReceived,
    /// To both: the invite was declined.
    InviteDeclined,
    /// To both: the invite lapsed at its expiry.
    InviteExpired,
    /// To all: a member joined at this slot.
    Joined {
        /// The joiner's slot.
        slot: MemberSlot,
    },
    /// To all and the removed member: a member was kicked.
    MemberKicked {
        /// The kicked member's slot.
        slot: MemberSlot,
    },
    /// To all and the leaver: a member left.
    MemberLeft {
        /// The leaver's slot.
        slot: MemberSlot,
    },
    /// To all: a member disconnected and its seat is held.
    MemberHeld {
        /// The disconnected member's slot.
        slot: MemberSlot,
    },
    /// To all: a held member reconnected.
    MemberReconnected {
        /// The reconnected member's slot.
        slot: MemberSlot,
    },
    /// To all: leadership moved to this slot.
    LeadershipTransferred {
        /// The new leader's slot.
        to: MemberSlot,
    },
    /// To all: a held seat lapsed and was reaped.
    MemberExpired {
        /// The lapsed member's slot.
        slot: MemberSlot,
    },
    /// To all, including the departing member: the party disbanded.
    Disbanded,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::units::Level;

    #[test]
    fn member_award_wire_is_pinned() {
        let award = MemberAward {
            slot: MemberSlot(2),
            gained: Exp(1137),
            level_ups: vec![LevelUp {
                level: Level::new(31).unwrap(),
            }],
        };
        assert_eq!(
            serde_json::to_string(&award).unwrap(),
            r#"{"slot":2,"gained":1137,"level_ups":[{"level":31}]}"#
        );
        let json = serde_json::to_string(&award).unwrap();
        assert_eq!(serde_json::from_str::<MemberAward>(&json).unwrap(), award);
    }

    #[test]
    fn invite_rejection_round_trips_every_reason_as_a_bare_string() {
        assert_eq!(
            serde_json::to_string(&InviteRejection::PartyFull).unwrap(),
            r#""party_full""#
        );
        for reason in [
            InviteRejection::SameActor,
            InviteRejection::AlreadyPartied,
            InviteRejection::PendingInvite,
            InviteRejection::TargetDead,
            InviteRejection::OutOfRange,
            InviteRejection::NotLeader,
            InviteRejection::PartyFull,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(
                serde_json::from_str::<InviteRejection>(&json).unwrap(),
                reason
            );
        }
    }

    #[test]
    fn accept_bounce_round_trips_every_reason_as_a_bare_string() {
        for reason in [
            AcceptBounce::InviterGone,
            AcceptBounce::OutOfRange,
            AcceptBounce::InviterNotLeader,
            AcceptBounce::PartyFull,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(serde_json::from_str::<AcceptBounce>(&json).unwrap(), reason);
        }
    }

    #[test]
    fn party_event_round_trips_every_kind() {
        assert_eq!(
            serde_json::to_string(&PartyEvent::Joined {
                slot: MemberSlot(1)
            })
            .unwrap(),
            r#"{"kind":"joined","slot":1}"#
        );
        for event in [
            PartyEvent::InviteSent,
            PartyEvent::InviteReceived,
            PartyEvent::InviteDeclined,
            PartyEvent::InviteExpired,
            PartyEvent::Joined {
                slot: MemberSlot(1),
            },
            PartyEvent::MemberKicked {
                slot: MemberSlot(2),
            },
            PartyEvent::MemberLeft {
                slot: MemberSlot(0),
            },
            PartyEvent::MemberHeld {
                slot: MemberSlot(3),
            },
            PartyEvent::MemberReconnected {
                slot: MemberSlot(3),
            },
            PartyEvent::LeadershipTransferred { to: MemberSlot(1) },
            PartyEvent::MemberExpired {
                slot: MemberSlot(4),
            },
            PartyEvent::Disbanded,
        ] {
            let json = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<PartyEvent>(&json).unwrap(), event);
        }
    }
}
