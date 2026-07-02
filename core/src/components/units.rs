//! Unit value newtypes embedded by every static-data schema; each invariant
//! is proven at construction (serde `try_from` at the parse boundary; total
//! `clamped` constructors on the compute path).

use core::num::{NonZeroU16, NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::components::levels::EnhanceLevel;

/// A zen amount (fees, prices, drops, storage caps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Zen(
    /// Zen units.
    pub u64,
);

/// An experience amount (table entries, per-kill gains).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Exp(
    /// Experience points.
    pub u64,
);

/// A 1-based character, monster, or requirement level. Zero is rejected at
/// construction; the era's level cap (400) is data, not a type bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct Level(NonZeroU16);

impl Level {
    /// The lowest level: 1. Total — the floor anchor for computed levels.
    pub const MIN: Self = Self(NonZeroU16::MIN);

    /// Builds a level; zero is rejected.
    ///
    /// # Errors
    /// Returns [`UnitError::LevelZero`] when `value` is zero.
    pub fn new(value: u16) -> Result<Self, UnitError> {
        NonZeroU16::new(value).map(Self).ok_or(UnitError::LevelZero)
    }

    /// Clamps a value into the 1-based level range: zero saturates up to
    /// [`Self::MIN`], values above `u16::MAX` saturate down to the widest
    /// level. Total, and takes `u64` so a wide computation reconstructs with
    /// no narrowing cast. Parsing external input stays on the fallible
    /// `new`/`try_from`, where zero is an error, never a clamp.
    #[must_use]
    pub fn clamped(value: u64) -> Self {
        match u16::try_from(value).map(NonZeroU16::new) {
            Ok(Some(level)) => Self(level),
            Ok(None) => Self::MIN,
            Err(_) => Self(NonZeroU16::MAX),
        }
    }

    /// The level value.
    #[must_use]
    pub fn get(self) -> u16 {
        self.0.get()
    }
}

impl TryFrom<u16> for Level {
    type Error = UnitError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Level> for u16 {
    fn from(level: Level) -> Self {
        level.0.get()
    }
}

/// A duration in milliseconds as stored in data files (fields suffixed
/// `_ms`); converted to whole ticks at the load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DurationMs(
    /// Milliseconds.
    pub u32,
);

/// Milliseconds per simulation tick — our time base (nonzero by construction).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u32", into = "u32")]
pub struct TickDuration(NonZeroU32);

impl TickDuration {
    /// Builds a tick duration; zero is rejected.
    ///
    /// # Errors
    /// Returns [`UnitError::ZeroTickDuration`] when `ms` is zero.
    pub fn new(ms: u32) -> Result<Self, UnitError> {
        NonZeroU32::new(ms)
            .map(Self)
            .ok_or(UnitError::ZeroTickDuration)
    }

    /// Milliseconds per tick.
    #[must_use]
    pub fn millis(self) -> NonZeroU32 {
        self.0
    }
}

impl TryFrom<u32> for TickDuration {
    type Error = UnitError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<TickDuration> for u32 {
    fn from(duration: TickDuration) -> Self {
        duration.0.get()
    }
}

/// An elemental resistance in the authentic Monster.txt unit: a raw byte
/// read as a numerator over 255.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Resistance(
    /// The Monster.txt resistance byte.
    pub u8,
);

impl Resistance {
    /// Denominator of the resist roll: byte scale.
    pub const DENOMINATOR: u16 = 255;
}

/// A probability as an integer numerator over 10,000 — the classic GS roll
/// grain (`rand() % 10000`). Every extracted pre-S3 chance converts exactly.
/// The only fine-grained probability unit in v2; no per-mille unit exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u16", into = "u16")]
pub struct ChancePer10000(u16);

impl ChancePer10000 {
    /// Denominator of the roll.
    pub const DENOMINATOR: u16 = 10_000;
    /// The impossible roll.
    pub const NEVER: Self = Self(0);
    /// The guaranteed roll.
    pub const ALWAYS: Self = Self(10_000);

    /// Builds a chance; numerators above 10,000 are rejected.
    ///
    /// # Errors
    /// Returns [`UnitError::ChanceAbove10000`] when `numerator` exceeds
    /// [`Self::DENOMINATOR`].
    pub fn new(numerator: u16) -> Result<Self, UnitError> {
        if numerator > Self::DENOMINATOR {
            return Err(UnitError::ChanceAbove10000 { value: numerator });
        }
        Ok(Self(numerator))
    }

