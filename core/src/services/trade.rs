//! Player↔player trade decisions: request, accept, offer, withdraw, zen
//! offer, rearrange, lock, unlock, and cancel — pure functions over one
//! [`TradeSession`] value per pair, the acting side's containers, and a
//! [`CarriedZen`] wallet. Every transition is `(session, actor, intent[,
//! containers]) -> (session | settlement, outcome, events)` with no RNG
//! anywhere in the family; the client never states a result. Escrow moves by
//! value — an offered item leaves the bag and lives in the session window, so
//! a double offer or a snapshot dupe is unrepresentable. The second lock is
//! the only completion trigger: it proves the full cross on cloned receivers
//! and commits all-or-nothing, or bounces back to open with both locks reset
//! and every offer intact. Cancel is total over every phase and reason —
//! death is never blocked, and value that cannot return rides an explicit
//! [`Overflow`].

use core::cmp::Ordering;

use serde::{Deserialize, Serialize};

use crate::components::inventory::{Cell, Inventory, PlacementRejection};
use crate::components::spatial::{Radius, WorldPos};
use crate::components::trade_window::Side;
use crate::components::units::{CarriedZen, CreditOutcome, DebitOutcome, Zen};
use crate::entities::trade_session::{TradeLocks, TradeOffer, TradeOffers, TradeSession};
use crate::events::trade::{
    BouncedProof, CancelReason, OfferOutcome, Overflow, RearrangeOutcome, RequestRejection,
    SettledSide, Settlement, SideFailure, TradeEvent, UnlockOutcome, WithdrawOutcome,
    ZenOfferOutcome,
};

/// The host-resolved facts about a request target — the id-free channel for
/// self, busy, and dead. Core never sees an id; the host computes these from
/// its own registries at the parse boundary. Every variant is a fact, never a
/// pre-baked decision — range is computed in core from `Available`'s position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TradeAvailability {
    /// The target resolved to the requester's own character.
    SameCharacter,
    /// The target already holds a session — core holds no cross-session
    /// registry; the host owns it.
    Busy,
    /// The target is not alive.
    Dead,
    /// A live, free target at this position.
    Available {
        /// The target's current position the reach rule is checked against.
        position: WorldPos,
    },
}

/// What one player carries into completion or settlement — the per-side
/// fusion of the two containers the cross and the return act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holdings {
    /// The player's bag.
    pub inventory: Inventory,
    /// The player's carried zen.
    pub wallet: CarriedZen,
}

/// What a trade request produced, kind-tagged. Carries the [`TradeSession`]
/// entity, so it lives in the service, not `events`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RequestOutcome {
    /// The request went out; the returned session is in the requested phase.
    Opened {
        /// The freshly requested session.
        session: TradeSession,
    },
    /// The request was refused with a named reason; no session exists.
    Rejected {
        /// Why the request was refused.
        reason: RequestRejection,
    },
}

/// What an accept produced, kind-tagged. Every variant hands the session
/// back, so it lives in the service, not `events`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AcceptOutcome {
    /// The partner accepted; the session is now open.
    Accepted {
        /// The opened session.
        session: TradeSession,
    },
    /// The requester tried to accept its own request; unchanged.
    WrongSide {
        /// The unchanged session.
        session: TradeSession,
    },
    /// The partner walked out of reach; unchanged.
    OutOfRange {
        /// The unchanged session.
        session: TradeSession,
    },
    /// An accept arrived for an already-open trade — a named untrusted-input
    /// refusal; unchanged.
    NotRequested {
        /// The unchanged session.
        session: TradeSession,
    },
}

/// What a lock produced, kind-tagged. The session rides here because it is
/// sometimes absent — `Completed` is terminal — while the two [`Holdings`]
/// always ride the return tuple, transformed only on completion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LockResult {
    /// The acting side is now locked; nothing crossed.
    Locked {
        /// The session with the acting side locked.
        session: TradeSession,
    },
    /// The acting side was already locked — a named idempotent no-op.
    AlreadyLocked {
        /// The unchanged session.
        session: TradeSession,
    },
    /// The session is still in the requested phase.
    NotOpen {
        /// The unchanged session.
        session: TradeSession,
    },
    /// The completion proof failed: back to open with both locks reset and
    /// every offer intact — never a cancel.
    Bounced {
        /// The session, back in the open phase, escrow intact.
        session: TradeSession,
        /// Which side or sides failed, and on which axes.
        proof: BouncedProof,
    },
    /// The proof passed and the whole escrow crossed; the session is
    /// consumed and the transformed holdings ride the return tuple.
    Completed,
}

/// The trade interaction reach shared by request and accept.
// Design pin: 12 tiles on the Euclidean `within_range` seam — a stated
// deviation from the era's Chebyshev view range; post-accept intents consult
// no position at all.
fn trade_reach() -> Radius {
    Radius::from_tiles(12)
}

/// Requests a trade with a host-resolved target. Self, busy, and dead arrive
/// as facts; range is computed here from the target's position. A refusal
/// names its reason — never silence.
#[must_use]
pub fn request(
    requester_pos: WorldPos,
    target: TradeAvailability,
) -> (RequestOutcome, Vec<TradeEvent>) {
    let rejection = match target {
        TradeAvailability::SameCharacter => RequestRejection::SelfTrade,
        TradeAvailability::Busy => RequestRejection::PartnerBusy,
        TradeAvailability::Dead => RequestRejection::PartnerDead,
        TradeAvailability::Available { position } => {
            if requester_pos.within_range(position, trade_reach()) {
                return (
                    RequestOutcome::Opened {
                        session: TradeSession::Requested,
                    },
                    vec![TradeEvent::Opened],
                );
            }
            RequestRejection::OutOfRange
        }
    };
    (RequestOutcome::Rejected { reason: rejection }, Vec::new())
}

/// Accepts a requested trade. Directionality is typed — only the partner can
/// accept — and range is re-checked against the current positions, since the
/// pair may have walked since the request.
#[must_use]
pub fn accept(
    session: TradeSession,
    actor: Side,
    requester_pos: WorldPos,
    partner_pos: WorldPos,
) -> (AcceptOutcome, Vec<TradeEvent>) {
    match session {
        TradeSession::Open { offers, locks } => (
            AcceptOutcome::NotRequested {
                session: TradeSession::Open { offers, locks },
            },
            Vec::new(),
        ),
        TradeSession::Requested => match actor {
            Side::Requester => (
                AcceptOutcome::WrongSide {
                    session: TradeSession::Requested,
                },
                Vec::new(),
            ),
            Side::Partner => {
                if !requester_pos.within_range(partner_pos, trade_reach()) {
                    return (
                        AcceptOutcome::OutOfRange {
                            session: TradeSession::Requested,
                        },
                        Vec::new(),
                    );
                }
                (
                    AcceptOutcome::Accepted {
                        session: TradeSession::opened(),
                    },
                    vec![TradeEvent::Accepted],
                )
            }
        },
    }
}

/// Offers the bag item covering `from` into the acting side's window at `to`
/// — escrow-by-move: the item leaves the bag and lives in the session, so
/// offering the same cell twice finds it empty. A worn item is offered by
/// host composition (unequip, then offer); no equipment path exists here. Any
/// content edit resets the locks and warns the partner.
#[must_use]
pub fn offer_item(
    session: TradeSession,
    actor: Side,
    inventory: Inventory,
    from: Cell,
    to: Cell,
) -> (TradeSession, Inventory, OfferOutcome, Vec<TradeEvent>) {
    let TradeSession::Open { offers, locks } = session else {
        return (
            TradeSession::Requested,
            inventory,
            OfferOutcome::NotOpen,
            Vec::new(),
        );
    };
    if locks == (TradeLocks::OneLocked { side: actor }) {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            OfferOutcome::SideLocked,
            Vec::new(),
        );
    }
    let Some(occupant) = inventory.occupant(from) else {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            OfferOutcome::NoItemAtSource,
            Vec::new(),
        );
    };
    let footprint = occupant.footprint;
    // The move runs on forks and both containers commit together; on any
    // refusal the originals stand untouched — the item never dangles.
    // `occupant` proved the cell covered; the removal's own rejection
    // re-answers the presence question — total, never a panic.
    let Ok((bag, item)) = inventory.clone().remove(from) else {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            OfferOutcome::NoItemAtSource,
            Vec::new(),
        );
    };
    let offer = offers.get(actor).clone();
    let window = match offer.window().clone().place(to, footprint, item) {
        Ok(window) => window,
        Err((_, _, PlacementRejection::CellOutOfBounds)) => {
            return (
                TradeSession::Open { offers, locks },
                inventory,
                OfferOutcome::WindowOutOfBounds,
                Vec::new(),
            );
        }
        // `NoItemAtCell` is not a placement verdict; the or-fold keeps the
        // dispatch total without a wildcard.
        Err((_, _, PlacementRejection::CellsOccupied | PlacementRejection::NoItemAtCell)) => {
            return (
                TradeSession::Open { offers, locks },
                inventory,
                OfferOutcome::WindowCellsOccupied,
                Vec::new(),
            );
        }
    };
    let (locks, deal_changed) = reset_locks(locks, actor);
    let offers = offers.with(actor, offer.with_window(window));
    let mut events = vec![TradeEvent::ItemOffered { by: actor, at: to }];
    events.extend(deal_changed);
    (
        TradeSession::Open { offers, locks },
        bag,
        OfferOutcome::Offered { at: to },
        events,
    )
}

