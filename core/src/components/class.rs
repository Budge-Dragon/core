//! The playable class roster: the closed `CharacterClass` enum, the total
//! `ClassSet` membership structure, and the client class code.

use serde::{Deserialize, Serialize};

/// A playable character class: the tiers of the five pre-S3 class lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CharacterClass {
    /// Base wizard class.
    DarkWizard,
    /// Second tier of the Dark Wizard line.
    SoulMaster,
    /// Base knight class.
    DarkKnight,
    /// Second tier of the Dark Knight line.
    BladeKnight,
    /// Base elf class.
    FairyElf,
    /// Second tier of the Fairy Elf line.
    MuseElf,
    /// Hybrid class, unlocked by account progress; no second tier pre-S3.
    MagicGladiator,
    /// Command class, unlocked by account progress.
    DarkLord,
}

impl CharacterClass {
    /// The roster in declaration order — a fixed-length array, so a new
    /// variant breaks its length and every match keyed by the roster.
    pub(crate) const ALL: [Self; 8] = [
        Self::DarkWizard,
        Self::SoulMaster,
        Self::DarkKnight,
        Self::BladeKnight,
        Self::FairyElf,
        Self::MuseElf,
        Self::MagicGladiator,
        Self::DarkLord,
    ];

    /// Whether the class trains the command stat — true for the Dark Lord line
    /// and no other. The single predicate every command-class pairing keys off,
    /// so the data record and the live character can never disagree on it.
    #[must_use]
    pub fn has_command(self) -> bool {
        match self {
            Self::DarkLord => true,
            Self::DarkWizard
            | Self::SoulMaster
            | Self::DarkKnight
            | Self::BladeKnight
            | Self::FairyElf
            | Self::MuseElf
            | Self::MagicGladiator => false,
        }
    }
}

/// Client-facing class code carried by the character-list packet
/// (`base*4 + tier*2` in the client encoding: 0/2/4/6/8/10/12/16). An open
/// wire byte: any `u8` is representable, and holding one proves nothing about
/// the roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ClassNumber(
    /// The class code as the client knows it.
    pub u8,
);

/// Class-membership set over the closed roster — a total structure:
/// `allows` is an exhaustive match, every class queried has an answer, and a
/// new roster variant breaks every consumer until answered. Wire form: array
/// of `snake_case` class names; a duplicated entry is a parse error; the empty
/// array is the legal all-false set (no class admitted).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<CharacterClass>", into = "Vec<CharacterClass>")]
pub struct ClassSet {
    /// Whether the Dark Wizard class is admitted.
    pub dark_wizard: bool,
    /// Whether the Soul Master class is admitted.
    pub soul_master: bool,
    /// Whether the Dark Knight class is admitted.
    pub dark_knight: bool,
    /// Whether the Blade Knight class is admitted.
    pub blade_knight: bool,
    /// Whether the Fairy Elf class is admitted.
    pub fairy_elf: bool,
    /// Whether the Muse Elf class is admitted.
    pub muse_elf: bool,
    /// Whether the Magic Gladiator class is admitted.
    pub magic_gladiator: bool,
    /// Whether the Dark Lord class is admitted.
    pub dark_lord: bool,
}

impl ClassSet {
    /// The empty set — no class admitted. A real domain value (skills'
    /// monster-only fact), not a fabricated default.
    pub const NONE: Self = Self {
        dark_wizard: false,
        soul_master: false,
        dark_knight: false,
        blade_knight: false,
        fairy_elf: false,
        muse_elf: false,
        magic_gladiator: false,
        dark_lord: false,
    };

    /// Total membership query — exhaustive match on the roster.
    #[must_use]
    pub fn allows(self, class: CharacterClass) -> bool {
        match class {
            CharacterClass::DarkWizard => self.dark_wizard,
            CharacterClass::SoulMaster => self.soul_master,
            CharacterClass::DarkKnight => self.dark_knight,
            CharacterClass::BladeKnight => self.blade_knight,
            CharacterClass::FairyElf => self.fairy_elf,
            CharacterClass::MuseElf => self.muse_elf,
            CharacterClass::MagicGladiator => self.magic_gladiator,
            CharacterClass::DarkLord => self.dark_lord,
        }
    }

    /// This set with `class` admitted — monotone and idempotent: admitting a
    /// member already present returns an equal set, and no class is ever
    /// removed.
    #[must_use]
    pub(crate) fn with(mut self, class: CharacterClass) -> Self {
        *self.slot_mut(class) = true;
        self
    }

