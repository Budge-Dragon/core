//! The generic inclusive interval `[min, max]` shared by every bounded-range
//! field in the data layer. `min <= max` is proven once at construction; the
//! accessors are then total.

use serde::{Deserialize, Serialize};

/// An inclusive interval `[min, max]` over an ordered, copyable value, with
/// `min <= max` guaranteed by construction. Parsed through its wire mirror at
/// the load boundary, so every accessor downstream is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    try_from = "RawInterval<T>",
    into = "RawInterval<T>",
    bound(
        serialize = "T: Ord + Copy + Serialize",
        deserialize = "T: Ord + Copy + core::fmt::Debug + Deserialize<'de>"
    )
)]
pub struct Interval<T> {
    min: T,
    max: T,
}

impl<T: Ord + Copy> Interval<T> {
    /// Builds an inclusive interval; `min > max` is rejected.
    ///
    /// # Errors
    /// Returns [`IntervalError`] when `min > max`.
    pub fn new(min: T, max: T) -> Result<Self, IntervalError<T>> {
        if min > max {
            return Err(IntervalError { min, max });
        }
        Ok(Self { min, max })
    }

    /// Inclusive lower bound.
    #[must_use]
    pub fn min(self) -> T {
        self.min
    }

    /// Inclusive upper bound.
    #[must_use]
    pub fn max(self) -> T {
        self.max
    }

    /// True iff `value` lies within the inclusive interval.
    #[must_use]
    pub fn contains(self, value: T) -> bool {
        self.min <= value && value <= self.max
    }
}

/// Wire mirror of [`Interval`]; edge order checked on the way in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RawInterval<T> {
    min: T,
    max: T,
}

impl<T: Ord + Copy> TryFrom<RawInterval<T>> for Interval<T> {
    type Error = IntervalError<T>;

    fn try_from(raw: RawInterval<T>) -> Result<Self, Self::Error> {
        Self::new(raw.min, raw.max)
    }
}

impl<T: Ord + Copy> From<Interval<T>> for RawInterval<T> {
    fn from(interval: Interval<T>) -> Self {
        Self {
            min: interval.min,
            max: interval.max,
        }
    }
}

/// Parse failure: an interval whose lower bound exceeds its upper bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IntervalError<T> {
    /// The rejected lower bound.
    pub min: T,
    /// The rejected upper bound.
    pub max: T,
}

impl<T: core::fmt::Debug> core::fmt::Display for IntervalError<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "interval min {:?} exceeds max {:?}", self.min, self.max)
    }
}

impl<T: core::fmt::Debug> core::error::Error for IntervalError<T> {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_inverted_bounds() {
        assert!(Interval::new(5u16, 3u16).is_err());
        assert_eq!(Interval::new(3u16, 3u16).unwrap().min(), 3);
    }

    #[test]
    fn accessors_and_contains_are_total() {
        let interval = Interval::new(2u16, 6u16).unwrap();
        assert_eq!(interval.min(), 2);
        assert_eq!(interval.max(), 6);
        assert!(interval.contains(2));
        assert!(interval.contains(6));
        assert!(!interval.contains(1));
        assert!(!interval.contains(7));
    }

    #[test]
    fn serde_round_trips_and_rejects_inverted() {
        let interval = Interval::new(4u16, 11u16).unwrap();
        let json = serde_json::to_string(&interval).unwrap();
        assert_eq!(json, r#"{"min":4,"max":11}"#);
        assert_eq!(
            serde_json::from_str::<Interval<u16>>(&json).unwrap(),
            interval
        );
        assert!(serde_json::from_str::<Interval<u16>>(r#"{"min":11,"max":4}"#).is_err());
    }
}
