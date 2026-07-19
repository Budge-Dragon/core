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
use crate::components::discovered_maps::DiscoveredMaps;
use crate::components::life::LifeState;
use crate::components::placement::Placement;
use crate::components::reputation::Reputation;
use crate::components::stats::Stats;
use crate::components::units::{CarriedZen, Exp, Level, MapNumber};
use crate::components::vitals::Vitals;
use crate::data::classes::ClassRecord;

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
    zen: CarriedZen,
    placement: Placement,
    vitals: Vitals,
    active_effects: ActiveEffects,
    life: LifeState,
    reputation: Reputation,
    discovered: DiscoveredMaps,
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
    zen: CarriedZen,
    placement: Placement,
    vitals: Vitals,
    /// A record that predates timed effects, or a freshly created character,
    /// carries none — the real "no active effects" value, not a fabricated
    /// default.
    #[serde(default = "ActiveEffects::empty")]
    active_effects: ActiveEffects,
    /// A record that predates the death lifecycle, or a freshly created
    /// character, carries no death — the real "no death recorded" value
    /// (`Alive`), not a fabricated default.
    #[serde(default = "LifeState::alive")]
    life: LifeState,
    /// A record that predates player-kill reputation, or a freshly created
    /// character, carries none — the real "no reputation recorded" value
    /// (clean), not a fabricated default.
    #[serde(default = "Reputation::clean")]
    reputation: Reputation,
    /// A record that predates map discovery carries no set at all; the gate
    /// seeds `{placement.map}`, the real "no discoveries recorded yet" value.
    /// `Option` rather than an empty default because a *present* set must
    /// contain the placement map or the record is rejected — absence and
    /// present-but-empty are different answers.
    #[serde(default)]
    discovered: Option<DiscoveredMaps>,
}

impl TryFrom<RawCharacter> for Character {
    type Error = CharacterError;

