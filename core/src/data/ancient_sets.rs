//! Record shape of `ancient_sets.json` — the ancient set roster, plus the
//! load-built membership lookup.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::components::bonus::{CombatBonus, ConditionalSetBonus};

use super::common::{ItemRef, Provenance};

/// An ancient set's roster number (1..=36). Newtype over `NonZeroU8`; parsed at
/// the host boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SetNumber(
    /// The roster number.
    pub core::num::NonZeroU8,
);

/// One ancient set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AncientSet {
    /// Roster number. Also review-flagged in data: the 1..36 ordering is
    /// transcribed from OpenMU's S6 initializer pending verification against a
    /// classic `SetItemOption` client file.
    pub set_number: SetNumber,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// The pieces the set consists of.
    pub pieces: Vec<AncientSetPiece>,
    /// Set options in unlock order: k distinct equipped pieces (k >= 2) unlock
    /// the first k-1; the complete set unlocks all.
    pub set_options: Vec<AncientSetOption>,
}

/// One piece of an ancient set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AncientSetPiece {
    /// The item serving as this piece — the game's own `{group, number}`
    /// identity.
    pub item: ItemRef,
    /// Which of the at most two ancient sets of this base item the piece
    /// belongs to.
    pub discriminator: AncientDiscriminator,
    /// Stat granted by the per-piece +5/+10 bonus; absent = the piece carries
    /// none (Gywen's Pendant of Ability).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bonus_stat: Option<AncientPieceStat>,
}

/// The client's own 1|2 ancient-set selector on a base item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub enum AncientDiscriminator {
    /// Encoded 1.
    First,
    /// Encoded 2.
    Second,
}

impl AncientDiscriminator {
    /// The client-encoded value `1` or `2`.
    #[must_use]
    pub fn encoded(self) -> u8 {
        match self {
            Self::First => 1,
            Self::Second => 2,
        }
    }
}

impl TryFrom<u8> for AncientDiscriminator {
    type Error = AncientDiscriminatorOutOfRange;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::First),
            2 => Ok(Self::Second),
            value => Err(AncientDiscriminatorOutOfRange { value }),
        }
    }
}

impl From<AncientDiscriminator> for u8 {
    fn from(discriminator: AncientDiscriminator) -> Self {
        discriminator.encoded()
    }
}

/// Parse failure: an ancient discriminator byte outside the client's `1|2`
/// encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AncientDiscriminatorOutOfRange {
    /// The rejected wire value.
    pub value: u8,
}

impl core::fmt::Display for AncientDiscriminatorOutOfRange {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ancient discriminator {} is not 1 or 2", self.value)
    }
}

impl core::error::Error for AncientDiscriminatorOutOfRange {}

/// The stats an ancient piece bonus can raise — exactly the four observed in
/// the roster. Deliberately narrower than the class base-stat set (no command
/// piece bonus exists) so an impossible record cannot parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AncientPieceStat {
    /// Strength.
    Strength,
    /// Agility.
    Agility,
    /// Vitality.
    Vitality,
    /// Energy.
    Energy,
}

/// One entry of a set's ordered unlock sequence. A resolved entry is a
/// stats-owned `CombatBonus` serialized inline; a conditional entry is one of
/// the enumerated `ConditionalSetBonus` kinds. The outer `scope` tag names the
/// branch, so the two inner `kind` namespaces cannot collide at parse — a
/// mistyped scope is a parse error, not a silent misread into the wrong branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum AncientSetOption {
    /// A resolved, unconditional contribution.
    Resolved(CombatBonus),
    /// A contribution whose application depends on a runtime equipment fact.
    Conditional(ConditionalSetBonus),
}

/// Load-built structure over the roster: resolves an equipped item's ancient
/// membership. `None` is genuine optionality — most items are not ancient.
/// Construction proves each `(item, discriminator)` names at most one piece, so
/// [`membership`](Self::membership)'s answer is unambiguous by that proof, never
/// by scan order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncientRoster {
    sets: Vec<AncientSet>,
}

