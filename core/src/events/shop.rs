//! The outcome events of the NPC-shop services: buy, sell, repair, and
//! repair-all. One kind-tagged outcome enum per decision — the
//! [`crate::events::inventory`] peer-enum grain, never an umbrella sum —
//! because each decision's payload differs: a buy carries its server-chosen
//! destination, a sell its proceeds, a repair-all a per-slot report. Balances
//! ride the success variants as [`CarriedZen`]; buy failures are bare (a buy
//! materializes a fresh instance only on success, so no move-only item ever
//! needs handing back). The zen-pickup outcome lives in the inventory service
//! — it hands a whole entity back, and an event never imports an entity.

use serde::{Deserialize, Serialize};

use crate::components::equipment::EquipmentSlot;
use crate::components::inventory::Cell;
use crate::components::units::{CarriedZen, Zen};

/// What a buy produced, kind-tagged. The bought item lives in the returned
/// inventory at the reported anchor; the successful variants carry the
/// post-debit balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuyOutcome {
    /// A fresh copy was placed at the server-chosen anchor.
    NewItem {
        /// The anchor cell the copy was placed at.
        at: Cell,
        /// The balance after the debit.
        balance: CarriedZen,
    },
    /// The whole shelf pack merged onto the existing stack anchored at `at` —
    /// a first-class success, never a failure signal.
    Merged {
        /// The anchor cell of the stack that absorbed the pack.
        at: Cell,
        /// The balance after the debit.
        balance: CarriedZen,
    },
    /// The buyer was outside the merchant's interaction reach.
    OutOfRange,
    /// The addressed slot anchors no entry — empty, or a covered non-anchor
    /// cell of a multi-cell entry.
    UnknownShelfSlot,
    /// No fitting anchor exists for the item's footprint on the new-item path.
    InventoryFull,
    /// The balance is below the price — on either buy path.
    InsufficientZen,
}

/// What a sell produced, kind-tagged. On [`Self::Sold`] the item was
/// destroyed by value inside the service — merchants never resell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SellOutcome {
    /// The item was destroyed and the proceeds credited. Zero-price sales
    /// still take this arm.
    Sold {
        /// The credited proceeds.
        proceeds: Zen,
        /// The balance after the credit.
        balance: CarriedZen,
    },
    /// The buyer was outside the merchant's interaction reach.
    OutOfRange,
    /// No item the sale can price covers the addressed cell.
    NoItemAtCell,
    /// Crediting the proceeds would overflow the carry cap; the item is kept
    /// and the inventory untouched.
    WalletFull,
}

/// What a single repair produced, kind-tagged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepairOutcome {
    /// The gauge was restored to full and `cost` debited.
    Repaired {
        /// The debited repair price.
        cost: Zen,
        /// The balance after the debit.
        balance: CarriedZen,
    },
    /// The item was already at full durability — no charge.
    AlreadyFull,
    /// The addressed item carries no repair path: a stack, a consumable,
    /// ammo, a pet, a jewel.
    NotRepairableKind,
    /// The addressed slot or cell holds nothing — no charge.
    Empty,
    /// An at-NPC repair with no merchant in reach; never on a self-repair.
    OutOfRange,
    /// The balance is below the repair price.
    InsufficientZen,
}

/// What a repair-all produced, kind-tagged. Range is a whole-interaction
/// precondition checked once before the walk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepairAllOutcome {
    /// An at-NPC repair-all with no merchant in reach; the walk never ran.
    OutOfRange,
    /// The slot walk ran: every walked slot's result in the classic order,
    /// stopping after the first unaffordable slot with earlier repairs kept,
    /// and the final balance.
    Walked {
        /// The walked slots' results, in walk order.
        slots: Vec<SlotRepair>,
        /// The balance after every kept repair.
        balance: CarriedZen,
    },
}

/// One slot's result in a repair-all walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRepair {
    /// The walked equipment slot.
    pub slot: EquipmentSlot,
    /// What happened at that slot.
    pub result: SlotRepairResult,
}