    fn try_from(raw: RawCharacter) -> Result<Self, Self::Error> {
        match (raw.class.has_command(), raw.stats) {
            (true, Stats::WithCommand { .. }) | (false, Stats::Standard { .. }) => {}
            (true, Stats::Standard { .. }) => {
                return Err(CharacterError::StandardStatsOnCommandClass(raw.class));
            }
            (false, Stats::WithCommand { .. }) => {
                return Err(CharacterError::CommandStatsOutsideCommandClass(raw.class));
            }
        }
        let discovered = match raw.discovered {
            None => DiscoveredMaps::single(raw.placement.map),
            Some(set) => {
                if !set.contains(raw.placement.map) {
                    return Err(CharacterError::DiscoveredMissingCurrentMap {
                        map: raw.placement.map,
                    });
                }
                set
            }
        };
        Ok(Self {
            class: raw.class,
            level: raw.level,
            experience: raw.experience,
            stats: raw.stats,
            unspent_points: raw.unspent_points,
            zen: raw.zen,
            placement: raw.placement,
            vitals: raw.vitals,
            active_effects: raw.active_effects,
            life: raw.life,
            reputation: raw.reputation,
            discovered,
        })
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
            life: character.life,
            reputation: character.reputation,
            discovered: Some(character.discovered),
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
    pub fn zen(&self) -> CarriedZen {
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

    /// Whether the character is alive or dead awaiting respawn.
    #[must_use]
    pub fn life(&self) -> LifeState {
        self.life
    }

    /// The character's player-kill reputation — clean or a flagged murderer.
    #[must_use]
    pub fn reputation(&self) -> Reputation {
        self.reputation
    }

    /// The maps the character has discovered by arriving on them. Always
    /// contains the current placement map, by construction.
    #[must_use]
    pub fn discovered(&self) -> &DiscoveredMaps {
        &self.discovered
    }

    /// A brand-new level-1 character of a class, standing at `placement` with
    /// `vitals`. Infallible by construction: `class` and `stats` are both derived
    /// from the ONE parse-proven [`ClassRecord`] — `class` from its identity,
    /// `stats` from `starting_stats.into()` — so the class-to-command pairing the
    /// [`TryFrom<RawCharacter>`] gate proves is already guaranteed here, and a
    /// mismatch is structurally impossible without a fallible re-check. Every
    /// progression scalar is seeded to its empty value through an infallible
    /// constant, and the discovered set is exactly the placement's own map, so the
    /// current-map invariant holds by construction too. Draws no randomness — the
    /// caller resolves the placement (its one landing pick) before calling.
    pub(crate) fn fresh(record: &ClassRecord, placement: Placement, vitals: Vitals) -> Character {
        Character {
            class: record.class,
            level: Level::MIN,
            experience: Exp::ZERO,
            stats: record.starting_stats.into(),
            unspent_points: 0,
            zen: CarriedZen::ZERO,
            placement,
            vitals,
            active_effects: ActiveEffects::EMPTY,
            life: LifeState::Alive,
            reputation: Reputation::clean(),
            discovered: DiscoveredMaps::single(placement.map),
        }
    }

    /// This character with its leveling scalars advanced; every other field —
    /// class, stats, zen, placement, vitals, active effects — carried unchanged.
    pub(crate) fn with_progress(
        self,
        level: Level,
        experience: Exp,
        unspent_points: u16,
    ) -> Character {
        Character {
            level,
            experience,
            unspent_points,
            ..self
        }
    }

    /// This character with its vitals reseated; every other field carried
    /// unchanged. The caller derives the refilled pools from the class formula.
    pub(crate) fn with_vitals(self, vitals: Vitals) -> Character {
        Character { vitals, ..self }
    }

    /// This character with its life state reseated; every other field carried
    /// unchanged. The death transitions flip it to `Dead` on a kill and back to
    /// `Alive` on respawn.
    pub(crate) fn with_life(self, life: LifeState) -> Character {
        Character { life, ..self }
    }

    /// This character with its player-kill reputation reseated; every other
    /// field carried unchanged. The reputation transitions flag, decay, and
    /// clear it through this writeback.
    pub(crate) fn with_reputation(self, reputation: Reputation) -> Character {
        Character { reputation, ..self }
    }

    /// This character with its carried zen reseated; every other field carried
    /// unchanged. The caller derives the docked balance through `CarriedZen`.
    pub(crate) fn with_zen(self, zen: CarriedZen) -> Character {
        Character { zen, ..self }
    }

    /// This character with its active effects reseated; every other field
    /// carried unchanged. Respawn reseats the empty store — a clean slate.
    pub(crate) fn with_effects(self, active_effects: ActiveEffects) -> Character {
        Character {
            active_effects,
            ..self
        }
    }

    /// This character arrived at `placement`: the placement is reseated AND
    /// the destination map joins the discovered set in one move, so "arrive
    /// without discovering" is unrepresentable. The shared writeback every
    /// map-crossing funnels through — warp, enter-gate traversal, respawn,
    /// town portal. Idempotent on the set for a same-map arrival; every other
    /// field carried unchanged.
    pub(crate) fn arrived_at(self, placement: Placement) -> Character {
        Character {
            placement,
            discovered: self.discovered.inserted(placement.map),
            ..self
        }
    }
}

/// Rejection of a record that contradicts a character invariant — the
/// class-to-stats command pairing, or a discovered set missing the map the
/// character stands on — at construction or the data-load boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CharacterError {
    /// Command stats on a class that does not train command.
    CommandStatsOutsideCommandClass(CharacterClass),
    /// Standard stats on a command class, which must train command.
    StandardStatsOnCommandClass(CharacterClass),
    /// A present discovered set that does not contain the placement map — a
    /// character standing on an undiscovered map is unrepresentable.
    DiscoveredMissingCurrentMap {
        /// The placement map the set fails to contain.
        map: MapNumber,
    },
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
            Self::DiscoveredMissingCurrentMap { map } => {
                write!(f, "discovered set does not contain the current map {map:?}")
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
    use crate::components::reputation::{PkStage, Standing};
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
            zen: CarriedZen::new(250_000).unwrap(),
            placement: placement(),
            vitals: vitals(),
            active_effects: ActiveEffects::EMPTY,
            life: LifeState::Alive,
            reputation: Reputation::clean(),
            discovered: None,
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

    /// A `ClassRecord` deserialized through its real parse gate — the only way
    /// to obtain one (the `starting_kit` inner list is module-private), so the
    /// fresh-constructor tests exercise the same proven pairing creation reads.
    fn class_record(value: serde_json::Value) -> ClassRecord {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn fresh_seeds_zero_progression_alive_clean_and_home_discovered() {
        let record = class_record(serde_json::json!({
            "class": "dark_knight", "number": 4,
            "creation": {"kind": "always"}, "evolution": {"kind": "terminal"},
            "home_map": 0, "points_per_level": 5,
            "starting_stats": {"kind": "standard", "strength": 28, "agility": 20, "vitality": 25, "energy": 10},
            "starting_kit": [],
            "fruit_points_divisor": 400, "warp_requirement": {"kind": "full"},
            "source_version": "075"
        }));
        let fresh = Character::fresh(&record, placement(), vitals());

        assert_eq!(fresh.class(), CharacterClass::DarkKnight);
        assert_eq!(fresh.level(), Level::MIN);
        assert_eq!(fresh.experience(), Exp::ZERO);
        assert_eq!(fresh.unspent_points(), 0);
        assert_eq!(fresh.zen(), CarriedZen::ZERO);
        assert_eq!(fresh.active_effects(), ActiveEffects::EMPTY);
        assert_eq!(fresh.life(), LifeState::Alive);
        assert_eq!(fresh.reputation(), Reputation::clean());
        assert_eq!(*fresh.discovered(), DiscoveredMaps::single(MapNumber(0)));
        assert_eq!(
            fresh.stats(),
            Stats::Standard {
                strength: 28,
                agility: 20,
                vitality: 25,
                energy: 10,
            }
        );
        // The class↔stats pairing re-proves on a persist round-trip.
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(&fresh).unwrap()).unwrap(),
            fresh
        );
    }

    #[test]
    fn fresh_dark_lord_carries_with_command_stats_derived_from_the_record() {
        let record = class_record(serde_json::json!({
            "class": "dark_lord", "number": 16,
            "creation": {"kind": "unlocked_at", "level": 250}, "evolution": {"kind": "terminal"},
            "home_map": 0, "points_per_level": 7,
            "starting_stats": {"kind": "with_command", "strength": 26, "agility": 20, "vitality": 20, "energy": 15, "command": 25},
            "starting_kit": [],
            "fruit_points_divisor": 500, "warp_requirement": {"kind": "fraction", "numerator": 2, "denominator": 3},
            "source_version": "s6"
        }));
        let fresh = Character::fresh(&record, placement(), vitals());
        assert_eq!(fresh.class(), CharacterClass::DarkLord);
        assert_eq!(
            fresh.stats(),
            Stats::WithCommand {
                strength: 26,
                agility: 20,
                vitality: 20,
                energy: 15,
                command: 25,
            }
        );
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
        assert_eq!(character.zen(), CarriedZen::new(250_000).unwrap());
        let json = serde_json::to_string(&character).unwrap();
        assert!(json.contains(r#""zen":250000"#), "zen is a bare integer");
        assert_eq!(serde_json::from_str::<Character>(&json).unwrap(), character);
    }

    #[test]
    fn a_persisted_zen_above_the_cap_fails_to_parse() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        value["zen"] = serde_json::json!(2_000_000_001_u64);
        assert!(serde_json::from_value::<Character>(value).is_err());
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
    fn with_progress_advances_scalars_and_carries_the_rest() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let grown = character
            .clone()
            .with_progress(Level::new(43).unwrap(), Exp(2_000_000), 20);
        assert_eq!(grown.level(), Level::new(43).unwrap());
        assert_eq!(grown.experience(), Exp(2_000_000));
        assert_eq!(grown.unspent_points(), 20);
        // Every other field is carried verbatim.
        assert_eq!(grown.class(), character.class());
        assert_eq!(grown.stats(), character.stats());
        assert_eq!(grown.zen(), character.zen());
        assert_eq!(grown.placement(), character.placement());
        assert_eq!(grown.vitals(), character.vitals());
        assert_eq!(grown.active_effects(), character.active_effects());
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(&grown).unwrap()).unwrap(),
            grown
        );
    }

