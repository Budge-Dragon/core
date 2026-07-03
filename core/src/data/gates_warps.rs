//! Record shape of `gates_warps.json` — the Gate.txt gate roles and the
//! Move.txt warp list, in one kind-tagged file.

use serde::{Deserialize, Serialize};

use crate::components::geometry::{Direction, Rect};
use crate::components::units::{Level, Zen};

use super::common::{GateNumber, MapNumber, Provenance};

/// A record of `gates_warps.json`, kind-tagged.
///
/// The gate kinds are Gate.txt's own flag column: 0 = spawn, 1 = enter,
/// 2 = target. Warps are the Move.txt list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GateWarpRecord {
    /// Gate.txt flag-0 row: a town/respawn spawn point.
    SpawnGate(SpawnGate),
    /// Gate.txt flag-1 row: a trigger area that teleports whoever steps in.
    EnterGate(EnterGate),
    /// Gate.txt flag-2 row: a landing area reachable only as a travel target.
    TargetGate(TargetGate),
    /// Move.txt row: an entry of the warp command list.
    Warp(Warp),
}

/// A town/respawn spawn point (Gate.txt flag 0).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnGate {
    /// Gate number — the key enter gates and warps target.
    pub number: GateNumber,
    /// Map the gate is on.
    pub map: MapNumber,
    /// Landing rectangle; arrival is a random tile inside it.
    pub area: Rect,
    /// Facing on arrival; absent = unspecified (Gate.txt direction byte 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// A landing area reachable only as a travel target (Gate.txt flag 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetGate {
    /// Gate number — the key enter gates and warps target.
    pub number: GateNumber,
    /// Map the gate is on.
    pub map: MapNumber,
    /// Landing rectangle; arrival is a random tile inside it.
    pub area: Rect,
    /// Facing on arrival; absent = unspecified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<Direction>,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// A trigger area that teleports whoever steps in (Gate.txt flag 1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnterGate {
    /// Gate number.
    pub number: GateNumber,
    /// Map the trigger area is on.
    pub map: MapNumber,
    /// Trigger rectangle.
    pub area: Rect,
    /// Gate travelers arrive at (a spawn or target gate, never an enter gate;
    /// proven at `Atlas::parse`).
    pub target_gate: GateNumber,
    /// Minimum character level to pass; absent = unrestricted (Gate.txt's
    /// level-0 sentinel, parsed to absence at extraction).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_level: Option<Level>,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// An entry of the warp command list (Move.txt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Warp {
    /// Position in the warp list.
    pub index: WarpIndex,
    /// Zen fee charged per warp.
    pub cost_zen: Zen,
    /// Minimum character level to warp (before the class warp reduction).
    pub min_level: Level,
    /// Gate travelers arrive at (a spawn or target gate; proven at parse).
    pub target_gate: GateNumber,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// Position of a warp entry in the warp list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WarpIndex(
    /// The warp-list position from Move.txt.
    pub u16,
);
