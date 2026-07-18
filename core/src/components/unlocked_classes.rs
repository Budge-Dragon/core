//! The set of character classes an account has earned the right to create. Data
//! only: a grow-only membership over the closed [`CharacterClass`] roster with
//! monotone insertion — no rules. Once a class is earned it is kept forever;
//! there is no removal, clear, or un-earn operation, so revocation is
//! unrepresentable and the permanent-unlock guarantee is type-level.

use serde::{Deserialize, Serialize};

use crate::components::class::{CharacterClass, ClassSet};

/// The classes an account may create. Wraps the total [`ClassSet`] behind a
/// grow-only surface (`empty` / `contains` / `unlocked`), and serialises
/// transparently as [`ClassSet`]'s flat array of `snake_case` class names —
/// reconstituting through the very same `TryFrom` parse gate as live
/// construction, so a persisted duplicate is rejected at the boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnlockedClasses(ClassSet);

impl UnlockedClasses {
    /// A fresh account's earned-set: no class earned yet — a real domain value
    /// (the brand-new account), not a fabricated default.
    #[must_use]
    pub const fn empty() -> Self {
        Self(ClassSet::NONE)
    }

    /// Whether `class` has been earned.
    #[must_use]
    pub fn contains(&self, class: CharacterClass) -> bool {
        self.0.allows(class)
    }

    /// This earned-set with `class` earned — monotone and idempotent: earning a
    /// class already held returns an equal set, and no class is ever un-earned.
    #[must_use]
    pub fn unlocked(self, class: CharacterClass) -> Self {
        Self(self.0.with(class))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_account_has_earned_nothing() {
        let set = UnlockedClasses::empty();
        assert!(!set.contains(CharacterClass::MagicGladiator));
        assert!(!set.contains(CharacterClass::DarkLord));
        assert!(!set.contains(CharacterClass::DarkKnight));
    }

    #[test]
    fn earning_is_monotone_and_idempotent() {
        let set = UnlockedClasses::empty().unlocked(CharacterClass::MagicGladiator);
        assert!(set.contains(CharacterClass::MagicGladiator));
        // Earning the same class again returns an equal set — no churn.
        assert_eq!(set.clone().unlocked(CharacterClass::MagicGladiator), set);
        assert!(!set.contains(CharacterClass::DarkLord));
    }

    #[test]
    fn earning_a_second_class_keeps_the_first() {
        let set = UnlockedClasses::empty()
            .unlocked(CharacterClass::MagicGladiator)
            .unlocked(CharacterClass::DarkLord);
        assert!(set.contains(CharacterClass::MagicGladiator));
        assert!(set.contains(CharacterClass::DarkLord));
    }

    #[test]
    fn wire_is_the_transparent_snake_case_name_array() {
        let set = UnlockedClasses::empty()
            .unlocked(CharacterClass::MagicGladiator)
            .unlocked(CharacterClass::DarkLord);
        let json = serde_json::to_string(&set).unwrap();
        // Declaration order: Magic Gladiator precedes Dark Lord on the roster.
        assert_eq!(json, r#"["magic_gladiator","dark_lord"]"#);
        assert_eq!(serde_json::from_str::<UnlockedClasses>(&json).unwrap(), set);
    }

    #[test]
    fn a_duplicated_wire_entry_is_a_parse_failure() {
        // Reconstitution runs through ClassSet's TryFrom gate, which rejects a
        // class named more than once — the same gate as live construction.
        assert!(
            serde_json::from_str::<UnlockedClasses>(r#"["magic_gladiator","magic_gladiator"]"#)
                .is_err()
        );
    }
}
