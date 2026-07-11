//! The set of maps a character has physically set foot on. Data only: an
//! ordered, deduplicated membership over [`MapNumber`] with monotone insertion —
//! no rules. The cross-field invariant ("the current map is always a member")
//! lives at the [`crate::entities::character::Character`] parse gate, not here.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::components::units::MapNumber;

/// The maps a character has discovered by arriving on them. Serialized
/// transparently as a flat, sorted, deduplicated array of bare map numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DiscoveredMaps(BTreeSet<MapNumber>);

impl DiscoveredMaps {
    /// A set holding exactly one map — the seed a fresh or legacy character
    /// gets: its own placement map, the real "no discoveries recorded yet"
    /// value.
    #[must_use]
    pub fn single(map: MapNumber) -> Self {
        Self(BTreeSet::from([map]))
    }

    /// Whether `map` has been discovered.
    #[must_use]
    pub fn contains(&self, map: MapNumber) -> bool {
        self.0.contains(&map)
    }

    /// This set with `map` inserted — monotone and idempotent: inserting a
    /// member returns an equal set.
    #[must_use]
    pub fn inserted(mut self, map: MapNumber) -> Self {
        self.0.insert(map);
        self
    }

    /// Every discovered map, in map-number order.
    pub fn iter(&self) -> impl Iterator<Item = MapNumber> + '_ {
        self.0.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_holds_exactly_its_seed_map() {
        let set = DiscoveredMaps::single(MapNumber(3));
        assert!(set.contains(MapNumber(3)));
        assert!(!set.contains(MapNumber(0)));
        assert_eq!(set.iter().collect::<Vec<_>>(), vec![MapNumber(3)]);
    }

    #[test]
    fn insertion_is_monotone_and_idempotent() {
        let set = DiscoveredMaps::single(MapNumber(0));
        let grown = set.inserted(MapNumber(4));
        assert!(grown.contains(MapNumber(0)));
        assert!(grown.contains(MapNumber(4)));
        // Re-inserting a member returns an equal set — no duplicate, no churn.
        assert_eq!(grown.clone().inserted(MapNumber(4)), grown);
        assert_eq!(grown.clone().inserted(MapNumber(0)), grown);
    }

    #[test]
    fn wire_is_a_flat_sorted_array_of_bare_map_numbers() {
        let set = DiscoveredMaps::single(MapNumber(4))
            .inserted(MapNumber(0))
            .inserted(MapNumber(2));
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, "[0,2,4]");
        assert_eq!(serde_json::from_str::<DiscoveredMaps>(&json).unwrap(), set);
    }

    #[test]
    fn a_duplicated_wire_entry_parses_into_one_membership() {
        let set: DiscoveredMaps = serde_json::from_str("[2,2,0]").unwrap();
        assert_eq!(serde_json::to_string(&set).unwrap(), "[0,2]");
    }
}
