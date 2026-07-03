//! The kill orchestrator: one victim, one killer, one bundled reward. It
//! resolves the victim's level once, awards experience first (the money drop
//! rides the gained amount), then resolves drops — the two reward services stay
//! independent, composed only here. Pure and deterministic: the RNG is threaded
//! experience-first, then loot, in a fixed order.

use rand_core::RngCore;

use crate::components::units::{Exp, Level};
use crate::data::atlas::Atlas;
use crate::data::monster_definitions::MonsterRole;
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
    let Some(victim_level) = victim_combat_level(victim, atlas) else {
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

/// The victim's combat level, or `None` when the victim's definition carries no
/// combat block (a passive NPC or the soccer ball) or resolves to nothing — a
/// genuine "not a fighting monster" absence, folded here rather than unwrapped.
fn victim_combat_level(victim: &MonsterInstance, atlas: &Atlas) -> Option<Level> {
    match atlas.monster(victim.number)?.role {
        MonsterRole::Monster { combat, .. }
        | MonsterRole::Guard { combat, .. }
        | MonsterRole::Trap { combat, .. } => Some(combat.level),
        MonsterRole::Npc { .. } | MonsterRole::SoccerBall => None,
    }
}
