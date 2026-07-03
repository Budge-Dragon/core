//! Fixed-point 2.5D spatial foundation: the continuous ground plane addressed
//! in exact signed integer sub-units.
//!
//! Positions, offsets, distances, facing, cones, and regions are all integer
//! value types with pure, total, deterministic predicates — no float, no clock,
//! no RNG, no hidden state — so the same answer comes out bit-for-bit on native,
//! wasm, and FFI. Classic tile coordinates project into this space through
//! [`crate::components::tile`], which depends on this module and never the
//! reverse.

use core::num::NonZeroU64;
use core::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

/// Fractional bits per classic tile: the `Q40.24` split in `i64`.
pub const TILE_SHIFT: u32 = 16;
/// Sub-units per classic tile (`2^TILE_SHIFT`).
pub const UNITS_PER_TILE: i64 = 1 << TILE_SHIFT;
/// Half a tile in sub-units — the tile-centre anchor offset.
pub const HALF_TILE: i64 = 1 << (TILE_SHIFT - 1);
/// Inclusive upper bound of a position component: `256` tiles in sub-units.
pub const WORLD_EXTENT: i64 = 256 * UNITS_PER_TILE;
/// Widest constructible [`Radius`] — twice the world extent (the plane's
/// diagonal is well under this, so any in-world range is representable).
pub const RADIUS_MAX: u64 = 2 * WORLD_EXTENT.unsigned_abs();
/// Largest cone denominator — the squared-cosine work stays exact in `i128`.
pub const CONE_DEN_MAX: u64 = 1 << 27;

/// The axis a spatial error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// Horizontal axis.
    X,
    /// Vertical axis.
    Y,
}

/// Rejection of malformed spatial input at the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpatialError {
    /// A position or facing component fell outside its allowed range.
    ComponentOutOfBounds {
        /// Which axis carried the bad component.
        axis: Axis,
        /// The offending raw sub-unit value.
        value: i64,
    },
    /// A rectangle's minimum corner exceeded its maximum on some axis.
    RectInverted {
        /// Minimum x.
        min_x: i64,
        /// Minimum y.
        min_y: i64,
        /// Maximum x.
        max_x: i64,
        /// Maximum y.
        max_y: i64,
    },
    /// A facing was the zero vector, which has no direction.
    ZeroFacing,
    /// A radius exceeded [`RADIUS_MAX`].
    RadiusOutOfBounds {
        /// The offending value.
        value: u64,
    },
    /// A cone's squared-cosine ratio was invalid (`num > den` or
    /// `den > CONE_DEN_MAX`).
    ConeRatioInvalid {
        /// Numerator.
        num: u64,
        /// Denominator.
        den: u64,
    },
}

impl core::fmt::Display for SpatialError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ComponentOutOfBounds { axis, value } => {
                write!(f, "position component out of bounds on {axis:?}: {value}")
            }
            Self::RectInverted {
                min_x,
                min_y,
                max_x,
                max_y,
            } => write!(f, "rect inverted: ({min_x},{min_y})..({max_x},{max_y})"),
            Self::ZeroFacing => write!(f, "facing has no direction (zero vector)"),
            Self::RadiusOutOfBounds { value } => write!(f, "radius out of bounds: {value}"),
            Self::ConeRatioInvalid { num, den } => {
                write!(f, "cone ratio invalid: {num}/{den}")
            }
        }
    }
}

impl core::error::Error for SpatialError {}

/// A fixed-point scalar in `Q40.24` (`2^16` sub-units per tile). The wire form
/// is a bare integer; the value itself is unbounded — bounds live on the
/// position types that embed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(from = "i64", into = "i64")]
pub struct Fixed(i64);

impl Fixed {
    /// Builds a scalar from its raw sub-unit value.
    #[must_use]
    pub const fn from_raw(raw: i64) -> Self {
        Self(raw)
    }

    /// The raw sub-unit value.
    #[must_use]
    pub const fn raw(self) -> i64 {
        self.0
    }

    /// Builds a scalar from whole tiles plus a sub-unit remainder.
    #[must_use]
    pub fn from_tile_parts(whole_tiles: i64, sub_units: i64) -> Self {
        Self((whole_tiles << TILE_SHIFT).saturating_add(sub_units))
    }

