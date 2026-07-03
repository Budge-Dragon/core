//! Classic MU tile authoring vocabulary and its projection into world space.
//!
//! These `u8`x`u8` tile types survive only as the data-authoring unit; they are
//! retired as live entity types. Conversion to world space is the sole
//! sanctioned integer-to-integer boundary — never through a float, never on a
//! live entity. This module depends on [`crate::components::spatial`] and never
//! the reverse: the permanent world types stay unaware that tiles exist.

use serde::{Deserialize, Serialize};

use crate::components::spatial::{
    Facing, HALF_TILE, TILE_SHIFT, UNITS_PER_TILE, WorldPos, WorldRect,
};

/// Bytes in one map's terrain sidecar: `256 x 256`, one attribute byte per tile.
pub const TERRAIN_LEN: usize = 256 * 256;

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
        Self {
            x: tile_index(pos.x().raw()),
            y: tile_index(pos.y().raw()),
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

impl TileFacing {
    /// Projects the eight-way compass facing to a continuous world [`Facing`].
    ///
    /// Tile-grid axis convention: `+x` runs **east**, `+y` runs **south**
    /// (screen-down), so `South` maps to the `+y` world direction and `North`
    /// to `-y`. Magnitude is irrelevant — the cone test is magnitude-invariant —
    /// so unit steps are used.
    #[must_use]
    pub fn to_facing(self) -> Facing {
        match self {
            Self::East => Facing::POS_X,
            Self::West => Facing::NEG_X,
            Self::South => Facing::POS_Y,
            Self::North => Facing::NEG_Y,
            Self::SouthEast => Facing::POS_X_POS_Y,
            Self::SouthWest => Facing::NEG_X_POS_Y,
            Self::NorthEast => Facing::POS_X_NEG_Y,
            Self::NorthWest => Facing::NEG_X_NEG_Y,
        }
    }
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkGrid([u64; 1024]);

impl WalkGrid {
    /// The two blocking bits of the on-disk terrain layout (documented on
    /// [`crate::data::terrain`]): `NoMove` (`0x02`) and `NoGround` (`0x04`). A
    /// tile is walkable iff neither is set; `SafeZone` (`0x01`) and `Water`
    /// (`0x08`) leave a tile walkable — classic water maps such as Atlans are
    /// fully traversable.
    const BLOCKED_MASK: u8 = 0x06;

    /// Builds a walk grid from its raw 1024-word bitset.
    #[must_use]
    pub const fn from_words(words: [u64; 1024]) -> Self {
        Self(words)
    }

    /// Builds the walk grid from a map's raw `256 x 256` terrain-attribute
    /// array — one byte per tile at index `y*256 + x`, matching [`walkable`]'s
    /// `bit = (y<<8)|x` convention. Total: the fixed array length makes a
    /// wrong-size input unrepresentable, and a tile is walkable iff neither
    /// blocking bit ([`WalkGrid::BLOCKED_MASK`]) is set.
    ///
    /// [`walkable`]: WalkGrid::walkable
    #[must_use]
    pub fn from_terrain(bytes: &[u8; TERRAIN_LEN]) -> Self {
        let mut words = [0u64; 1024];
        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(64)) {
            let mut packed = 0u64;
            for (bit, &attr) in chunk.iter().enumerate() {
                if (attr & Self::BLOCKED_MASK) == 0 {
                    packed |= 1u64 << bit;
                }
            }
            *word = packed;
        }
        Self(words)
    }

    /// Whether the tile containing a world position is walkable — the live,
    /// world-space query. The tile grid is a private implementation detail;
    /// callers never name a tile. Total — every in-world position resolves to a
    /// grid cell (a position is bounded to `[0, WORLD_EXTENT]` by its type).
    #[must_use]
    pub fn walkable(&self, pos: WorldPos) -> bool {
        let tile = TileCoord::from_world(pos);
        let bit = (usize::from(tile.y) << 8) | usize::from(tile.x);
        let mask = 1u64 << (bit & 63);
        self.0.get(bit >> 6).is_some_and(|w| w & mask != 0)
    }

    /// The world positions of every walkable tile whose centre lies inside
    /// `rect`, yielded in deterministic row-major (y then x) order. Pure,
    /// RNG-free, and target-independent, so the same index maps to the same
    /// [`WorldPos`] bit-for-bit on native, wasm, and FFI.
    ///
    /// The rectangle-to-tile arithmetic stays private to this module — callers
    /// receive world positions and never name a tile — so the walk grid keeps
    /// hiding its cells while still answering "which walkable spots are in this
    /// area?" for spawn placement and warp landing.
    pub fn walkable_positions_in(&self, rect: WorldRect) -> impl Iterator<Item = WorldPos> + '_ {
        let min = TileCoord::from_world(rect.min());
        let max = TileCoord::from_world(rect.max());
        (min.y..=max.y)
            .flat_map(move |y| (min.x..=max.x).map(move |x| TileCoord::new(x, y)))
            .map(TileCoord::to_world)
            .filter(move |&centre| rect.contains(centre) && self.walkable(centre))
    }
}

