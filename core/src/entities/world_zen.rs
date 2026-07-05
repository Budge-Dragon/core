//! A pile of zen lying on the ground: a drop amount at a world position on a
//! map, with the tick it despawns at. It pairs [`WorldPos`] with [`MapNumber`]
//! directly — a ground pile has no facing or movement, so it carries no
//! [`crate::components::placement::Placement`], honoring "a position never
//! travels without its map." `Clone`, deliberately not `Copy`, even though
//! every field is `Copy`: pickup consumes the pile by value and a rejected
//! pickup must hand the only copy back — a `Copy` derive would let a host
//! double-credit from a retained original.

use serde::{Deserialize, Serialize};

use crate::components::spatial::WorldPos;
use crate::components::units::{MapNumber, Tick, Zen};

/// A pile of zen on the ground: how much, where it lies, on which map, and
/// when it despawns. Plain data. `amount` is a [`Zen`] drop amount, not a
/// balance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldZen {
    /// The dropped amount.
    pub amount: Zen,
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

    #[test]
    fn wire_round_trips() {
        let world_zen = WorldZen {
            amount: Zen(40_000),
            position: WorldPos::clamped(163_840, 229_376),
            map: MapNumber(0),
            despawn: Tick(1200),
        };
        let json = serde_json::to_string(&world_zen).unwrap();
        assert_eq!(serde_json::from_str::<WorldZen>(&json).unwrap(), world_zen);
    }
}
