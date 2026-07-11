//! An item lying on the ground: a rolled instance at a world position on a
//! map, with the tick it despawns at and its kill-locked ownership window. It
//! pairs [`WorldPos`] with [`MapNumber`] directly — a ground item has no
//! facing or movement, so it carries no
//! [`crate::components::placement::Placement`], honoring "a position never
//! travels without its map." Not `Copy`: it holds the move-only
//! [`ItemInstance`], so a rejected pickup must hand the whole world item back.

use serde::{Deserialize, Serialize};

use crate::components::drop_claim::DropClaim;
use crate::components::item_instance::ItemInstance;
use crate::components::spatial::WorldPos;
use crate::components::units::{MapNumber, Tick};

/// An item on the ground: which instance, where it lies, on which map, when it
/// despawns, and who holds its claim. Plain data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldItem {
    /// The rolled item instance lying on the ground.
    pub instance: ItemInstance,
    /// Where it lies.
    pub position: WorldPos,
    /// The map it lies on.
    pub map: MapNumber,
    /// The tick at which it despawns.
    pub despawn: Tick,
    /// The kill-locked ownership window (or `Unclaimed`).
    pub claim: DropClaim,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::spatial::WorldPos;
    use crate::components::units::ItemLevel;

    fn ground_item(claim: DropClaim) -> WorldItem {
        WorldItem {
            instance: ItemInstance {
                item: ItemRef {
                    group: 0,
                    number: 3,
                },
                level: ItemLevel::ZERO,
                roll: RarityRoll::Normal,
                normal_option: None,
                luck: LuckRoll::Plain,
                skill: SkillRoll::NoSkill,
                durability: Durability::full(30),
                augment: CraftedAugment::None,
            },
            position: WorldPos::clamped(163_840, 229_376),
            map: MapNumber(0),
            despawn: Tick(1200),
            claim,
        }
    }

    #[test]
    fn wire_round_trips() {
        let world_item = ground_item(DropClaim::Claimed { until: Tick(720) });
        let json = serde_json::to_string(&world_item).unwrap();
        assert!(json.contains(r#""claim":{"kind":"claimed","until":720}"#));
        assert!(json.contains(r#""despawn":1200"#));
        assert_eq!(
            serde_json::from_str::<WorldItem>(&json).unwrap(),
            world_item
        );
    }

    #[test]
    fn both_claim_variants_round_trip_on_the_world_item() {
        for claim in [
            DropClaim::Claimed { until: Tick(720) },
            DropClaim::Unclaimed,
        ] {
            let world_item = ground_item(claim);
            let json = serde_json::to_string(&world_item).unwrap();
            assert_eq!(
                serde_json::from_str::<WorldItem>(&json).unwrap(),
                world_item
            );
        }
    }
}
