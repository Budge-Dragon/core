//! `classes.json` shapes: the per-class record and the total class table.

use core::num::{NonZeroU16, NonZeroU32};

use serde::{Deserialize, Serialize};

use crate::components::class::{CharacterClass, ClassNumber};
use crate::components::units::Level;

use super::common::{MapNumber, Provenance};

/// The command-class energy floor: Dark Lord's creation energy, below which
/// its `with_command` starting stats cannot fall.
const COMMAND_ENERGY_FLOOR: u16 = 15;

/// One class record: every extracted per-class fact, invariants proven by
/// smart constructors at deserialization. The raw-record `try_from` proves the
/// cross-field pairing: `with_command` starting stats appear on the
/// `dark_lord` record and nowhere else, and that record's energy meets the
/// command-class floor of 15.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawClassRecord")]
pub struct ClassRecord {
    /// Roster identity — the record key.
    pub class: CharacterClass,
    /// Client class code (authentic protocol values 0/2/4/6/8/10/12/16).
    pub number: ClassNumber,
    /// Creation-window availability.
    pub creation: CreationGate,
    /// Second-tier class change, where the line has one.
    pub evolution: Evolution,
    /// Map a new character starts on (Lorencia 0; elves Noria 3). Atlas-proven.
    pub home_map: MapNumber,
    /// Stat points granted per level-up (5; MG/DL 7).
    pub points_per_level: u8,
    /// Creation base stats.
    pub starting_stats: StartingStats,
    /// Creation vitals.
    pub starting_vitals: StartingVitals,
    /// Divisor of the fruit budget curve; nonzero by parse, so division by it
    /// is total.
    pub fruit_points_divisor: NonZeroU32,
    /// Gate-requirement scaling for warps.
    pub warp_requirement: WarpRequirement,
    /// Extraction provenance.
    #[serde(flatten)]
    pub provenance: Provenance,
}

/// Wire mirror of [`ClassRecord`], validated on the way in.
#[derive(Debug, Clone, Deserialize)]
struct RawClassRecord {
    class: CharacterClass,
    number: ClassNumber,
    creation: CreationGate,
    evolution: Evolution,
    home_map: MapNumber,
    points_per_level: u8,
    starting_stats: StartingStats,
    starting_vitals: StartingVitals,
    fruit_points_divisor: NonZeroU32,
    warp_requirement: WarpRequirement,
    #[serde(flatten)]
    provenance: Provenance,
}

impl TryFrom<RawClassRecord> for ClassRecord {
    type Error = ClassRecordError;

    fn try_from(raw: RawClassRecord) -> Result<Self, Self::Error> {
        match (raw.class, raw.starting_stats) {
            (CharacterClass::DarkLord, StartingStats::WithCommand { energy, .. }) => {
                if energy < COMMAND_ENERGY_FLOOR {
                    return Err(ClassRecordError::EnergyBelowCommandFloor { energy });
                }
            }
            (CharacterClass::DarkLord, StartingStats::Standard { .. }) => {
                return Err(ClassRecordError::StandardStatsOnDarkLord);
            }
            (other, StartingStats::WithCommand { .. }) => {
                return Err(ClassRecordError::CommandStatsOutsideDarkLord(other));
            }
            (
                CharacterClass::DarkWizard
                | CharacterClass::SoulMaster
                | CharacterClass::DarkKnight
                | CharacterClass::BladeKnight
                | CharacterClass::FairyElf
                | CharacterClass::MuseElf
                | CharacterClass::MagicGladiator,
                StartingStats::Standard { .. },
            ) => {}
        }
        Ok(Self {
            class: raw.class,
            number: raw.number,
            creation: raw.creation,
            evolution: raw.evolution,
            home_map: raw.home_map,
            points_per_level: raw.points_per_level,
            starting_stats: raw.starting_stats,
            starting_vitals: raw.starting_vitals,
            fruit_points_divisor: raw.fruit_points_divisor,
            warp_requirement: raw.warp_requirement,
            provenance: raw.provenance,
        })
    }
}

/// How a class becomes available in the character-creation window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CreationGate {
    /// Creatable from the start.
    Always,
    /// Creatable once another character on the same account reaches this level.
    UnlockedAt {
        /// Required level of the highest character on the account.
        level: Level,
    },
    /// Never creatable — second tiers exist only via class change.
    EvolutionOnly,
}