/// The result at one walked slot, kind-tagged. A not-repairable-kind arm is
/// absent by design: the walk skips `Pet`, and every other worn slot
/// structurally holds a repairable kind (ammo cannot be worn).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotRepairResult {
    /// Restored to full, `cost` debited.
    Repaired {
        /// The debited repair price.
        cost: Zen,
    },
    /// Already at full durability — no charge.
    AlreadyFull,
    /// The slot held nothing to price — no charge.
    Empty,
    /// The slot's repair price exceeds the running balance; the walk stops
    /// here with earlier repairs kept. Always the last entry when present.
    Unaffordable {
        /// The unpayable repair price.
        cost: Zen,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buy_outcome_wire_pins() {
        assert_eq!(
            serde_json::to_string(&BuyOutcome::NewItem {
                at: Cell { row: 0, col: 0 },
                balance: CarriedZen::new(250_000).unwrap(),
            })
            .unwrap(),
            r#"{"kind":"new_item","at":{"row":0,"col":0},"balance":250000}"#
        );
        assert_eq!(
            serde_json::to_string(&BuyOutcome::Merged {
                at: Cell { row: 2, col: 5 },
                balance: CarriedZen::new(980).unwrap(),
            })
            .unwrap(),
            r#"{"kind":"merged","at":{"row":2,"col":5},"balance":980}"#
        );
        assert_eq!(
            serde_json::to_string(&BuyOutcome::UnknownShelfSlot).unwrap(),
            r#"{"kind":"unknown_shelf_slot"}"#
        );
    }

    #[test]
    fn buy_outcome_round_trips_every_kind() {
        for outcome in [
            BuyOutcome::NewItem {
                at: Cell { row: 1, col: 2 },
                balance: CarriedZen::new(10).unwrap(),
            },
            BuyOutcome::Merged {
                at: Cell { row: 1, col: 2 },
                balance: CarriedZen::new(10).unwrap(),
            },
            BuyOutcome::OutOfRange,
            BuyOutcome::UnknownShelfSlot,
            BuyOutcome::InventoryFull,
            BuyOutcome::InsufficientZen,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(serde_json::from_str::<BuyOutcome>(&json).unwrap(), outcome);
        }
    }

    #[test]
    fn sell_outcome_wire_pins_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&SellOutcome::Sold {
                proceeds: Zen(380),
                balance: CarriedZen::new(250_380).unwrap(),
            })
            .unwrap(),
            r#"{"kind":"sold","proceeds":380,"balance":250380}"#
        );
        for outcome in [
            SellOutcome::Sold {
                proceeds: Zen(0),
                balance: CarriedZen::new(0).unwrap(),
            },
            SellOutcome::OutOfRange,
            SellOutcome::NoItemAtCell,
            SellOutcome::WalletFull,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(serde_json::from_str::<SellOutcome>(&json).unwrap(), outcome);
        }
    }

    #[test]
    fn repair_outcome_wire_pins_and_round_trips() {
        assert_eq!(
            serde_json::to_string(&RepairOutcome::Repaired {
                cost: Zen(46),
                balance: CarriedZen::new(954).unwrap(),
            })
            .unwrap(),
            r#"{"kind":"repaired","cost":46,"balance":954}"#
        );
        for outcome in [
            RepairOutcome::Repaired {
                cost: Zen(1),
                balance: CarriedZen::new(0).unwrap(),
            },
            RepairOutcome::AlreadyFull,
            RepairOutcome::NotRepairableKind,
            RepairOutcome::Empty,
            RepairOutcome::OutOfRange,
            RepairOutcome::InsufficientZen,
        ] {
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(
                serde_json::from_str::<RepairOutcome>(&json).unwrap(),
                outcome
            );
        }
    }

    #[test]
    fn repair_all_outcome_wire_pin_and_round_trip() {
        let walked = RepairAllOutcome::Walked {
            slots: vec![
                SlotRepair {
                    slot: EquipmentSlot::LeftHand,
                    result: SlotRepairResult::Repaired { cost: Zen(46) },
                },
                SlotRepair {
                    slot: EquipmentSlot::RightHand,
                    result: SlotRepairResult::AlreadyFull,
                },
                SlotRepair {
                    slot: EquipmentSlot::Helm,
                    result: SlotRepairResult::Empty,
                },
                SlotRepair {
                    slot: EquipmentSlot::Armor,
                    result: SlotRepairResult::Unaffordable { cost: Zen(400) },
                },
            ],
            balance: CarriedZen::new(10).unwrap(),
        };
        assert_eq!(
            serde_json::to_string(&walked).unwrap(),
            r#"{"kind":"walked","slots":[{"slot":"left_hand","result":{"kind":"repaired","cost":46}},{"slot":"right_hand","result":{"kind":"already_full"}},{"slot":"helm","result":{"kind":"empty"}},{"slot":"armor","result":{"kind":"unaffordable","cost":400}}],"balance":10}"#
        );
        let json = serde_json::to_string(&walked).unwrap();
        assert_eq!(
            serde_json::from_str::<RepairAllOutcome>(&json).unwrap(),
            walked
        );
        assert_eq!(
            serde_json::to_string(&RepairAllOutcome::OutOfRange).unwrap(),
            r#"{"kind":"out_of_range"}"#
        );
    }
}
