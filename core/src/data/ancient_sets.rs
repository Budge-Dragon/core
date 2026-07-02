//! Record shape of `ancient_sets.json` — the ancient set roster, plus the
//! load-built membership lookup.

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
    /// Client-visible set name — display/debug only, never a key.
    pub name: String,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
/// the enumerated `ConditionalSetBonus` kinds. The two kind namespaces are
/// disjoint, so the untagged split is unambiguous at parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AncientSetOption {
    /// A resolved, unconditional contribution.
    Resolved(CombatBonus),
    /// A contribution whose application depends on a runtime equipment fact.
    Conditional(ConditionalSetBonus),
}

/// Load-built structure over the roster: resolves an equipped item's ancient
/// membership. `None` is genuine optionality — most items are not ancient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AncientRoster {
    sets: Vec<AncientSet>,
}

impl AncientRoster {
    /// Holds the roster for membership resolution.
    #[must_use]
    pub fn build(sets: Vec<AncientSet>) -> Self {
        Self { sets }
    }

    /// The sets in load order.
    #[must_use]
    pub fn sets(&self) -> &[AncientSet] {
        &self.sets
    }

    /// The set and piece a concrete ancient item belongs to; `None` when the
    /// item is not an ancient piece — the one genuine optionality, resolved by
    /// a total scan with no index round-trip.
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
