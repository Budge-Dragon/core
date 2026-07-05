//! A live player↔player trade — core's first two-party aggregate. Plain serde
//! data, host-persisted between intents (the host owns the session registry;
//! core never holds it). Per-phase data lives on its variant only: a requested
//! trade has no windows, no escrow, and no locks, so offering before the
//! partner accepts is unrepresentable — there is no window field to place
//! into. All behavior lives in [`crate::services::trade`].

use serde::{Deserialize, Serialize};

use crate::components::trade_window::{Side, TradeWindow};
use crate::components::units::Zen;

/// The trade pair's lifecycle phase and its per-phase data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TradeSession {
    /// Asymmetric: the partner has not accepted. Holds nothing — the two side
    /// labels are structural and positions arrive per intent.
    Requested,
    /// Symmetric: both sides may offer. Two offers plus the pair lock state.
    Open {
        /// The two sides' escrowed offers.
        offers: TradeOffers,
        /// The pair lock handshake state.
        locks: TradeLocks,
    },
}

impl TradeSession {
    /// A freshly opened trade: empty windows, zero escrow, neither side
    /// locked — real domain values, not fabricated defaults.
    #[must_use]
    pub fn opened() -> Self {
        Self::Open {
            offers: TradeOffers::empty(),
            locks: TradeLocks::NeitherLocked,
        }
    }
}

/// The two sides' offers, addressed by [`Side`] — a total structure: every
/// side has an offer, so `get` returns a value, never an `Option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeOffers {
    requester: TradeOffer,
    partner: TradeOffer,
}

impl TradeOffers {
    /// Both sides empty — the freshly opened pair.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            requester: TradeOffer::empty(),
            partner: TradeOffer::empty(),
        }
    }

    /// The addressed side's offer — total, no default.
    #[must_use]
    pub fn get(&self, side: Side) -> &TradeOffer {
        match side {
            Side::Requester => &self.requester,
            Side::Partner => &self.partner,
        }
    }

    /// This pair with `side`'s offer replaced — value-in/value-out.
    #[must_use]
    pub fn with(self, side: Side, offer: TradeOffer) -> Self {
        match side {
            Side::Requester => Self {
                requester: offer,
                ..self
            },
            Side::Partner => Self {
                partner: offer,
                ..self
            },
        }
    }
}

/// One side's escrowed goods: the window items and the offered zen. Zero zen
/// is a real domain value — nothing offered — never an `Option`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TradeOffer {
    window: TradeWindow,
    escrow_zen: Zen,
}

impl TradeOffer {
    /// An empty offer: an empty window and zero escrowed zen.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            window: TradeWindow::empty(),
            escrow_zen: Zen(0),
        }
    }

    /// The escrowed window.
    #[must_use]
    pub fn window(&self) -> &TradeWindow {
        &self.window
    }

    /// The escrowed zen amount.
    #[must_use]
    pub fn escrow_zen(&self) -> Zen {
        self.escrow_zen
    }

    /// This offer with its window replaced — value-in/value-out.
    #[must_use]
    pub fn with_window(self, window: TradeWindow) -> Self {
        Self { window, ..self }
    }

    /// This offer with its escrowed zen replaced — value-in/value-out.
    #[must_use]
    pub fn with_escrow_zen(self, escrow_zen: Zen) -> Self {
        Self { escrow_zen, ..self }
    }
}

/// The pair lock handshake. Two variants make "both locked" unrepresentable:
/// the second lock completes atomically, so a both-locked value never rests —
/// the structural fix for the coupled-machine deadlock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TradeLocks {
    /// Neither side has locked.
    NeitherLocked,
    /// Exactly one side has locked.
    OneLocked {
        /// The locked side.
        side: Side,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::inventory::{Cell, Footprint};
    use crate::components::item_instance::{
        CraftedAugment, Durability, ItemInstance, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
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

    #[test]
    fn requested_carries_no_fields_on_the_wire() {
        assert_eq!(
            serde_json::to_string(&TradeSession::Requested).unwrap(),
            r#"{"kind":"requested"}"#
        );
        assert_eq!(
            serde_json::from_str::<TradeSession>(r#"{"kind":"requested"}"#).unwrap(),
            TradeSession::Requested
        );
    }

    #[test]
    fn opened_starts_with_empty_windows_zero_escrow_neither_locked() {
        let TradeSession::Open { offers, locks } = TradeSession::opened() else {
            panic!("opened() must be Open");
        };
        assert_eq!(locks, TradeLocks::NeitherLocked);
        assert!(offers.get(Side::Requester).window().placed().is_empty());
        assert!(offers.get(Side::Partner).window().placed().is_empty());
        assert_eq!(offers.get(Side::Requester).escrow_zen(), Zen(0));
        assert_eq!(offers.get(Side::Partner).escrow_zen(), Zen(0));
    }

    #[test]
    fn offers_with_replaces_only_the_addressed_side() {
        let offers =
            TradeOffers::empty().with(Side::Partner, TradeOffer::empty().with_escrow_zen(Zen(500)));
        assert_eq!(offers.get(Side::Partner).escrow_zen(), Zen(500));
        assert_eq!(offers.get(Side::Requester).escrow_zen(), Zen(0));
    }

    #[test]
    fn trade_locks_wire_pins() {
        assert_eq!(
            serde_json::to_string(&TradeLocks::NeitherLocked).unwrap(),
            r#"{"kind":"neither_locked"}"#
        );
        assert_eq!(
            serde_json::to_string(&TradeLocks::OneLocked {
                side: Side::Requester
            })
            .unwrap(),
            r#"{"kind":"one_locked","side":"requester"}"#
        );
        for locks in [
            TradeLocks::NeitherLocked,
            TradeLocks::OneLocked {
                side: Side::Requester,
            },
            TradeLocks::OneLocked {
                side: Side::Partner,
            },
        ] {
            let json = serde_json::to_string(&locks).unwrap();
            assert_eq!(serde_json::from_str::<TradeLocks>(&json).unwrap(), locks);
        }
    }

    #[test]
    fn a_live_open_session_round_trips_windows_and_escrow() {
        let window = TradeWindow::empty()
            .place(
                Cell { row: 0, col: 0 },
                Footprint::new(2, 2).unwrap(),
                item(3),
            )
            .unwrap();
        let session = TradeSession::Open {
            offers: TradeOffers::empty()
                .with(
                    Side::Requester,
                    TradeOffer::empty()
                        .with_window(window)
                        .with_escrow_zen(Zen(400_000)),
                )
                .with(
                    Side::Partner,
                    TradeOffer::empty().with_escrow_zen(Zen(100_000)),
                ),
            locks: TradeLocks::OneLocked {
                side: Side::Partner,
            },
        };
        let json = serde_json::to_string(&session).unwrap();
        let reparsed = serde_json::from_str::<TradeSession>(&json).unwrap();
        assert_eq!(reparsed, session);
    }
}
