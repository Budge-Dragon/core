//! Record shape of `exp_tables.json` plus the parsed total-lookup curve and
//! the curve-bounded level it mints. Single-record file; it owns the level cap.

use serde::{Deserialize, Serialize};

use crate::components::units::{Exp, Level};

use super::common::Provenance;

/// The experience-curve record.
///
/// Curve content (authentic Webzen; the table is authoritative, this note is
/// documentation): `total(level) = 10*(level+8)*(level-1)^2` for `level < 256`,
/// plus `1000*(level-247)*(level-256)^2` for `level >= 256`; `total(1) = 0`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExpTable {
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// Highest reachable character level; nonzero proven at deserialize by the
    /// shared `Level` unit.
    pub max_level: Level,
    /// Total accumulated experience required to hold each level; dense,
    /// index = level - 1, length = `max_level`.
    pub total_exp_by_level: Vec<Exp>,
}

/// Parsed, total lookup over the curve. Construction proves the invariants once
/// (length == `max_level`, monotonically non-decreasing totals); every later
/// lookup is total.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpCurve {
    max_level: Level,
    totals: Vec<Exp>,
    /// Captured at parse from the last curve entry (construction proves it
    /// present, since `max_level` is nonzero so a valid table is non-empty), so
    /// the cap clamp is a total field read, never a fallible `level(max_level)`
    /// lookup.
    cap_total: Exp,
}

/// A level proven to lie within the curve's domain (`1..=max_level`), carrying
/// the total experience required to hold it. Minted only by [`ExpCurve::level`],
/// so both the level and its total are resolved at the one parse boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CurveLevel {
    level: Level,
    total: Exp,
}

impl CurveLevel {
    /// The underlying shared unit value.
    #[must_use]
    pub fn level(self) -> Level {
        self.level
    }

    /// Total experience required to hold this level. Total by type: resolved
    /// when the curve minted the value, so no lookup can fail here.
    #[must_use]
    pub fn total_to_hold(self) -> Exp {
        self.total
    }
}

impl ExpCurve {
    /// Parses the wire record once at the host load boundary.
    ///
    /// # Errors
    /// Returns [`ExpTableError::LengthMismatch`] when the dense table length
    /// does not equal `max_level`, or [`ExpTableError::NonMonotonic`] when the
    /// totals decrease at any step.
    pub fn parse(table: ExpTable) -> Result<Self, ExpTableError> {
        let expected = usize::from(table.max_level.get());
        let Some((&cap_total, _)) = table.total_exp_by_level.split_last() else {
            return Err(ExpTableError::LengthMismatch { expected, found: 0 });
        };
        let found = table.total_exp_by_level.len();
        if found != expected {
            return Err(ExpTableError::LengthMismatch { expected, found });
        }
        let mut previous: Option<Exp> = None;
        for &total in &table.total_exp_by_level {
            match previous {
                Some(prev) if total < prev => {
                    return Err(ExpTableError::NonMonotonic { total });
                }
                Some(_) | None => previous = Some(total),
            }
        }
        Ok(Self {
            max_level: table.max_level,
            totals: table.total_exp_by_level,
            cap_total,
        })
    }

    /// The level cap this curve defines.
    #[must_use]
    pub fn max_level(&self) -> Level {
        self.max_level
    }

    /// The total experience required to hold the level cap.
    #[must_use]
    pub fn cap_total(&self) -> Exp {
        self.cap_total
    }

    /// Mints a [`CurveLevel`] proven within `1..=max_level`, resolving its
    /// total experience — the type's only constructor.
    ///
    /// # Errors
    /// Returns [`ExpTableError::LevelOutOfRange`] when `raw` is zero or above
    /// the curve's cap.
    pub fn level(&self, raw: u16) -> Result<CurveLevel, ExpTableError> {
        let index = raw
            .checked_sub(1)
            .ok_or(ExpTableError::LevelOutOfRange { level: raw })?;
        let total = self
            .totals
            .get(usize::from(index))
            .copied()
            .ok_or(ExpTableError::LevelOutOfRange { level: raw })?;
        Ok(CurveLevel {
            level: Level::clamped(u64::from(raw)),
            total,
        })
    }
}

/// Load failure parsing the experience curve, or a level outside its domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpTableError {
    /// The dense table length does not equal `max_level`.
    LengthMismatch {
        /// The `max_level` the table declares.
        expected: usize,
        /// The number of entries found.
        found: usize,
    },
    /// The totals decrease at some step — not a monotonic curve.
    NonMonotonic {
        /// The out-of-order total.
        total: Exp,
    },
    /// A level outside `1..=max_level`.
    LevelOutOfRange {
        /// The rejected level.
        level: u16,
    },
}

impl core::fmt::Display for ExpTableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LengthMismatch { expected, found } => {
                write!(
                    f,
                    "exp table length {found} does not match max_level {expected}"
                )
            }
            Self::NonMonotonic { total } => {
                write!(
                    f,
                    "exp table total {total:?} decreases below its predecessor"
                )
            }
            Self::LevelOutOfRange { level } => {
                write!(f, "level {level} is outside the curve's domain")
            }
        }
    }
}

impl core::error::Error for ExpTableError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::common::{Provenance, SourceVersion};

    fn table(max_level: u16, totals: Vec<Exp>) -> ExpTable {
        ExpTable {
            provenance: Provenance {
                source_version: SourceVersion::V075,
                review: None,
            },
            max_level: Level::new(max_level).unwrap(),
            total_exp_by_level: totals,
        }
    }

    #[test]
    fn cap_total_equals_the_max_level_total() {
        let curve = ExpCurve::parse(table(4, vec![Exp(0), Exp(100), Exp(400), Exp(900)])).unwrap();
        assert_eq!(
            curve.cap_total(),
            curve
                .level(curve.max_level().get())
                .unwrap()
                .total_to_hold()
        );
        assert_eq!(curve.cap_total(), Exp(900));
    }

    #[test]
    fn an_empty_table_is_a_length_mismatch() {
        assert_eq!(
            ExpCurve::parse(table(3, Vec::new())),
            Err(ExpTableError::LengthMismatch {
                expected: 3,
                found: 0,
            })
        );
    }
}
