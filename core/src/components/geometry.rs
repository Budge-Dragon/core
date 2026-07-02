//! Map-grid geometry shared by every position-carrying schema.

use serde::{Deserialize, Serialize};

/// A single map tile on the 256x256 grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Point {
    /// X coordinate.
    pub x: u8,
    /// Y coordinate.
    pub y: u8,
}

/// Inclusive rectangle in map coordinates with edge order proven at
/// construction (`x1 <= x2`, `y1 <= y2`) — the Gate.txt / MonsterSetBase
/// area columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RectWire", into = "RectWire")]
pub struct Rect {
    x1: u8,
    y1: u8,
    x2: u8,
    y2: u8,
}

impl Rect {
    /// Builds an edge-ordered rectangle.
    ///
    /// # Errors
    /// Returns [`GeometryError::RectEdgesOutOfOrder`] when `x1 > x2` or
    /// `y1 > y2`.
    pub fn new(x1: u8, y1: u8, x2: u8, y2: u8) -> Result<Self, GeometryError> {
        if x1 > x2 || y1 > y2 {
            return Err(GeometryError::RectEdgesOutOfOrder { x1, y1, x2, y2 });
        }
        Ok(Self { x1, y1, x2, y2 })
    }

    /// Left edge.
    #[must_use]
    pub fn x1(self) -> u8 {
        self.x1
    }

    /// Top edge.
    #[must_use]
    pub fn y1(self) -> u8 {
        self.y1
    }

    /// Right edge.
    #[must_use]
    pub fn x2(self) -> u8 {
        self.x2
    }

    /// Bottom edge.
    #[must_use]
    pub fn y2(self) -> u8 {
        self.y2
    }

    /// Whether the point lies inside the inclusive bounds.
    #[must_use]
    pub fn contains(self, point: Point) -> bool {
        self.x1 <= point.x && point.x <= self.x2 && self.y1 <= point.y && point.y <= self.y2
    }
}

/// Wire shape of [`Rect`]; ordering is checked on the way in.
#[derive(Serialize, Deserialize)]
struct RectWire {
    x1: u8,
    y1: u8,
    x2: u8,
    y2: u8,
}

impl TryFrom<RectWire> for Rect {
    type Error = GeometryError;

    fn try_from(wire: RectWire) -> Result<Self, Self::Error> {
        Self::new(wire.x1, wire.y1, wire.x2, wire.y2)
    }
}

impl From<Rect> for RectWire {
    fn from(rect: Rect) -> Self {
        Self {
            x1: rect.x1,
            y1: rect.y1,
            x2: rect.x2,
            y2: rect.y2,
        }
    }
}

/// Rejection of malformed geometry at the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryError {
    /// Rectangle edges out of order.
    RectEdgesOutOfOrder {
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

impl core::fmt::Display for GeometryError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::RectEdgesOutOfOrder { x1, y1, x2, y2 } => {
                write!(f, "rect edges out of order: ({x1},{y1})..({x2},{y2})")
            }
        }
    }
}

/// A map footprint that classic files express as either one tile or an area
/// (MonsterSetBase single-spot rows vs area rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Footprint {
    /// A single tile.
    Spot {
        /// The tile.
        at: Point,
    },
    /// An inclusive rectangle.
    Area {
        /// The area.
        rect: Rect,
    },
}

/// Eight-way compass facing. Serialized by name; core assigns no wire
/// ordinals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_rejects_reversed_edges() {
        assert!(Rect::new(10, 0, 5, 0).is_err());
        assert!(Rect::new(0, 10, 0, 5).is_err());
        assert!(Rect::new(5, 5, 5, 5).is_ok());
    }

    #[test]
    fn rect_exposes_ordered_edges() {
        let rect = Rect::new(5, 10, 20, 30).unwrap();
        assert_eq!(
            (rect.x1(), rect.y1(), rect.x2(), rect.y2()),
            (5, 10, 20, 30)
        );
    }

    #[test]
    fn rect_contains_is_inclusive() {
        let rect = Rect::new(5, 5, 10, 10).unwrap();
        assert!(rect.contains(Point { x: 5, y: 5 }));
        assert!(rect.contains(Point { x: 10, y: 10 }));
        assert!(rect.contains(Point { x: 7, y: 8 }));
        assert!(!rect.contains(Point { x: 4, y: 7 }));
        assert!(!rect.contains(Point { x: 7, y: 11 }));
    }

    #[test]
    fn rect_serde_rejects_reversed_edges() {
        let ok = r#"{"x1":1,"y1":2,"x2":3,"y2":4}"#;
        let rect: Rect = serde_json::from_str(ok).unwrap();
        assert_eq!(rect, Rect::new(1, 2, 3, 4).unwrap());
        let bad = r#"{"x1":9,"y1":2,"x2":3,"y2":4}"#;
        assert!(serde_json::from_str::<Rect>(bad).is_err());
    }

    #[test]
    fn footprint_kind_tags_on_the_wire() {
        let spot = Footprint::Spot {
            at: Point { x: 121, y: 232 },
        };
        let json = serde_json::to_string(&spot).unwrap();
        assert_eq!(json, r#"{"kind":"spot","at":{"x":121,"y":232}}"#);
        let round: Footprint = serde_json::from_str(&json).unwrap();
        assert_eq!(round, spot);
    }

    #[test]
    fn direction_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&Direction::SouthWest).unwrap(),
            r#""south_west""#
        );
        assert_eq!(
            serde_json::from_str::<Direction>(r#""north_east""#).unwrap(),
            Direction::NorthEast
        );
    }
}