    #[test]
    fn with_vitals_reseats_only_the_pools() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let refilled = Vitals {
            health: Pool::full(1500),
            mana: Pool::full(500),
            ability: Pool::full(42),
        };
        let reseated = character.clone().with_vitals(refilled);
        assert_eq!(reseated.vitals(), refilled);
        assert_eq!(reseated.level(), character.level());
        assert_eq!(reseated.experience(), character.experience());
        assert_eq!(reseated.unspent_points(), character.unspent_points());
        assert_eq!(reseated.class(), character.class());
        assert_eq!(reseated.stats(), character.stats());
        assert_eq!(reseated.zen(), character.zen());
        assert_eq!(reseated.placement(), character.placement());
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

    #[test]
    fn life_defaults_alive_and_dead_round_trips() {
        use crate::components::units::Tick;

        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        // A fresh character records no death.
        assert_eq!(character.life(), LifeState::Alive);

        let value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();

        // A persisted record that omits the field still parses (defaults Alive).
        let mut object = value.as_object().unwrap().clone();
        object.remove("life");
        let legacy: Character = serde_json::from_value(serde_json::Value::Object(object)).unwrap();
        assert_eq!(legacy.life(), LifeState::Alive);

        // A record marked Dead round-trips through the wire, its life tagged
        // "dead" carrying respawn_at.
        let mut dead_record = value;
        dead_record["life"] = serde_json::json!({"kind": "dead", "respawn_at": 903});
        let dead: Character = serde_json::from_value(dead_record).unwrap();
        assert_eq!(
            dead.life(),
            LifeState::Dead {
                respawn_at: Tick(903)
            }
        );
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(&dead).unwrap()).unwrap(),
            dead
        );
    }

    #[test]
    fn a_new_character_is_clean_and_reputation_round_trips() {
        use crate::components::units::Tick;

        let c = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        assert_eq!(c.reputation(), Reputation::clean());
        let flagged = c
            .reputation()
            .with_standing(Standing::Flagged {
                stage: PkStage::Warning,
                decays_at: Tick(7),
            })
            .with_recorded_kill();
        let c = c.with_reputation(flagged);
        assert_eq!(c.reputation(), flagged);
        // Wire round-trip through RawCharacter.
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(
            serde_json::from_str::<Character>(&json)
                .unwrap()
                .reputation(),
            flagged
        );
    }

    #[test]
    fn a_pre_w_pk_wire_form_defaults_to_clean() {
        let c = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let mut v = serde_json::to_value(&c).unwrap();
        v.as_object_mut().unwrap().remove("reputation");
        assert_eq!(
            serde_json::from_value::<Character>(v).unwrap().reputation(),
            Reputation::clean()
        );
    }

    /// Every field but the one the builder reseats is carried verbatim.
    fn assert_carries_all_but(reseated: &Character, base: &Character) {
        assert_eq!(reseated.class(), base.class());
        assert_eq!(reseated.level(), base.level());
        assert_eq!(reseated.experience(), base.experience());
        assert_eq!(reseated.stats(), base.stats());
        assert_eq!(reseated.unspent_points(), base.unspent_points());
        assert_eq!(reseated.discovered(), base.discovered());
        // Persist round-trip — the class↔stats gate re-proves on load.
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(reseated).unwrap()).unwrap(),
            *reseated
        );
    }

    #[test]
    fn with_life_reseats_only_life() {
        use crate::components::units::Tick;
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let dead = character.clone().with_life(LifeState::Dead {
            respawn_at: Tick(903),
        });
        assert_eq!(
            dead.life(),
            LifeState::Dead {
                respawn_at: Tick(903)
            }
        );
        assert_carries_all_but(&dead, &character);
        assert_eq!(dead.zen(), character.zen());
        assert_eq!(dead.placement(), character.placement());
        assert_eq!(dead.vitals(), character.vitals());
        assert_eq!(dead.active_effects(), character.active_effects());
    }

    #[test]
    fn with_zen_reseats_only_zen() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let docked = character
            .clone()
            .with_zen(CarriedZen::new(990_000).unwrap());
        assert_eq!(docked.zen(), CarriedZen::new(990_000).unwrap());
        assert_carries_all_but(&docked, &character);
        assert_eq!(docked.life(), character.life());
        assert_eq!(docked.placement(), character.placement());
        assert_eq!(docked.vitals(), character.vitals());
        assert_eq!(docked.active_effects(), character.active_effects());
    }

    #[test]
    fn with_effects_reseats_only_effects() {
        use crate::components::active_effect::ActiveEffect;
        use crate::components::units::Tick;
        let seeded = {
            let base = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
            base.with_effects(ActiveEffects::EMPTY.with(ActiveEffect::Defense { expiry: Tick(80) }))
        };
        // Clearing back to empty reseats only the store.
        let cleared = seeded.clone().with_effects(ActiveEffects::EMPTY);
        assert_eq!(cleared.active_effects(), ActiveEffects::EMPTY);
        assert_carries_all_but(&cleared, &seeded);
        assert_eq!(cleared.life(), seeded.life());
        assert_eq!(cleared.zen(), seeded.zen());
        assert_eq!(cleared.placement(), seeded.placement());
        assert_eq!(cleared.vitals(), seeded.vitals());
    }

    #[test]
    fn arrived_at_reseats_placement_and_discovers() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let landing = Placement {
            position: TileCoord::new(174, 112).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(3),
        };
        let moved = character.clone().arrived_at(landing);
        assert_eq!(moved.placement(), landing);
        // The destination map joined the set; the home map is still a member.
        assert!(moved.discovered().contains(MapNumber(3)));
        assert!(moved.discovered().contains(MapNumber(0)));
        // Every other field is carried verbatim.
        assert_eq!(moved.class(), character.class());
        assert_eq!(moved.level(), character.level());
        assert_eq!(moved.experience(), character.experience());
        assert_eq!(moved.stats(), character.stats());
        assert_eq!(moved.unspent_points(), character.unspent_points());
        assert_eq!(moved.life(), character.life());
        assert_eq!(moved.zen(), character.zen());
        assert_eq!(moved.vitals(), character.vitals());
        assert_eq!(moved.active_effects(), character.active_effects());
        // Persist round-trip — the parse gates re-prove on load.
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(&moved).unwrap()).unwrap(),
            moved
        );
    }

    #[test]
    fn arrived_at_the_current_map_is_idempotent_on_the_discovered_set() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let reseated = character.clone().arrived_at(Placement {
            position: TileCoord::new(174, 112).to_world(),
            facing: Facing::POS_X,
            movement: Movement::Grounded,
            map: MapNumber(0),
        });
        assert_eq!(reseated.discovered(), character.discovered());
        assert_eq!(reseated.placement().map, MapNumber(0));
    }

    #[test]
    fn an_absent_discovered_field_seeds_the_placement_map() {
        // The raw fixture carries no set (a legacy record); the gate seeds
        // exactly {placement.map}.
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        assert_eq!(
            *character.discovered(),
            DiscoveredMaps::single(MapNumber(0))
        );

        // The same absence on the wire: a persisted record that omits the
        // field still parses and seeds its own map.
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("discovered");
        let legacy: Character = serde_json::from_value(value).unwrap();
        assert_eq!(*legacy.discovered(), DiscoveredMaps::single(MapNumber(0)));
    }

    #[test]
    fn a_present_discovered_set_missing_the_current_map_fails_to_parse() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        // Present but not containing the placement map (0) — rejected; an
        // empty array is the same "present but not containing" refusal.
        for corrupted in [serde_json::json!([3, 4]), serde_json::json!([])] {
            value["discovered"] = corrupted;
            assert!(serde_json::from_value::<Character>(value.clone()).is_err());
        }
    }

    #[test]
    fn a_present_discovered_set_containing_the_current_map_round_trips() {
        let character = Character::try_from(raw(CharacterClass::DarkKnight, standard())).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&character).unwrap()).unwrap();
        value["discovered"] = serde_json::json!([0, 2, 4]);
        let traveled: Character = serde_json::from_value(value).unwrap();
        assert_eq!(
            *traveled.discovered(),
            DiscoveredMaps::single(MapNumber(0))
                .inserted(MapNumber(2))
                .inserted(MapNumber(4))
        );
        assert_eq!(
            serde_json::from_str::<Character>(&serde_json::to_string(&traveled).unwrap()).unwrap(),
            traveled
        );
    }
}