/// Withdraws the window item covering `from` back into the bag at the first
/// fitting anchor. A bag with no fitting region refuses and the item stays
/// escrowed — still owned, never destroyed. Any content edit resets the locks
/// and warns the partner.
#[must_use]
pub fn withdraw_item(
    session: TradeSession,
    actor: Side,
    from: Cell,
    inventory: Inventory,
) -> (TradeSession, Inventory, WithdrawOutcome, Vec<TradeEvent>) {
    let TradeSession::Open { offers, locks } = session else {
        return (
            TradeSession::Requested,
            inventory,
            WithdrawOutcome::NotOpen,
            Vec::new(),
        );
    };
    if locks == (TradeLocks::OneLocked { side: actor }) {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            WithdrawOutcome::SideLocked,
            Vec::new(),
        );
    }
    let offer = offers.get(actor).clone();
    let Some(occupant) = offer.window().occupant(from) else {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            WithdrawOutcome::NoItemAtWindowCell,
            Vec::new(),
        );
    };
    let footprint = occupant.footprint;
    let Some(anchor) = inventory.first_fit(footprint) else {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            WithdrawOutcome::InventoryFull,
            Vec::new(),
        );
    };
    // The move runs on forks and both containers commit together; on any
    // refusal the originals stand untouched and the item stays escrowed.
    // `occupant` proved the cell covered; the removal's own rejection
    // re-answers the presence question — total, never a panic.
    let Ok((window, item, _footprint)) = offer.window().clone().remove(from) else {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            WithdrawOutcome::NoItemAtWindowCell,
            Vec::new(),
        );
    };
    // `first_fit` proved the region free; the placement's own rejection
    // re-answers the space question — total, never a panic.
    let Ok(bag) = inventory.clone().place(anchor, footprint, item) else {
        return (
            TradeSession::Open { offers, locks },
            inventory,
            WithdrawOutcome::InventoryFull,
            Vec::new(),
        );
    };
    let (locks, deal_changed) = reset_locks(locks, actor);
    let offers = offers.with(actor, offer.with_window(window));
    let mut events = vec![TradeEvent::ItemWithdrawn {
        by: actor,
        at: from,
    }];
    events.extend(deal_changed);
    (
        TradeSession::Open { offers, locks },
        bag,
        WithdrawOutcome::Withdrawn { at: from },
        events,
    )
}

/// Sets the acting side's zen offer to `amount`, moving the wallet by only
/// the delta: a raise debits the difference, a lower credits it back (refused
/// [`ZenOfferOutcome::WalletFull`] when the credit would over-cap — offer and
/// wallet unchanged, value never destroyed), and re-setting the same amount
/// is a net-zero success with no wallet operation, even at a zero or capped
/// wallet. Any content edit resets the locks and warns the partner.
#[must_use]
pub fn offer_zen(
    session: TradeSession,
    actor: Side,
    wallet: CarriedZen,
    amount: Zen,
) -> (TradeSession, CarriedZen, ZenOfferOutcome, Vec<TradeEvent>) {
    let TradeSession::Open { offers, locks } = session else {
        return (
            TradeSession::Requested,
            wallet,
            ZenOfferOutcome::NotOpen,
            Vec::new(),
        );
    };
    if locks == (TradeLocks::OneLocked { side: actor }) {
        return (
            TradeSession::Open { offers, locks },
            wallet,
            ZenOfferOutcome::SideLocked,
            Vec::new(),
        );
    }
    let offer = offers.get(actor).clone();
    let escrow = offer.escrow_zen();
    let balance = match amount.cmp(&escrow) {
        Ordering::Equal => wallet,
        Ordering::Greater => match wallet.debit(Zen(amount.0.saturating_sub(escrow.0))) {
            DebitOutcome::Debited { balance } => balance,
            DebitOutcome::Insufficient { .. } => {
                return (
                    TradeSession::Open { offers, locks },
                    wallet,
                    ZenOfferOutcome::Unaffordable { wallet },
                    Vec::new(),
                );
            }
        },
        Ordering::Less => match wallet.credit(Zen(escrow.0.saturating_sub(amount.0))) {
            CreditOutcome::Credited { balance } => balance,
            CreditOutcome::OverCap { .. } => {
                return (
                    TradeSession::Open { offers, locks },
                    wallet,
                    ZenOfferOutcome::WalletFull,
                    Vec::new(),
                );
            }
        },
    };
    let (locks, deal_changed) = reset_locks(locks, actor);
    let offers = offers.with(actor, offer.with_escrow_zen(amount));
    let mut events = vec![TradeEvent::ZenOffered { by: actor, amount }];
    events.extend(deal_changed);
    (
        TradeSession::Open { offers, locks },
        balance,
        ZenOfferOutcome::Offered {
            escrowed: amount,
            wallet: balance,
        },
        events,
    )
}

/// Re-anchors an offered item within the acting side's own window — a
/// content-preserving move: the same item, still offered, so no lock resets
/// and no deal-changed warning fires. Rejects onto-occupied and never swaps.
#[must_use]
pub fn rearrange(
    session: TradeSession,
    actor: Side,
    from: Cell,
    to: Cell,
) -> (TradeSession, RearrangeOutcome, Vec<TradeEvent>) {
    let TradeSession::Open { offers, locks } = session else {
        return (
            TradeSession::Requested,
            RearrangeOutcome::NotOpen,
            Vec::new(),
        );
    };
    if locks == (TradeLocks::OneLocked { side: actor }) {
        return (
            TradeSession::Open { offers, locks },
            RearrangeOutcome::SideLocked,
            Vec::new(),
        );
    }
    let offer = offers.get(actor).clone();
    let outcome = match offer.window().clone().move_to(from, to) {
        Ok(window) => {
            let offers = offers.with(actor, offer.with_window(window));
            return (
                TradeSession::Open { offers, locks },
                RearrangeOutcome::Rearranged { from, to },
                vec![TradeEvent::ItemRearranged {
                    by: actor,
                    from,
                    to,
                }],
            );
        }
        Err((_, PlacementRejection::NoItemAtCell)) => RearrangeOutcome::NoItemAtWindowCell,
        Err((_, PlacementRejection::CellsOccupied)) => RearrangeOutcome::WindowCellsOccupied,
        Err((_, PlacementRejection::CellOutOfBounds)) => RearrangeOutcome::WindowOutOfBounds,
    };
    (TradeSession::Open { offers, locks }, outcome, Vec::new())
}

