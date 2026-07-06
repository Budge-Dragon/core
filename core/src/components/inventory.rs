//! The grid container an item instance occupies a cell rectangle in. The
//! component owns *only* the geometry/presence invariants — every footprint in
//! bounds, no two footprints overlapping — proven at construction and re-proven
//! on parse, exactly the [`crate::components::pool::Pool`] grain. Kind, slot,
//! and class-wear rules need a definition and belong to the inventory service;
//! the container never reaches into `data`. Footprints enter as intent (the
//! host did the Atlas lookup).
//!
//! Operations are value-in/value-out: they take `self` and return the new
//! state. A rejected operation returns the unchanged inventory (it holds a
//! `Vec`, so it is not `Copy` and must not be lost), and `place` also hands the
//! bounced [`ItemInstance`] back (it is move-only, so it would otherwise be
//! destroyed).

use core::num::NonZeroU8;

use serde::{Deserialize, Serialize};

use crate::components::item_instance::{Durability, ItemInstance};

/// A grid cell coordinate. Plain data — in-bounds is checked against the grid's
/// `rows`/`cols`, not a field invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    /// Row index (0-based).
    pub row: u8,
    /// Column index (0-based).
    pub col: u8,
}

/// An item's cell footprint. `NonZeroU8` on both axes makes a zero-size item
/// unrepresentable; built at the host parse boundary from the definition's
/// `width`/`height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    width: NonZeroU8,
    height: NonZeroU8,
}

impl Footprint {
    /// Builds a footprint, rejecting a zero width or height.
    ///
    /// # Errors
    /// Returns [`FootprintError::ZeroWidth`] or [`FootprintError::ZeroHeight`]
    /// when the corresponding dimension is zero.
    pub fn new(width: u8, height: u8) -> Result<Self, FootprintError> {
        let width = NonZeroU8::new(width).ok_or(FootprintError::ZeroWidth)?;
        let height = NonZeroU8::new(height).ok_or(FootprintError::ZeroHeight)?;
        Ok(Self { width, height })
    }

    /// The width in cells.
    #[must_use]
    pub const fn width(self) -> NonZeroU8 {
        self.width
    }

    /// The height in cells.
    #[must_use]
    pub const fn height(self) -> NonZeroU8 {
        self.height
    }
}

/// Rejection of a zero-size footprint at construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FootprintError {
    /// The width was zero.
    ZeroWidth,
    /// The height was zero.
    ZeroHeight,
}

impl core::fmt::Display for FootprintError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ZeroWidth => write!(f, "a footprint must have a nonzero width"),
            Self::ZeroHeight => write!(f, "a footprint must have a nonzero height"),
        }
    }
}

impl core::error::Error for FootprintError {}

/// One placed item: where it anchors, how large it is, and the instance itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacedItem {
    /// The top-left cell the footprint anchors at.
    pub anchor: Cell,
    /// The item's cell footprint.
    pub footprint: Footprint,
    /// The item instance occupying the footprint.
    pub item: ItemInstance,
}

/// The half-open cell rectangle a placed item covers, in `u16` so an anchor
/// plus a footprint can never overflow.
#[derive(Clone, Copy)]
struct Rect {
    row_lo: u16,
    row_hi: u16,
    col_lo: u16,
    col_hi: u16,
}

impl Rect {
    fn of(anchor: Cell, footprint: Footprint) -> Self {
        let row_lo = u16::from(anchor.row);
        let col_lo = u16::from(anchor.col);
        Self {
            row_lo,
            row_hi: row_lo.saturating_add(u16::from(footprint.height().get())),
            col_lo,
            col_hi: col_lo.saturating_add(u16::from(footprint.width().get())),
        }
    }

    fn in_bounds(self, rows: u8, cols: u8) -> bool {
        self.row_hi <= u16::from(rows) && self.col_hi <= u16::from(cols)
    }

    fn contains_cell(self, cell: Cell) -> bool {
        let row = u16::from(cell.row);
        let col = u16::from(cell.col);
        self.row_lo <= row && row < self.row_hi && self.col_lo <= col && col < self.col_hi
    }

    fn overlaps(self, other: Rect) -> bool {
        self.row_lo < other.row_hi
            && other.row_lo < self.row_hi
            && self.col_lo < other.col_hi
            && other.col_lo < self.col_hi
    }
}

