//! Classic MU tile authoring vocabulary and its projection into world space.
//!
//! These `u8`x`u8` tile types survive only as the data-authoring unit; they are
//! retired as live entity types. Conversion to world space is the sole
//! sanctioned integer-to-integer boundary — never through a float, never on a
//! live entity. This module depends on [`crate::components::spatial`] and never
//! the reverse: the permanent world types stay unaware that tiles exist.

use serde::{Deserialize, Serialize};

use crate::components::spatial::{HALF_TILE, TILE_SHIFT, UNITS_PER_TILE, WorldPos, WorldRect};

/// A single map tile on the 256x256 grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TileCoord {
    x: u8,
    y: u8,
}

impl TileCoord {
    /// Builds a tile coordinate. Total — every `u8` pair is a valid tile.
    #[must_use]
    pub const fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }

    /// The x coordinate.
    #[must_use]
    pub const fn x(self) -> u8 {
        self.x
    }

    /// The y coordinate.
    #[must_use]
    pub const fn y(self) -> u8 {
        self.y
    }

    /// Projects to the world position at the tile centre.
    #[must_use]
    pub fn to_world(self) -> WorldPos {
        WorldPos::clamped(
            i64::from(self.x)
                .saturating_mul(UNITS_PER_TILE)
                .saturating_add(HALF_TILE),
            i64::from(self.y)
                .saturating_mul(UNITS_PER_TILE)
                .saturating_add(HALF_TILE),
        )
    }

    /// The tile a world position falls in (floor of each component). Total —
    /// components are in `[0, WORLD_EXTENT]`, so the shift floors into
    /// `[0, 255]`.
    #[must_use]
    pub fn from_world(pos: WorldPos) -> Self {
        let tx = (pos.x().raw() >> TILE_SHIFT).clamp(0, 255);
        let ty = (pos.y().raw() >> TILE_SHIFT).clamp(0, 255);
        Self {
            x: narrow_u8(tx),
            y: narrow_u8(ty),
        }
    }
}

/// Wire shape of [`TileArea`]; edge ordering is checked on the way in.
#[derive(Serialize, Deserialize)]
struct TileAreaWire {
    x1: u8,
    y1: u8,
    x2: u8,
    y2: u8,
}

/// An inclusive rectangle in tile coordinates with edge order proven at
/// construction (`x1 <= x2`, `y1 <= y2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "TileAreaWire", into = "TileAreaWire")]
pub struct TileArea {
    x1: u8,
    y1: u8,
    x2: u8,
    y2: u8,
}

impl TileArea {
    /// Builds an edge-ordered tile rectangle.
    ///
    /// # Errors
    /// Returns [`TileError::AreaInverted`] when `x1 > x2` or `y1 > y2`.
    pub fn new(x1: u8, y1: u8, x2: u8, y2: u8) -> Result<Self, TileError> {
        if x1 > x2 || y1 > y2 {
            return Err(TileError::AreaInverted { x1, y1, x2, y2 });
        }
        Ok(Self { x1, y1, x2, y2 })
    }

    /// Left edge.
    #[must_use]
    pub const fn x1(self) -> u8 {
        self.x1
    }

    /// Top edge.
    #[must_use]
    pub const fn y1(self) -> u8 {
        self.y1
    }

    /// Right edge.
    #[must_use]
    pub const fn x2(self) -> u8 {
        self.x2
    }

    /// Bottom edge.
    #[must_use]
    pub const fn y2(self) -> u8 {
        self.y2
    }

    /// Whether a tile lies inside the inclusive bounds.
    #[must_use]
    pub fn contains(self, tile: TileCoord) -> bool {
        self.x1 <= tile.x && tile.x <= self.x2 && self.y1 <= tile.y && tile.y <= self.y2
    }

    /// Projects to the world rectangle covering every whole cell in the area.
    #[must_use]
    pub fn to_world(self) -> WorldRect {
        let min = WorldPos::clamped(
            i64::from(self.x1).saturating_mul(UNITS_PER_TILE),
            i64::from(self.y1).saturating_mul(UNITS_PER_TILE),
        );
        let max = WorldPos::clamped(
            i64::from(self.x2)
                .saturating_add(1)
                .saturating_mul(UNITS_PER_TILE),
            i64::from(self.y2)
                .saturating_add(1)
                .saturating_mul(UNITS_PER_TILE),
        );
        WorldRect::spanning(min, max)
    }
}

impl TryFrom<TileAreaWire> for TileArea {
    type Error = TileError;

    fn try_from(wire: TileAreaWire) -> Result<Self, Self::Error> {
        Self::new(wire.x1, wire.y1, wire.x2, wire.y2)
    }
}

impl From<TileArea> for TileAreaWire {
    fn from(area: TileArea) -> Self {
        Self {
            x1: area.x1,
            y1: area.y1,
            x2: area.x2,
            y2: area.y2,
        }
    }
}

/// A map footprint that classic files express as either one tile or an area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Footprint {
    /// A single tile.
    Spot {
        /// The tile.
        at: TileCoord,
    },
    /// An inclusive rectangle.
    Area {
        /// The area.
        rect: TileArea,
    },
}

/// Eight-way compass facing — an authoring unit only. Serialized by name; core
/// assigns no wire ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TileFacing {
    /// West.
    West,
    /// South-west.
    SouthWest,
    /// South.
    South,
    /// South-east.
    SouthEast,
    /// East.
    East,
    /// North-east.
    NorthEast,
    /// North.
    North,
    /// North-west.
    NorthWest,
}

