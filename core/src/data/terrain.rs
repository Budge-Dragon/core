//! Host-filled terrain sidecar port: raw per-map walkability bytes, length-
//! checked at the load boundary into a fixed-size holder so downstream parsing
//! into a [`WalkGrid`](crate::components::tile::WalkGrid) is total.
//!
//! Wire contract — the sole source of truth for the byte format the host fills:
//! each of the `256 x 256` bytes is one tile's attribute set at index
//! `y*256 + x`, in the dataset's on-disk layout `SafeZone 0x01`, `NoMove 0x02`,
//! `NoGround 0x04`, `Water 0x08`. A tile is walkable iff neither `NoMove` nor
//! `NoGround` is set; `Water` is traversable
//! (see [`WalkGrid::from_terrain`](crate::components::tile::WalkGrid::from_terrain)).

use crate::components::tile::TERRAIN_LEN;

use super::common::MapNumber;

/// One map's raw terrain-attribute bytes, length proven by type at
/// construction — the sole parse boundary for a sidecar's size.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerrainBytes(Box<[u8; TERRAIN_LEN]>);

impl TerrainBytes {
    /// Wraps a raw byte buffer, rejecting any length other than [`TERRAIN_LEN`].
    ///
    /// # Errors
    /// Returns [`TerrainError::WrongLength`] when `bytes.len() != TERRAIN_LEN`.
    pub fn new(bytes: Vec<u8>) -> Result<Self, TerrainError> {
        match <Box<[u8; TERRAIN_LEN]>>::try_from(bytes.into_boxed_slice()) {
            Ok(array) => Ok(Self(array)),
            Err(slice) => Err(TerrainError::WrongLength { len: slice.len() }),
        }
    }

    /// The raw attribute bytes, length proven by type.
    #[must_use]
    pub fn as_array(&self) -> &[u8; TERRAIN_LEN] {
        &self.0
    }
}

/// A map paired with its raw terrain bytes — the host fills one per map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapTerrain {
    /// Map the bytes describe.
    pub map: MapNumber,
    /// Raw `256 x 256` attribute bytes.
    pub bytes: TerrainBytes,
}

/// Rejection of a malformed terrain sidecar at the load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainError {
    /// The buffer was not exactly [`TERRAIN_LEN`] bytes.
    WrongLength {
        /// The actual length received.
        len: usize,
    },
}

impl core::fmt::Display for TerrainError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongLength { len } => {
                write!(f, "terrain sidecar has {len} bytes, expected {TERRAIN_LEN}")
            }
        }
    }
}

impl core::error::Error for TerrainError {}