/// Wire mirror of [`Inventory`]; the geometry invariants are re-proven on the
/// way in by folding each placed item through [`Inventory::place`], since a
/// persisted container loaded from a host is untrusted.
#[derive(Serialize, Deserialize)]
struct InventoryWire {
    rows: u8,
    cols: u8,
    placed: Vec<PlacedItem>,
}

/// A `rows x cols` storage grid holding placed items. The invariant — every
/// footprint in bounds, no two overlapping — is preserved by every operation
/// and re-proven on parse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "InventoryWire", into = "InventoryWire")]
pub struct Inventory {
    rows: u8,
    cols: u8,
    placed: Vec<PlacedItem>,
}

impl Inventory {
    /// An empty grid of the given size — the host builds it from a
    /// [`crate::data::game_config::GridSize`] at the boundary.
    #[must_use]
    pub fn empty(rows: u8, cols: u8) -> Self {
        Self {
            rows,
            cols,
            placed: Vec::new(),
        }
    }

    /// The number of grid rows.
    #[must_use]
    pub const fn rows(&self) -> u8 {
        self.rows
    }

    /// The number of grid columns.
    #[must_use]
    pub const fn cols(&self) -> u8 {
        self.cols
    }

    /// The placed items, in insertion order.
    #[must_use]
    pub fn placed(&self) -> &[PlacedItem] {
        &self.placed
    }

    /// The item occupying `cell`, addressed by any cell its footprint covers —
    /// not just its anchor. Genuine optionality: the cell may be empty.
    #[must_use]
    pub fn occupant(&self, cell: Cell) -> Option<&PlacedItem> {
        self.placed
            .iter()
            .find(|item| Rect::of(item.anchor, item.footprint).contains_cell(cell))
    }

    /// The index and footprint of the item covering `cell`, if any — the seam
    /// `remove`/`move_to` locate an occupant through without re-indexing.
    fn locate(&self, cell: Cell) -> Option<(usize, Footprint)> {
        self.placed.iter().enumerate().find_map(|(index, item)| {
            Rect::of(item.anchor, item.footprint)
                .contains_cell(cell)
                .then_some((index, item.footprint))
        })
    }

    /// Places an item at `anchor` with `footprint`. On success the item is
    /// stored; on rejection the unchanged inventory and the bounced item are
    /// handed back with the reason.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::CellOutOfBounds`] when the footprint would
    /// leave the grid, or [`PlacementRejection::CellsOccupied`] when it would
    /// overlap a placed item.
    pub fn place(
        self,
        anchor: Cell,
        footprint: Footprint,
        item: ItemInstance,
    ) -> Result<Self, (Self, ItemInstance, PlacementRejection)> {
        let candidate = Rect::of(anchor, footprint);
        if !candidate.in_bounds(self.rows, self.cols) {
            return Err((self, item, PlacementRejection::CellOutOfBounds));
        }
        let clashes = self
            .placed
            .iter()
            .any(|placed| candidate.overlaps(Rect::of(placed.anchor, placed.footprint)));
        if clashes {
            return Err((self, item, PlacementRejection::CellsOccupied));
        }
        let mut placed = self.placed;
        placed.push(PlacedItem {
            anchor,
            footprint,
            item,
        });
        Ok(Self {
            rows: self.rows,
            cols: self.cols,
            placed,
        })
    }

    /// Removes the item covering `cell`, handing it out. On rejection the
    /// unchanged inventory is handed back with the reason.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::NoItemAtCell`] when no item covers `cell`.
    pub fn remove(self, cell: Cell) -> Result<(Self, ItemInstance), (Self, PlacementRejection)> {
        let Some((index, _)) = self.locate(cell) else {
            return Err((self, PlacementRejection::NoItemAtCell));
        };
        let mut placed = self.placed;
        let removed = placed.remove(index);
        Ok((
            Self {
                rows: self.rows,
                cols: self.cols,
                placed,
            },
            removed.item,
        ))
    }

    /// The first anchor `footprint` fits at, scanning row-major — row 0 left
    /// to right, then row 1, and so on. Genuine optionality: a grid with no
    /// fitting region has no anchor.
    #[must_use]
    pub fn first_fit(&self, footprint: Footprint) -> Option<Cell> {
        (0..self.rows).find_map(|row| {
            (0..self.cols).find_map(|col| {
                let anchor = Cell { row, col };
                let candidate = Rect::of(anchor, footprint);
                let fits = candidate.in_bounds(self.rows, self.cols)
                    && !self.placed.iter().any(|placed| {
                        candidate.overlaps(Rect::of(placed.anchor, placed.footprint))
                    });
                fits.then_some(anchor)
            })
        })
    }

