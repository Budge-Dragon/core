//! The outcome and event values of the player↔player trade services. One
//! kind-tagged outcome enum per decision — the [`crate::events::shop`]
//! peer-enum grain, never an umbrella sum. Only component-carrying outcomes
//! live here; the outcomes that hand back a whole session entity ride in
//! [`crate::services::trade`], since an event never imports an entity. Every
//! refusal is named — no silent drop, no clamp, and no value ever destroyed:
//! what cannot return to its owner rides an explicit [`Overflow`].

use serde::{Deserialize, Serialize};

use crate::components::inventory::Cell;
use crate::components::inventory::Inventory;
use crate::components::item_instance::ItemInstance;
use crate::components::trade_window::Side;
use crate::components::units::{CarriedZen, Zen};

/// Every named request refusal — bare-string `snake_case` (the
/// [`crate::events::inventory::EquipRejection`] grain). Self, busy, and dead
/// are first-class named refusals, never silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequestRejection {
    /// The target resolved to the requester's own character.
    SelfTrade,
    /// The target already holds a trade session.
    PartnerBusy,
    /// The target is not alive.
    PartnerDead,
    /// The target stands outside the trade reach.
    OutOfRange,
}

/// What an item offer produced, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OfferOutcome {
    /// The item moved from the bag into the acting side's window at `at`.
    Offered {
        /// The window anchor the item was placed at.
        at: Cell,
    },
    /// The session is still in the requested phase.
    NotOpen,
    /// The acting side is locked — it must unlock before editing.
    SideLocked,
    /// The addressed inventory cell holds nothing — a double offer lands here,
    /// because escrow-by-move left the source cell empty.
    NoItemAtSource,
    /// The destination overlaps an item already placed in the window.
    WindowCellsOccupied,
    /// The footprint would leave the 4×8 window.
    WindowOutOfBounds,
}

/// What an item withdrawal produced, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WithdrawOutcome {
    /// The item moved from the window back into the bag.
    Withdrawn {
        /// The window cell the item was withdrawn from.
        at: Cell,
    },
    /// The session is still in the requested phase.
    NotOpen,
    /// The acting side is locked — it must unlock before editing.
    SideLocked,
    /// The addressed window cell holds nothing.
    NoItemAtWindowCell,
    /// The bag has no fitting region — the item stays escrowed, never lost.
    InventoryFull,
}

/// What a zen offer produced, kind-tagged. Delta semantics: the wallet moves
/// by only the change — a raise debits the difference, a lower credits it
/// back, and re-setting the same amount is a net-zero success with no wallet
/// operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ZenOfferOutcome {
    /// The offer now stands at `escrowed`; the wallet moved by the delta only.
    Offered {
        /// The full escrowed amount now recorded against the acting side.
        escrowed: Zen,
        /// The balance after the delta moved.
        wallet: CarriedZen,
    },
    /// The session is still in the requested phase.
    NotOpen,
    /// The acting side is locked — it must unlock before editing.
    SideLocked,
    /// The raise's delta exceeds the balance; prior offer and wallet
    /// preserved.
    Unaffordable {
        /// The unchanged balance.
        wallet: CarriedZen,
    },
    /// The lower's credit-back would exceed the carry cap; offer and wallet
    /// unchanged — value is never destroyed.
    WalletFull,
}

/// What an intra-window rearrange produced, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RearrangeOutcome {
    /// The item re-anchored within its own window; no lock was disturbed.
    Rearranged {
        /// The cell the item moved from.
        from: Cell,
        /// The anchor the item now sits at.
        to: Cell,
    },
    /// The session is still in the requested phase.
    NotOpen,
    /// The acting side is locked — it must unlock before editing.
    SideLocked,
    /// The addressed window cell holds nothing.
    NoItemAtWindowCell,
    /// The destination overlaps another placed window item.
    WindowCellsOccupied,
    /// The moved footprint would leave the 4×8 window.
    WindowOutOfBounds,
}

