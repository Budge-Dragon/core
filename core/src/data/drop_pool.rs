//! The per-level drop index over the droppable item definitions, built once at
//! load so drop resolution range-queries the eligible pool instead of
//! linear-scanning every item on every kill.

use std::collections::BTreeMap;

use crate::components::interval::Interval;
use crate::data::common::ItemRef;
use crate::data::item_definitions::ItemDefinition;

/// Droppable items grouped by their base drop level, built once at load so drop
/// resolution range-queries the eligible pool (`O(log n)` plus the matches)
/// instead of linear-scanning every item definition on every kill. An item
/// enters the pool only when it drops from monsters; the classic per-level drop
/// index (OpenMU's `DropItemGroup`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropPool {
    by_drop_level: BTreeMap<u8, Vec<ItemRef>>,
}

impl DropPool {
    pub(crate) fn build<'a>(items: impl Iterator<Item = &'a ItemDefinition>) -> Self {
        let mut by_drop_level: BTreeMap<u8, Vec<ItemRef>> = BTreeMap::new();
        for item in items {
            if item.drops_from_monsters {
                by_drop_level
                    .entry(item.drop_level)
                    .or_default()
                    .push(item.id);
            }
        }
        Self { by_drop_level }
    }

    /// The droppable items whose base drop level falls in the inclusive window
    /// (a monster's level pool: floor = `monster_level - gap`, ceiling =
    /// `monster_level`). The window is an [`Interval`], so `min <= max` is
    /// proven and the range query never panics; an empty iterator is the
    /// genuine "no eligible item" answer.
    pub fn in_window(&self, window: Interval<u8>) -> impl Iterator<Item = ItemRef> + '_ {
        self.by_drop_level
            .range(window.min()..=window.max())
            .flat_map(|(_, refs)| refs.iter().copied())
    }
}
