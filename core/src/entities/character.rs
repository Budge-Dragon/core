//! A live player character: the mutable world-presence aggregate every player
//! system acts on. Its fields are private because construction proves one
//! cross-field invariant — a character trains the command stat if and only if
//! its class is a command class. That proof is the single gate shared by serde
//! deserialization and programmatic construction, via [`TryFrom<RawCharacter>`],
//! mirroring the data-side [`crate::data::classes::ClassRecord`] precedent so
//! the two can never disagree on which classes carry command.

use serde::{Deserialize, Serialize};

use crate::components::active_effect::ActiveEffects;
use crate::components::class::CharacterClass;
use crate::components::placement::Placement;
use crate::components::stats::Stats;
use crate::components::units::{Exp, Level, Zen};
use crate::components::vitals::Vitals;

/// A live player character. Private fields: construction (serde or otherwise)
/// proves the class-to-stats pairing, so a held `Character` is always valid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawCharacter", into = "RawCharacter")]
pub struct Character {
    class: CharacterClass,
    level: Level,
    experience: Exp,
    stats: Stats,
    unspent_points: u16,
    zen: Zen,
    placement: Placement,
    vitals: Vitals,
    active_effects: ActiveEffects,
}

/// Wire mirror of [`Character`]. The invariant gate re-proves on the way in,
/// since a persisted character loaded from a host is untrusted.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct RawCharacter {
    class: CharacterClass,
    level: Level,
    experience: Exp,
    stats: Stats,
    unspent_points: u16,
    zen: Zen,
    placement: Placement,
    vitals: Vitals,
    /// A record that predates timed effects, or a freshly created character,
    /// carries none — the real "no active effects" value, not a fabricated
    /// default.
    #[serde(default = "ActiveEffects::empty")]
    active_effects: ActiveEffects,
}

impl TryFrom<RawCharacter> for Character {
    type Error = CharacterError;

    fn try_from(raw: RawCharacter) -> Result<Self, Self::Error> {
        match (raw.class.has_command(), raw.stats) {
            (true, Stats::WithCommand { .. }) | (false, Stats::Standard { .. }) => Ok(Self {
                class: raw.class,
                level: raw.level,
                experience: raw.experience,
                stats: raw.stats,
                unspent_points: raw.unspent_points,
                zen: raw.zen,
                placement: raw.placement,
                vitals: raw.vitals,
                active_effects: raw.active_effects,
            }),
            (true, Stats::Standard { .. }) => {
                Err(CharacterError::StandardStatsOnCommandClass(raw.class))
            }
            (false, Stats::WithCommand { .. }) => {
                Err(CharacterError::CommandStatsOutsideCommandClass(raw.class))
            }
        }
    }
}

impl From<Character> for RawCharacter {
    fn from(character: Character) -> Self {
        Self {
            class: character.class,
            level: character.level,
            experience: character.experience,
            stats: character.stats,
            unspent_points: character.unspent_points,
            zen: character.zen,
            placement: character.placement,
            vitals: character.vitals,
            active_effects: character.active_effects,
        }
    }
}

impl Character {
    /// The character's class.
    #[must_use]
    pub fn class(&self) -> CharacterClass {
        self.class
    }

    /// The character's level.
    #[must_use]
    pub fn level(&self) -> Level {
        self.level
    }

    /// The character's accumulated experience.
    #[must_use]
    pub fn experience(&self) -> Exp {
        self.experience
    }

    /// The character's trainable attributes.
    #[must_use]
    pub fn stats(&self) -> Stats {
        self.stats
    }

    /// Stat points earned but not yet allocated.
    #[must_use]
    pub fn unspent_points(&self) -> u16 {
        self.unspent_points
    }

    /// The zen the character carries.
    #[must_use]
    pub fn zen(&self) -> Zen {
        self.zen
    }

    /// Where the character stands and which way it faces.
    #[must_use]
    pub fn placement(&self) -> Placement {
        self.placement
    }

    /// The character's health, mana, and ability pools.
    #[must_use]
    pub fn vitals(&self) -> Vitals {
        self.vitals
    }

    /// The character's live timed effects.
    #[must_use]
    pub fn active_effects(&self) -> ActiveEffects {
        self.active_effects
    }
}

/// Rejection of a class-to-stats pairing that contradicts the command-class
/// rule, at construction or the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterError {
    /// Command stats on a class that does not train command.
    CommandStatsOutsideCommandClass(CharacterClass),
    /// Standard stats on a command class, which must train command.
    StandardStatsOnCommandClass(CharacterClass),
}