    /// Saturating scalar multiply by an integer factor — total and
    /// deterministic; the saturation branch is the defined fallback, never the
    /// live path for in-world values.
    #[must_use]
    pub fn scale(self, k: i64) -> Self {
        Self(self.0.saturating_mul(k))
    }
}

impl Add for Fixed {
    type Output = Fixed;

    fn add(self, other: Fixed) -> Fixed {
        Self(self.0.saturating_add(other.0))
    }
}

impl Sub for Fixed {
    type Output = Fixed;

    fn sub(self, other: Fixed) -> Fixed {
        Self(self.0.saturating_sub(other.0))
    }
}

impl From<i64> for Fixed {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

impl From<Fixed> for i64 {
    fn from(value: Fixed) -> Self {
        value.0
    }
}

/// Private wire mirror shared by every two-component spatial type.
#[derive(Serialize, Deserialize)]
struct Vec2Wire {
    x: i64,
    y: i64,
}

/// A point on the ground plane; each component is in `[0, WORLD_EXTENT]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec2Wire", into = "Vec2Wire")]
pub struct WorldPos {
    x: Fixed,
    y: Fixed,
}

impl WorldPos {
    /// Builds a position, rejecting any component outside `[0, WORLD_EXTENT]`.
    ///
    /// # Errors
    /// Returns [`SpatialError::ComponentOutOfBounds`] for an out-of-range
    /// component.
    pub fn new(x: Fixed, y: Fixed) -> Result<Self, SpatialError> {
        if !in_world(x.0) {
            return Err(SpatialError::ComponentOutOfBounds {
                axis: Axis::X,
                value: x.0,
            });
        }
        if !in_world(y.0) {
            return Err(SpatialError::ComponentOutOfBounds {
                axis: Axis::Y,
                value: y.0,
            });
        }
        Ok(Self { x, y })
    }

    /// Builds a position by clamping each raw component into
    /// `[0, WORLD_EXTENT]`. Total — the compute-path constructor.
    #[must_use]
    pub fn clamped(x: i64, y: i64) -> Self {
        Self {
            x: Fixed(x.clamp(0, WORLD_EXTENT)),
            y: Fixed(y.clamp(0, WORLD_EXTENT)),
        }
    }

    /// The x component.
    #[must_use]
    pub const fn x(self) -> Fixed {
        self.x
    }

    /// The y component.
    #[must_use]
    pub const fn y(self) -> Fixed {
        self.y
    }

    /// Squared Euclidean distance to another position — exact, sqrt-free.
    #[must_use]
    pub fn distance_sq(self, other: WorldPos) -> DistanceSq {
        let dx = u128::from((self.x.0 - other.x.0).unsigned_abs());
        let dy = u128::from((self.y.0 - other.y.0).unsigned_abs());
        DistanceSq(dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy)))
    }

    /// Whether another position is within a radius — the sqrt-free
    /// `distance_sq <= radius^2` test.
    #[must_use]
    pub fn within_range(self, other: WorldPos, radius: Radius) -> bool {
        self.distance_sq(other).0 <= radius.squared().0
    }
}

impl TryFrom<Vec2Wire> for WorldPos {
    type Error = SpatialError;

    fn try_from(wire: Vec2Wire) -> Result<Self, Self::Error> {
        Self::new(Fixed(wire.x), Fixed(wire.y))
    }
}

impl From<WorldPos> for Vec2Wire {
    fn from(pos: WorldPos) -> Self {
        Self {
            x: pos.x.0,
            y: pos.y.0,
        }
    }
}

/// An offset on the ground plane — a difference of positions. Unbounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "Vec2Wire", into = "Vec2Wire")]
pub struct WorldVec {
    x: Fixed,
    y: Fixed,
}

impl WorldVec {
    /// Builds an offset. Total.
    #[must_use]
    pub const fn new(x: Fixed, y: Fixed) -> Self {
        Self { x, y }
    }

    /// The x component.
    #[must_use]
    pub const fn x(self) -> Fixed {
        self.x
    }

    /// The y component.
    #[must_use]
    pub const fn y(self) -> Fixed {
        self.y
    }

    /// Saturating componentwise scale by an integer factor.
    #[must_use]
    pub fn scale(self, k: i64) -> WorldVec {
        Self {
            x: self.x.scale(k),
            y: self.y.scale(k),
        }
    }