/// Locks the acting side's offer. The first lock marks the side and mirrors
/// it honestly; the second lock is the only completion trigger — it runs the
/// full-batch prove-then-move cross and either commits atomically
/// ([`LockResult::Completed`], transformed holdings in the tuple) or bounces
/// back to open with both locks reset and every offer intact.
/// Both holdings are always threaded and always handed back — the caller
/// cannot know in advance which lock completes.
#[must_use]
pub fn lock(
    session: TradeSession,
    actor: Side,
    requester: Holdings,
    partner: Holdings,
) -> (Holdings, Holdings, LockResult, Vec<TradeEvent>) {
    let TradeSession::Open { offers, locks } = session else {
        return (
            requester,
            partner,
            LockResult::NotOpen {
                session: TradeSession::Requested,
            },
            Vec::new(),
        );
    };
    match locks {
        TradeLocks::NeitherLocked => (
            requester,
            partner,
            LockResult::Locked {
                session: TradeSession::Open {
                    offers,
                    locks: TradeLocks::OneLocked { side: actor },
                },
            },
            vec![TradeEvent::Locked { by: actor }],
        ),
        TradeLocks::OneLocked { side } if side == actor => (
            requester,
            partner,
            LockResult::AlreadyLocked {
                session: TradeSession::Open { offers, locks },
            },
            Vec::new(),
        ),
        // The other side already locked — this is the second press.
        TradeLocks::OneLocked { side: _ } => complete(offers, requester, partner),
    }
}

/// Unlocks the acting side's own lock. Honest mirror: an unlock emits
/// [`TradeEvent::Unlocked`], never a lock; unlocking a side that holds no
/// lock is a named idempotent no-op.
#[must_use]
pub fn unlock(
    session: TradeSession,
    actor: Side,
) -> (TradeSession, UnlockOutcome, Vec<TradeEvent>) {
    let TradeSession::Open { offers, locks } = session else {
        return (TradeSession::Requested, UnlockOutcome::NotOpen, Vec::new());
    };
    match locks {
        TradeLocks::OneLocked { side } if side == actor => (
            TradeSession::Open {
                offers,
                locks: TradeLocks::NeitherLocked,
            },
            UnlockOutcome::Unlocked,
            vec![TradeEvent::Unlocked { by: actor }],
        ),
        TradeLocks::NeitherLocked | TradeLocks::OneLocked { .. } => (
            TradeSession::Open { offers, locks },
            UnlockOutcome::AlreadyUnlocked,
            Vec::new(),
        ),
    }
}

/// Cancels the trade — total over both phases and every reason, so death is
/// never blocked. Each side's escrow returns to its own current containers:
/// items re-placed by first-fit, zen credited back cap-checked; what cannot
/// land rides [`Overflow`] for the host to ground-drop. The event carries the
/// reason, so a decline reads as declined.
#[must_use]
pub fn cancel(
    session: TradeSession,
    reason: CancelReason,
    requester: Holdings,
    partner: Holdings,
) -> (Settlement, Vec<TradeEvent>) {
    let settlement = match session {
        TradeSession::Requested => Settlement {
            requester: SettledSide {
                inventory: requester.inventory,
                wallet: requester.wallet,
                overflow: Overflow::empty(),
            },
            partner: SettledSide {
                inventory: partner.inventory,
                wallet: partner.wallet,
                overflow: Overflow::empty(),
            },
        },
        TradeSession::Open { offers, locks: _ } => Settlement {
            requester: settle(offers.get(Side::Requester), requester),
            partner: settle(offers.get(Side::Partner), partner),
        },
    };
    (settlement, vec![TradeEvent::Cancelled { reason }])
}

/// The locks after a content edit by `actor`, plus the deal-changed warning
/// when a partner lock was cleared. Runs after the side-locked gate, so a
/// standing lock here belongs to the partner.
fn reset_locks(locks: TradeLocks, actor: Side) -> (TradeLocks, Option<TradeEvent>) {
    match locks {
        TradeLocks::NeitherLocked => (TradeLocks::NeitherLocked, None),
        TradeLocks::OneLocked { side: _ } => (
            TradeLocks::NeitherLocked,
            Some(TradeEvent::DealChanged { by: actor }),
        ),
    }
}

/// One side's completion verdict: the receiver either fits the whole incoming
/// batch and has wallet headroom for the incoming zen, or fails on the named
/// axes. `Ready` carries the proven post-commit containers.
enum SideVerdict {
    /// Every incoming item landed on the fork and the credit fit under the
    /// cap; the fork and balance are the post-commit containers.
    Ready {
        /// The receiver's bag with the whole incoming batch placed.
        inventory: Inventory,
        /// The receiver's wallet with the incoming zen credited.
        wallet: CarriedZen,
    },
    /// At least one axis failed; nothing may cross.
    Failed {
        /// The failed axis or axes.
        failure: SideFailure,
    },
}

/// Proves one direction of the cross on clones: the giver's whole window
/// placed item-by-item onto one progressively filled fork of the receiver's
/// bag (two items that each fit alone but not together fail together — never
/// a partial commit), and the giver's escrowed zen credited onto a copy of
/// the receiver's wallet, cap-checked. Placing clones keeps the session
/// untouched, so a bounce needs no rebuild.
fn receive_verdict(receiver: &Holdings, giving: &TradeOffer) -> SideVerdict {
    let mut fork = receiver.inventory.clone();
    let mut fits = true;
    for placed in giving.window().placed() {
        let footprint = placed.footprint;
        let Some(anchor) = fork.first_fit(footprint) else {
            fits = false;
            break;
        };
        match fork.place(anchor, footprint, placed.item.clone()) {
            Ok(next) => fork = next,
            // `first_fit` proved the region free; the placement's own
            // rejection re-answers the space question — total, never a panic.
            Err((rolled_back, _, _)) => {
                fork = rolled_back;
                fits = false;
                break;
            }
        }
    }
    match (fits, receiver.wallet.credit(giving.escrow_zen())) {
        (true, CreditOutcome::Credited { balance }) => SideVerdict::Ready {
            inventory: fork,
            wallet: balance,
        },
        (false, CreditOutcome::Credited { .. }) => SideVerdict::Failed {
            failure: SideFailure::ItemsDoNotFit,
        },
        (true, CreditOutcome::OverCap { .. }) => SideVerdict::Failed {
            failure: SideFailure::WalletWouldOverflow,
        },
        (false, CreditOutcome::OverCap { .. }) => SideVerdict::Failed {
            failure: SideFailure::ItemsAndWallet,
        },
    }
}

/// The second lock's prove-then-move: both directions proven on clones, then
/// an all-or-nothing verdict. On unanimous fit the proven forks become the
/// new bags and the credited balances the new wallets — the consumed
/// session's windows drop, so each escrowed item survives exactly once, in
/// its receiver. On any failure nothing crosses: the original holdings return
/// unchanged and the session goes back to open with both locks reset and
/// every offer intact.
fn complete(
    offers: TradeOffers,
    requester: Holdings,
    partner: Holdings,
) -> (Holdings, Holdings, LockResult, Vec<TradeEvent>) {
    let requester_verdict = receive_verdict(&requester, offers.get(Side::Partner));
    let partner_verdict = receive_verdict(&partner, offers.get(Side::Requester));
    let proof = match (requester_verdict, partner_verdict) {
        (
            SideVerdict::Ready {
                inventory: requester_inventory,
                wallet: requester_wallet,
            },
            SideVerdict::Ready {
                inventory: partner_inventory,
                wallet: partner_wallet,
            },
        ) => {
            return (
                Holdings {
                    inventory: requester_inventory,
                    wallet: requester_wallet,
                },
                Holdings {
                    inventory: partner_inventory,
                    wallet: partner_wallet,
                },
                LockResult::Completed,
                vec![TradeEvent::Completed],
            );
        }
        (SideVerdict::Failed { failure }, SideVerdict::Ready { .. }) => {
            BouncedProof::Requester { failure }
        }
        (SideVerdict::Ready { .. }, SideVerdict::Failed { failure }) => {
            BouncedProof::Partner { failure }
        }
        (
            SideVerdict::Failed {
                failure: requester_failure,
            },
            SideVerdict::Failed {
                failure: partner_failure,
            },
        ) => BouncedProof::Both {
            requester: requester_failure,
            partner: partner_failure,
        },
    };
    (
        requester,
        partner,
        LockResult::Bounced {
            session: TradeSession::Open {
                offers,
                locks: TradeLocks::NeitherLocked,
            },
            proof,
        },
        Vec::new(),
    )
}

