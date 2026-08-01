//! Unit value newtypes embedded by every static-data schema; each invariant
//! is proven at construction (serde `try_from` at the parse boundary; total
//! `clamped` constructors on the compute path).

use core::num::{NonZeroU16, NonZeroU32};
use core::ops::{Add, Sub};

use serde::{Deserialize, Serialize};

use crate::components::levels::EnhanceLevel;

/// A zen amount (fees, prices, drops, storage caps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Zen(
    /// Zen units.
    pub u64,
);

/// A carried zen balance, proven `<= 2_000_000_000` at construction and on
/// the wire. Distinct from [`Zen`] (a price, fee, or drop amount, unbounded):
/// a loaded excellent item's buy price reachably exceeds the carry cap, so a
/// price is not a balance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct CarriedZen(u64);

impl CarriedZen {
    /// The carry cap. A design pin, not a source extraction: OpenMU carries
    /// only a config `int.MaxValue`; the classic client's 2e9 cap appears
    /// nowhere in it. The constant is ours.
    pub const CAP: u64 = 2_000_000_000;

    /// An empty balance — a freshly created character carries no zen. A real
    /// domain value, not a fabricated default; the infallible seed the fallible
    /// [`Self::new`] would otherwise need a banned suppressor to produce.
    pub const ZERO: Self = Self(0);

    /// Builds a balance; values above the carry cap are rejected. No
    /// `clamped` twin (unlike [`Level`]/[`Percent`]): a saturating money
    /// constructor silently destroys zen. Fallible construction only.
    ///
    /// # Errors
    /// Returns [`UnitError::ZenAboveCap`] when `value` exceeds [`Self::CAP`].
    pub fn new(value: u64) -> Result<Self, UnitError> {
        if value > Self::CAP {
            return Err(UnitError::ZenAboveCap { value });
        }
        Ok(Self(value))
    }

    /// The balance value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Adds a credit; a summed balance over the cap is rejected and the
    /// balance preserved — never clamped. The cap edge is inclusive.
    #[must_use]
    pub fn credit(self, amount: Zen) -> CreditOutcome {
        let sum = self.0.saturating_add(amount.0);
        if sum > Self::CAP {
            return CreditOutcome::OverCap { balance: self };
        }
        CreditOutcome::Credited { balance: Self(sum) }
    }

    /// Removes a debit; an amount over the balance is rejected and the
    /// balance preserved.
    #[must_use]
    pub fn debit(self, amount: Zen) -> DebitOutcome {
        match self.0.checked_sub(amount.0) {
            Some(remaining) => DebitOutcome::Debited {
                balance: Self(remaining),
            },
            None => DebitOutcome::Insufficient { balance: self },
        }
    }
}

impl TryFrom<u64> for CarriedZen {
    type Error = UnitError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<CarriedZen> for u64 {
    fn from(zen: CarriedZen) -> Self {
        zen.0
    }
}

/// Result of a [`CarriedZen::credit`] — the cap edge is inclusive. Plain
/// (non-serde): consumed and re-mapped by services onto their own wire
/// outcomes; it never crosses a port itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreditOutcome {
    /// The credit fit under the cap.
    Credited {
        /// The new balance after the credit.
        balance: CarriedZen,
    },
    /// The summed balance would exceed the cap; nothing was credited.
    OverCap {
        /// The unchanged balance.
        balance: CarriedZen,
    },
}

/// Result of a [`CarriedZen::debit`]. Plain (non-serde): consumed and
/// re-mapped by services onto their own wire outcomes; it never crosses a
/// port itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebitOutcome {
    /// The debit was covered.
    Debited {
        /// The new balance after the debit.
        balance: CarriedZen,
    },
    /// The amount exceeds the balance; nothing was debited.
    Insufficient {
        /// The unchanged balance.
        balance: CarriedZen,
    },
}

/// An experience amount (table entries, per-kill gains).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Exp(
    /// Experience points.
    pub u64,
);

impl Exp {
    /// No experience — a freshly created character starts at zero. A real
    /// domain value, not a fabricated default.
    pub const ZERO: Self = Self(0);
}