    /// Widened dot product, exact in `i128`.
    #[must_use]
    pub fn dot(self, other: WorldVec) -> i128 {
        i128::from(self.x.0)
            .saturating_mul(i128::from(other.x.0))
            .saturating_add(i128::from(self.y.0).saturating_mul(i128::from(other.y.0)))
    }

    /// Squared length, `dot(self, self)`.
    #[must_use]
    pub fn length_sq(self) -> DistanceSq {
        DistanceSq(self.dot(self).unsigned_abs())
    }
}

impl From<Vec2Wire> for WorldVec {
    fn from(wire: Vec2Wire) -> Self {
        Self {
            x: Fixed(wire.x),
            y: Fixed(wire.y),
        }
    }
}

impl From<WorldVec> for Vec2Wire {
    fn from(vec: WorldVec) -> Self {
        Self {
            x: vec.x.0,
            y: vec.y.0,
        }
    }
}

impl Sub<WorldPos> for WorldPos {
    type Output = WorldVec;

    fn sub(self, other: WorldPos) -> WorldVec {
        WorldVec {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl Add<WorldVec> for WorldPos {
    type Output = WorldPos;

    fn add(self, offset: WorldVec) -> WorldPos {
        WorldPos::clamped(
            self.x.0.saturating_add(offset.x.0),
            self.y.0.saturating_add(offset.y.0),
        )
    }
}

impl Add<WorldVec> for WorldVec {
    type Output = WorldVec;

    fn add(self, other: WorldVec) -> WorldVec {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl Sub<WorldVec> for WorldVec {
    type Output = WorldVec;

    fn sub(self, other: WorldVec) -> WorldVec {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

/// A linear range in sub-units. Distinct from the squared [`DistanceSq`] it is
/// compared against, so the two can never be mixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct Radius(u64);

impl Radius {
    /// Builds a radius, rejecting values above [`RADIUS_MAX`].
    ///
    /// # Errors
    /// Returns [`SpatialError::RadiusOutOfBounds`] when `value > RADIUS_MAX`.
    pub fn new(value: u64) -> Result<Self, SpatialError> {
        if value > RADIUS_MAX {
            Err(SpatialError::RadiusOutOfBounds { value })
        } else {
            Ok(Self(value))
        }
    }

    /// The linear value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// The squared radius — the value a [`DistanceSq`] is compared against.
    #[must_use]
    pub fn squared(self) -> DistanceSq {
        DistanceSq(u128::from(self.0).pow(2))
    }
}

impl TryFrom<u64> for Radius {
    type Error = SpatialError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Radius> for u64 {
    fn from(radius: Radius) -> Self {
        radius.0
    }
}

/// A squared distance — a computed return value, never on the wire. Distinct
/// from the linear [`Radius`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DistanceSq(u128);

impl DistanceSq {
    /// The squared value.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

/// A continuous heading as a non-zero direction vector. Set by snapping to a
/// direction (e.g. `target - pos`), never gradually rotated; cone half-widths
/// are `<= 90 deg`. If turn-rate-limited rotation is ever needed, a Binary
/// Angle (BAM) is the canonical form and this type must be revisited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "Vec2Wire", into = "Vec2Wire")]
pub struct Facing(WorldVec);

impl Facing {
    /// Unit facing along `+x`.
    pub const POS_X: Self = Self(WorldVec::new(Fixed::from_raw(1), Fixed::from_raw(0)));
    /// Unit facing along `-x`.
    pub const NEG_X: Self = Self(WorldVec::new(Fixed::from_raw(-1), Fixed::from_raw(0)));
    /// Unit facing along `+y`.
    pub const POS_Y: Self = Self(WorldVec::new(Fixed::from_raw(0), Fixed::from_raw(1)));
    /// Unit facing along `-y`.
    pub const NEG_Y: Self = Self(WorldVec::new(Fixed::from_raw(0), Fixed::from_raw(-1)));
    /// Unit facing along `+x,+y`.
    pub const POS_X_POS_Y: Self = Self(WorldVec::new(Fixed::from_raw(1), Fixed::from_raw(1)));
    /// Unit facing along `+x,-y`.
    pub const POS_X_NEG_Y: Self = Self(WorldVec::new(Fixed::from_raw(1), Fixed::from_raw(-1)));
    /// Unit facing along `-x,+y`.
    pub const NEG_X_POS_Y: Self = Self(WorldVec::new(Fixed::from_raw(-1), Fixed::from_raw(1)));
    /// Unit facing along `-x,-y`.
    pub const NEG_X_NEG_Y: Self = Self(WorldVec::new(Fixed::from_raw(-1), Fixed::from_raw(-1)));

    /// Builds a facing, rejecting the zero vector and out-of-bounds components.
    /// The magnitude is not normalized; the cone test is magnitude-invariant.
    ///
    /// # Errors
    /// Returns [`SpatialError::ZeroFacing`] for the zero vector, or
    /// [`SpatialError::ComponentOutOfBounds`] when `|component| > WORLD_EXTENT`.
    pub fn new(vector: WorldVec) -> Result<Self, SpatialError> {
        if vector.x.0 == 0 && vector.y.0 == 0 {
            return Err(SpatialError::ZeroFacing);
        }
        if !facing_in_bounds(vector.x.0) {
            return Err(SpatialError::ComponentOutOfBounds {
                axis: Axis::X,
                value: vector.x.0,
            });
        }
        if !facing_in_bounds(vector.y.0) {
            return Err(SpatialError::ComponentOutOfBounds {
                axis: Axis::Y,
                value: vector.y.0,
            });
        }
        Ok(Self(vector))
    }

    /// The underlying direction vector.
    #[must_use]
    pub const fn vector(self) -> WorldVec {
        self.0
    }

    /// Whether `target` lies within the frontal cone of half-width `half`
    /// centred on this facing at `apex`. Exact integer squared-cosine test — no
    /// trig, no sqrt, no normalize.
    #[must_use]
    pub fn frontal_contains(&self, apex: WorldPos, target: WorldPos, half: ConeHalfWidth) -> bool {
        let fx = i128::from(self.0.x.0);
        let fy = i128::from(self.0.y.0);
        let vx = i128::from(target.x.0 - apex.x.0);
        let vy = i128::from(target.y.0 - apex.y.0);
        let d = fx.saturating_mul(vx).saturating_add(fy.saturating_mul(vy));
        if d < 0 {
            return false;
        }
        let f_sq = fx.saturating_mul(fx).saturating_add(fy.saturating_mul(fy));
        let v_sq = vx.saturating_mul(vx).saturating_add(vy.saturating_mul(vy));
        let lhs = d
            .saturating_mul(d)
            .saturating_mul(i128::from(half.den_get()));
        let rhs = i128::from(half.num())
            .saturating_mul(f_sq)
            .saturating_mul(v_sq);
        lhs >= rhs
    }
}

impl TryFrom<Vec2Wire> for Facing {
    type Error = SpatialError;

    fn try_from(wire: Vec2Wire) -> Result<Self, Self::Error> {
        Self::new(WorldVec {
            x: Fixed(wire.x),
            y: Fixed(wire.y),
        })
    }
}

impl From<Facing> for Vec2Wire {
    fn from(facing: Facing) -> Self {
        Self {
            x: facing.0.x.0,
            y: facing.0.y.0,
        }
    }
}

/// Private wire mirror of [`ConeHalfWidth`].
#[derive(Serialize, Deserialize)]
struct ConeWire {
    num: u64,
    den: NonZeroU64,
}

/// A cone half-width as an exact squared cosine `num/den` (`cos^2 theta`).
/// Larger angles have smaller `num`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "ConeWire", into = "ConeWire")]
pub struct ConeHalfWidth {
    num: u64,
    den: NonZeroU64,
}

impl ConeHalfWidth {
    /// A 45-degree half-width (`cos^2 45 = 1/2`).
    pub const DEG_45: Self = Self {
        num: 1,
        den: NonZeroU64::MIN.saturating_add(1),
    };
    /// A 90-degree half-width (`cos^2 90 = 0`).
    pub const DEG_90: Self = Self {
        num: 0,
        den: NonZeroU64::MIN,
    };

    /// Builds a cone half-width.
    ///
    /// # Errors
    /// Returns [`SpatialError::ConeRatioInvalid`] when `num > den` or
    /// `den > CONE_DEN_MAX`.
    pub fn new(num: u64, den: NonZeroU64) -> Result<Self, SpatialError> {
        if num > den.get() || den.get() > CONE_DEN_MAX {
            return Err(SpatialError::ConeRatioInvalid {
                num,
                den: den.get(),
            });
        }
        Ok(Self { num, den })
    }

    /// The numerator of `cos^2 theta`.
    #[must_use]
    pub const fn num(self) -> u64 {
        self.num
    }

    /// The denominator of `cos^2 theta`.
    #[must_use]
    pub const fn den_get(self) -> u64 {
        self.den.get()
    }
}

impl TryFrom<ConeWire> for ConeHalfWidth {
    type Error = SpatialError;

    fn try_from(wire: ConeWire) -> Result<Self, Self::Error> {
        Self::new(wire.num, wire.den)
    }
}

impl From<ConeHalfWidth> for ConeWire {
    fn from(cone: ConeHalfWidth) -> Self {
        Self {
            num: cone.num,
            den: cone.den,
        }
    }
}

/// Private wire mirror of [`WorldRect`].
#[derive(Serialize, Deserialize)]
struct RectWire {
    min: WorldPos,
    max: WorldPos,
}

/// An axis-aligned rectangle on the ground plane; `min <= max` on both axes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "RectWire", into = "RectWire")]
pub struct WorldRect {
    min: WorldPos,
    max: WorldPos,
}

impl WorldRect {
    /// Builds a rectangle from ordered corners.
    ///
    /// # Errors
    /// Returns [`SpatialError::RectInverted`] when `min` exceeds `max` on
    /// either axis.
    pub fn new(min: WorldPos, max: WorldPos) -> Result<Self, SpatialError> {
        if min.x.0 > max.x.0 || min.y.0 > max.y.0 {
            return Err(SpatialError::RectInverted {
                min_x: min.x.0,
                min_y: min.y.0,
                max_x: max.x.0,
                max_y: max.y.0,
            });
        }
        Ok(Self { min, max })
    }

    /// Builds the rectangle spanning two corners by sorting componentwise —
    /// total, cannot invert.
    #[must_use]
    pub fn spanning(a: WorldPos, b: WorldPos) -> Self {
        Self {
            min: WorldPos {
                x: Fixed(a.x.0.min(b.x.0)),
                y: Fixed(a.y.0.min(b.y.0)),
            },
            max: WorldPos {
                x: Fixed(a.x.0.max(b.x.0)),
                y: Fixed(a.y.0.max(b.y.0)),
            },
        }
    }

    /// The minimum corner.
    #[must_use]
    pub const fn min(self) -> WorldPos {
        self.min
    }

    /// The maximum corner.
    #[must_use]
    pub const fn max(self) -> WorldPos {
        self.max
    }

    /// Whether a point lies inside the inclusive bounds.
    #[must_use]
    pub fn contains(&self, p: WorldPos) -> bool {
        self.min.x.0 <= p.x.0
            && p.x.0 <= self.max.x.0
            && self.min.y.0 <= p.y.0
            && p.y.0 <= self.max.y.0
    }
}

impl TryFrom<RectWire> for WorldRect {
    type Error = SpatialError;

    fn try_from(wire: RectWire) -> Result<Self, Self::Error> {
        Self::new(wire.min, wire.max)
    }
}

impl From<WorldRect> for RectWire {
    fn from(rect: WorldRect) -> Self {
        Self {
            min: rect.min,
            max: rect.max,
        }
    }
}

/// A spatial query region, kind-tagged. `contains` is total and exhaustive — a
/// new variant breaks the build.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Region {
    /// A disc of `radius` around `center`.
    Circle {
        /// Centre of the disc.
        center: WorldPos,
        /// Disc radius.
        radius: Radius,
    },
    /// An axis-aligned rectangle.
    Rect {
        /// The rectangle.
        rect: WorldRect,
    },
    /// A frontal cone: within `range` of `apex` and inside the facing cone.
    Cone {
        /// Cone apex.
        apex: WorldPos,
        /// Cone facing.
        facing: Facing,
        /// Cone half-width.
        half_width: ConeHalfWidth,
        /// Maximum range from the apex.
        range: Radius,
    },
}

impl Region {
    /// Whether a point lies inside the region.
    #[must_use]
    pub fn contains(&self, p: WorldPos) -> bool {
        match self {
            Region::Circle { center, radius } => center.within_range(p, *radius),
            Region::Rect { rect } => rect.contains(p),
            Region::Cone {
                apex,
                facing,
                half_width,
                range,
            } => apex.within_range(p, *range) && facing.frontal_contains(*apex, p, *half_width),
        }
    }
}

fn in_world(value: i64) -> bool {
    (0..=WORLD_EXTENT).contains(&value)
}

fn facing_in_bounds(value: i64) -> bool {
    (-WORLD_EXTENT..=WORLD_EXTENT).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn pos(x: i64, y: i64) -> WorldPos {
        WorldPos::clamped(x, y)
    }

    fn vec(x: i64, y: i64) -> WorldVec {
        WorldVec::new(Fixed::from_raw(x), Fixed::from_raw(y))
    }

    fn facing(x: i64, y: i64) -> Facing {
        Facing::new(vec(x, y)).unwrap()
    }

    #[test]
    fn fixed_from_tile_parts_and_raw() {
        assert_eq!(Fixed::from_tile_parts(3, 128).raw(), 196_736);
        assert_eq!(Fixed::from_raw(-5000).raw(), -5000);
    }

    #[test]
    fn fixed_round_trips_bare_integer() {
        let json = serde_json::to_string(&Fixed::from_raw(196_736)).unwrap();
        assert_eq!(json, "196736");
        assert_eq!(
            serde_json::from_str::<Fixed>("196736").unwrap().raw(),
            196_736
        );
    }

    #[test]
    fn world_pos_rejects_out_of_bounds() {
        assert!(WorldPos::new(Fixed::from_raw(-1), Fixed::from_raw(0)).is_err());
        assert!(WorldPos::new(Fixed::from_raw(0), Fixed::from_raw(WORLD_EXTENT + 1)).is_err());
        assert!(WorldPos::new(Fixed::from_raw(0), Fixed::from_raw(WORLD_EXTENT)).is_ok());
    }

    #[test]
    fn world_pos_wire_round_trip_and_bounds() {
        let p = pos(163_840, 229_376);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, r#"{"x":163840,"y":229376}"#);
        assert_eq!(serde_json::from_str::<WorldPos>(&json).unwrap(), p);
        assert!(serde_json::from_str::<WorldPos>(r#"{"x":-1,"y":0}"#).is_err());
    }

    #[test]
    fn vector_add_sub_scale() {
        let a = pos(10, 20);
        let sum = a + vec(3, -5);
        assert_eq!(sum, pos(13, 15));
        let d = pos(10, 20) - pos(4, 32);
        assert_eq!(d, vec(6, -12));
        assert_eq!(d.scale(3), vec(18, -36));
    }

    #[test]
    fn pos_plus_vec_clamps_into_world() {
        let edge = pos(WORLD_EXTENT, WORLD_EXTENT);
        let moved = edge + vec(WORLD_EXTENT, WORLD_EXTENT);
        assert_eq!(moved, pos(WORLD_EXTENT, WORLD_EXTENT));
        let origin = pos(0, 0);
        assert_eq!(origin + vec(-100, -100), pos(0, 0));
    }

    #[test]
    fn distance_and_within_range() {
        assert_eq!(pos(0, 0).distance_sq(pos(3, 4)).get(), 25);
        assert!(pos(0, 0).within_range(pos(3, 4), Radius::new(5).unwrap()));
        assert!(!pos(0, 0).within_range(pos(3, 4), Radius::new(4).unwrap()));
        assert!(pos(0, 0).within_range(pos(65_535, 0), Radius::new(65_535).unwrap()));
        assert!(!pos(0, 0).within_range(pos(65_535, 0), Radius::new(65_534).unwrap()));
    }

    #[test]
    fn radius_rejects_out_of_bounds() {
        assert!(Radius::new(RADIUS_MAX).is_ok());
        assert!(Radius::new(RADIUS_MAX + 1).is_err());
        assert_eq!(Radius::new(5).unwrap().squared().get(), 25);
    }

    #[test]
    fn radius_wire_round_trip() {
        let json = serde_json::to_string(&Radius::new(10).unwrap()).unwrap();
        assert_eq!(json, "10");
        assert!(serde_json::from_str::<Radius>(&(RADIUS_MAX + 1).to_string()).is_err());
    }

    #[test]
    fn facing_rejects_zero_and_out_of_bounds() {
        assert_eq!(Facing::new(vec(0, 0)), Err(SpatialError::ZeroFacing));
        assert!(matches!(
            Facing::new(vec(WORLD_EXTENT + 1, 0)),
            Err(SpatialError::ComponentOutOfBounds { axis: Axis::X, .. })
        ));
        assert!(Facing::new(vec(1, 0)).is_ok());
    }

    #[test]
    fn facing_wire_round_trip_rejects_zero() {
        let json = serde_json::to_string(&facing(1, 0)).unwrap();
        assert_eq!(json, r#"{"x":1,"y":0}"#);
        assert_eq!(serde_json::from_str::<Facing>(&json).unwrap(), facing(1, 0));
        assert!(serde_json::from_str::<Facing>(r#"{"x":0,"y":0}"#).is_err());
    }

    #[test]
    fn cone_six_cases() {
        let east = facing(1, 0);
        let apex = pos(0, 0);
        let d45 = ConeHalfWidth::DEG_45;
        let d90 = ConeHalfWidth::DEG_90;
        // straight ahead: in 45.
        assert!(east.frontal_contains(apex, pos(10, 0), d45));
        // behind (d < 0): out.
        assert!(!facing(1, 0).frontal_contains(pos(20, 0), pos(10, 0), d45));
        // angular edge of 45 (50 >= 50): in.
        assert!(east.frontal_contains(apex, pos(5, 5), d45));
        // just outside 45 (50 >= 61 false): out.
        assert!(!east.frontal_contains(apex, pos(5, 6), d45));
        // straight up: out of 45, in 90.
        assert!(!east.frontal_contains(apex, pos(0, 5), d45));
        assert!(east.frontal_contains(apex, pos(0, 5), d90));
        // magnitude invariance.
        assert_eq!(
            east.frontal_contains(apex, pos(5, 5), d45),
            facing(7, 0).frontal_contains(apex, pos(5, 5), d45)
        );
    }

    #[test]
    fn cone_wire_round_trip() {
        let json = serde_json::to_string(&ConeHalfWidth::DEG_45).unwrap();
        assert_eq!(json, r#"{"num":1,"den":2}"#);
        assert_eq!(
            serde_json::from_str::<ConeHalfWidth>(&json).unwrap(),
            ConeHalfWidth::DEG_45
        );
        assert!(serde_json::from_str::<ConeHalfWidth>(r#"{"num":3,"den":2}"#).is_err());
    }

    #[test]
    fn rect_new_spanning_contains() {
        let lo = pos(10, 10);
        let hi = pos(20, 30);
        let rect = WorldRect::new(lo, hi).unwrap();
        assert!(rect.contains(lo));
        assert!(rect.contains(hi));
        assert!(rect.contains(pos(15, 20)));
        assert!(!rect.contains(pos(9, 20)));
        assert!(WorldRect::new(hi, lo).is_err());
        assert_eq!(WorldRect::spanning(hi, lo), rect);
    }

    #[test]
    fn rect_wire_rejects_inverted() {
        let ok = r#"{"min":{"x":10,"y":10},"max":{"x":20,"y":30}}"#;
        assert!(serde_json::from_str::<WorldRect>(ok).is_ok());
        let bad = r#"{"min":{"x":20,"y":10},"max":{"x":10,"y":30}}"#;
        assert!(serde_json::from_str::<WorldRect>(bad).is_err());
    }

    #[test]
    fn region_containment_and_wire() {
        let circle = Region::Circle {
            center: pos(0, 0),
            radius: Radius::new(5).unwrap(),
        };
        assert!(circle.contains(pos(3, 4)));
        assert!(!circle.contains(pos(4, 4)));
        assert_eq!(
            serde_json::to_string(&circle).unwrap(),
            r#"{"kind":"circle","center":{"x":0,"y":0},"radius":5}"#
        );

        let rect = Region::Rect {
            rect: WorldRect::new(pos(10, 10), pos(20, 30)).unwrap(),
        };
        assert!(rect.contains(pos(10, 10)));
        assert!(!rect.contains(pos(21, 10)));
        assert_eq!(
            serde_json::to_string(&rect).unwrap(),
            r#"{"kind":"rect","rect":{"min":{"x":10,"y":10},"max":{"x":20,"y":30}}}"#
        );

        let cone = Region::Cone {
            apex: pos(0, 0),
            facing: facing(1, 0),
            half_width: ConeHalfWidth::DEG_45,
            range: Radius::new(10).unwrap(),
        };
        assert!(cone.contains(pos(5, 5)));
        assert!(!cone.contains(pos(8, 9))); // in range, out of angle
        assert!(!cone.contains(pos(100, 0))); // in angle, out of range
        assert_eq!(
            serde_json::to_string(&cone).unwrap(),
            r#"{"kind":"cone","apex":{"x":0,"y":0},"facing":{"x":1,"y":0},"half_width":{"num":1,"den":2},"range":10}"#
        );
    }

    #[test]
    fn serialize_identity_is_stable() {
        let region = Region::Cone {
            apex: pos(1, 2),
            facing: facing(3, 4),
            half_width: ConeHalfWidth::DEG_45,
            range: Radius::new(9).unwrap(),
        };
        let first = serde_json::to_string(&region).unwrap();
        let second = serde_json::to_string(&region).unwrap();
        assert_eq!(first, second);
    }

    proptest! {
        #[test]
        fn within_range_matches_distance_sq(
            ax in 0i64..=WORLD_EXTENT, ay in 0i64..=WORLD_EXTENT,
            bx in 0i64..=WORLD_EXTENT, by in 0i64..=WORLD_EXTENT,
            r in 0u64..=RADIUS_MAX,
        ) {
            let a = WorldPos::clamped(ax, ay);
            let b = WorldPos::clamped(bx, by);
            let radius = Radius::new(r).unwrap();
            prop_assert_eq!(
                a.within_range(b, radius),
                a.distance_sq(b).get() <= radius.squared().get()
            );
        }

        #[test]
        fn distance_sq_is_symmetric(
            ax in 0i64..=WORLD_EXTENT, ay in 0i64..=WORLD_EXTENT,
            bx in 0i64..=WORLD_EXTENT, by in 0i64..=WORLD_EXTENT,
        ) {
            let a = WorldPos::clamped(ax, ay);
            let b = WorldPos::clamped(bx, by);
            prop_assert_eq!(a.distance_sq(b), b.distance_sq(a));
            prop_assert_eq!(a.distance_sq(a).get(), 0);
        }

        #[test]
        fn vec_add_then_sub_offset_is_identity(
            ax in -100_000i64..=100_000, ay in -100_000i64..=100_000,
            dx in -100_000i64..=100_000, dy in -100_000i64..=100_000,
        ) {
            let a = WorldVec::new(Fixed::from_raw(ax), Fixed::from_raw(ay));
            let d = WorldVec::new(Fixed::from_raw(dx), Fixed::from_raw(dy));
            let zero = WorldVec::new(Fixed::from_raw(0), Fixed::from_raw(0));
            prop_assert_eq!((a + d) - d, a);
            prop_assert_eq!(a - a, zero);
        }

        #[test]
        fn radius_squared_never_overflows(r in 0u64..=RADIUS_MAX) {
            let radius = Radius::new(r).unwrap();
            prop_assert_eq!(radius.squared().get(), u128::from(r) * u128::from(r));
        }

        #[test]
        fn cone_scale_invariant_in_facing_magnitude(
            k in 1i64..=64,
            tx in -32i64..=32, ty in -32i64..=32,
        ) {
            let apex = WorldPos::clamped(1_000, 1_000);
            let target = WorldPos::clamped(1_000 + tx, 1_000 + ty);
            let base = Facing::new(WorldVec::new(Fixed::from_raw(1), Fixed::from_raw(1))).unwrap();
            let scaled = Facing::new(
                WorldVec::new(Fixed::from_raw(k), Fixed::from_raw(k))
            ).unwrap();
            prop_assert_eq!(
                base.frontal_contains(apex, target, ConeHalfWidth::DEG_45),
                scaled.frontal_contains(apex, target, ConeHalfWidth::DEG_45)
            );
        }
    }
}
