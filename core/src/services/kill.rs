//! The kill orchestrator: one victim, one killer, one bundled reward. It
//! resolves the victim's level once, awards experience first (the money drop
//! rides the gained amount), then resolves drops — the two reward services stay
//! independent, composed only here. Pure and deterministic: the RNG is threaded
//! experience-first, then loot, in a fixed order.

use rand_core::RngCore;

use crate::components::units::Exp;
use crate::data::atlas::Atlas;
use crate::data::monster_definitions::MonsterDefinition;
use crate::entities::character::Character;
use crate::entities::monster_instance::MonsterInstance;
use crate::events::kill::KillResolution;
use crate::events::loot::{Drop, DropResolution};
use crate::events::progression::ExpAward;
use crate::services::experience::award_kill_experience;
use crate::services::loot::resolve_kill_drops;

/// Resolves a full kill into its bundled reward: experience (and the level-ups
/// it crosses) awarded first, then drops threaded with the gained experience so a
/// money drop is `experience + 7`. A victim that is not a fighting monster (no
/// combat block) yields no reward — a real "nothing awarded" outcome, not a
/// fabricated one.
#[must_use]
pub fn resolve_kill(
    killer: &Character,
    victim: &MonsterInstance,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> KillResolution {
    let Some(victim_level) = atlas
        .monster(victim.number)
        .and_then(MonsterDefinition::combat_level)
    else {
        return KillResolution {
            drops: DropResolution {
                category: Drop::Nothing,
                specials: Vec::new(),
            },
            experience: ExpAward { gained: Exp(0) },
            level_ups: Vec::new(),
        };
    };
    let (gained, level_ups) = award_kill_experience(killer, victim_level, atlas, rng);
    let drops = resolve_kill_drops(victim, victim_level, gained, atlas, rng);
    KillResolution {
        drops,
        experience: ExpAward { gained },
        level_ups,
    }
}