    /// Raises the gauge of the item covering `cell` by `add` — the whole-pack
    /// stack absorb of a buy merge (a stack's piece count is its
    /// `durability.current()`). Data-free: the ceiling is the target's own
    /// `durability.max()`, never a definition lookup; the stackability gate
    /// and the identity match live in the service holding the resolved
    /// definition. On rejection the unchanged inventory is handed back.
    ///
    /// # Errors
    /// Returns [`AbsorbRejection::NoItemAtCell`] when no item covers `cell`,
    /// or [`AbsorbRejection::WouldOverflow`] when `current + add` exceeds the
    /// target's own maximum — the whole pack must fit; no partial absorb
    /// exists.
    pub fn absorb(self, cell: Cell, add: NonZeroU8) -> Result<Self, (Self, AbsorbRejection)> {
        let Some(occupant) = self.occupant(cell) else {
            return Err((self, AbsorbRejection::NoItemAtCell));
        };
        let anchor = occupant.anchor;
        let gauge = occupant.item.durability;
        let Some(sum) = gauge.current().checked_add(add.get()) else {
            return Err((self, AbsorbRejection::WouldOverflow));
        };
        // The gauge constructor IS the ceiling proof — its rejection is the
        // overflow answer, never a re-checked precondition.
        let Ok(raised) = Durability::new(sum, gauge.max()) else {
            return Err((self, AbsorbRejection::WouldOverflow));
        };
        let placed = self
            .placed
            .into_iter()
            .map(|mut placed| {
                if placed.anchor == anchor {
                    placed.item.durability = raised;
                }
                placed
            })
            .collect();
        Ok(Self {
            rows: self.rows,
            cols: self.cols,
            placed,
        })
    }

    /// Consumes one piece of the stack covering `cell`: lowers its gauge by one
    /// (a stack's piece count is its `durability.current()`), or removes the
    /// whole item when the last piece is consumed — no zero-count ghost cell
    /// survives. The decrement twin of [`Self::absorb`]; the whole-item removal
    /// on the last piece is folded here, not banned inside [`Durability`]. On
    /// rejection the unchanged inventory is handed back.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::NoItemAtCell`] when no item covers `cell`.
    pub fn consume_one(self, cell: Cell) -> Result<Self, (Self, PlacementRejection)> {
        let Some((index, _)) = self.locate(cell) else {
            return Err((self, PlacementRejection::NoItemAtCell));
        };
        let mut placed = self.placed;
        let mut consumed = placed.remove(index);
        if let Some(lowered) = consumed.item.durability.decremented() {
            consumed.item.durability = lowered;
            placed.insert(index, consumed);
        }
        Ok(Self {
            rows: self.rows,
            cols: self.cols,
            placed,
        })
    }

    /// Moves the item covering `from` so its anchor is `to`, reusing its stored
    /// footprint. Self-overlap on the moving item's own old cells is legal — the
    /// overlap test excludes it. On rejection the unchanged inventory is handed
    /// back with the reason.
    ///
    /// # Errors
    /// Returns [`PlacementRejection::NoItemAtCell`] when no item covers `from`,
    /// [`PlacementRejection::CellOutOfBounds`] when the moved footprint would
    /// leave the grid, or [`PlacementRejection::CellsOccupied`] when it would
    /// overlap another placed item.
    pub fn move_to(self, from: Cell, to: Cell) -> Result<Self, (Self, PlacementRejection)> {
        let Some((index, footprint)) = self.locate(from) else {
            return Err((self, PlacementRejection::NoItemAtCell));
        };
        let candidate = Rect::of(to, footprint);
        if !candidate.in_bounds(self.rows, self.cols) {
            return Err((self, PlacementRejection::CellOutOfBounds));
        }
        let clashes = self.placed.iter().enumerate().any(|(other, placed)| {
            other != index && candidate.overlaps(Rect::of(placed.anchor, placed.footprint))
        });
        if clashes {
            return Err((self, PlacementRejection::CellsOccupied));
        }
        let placed = self
            .placed
            .into_iter()
            .enumerate()
            .map(|(other, placed)| {
                if other == index {
                    PlacedItem {
                        anchor: to,
                        footprint: placed.footprint,
                        item: placed.item,
                    }
                } else {
                    placed
                }
            })
            .collect();
        Ok(Self {
            rows: self.rows,
            cols: self.cols,
            placed,
        })
    }
}

