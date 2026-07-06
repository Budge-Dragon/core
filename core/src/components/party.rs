//! The positional identity and per-slot lifecycle of a party roster: a
//! [`MemberSlot`] names a seat inside one party (never a host account id), a
//! [`Membership`] carries a seat's connection lifecycle, a [`Leadership`] names
//! who leads (or that no one is online to), and a [`Vitality`] is the host's
//! two-state liveness fact at a share. Data and invariants only — every rule
//! (who leads after a removal, when a party disbands, who earns a share) lives
//! in [`crate::services::party`].

use serde::{Deserialize, Serialize};

use crate::components::units::Tick;

/// A member's positional identity inside one party — never a host account id.
/// Bounded `0..CAP`; the host owns the account↔slot map (the trade `Side` grain,
/// generalized to N). Lowest-free-slot reuse keeps it a bounded `0..CAP`
/// identity across leave and rejoin, so earliest-slot succession stays
/// well-defined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MemberSlot(
    /// The seat index, `0..CAP`.
    pub u8,
);

impl MemberSlot {
    /// The member-count cap (classic 5). Valid slots are `0..CAP`.
    pub const CAP: u8 = 5;
}

/// A member's connection lifecycle. A proper sum: `Held` carries its expiry only
/// on that variant, so a connected member with a stray expiry — or a held one
/// with none — is unrepresentable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Membership {
    /// The member is connected and administers/earns normally.
    Active,
    /// The member disconnected; the seat is reserved until `expires`.
    Held {
        /// The tick the reserved seat lapses at.
        expires: Tick,
    },
}

/// The party's leadership. Two variants make "leader is a non-member, offline,
/// or nonexistent" unrepresentable: `Led { by }` always names an `Active` member
/// (transition-maintained by [`crate::services::party`]), and `Vacant` is the
/// real all-`Held` state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Leadership {
    /// Led by an `Active` member at this slot.
    Led {
        /// The leader's slot — always an `Active` member.
        by: MemberSlot,
    },
    /// No member is `Active`; no one is online to administer. The first member
    /// to reconnect takes leadership.
    Vacant,
}

/// A member's liveness at a kill or pickup — the host-resolved share fact, a
/// two-state variant (the `TradeAvailability::Dead` grain), never `alive: bool`.
/// A dead member earns no share; the qualification rule lives in
/// [`crate::services::party`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Vitality {
    /// The member is alive and eligible for a share.
    Alive,
    /// The member is dead and earns no share.
    Dead,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_slot_is_a_bare_integer_positional_identity() {
        assert_eq!(serde_json::to_string(&MemberSlot(2)).unwrap(), "2");
        assert_eq!(
            serde_json::from_str::<MemberSlot>("2").unwrap(),
            MemberSlot(2)
        );
        // Two slots compare by position, never by a host id.
        assert!(MemberSlot(0) < MemberSlot(1));
        assert_eq!(MemberSlot::CAP, 5);
    }

    #[test]
    fn membership_round_trips_active_and_held_with_its_expiry() {
        assert_eq!(
            serde_json::to_string(&Membership::Active).unwrap(),
            r#"{"kind":"active"}"#
        );
        assert_eq!(
            serde_json::to_string(&Membership::Held {
                expires: Tick(1300)
            })
            .unwrap(),
            r#"{"kind":"held","expires":1300}"#
        );
        for membership in [
            Membership::Active,
            Membership::Held {
                expires: Tick(1300),
            },
        ] {
            let json = serde_json::to_string(&membership).unwrap();
            assert_eq!(
                serde_json::from_str::<Membership>(&json).unwrap(),
                membership
            );
        }
    }

    #[test]
    fn leadership_round_trips_led_and_vacant() {
        assert_eq!(
            serde_json::to_string(&Leadership::Led { by: MemberSlot(0) }).unwrap(),
            r#"{"kind":"led","by":0}"#
        );
        assert_eq!(
            serde_json::to_string(&Leadership::Vacant).unwrap(),
            r#"{"kind":"vacant"}"#
        );
        for leadership in [Leadership::Led { by: MemberSlot(3) }, Leadership::Vacant] {
            let json = serde_json::to_string(&leadership).unwrap();
            assert_eq!(
                serde_json::from_str::<Leadership>(&json).unwrap(),
                leadership
            );
        }
    }

    #[test]
    fn vitality_round_trips_alive_and_dead() {
        assert_eq!(
            serde_json::to_string(&Vitality::Alive).unwrap(),
            r#"{"kind":"alive"}"#
        );
        assert_eq!(
            serde_json::to_string(&Vitality::Dead).unwrap(),
            r#"{"kind":"dead"}"#
        );
        for vitality in [Vitality::Alive, Vitality::Dead] {
            let json = serde_json::to_string(&vitality).unwrap();
            assert_eq!(serde_json::from_str::<Vitality>(&json).unwrap(), vitality);
        }
    }
}