    /// Clamps a value into range: values above 10,000 saturate to
    /// [`Self::ALWAYS`]. Total, and takes `u64` so a wide computation feeds it
    /// with no narrowing cast. Parsing external input stays on the fallible
    /// `new`/`try_from`, where out-of-range is an error, never a clamp.
    #[must_use]
    pub fn clamped(value: u64) -> Self {
        match u16::try_from(value) {
            Ok(numerator) => Self(numerator.min(Self::DENOMINATOR)),
            Err(_) => Self(Self::DENOMINATOR),
        }
    }

    /// Numerator over 10,000.
    #[must_use]
    pub fn numerator(self) -> u16 {
        self.0
    }
}

impl TryFrom<u16> for ChancePer10000 {
    type Error = UnitError;

    fn try_from(value: u16) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ChancePer10000> for u16 {
    fn from(chance: ChancePer10000) -> Self {
        chance.0
    }
}

/// Whole percent points `0..=100` — the authentic chaos-machine success unit
/// and the only percent probability unit in v2. Rate multipliers that may
/// exceed 100% are plain `u16` `_percent` fields, not this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Percent(u8);

impl Percent {
    /// Denominator of the roll.
    pub const DENOMINATOR: u8 = 100;

    /// Builds a percentage; values above 100 are rejected.
    ///
    /// # Errors
    /// Returns [`UnitError::PercentAbove100`] when `points` exceeds
    /// [`Self::DENOMINATOR`].
    pub fn new(points: u8) -> Result<Self, UnitError> {
        if points > Self::DENOMINATOR {
            return Err(UnitError::PercentAbove100 { value: points });
        }
        Ok(Self(points))
    }

    /// Clamps a value into range: values above 100 saturate to certainty.
    /// Total, and takes `u64` so a wide computation feeds it with no
    /// narrowing cast. Parsing external input stays on the fallible
    /// `new`/`try_from`, where out-of-range is an error, never a clamp.
    #[must_use]
    pub fn clamped(value: u64) -> Self {
        match u8::try_from(value) {
            Ok(points) => Self(points.min(Self::DENOMINATOR)),
            Err(_) => Self(Self::DENOMINATOR),
        }
    }

    /// Whole percent points.
    #[must_use]
    pub fn points(self) -> u8 {
        self.0
    }
}

impl TryFrom<u8> for Percent {
    type Error = UnitError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Percent> for u8 {
    fn from(percent: Percent) -> Self {
        percent.0
    }
}

/// An item plus-level, bounded by the client's 4-bit wire field (`0..=15`).
/// The enhanceable subrange is the distinct [`EnhanceLevel`]; box tiers ride
/// item levels beyond it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct ItemLevel(u8);

impl ItemLevel {
    /// Largest value the client's 4-bit item-level field can carry.
    pub const WIRE_MAX: u8 = 15;

    /// Builds an item level; values above 15 are rejected.
    ///
    /// # Errors
    /// Returns [`UnitError::ItemLevelAbove15`] when `level` exceeds
    /// [`Self::WIRE_MAX`].
    pub fn new(level: u8) -> Result<Self, UnitError> {
        if level > Self::WIRE_MAX {
            return Err(UnitError::ItemLevelAbove15 { value: level });
        }
        Ok(Self(level))
    }

    /// The plus-level value.
    #[must_use]
    pub fn get(self) -> u8 {
        self.0
    }

