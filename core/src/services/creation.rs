//! Character creation: turning a class into a fresh, playable level-1 character
//! with its worn starter gear. One pure constructor family in the
//! [`crate::services::spawn`] / [`crate::services::death::respawn`] shape — it
//! takes a class plus the resolved dataset and returns a *value* (never a
//! `(state, events)` transition and never an event: a created character has no
//! observable outcome to deliver, only the new state itself).
//!
//! Creation is **infallible**. The two upstream gates — creatability (the
//! W-ACCOUNT unlock verdict) and account capacity ([`crate::components::character_slot::CharacterSlot::CAP`])
//! — are the host's to enforce before it ever calls here, and the class's home
//! map is proven at Atlas load to own a walkable town landing, so this function
//! only builds. It draws exactly one random word: the landing pick, shared with
//! respawn through [`resolve_spawn_gate_landing`].

use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::components::class::CharacterClass;
use crate::components::equipment::Equipment;
use crate::components::stats::Stats;
use crate::components::units::Level;
use crate::components::vitals::Vitals;
use crate::data::atlas::Atlas;
use crate::entities::character::Character;
use crate::services::movement::resolve_town_landing;
use crate::services::profile::profile_of;

/// The two live values character creation mints: the fresh [`Character`] and its
/// worn starter [`Equipment`]. They are separate values because a `Character`
/// holds no worn set — the host keys equipment at a parallel index (the
/// `paper_host` seating convention). The empty inventory bag is the host's to
/// mint, so it is deliberately absent here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreatedCharacter {
    /// The brand-new level-1 character.
    pub character: Character,
    /// The starter gear it is created wearing (empty for Dark Wizard).
    pub equipment: Equipment,
}

/// Builds a fresh level-1 character of `class` with its worn starter kit. Pure,
/// deterministic, infallible; draws exactly one random word (the landing pick).
///
/// The character carries the class's starting stats (mapped 1:1 from the
/// parse-proven record), no experience, no unspent points, no zen, full
/// health/mana/ability at the class-formula maxima, a clean record, and only its
/// home town discovered — standing on a random walkable tile of that town, alive.
/// Its worn set is the class's authored starter items, each a plain instance at
/// full base durability. Vitals seed from the **same** source respawn uses —
/// `Pool::full` at the [`profile_of`] maxima — so a fresh character and a
/// respawned one of the same class share one source of truth.
#[must_use]
pub fn create_character(
    class: CharacterClass,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> CreatedCharacter {
    let record = atlas.classes().record(class);

    let stats = Stats::from(record.starting_stats);
    let (_profile, maxima) = profile_of(class, Level::MIN, &stats);
    let vitals = Vitals::full(maxima);

    let placement = resolve_town_landing(atlas, record.home_map, rng);

    let character = Character::fresh(record, placement, vitals);
    let equipment = atlas
        .starting_kit(class)
        .iter()
        .fold(Equipment::empty(), |worn, entry| {
            worn.with(entry.slot, entry.item_instance.clone())
        });

    CreatedCharacter {
        character,
        equipment,
    }
}
