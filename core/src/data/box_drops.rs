//! Record shape of `box_drops.json` — openable-box contents keyed by the box
//! item and its own plus level.

use serde::{Deserialize, Serialize};

use crate::components::collections::OneOrMore;
use crate::components::interval::Interval;
use crate::components::units::{ChancePer10000, ItemLevel, Zen};

use super::common::{ItemRef, Provenance};

/// Contents of one openable box at one box plus level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoxDrop {
    /// The box item this record opens (e.g. Box of Luck 14/11).
    pub box_item: ItemRef,
    /// The box's own plus level this record applies to.
    pub box_level: ItemLevel,
    /// Chance the box yields an item; on failure it yields `money_fallback`.
    pub item_roll_per_10000: ChancePer10000,
    /// Uniform pick among these on an item yield.
    pub items: OneOrMore<ItemRef>,
    /// Plus level of the yielded item, uniform inclusive.
    pub item_level_range: ItemLevelRange,
    /// Zen yielded when the item roll fails.
    pub money_fallback: Zen,
    /// Extraction provenance: dataset era plus optional curation note.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// Inclusive plus-level range over [`ItemLevel`]; `min <= max` proven at parse.
pub type ItemLevelRange = Interval<ItemLevel>;
