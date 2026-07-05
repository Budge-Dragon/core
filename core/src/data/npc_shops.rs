//! Record shape of `npc_shops.json` — the eleven era merchants' static shelf
//! catalogs.
//!
//! Each record is one merchant's shelf: which NPC, provenance, and the shelf
//! entries. An entry carries its 8×15 anchor cell, its item identity, its
//! plus-level, and a kind-tagged stock holding only the facts its
//! materialization family configures. Anchor uniqueness, footprint fit, and
//! no-overlap are extractor-proven and re-proven at Atlas parse — never a
//! field invariant here.

use core::num::NonZeroU8;

use serde::{Deserialize, Serialize};

use crate::components::item_instance::{LuckRoll, RolledNormalOption, SkillRoll};
use crate::components::levels::EnhanceLevel;

use super::common::{ItemRef, MonsterNumber, Provenance};

/// One merchant's shelf catalog: which NPC, provenance, and the shelf entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerchantShop {
    /// The merchant NPC number (a merchant-window monster definition).
    pub npc: MonsterNumber,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
    /// The shelf entries. Anchor uniqueness and 8×15 no-overlap are proven at
    /// the extractor and re-proven at Atlas parse — not a field invariant
    /// here.
    pub shelf: Vec<ShelfEntry>,
}

/// A shelf anchor cell as the classic `row*8 + col` byte on the 8×15 grid.
/// The byte is proven in-grid at construction and on the wire; footprint fit
/// and no-overlap are a parse-time geometric proof over the joined
/// definitions, not a field invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct ShelfSlot(u8);

impl ShelfSlot {
    // Design pins, NOT W-SRC: OpenMU has no server-side grid bound (consult
    // §E.1.2, open question 7). 8 columns is slot arithmetic, 15 rows the
    // classic client window (spec L6).
    /// Columns of the shelf grid.
    pub const COLUMNS: u8 = 8;
    /// Rows of the shelf grid.
    pub const ROWS: u8 = 15;
    /// Total cells of the shelf grid.
    pub const CELLS: u8 = Self::COLUMNS * Self::ROWS;

    /// Builds a shelf slot; a byte past the grid is rejected.
    ///
    /// # Errors
    /// Returns [`ShopSchemaError::SlotOutOfGrid`] when `byte` is not below
    /// [`Self::CELLS`].
    pub fn new(byte: u8) -> Result<Self, ShopSchemaError> {
        if byte >= Self::CELLS {
            return Err(ShopSchemaError::SlotOutOfGrid { byte });
        }
        Ok(Self(byte))
    }

    /// The classic wire byte.
    #[must_use]
    pub const fn byte(self) -> u8 {
        self.0
    }

    /// Row index (0-based, top row first).
    #[must_use]
    pub const fn row(self) -> u8 {
        self.0 / Self::COLUMNS
    }

    /// Column index (0-based, left column first).
    #[must_use]
    pub const fn col(self) -> u8 {
        self.0 % Self::COLUMNS
    }
}

impl TryFrom<u8> for ShelfSlot {
    type Error = ShopSchemaError;

    fn try_from(byte: u8) -> Result<Self, Self::Error> {
        Self::new(byte)
    }
}

impl From<ShelfSlot> for u8 {
    fn from(slot: ShelfSlot) -> Self {
        slot.0
    }
}

/// Rejection of an out-of-grid shelf byte at the parse boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopSchemaError {
    /// The anchor byte falls past the 8×15 grid's 120 cells.
    SlotOutOfGrid {
        /// The rejected wire byte.
        byte: u8,
    },
}

impl core::fmt::Display for ShopSchemaError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::SlotOutOfGrid { byte } => {
                write!(f, "shelf slot {byte} is outside the 8x15 grid")
            }
        }
    }
}

impl core::error::Error for ShopSchemaError {}

/// One shelf entry: where it sits, what it is, its plus-level, and its
/// family-specific materialization facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShelfEntry {
    /// The 8×15 anchor cell.
    pub slot: ShelfSlot,
    /// The item identity (joined to a definition at Atlas parse).
    pub item: ItemRef,
    /// The configured plus-level. `EnhanceLevel`, so full-durability
    /// materialization is total by construction — no wire-level `None` case.
    pub level: EnhanceLevel,
    /// The family-specific facts that decide materialization and merge.
    #[serde(flatten)]
    pub stock: ShelfStock,
    /// Curation note on the entry (the two Hanzo fixes, the Vine mixed
    /// levels, the two option-2 shields); absent = uncontested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<String>,
}

/// What a shelf entry materializes into, kind-tagged. One variant per
/// materialization shape, so an ammo entry carrying a stack count — or a
/// potion carrying a skill roll — is unrepresentable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "stock", rename_all = "snake_case")]
pub enum ShelfStock {
    /// A wearable piece: roll facts configured on the shelf; materialized at
    /// full durability for its level. The only merge-ineligible family that
    /// carries roll facts.
    Gear {
        /// Whether the piece carries luck.
        luck: LuckRoll,
        /// Whether the piece carries its weapon skill.
        skill: SkillRoll,
        /// A pre-applied Jewel-of-Life normal option — genuine optionality
        /// (two era shields carry a level-2 option; most gear carries none).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        option: Option<RolledNormalOption>,
    },
    /// A stackable consumable: the shelf pack's piece count (potions ×1/×3,
    /// Apple, Antidote). The ONLY merge-eligible family. `pieces` is `1..=`
    /// the definition's stack cap (its durability), proven at Atlas parse.
    Stack {
        /// Pieces in the shelf pack.
        pieces: NonZeroU8,
    },
    /// Ammunition: one full quiver per purchase, no stack field — a "quiver
    /// with a stack count" is unrepresentable. Materialized to the
    /// definition's full round count.
    Quiver,
    /// A single durability-1 piece: skill scrolls, orbs and summon orbs, and
    /// the durability-1 consumables (Ale, Town Portal Scroll). Materialized
    /// at durability 1; never merge-eligible.
    Single,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_options::NormalOption;
    use crate::components::levels::OptionLevel;
    use crate::data::common::{DataFile, SourceVersion};

