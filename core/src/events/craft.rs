//! The outcome event of a chaos-machine mix. One kind-tagged enum: rejected
//! (nothing consumed), failed (fee kept, per-family fates), or succeeded
//! (created item plus untouched returns). Every input instance lands in
//! exactly one outcome position or is consumed by value inside the service —
//! silent item loss is unrepresentable by shape.

use serde::{Deserialize, Serialize};

use crate::components::item_instance::ItemInstance;
use crate::components::item_ref::ItemRef;
use crate::components::units::{CarriedZen, Zen};

/// The one outcome of a chaos-machine mix, kind-tagged. `zen` on the charged
/// variants is the new balance after the fee — the fee is charged up front and
/// never refunded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MixOutcome {
    /// Nothing consumed, no fee; every input handed back.
    Rejected {
        /// Why the mix was rejected.
        reason: RejectReason,
        /// Every placed item, handed back untouched.
        items: Vec<ItemInstance>,
    },
    /// The success roll failed: the fee is kept and each input's fate is
    /// reported as a value; downgraded and returned instances are handed back.
    Failed {
        /// The fee that was charged and kept.
        fee: Zen,
        /// The new zen balance after the fee.
        zen: CarriedZen,
        /// Each input's fate — every input appears exactly once.
        casualties: Vec<Casualty>,
    },
    /// The roll succeeded: `created` is the new or in-place upgraded instance
    /// (move-only — it exists exactly once, here); `returned` are the inputs
    /// the recipe left untouched; consumed ingredients appear in neither.
    Success {
        /// The fee that was charged and kept.
        fee: Zen,
        /// The new zen balance after the fee.
        zen: CarriedZen,
        /// The crafted (or upgraded-in-place) item.
        created: ItemInstance,
        /// The inputs the recipe left untouched (a ticket family's extras).
        returned: Vec<ItemInstance>,
    },
}

/// Why a mix was rejected — closed, exactly two. Below-minimum counts,
/// leftovers, unequal ticket levels, and a worn horn are all per-recipe attempt
/// failures inside the inference scan, folded into
/// [`RejectReason::NoRecipeMatch`], never distinct wire codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The placed items form no catalog recipe.
    NoRecipeMatch,
    /// A recipe matched but its fee exceeds the balance.
    InsufficientZen,
}

/// One input's fate on a failed mix, kind-tagged — the three fates are
/// mutually exclusive by shape. A destroyed instance is consumed by value, so
/// only its identity survives as a report: carrying [`ItemRef`] (not the live
/// instance) makes "re-storing a destroyed item" unrepresentable at the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Casualty {
    /// Consumed by value; only the identity survives as a report.
    Destroyed {
        /// The destroyed item's identity.
        item: ItemRef,
    },
    /// Handed back at a lower level (a chaos-weapon or first-wings sacrifice).
    Downgraded {
        /// The downgraded instance, handed back.
        item: ItemInstance,
    },
    /// Handed back untouched (a ticket family's ignored extras).
    Returned {
        /// The untouched instance, handed back.
        item: ItemInstance,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::units::ItemLevel;

    fn instance() -> ItemInstance {
        ItemInstance {
            item: ItemRef {
                group: 12,
                number: 15,
            },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(1),
            augment: CraftedAugment::None,
        }
    }

    #[test]
    fn rejected_round_trips_with_both_reasons() {
        for reason in [RejectReason::NoRecipeMatch, RejectReason::InsufficientZen] {
            let outcome = MixOutcome::Rejected {
                reason,
                items: vec![instance()],
            };
            let json = serde_json::to_string(&outcome).unwrap();
            assert_eq!(serde_json::from_str::<MixOutcome>(&json).unwrap(), outcome);
        }
        assert_eq!(
            serde_json::to_string(&RejectReason::NoRecipeMatch).unwrap(),
            r#""no_recipe_match""#
        );
        assert_eq!(
            serde_json::to_string(&RejectReason::InsufficientZen).unwrap(),
            r#""insufficient_zen""#
        );
    }

    #[test]
    fn failed_round_trips_with_every_casualty_kind() {
        let outcome = MixOutcome::Failed {
            fee: Zen(250_000),
            zen: CarriedZen::new(750_000).unwrap(),
            casualties: vec![
                Casualty::Destroyed {
                    item: ItemRef {
                        group: 12,
                        number: 15,
                    },
                },
                Casualty::Downgraded { item: instance() },
                Casualty::Returned { item: instance() },
            ],
        };
        let json = serde_json::to_string(&outcome).unwrap();
        assert!(
            json.contains(r#""fee":250000,"zen":750000"#),
            "the balance stays a bare integer on the wire"
        );
        assert_eq!(serde_json::from_str::<MixOutcome>(&json).unwrap(), outcome);
    }

    #[test]
    fn success_round_trips() {
        let outcome = MixOutcome::Success {
            fee: Zen(5_000_000),
            zen: CarriedZen::new(0).unwrap(),
            created: instance(),
            returned: vec![instance()],
        };
        let json = serde_json::to_string(&outcome).unwrap();
        assert_eq!(serde_json::from_str::<MixOutcome>(&json).unwrap(), outcome);
    }
}