/// The classic tile index a world component falls in: floor to whole tiles,
/// capped at the last tile (255). Total by construction — the shifted value is
/// clamped into `0..=255`, then its low byte is read cast-free (the higher
/// bytes are proven zero by the clamp, mirroring the byte-decomposition narrows
/// in [`crate::components::spatial`]). No fallible narrowing, so no dead
/// saturation arm exists to model an impossible failure.
fn tile_index(raw: i64) -> u8 {
    let capped = (raw >> TILE_SHIFT).clamp(0, i64::from(u8::MAX));
    let [index, ..] = capped.to_le_bytes();
    index
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
        assert!(grid.walkable(TileCoord::new(1, 0).to_world()));
        assert!(!grid.walkable(TileCoord::new(0, 0).to_world()));
        assert!(!grid.walkable(TileCoord::new(255, 255).to_world()));
    }

    #[test]
    fn walk_grid_from_terrain_applies_blocked_mask() {
        let mut bytes = vec![0u8; TERRAIN_LEN];
        // On-disk layout: SafeZone 0x01, NoMove 0x02, NoGround 0x04, Water 0x08.
        // (0,0) open; (1,0) SafeZone -> walkable; (2,0) NoMove -> blocked;
        // (3,0) NoGround -> blocked; (4,0) Water -> walkable; (5,0) both
        // blocking bits -> blocked.
        bytes[1] = 0x01;
        bytes[2] = 0x02;
        bytes[3] = 0x04;
        bytes[4] = 0x08;
        bytes[5] = 0x06;
        let array: &[u8; TERRAIN_LEN] = bytes.as_slice().try_into().unwrap();
        let grid = WalkGrid::from_terrain(array);
        assert!(grid.walkable(TileCoord::new(0, 0).to_world()));
        assert!(grid.walkable(TileCoord::new(1, 0).to_world()));
        assert!(!grid.walkable(TileCoord::new(2, 0).to_world()));
        assert!(!grid.walkable(TileCoord::new(3, 0).to_world()));
        assert!(grid.walkable(TileCoord::new(4, 0).to_world()));
        assert!(!grid.walkable(TileCoord::new(5, 0).to_world()));
    }

    #[test]
    fn walkable_positions_in_yields_walkable_centres_row_major() {
        let mut words = [0u64; 1024];
        for (x, y) in [(5u8, 5u8), (6, 5), (5, 6)] {
            let bit = (usize::from(y) << 8) | usize::from(x);
            words[bit >> 6] |= 1u64 << (bit & 63);
        }
        // (6, 6) stays blocked.
        let grid = WalkGrid::from_words(words);
        let rect = TileArea::new(5, 5, 6, 6).unwrap().to_world();
        let got: Vec<WorldPos> = grid.walkable_positions_in(rect).collect();
        assert_eq!(
            got,
            vec![
                TileCoord::new(5, 5).to_world(),
                TileCoord::new(6, 5).to_world(),
                TileCoord::new(5, 6).to_world(),
            ]
        );
        // Every yielded position is walkable and sits inside the rect.
        assert!(
            got.iter()
                .all(|&pos| grid.walkable(pos) && rect.contains(pos))
        );
    }

    #[test]
    fn walkable_positions_in_is_empty_when_nothing_walkable() {
        let grid = WalkGrid::from_words([0u64; 1024]);
        let rect = TileArea::new(10, 10, 20, 20).unwrap().to_world();
        assert_eq!(grid.walkable_positions_in(rect).count(), 0);
    }

    #[test]
    fn tile_facing_to_facing_is_nonzero_and_axis_aligned() {
        use crate::components::spatial::Facing;
        assert_eq!(TileFacing::South.to_facing(), Facing::POS_Y);
        assert_eq!(TileFacing::North.to_facing(), Facing::NEG_Y);
        assert_eq!(TileFacing::East.to_facing(), Facing::POS_X);
    }

    #[test]
    fn conversions_are_deterministic() {
        let a = TileCoord::new(7, 9).to_world();
        let b = TileCoord::new(7, 9).to_world();
        assert_eq!(a, b);
    }
}