    #[test]
    fn shelf_slot_bounds_and_arithmetic() {
        assert_eq!(ShelfSlot::CELLS, 120);
        let last = ShelfSlot::new(119).unwrap();
        assert_eq!(last.row(), 14);
        assert_eq!(last.col(), 7);
        let mid = ShelfSlot::new(50).unwrap();
        assert_eq!(mid.byte(), 50);
        assert_eq!(mid.row(), 6);
        assert_eq!(mid.col(), 2);
        assert_eq!(
            ShelfSlot::new(120),
            Err(ShopSchemaError::SlotOutOfGrid { byte: 120 })
        );
    }

    #[test]
    fn shelf_slot_wire_rejects_an_out_of_grid_byte() {
        assert!(serde_json::from_str::<ShelfSlot>("119").is_ok());
        assert!(serde_json::from_str::<ShelfSlot>("120").is_err());
    }

    #[test]
    fn gear_entry_wire_round_trips_with_the_flattened_stock_tag() {
        let entry = ShelfEntry {
            slot: ShelfSlot::new(24).unwrap(),
            item: ItemRef {
                group: 8,
                number: 3,
            },
            level: EnhanceLevel::L3,
            stock: ShelfStock::Gear {
                luck: LuckRoll::Plain,
                skill: SkillRoll::NoSkill,
                option: None,
            },
            review: None,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert_eq!(
            json,
            r#"{"slot":24,"item":{"group":8,"number":3},"level":3,"stock":"gear","luck":"plain","skill":"no_skill"}"#
        );
        assert_eq!(serde_json::from_str::<ShelfEntry>(&json).unwrap(), entry);
    }

    #[test]
    fn gear_entry_parses_a_preapplied_option() {
        let json = r#"{"slot":78,"item":{"group":6,"number":3},"level":3,"stock":"gear","luck":"lucky","skill":"no_skill","option":{"option":"defense_rate","level":2}}"#;
        let entry: ShelfEntry = serde_json::from_str(json).unwrap();
        assert_eq!(
            entry.stock,
            ShelfStock::Gear {
                luck: LuckRoll::Lucky,
                skill: SkillRoll::NoSkill,
                option: Some(RolledNormalOption {
                    option: NormalOption::DefenseRate,
                    level: OptionLevel::L2,
                }),
            }
        );
    }

    #[test]
    fn stack_quiver_and_single_wire_shapes_round_trip() {
        let stack =
            r#"{"slot":8,"item":{"group":14,"number":0},"level":0,"stock":"stack","pieces":3}"#;
        let entry: ShelfEntry = serde_json::from_str(stack).unwrap();
        assert_eq!(
            entry.stock,
            ShelfStock::Stack {
                pieces: NonZeroU8::new(3).unwrap(),
            }
        );
        assert_eq!(serde_json::to_string(&entry).unwrap(), stack);

        let quiver = r#"{"slot":22,"item":{"group":4,"number":7},"level":0,"stock":"quiver"}"#;
        let entry: ShelfEntry = serde_json::from_str(quiver).unwrap();
        assert_eq!(entry.stock, ShelfStock::Quiver);
        assert_eq!(serde_json::to_string(&entry).unwrap(), quiver);

        let single = r#"{"slot":24,"item":{"group":12,"number":11},"level":4,"stock":"single"}"#;
        let entry: ShelfEntry = serde_json::from_str(single).unwrap();
        assert_eq!(entry.stock, ShelfStock::Single);
        assert_eq!(entry.level, EnhanceLevel::L4);
        assert_eq!(serde_json::to_string(&entry).unwrap(), single);
    }

    #[test]
    fn a_zero_piece_stack_is_rejected_on_the_wire() {
        let json =
            r#"{"slot":8,"item":{"group":14,"number":0},"level":0,"stock":"stack","pieces":0}"#;
        assert!(serde_json::from_str::<ShelfEntry>(json).is_err());
    }

    #[test]
    fn an_entry_level_past_the_enhance_cap_is_rejected() {
        let json = r#"{"slot":0,"item":{"group":0,"number":0},"level":12,"stock":"gear","luck":"plain","skill":"no_skill"}"#;
        assert!(serde_json::from_str::<ShelfEntry>(json).is_err());
    }

    #[test]
    fn record_envelope_parses_provenance_and_entry_review() {
        let json = r#"{"records":[{"npc":251,"source_version":"075","shelf":[
            {"slot":76,"item":{"group":0,"number":7},"level":3,"stock":"gear","luck":"lucky","skill":"with_skill","review":"moved off the duplicate anchor"}
        ]}]}"#;
        let file: DataFile<MerchantShop> = serde_json::from_str(json).unwrap();
        let record = file.records.first().unwrap();
        assert_eq!(record.npc, MonsterNumber(251));
        assert_eq!(record.provenance.source_version, SourceVersion::V075);
        assert_eq!(record.provenance.review, None);
        let entry = record.shelf.first().unwrap();
        assert_eq!(
            entry.review.as_deref(),
            Some("moved off the duplicate anchor")
        );
    }
}
