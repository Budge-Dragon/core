//! The bounded gauge shared by every depletable resource: a current value that
//! is never allowed above its maximum. Health, mana, and ability points are all
//! [`Pool`]s; the `current <= max` invariant is proven at construction so no
//! consumer re-checks it.

use serde::{Deserialize, Serialize};

/// Wire mirror of [`Pool`]; the `current <= max` invariant is re-proven on the
/// way in, since a persisted pool loaded from a host is untrusted.
#[derive(Serialize, Deserialize)]
struct PoolWire {
    current: u32,
    max: u32,
}

/// A bounded gauge: a `current` value in `0..=max`. The invariant
/// `current <= max` holds by construction — [`Pool::new`] rejects an over-full
/// pool, and [`Pool::full`] cannot produce one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PoolWire", into = "PoolWire")]
pub struct Pool {
    current: u32,
    max: u32,
}

impl Pool {
    /// Builds a pool, rejecting a current value above the maximum.
    ///
    /// # Errors
    /// Returns [`PoolError::CurrentExceedsMax`] when `current > max`.
    pub fn new(current: u32, max: u32) -> Result<Self, PoolError> {
        if current > max {
            return Err(PoolError::CurrentExceedsMax { current, max });
        }
        Ok(Self { current, max })
    }

    /// Builds a full pool: `current == max`. The seed-at-max path — a freshly
    /// spawned entity is at full health.
    #[must_use]
    pub fn full(max: u32) -> Self {
        Self { current: max, max }
    }

    /// The current value.
    #[must_use]
    pub const fn current(self) -> u32 {
        self.current
    }

    /// The maximum value.
    #[must_use]
    pub const fn max(self) -> u32 {
        self.max
    }

    /// This pool with `amount` removed from `current`, saturating at zero; the
    /// maximum is unchanged. The shared reduction path for damage taken and for
    /// spending a resource (mana/ability) on a cast.
    #[must_use]
    pub const fn reduced(self, amount: u32) -> Pool {
        Pool {
            current: self.current.saturating_sub(amount),
            max: self.max,
        }
    }

    /// This pool with `amount` added to `current`, clamped at the maximum; the
    /// maximum is unchanged. The shared recovery path for a heal or a
    /// resource regeneration — an over-heal saturates at full, never above.
    #[must_use]
    pub const fn restored(self, amount: u32) -> Pool {
        let raised = self.current.saturating_add(amount);
        let current = if raised > self.max { self.max } else { raised };
        Pool {
            current,
            max: self.max,
        }
    }
}

impl TryFrom<PoolWire> for Pool {
    type Error = PoolError;

    fn try_from(wire: PoolWire) -> Result<Self, Self::Error> {
        Self::new(wire.current, wire.max)
    }
}

impl From<Pool> for PoolWire {
    fn from(pool: Pool) -> Self {
        Self {
            current: pool.current,
            max: pool.max,
        }
    }
}

/// Rejection of a malformed pool at construction or the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolError {
    /// The current value exceeded the maximum.
    CurrentExceedsMax {
        /// The offending current value.
        current: u32,
        /// The maximum it exceeded.
        max: u32,
    },
}

impl core::fmt::Display for PoolError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CurrentExceedsMax { current, max } => {
                write!(f, "pool current {current} exceeds max {max}")
            }
        }
    }
}

impl core::error::Error for PoolError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_accepts_current_below_and_at_max() {
        assert_eq!(Pool::new(30, 60).unwrap().current(), 30);
        let full = Pool::new(60, 60).unwrap();
        assert_eq!(full.current(), 60);
        assert_eq!(full.max(), 60);
    }

    #[test]
    fn new_rejects_current_above_max() {
        assert_eq!(
            Pool::new(61, 60),
            Err(PoolError::CurrentExceedsMax {
                current: 61,
                max: 60
            })
        );
    }

    #[test]
    fn empty_pool_is_legal() {
        let empty = Pool::new(0, 0).unwrap();
        assert_eq!(empty.current(), 0);
        assert_eq!(empty.max(), 0);
    }

    #[test]
    fn full_seeds_current_at_max() {
        let pool = Pool::full(15_000);
        assert_eq!(pool.current(), 15_000);
        assert_eq!(pool.max(), 15_000);
    }

    #[test]
    fn reduced_saturates_at_zero_and_keeps_max() {
        let pool = Pool::new(30, 60).unwrap();
        assert_eq!(pool.reduced(10), Pool::new(20, 60).unwrap());
        // Over-reduction saturates the current value at zero.
        let drained = pool.reduced(1000);
        assert_eq!(drained.current(), 0);
        assert_eq!(drained.max(), 60);
        // Reducing by zero is the identity.
        assert_eq!(pool.reduced(0), pool);
    }

    #[test]
    fn restored_clamps_at_max_and_keeps_max() {
        let pool = Pool::new(30, 60).unwrap();
        assert_eq!(pool.restored(10), Pool::new(40, 60).unwrap());
        // Over-heal clamps the current value at the maximum.
        let full = pool.restored(1000);
        assert_eq!(full.current(), 60);
        assert_eq!(full.max(), 60);
        // Restoring by zero is the identity.
        assert_eq!(pool.restored(0), pool);
    }

    #[test]
    fn wire_round_trips() {
        let pool = Pool::new(25, 60).unwrap();
        let json = serde_json::to_string(&pool).unwrap();
        assert_eq!(json, r#"{"current":25,"max":60}"#);
        assert_eq!(serde_json::from_str::<Pool>(&json).unwrap(), pool);
    }

    #[test]
    fn wire_rejects_over_full_pool() {
        assert!(serde_json::from_str::<Pool>(r#"{"current":61,"max":60}"#).is_err());
    }
}
