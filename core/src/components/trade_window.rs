//! One side of a player↔player trade: the [`Side`] addressing tag and the
//! [`TradeWindow`] — a fixed 4×8 escrow grid as a restricted-interface newtype
//! over the proven [`Inventory`] geometry. The wrapped inventory is private and
//! only the placement family is delegated; `absorb` is not, so stacking into a
//! trade window is unrepresentable, not merely never called. The sole
//! constructor hardcodes the dimensions and the wire re-proves them on parse,
//! so a window of any other size cannot exist.

use serde::{Deserialize, Serialize};

use crate::components::inventory::{Cell, Footprint, Inventory, PlacedItem, PlacementRejection};
use crate::components::item_instance::ItemInstance;

/// Which of the two trade parties an intent or event concerns. Core never sees
/// a player or account id — the host owns the id→side map. The labels persist
/// from the asymmetric request phase through the symmetric offer phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// The party that asked for the trade.
    Requester,
    /// The party that was asked.
    Partner,
}

impl Side {
    /// The opposite party — total, no default.
    #[must_use]
    pub fn other(self) -> Side {
        match self {
            Side::Requester => Side::Partner,
            Side::Partner => Side::Requester,
        }
    }
}

/// One side's 4×8 trade window. Every operation preserves the wrapped grid's
/// geometry invariants; no stack-merge path exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Inventory", into = "Inventory")]
pub struct TradeWindow(Inventory);

impl TradeWindow {
    /// The window's row count.
    pub const ROWS: u8 = 4;
    /// The window's column count.
    pub const COLS: u8 = 8;

    /// The only constructor — an empty 4×8 window. No dimension parameter
    /// exists, so an oversized window is a type impossibility, not a
    /// convention.
    #[must_use]
    pub fn empty() -> Self {
        Self(Inventory::empty(Self::ROWS, Self::COLS))
    }

    /// Places an item at `anchor` with `footprint`. On rejection the unchanged
    /// window and the bounced item are handed back with the reason.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::CellOutOfBounds`] when the footprint
    /// would leave the window, or [`PlacementRejection::CellsOccupied`] when
    /// it would overlap a placed item.
    pub fn place(
        self,
        anchor: Cell,
        footprint: Footprint,
        item: ItemInstance,
    ) -> Result<Self, (Self, ItemInstance, PlacementRejection)> {
        match self.0.place(anchor, footprint, item) {
            Ok(inventory) => Ok(Self(inventory)),
            Err((inventory, item, reason)) => Err((Self(inventory), item, reason)),
        }
    }

    /// Removes the item covering `cell`, handing out the item and its stored
    /// footprint — the window remembered it, so no definition lookup is needed
    /// to re-home the item. On rejection the unchanged window is handed back.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::NoItemAtCell`] when no item covers
    /// `cell`.
    pub fn remove(
        self,
        cell: Cell,
    ) -> Result<(Self, ItemInstance, Footprint), (Self, PlacementRejection)> {
        let Some(occupant) = self.0.occupant(cell) else {
            return Err((self, PlacementRejection::NoItemAtCell));
        };
        let footprint = occupant.footprint;
        match self.0.remove(cell) {
            Ok((inventory, item)) => Ok((Self(inventory), item, footprint)),
            // `occupant` proved the cell covered; the removal's own rejection
            // re-answers the presence question — total, never a panic.
            Err((inventory, reason)) => Err((Self(inventory), reason)),
        }
    }

    /// Moves the item covering `from` so its anchor is `to`, reusing its
    /// stored footprint. Rejects onto-occupied and never swaps. On rejection
    /// the unchanged window is handed back with the reason.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::NoItemAtCell`] when no item covers
    /// `from`, [`PlacementRejection::CellOutOfBounds`] when the moved
    /// footprint would leave the window, or
    /// [`PlacementRejection::CellsOccupied`] when it would overlap another
    /// placed item.
    pub fn move_to(self, from: Cell, to: Cell) -> Result<Self, (Self, PlacementRejection)> {
        match self.0.move_to(from, to) {
            Ok(inventory) => Ok(Self(inventory)),
            Err((inventory, reason)) => Err((Self(inventory), reason)),
        }
    }

    /// The item occupying `cell`, addressed by any cell its footprint covers.
    /// Genuine optionality: the cell may be empty.
    #[must_use]
    pub fn occupant(&self, cell: Cell) -> Option<&PlacedItem> {
        self.0.occupant(cell)
    }

    /// The placed items, in insertion order.
    #[must_use]
    pub fn placed(&self) -> &[PlacedItem] {
        self.0.placed()
    }
}

impl TryFrom<Inventory> for TradeWindow {
    type Error = TradeWindowError;

    fn try_from(inventory: Inventory) -> Result<Self, Self::Error> {
        // Parse-don't-validate: a hostile wire otherwise loads any size.
        if inventory.rows() != Self::ROWS || inventory.cols() != Self::COLS {
            return Err(TradeWindowError::WrongDimensions {
                rows: inventory.rows(),
                cols: inventory.cols(),
            });
        }
        Ok(Self(inventory))
    }
}

impl From<TradeWindow> for Inventory {
    fn from(window: TradeWindow) -> Self {
        window.0
    }
}

/// Rejection of a wire grid that is not 4×8 at the parse boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeWindowError {
    /// The parsed grid's dimensions are not the trade window's 4×8.
    WrongDimensions {
        /// The rejected row count.
        rows: u8,
        /// The rejected column count.
        cols: u8,
    },
}

impl core::fmt::Display for TradeWindowError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::WrongDimensions { rows, cols } => {
                write!(f, "a trade window must be 4x8, got {rows}x{cols}")
            }
        }
    }
}