    /// The enhancement level this wire level denotes, when it denotes one.
    /// Genuine domain optionality: levels 12..=15 are box-tier pseudo-levels
    /// outside every enhancement curve. Resolved once, at the boundary.
    #[must_use]
    pub fn enhance_level(self) -> Option<EnhanceLevel> {
        EnhanceLevel::try_from(self.0).ok()
    }
}

impl TryFrom<u8> for ItemLevel {
    type Error = UnitError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ItemLevel> for u8 {
    fn from(level: ItemLevel) -> Self {
        level.0
    }
}

impl From<EnhanceLevel> for ItemLevel {
    fn from(level: EnhanceLevel) -> Self {
        Self(level.wire())
    }
}

/// Rejection of an out-of-range unit at the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitError {
    /// Chance numerator above 10,000.
    ChanceAbove10000 {
        /// The rejected numerator.
        value: u16,
    },
    /// Percent points above 100.
    PercentAbove100 {
        /// The rejected value.
        value: u8,
    },
    /// Item level above the client's 4-bit wire field.
    ItemLevelAbove15 {
        /// The rejected value.
        value: u8,
    },
    /// Level of zero — levels are 1-based.
    LevelZero,
    /// Tick duration of zero milliseconds.
    ZeroTickDuration,
}

impl core::fmt::Display for UnitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ChanceAbove10000 { value } => {
                write!(f, "chance numerator {value} exceeds 10000")
            }
            Self::PercentAbove100 { value } => write!(f, "percent {value} exceeds 100"),
            Self::ItemLevelAbove15 { value } => write!(f, "item level {value} exceeds 15"),
            Self::LevelZero => write!(f, "level must be at least 1"),
            Self::ZeroTickDuration => write!(f, "tick duration must be nonzero"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_rejects_zero_and_accepts_one() {
        assert_eq!(Level::new(0), Err(UnitError::LevelZero));
        assert_eq!(Level::new(1).unwrap().get(), 1);
        assert_eq!(Level::MIN.get(), 1);
    }

    #[test]
    fn level_clamped_saturates_both_ends() {
        assert_eq!(Level::clamped(0), Level::MIN);
        assert_eq!(Level::clamped(42).get(), 42);
        assert_eq!(Level::clamped(u64::MAX).get(), u16::MAX);
        assert_eq!(Level::clamped(u64::from(u16::MAX)).get(), u16::MAX);
    }

    #[test]
    fn level_serde_round_trip_rejects_zero() {
        let level = Level::new(150).unwrap();
        let json = serde_json::to_string(&level).unwrap();
        assert_eq!(json, "150");
        assert_eq!(serde_json::from_str::<Level>(&json).unwrap(), level);
        assert!(serde_json::from_str::<Level>("0").is_err());
    }

    #[test]
    fn chance_rejects_above_denominator_and_clamps() {
        assert!(ChancePer10000::new(10_001).is_err());
        assert_eq!(ChancePer10000::new(10_000).unwrap(), ChancePer10000::ALWAYS);
        assert_eq!(ChancePer10000::NEVER.numerator(), 0);
        assert_eq!(ChancePer10000::clamped(999_999), ChancePer10000::ALWAYS);
        assert_eq!(ChancePer10000::clamped(2500).numerator(), 2500);
    }

    #[test]
    fn chance_orders_by_numerator() {
        assert!(ChancePer10000::NEVER < ChancePer10000::ALWAYS);
        assert!(ChancePer10000::new(2500).unwrap() < ChancePer10000::new(3000).unwrap());
    }

    #[test]
    fn percent_rejects_above_100_and_clamps() {
        assert!(Percent::new(101).is_err());
        assert_eq!(Percent::new(100).unwrap().points(), 100);
        assert_eq!(Percent::clamped(250).points(), 100);
        assert_eq!(Percent::clamped(25).points(), 25);
    }

    #[test]
    fn item_level_rejects_above_15_and_crosses_to_enhance() {
        assert!(ItemLevel::new(16).is_err());
        assert_eq!(
            ItemLevel::new(11).unwrap().enhance_level(),
            Some(EnhanceLevel::L11)
        );
        assert_eq!(ItemLevel::new(12).unwrap().enhance_level(), None);
        assert_eq!(ItemLevel::new(15).unwrap().enhance_level(), None);
    }

    #[test]
    fn enhance_level_widens_into_item_level() {
        assert_eq!(ItemLevel::from(EnhanceLevel::L7).get(), 7);
    }

    #[test]
    fn tick_duration_rejects_zero() {
        assert!(TickDuration::new(0).is_err());
        assert_eq!(TickDuration::new(50).unwrap().millis().get(), 50);
    }

    #[test]
    fn resistance_denominator_is_byte_scale() {
        assert_eq!(Resistance::DENOMINATOR, 255);
        assert_eq!(Resistance(3).0, 3);
    }
}