impl core::fmt::Display for CharacterError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::CommandStatsOutsideCommandClass(class) => {
                write!(f, "command stats on non-command class {class:?}")
            }
            Self::StandardStatsOnCommandClass(class) => {
                write!(f, "standard stats on command class {class:?}")
            }
        }
    }
}

impl core::error::Error for CharacterError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::movement::Movement;
    use crate::components::pool::Pool;
    use crate::components::spatial::Facing;
    use crate::components::tile::TileCoord;
    use crate::components::units::MapNumber;

    fn placement() -> Placement {
        Placement {
            position: TileCoord::new(180, 120).to_world(),
            facing: Facing::POS_Y,
            movement: Movement::Grounded,
            map: MapNumber(0),
        }
    }

    fn vitals() -> Vitals {
        Vitals {
            health: Pool::full(700),
            mana: Pool::full(200),
            ability: Pool::full(1),
        }
    }

    fn raw(class: CharacterClass, stats: Stats) -> RawCharacter {
        RawCharacter {
            class,
            level: Level::new(42).unwrap(),
            experience: Exp(1_234_567),
            stats,
            unspent_points: 15,
            zen: Zen(250_000),
            placement: placement(),
            vitals: vitals(),
            active_effects: ActiveEffects::EMPTY,
        }
    }

    fn standard() -> Stats {
        Stats::Standard {
            strength: 60,
            agility: 40,
            vitality: 50,
            energy: 30,
        }
    }

    fn with_command() -> Stats {
        Stats::WithCommand {
            strength: 55,
            agility: 35,
            vitality: 45,
            energy: 25,
            command: 30,
        }
    }

    #[test]
    fn dark_knight_standard_round_trips() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        assert_eq!(character.class(), CharacterClass::DarkKnight);
        assert_eq!(character.level().get(), 42);
        assert_eq!(character.stats(), standard());
        assert_eq!(character.zen(), Zen(250_000));
        let json = serde_json::to_string(&character).unwrap();
        assert_eq!(serde_json::from_str::<Character>(&json).unwrap(), character);
    }

    #[test]
    fn zen_is_a_required_wire_field() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("zen");
        assert!(serde_json::from_value::<Character>(value).is_err());
    }

    #[test]
    fn active_effects_seed_empty_and_round_trip() {
        use crate::components::active_effect::{ActiveEffect, ActiveEffects};
        use crate::components::units::Tick;

        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        // A fresh character seeds no active effects.
        assert_eq!(character.active_effects(), ActiveEffects::EMPTY);

        // A persisted record that omits the field still parses (defaults empty).
        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        let mut object = value.as_object().unwrap().clone();
        object.remove("active_effects");
        let legacy: Character = serde_json::from_value(serde_json::Value::Object(object)).unwrap();
        assert_eq!(legacy.active_effects(), ActiveEffects::EMPTY);

        // A character carrying an effect round-trips through the wire unchanged.
        let mut with_effect = value;
        with_effect["active_effects"] = serde_json::to_value(
            ActiveEffects::EMPTY.with(ActiveEffect::Defense { expiry: Tick(80) }),
        )
        .unwrap();
        let loaded: Character = serde_json::from_value(with_effect).unwrap();
        assert_eq!(loaded.active_effects().defense(), Some(Tick(80)));
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(&loaded).unwrap()).unwrap(),
            loaded
        );
    }

    #[test]
    fn dark_lord_with_command_round_trips() {
        let character = Character::try_from(raw(CharacterClass::DarkLord, with_command())).unwrap();
        assert_eq!(character.class(), CharacterClass::DarkLord);
        assert_eq!(character.stats(), with_command());
        let json = serde_json::to_string(&character).unwrap();
        assert_eq!(serde_json::from_str::<Character>(&json).unwrap(), character);
    }

    #[test]
    fn command_stats_on_non_command_class_are_rejected() {
        assert_eq!(
            Character::try_from(raw(CharacterClass::DarkWizard, with_command())),
            Err(CharacterError::CommandStatsOutsideCommandClass(
                CharacterClass::DarkWizard
            ))
        );
    }

    #[test]
    fn standard_stats_on_command_class_are_rejected() {
        assert_eq!(
            Character::try_from(raw(CharacterClass::DarkLord, standard())),
            Err(CharacterError::StandardStatsOnCommandClass(
                CharacterClass::DarkLord
            ))
        );
    }

    #[test]
    fn deserialization_reproves_the_gate() {
        let character = Character::try_from(raw(CharacterClass::DarkLord, with_command())).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        // Corrupt the persisted record: keep command stats, flip the class.
        value["class"] = serde_json::Value::String("dark_wizard".to_owned());
        assert!(serde_json::from_value::<Character>(value).is_err());
    }
}
