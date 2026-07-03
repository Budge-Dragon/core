//! Small closed level keys, one enum variant per level. Every curve, skin,
//! fee, or magnitude table keyed by one of these types is read through an
//! exhaustive match — total by the exhaustiveness proof, with no indexing,
//! no absent entries, and no wildcard arms. Wire mapping (`u8` <-> enum)
//! happens once, at the parse boundary.

use serde::{Deserialize, Serialize};

/// An enhancement level `+0..=+11` — the domain of every enhancement curve.
/// One variant per level, so any accessor keyed by this type is provably
/// total. Distinct from the wire item level, whose enhancement crossing is
/// the only bridge into this range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum EnhanceLevel {
    /// +0.
    L0,
    /// +1.
    L1,
    /// +2.
    L2,
    /// +3.
    L3,
    /// +4.
    L4,
    /// +5.
    L5,
    /// +6.
    L6,
    /// +7.
    L7,
    /// +8.
    L8,
    /// +9.
    L9,
    /// +10.
    L10,
    /// +11.
    L11,
}

impl EnhanceLevel {
    /// The wire value `0..=11`.
    #[must_use]
    pub fn wire(self) -> u8 {
        match self {
            Self::L0 => 0,
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
            Self::L4 => 4,
            Self::L5 => 5,
            Self::L6 => 6,
            Self::L7 => 7,
            Self::L8 => 8,
            Self::L9 => 9,
            Self::L10 => 10,
            Self::L11 => 11,
        }
    }
}

impl TryFrom<u8> for EnhanceLevel {
    type Error = LevelKeyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::L0),
            1 => Ok(Self::L1),
            2 => Ok(Self::L2),
            3 => Ok(Self::L3),
            4 => Ok(Self::L4),
            5 => Ok(Self::L5),
            6 => Ok(Self::L6),
            7 => Ok(Self::L7),
            8 => Ok(Self::L8),
            9 => Ok(Self::L9),
            10 => Ok(Self::L10),
            11 => Ok(Self::L11),
            value => Err(LevelKeyError::EnhanceAbove11 { value }),
        }
    }
}

impl From<EnhanceLevel> for u8 {
    fn from(level: EnhanceLevel) -> Self {
        level.wire()
    }
}

/// An ammunition option level `0..=2` — the domain of the ammunition
/// damage-percent table. One variant per level, read only through exhaustive
/// match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum AmmoLevel {
    /// +0.
    L0,
    /// +1.
    L1,
    /// +2.
    L2,
}

impl AmmoLevel {
    /// The wire value `0..=2`.
    #[must_use]
    pub fn wire(self) -> u8 {
        match self {
            Self::L0 => 0,
            Self::L1 => 1,
            Self::L2 => 2,
        }
    }
}

impl TryFrom<u8> for AmmoLevel {
    type Error = LevelKeyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::L0),
            1 => Ok(Self::L1),
            2 => Ok(Self::L2),
            value => Err(LevelKeyError::AmmoLevelAbove2 { value }),
        }
    }
}

impl From<AmmoLevel> for u8 {
    fn from(level: AmmoLevel) -> Self {
        level.wire()
    }
}

/// A transformation-ring level `0..=5` — the domain of the ring's
/// level-to-skin mapping. One variant per level, read only through exhaustive
/// match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum TransformationLevel {
    /// +0.
    L0,
    /// +1.
    L1,
    /// +2.
    L2,
    /// +3.
    L3,
    /// +4.
    L4,
    /// +5.
    L5,
}

impl TransformationLevel {
    /// The roster in declaration order — a fixed-length array keyed by the
    /// enum, so every level's skin is reachable through one total iteration.
    pub(crate) const ALL: [Self; 6] = [Self::L0, Self::L1, Self::L2, Self::L3, Self::L4, Self::L5];

    /// The wire value `0..=5`.
    #[must_use]
    pub fn wire(self) -> u8 {
        match self {
            Self::L0 => 0,
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
            Self::L4 => 4,
            Self::L5 => 5,
        }
    }
}

impl TryFrom<u8> for TransformationLevel {
    type Error = LevelKeyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::L0),
            1 => Ok(Self::L1),
            2 => Ok(Self::L2),
            3 => Ok(Self::L3),
            4 => Ok(Self::L4),
            5 => Ok(Self::L5),
            value => Err(LevelKeyError::TransformationLevelAbove5 { value }),
        }
    }
}