/// Whether and how a class evolves into its line's second tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Evolution {
    /// Class change into the second tier.
    Evolves {
        /// The second-tier class.
        into: CharacterClass,
        /// Character level at which the class change unlocks.
        at_level: Level,
    },
    /// The line's final pre-S3 tier.
    Terminal,
}

/// Creation-time base stats. Dark Lord is the only class with a fifth stat,
/// so the fifth stat is a variant, never an `Option`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StartingStats {
    /// The four classic trainable stats.
    Standard {
        /// Starting strength.
        strength: u16,
        /// Starting agility.
        agility: u16,
        /// Starting vitality.
        vitality: u16,
        /// Starting energy.
        energy: u16,
    },
    /// The four classic stats plus Command (Dark Lord line).
    WithCommand {
        /// Starting strength.
        strength: u16,
        /// Starting agility.
        agility: u16,
        /// Starting vitality.
        vitality: u16,
        /// Starting energy.
        energy: u16,
        /// Starting command.
        command: u16,
    },
}

/// Parse failure on one record: the starting-stats shape contradicts the
/// `class` discriminator, or the command-class energy floor is broken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassRecordError {
    /// `with_command` starting stats on a class other than `dark_lord`.
    CommandStatsOutsideDarkLord(CharacterClass),
    /// `standard` starting stats on the `dark_lord` record.
    StandardStatsOnDarkLord,
    /// `dark_lord` energy below the command-class floor of 15.
    EnergyBelowCommandFloor {
        /// The rejected energy value.
        energy: u16,
    },
}

impl core::fmt::Display for ClassRecordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CommandStatsOutsideDarkLord(class) => {
                write!(f, "command starting stats on non-dark-lord class {class:?}")
            }
            Self::StandardStatsOnDarkLord => {
                write!(f, "standard starting stats on the dark_lord record")
            }
            Self::EnergyBelowCommandFloor { energy } => {
                write!(f, "dark lord energy {energy} below the command floor 15")
            }
        }
    }
}

impl core::error::Error for ClassRecordError {}

/// Creation-time current vitals. The uniform `ability: 1` is review-flagged on
/// every record (OpenMU initializer seed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartingVitals {
    /// Starting current HP.
    pub health: u16,
    /// Starting current mana.
    pub mana: u16,
    /// Starting current AG.
    pub ability: u16,
}

/// Gate-requirement scaling for warp eligibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WarpRequirement {
    /// The full gate requirement applies.
    Full,
    /// The class pays this fraction of the requirement (MG/DL: 2/3).
    /// Numerator and denominator nonzero by parse.
    Fraction {
        /// Numerator of the fraction.
        numerator: NonZeroU16,
        /// Denominator of the fraction.
        denominator: NonZeroU16,
    },
}

/// Total class lookup: all eight classes present exactly once and all class
/// codes distinct, proven by `TryFrom<Vec<ClassRecord>>`. Eight named fields —
/// a new roster variant breaks construction and every accessor until handled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<ClassRecord>", into = "Vec<ClassRecord>")]
pub struct ClassTable {
    dark_wizard: ClassRecord,
    soul_master: ClassRecord,
    dark_knight: ClassRecord,
    blade_knight: ClassRecord,
    fairy_elf: ClassRecord,
    muse_elf: ClassRecord,
    magic_gladiator: ClassRecord,
    dark_lord: ClassRecord,
}

impl ClassTable {
    /// Total accessor — exhaustive match on the roster; never `Option`.
    #[must_use]
    pub fn record(&self, class: CharacterClass) -> &ClassRecord {
        match class {
            CharacterClass::DarkWizard => &self.dark_wizard,
            CharacterClass::SoulMaster => &self.soul_master,
            CharacterClass::DarkKnight => &self.dark_knight,
            CharacterClass::BladeKnight => &self.blade_knight,
            CharacterClass::FairyElf => &self.fairy_elf,
            CharacterClass::MuseElf => &self.muse_elf,
            CharacterClass::MagicGladiator => &self.magic_gladiator,
            CharacterClass::DarkLord => &self.dark_lord,
        }
    }

    /// Wire class code to class. `None`: the code names no roster class —
    /// genuine optionality of an open byte.
    #[must_use]
    pub fn class_by_number(&self, number: ClassNumber) -> Option<CharacterClass> {
        Self::ROSTER
            .into_iter()
            .find(|&class| self.record(class).number == number)
    }