    fn slot_mut(&mut self, class: CharacterClass) -> &mut bool {
        match class {
            CharacterClass::DarkWizard => &mut self.dark_wizard,
            CharacterClass::SoulMaster => &mut self.soul_master,
            CharacterClass::DarkKnight => &mut self.dark_knight,
            CharacterClass::BladeKnight => &mut self.blade_knight,
            CharacterClass::FairyElf => &mut self.fairy_elf,
            CharacterClass::MuseElf => &mut self.muse_elf,
            CharacterClass::MagicGladiator => &mut self.magic_gladiator,
            CharacterClass::DarkLord => &mut self.dark_lord,
        }
    }
}

impl TryFrom<Vec<CharacterClass>> for ClassSet {
    type Error = DuplicateClassEntry;

    fn try_from(classes: Vec<CharacterClass>) -> Result<Self, Self::Error> {
        let mut set = Self::NONE;
        for class in classes {
            let slot = set.slot_mut(class);
            if *slot {
                return Err(DuplicateClassEntry(class));
            }
            *slot = true;
        }
        Ok(set)
    }
}

impl From<ClassSet> for Vec<CharacterClass> {
    fn from(set: ClassSet) -> Self {
        CharacterClass::ALL
            .into_iter()
            .filter(|&class| set.allows(class))
            .collect()
    }
}

/// Parse failure: a class listed more than once in a class-set array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DuplicateClassEntry(
    /// The class that appeared more than once.
    pub CharacterClass,
);

impl core::fmt::Display for DuplicateClassEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "class listed more than once in a class set: {:?}",
            self.0
        )
    }
}

impl core::error::Error for DuplicateClassEntry {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_reflects_membership() {
        let set =
            ClassSet::try_from(vec![CharacterClass::DarkWizard, CharacterClass::MuseElf]).unwrap();
        assert!(set.allows(CharacterClass::DarkWizard));
        assert!(set.allows(CharacterClass::MuseElf));
        assert!(!set.allows(CharacterClass::DarkKnight));
        assert!(!set.allows(CharacterClass::DarkLord));
    }

    #[test]
    fn allows_is_total_over_the_roster() {
        let set = ClassSet::NONE;
        for class in CharacterClass::ALL {
            assert!(!set.allows(class));
        }
    }

    #[test]
    fn empty_array_is_the_all_false_set() {
        let set = ClassSet::try_from(Vec::new()).unwrap();
        assert_eq!(set, ClassSet::NONE);
    }

    #[test]
    fn duplicate_entry_is_rejected() {
        let err = ClassSet::try_from(vec![CharacterClass::FairyElf, CharacterClass::FairyElf])
            .unwrap_err();
        assert_eq!(err, DuplicateClassEntry(CharacterClass::FairyElf));
    }

    #[test]
    fn with_admits_a_member_and_is_monotone_and_idempotent() {
        let set = ClassSet::NONE.with(CharacterClass::MagicGladiator);
        assert!(set.allows(CharacterClass::MagicGladiator));
        assert!(!set.allows(CharacterClass::DarkLord));
        // Re-admitting a member returns an equal set — no churn, no removal.
        assert_eq!(set.with(CharacterClass::MagicGladiator), set);
        // Admitting a second keeps the first (monotone).
        let grown = set.with(CharacterClass::DarkLord);
        assert!(grown.allows(CharacterClass::MagicGladiator));
        assert!(grown.allows(CharacterClass::DarkLord));
    }

    #[test]
    fn class_set_round_trips_in_declaration_order() {
        let set = ClassSet::try_from(vec![
            CharacterClass::MagicGladiator,
            CharacterClass::DarkWizard,
        ])
        .unwrap();
        let back: Vec<CharacterClass> = set.into();
        assert_eq!(
            back,
            vec![CharacterClass::DarkWizard, CharacterClass::MagicGladiator]
        );
    }

    #[test]
    fn class_set_serializes_as_name_array() {
        let set = ClassSet::try_from(vec![
            CharacterClass::DarkWizard,
            CharacterClass::SoulMaster,
            CharacterClass::MagicGladiator,
        ])
        .unwrap();
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["dark_wizard","soul_master","magic_gladiator"]"#);
        assert_eq!(serde_json::from_str::<ClassSet>(&json).unwrap(), set);
    }

    #[test]
    fn class_set_serde_rejects_duplicates() {
        assert!(serde_json::from_str::<ClassSet>(r#"["muse_elf","muse_elf"]"#).is_err());
    }

    #[test]
    fn character_class_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&CharacterClass::DarkKnight).unwrap(),
            r#""dark_knight""#
        );
    }

    #[test]
    fn only_dark_lord_has_command() {
        for class in CharacterClass::ALL {
            let expected = class == CharacterClass::DarkLord;
            assert_eq!(class.has_command(), expected, "{class:?}");
        }
    }
}
