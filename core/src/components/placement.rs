//! Where a mobile entity stands and which way it faces: its world position, its
//! heading, and how it crosses the ground plane. A plain named grouping — no
//! cross-field invariant — composed from the spatial and movement vocabulary.

use serde::{Deserialize, Serialize};

use crate::components::movement::Movement;
use crate::components::spatial::{Facing, WorldPos};
use crate::components::units::MapNumber;

/// A mobile entity's world placement: position, facing, traversal mode, and the
/// map its position is framed in — a [`WorldPos`] never travels without its map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Placement {
    /// Where the entity stands.
    pub position: WorldPos,
    /// Which way it faces.
    pub facing: Facing,
    /// How it crosses the ground plane.
    pub movement: Movement,
    /// Which map the position is framed in.
    pub map: MapNumber,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::tile::TileCoord;

    #[test]
    fn wire_round_trips() {
        let placement = Placement {
            position: TileCoord::new(2, 3).to_world(),
            facing: Facing::POS_Y,
            movement: Movement::Grounded,
            map: MapNumber(0),
        };
        let json = serde_json::to_string(&placement).unwrap();
        assert_eq!(
            json,
            r#"{"position":{"x":163840,"y":229376},"facing":{"x":0,"y":1},"movement":"grounded","map":0}"#
        );
        assert_eq!(serde_json::from_str::<Placement>(&json).unwrap(), placement);
    }
}