/// What an unlock produced, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UnlockOutcome {
    /// The acting side's lock cleared.
    Unlocked,
    /// The session is still in the requested phase.
    NotOpen,
    /// The acting side held no lock — a named idempotent no-op.
    AlreadyUnlocked,
}

/// Why a trade ended without completing. `Declined` folds a partner's refusal
/// of the request into the cancel path, so a decline reads as "declined,"
/// never a bare "cancelled."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    /// A player clicked cancel.
    Explicit,
    /// The partner declined the request.
    Declined,
    /// A player disconnected.
    Disconnected,
    /// A player died — death is never blocked by a trade.
    Died,
    /// The host ended the trade by policy.
    HostPolicy,
}

/// The total, value-conserving settlement of a cancelled trade. Each side's
/// escrow returns to its own current containers; what cannot land is explicit
/// [`Overflow`] the host ground-drops. Settlement never fails and never
/// destroys value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settlement {
    /// The requester's settled containers and overflow.
    pub requester: SettledSide,
    /// The partner's settled containers and overflow.
    pub partner: SettledSide,
}

/// One side of a settlement: the owner's containers with the escrow returned,
/// plus whatever could not land.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettledSide {
    /// The bag with the escrowed items re-placed by first-fit.
    pub inventory: Inventory,
    /// The wallet with the escrowed zen credited back, cap-checked.
    pub wallet: CarriedZen,
    /// What did not fit or could not credit.
    pub overflow: Overflow,
}

/// Escrow that could not return to its owner — carried as move-only values
/// for the host to ground-drop. Reachable only on the cancel path (a
/// mid-trade pickup can fill the hole an escrowed item left); the completion
/// terminal carries no overflow because its proof guarantees fit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Overflow {
    /// Escrowed items with no fitting region left in the owner's bag.
    pub items: Vec<ItemInstance>,
    /// Escrowed zen the wallet had no headroom for; zero means none — a real
    /// value, not an `Option`.
    pub zen: Zen,
}

impl Overflow {
    /// No overflow: no items, zero zen.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            items: Vec::new(),
            zen: Zen(0),
        }
    }
}

/// A completion-proof failure on one side — at least one axis failed. A side
/// that passed cleanly is never named, so "named with no failure" is
/// unrepresentable, and one side can honestly report both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideFailure {
    /// The incoming items found no region in the receiver's bag.
    ItemsDoNotFit,
    /// The incoming zen would push the receiver's wallet past the carry cap.
    WalletWouldOverflow,
    /// Both axes failed at once.
    ItemsAndWallet,
}

/// Why a completion bounced, naming each failing side honestly. A bounce with
/// zero failures is unrepresentable — that case is a completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BouncedProof {
    /// Only the requester's receiving proof failed.
    Requester {
        /// The failed axis or axes.
        failure: SideFailure,
    },
    /// Only the partner's receiving proof failed.
    Partner {
        /// The failed axis or axes.
        failure: SideFailure,
    },
    /// Both sides' receiving proofs failed — each named independently.
    Both {
        /// The requester's failed axis or axes.
        requester: SideFailure,
        /// The partner's failed axis or axes.
        partner: SideFailure,
    },
}