/// Rejection of malformed tile geometry at the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileError {
    /// Tile-area edges out of order.
    AreaInverted {
        /// Left edge.
        x1: u8,
        /// Top edge.
        y1: u8,
        /// Right edge.
        x2: u8,
        /// Bottom edge.
        y2: u8,
    },
}

impl core::fmt::Display for TileError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::AreaInverted { x1, y1, x2, y2 } => {
                write!(f, "tile area edges out of order: ({x1},{y1})..({x2},{y2})")
            }
        }
    }
}

impl core::error::Error for TileError {}

/// A per-tile walkability bitset over the 256x256 grid — one bit per tile.
pub struct WalkGrid([u64; 1024]);

impl WalkGrid {
    /// Builds a walk grid from its raw 1024-word bitset.
    #[must_use]
    pub const fn from_words(words: [u64; 1024]) -> Self {
        Self(words)
    }

    /// Whether a tile is walkable. Total — an out-of-word index answers `false`.
    #[must_use]
    pub fn walkable(&self, tile: TileCoord) -> bool {
        let bit = (u32::from(tile.y) << 8) | u32::from(tile.x);
        let word = match usize::try_from(bit >> 6) {
            Ok(index) => index,
            Err(_) => usize::MAX,
        };
        let mask = 1u64 << (bit & 63);
        self.0.get(word).is_some_and(|w| w & mask != 0)
    }
}

fn narrow_u8(value: i64) -> u8 {
    match u8::try_from(value) {
        Ok(v) => v,
        Err(_) => {
            if value < 0 {
                u8::MIN
            } else {
                u8::MAX
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::spatial::WORLD_EXTENT;

    #[test]
    fn tile_to_world_centre_anchor() {
        assert_eq!(
            TileCoord::new(2, 3).to_world(),
            WorldPos::clamped(163_840, 229_376)
        );
    }

    #[test]
    fn extreme_tile_stays_in_bounds() {
        let corner = TileCoord::new(255, 255).to_world();
        assert!(corner.x().raw() <= WORLD_EXTENT);
        assert!(corner.y().raw() <= WORLD_EXTENT);
        assert_eq!(corner.x().raw(), 16_744_448);
    }

    #[test]
    fn tile_world_round_trip_over_all_tiles() {
        for x in 0u8..=255 {
            for y in 0u8..=255 {
                let tile = TileCoord::new(x, y);
                assert_eq!(TileCoord::from_world(tile.to_world()), tile);
            }
        }
    }

    #[test]
    fn tile_area_to_world_covers_whole_cells() {
        let area = TileArea::new(0, 0, 1, 1).unwrap();
        let rect = area.to_world();
        assert_eq!(rect.min(), WorldPos::clamped(0, 0));
        assert_eq!(
            rect.max(),
            WorldPos::clamped(2 * UNITS_PER_TILE, 2 * UNITS_PER_TILE)
        );
    }

    #[test]
    fn tile_area_rejects_inverted() {
        assert!(TileArea::new(10, 0, 5, 0).is_err());
        assert!(TileArea::new(5, 5, 5, 5).is_ok());
        let bad = r#"{"x1":9,"y1":2,"x2":3,"y2":4}"#;
        assert!(serde_json::from_str::<TileArea>(bad).is_err());
    }

    #[test]
    fn tile_area_contains_is_inclusive() {
        let area = TileArea::new(5, 5, 10, 10).unwrap();
        assert!(area.contains(TileCoord::new(5, 5)));
        assert!(area.contains(TileCoord::new(10, 10)));
        assert!(!area.contains(TileCoord::new(4, 7)));
    }

    #[test]
    fn footprint_kind_tags_on_the_wire() {
        let spot = Footprint::Spot {
            at: TileCoord::new(121, 232),
        };
        let json = serde_json::to_string(&spot).unwrap();
        assert_eq!(json, r#"{"kind":"spot","at":{"x":121,"y":232}}"#);
        assert_eq!(serde_json::from_str::<Footprint>(&json).unwrap(), spot);
    }

    #[test]
    fn tile_facing_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&TileFacing::SouthWest).unwrap(),
            r#""south_west""#
        );
        assert_eq!(
            serde_json::from_str::<TileFacing>(r#""north_east""#).unwrap(),
            TileFacing::NorthEast
        );
    }

    #[test]
    fn tile_coord_wire_round_trip() {
        let json = serde_json::to_string(&TileCoord::new(121, 232)).unwrap();
        assert_eq!(json, r#"{"x":121,"y":232}"#);
        assert_eq!(
            serde_json::from_str::<TileCoord>(&json).unwrap(),
            TileCoord::new(121, 232)
        );
    }

    #[test]
    fn walk_grid_walkable_is_total() {
        let mut words = [0u64; 1024];
        // Set the bit for tile (1, 0): bit index 1.
        words[0] = 0b10;
        let grid = WalkGrid::from_words(words);
        assert!(grid.walkable(TileCoord::new(1, 0)));
        assert!(!grid.walkable(TileCoord::new(0, 0)));
        assert!(!grid.walkable(TileCoord::new(255, 255)));
    }

    #[test]
    fn conversions_are_deterministic() {
        let a = TileCoord::new(7, 9).to_world();
        let b = TileCoord::new(7, 9).to_world();
        assert_eq!(a, b);
    }
}
