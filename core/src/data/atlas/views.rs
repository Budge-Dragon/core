//! The resolved view types the [`Atlas`](super::Atlas) hands out: landings,
//! enter-gate and warp views, spawn entries, and the per-map handle — each a
//! borrow over resolved state whose referents were proven present at parse. The
//! backing `Resolved*` records are retained by the Atlas and read back through
//! these views; only the Atlas mints them, so no view has a public fabricating
//! constructor.

use crate::components::spatial::{Facing, WorldRect};
use crate::components::tile::WalkGrid;
use crate::data::common::MapNumber;
use crate::data::gates_warps::{EnterGate, Warp};
use crate::data::map_definitions::MapDefinition;
use crate::data::monster_definitions::MonsterDefinition;
use crate::data::spawns::Spawn;

/// The landing side of a resolved gate reference, in world space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Landing {
    /// Map the traveler lands on.
    pub map: MapNumber,
    /// Landing rectangle in world space.
    pub area: WorldRect,
    /// Facing on arrival; absent = unspecified (never fabricated).
    pub facing: Option<Facing>,
}

/// An enter gate with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnterGateView<'a> {
    /// The trigger-side record.
    pub gate: &'a EnterGate,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// An enter gate with its trigger area projected to world space and its
/// landing resolved at parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedEnterGate {
    pub(super) gate: EnterGate,
    pub(super) trigger: WorldRect,
    pub(super) landing: Landing,
}

/// A warp entry with its landing resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarpView<'a> {
    /// The warp-list record.
    pub warp: &'a Warp,
    /// Where its target gate lands travelers.
    pub landing: Landing,
}

/// A spawn record joined to the monster definition it names, retained per map
/// at parse. The join is proven total here (the monster is looked up once,
/// during resolution), so a consuming service reaches the definition without an
/// `Option` — mirroring the `warps: Vec<(Warp, Landing)>` owned-join precedent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResolvedSpawn {
    pub(super) spawn: Spawn,
    pub(super) monster: MonsterDefinition,
}

/// A spawn record borrowed with the monster definition it resolves to — the
/// public view over the atlas's owned spawn-to-monster join, mirroring
/// [`WarpView`]/[`EnterGateView`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnEntry<'a> {
    /// The spawn record.
    pub spawn: &'a Spawn,
    /// The monster definition it names — resolution proven at parse.
    pub monster: &'a MonsterDefinition,
}

/// A proven-present view of one map: its definition, its walk grid, and its
/// spawns joined to their monster definitions. Minted only by the [`Atlas`](super::Atlas)
/// from resolved state — there is no public fabricating constructor — so its
/// walk grid and spawns are total, never `Option`.
#[derive(Debug, Clone, Copy)]
pub struct MapHandle<'a> {
    pub(super) definition: &'a MapDefinition,
    pub(super) walk_grid: &'a WalkGrid,
    pub(super) spawns: &'a [ResolvedSpawn],
}

impl<'a> MapHandle<'a> {
    /// The map's definition — its environment and, on Arena, its soccer pitch.
    #[must_use]
    pub fn definition(&self) -> &'a MapDefinition {
        self.definition
    }

    /// The map's walk grid. Total — presence was proven at parse.
    #[must_use]
    pub fn walk_grid(&self) -> &'a WalkGrid {
        self.walk_grid
    }

    /// The map's spawn entries, each already joined to its monster definition.
    pub fn spawns(&self) -> impl Iterator<Item = SpawnEntry<'a>> {
        self.spawns.iter().map(|resolved| SpawnEntry {
            spawn: &resolved.spawn,
            monster: &resolved.monster,
        })
    }
}
