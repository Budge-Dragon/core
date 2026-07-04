//! An item lying on the ground: a rolled instance at a world position on a map,
//! with the tick it despawns at. It pairs [`WorldPos`] with [`MapNumber`]
//! directly — a ground item has no facing or movement, so it carries no
//! [`crate::components::placement::Placement`], honoring "a position never
//! travels without its map." Not `Copy`: it holds the move-only
//! [`ItemInstance`], so a rejected pickup must hand the whole world item back.

use serde::{Deserialize, Serialize};

use crate::components::item_instance::ItemInstance;
use crate::components::spatial::WorldPos;
use crate::components::units::{MapNumber, Tick};

/// An item on the ground: which instance, where it lies, on which map, and when
/// it despawns. Plain data.
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

    #[test]
    fn wire_round_trips() {
        let world_item = WorldItem {
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
        };
        let json = serde_json::to_string(&world_item).unwrap();
        assert_eq!(
            serde_json::from_str::<WorldItem>(&json).unwrap(),
            world_item
        );
    }
}