/// Map number as the client knows it — a single byte pre-S3. Lives here, the
/// lowest vocabulary layer, because a component ([`crate::components::placement::Placement`])
/// composes it; the other identity newtypes stay in `data::common`, referenced
/// only by entities and data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MapNumber(
    /// The client map number.
    pub u8,
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

impl DurationMs {
    /// This millisecond delay as a whole-tick cadence, rounding up so an action
    /// never fires faster than its authored delay: `0` ms yields zero ticks,
    /// any positive delay at least one. Integer division only, so the same
    /// cadence comes out on every target.
    #[must_use]
    pub fn in_ticks(self, tick: TickDuration) -> Ticks {
        let ms = u64::from(self.0);
        let per = u64::from(tick.millis().get());
        Ticks(ms.div_ceil(per))
    }
}

/// An absolute simulation tick — a point on the tick timeline. Host-supplied
/// input; core never reads a clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tick(
    /// The tick index on the timeline.
    pub u64,
);

impl Tick {
    /// Whether this tick has been reached at `now` (`now >= self`) — the
    /// readiness predicate a cadence checks before acting. [`Tick(0)`](Tick) is
    /// always reached, so a freshly seeded action is ready immediately.
    #[must_use]
    pub fn reached(self, now: Tick) -> bool {
        now.0 >= self.0
    }
}

/// A duration measured in whole ticks — the cadence gap between two actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Ticks(
    /// The number of whole ticks.
    pub u64,
);

impl Ticks {
    /// This tick span as whole seconds — the reverse read of
    /// [`DurationMs::in_ticks`], flooring so a partial second never counts.
    /// Pure integer math (a saturating widen, then floor division), so the
    /// same count comes out on every target.
    #[must_use]
    pub fn whole_seconds(self, tick: TickDuration) -> u64 {
        self.0.saturating_mul(u64::from(tick.millis().get())) / 1000
    }

    /// This tick span as whole minutes — sixty whole seconds per minute,
    /// flooring so a partial minute never counts.
    #[must_use]
    pub fn whole_minutes(self, tick: TickDuration) -> u64 {
        self.whole_seconds(tick) / 60
    }
}

impl Add<Ticks> for Tick {
    type Output = Tick;

    /// This tick advanced by a duration, saturating at the timeline's end — the
    /// affine `point + duration` operation.
    fn add(self, delay: Ticks) -> Tick {
        Tick(self.0.saturating_add(delay.0))
    }
}

impl Sub<Ticks> for Tick {
    type Output = Tick;

    /// This tick pulled back by a duration, saturating at the timeline's start
    /// — the affine `point - duration` operation, symmetric with [`Add`].
    fn sub(self, delay: Ticks) -> Tick {
        Tick(self.0.saturating_sub(delay.0))
    }
}

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
    /// Zero percent — the authentic gearless chance (no critical, excellent,
    /// defense-ignore, or double-damage roll succeeds). A real domain value, not
    /// a fabricated default.
    pub const ZERO: Self = Self(0);

    /// [`Self::ZERO`] as a function — the serde deserialize-default path for
    /// gearless-zero profile fields (`#[serde(default = "...")]` needs a fn).
    #[must_use]
    pub const fn zero() -> Self {
        Self::ZERO
    }

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
    /// Plus level zero — a base (un-enhanced) item instance. A real domain
    /// value, not a fabricated default.
    pub const ZERO: Self = Self(0);

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

    /// Clamps a value into the wire item-level range: values above 15 saturate
    /// to [`Self::WIRE_MAX`]. Total, and takes `u64` so a wide computation feeds
    /// it with no narrowing cast — the compute-path constructor for a derived
    /// plus level. Parsing external input stays on the fallible `new`/`try_from`,
    /// where out-of-range is an error, never a clamp.
    #[must_use]
    pub fn clamped(value: u64) -> Self {
        match u8::try_from(value) {
            Ok(level) => Self(level.min(Self::WIRE_MAX)),
            Err(_) => Self(Self::WIRE_MAX),
        }
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

    /// The enhancement level this wire level denotes, with the box-tier levels
    /// (12..=15, which carry no enhancement row) folded to the +0 curve entry —
    /// the total read every enhancement-curve lookup shares.
    #[must_use]
    pub fn enhance_level_or_zero(self) -> EnhanceLevel {
        match self.enhance_level() {
            Some(enhance) => enhance,
            None => EnhanceLevel::L0,
        }
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
    /// Carried zen above the carry cap.
    ZenAboveCap {
        /// The rejected value.
        value: u64,
    },
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
            Self::ZenAboveCap { value } => {
                write!(f, "carried zen {value} exceeds 2000000000")
            }
        }
    }
}

