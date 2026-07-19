//! The positional identity of a character within one account's roster. A
//! [`CharacterSlot`] names a seat in a single account (never a host account id),
//! mirroring the party [`crate::components::party::MemberSlot`] grain. Data and
//! the capacity NUMBER only — the count-and-refuse enforcement is a host duty
//! (core holds no roster), exactly as [`MemberSlot`] leaves the party-size
//! enforcement to [`crate::services::party`].
//!
//! [`MemberSlot`]: crate::components::party::MemberSlot

use serde::{Deserialize, Serialize};

/// A character's positional identity inside one account's roster — never a host
/// account id. Bounded `0..CAP`; the host owns the account↔slot map (the party
/// [`crate::components::party::MemberSlot`] grain). Lowest-free-slot reuse keeps
/// it a bounded identity across deletion and re-creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CharacterSlot(
    /// The seat index, `0..CAP`.
    pub u8,
);

impl CharacterSlot {
    /// The per-account character cap (classic 5). Valid slots are `0..CAP`.
    /// Core owns this NUMBER; the host owns the "count `< CAP` else refuse"
    /// enforcement, since core models no roster.
    pub const CAP: u8 = 5;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_slot_is_a_bare_integer_positional_identity() {
        assert_eq!(serde_json::to_string(&CharacterSlot(3)).unwrap(), "3");
        assert_eq!(
            serde_json::from_str::<CharacterSlot>("3").unwrap(),
            CharacterSlot(3)
        );
        // Two slots compare by position, never by a host id.
        assert!(CharacterSlot(0) < CharacterSlot(1));
    }

    #[test]
    fn the_per_account_cap_is_five() {
        assert_eq!(CharacterSlot::CAP, 5);
    }
}