impl AncientRoster {
    /// Holds the roster for membership resolution, proving that no
    /// `(item, discriminator)` pair appears on two pieces.
    ///
    /// # Errors
    /// Returns [`AncientRosterError::DuplicateMembership`] when two pieces claim
    /// the same base item under the same ancient discriminator — the state that
    /// would make membership scan-order-dependent.
    pub fn build(sets: Vec<AncientSet>) -> Result<Self, AncientRosterError> {
        let mut seen = BTreeSet::new();
        for set in &sets {
            for piece in &set.pieces {
                if !seen.insert((piece.item, piece.discriminator)) {
                    return Err(AncientRosterError::DuplicateMembership {
                        item: piece.item,
                        discriminator: piece.discriminator,
                    });
                }
            }
        }
        Ok(Self { sets })
    }

    /// The sets in load order.
    #[must_use]
    pub fn sets(&self) -> impl ExactSizeIterator<Item = &AncientSet> {
        self.sets.iter()
    }

    /// The set and piece a concrete ancient item belongs to; `None` when the
    /// item is not an ancient piece — the one genuine optionality. Uniqueness
    /// proven at construction, so the first match is the only match.
    #[must_use]
    pub fn membership(
        &self,
        item: ItemRef,
        discriminator: AncientDiscriminator,
    ) -> Option<(&AncientSet, &AncientSetPiece)> {
        self.sets.iter().find_map(|set| {
            set.pieces
                .iter()
                .find(|piece| piece.item == item && piece.discriminator == discriminator)
                .map(|piece| (set, piece))
        })
    }
}

/// Load failure assembling the ancient roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AncientRosterError {
    /// Two pieces claim the same base item under the same ancient
    /// discriminator, so membership would depend on scan order.
    DuplicateMembership {
        /// The base item claimed twice.
        item: ItemRef,
        /// The discriminator both pieces share.
        discriminator: AncientDiscriminator,
    },
}

impl core::fmt::Display for AncientRosterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::DuplicateMembership {
                item,
                discriminator,
            } => write!(
                f,
                "item {item:?} appears on two ancient pieces under discriminator {discriminator:?}"
            ),
        }
    }
}

impl core::error::Error for AncientRosterError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::units::Percent;

    #[test]
    fn resolved_option_round_trips_under_the_scope_tag() {
        let opt = AncientSetOption::Resolved(CombatBonus::Strength { amount: 10 });
        let json = serde_json::to_string(&opt).unwrap();
        assert_eq!(
            json,
            r#"{"scope":"resolved","kind":"strength","amount":10}"#
        );
        assert_eq!(
            serde_json::from_str::<AncientSetOption>(&json).unwrap(),
            opt
        );
    }

    #[test]
    fn conditional_option_round_trips_under_the_scope_tag() {
        let opt = AncientSetOption::Conditional(ConditionalSetBonus::DefenseWithShieldPct {
            percent: Percent::new(25).unwrap(),
        });
        let json = serde_json::to_string(&opt).unwrap();
        assert_eq!(
            json,
            r#"{"scope":"conditional","kind":"defense_with_shield_pct","percent":25}"#
        );
        assert_eq!(
            serde_json::from_str::<AncientSetOption>(&json).unwrap(),
            opt
        );
    }

    #[test]
    fn a_conditional_kind_under_the_resolved_scope_is_a_parse_error() {
        // The outer scope tag type-guards the split: a conditional-only kind
        // cannot silently misread into `Resolved`.
        let bad = r#"{"scope":"resolved","kind":"defense_with_shield_pct","percent":25}"#;
        assert!(serde_json::from_str::<AncientSetOption>(bad).is_err());
    }

    #[test]
    fn build_rejects_a_duplicated_membership() {
        let piece = AncientSetPiece {
            item: ItemRef {
                group: 8,
                number: 5,
            },
            discriminator: AncientDiscriminator::First,
            bonus_stat: None,
        };
        let set = |set_number: u8| AncientSet {
            set_number: SetNumber(core::num::NonZeroU8::new(set_number).unwrap()),
            provenance: Provenance {
                source_version: super::super::common::SourceVersion::S6,
                review: None,
            },
            pieces: vec![piece],
            set_options: Vec::new(),
        };
        let err = AncientRoster::build(vec![set(1), set(2)]).unwrap_err();
        assert!(matches!(
            err,
            AncientRosterError::DuplicateMembership { .. }
        ));
    }
}