impl TryFrom<InventoryWire> for Inventory {
    type Error = PlacementRejection;

    fn try_from(wire: InventoryWire) -> Result<Self, Self::Error> {
        let mut inventory = Self::empty(wire.rows, wire.cols);
        for placed in wire.placed {
            inventory = match inventory.place(placed.anchor, placed.footprint, placed.item) {
                Ok(inventory) => inventory,
                Err((_, _, reason)) => return Err(reason),
            };
        }
        Ok(inventory)
    }
}

impl From<Inventory> for InventoryWire {
    fn from(inventory: Inventory) -> Self {
        Self {
            rows: inventory.rows,
            cols: inventory.cols,
            placed: inventory.placed,
        }
    }
}

/// Why a container operation was rejected — component-owned, the geometry and
/// presence outcomes the inventory itself decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlacementRejection {
    /// The footprint would leave the grid.
    CellOutOfBounds,
    /// The footprint would overlap a placed item.
    CellsOccupied,
    /// No item covers the addressed cell.
    NoItemAtCell,
}

impl core::fmt::Display for PlacementRejection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CellOutOfBounds => write!(f, "the footprint leaves the grid"),
            Self::CellsOccupied => write!(f, "the footprint overlaps a placed item"),
            Self::NoItemAtCell => write!(f, "no item covers the addressed cell"),
        }
    }
}

impl core::error::Error for PlacementRejection {}

/// Why a stack absorb was rejected — component-owned, decided from the
/// target's presence and its own gauge ceiling alone. Plain (non-serde):
/// consumed and re-mapped by services onto their own wire outcomes; it
/// never crosses a port itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbsorbRejection {
    /// No item covers the addressed cell.
    NoItemAtCell,
    /// The whole addition would push the gauge past its own maximum.
    WouldOverflow,
}

impl core::fmt::Display for AbsorbRejection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::NoItemAtCell => write!(f, "no item covers the addressed cell"),
            Self::WouldOverflow => write!(f, "the addition would overflow the gauge"),
        }
    }
}