impl core::error::Error for UnitError {}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn level_wire_round_trips_across_its_valid_range(value in 1u16..=u16::MAX) {
            let level = Level::new(value).unwrap();
            let json = serde_json::to_string(&level).unwrap();
            prop_assert_eq!(serde_json::from_str::<Level>(&json).unwrap(), level);
            prop_assert_eq!(level.get(), value);
        }

        #[test]
        fn zen_wire_round_trips_across_the_full_range(value in any::<u64>()) {
            let zen = Zen(value);
            let json = serde_json::to_string(&zen).unwrap();
            prop_assert_eq!(serde_json::from_str::<Zen>(&json).unwrap(), zen);
        }

        #[test]
        fn exp_wire_round_trips_across_the_full_range(value in any::<u64>()) {
            let exp = Exp(value);
            let json = serde_json::to_string(&exp).unwrap();
            prop_assert_eq!(serde_json::from_str::<Exp>(&json).unwrap(), exp);
        }
    }

    #[test]
    fn level_rejects_out_of_range_on_the_wire() {
        // Zero is the only out-of-range wire value (the domain is `1..=u16::MAX`);
        // a value above `u16::MAX` cannot deserialize into the `u16` mirror.
        assert!(Level::new(0).is_err());
        assert!(serde_json::from_str::<Level>("0").is_err());
        assert!(serde_json::from_str::<Level>("70000").is_err());
    }

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
    fn item_level_clamped_saturates_at_wire_max() {
        assert_eq!(ItemLevel::clamped(0), ItemLevel::ZERO);
        assert_eq!(ItemLevel::clamped(9).get(), 9);
        assert_eq!(ItemLevel::clamped(15).get(), 15);
        assert_eq!(ItemLevel::clamped(16).get(), 15);
        assert_eq!(ItemLevel::clamped(u64::MAX).get(), 15);
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

    #[test]
    fn duration_ms_in_ticks_rounds_up() {
        let per = TickDuration::new(50).unwrap();
        assert_eq!(DurationMs(400).in_ticks(per), Ticks(8));
        assert_eq!(DurationMs(410).in_ticks(per), Ticks(9));
        assert_eq!(DurationMs(1).in_ticks(per), Ticks(1));
        assert_eq!(DurationMs(0).in_ticks(per), Ticks(0));
    }

    #[test]
    fn ticks_whole_seconds_floors_partial_seconds() {
        let per = TickDuration::new(100).unwrap();
        assert_eq!(Ticks(0).whole_seconds(per), 0);
        assert_eq!(Ticks(9).whole_seconds(per), 0);
        assert_eq!(Ticks(10).whole_seconds(per), 1);
        assert_eq!(Ticks(19).whole_seconds(per), 1);
        assert_eq!(Ticks(300).whole_seconds(per), 30);
    }

    #[test]
    fn ticks_whole_seconds_reverses_in_ticks() {
        let per = TickDuration::new(100).unwrap();
        assert_eq!(DurationMs(30_000).in_ticks(per).whole_seconds(per), 30);
        assert_eq!(
            DurationMs(1_200_000).in_ticks(per).whole_seconds(per),
            1_200
        );
    }

    #[test]
    fn ticks_whole_minutes_floors_partial_minutes() {
        let per = TickDuration::new(100).unwrap();
        assert_eq!(Ticks(599).whole_minutes(per), 0);
        assert_eq!(Ticks(600).whole_minutes(per), 1);
        assert_eq!(Ticks(3_000).whole_minutes(per), 5);
        assert_eq!(Ticks(3_599).whole_minutes(per), 5);
    }

    #[test]
    fn ticks_whole_seconds_saturates_at_the_ceiling() {
        let per = TickDuration::new(1_000).unwrap();
        assert_eq!(Ticks(u64::MAX).whole_seconds(per), u64::MAX / 1_000);
    }

    #[test]
    fn tick_reached_is_inclusive_and_ready_at_zero() {
        assert!(Tick(5).reached(Tick(5)));
        assert!(Tick(5).reached(Tick(6)));
        assert!(!Tick(5).reached(Tick(4)));
        assert!(Tick(0).reached(Tick(0)));
    }

    #[test]
    fn tick_add_saturates() {
        assert_eq!(Tick(5) + Ticks(3), Tick(8));
        assert_eq!(Tick(u64::MAX) + Ticks(1), Tick(u64::MAX));
    }

    #[test]
    fn tick_and_ticks_round_trip_as_bare_integers() {
        assert_eq!(serde_json::to_string(&Tick(42)).unwrap(), "42");
        assert_eq!(serde_json::from_str::<Tick>("42").unwrap(), Tick(42));
        assert_eq!(serde_json::to_string(&Ticks(7)).unwrap(), "7");
        assert_eq!(serde_json::from_str::<Ticks>("7").unwrap(), Ticks(7));
    }

    #[test]
    fn carried_zen_constructs_at_the_cap_and_rejects_above() {
        assert_eq!(CarriedZen::new(2_000_000_000).unwrap().get(), 2_000_000_000);
        assert_eq!(
            CarriedZen::new(2_000_000_001),
            Err(UnitError::ZenAboveCap {
                value: 2_000_000_001
            })
        );
    }

    #[test]
    fn carried_zen_zero_is_the_empty_balance_no_suppressor() {
        assert_eq!(CarriedZen::ZERO.get(), 0);
        assert_eq!(CarriedZen::ZERO, CarriedZen::new(0).unwrap());
    }

    #[test]
    fn exp_zero_is_the_empty_amount() {
        assert_eq!(Exp::ZERO, Exp(0));
    }

    #[test]
    fn carried_zen_credit_sums_below_the_cap() {
        let balance = CarriedZen::new(250_000).unwrap();
        assert_eq!(
            balance.credit(Zen(40_000)),
            CreditOutcome::Credited {
                balance: CarriedZen::new(290_000).unwrap()
            }
        );
    }

    #[test]
    fn carried_zen_credit_cap_edge_is_inclusive() {
        let balance = CarriedZen::new(1_999_999_999).unwrap();
        assert_eq!(
            balance.credit(Zen(1)),
            CreditOutcome::Credited {
                balance: CarriedZen::new(CarriedZen::CAP).unwrap()
            }
        );
    }

    #[test]
    fn carried_zen_credit_over_the_cap_preserves_the_balance() {
        let balance = CarriedZen::new(1_999_999_999).unwrap();
        assert_eq!(balance.credit(Zen(2)), CreditOutcome::OverCap { balance });
    }

    #[test]
    fn carried_zen_debit_reduces_when_funds_suffice() {
        let balance = CarriedZen::new(1_000_000).unwrap();
        assert_eq!(
            balance.debit(Zen(250_000)),
            DebitOutcome::Debited {
                balance: CarriedZen::new(750_000).unwrap()
            }
        );
    }

    #[test]
    fn carried_zen_debit_exactly_to_zero() {
        let balance = CarriedZen::new(250_000).unwrap();
        assert_eq!(
            balance.debit(Zen(250_000)),
            DebitOutcome::Debited {
                balance: CarriedZen::new(0).unwrap()
            }
        );
    }

    #[test]
    fn carried_zen_debit_over_the_balance_preserves_it() {
        let balance = CarriedZen::new(250_000).unwrap();
        assert_eq!(
            balance.debit(Zen(250_001)),
            DebitOutcome::Insufficient { balance }
        );
    }

    #[test]
    fn carried_zen_wire_is_a_bare_integer_reproven_on_parse() {
        let balance = CarriedZen::new(250_000).unwrap();
        assert_eq!(serde_json::to_string(&balance).unwrap(), "250000");
        assert_eq!(
            serde_json::from_str::<CarriedZen>("250000").unwrap(),
            balance
        );
        assert!(serde_json::from_str::<CarriedZen>("2000000001").is_err());
    }

    #[test]
    fn map_number_round_trips_as_bare_integer() {
        assert_eq!(serde_json::to_string(&MapNumber(5)).unwrap(), "5");
        assert_eq!(
            serde_json::from_str::<MapNumber>("5").unwrap(),
            MapNumber(5)
        );
    }
}