/// The partner-mirroring and both-notifying trade events — returned values,
/// never side effects. Each names the [`Side`] it concerns; the host owns the
/// id map and event delivery. An unlock always mirrors as `Unlocked`, never as
/// a lock; any content edit that cleared the partner's lock is named
/// `DealChanged`, so the lock-then-swap scam is impossible.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TradeEvent {
    /// Both sides: a trade window opened for the pair.
    Opened,
    /// Both sides: the partner accepted; the windows are live.
    Accepted,
    /// Partner mirror: a side offered an item into its window.
    ItemOffered {
        /// The offering side.
        by: Side,
        /// The window anchor the item was placed at.
        at: Cell,
    },
    /// Partner mirror: a side withdrew an offered item.
    ItemWithdrawn {
        /// The withdrawing side.
        by: Side,
        /// The window cell the item left.
        at: Cell,
    },
    /// Partner mirror: a side re-anchored an item within its own window — a
    /// content-preserving move that resets no locks.
    ItemRearranged {
        /// The rearranging side.
        by: Side,
        /// The cell the item moved from.
        from: Cell,
        /// The anchor the item now sits at.
        to: Cell,
    },
    /// Partner mirror: a side's zen offer now stands at `amount`.
    ZenOffered {
        /// The offering side.
        by: Side,
        /// The full offered amount.
        amount: Zen,
    },
    /// Partner mirror: a side locked its offer.
    Locked {
        /// The locking side.
        by: Side,
    },
    /// Partner mirror: a side unlocked its offer — honest, never read as a
    /// lock.
    Unlocked {
        /// The unlocking side.
        by: Side,
    },
    /// Partner warning: a content edit reset the locks.
    DealChanged {
        /// The editing side.
        by: Side,
    },
    /// Both sides: the swap committed.
    Completed,
    /// Both sides: the trade ended without completing, with the reason.
    Cancelled {
        /// Why the trade ended.
        reason: CancelReason,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carried(value: u64) -> CarriedZen {
        CarriedZen::new(value).unwrap()
    }

    fn cell(row: u8, col: u8) -> Cell {
        Cell { row, col }
    }

    #[test]
    fn request_rejection_round_trips_every_reason_as_a_bare_string() {
        assert_eq!(
            serde_json::to_string(&RequestRejection::SelfTrade).unwrap(),
            r#""self_trade""#
        );
        for reason in [
            RequestRejection::SelfTrade,
            RequestRejection::PartnerBusy,
            RequestRejection::PartnerDead,
            RequestRejection::OutOfRange,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(
                serde_json::from_str::<RequestRejection>(&json).unwrap(),
                reason
            );
        }
    }

    #[test]
    fn offer_outcome_round_trips_every_kind() {
        assert_eq!(
            serde_json::to_string(&OfferOutcome::Offered { at: cell(0, 1) }).unwrap(),
            r#"{"kind":"offered","at":{"row":0,"col":1}}"#
        );
        for outcome in [
            OfferOutcome::Offered { at: cell(2, 3) },
            OfferOutcome::NotOpen,
            OfferOutcome::SideLocked,
            OfferOutcome::NoItemAtSource,
            OfferOutcome::WindowCellsOccupied,
            OfferOutcome::WindowOutOfBounds,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<OfferOutcome>(&json).unwrap(),
                outcome
            );
        }
    }

    #[test]
    fn withdraw_outcome_round_trips_every_kind() {
        for outcome in [
            WithdrawOutcome::Withdrawn { at: cell(1, 1) },
            WithdrawOutcome::NotOpen,
            WithdrawOutcome::SideLocked,
            WithdrawOutcome::NoItemAtWindowCell,
            WithdrawOutcome::InventoryFull,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<WithdrawOutcome>(&json).unwrap(),
                outcome
            );
        }
    }

    #[test]
    fn zen_offer_outcome_wire_pin_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&ZenOfferOutcome::Offered {
                escrowed: Zen(400_000),
                wallet: carried(600_000),
            })
            .unwrap(),
            r#"{"kind":"offered","escrowed":400000,"wallet":600000}"#
        );
        for outcome in [
            ZenOfferOutcome::Offered {
                escrowed: Zen(0),
                wallet: carried(0),
            },
            ZenOfferOutcome::NotOpen,
            ZenOfferOutcome::SideLocked,
            ZenOfferOutcome::Unaffordable {
                wallet: carried(300_000),
            },
            ZenOfferOutcome::WalletFull,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<ZenOfferOutcome>(&json).unwrap(),
                outcome
            );
        }
    }

    #[test]
    fn rearrange_outcome_round_trips_every_kind() {
        for outcome in [
            RearrangeOutcome::Rearranged {
                from: cell(0, 0),
                to: cell(2, 0),
            },
            RearrangeOutcome::NotOpen,
            RearrangeOutcome::SideLocked,
            RearrangeOutcome::NoItemAtWindowCell,
            RearrangeOutcome::WindowCellsOccupied,
            RearrangeOutcome::WindowOutOfBounds,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<RearrangeOutcome>(&json).unwrap(),
                outcome
            );
        }
    }

    #[test]
    fn unlock_outcome_and_cancel_reason_round_trip() {
        for outcome in [
            UnlockOutcome::Unlocked,
            UnlockOutcome::NotOpen,
            UnlockOutcome::AlreadyUnlocked,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<UnlockOutcome>(&json).unwrap(),
                outcome
            );
        }
        for reason in [
            CancelReason::Explicit,
            CancelReason::Declined,
            CancelReason::Disconnected,
            CancelReason::Died,
            CancelReason::HostPolicy,
        ] {
            let json = serde_json::to_string(&reason).unwrap();
            assert_eq!(serde_json::from_str::<CancelReason>(&json).unwrap(), reason);
        }
    }

    #[test]
    fn bounced_proof_wire_pin_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&BouncedProof::Both {
                requester: SideFailure::ItemsDoNotFit,
                partner: SideFailure::WalletWouldOverflow,
            })
            .unwrap(),
            r#"{"kind":"both","requester":"items_do_not_fit","partner":"wallet_would_overflow"}"#
        );
        for proof in [
            BouncedProof::Requester {
                failure: SideFailure::ItemsDoNotFit,
            },
            BouncedProof::Partner {
                failure: SideFailure::WalletWouldOverflow,
            },
            BouncedProof::Both {
                requester: SideFailure::ItemsAndWallet,
                partner: SideFailure::ItemsDoNotFit,
            },
        ] {
            let json = serde_json::to_string(&proof).unwrap();
            assert_eq!(serde_json::from_str::<BouncedProof>(&json).unwrap(), proof);
        }
    }

    #[test]
    fn settlement_round_trips_containers_and_overflow() {
        let settlement = Settlement {
            requester: SettledSide {
                inventory: Inventory::empty(15, 8),
                wallet: carried(1_000_000),
                overflow: Overflow::empty(),
            },
            partner: SettledSide {
                inventory: Inventory::empty(15, 8),
                wallet: carried(2_000_000_000),
                overflow: Overflow {
                    items: Vec::new(),
                    zen: Zen(500_000),
                },
            },
        };
        let json = serde_json::to_string(&settlement).unwrap();
        assert_eq!(
            serde_json::from_str::<Settlement>(&json).unwrap(),
            settlement
        );
    }

    #[test]
    fn trade_event_round_trips_every_kind() {
        assert_eq!(
            serde_json::to_string(&TradeEvent::Cancelled {
                reason: CancelReason::Declined,
            })
            .unwrap(),
            r#"{"kind":"cancelled","reason":"declined"}"#
        );
        for event in [
            TradeEvent::Opened,
            TradeEvent::Accepted,
            TradeEvent::ItemOffered {
                by: Side::Requester,
                at: cell(0, 0),
            },
            TradeEvent::ItemWithdrawn {
                by: Side::Partner,
                at: cell(1, 2),
            },
            TradeEvent::ItemRearranged {
                by: Side::Requester,
                from: cell(0, 0),
                to: cell(2, 0),
            },
            TradeEvent::ZenOffered {
                by: Side::Partner,
                amount: Zen(250_000),
            },
            TradeEvent::Locked {
                by: Side::Requester,
            },
            TradeEvent::Unlocked {
                by: Side::Requester,
            },
            TradeEvent::DealChanged { by: Side::Partner },
            TradeEvent::Completed,
            TradeEvent::Cancelled {
                reason: CancelReason::Died,
            },
        ] {
            let json = serde_json::to_string(&event).unwrap();
            assert_eq!(serde_json::from_str::<TradeEvent>(&json).unwrap(), event);
        }
    }
}