    const ROSTER: [CharacterClass; 8] = [
        CharacterClass::DarkWizard,
        CharacterClass::SoulMaster,
        CharacterClass::DarkKnight,
        CharacterClass::BladeKnight,
        CharacterClass::FairyElf,
        CharacterClass::MuseElf,
        CharacterClass::MagicGladiator,
        CharacterClass::DarkLord,
    ];
}

/// Load failure assembling the total table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassTableError {
    /// A roster class has no record.
    MissingClass(CharacterClass),
    /// A roster class has more than one record.
    DuplicateClass(CharacterClass),
    /// Two records claim the same client class code.
    DuplicateNumber(ClassNumber),
}

impl core::fmt::Display for ClassTableError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MissingClass(class) => write!(f, "no record for class {class:?}"),
            Self::DuplicateClass(class) => write!(f, "duplicate record for class {class:?}"),
            Self::DuplicateNumber(number) => write!(f, "duplicate class number {number:?}"),
        }
    }
}

impl core::error::Error for ClassTableError {}

/// One roster slot while assembling the table: the record, or its absence.
enum Slot {
    Empty,
    Filled(ClassRecord),
}

impl Slot {
    fn place(&mut self, record: ClassRecord) -> Result<(), ClassTableError> {
        match self {
            Self::Empty => {
                *self = Self::Filled(record);
                Ok(())
            }
            Self::Filled(existing) => Err(ClassTableError::DuplicateClass(existing.class)),
        }
    }

    fn take(self, class: CharacterClass) -> Result<ClassRecord, ClassTableError> {
        match self {
            Self::Empty => Err(ClassTableError::MissingClass(class)),
            Self::Filled(record) => Ok(record),
        }
    }
}

impl TryFrom<Vec<ClassRecord>> for ClassTable {
    type Error = ClassTableError;

    fn try_from(records: Vec<ClassRecord>) -> Result<Self, Self::Error> {
        let mut dark_wizard = Slot::Empty;
        let mut soul_master = Slot::Empty;
        let mut dark_knight = Slot::Empty;
        let mut blade_knight = Slot::Empty;
        let mut fairy_elf = Slot::Empty;
        let mut muse_elf = Slot::Empty;
        let mut magic_gladiator = Slot::Empty;
        let mut dark_lord = Slot::Empty;
        let mut numbers: Vec<ClassNumber> = Vec::new();

        for record in records {
            if numbers.contains(&record.number) {
                return Err(ClassTableError::DuplicateNumber(record.number));
            }
            numbers.push(record.number);
            let slot = match record.class {
                CharacterClass::DarkWizard => &mut dark_wizard,
                CharacterClass::SoulMaster => &mut soul_master,
                CharacterClass::DarkKnight => &mut dark_knight,
                CharacterClass::BladeKnight => &mut blade_knight,
                CharacterClass::FairyElf => &mut fairy_elf,
                CharacterClass::MuseElf => &mut muse_elf,
                CharacterClass::MagicGladiator => &mut magic_gladiator,
                CharacterClass::DarkLord => &mut dark_lord,
            };
            slot.place(record)?;
        }

        Ok(Self {
            dark_wizard: dark_wizard.take(CharacterClass::DarkWizard)?,
            soul_master: soul_master.take(CharacterClass::SoulMaster)?,
            dark_knight: dark_knight.take(CharacterClass::DarkKnight)?,
            blade_knight: blade_knight.take(CharacterClass::BladeKnight)?,
            fairy_elf: fairy_elf.take(CharacterClass::FairyElf)?,
            muse_elf: muse_elf.take(CharacterClass::MuseElf)?,
            magic_gladiator: magic_gladiator.take(CharacterClass::MagicGladiator)?,
            dark_lord: dark_lord.take(CharacterClass::DarkLord)?,
        })
    }
}

impl From<ClassTable> for Vec<ClassRecord> {
    fn from(table: ClassTable) -> Self {
        vec![
            table.dark_wizard,
            table.soul_master,
            table.dark_knight,
            table.blade_knight,
            table.fairy_elf,
            table.muse_elf,
            table.magic_gladiator,
            table.dark_lord,
        ]
    }
}
