//! Record shapes of `gates_warps.json` — exit gates, enter gates, and the
//! warp list, mixed in one kind-tagged file.

use serde::{Deserialize, Serialize};

use super::common::{Direction, GateNumber, MapRef, Rect, SourceVersion};

/// A record of `gates_warps.json`, kind-tagged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum GateWarpRecord {
    /// A landing area travelers arrive in.
    ExitGate(ExitGate),
    /// A trigger area that teleports whoever steps in.
    EnterGate(EnterGate),
    /// An entry of the warp command list.
    Warp(Warp),
}

/// A landing area travelers arrive in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitGate {
    /// Gate number, the key enter gates and warps target.
    pub number: GateNumber,
    /// Map the gate is on.
    pub map: MapRef,
    /// Landing rectangle.
    pub area: Rect,
    /// Facing direction on arrival; absent = unspecified.
    pub direction: Option<Direction>,
    /// Whether dead/new characters spawn here.
    pub is_spawn_gate: bool,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// A trigger area that teleports whoever steps in.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnterGate {
    /// Gate number.
    pub number: GateNumber,
    /// Map the gate is on.
    pub map: MapRef,
    /// Trigger rectangle.
    pub area: Rect,
    /// Exit gate travelers arrive at.
    pub target_gate: GateNumber,
    /// Minimum character level to pass.
    pub min_level: u16,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}

/// An entry of the warp command list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Warp {
    /// Position in the warp list.
    pub index: u16,
    /// Display name.
    pub name: String,
    /// Money charged per warp.
    pub cost_zen: u32,
    /// Minimum character level to warp.
    pub min_level: u16,
    /// Exit gate travelers arrive at.
    pub target_gate: GateNumber,
    /// Dataset era the record was extracted from.
    pub source_version: SourceVersion,
    /// Era-doubt note for curated backports; absent = uncontested.
    pub review: Option<String>,
}