impl From<TransformationLevel> for u8 {
    fn from(level: TransformationLevel) -> Self {
        level.wire()
    }
}

/// A Jewel-of-Life option level `+1..=+4` — the level a normal option carries.
/// One variant per level, so every per-level magnitude accessor and every cap
/// comparison is an exhaustive match or an `Ord` comparison over the variants,
/// never an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum OptionLevel {
    /// +1.
    L1,
    /// +2.
    L2,
    /// +3.
    L3,
    /// +4.
    L4,
}

impl OptionLevel {
    /// The wire value `1..=4`.
    #[must_use]
    pub fn wire(self) -> u8 {
        match self {
            Self::L1 => 1,
            Self::L2 => 2,
            Self::L3 => 3,
            Self::L4 => 4,
        }
    }
}

impl TryFrom<u8> for OptionLevel {
    type Error = LevelKeyError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::L1),
            2 => Ok(Self::L2),
            3 => Ok(Self::L3),
            4 => Ok(Self::L4),
            value => Err(LevelKeyError::OptionLevelOutOfRange { value }),
        }
    }
}

impl From<OptionLevel> for u8 {
    fn from(level: OptionLevel) -> Self {
        level.wire()
    }
}

/// A Nova charge stage `1..=12` — the key of every per-stage Nova magnitude.
/// Charge state minted tick by tick as the caster charges, not parsed from a
/// wire integer; the twelve-variant bound is structural, so a thirteenth
/// stage is unrepresentable and no cap constant is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NovaStage {
    /// Stage 1.
    S1,
    /// Stage 2.
    S2,
    /// Stage 3.
    S3,
    /// Stage 4.
    S4,
    /// Stage 5.
    S5,
    /// Stage 6.
    S6,
    /// Stage 7.
    S7,
    /// Stage 8.
    S8,
    /// Stage 9.
    S9,
    /// Stage 10.
    S10,
    /// Stage 11.
    S11,
    /// Stage 12.
    S12,
}

impl NovaStage {
    /// The stage number `1..=12`.
    #[must_use]
    pub fn stage(self) -> u8 {
        match self {
            Self::S1 => 1,
            Self::S2 => 2,
            Self::S3 => 3,
            Self::S4 => 4,
            Self::S5 => 5,
            Self::S6 => 6,
            Self::S7 => 7,
            Self::S8 => 8,
            Self::S9 => 9,
            Self::S10 => 10,
            Self::S11 => 11,
            Self::S12 => 12,
        }
    }
}

/// A Devil's Square ticket level `1..=7` — the total key of the family's
/// per-level tables. Minted from a placed item's plus-level, never parsed
/// from a wire integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DevilSquareLevel {
    /// Level 1.
    One,
    /// Level 2.
    Two,
    /// Level 3.
    Three,
    /// Level 4.
    Four,
    /// Level 5.
    Five,
    /// Level 6.
    Six,
    /// Level 7.
    Seven,
}

impl DevilSquareLevel {
    /// Total per-level table lookup: destructures the array and matches all
    /// seven variants — no indexing, no `Option`.
    #[must_use]
    pub fn pick<T: Copy>(self, table: &[T; 7]) -> T {
        let [one, two, three, four, five, six, seven] = table;
        match self {
            Self::One => *one,
            Self::Two => *two,
            Self::Three => *three,
            Self::Four => *four,
            Self::Five => *five,
            Self::Six => *six,
            Self::Seven => *seven,
        }
    }
}

/// A Blood Castle ticket level `1..=8` — the total key of the family's
/// per-level tables. Minted from a placed item's plus-level, never parsed
/// from a wire integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BloodCastleLevel {
    /// Level 1.
    One,
    /// Level 2.
    Two,
    /// Level 3.
    Three,
    /// Level 4.
    Four,
    /// Level 5.
    Five,
    /// Level 6.
    Six,
    /// Level 7.
    Seven,
    /// Level 8.
    Eight,
}

impl BloodCastleLevel {
    /// Total per-level table lookup: destructures the array and matches all
    /// eight variants — no indexing, no `Option`.
    #[must_use]
    pub fn pick<T: Copy>(self, table: &[T; 8]) -> T {
        let [one, two, three, four, five, six, seven, eight] = table;
        match self {
            Self::One => *one,
            Self::Two => *two,
            Self::Three => *three,
            Self::Four => *four,
            Self::Five => *five,
            Self::Six => *six,
            Self::Seven => *seven,
            Self::Eight => *eight,
        }
    }
}