impl core::error::Error for TradeWindowError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::item_instance::{
        CraftedAugment, Durability, LuckRoll, RarityRoll, SkillRoll,
    };
    use crate::components::item_ref::ItemRef;
    use crate::components::units::ItemLevel;

    fn item(number: u16) -> ItemInstance {
        ItemInstance {
            item: ItemRef { group: 0, number },
            level: ItemLevel::ZERO,
            roll: RarityRoll::Normal,
            normal_option: None,
            luck: LuckRoll::Plain,
            skill: SkillRoll::NoSkill,
            durability: Durability::full(30),
            augment: CraftedAugment::None,
        }
    }

    fn cell(row: u8, col: u8) -> Cell {
        Cell { row, col }
    }

    fn footprint(width: u8, height: u8) -> Footprint {
        Footprint::new(width, height).unwrap()
    }

    #[test]
    fn side_other_flips_the_parties() {
        assert_eq!(Side::Requester.other(), Side::Partner);
        assert_eq!(Side::Partner.other(), Side::Requester);
    }

    #[test]
    fn side_round_trips_as_a_bare_snake_case_string() {
        assert_eq!(
            serde_json::to_string(&Side::Requester).unwrap(),
            r#""requester""#
        );
        assert_eq!(
            serde_json::to_string(&Side::Partner).unwrap(),
            r#""partner""#
        );
        assert_eq!(
            serde_json::from_str::<Side>(r#""requester""#).unwrap(),
            Side::Requester
        );
        assert_eq!(
            serde_json::from_str::<Side>(r#""partner""#).unwrap(),
            Side::Partner
        );
    }

    #[test]
    fn empty_window_is_four_by_eight() {
        let window = TradeWindow::empty();
        assert!(window.placed().is_empty());
        // The 4-row bound: a 4-tall footprint places at row 0 but not row 1.
        assert!(
            window
                .clone()
                .place(cell(0, 0), footprint(1, 4), item(1))
                .is_ok()
        );
        let (_, _, reason) = window
            .clone()
            .place(cell(1, 0), footprint(1, 4), item(1))
            .unwrap_err();
        assert_eq!(reason, PlacementRejection::CellOutOfBounds);
        // The 8-column bound: an 8-wide footprint fits, a 9-wide never does.
        assert!(
            window
                .clone()
                .place(cell(0, 0), footprint(8, 1), item(2))
                .is_ok()
        );
        let (_, _, reason) = window
            .place(cell(0, 0), footprint(9, 1), item(2))
            .unwrap_err();
        assert_eq!(reason, PlacementRejection::CellOutOfBounds);
    }

    #[test]
    fn wire_reproves_the_dimensions_on_parse() {
        let window = TradeWindow::empty();
        let json = serde_json::to_string(&window).unwrap();
        assert_eq!(json, r#"{"rows":4,"cols":8,"placed":[]}"#);
        assert_eq!(serde_json::from_str::<TradeWindow>(&json).unwrap(), window);
        // A hostile wire of any other size is rejected on parse.
        assert!(serde_json::from_str::<TradeWindow>(r#"{"rows":8,"cols":8,"placed":[]}"#).is_err());
        assert!(
            serde_json::from_str::<TradeWindow>(r#"{"rows":99,"cols":99,"placed":[]}"#).is_err()
        );
    }

    #[test]
    fn place_and_occupant_delegate_to_the_grid_geometry() {
        let window = TradeWindow::empty()
            .place(cell(0, 0), footprint(2, 2), item(7))
            .unwrap();
        assert!(window.occupant(cell(1, 1)).is_some());
        assert!(window.occupant(cell(2, 2)).is_none());
        let (window, bounced, reason) = window
            .place(cell(1, 1), footprint(2, 2), item(8))
            .unwrap_err();
        assert_eq!(reason, PlacementRejection::CellsOccupied);
        assert_eq!(bounced.item.number, 8);
        assert_eq!(window.placed().len(), 1);
    }

    #[test]
    fn remove_hands_back_the_item_and_its_stored_footprint() {
        let window = TradeWindow::empty()
            .place(cell(0, 3), footprint(2, 3), item(5))
            .unwrap();
        let (window, removed, fp) = window.remove(cell(1, 4)).unwrap();
        assert_eq!(removed.item.number, 5);
        assert_eq!(fp, footprint(2, 3));
        assert!(window.placed().is_empty());
        let (_, reason) = TradeWindow::empty().remove(cell(0, 0)).unwrap_err();
        assert_eq!(reason, PlacementRejection::NoItemAtCell);
    }

    #[test]
    fn move_to_reanchors_within_the_window_and_never_swaps() {
        let window = TradeWindow::empty()
            .place(cell(0, 0), footprint(1, 1), item(1))
            .unwrap()
            .place(cell(2, 0), footprint(1, 1), item(2))
            .unwrap();
        let (window, reason) = window.move_to(cell(0, 0), cell(2, 0)).unwrap_err();
        assert_eq!(reason, PlacementRejection::CellsOccupied);
        let window = window.move_to(cell(0, 0), cell(0, 5)).unwrap();
        assert!(window.occupant(cell(0, 5)).is_some());
        assert!(window.occupant(cell(0, 0)).is_none());
    }

    #[test]
    fn a_placed_item_round_trips_the_wire_unchanged() {
        let window = TradeWindow::empty()
            .place(cell(1, 2), footprint(2, 2), item(42))
            .unwrap();
        let json = serde_json::to_string(&window).unwrap();
        let reparsed = serde_json::from_str::<TradeWindow>(&json).unwrap();
        assert_eq!(reparsed, window);
        assert_eq!(reparsed.occupant(cell(1, 2)).unwrap().item, item(42));
    }
}