impl core::error::Error for AbsorbRejection {}

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
    fn footprint_rejects_zero_dimensions() {
        assert_eq!(Footprint::new(0, 1), Err(FootprintError::ZeroWidth));
        assert_eq!(Footprint::new(1, 0), Err(FootprintError::ZeroHeight));
        assert!(serde_json::from_str::<Footprint>(r#"{"width":0,"height":2}"#).is_err());
    }

    #[test]
    fn place_stores_and_occupant_addresses_any_covered_cell() {
        let inventory = Inventory::empty(8, 8);
        let inventory = inventory
            .place(cell(1, 1), footprint(2, 3), item(3))
            .unwrap();
        assert_eq!(inventory.placed().len(), 1);
        // A 2-wide, 3-tall footprint at (row 1, col 1) covers rows 1..4, cols 1..3.
        assert!(inventory.occupant(cell(1, 1)).is_some());
        assert!(inventory.occupant(cell(3, 2)).is_some());
        assert!(inventory.occupant(cell(0, 1)).is_none());
        assert!(inventory.occupant(cell(1, 3)).is_none());
    }

    #[test]
    fn place_out_of_bounds_hands_item_back_unchanged() {
        let inventory = Inventory::empty(4, 4);
        let (inventory, item, reason) = inventory
            .place(cell(3, 3), footprint(2, 2), item(3))
            .unwrap_err();
        assert_eq!(reason, PlacementRejection::CellOutOfBounds);
        assert_eq!(item.item.number, 3);
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn place_over_an_occupied_cell_is_rejected() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(2, 2), item(1))
            .unwrap();
        let (inventory, item, reason) = inventory
            .place(cell(1, 1), footprint(2, 2), item(2))
            .unwrap_err();
        assert_eq!(reason, PlacementRejection::CellsOccupied);
        assert_eq!(item.item.number, 2);
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn adjacent_footprints_do_not_overlap() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(2, 2), item(1))
            .unwrap()
            // Touching on the col-2 edge (half-open), so no overlap.
            .place(cell(0, 2), footprint(2, 2), item(2))
            .unwrap();
        assert_eq!(inventory.placed().len(), 2);
    }

    #[test]
    fn remove_hands_the_item_out() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(2, 2), footprint(1, 1), item(7))
            .unwrap();
        let (inventory, removed) = inventory.remove(cell(2, 2)).unwrap();
        assert_eq!(removed.item.number, 7);
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn remove_empty_cell_is_rejected() {
        let inventory = Inventory::empty(8, 8);
        let (inventory, reason) = inventory.remove(cell(0, 0)).unwrap_err();
        assert_eq!(reason, PlacementRejection::NoItemAtCell);
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn move_to_a_free_region_reanchors_the_item() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(2, 2), item(4))
            .unwrap();
        let inventory = inventory.move_to(cell(0, 0), cell(4, 4)).unwrap();
        assert!(inventory.occupant(cell(4, 4)).is_some());
        assert!(inventory.occupant(cell(0, 0)).is_none());
    }

    #[test]
    fn move_over_its_own_old_cells_is_legal() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(3, 3), item(4))
            .unwrap();
        // Shift by one cell — the new footprint overlaps the old one, which is
        // the moving item itself, so it is allowed.
        let inventory = inventory.move_to(cell(0, 0), cell(1, 1)).unwrap();
        assert!(inventory.occupant(cell(1, 1)).is_some());
    }

    #[test]
    fn move_onto_another_item_is_rejected() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(2, 2), item(1))
            .unwrap()
            .place(cell(4, 4), footprint(2, 2), item(2))
            .unwrap();
        let (inventory, reason) = inventory.move_to(cell(0, 0), cell(4, 4)).unwrap_err();
        assert_eq!(reason, PlacementRejection::CellsOccupied);
        assert!(inventory.occupant(cell(0, 0)).is_some());
    }

    #[test]
    fn move_out_of_bounds_is_rejected() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(2, 2), item(1))
            .unwrap();
        let (inventory, reason) = inventory.move_to(cell(0, 0), cell(7, 7)).unwrap_err();
        assert_eq!(reason, PlacementRejection::CellOutOfBounds);
        assert!(inventory.occupant(cell(0, 0)).is_some());
    }

    #[test]
    fn first_fit_over_an_empty_grid_anchors_at_the_origin() {
        let inventory = Inventory::empty(15, 8);
        assert_eq!(inventory.first_fit(footprint(1, 1)), Some(cell(0, 0)));
        assert_eq!(inventory.first_fit(footprint(2, 3)), Some(cell(0, 0)));
        assert_eq!(inventory.first_fit(footprint(8, 15)), Some(cell(0, 0)));
    }

    #[test]
    fn first_fit_scans_row_major() {
        // A full row 0 sends a 1x1 to the start of row 1.
        let inventory = Inventory::empty(15, 8)
            .place(cell(0, 0), footprint(8, 1), item(1))
            .unwrap();
        assert_eq!(inventory.first_fit(footprint(1, 1)), Some(cell(1, 0)));
        // A partial obstruction is skipped to the first fitting anchor on the
        // same row.
        let inventory = Inventory::empty(15, 8)
            .place(cell(0, 0), footprint(3, 2), item(2))
            .unwrap();
        assert_eq!(inventory.first_fit(footprint(2, 2)), Some(cell(0, 3)));
        // A footprint too wide for the remaining row space drops below.
        assert_eq!(inventory.first_fit(footprint(6, 1)), Some(cell(2, 0)));
    }

    #[test]
    fn first_fit_reports_no_anchor_on_a_grid_with_no_region() {
        let inventory = Inventory::empty(2, 2)
            .place(cell(0, 0), footprint(2, 2), item(1))
            .unwrap();
        assert_eq!(inventory.first_fit(footprint(1, 1)), None);
        // An oversized footprint never fits an empty grid either.
        assert_eq!(Inventory::empty(2, 2).first_fit(footprint(3, 1)), None);
    }

    fn stack(number: u16, pieces: u8, cap: u8) -> ItemInstance {
        let mut stack = item(number);
        stack.durability = Durability::new(pieces, cap).unwrap();
        stack
    }

    #[test]
    fn absorb_raises_the_gauge_and_adds_no_placed_item() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(2, 2), footprint(1, 1), stack(5, 1, 3))
            .unwrap();
        let inventory = inventory
            .absorb(cell(2, 2), NonZeroU8::new(2).unwrap())
            .unwrap();
        assert_eq!(inventory.placed().len(), 1);
        let occupant = inventory.occupant(cell(2, 2)).unwrap();
        assert_eq!(occupant.item.durability, Durability::new(3, 3).unwrap());
    }

    #[test]
    fn absorb_that_would_overflow_hands_the_inventory_back_unchanged() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(1, 1), stack(5, 2, 3))
            .unwrap();
        let (inventory, rejection) = inventory
            .absorb(cell(0, 0), NonZeroU8::new(2).unwrap())
            .unwrap_err();
        assert_eq!(rejection, AbsorbRejection::WouldOverflow);
        let occupant = inventory.occupant(cell(0, 0)).unwrap();
        assert_eq!(occupant.item.durability, Durability::new(2, 3).unwrap());
        // A u8-overflowing addition is the same rejection.
        let full = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(1, 1), stack(5, 255, 255))
            .unwrap();
        let (_, rejection) = full
            .absorb(cell(0, 0), NonZeroU8::new(1).unwrap())
            .unwrap_err();
        assert_eq!(rejection, AbsorbRejection::WouldOverflow);
    }

    #[test]
    fn absorb_addresses_any_covered_cell_and_rejects_an_empty_one() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(1, 1), footprint(2, 2), stack(5, 1, 30))
            .unwrap();
        // A covered non-anchor cell reaches the same occupant.
        let inventory = inventory
            .absorb(cell(2, 2), NonZeroU8::new(1).unwrap())
            .unwrap();
        let occupant = inventory.occupant(cell(1, 1)).unwrap();
        assert_eq!(occupant.item.durability, Durability::new(2, 30).unwrap());
        let (_, rejection) = inventory
            .absorb(cell(7, 7), NonZeroU8::new(1).unwrap())
            .unwrap_err();
        assert_eq!(rejection, AbsorbRejection::NoItemAtCell);
    }

    #[test]
    fn consume_one_lowers_a_multi_piece_stack_by_one() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(2, 2), footprint(1, 1), stack(5, 3, 3))
            .unwrap();
        let inventory = inventory.consume_one(cell(2, 2)).unwrap();
        assert_eq!(inventory.placed().len(), 1);
        let occupant = inventory.occupant(cell(2, 2)).unwrap();
        assert_eq!(occupant.item.durability, Durability::new(2, 3).unwrap());
    }

    #[test]
    fn consume_one_last_piece_empties_the_cell() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(1, 1), footprint(1, 1), stack(5, 1, 3))
            .unwrap();
        let inventory = inventory.consume_one(cell(1, 1)).unwrap();
        // The whole item is gone — no zero-count stack lingers at the cell.
        assert!(inventory.occupant(cell(1, 1)).is_none());
        assert!(inventory.placed().is_empty());
    }

    #[test]
    fn consume_one_addresses_any_covered_cell_and_rejects_an_empty_one() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(1, 1), footprint(2, 2), stack(5, 2, 3))
            .unwrap();
        // A covered non-anchor cell reaches the same stack.
        let inventory = inventory.consume_one(cell(2, 2)).unwrap();
        let occupant = inventory.occupant(cell(1, 1)).unwrap();
        assert_eq!(occupant.item.durability, Durability::new(1, 3).unwrap());
        let (inventory, reason) = inventory.consume_one(cell(7, 7)).unwrap_err();
        assert_eq!(reason, PlacementRejection::NoItemAtCell);
        assert_eq!(inventory.placed().len(), 1);
    }

    #[test]
    fn wire_round_trips_and_reproves_overlap() {
        let inventory = Inventory::empty(8, 8)
            .place(cell(0, 0), footprint(2, 2), item(1))
            .unwrap();
        let json = serde_json::to_string(&inventory).unwrap();
        assert_eq!(serde_json::from_str::<Inventory>(&json).unwrap(), inventory);
        // A hostile wire with two overlapping footprints is rejected on parse.
        let overlapping = r#"{"rows":8,"cols":8,"placed":[
            {"anchor":{"row":0,"col":0},"footprint":{"width":2,"height":2},"item":{"item":{"group":0,"number":1},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30}}},
            {"anchor":{"row":1,"col":1},"footprint":{"width":2,"height":2},"item":{"item":{"group":0,"number":2},"level":0,"roll":{"kind":"normal"},"normal_option":null,"luck":"plain","skill":"no_skill","durability":{"current":30,"max":30}}}
        ]}"#;
        assert!(serde_json::from_str::<Inventory>(overlapping).is_err());
    }
}