/// Rejection of an out-of-range level key at the parse boundary. Each
/// wire-parsed level enum contributes exactly its own rejection variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelKeyError {
    /// Enhancement level above the +11 cap.
    EnhanceAbove11 {
        /// The rejected wire value.
        value: u8,
    },
    /// Ammunition level above the +2 cap.
    AmmoLevelAbove2 {
        /// The rejected wire value.
        value: u8,
    },
    /// Transformation-ring level above the +5 cap.
    TransformationLevelAbove5 {
        /// The rejected wire value.
        value: u8,
    },
    /// Jewel-of-Life option level outside the `1..=4` range.
    OptionLevelOutOfRange {
        /// The rejected wire value.
        value: u8,
    },
}

impl core::fmt::Display for LevelKeyError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EnhanceAbove11 { value } => {
                write!(f, "enhance level {value} exceeds 11")
            }
            Self::AmmoLevelAbove2 { value } => {
                write!(f, "ammo level {value} exceeds 2")
            }
            Self::TransformationLevelAbove5 { value } => {
                write!(f, "transformation level {value} exceeds 5")
            }
            Self::OptionLevelOutOfRange { value } => {
                write!(f, "option level {value} is outside 1..=4")
            }
        }
    }
}

impl core::error::Error for LevelKeyError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enhance_level_round_trips_every_variant() {
        for wire in 0..=11u8 {
            let level = EnhanceLevel::try_from(wire).unwrap();
            assert_eq!(level.wire(), wire);
            assert_eq!(u8::from(level), wire);
        }
    }

    #[test]
    fn enhance_level_rejects_above_cap() {
        assert_eq!(
            EnhanceLevel::try_from(12),
            Err(LevelKeyError::EnhanceAbove11 { value: 12 })
        );
    }

    #[test]
    fn ammo_level_round_trips_and_rejects() {
        for wire in 0..=2u8 {
            assert_eq!(AmmoLevel::try_from(wire).unwrap().wire(), wire);
        }
        assert_eq!(
            AmmoLevel::try_from(3),
            Err(LevelKeyError::AmmoLevelAbove2 { value: 3 })
        );
    }

    #[test]
    fn transformation_level_round_trips_and_rejects() {
        for wire in 0..=5u8 {
            assert_eq!(TransformationLevel::try_from(wire).unwrap().wire(), wire);
        }
        assert_eq!(
            TransformationLevel::try_from(6),
            Err(LevelKeyError::TransformationLevelAbove5 { value: 6 })
        );
    }

    #[test]
    fn option_level_round_trips_and_rejects_zero_and_five() {
        for wire in 1..=4u8 {
            assert_eq!(OptionLevel::try_from(wire).unwrap().wire(), wire);
        }
        assert_eq!(
            OptionLevel::try_from(0),
            Err(LevelKeyError::OptionLevelOutOfRange { value: 0 })
        );
        assert_eq!(
            OptionLevel::try_from(5),
            Err(LevelKeyError::OptionLevelOutOfRange { value: 5 })
        );
    }

    #[test]
    fn option_level_orders_by_magnitude() {
        assert!(OptionLevel::L3 < OptionLevel::L4);
        assert!(OptionLevel::L1 < OptionLevel::L2);
    }

    #[test]
    fn nova_stage_reports_its_number() {
        assert_eq!(NovaStage::S1.stage(), 1);
        assert_eq!(NovaStage::S12.stage(), 12);
        assert!(NovaStage::S1 < NovaStage::S12);
    }

    #[test]
    fn devil_square_pick_is_total_over_every_variant() {
        let table = [10u32, 20, 30, 40, 50, 60, 70];
        assert_eq!(DevilSquareLevel::One.pick(&table), 10);
        assert_eq!(DevilSquareLevel::Four.pick(&table), 40);
        assert_eq!(DevilSquareLevel::Seven.pick(&table), 70);
    }

    #[test]
    fn blood_castle_pick_is_total_over_every_variant() {
        let table = [1u16, 2, 3, 4, 5, 6, 7, 8];
        assert_eq!(BloodCastleLevel::One.pick(&table), 1);
        assert_eq!(BloodCastleLevel::Eight.pick(&table), 8);
    }
}