/// One side's total settlement: every escrowed item re-placed into the
/// owner's current bag by first-fit, then the escrowed zen credited back
/// cap-checked. Whatever cannot land rides the returned [`Overflow`] — the
/// settlement has no gate that can fail.
fn settle(offer: &TradeOffer, holdings: Holdings) -> SettledSide {
    let Holdings {
        mut inventory,
        wallet,
    } = holdings;
    let mut overflow_items = Vec::new();
    for placed in offer.window().placed() {
        let footprint = placed.footprint;
        let item = placed.item.clone();
        inventory = if let Some(anchor) = inventory.first_fit(footprint) {
            match inventory.place(anchor, footprint, item) {
                Ok(next) => next,
                // `first_fit` proved the region free; the placement's own
                // rejection re-answers the space question — total, never a
                // panic.
                Err((rolled_back, item, _)) => {
                    overflow_items.push(item);
                    rolled_back
                }
            }
        } else {
            overflow_items.push(item);
            inventory
        };
    }
    let (wallet, overflow_zen) = match wallet.credit(offer.escrow_zen()) {
        CreditOutcome::Credited { balance } => (balance, Zen(0)),
        CreditOutcome::OverCap { balance } => (balance, offer.escrow_zen()),
    };
    SettledSide {
        inventory,
        wallet,
        overflow: Overflow {
            items: overflow_items,
            zen: overflow_zen,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::inventory::Footprint;
    use crate::components::item_instance::{
        CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::tile::TileCoord;
    use crate::components::trade_window::TradeWindow;
    use crate::components::units::ItemLevel;

    fn item(number: u16) -> ItemInstance {
        ItemInstance {
            item: ItemRef { group: 0, number },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
            augment: CraftedAugment::None,
        }
    }

    fn cell(row: u8, col: u8) -> Cell {
        Cell { row, col }
    }

    fn footprint(width: u8, height: u8) -> Footprint {
        Footprint::new(width, height).unwrap()
    }

    fn carried(value: u64) -> CarriedZen {
        CarriedZen::new(value).unwrap()
    }

    fn pos(x: u8, y: u8) -> WorldPos {
        TileCoord::new(x, y).to_world()
    }

    fn bag() -> Inventory {
        Inventory::empty(15, 8)
    }

    fn holdings(inventory: Inventory, wallet: u64) -> Holdings {
        Holdings {
            inventory,
            wallet: carried(wallet),
        }
    }

    fn offer_with_item(number: u16, fp: Footprint, at: Cell) -> TradeOffer {
        TradeOffer::empty().with_window(TradeWindow::empty().place(at, fp, item(number)).unwrap())
    }

    fn open_session(requester: TradeOffer, partner: TradeOffer, locks: TradeLocks) -> TradeSession {
        TradeSession::Open {
            offers: TradeOffers::empty()
                .with(Side::Requester, requester)
                .with(Side::Partner, partner),
            locks,
        }
    }

    fn offers_of(session: &TradeSession) -> &TradeOffers {
        match session {
            TradeSession::Open { offers, .. } => offers,
            TradeSession::Requested => panic!("expected an open session"),
        }
    }

    fn locks_of(session: &TradeSession) -> TradeLocks {
        match session {
            TradeSession::Open { locks, .. } => *locks,
            TradeSession::Requested => panic!("expected an open session"),
        }
    }

    // --- Request. -----------------------------------------------------------

    #[test]
    fn request_to_an_available_in_range_partner_opens_a_requested_session() {
        let (outcome, events) = request(
            pos(0, 0),
            TradeAvailability::Available {
                position: pos(12, 0),
            },
        );
        assert_eq!(
            outcome,
            RequestOutcome::Opened {
                session: TradeSession::Requested
            }
        );
        assert_eq!(events, vec![TradeEvent::Opened]);
    }

    #[test]
    fn request_reach_is_inclusive_at_twelve_tiles_and_out_at_thirteen() {
        let (at_edge, _) = request(
            pos(0, 0),
            TradeAvailability::Available {
                position: pos(12, 0),
            },
        );
        assert!(matches!(at_edge, RequestOutcome::Opened { .. }));
        let (past_edge, events) = request(
            pos(0, 0),
            TradeAvailability::Available {
                position: pos(13, 0),
            },
        );
        assert_eq!(
            past_edge,
            RequestOutcome::Rejected {
                reason: RequestRejection::OutOfRange
            }
        );
        assert!(events.is_empty());
    }

    #[test]
    fn request_names_self_busy_and_dead_refusals() {
        for (target, reason) in [
            (
                TradeAvailability::SameCharacter,
                RequestRejection::SelfTrade,
            ),
            (TradeAvailability::Busy, RequestRejection::PartnerBusy),
            (TradeAvailability::Dead, RequestRejection::PartnerDead),
        ] {
            let (outcome, events) = request(pos(0, 0), target);
            assert_eq!(outcome, RequestOutcome::Rejected { reason });
            assert!(events.is_empty());
        }
    }

    // --- Accept. ------------------------------------------------------------

    #[test]
    fn accept_by_the_partner_in_range_opens_the_windows() {
        let (outcome, events) =
            accept(TradeSession::Requested, Side::Partner, pos(0, 0), pos(5, 0));
        assert_eq!(
            outcome,
            AcceptOutcome::Accepted {
                session: TradeSession::opened()
            }
        );
        assert_eq!(events, vec![TradeEvent::Accepted]);
    }

    #[test]
    fn the_requester_cannot_accept_its_own_request() {
        let (outcome, events) = accept(
            TradeSession::Requested,
            Side::Requester,
            pos(0, 0),
            pos(5, 0),
        );
        assert_eq!(
            outcome,
            AcceptOutcome::WrongSide {
                session: TradeSession::Requested
            }
        );
        assert!(events.is_empty());
    }

    #[test]
    fn accept_rechecks_range_against_the_current_positions() {
        let (outcome, _) = accept(
            TradeSession::Requested,
            Side::Partner,
            pos(0, 0),
            pos(13, 0),
        );
        assert_eq!(
            outcome,
            AcceptOutcome::OutOfRange {
                session: TradeSession::Requested
            }
        );
    }

    #[test]
    fn accept_on_an_already_open_session_is_not_requested() {
        let (outcome, events) = accept(TradeSession::opened(), Side::Partner, pos(0, 0), pos(5, 0));
        assert_eq!(
            outcome,
            AcceptOutcome::NotRequested {
                session: TradeSession::opened()
            }
        );
        assert!(events.is_empty());
    }

    // --- Offer an item (escrow-by-move). -------------------------------------

    #[test]
    fn offer_item_moves_the_item_out_of_the_bag_into_the_window() {
        let stored = bag().place(cell(0, 0), footprint(2, 2), item(1)).unwrap();
        let (session, inventory, outcome, events) = offer_item(
            TradeSession::opened(),
            Side::Requester,
            stored,
            cell(0, 0),
            cell(0, 0),
        );
        assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 0) });
        assert!(inventory.occupant(cell(0, 0)).is_none());
        let window = offers_of(&session).get(Side::Requester).window();
        assert_eq!(window.occupant(cell(0, 0)).unwrap().item, item(1));
        assert_eq!(
            events,
            vec![TradeEvent::ItemOffered {
                by: Side::Requester,
                at: cell(0, 0),
            }]
        );
    }

    #[test]
    fn offer_item_onto_an_occupied_window_cell_keeps_the_item_in_the_bag() {
        let session = open_session(
            offer_with_item(1, footprint(2, 2), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let stored = bag().place(cell(2, 2), footprint(1, 1), item(2)).unwrap();
        let (after, inventory, outcome, events) = offer_item(
            session.clone(),
            Side::Requester,
            stored,
            cell(2, 2),
            cell(0, 0),
        );
        assert_eq!(outcome, OfferOutcome::WindowCellsOccupied);
        assert!(inventory.occupant(cell(2, 2)).is_some());
        assert_eq!(after, session);
        assert!(events.is_empty());
    }

    #[test]
    fn offer_item_whose_footprint_leaves_the_window_keeps_the_item() {
        let stored = bag().place(cell(0, 0), footprint(1, 3), item(3)).unwrap();
        let (after, inventory, outcome, _) = offer_item(
            TradeSession::opened(),
            Side::Requester,
            stored,
            cell(0, 0),
            cell(3, 0),
        );
        assert_eq!(outcome, OfferOutcome::WindowOutOfBounds);
        assert!(inventory.occupant(cell(0, 0)).is_some());
        assert_eq!(after, TradeSession::opened());
    }

    #[test]
    fn the_same_source_cell_cannot_be_offered_twice() {
        let stored = bag().place(cell(0, 0), footprint(2, 2), item(1)).unwrap();
        let (session, inventory, _, _) = offer_item(
            TradeSession::opened(),
            Side::Requester,
            stored,
            cell(0, 0),
            cell(0, 0),
        );
        let (session, _, outcome, events) =
            offer_item(session, Side::Requester, inventory, cell(0, 0), cell(0, 2));
        assert_eq!(outcome, OfferOutcome::NoItemAtSource);
        assert_eq!(
            offers_of(&session)
                .get(Side::Requester)
                .window()
                .placed()
                .len(),
            1
        );
        assert!(events.is_empty());
    }

    #[test]
    fn offer_item_never_merges_stacks_into_the_window() {
        let session = open_session(
            offer_with_item(7, footprint(1, 1), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let stored = bag().place(cell(3, 3), footprint(1, 1), item(7)).unwrap();
        let (session, _, outcome, _) =
            offer_item(session, Side::Requester, stored, cell(3, 3), cell(0, 1));
        assert_eq!(outcome, OfferOutcome::Offered { at: cell(0, 1) });
        let window = offers_of(&session).get(Side::Requester).window();
        assert_eq!(window.placed().len(), 2);
    }

    #[test]
    fn offer_item_gates_the_requested_phase_and_the_locked_side() {
        let stored = bag().place(cell(0, 0), footprint(1, 1), item(1)).unwrap();
        let (session, inventory, outcome, _) = offer_item(
            TradeSession::Requested,
            Side::Requester,
            stored,
            cell(0, 0),
            cell(0, 0),
        );
        assert_eq!(session, TradeSession::Requested);
        assert_eq!(outcome, OfferOutcome::NotOpen);
        let locked = open_session(
            TradeOffer::empty(),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
        );
        let (after, _, outcome, _) = offer_item(
            locked.clone(),
            Side::Requester,
            inventory,
            cell(0, 0),
            cell(0, 0),
        );
        assert_eq!(outcome, OfferOutcome::SideLocked);
        assert_eq!(after, locked);
    }

    // --- Withdraw an item. ----------------------------------------------------

    #[test]
    fn withdraw_item_returns_the_item_to_the_bag_and_clears_the_window() {
        let session = open_session(
            offer_with_item(1, footprint(2, 2), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let (session, inventory, outcome, events) =
            withdraw_item(session, Side::Requester, cell(0, 0), bag());
        assert_eq!(outcome, WithdrawOutcome::Withdrawn { at: cell(0, 0) });
        assert_eq!(inventory.occupant(cell(0, 0)).unwrap().item, item(1));
        assert!(
            offers_of(&session)
                .get(Side::Requester)
                .window()
                .placed()
                .is_empty()
        );
        assert_eq!(
            events,
            vec![TradeEvent::ItemWithdrawn {
                by: Side::Requester,
                at: cell(0, 0),
            }]
        );
    }

    #[test]
    fn withdraw_from_an_empty_window_cell_is_refused() {
        let (session, _, outcome, events) =
            withdraw_item(TradeSession::opened(), Side::Requester, cell(0, 0), bag());
        assert_eq!(outcome, WithdrawOutcome::NoItemAtWindowCell);
        assert_eq!(session, TradeSession::opened());
        assert!(events.is_empty());
    }

    #[test]
    fn withdraw_into_a_full_bag_keeps_the_item_escrowed() {
        let session = open_session(
            offer_with_item(1, footprint(1, 1), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let full = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(9))
            .unwrap();
        let (after, inventory, outcome, _) =
            withdraw_item(session.clone(), Side::Requester, cell(0, 0), full.clone());
        assert_eq!(outcome, WithdrawOutcome::InventoryFull);
        assert_eq!(inventory, full);
        assert_eq!(after, session);
    }

    #[test]
    fn withdraw_gates_the_requested_phase_and_the_locked_side() {
        let (_, _, outcome, _) =
            withdraw_item(TradeSession::Requested, Side::Requester, cell(0, 0), bag());
        assert_eq!(outcome, WithdrawOutcome::NotOpen);
        let locked = open_session(
            offer_with_item(1, footprint(1, 1), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
        );
        let (_, _, outcome, _) = withdraw_item(locked, Side::Requester, cell(0, 0), bag());
        assert_eq!(outcome, WithdrawOutcome::SideLocked);
    }

    // --- Rearrange (content-preserving). --------------------------------------

    #[test]
    fn rearrange_reanchors_the_item_and_resets_no_locks() {
        let session = open_session(
            offer_with_item(1, footprint(2, 2), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (session, outcome, events) =
            rearrange(session, Side::Requester, cell(0, 0), cell(2, 0));
        assert_eq!(
            outcome,
            RearrangeOutcome::Rearranged {
                from: cell(0, 0),
                to: cell(2, 0),
            }
        );
        assert_eq!(
            locks_of(&session),
            TradeLocks::OneLocked {
                side: Side::Partner
            }
        );
        let window = offers_of(&session).get(Side::Requester).window();
        assert!(window.occupant(cell(2, 0)).is_some());
        assert!(window.occupant(cell(0, 1)).is_none());
        assert_eq!(
            events,
            vec![TradeEvent::ItemRearranged {
                by: Side::Requester,
                from: cell(0, 0),
                to: cell(2, 0),
            }]
        );
    }

    #[test]
    fn rearrange_refusals_leave_the_window_unchanged() {
        let both = TradeOffer::empty().with_window(
            TradeWindow::empty()
                .place(cell(0, 0), footprint(1, 1), item(1))
                .unwrap()
                .place(cell(2, 0), footprint(1, 1), item(2))
                .unwrap(),
        );
        let session = open_session(both, TradeOffer::empty(), TradeLocks::NeitherLocked);
        let (session, outcome, _) = rearrange(session, Side::Requester, cell(0, 0), cell(2, 0));
        assert_eq!(outcome, RearrangeOutcome::WindowCellsOccupied);
        let (session, outcome, _) = rearrange(session, Side::Requester, cell(1, 1), cell(3, 3));
        assert_eq!(outcome, RearrangeOutcome::NoItemAtWindowCell);
        let (session, outcome, _) = rearrange(session, Side::Requester, cell(0, 0), cell(0, 8));
        assert_eq!(outcome, RearrangeOutcome::WindowOutOfBounds);
        let window = offers_of(&session).get(Side::Requester).window();
        assert!(window.occupant(cell(0, 0)).is_some());
        assert!(window.occupant(cell(2, 0)).is_some());
    }

    #[test]
    fn rearrange_gates_the_requested_phase_and_the_locked_side() {
        let (_, outcome, _) = rearrange(
            TradeSession::Requested,
            Side::Requester,
            cell(0, 0),
            cell(1, 0),
        );
        assert_eq!(outcome, RearrangeOutcome::NotOpen);
        let locked = open_session(
            offer_with_item(1, footprint(1, 1), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
        );
        let (_, outcome, _) = rearrange(locked, Side::Requester, cell(0, 0), cell(1, 0));
        assert_eq!(outcome, RearrangeOutcome::SideLocked);
    }

    // --- Zen offer (delta semantics). ------------------------------------------

    #[test]
    fn a_first_zen_offer_debits_the_delta() {
        let (session, wallet, outcome, events) = offer_zen(
            TradeSession::opened(),
            Side::Requester,
            carried(1_000_000),
            Zen(400_000),
        );
        assert_eq!(
            outcome,
            ZenOfferOutcome::Offered {
                escrowed: Zen(400_000),
                wallet: carried(600_000),
            }
        );
        assert_eq!(wallet, carried(600_000));
        assert_eq!(
            offers_of(&session).get(Side::Requester).escrow_zen(),
            Zen(400_000)
        );
        assert_eq!(
            events,
            vec![TradeEvent::ZenOffered {
                by: Side::Requester,
                amount: Zen(400_000),
            }]
        );
    }

    #[test]
    fn lowering_to_zero_credits_the_delta_back() {
        let session = open_session(
            TradeOffer::empty().with_escrow_zen(Zen(400_000)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let (session, wallet, outcome, _) =
            offer_zen(session, Side::Requester, carried(600_000), Zen(0));
        assert_eq!(
            outcome,
            ZenOfferOutcome::Offered {
                escrowed: Zen(0),
                wallet: carried(1_000_000),
            }
        );
        assert_eq!(wallet, carried(1_000_000));
        assert_eq!(
            offers_of(&session).get(Side::Requester).escrow_zen(),
            Zen(0)
        );
    }

    #[test]
    fn resetting_the_same_amount_is_a_net_zero_success_even_at_a_zero_wallet() {
        let session = open_session(
            TradeOffer::empty().with_escrow_zen(Zen(500_000)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let (_, wallet, outcome, _) = offer_zen(session, Side::Requester, carried(0), Zen(500_000));
        assert_eq!(
            outcome,
            ZenOfferOutcome::Offered {
                escrowed: Zen(500_000),
                wallet: carried(0),
            }
        );
        assert_eq!(wallet, carried(0));
    }

    #[test]
    fn raising_debits_only_the_difference() {
        let session = open_session(
            TradeOffer::empty().with_escrow_zen(Zen(200_000)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let (_, wallet, outcome, _) =
            offer_zen(session, Side::Requester, carried(300_000), Zen(500_000));
        assert_eq!(
            outcome,
            ZenOfferOutcome::Offered {
                escrowed: Zen(500_000),
                wallet: carried(0),
            }
        );
        assert_eq!(wallet, carried(0));
    }

    #[test]
    fn an_unaffordable_raise_preserves_the_prior_offer_and_wallet() {
        let (session, wallet, outcome, events) = offer_zen(
            TradeSession::opened(),
            Side::Requester,
            carried(300_000),
            Zen(400_000),
        );
        assert_eq!(
            outcome,
            ZenOfferOutcome::Unaffordable {
                wallet: carried(300_000)
            }
        );
        assert_eq!(wallet, carried(300_000));
        assert_eq!(
            offers_of(&session).get(Side::Requester).escrow_zen(),
            Zen(0)
        );
        assert!(events.is_empty());
    }

    #[test]
    fn a_lower_whose_credit_back_would_overcap_the_wallet_is_wallet_full() {
        let session = open_session(
            TradeOffer::empty().with_escrow_zen(Zen(500_000)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        let (session, wallet, outcome, events) =
            offer_zen(session, Side::Requester, carried(1_999_999_999), Zen(0));
        assert_eq!(outcome, ZenOfferOutcome::WalletFull);
        assert_eq!(wallet, carried(1_999_999_999));
        assert_eq!(
            offers_of(&session).get(Side::Requester).escrow_zen(),
            Zen(500_000)
        );
        assert!(events.is_empty());
    }

    #[test]
    fn an_empty_purse_clears_at_zero_and_rejects_any_positive_raise() {
        let (session, _, outcome, _) =
            offer_zen(TradeSession::opened(), Side::Requester, carried(0), Zen(0));
        assert_eq!(
            outcome,
            ZenOfferOutcome::Offered {
                escrowed: Zen(0),
                wallet: carried(0),
            }
        );
        let (_, _, outcome, _) = offer_zen(session, Side::Requester, carried(0), Zen(1));
        assert_eq!(
            outcome,
            ZenOfferOutcome::Unaffordable { wallet: carried(0) }
        );
    }

    #[test]
    fn offer_zen_gates_the_requested_phase_and_the_locked_side() {
        let (_, _, outcome, _) =
            offer_zen(TradeSession::Requested, Side::Requester, carried(0), Zen(0));
        assert_eq!(outcome, ZenOfferOutcome::NotOpen);
        let locked = open_session(
            TradeOffer::empty(),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
        );
        let (_, _, outcome, _) = offer_zen(locked, Side::Requester, carried(100), Zen(50));
        assert_eq!(outcome, ZenOfferOutcome::SideLocked);
    }

    // --- Lock / unlock / edit-reset. -------------------------------------------

    #[test]
    fn the_first_lock_marks_the_acting_side_and_mirrors_honestly() {
        let (requester, partner, result, events) = lock(
            TradeSession::opened(),
            Side::Requester,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        assert_eq!(
            result,
            LockResult::Locked {
                session: open_session(
                    TradeOffer::empty(),
                    TradeOffer::empty(),
                    TradeLocks::OneLocked {
                        side: Side::Requester,
                    },
                ),
            }
        );
        assert_eq!(requester, holdings(bag(), 0));
        assert_eq!(partner, holdings(bag(), 0));
        assert_eq!(
            events,
            vec![TradeEvent::Locked {
                by: Side::Requester
            }]
        );
    }

    #[test]
    fn locking_an_already_locked_side_is_a_named_noop() {
        let locked = open_session(
            TradeOffer::empty(),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
        );
        let (_, _, result, events) = lock(
            locked.clone(),
            Side::Requester,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        assert_eq!(result, LockResult::AlreadyLocked { session: locked });
        assert!(events.is_empty());
    }

    #[test]
    fn lock_in_the_requested_phase_is_not_open() {
        let (_, _, result, _) = lock(
            TradeSession::Requested,
            Side::Requester,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        assert_eq!(
            result,
            LockResult::NotOpen {
                session: TradeSession::Requested
            }
        );
    }

    #[test]
    fn unlock_mirrors_unlocked_never_locked() {
        let locked = open_session(
            TradeOffer::empty(),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
        );
        let (session, outcome, events) = unlock(locked, Side::Requester);
        assert_eq!(outcome, UnlockOutcome::Unlocked);
        assert_eq!(locks_of(&session), TradeLocks::NeitherLocked);
        assert_eq!(
            events,
            vec![TradeEvent::Unlocked {
                by: Side::Requester
            }]
        );
    }

    #[test]
    fn unlock_by_a_side_that_holds_no_lock_is_already_unlocked() {
        let (session, outcome, events) = unlock(TradeSession::opened(), Side::Requester);
        assert_eq!(outcome, UnlockOutcome::AlreadyUnlocked);
        assert_eq!(session, TradeSession::opened());
        assert!(events.is_empty());
        let partner_locked = open_session(
            TradeOffer::empty(),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (session, outcome, _) = unlock(partner_locked.clone(), Side::Requester);
        assert_eq!(outcome, UnlockOutcome::AlreadyUnlocked);
        assert_eq!(session, partner_locked);
        let (_, outcome, _) = unlock(TradeSession::Requested, Side::Requester);
        assert_eq!(outcome, UnlockOutcome::NotOpen);
    }

    #[test]
    fn every_content_edit_resets_the_partner_lock_and_warns() {
        let partner_locked = || {
            open_session(
                offer_with_item(1, footprint(1, 1), cell(0, 0)),
                TradeOffer::empty(),
                TradeLocks::OneLocked {
                    side: Side::Partner,
                },
            )
        };
        let stored = bag().place(cell(0, 0), footprint(1, 1), item(2)).unwrap();
        let (session, _, _, events) = offer_item(
            partner_locked(),
            Side::Requester,
            stored,
            cell(0, 0),
            cell(2, 2),
        );
        assert_eq!(locks_of(&session), TradeLocks::NeitherLocked);
        assert!(events.contains(&TradeEvent::DealChanged {
            by: Side::Requester
        }));
        let (session, _, _, events) =
            withdraw_item(partner_locked(), Side::Requester, cell(0, 0), bag());
        assert_eq!(locks_of(&session), TradeLocks::NeitherLocked);
        assert!(events.contains(&TradeEvent::DealChanged {
            by: Side::Requester
        }));
        let (session, _, _, events) =
            offer_zen(partner_locked(), Side::Requester, carried(1_000), Zen(500));
        assert_eq!(locks_of(&session), TradeLocks::NeitherLocked);
        assert!(events.contains(&TradeEvent::DealChanged {
            by: Side::Requester
        }));
    }

    // --- Completion (the second lock). ------------------------------------------

    fn feasible_session(locks: TradeLocks) -> TradeSession {
        open_session(
            offer_with_item(1, footprint(2, 2), cell(0, 0)).with_escrow_zen(Zen(400_000)),
            offer_with_item(2, footprint(1, 3), cell(0, 0)).with_escrow_zen(Zen(100_000)),
            locks,
        )
    }

    #[test]
    fn the_second_lock_crosses_every_item_and_all_zen_at_once() {
        let session = feasible_session(TradeLocks::OneLocked {
            side: Side::Partner,
        });
        let (requester, partner, result, events) = lock(
            session,
            Side::Requester,
            holdings(bag(), 600_000),
            holdings(bag(), 900_000),
        );
        assert_eq!(result, LockResult::Completed);
        assert_eq!(events, vec![TradeEvent::Completed]);
        assert_eq!(
            requester.inventory.occupant(cell(0, 0)).unwrap().item,
            item(2)
        );
        assert_eq!(requester.wallet, carried(700_000));
        assert_eq!(
            partner.inventory.occupant(cell(0, 0)).unwrap().item,
            item(1)
        );
        assert_eq!(partner.wallet, carried(1_300_000));
    }

    #[test]
    fn the_full_batch_proof_fails_two_items_that_fit_alone_but_not_together() {
        let partner_offer = TradeOffer::empty().with_window(
            TradeWindow::empty()
                .place(cell(0, 0), footprint(2, 2), item(1))
                .unwrap()
                .place(cell(0, 2), footprint(2, 2), item(2))
                .unwrap(),
        );
        let session = open_session(
            TradeOffer::empty(),
            partner_offer,
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        // Each 2x2 item alone fits the empty 2x2 bag; the batch does not.
        let (_, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(Inventory::empty(2, 2), 0),
            holdings(bag(), 0),
        );
        let LockResult::Bounced { proof, .. } = result else {
            panic!("expected a bounce");
        };
        assert_eq!(
            proof,
            BouncedProof::Requester {
                failure: SideFailure::ItemsDoNotFit
            }
        );
    }

    #[test]
    fn a_full_receiver_bounces_to_open_with_everything_intact() {
        let session = open_session(
            TradeOffer::empty(),
            offer_with_item(1, footprint(1, 1), cell(0, 0)),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let full = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(9))
            .unwrap();
        let requester_in = holdings(full, 0);
        let partner_in = holdings(bag(), 0);
        let (requester, partner, result, events) = lock(
            session.clone(),
            Side::Requester,
            requester_in.clone(),
            partner_in.clone(),
        );
        let LockResult::Bounced {
            session: bounced,
            proof,
        } = result
        else {
            panic!("expected a bounce");
        };
        assert_eq!(
            proof,
            BouncedProof::Requester {
                failure: SideFailure::ItemsDoNotFit
            }
        );
        assert_eq!(locks_of(&bounced), TradeLocks::NeitherLocked);
        assert_eq!(offers_of(&bounced), offers_of(&session));
        assert_eq!(requester, requester_in);
        assert_eq!(partner, partner_in);
        assert!(events.is_empty());
    }

    #[test]
    fn a_credit_past_the_carry_cap_bounces_with_the_wallet_unchanged() {
        let session = open_session(
            TradeOffer::empty(),
            TradeOffer::empty().with_escrow_zen(Zen(1_000_000_000)),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (requester, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(bag(), 1_500_000_000),
            holdings(bag(), 0),
        );
        let LockResult::Bounced { proof, .. } = result else {
            panic!("expected a bounce");
        };
        assert_eq!(
            proof,
            BouncedProof::Requester {
                failure: SideFailure::WalletWouldOverflow
            }
        );
        assert_eq!(requester.wallet, carried(1_500_000_000));
    }

    #[test]
    fn a_cap_edge_credit_landing_exactly_on_the_cap_completes() {
        let session = open_session(
            TradeOffer::empty(),
            TradeOffer::empty().with_escrow_zen(Zen(1)),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (requester, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(bag(), 1_999_999_999),
            holdings(bag(), 0),
        );
        assert_eq!(result, LockResult::Completed);
        assert_eq!(requester.wallet, carried(2_000_000_000));
    }

    #[test]
    fn a_side_failing_both_axes_reports_items_and_wallet() {
        let session = open_session(
            TradeOffer::empty(),
            offer_with_item(1, footprint(2, 2), cell(0, 0)).with_escrow_zen(Zen(1_000_000_000)),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let full = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(9))
            .unwrap();
        let (_, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(full, 1_500_000_000),
            holdings(bag(), 0),
        );
        let LockResult::Bounced { proof, .. } = result else {
            panic!("expected a bounce");
        };
        assert_eq!(
            proof,
            BouncedProof::Requester {
                failure: SideFailure::ItemsAndWallet
            }
        );
    }

    #[test]
    fn both_sides_failing_names_both_sides_and_both_reasons() {
        let session = open_session(
            TradeOffer::empty().with_escrow_zen(Zen(1_000_000_000)),
            offer_with_item(1, footprint(2, 2), cell(0, 0)),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let full = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(9))
            .unwrap();
        let (_, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(full, 0),
            holdings(bag(), 1_500_000_000),
        );
        let LockResult::Bounced { proof, .. } = result else {
            panic!("expected a bounce");
        };
        assert_eq!(
            proof,
            BouncedProof::Both {
                requester: SideFailure::ItemsDoNotFit,
                partner: SideFailure::WalletWouldOverflow,
            }
        );
    }

    #[test]
    fn empty_windows_and_zero_offers_complete_as_a_noop() {
        let session = open_session(
            TradeOffer::empty(),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (requester, partner, result, _) = lock(
            session,
            Side::Requester,
            holdings(bag(), 123),
            holdings(bag(), 456),
        );
        assert_eq!(result, LockResult::Completed);
        assert_eq!(requester, holdings(bag(), 123));
        assert_eq!(partner, holdings(bag(), 456));
    }

    #[test]
    fn a_one_sided_gift_and_a_pure_zen_trade_complete() {
        let gift = open_session(
            offer_with_item(5, footprint(1, 1), cell(0, 0)),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (requester, partner, result, _) = lock(
            gift,
            Side::Requester,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        assert_eq!(result, LockResult::Completed);
        assert!(requester.inventory.placed().is_empty());
        assert_eq!(
            partner.inventory.occupant(cell(0, 0)).unwrap().item,
            item(5)
        );
        let pure_zen = open_session(
            TradeOffer::empty().with_escrow_zen(Zen(250_000)),
            TradeOffer::empty(),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let (requester, partner, result, _) = lock(
            pure_zen,
            Side::Requester,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        assert_eq!(result, LockResult::Completed);
        assert!(partner.inventory.placed().is_empty());
        assert_eq!(partner.wallet, carried(250_000));
        assert_eq!(requester.wallet, carried(0));
    }

    #[test]
    fn two_locks_complete_order_independently() {
        let run = |first: Side, second: Side| {
            let session = feasible_session(TradeLocks::NeitherLocked);
            let (_, _, result, _) = lock(
                session,
                first,
                holdings(bag(), 600_000),
                holdings(bag(), 900_000),
            );
            let LockResult::Locked { session } = result else {
                panic!("the first lock must not complete");
            };
            lock(
                session,
                second,
                holdings(bag(), 600_000),
                holdings(bag(), 900_000),
            )
        };
        let (req_a, par_a, result_a, _) = run(Side::Requester, Side::Partner);
        let (req_b, par_b, result_b, _) = run(Side::Partner, Side::Requester);
        assert_eq!(result_a, LockResult::Completed);
        assert_eq!(result_b, LockResult::Completed);
        assert_eq!(req_a, req_b);
        assert_eq!(par_a, par_b);
    }

    #[test]
    fn a_bounced_trade_relocks_and_completes_after_the_bag_frees_up() {
        let session = open_session(
            TradeOffer::empty(),
            offer_with_item(1, footprint(1, 1), cell(0, 0)),
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        );
        let full = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(9))
            .unwrap();
        let (_, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(full, 0),
            holdings(bag(), 0),
        );
        let LockResult::Bounced { session, .. } = result else {
            panic!("expected a bounce");
        };
        // Escrow intact after the bounce: a re-lock pair with a freed bag
        // completes the same cross.
        let (_, _, result, _) = lock(
            session,
            Side::Partner,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        let LockResult::Locked { session } = result else {
            panic!("the re-lock must mark one side");
        };
        let (requester, _, result, _) = lock(
            session,
            Side::Requester,
            holdings(bag(), 0),
            holdings(bag(), 0),
        );
        assert_eq!(result, LockResult::Completed);
        assert_eq!(
            requester.inventory.occupant(cell(0, 0)).unwrap().item,
            item(1)
        );
    }

    // --- Cancel (total settlement). ----------------------------------------------

    #[test]
    fn an_explicit_cancel_returns_every_escrow_to_its_owner() {
        let session = feasible_session(TradeLocks::NeitherLocked);
        let (settlement, events) = cancel(
            session,
            CancelReason::Explicit,
            holdings(bag(), 600_000),
            holdings(bag(), 900_000),
        );
        assert_eq!(
            settlement
                .requester
                .inventory
                .occupant(cell(0, 0))
                .unwrap()
                .item,
            item(1)
        );
        assert_eq!(settlement.requester.wallet, carried(1_000_000));
        assert_eq!(settlement.requester.overflow, Overflow::empty());
        assert_eq!(
            settlement
                .partner
                .inventory
                .occupant(cell(0, 0))
                .unwrap()
                .item,
            item(2)
        );
        assert_eq!(settlement.partner.wallet, carried(1_000_000));
        assert_eq!(settlement.partner.overflow, Overflow::empty());
        assert_eq!(
            events,
            vec![TradeEvent::Cancelled {
                reason: CancelReason::Explicit
            }]
        );
    }

    #[test]
    fn cancel_returns_escrow_for_every_reason_including_death() {
        for reason in [
            CancelReason::Explicit,
            CancelReason::Declined,
            CancelReason::Disconnected,
            CancelReason::Died,
            CancelReason::HostPolicy,
        ] {
            let session = feasible_session(TradeLocks::OneLocked {
                side: Side::Partner,
            });
            let (settlement, events) = cancel(
                session,
                reason,
                holdings(bag(), 600_000),
                holdings(bag(), 900_000),
            );
            assert_eq!(settlement.requester.wallet, carried(1_000_000));
            assert_eq!(settlement.partner.wallet, carried(1_000_000));
            assert_eq!(settlement.requester.inventory.placed().len(), 1);
            assert_eq!(settlement.partner.inventory.placed().len(), 1);
            assert_eq!(events, vec![TradeEvent::Cancelled { reason }]);
        }
    }

    #[test]
    fn cancelling_a_requested_session_passes_the_containers_through() {
        let stored = bag().place(cell(4, 4), footprint(1, 1), item(3)).unwrap();
        let (settlement, events) = cancel(
            TradeSession::Requested,
            CancelReason::Declined,
            holdings(stored.clone(), 42),
            holdings(bag(), 7),
        );
        assert_eq!(settlement.requester.inventory, stored);
        assert_eq!(settlement.requester.wallet, carried(42));
        assert_eq!(settlement.requester.overflow, Overflow::empty());
        assert_eq!(settlement.partner.inventory, bag());
        assert_eq!(settlement.partner.wallet, carried(7));
        assert_eq!(settlement.partner.overflow, Overflow::empty());
        assert_eq!(
            events,
            vec![TradeEvent::Cancelled {
                reason: CancelReason::Declined
            }]
        );
    }

    #[test]
    fn escrow_that_no_longer_fits_or_credits_rides_the_overflow() {
        let session = open_session(
            offer_with_item(1, footprint(2, 2), cell(0, 0)).with_escrow_zen(Zen(500_000)),
            TradeOffer::empty(),
            TradeLocks::NeitherLocked,
        );
        // A mid-trade pickup filled the hole and the wallet.
        let full = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(9))
            .unwrap();
        let (settlement, _) = cancel(
            session,
            CancelReason::Died,
            holdings(full.clone(), 1_999_999_999),
            holdings(bag(), 0),
        );
        assert_eq!(settlement.requester.inventory, full);
        assert_eq!(settlement.requester.wallet, carried(1_999_999_999));
        assert_eq!(settlement.requester.overflow.items, vec![item(1)]);
        assert_eq!(settlement.requester.overflow.zen, Zen(500_000));
    }

    #[test]
    fn every_cancel_conserves_items_and_zen() {
        let requester_offer = TradeOffer::empty()
            .with_window(
                TradeWindow::empty()
                    .place(cell(0, 0), footprint(1, 1), item(1))
                    .unwrap()
                    .place(cell(1, 0), footprint(1, 1), item(2))
                    .unwrap(),
            )
            .with_escrow_zen(Zen(300_000));
        let partner_offer = TradeOffer::empty()
            .with_window(
                TradeWindow::empty()
                    .place(cell(0, 0), footprint(1, 1), item(3))
                    .unwrap()
                    .place(cell(1, 0), footprint(1, 1), item(4))
                    .unwrap(),
            )
            .with_escrow_zen(Zen(100_000));
        let session = open_session(requester_offer, partner_offer, TradeLocks::NeitherLocked);
        let (settlement, _) = cancel(
            session,
            CancelReason::Disconnected,
            holdings(bag(), 700_000),
            holdings(bag(), 900_000),
        );
        let landed = |side: &SettledSide| side.inventory.placed().len() + side.overflow.items.len();
        assert_eq!(landed(&settlement.requester), 2);
        assert_eq!(landed(&settlement.partner), 2);
        // Zen conserved: wallet credit plus overflow equals the escrow.
        assert_eq!(
            settlement.requester.wallet.get() + settlement.requester.overflow.zen.0,
            700_000 + 300_000
        );
        assert_eq!(
            settlement.partner.wallet.get() + settlement.partner.overflow.zen.0,
            900_000 + 100_000
        );
    }

    // --- Wire round-trips for the service-owned types. ----------------------------

    #[test]
    fn trade_availability_round_trips_every_kind() {
        assert_eq!(
            serde_json::to_string(&TradeAvailability::SameCharacter).unwrap(),
            r#"{"kind":"same_character"}"#
        );
        for target in [
            TradeAvailability::SameCharacter,
            TradeAvailability::Busy,
            TradeAvailability::Dead,
            TradeAvailability::Available {
                position: pos(3, 4),
            },
        ] {
            let json = serde_json::to_string(&target).unwrap();
            assert_eq!(
                serde_json::from_str::<TradeAvailability>(&json).unwrap(),
                target
            );
        }
    }

    #[test]
    fn request_and_accept_outcomes_round_trip() {
        for outcome in [
            RequestOutcome::Opened {
                session: TradeSession::Requested,
            },
            RequestOutcome::Rejected {
                reason: RequestRejection::PartnerBusy,
            },
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<RequestOutcome>(&json).unwrap(),
                outcome
            );
        }
        for outcome in [
            AcceptOutcome::Accepted {
                session: TradeSession::opened(),
            },
            AcceptOutcome::WrongSide {
                session: TradeSession::Requested,
            },
            AcceptOutcome::OutOfRange {
                session: TradeSession::Requested,
            },
            AcceptOutcome::NotRequested {
                session: TradeSession::opened(),
            },
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<AcceptOutcome>(&json).unwrap(),
                outcome
            );
        }
    }

    #[test]
    fn lock_result_and_holdings_round_trip() {
        assert_eq!(
            serde_json::to_string(&LockResult::Completed).unwrap(),
            r#"{"kind":"completed"}"#
        );
        for result in [
            LockResult::Locked {
                session: TradeSession::opened(),
            },
            LockResult::AlreadyLocked {
                session: TradeSession::opened(),
            },
            LockResult::NotOpen {
                session: TradeSession::Requested,
            },
            LockResult::Bounced {
                session: TradeSession::opened(),
                proof: BouncedProof::Both {
                    requester: SideFailure::ItemsDoNotFit,
                    partner: SideFailure::WalletWouldOverflow,
                },
            },
            LockResult::Completed,
        ] {
            let json = serde_json::to_string(&result).unwrap();
            assert_eq!(serde_json::from_str::<LockResult>(&json).unwrap(), result);
        }
        let holdings = holdings(bag(), 250_000);
        let json = serde_json::to_string(&holdings).unwrap();
        assert_eq!(serde_json::from_str::<Holdings>(&json).unwrap(), holdings);
    }
}
