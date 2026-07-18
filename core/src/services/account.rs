//! Account progression: earning class-creation rights on a level crossing, and
//! the authoritative creation gate. Both are pure, RNG-free, clockless
//! functions over the account's earned-set and the static class table.
//!
//! The unlock step keys on the account's earned-set membership — not on how many
//! levels a crossing spanned — so it is idempotent by set semantics: re-running
//! the same crossing earns nothing. The creation gate reads the data
//! [`CreationGate`] as the primary discriminator, consulting the earned-set only
//! on the level-gated arm, so a stray member can never make an always-open or
//! evolution-only class disagree with its data.

use crate::components::class::CharacterClass;
use crate::components::units::Level;
use crate::components::unlocked_classes::UnlockedClasses;
use crate::data::classes::{ClassTable, CreationGate};
use crate::events::account::{ClassUnlocked, CreationVerdict};

/// Earns every level-gated class the character has now reached and the account
/// has not yet earned, given the top level a crossing `reached`. Grows the
/// earned-set and announces each newly-earned class, in roster order. Pure,
/// RNG-free, and idempotent: re-running at a level whose classes are all held
/// earns nothing and returns an equal set.
#[must_use]
pub fn unlock_classes_for_level(
    unlocked: UnlockedClasses,
    reached: Level,
    classes: &ClassTable,
) -> (UnlockedClasses, Vec<ClassUnlocked>) {
    let mut earned = unlocked;
    let mut events = Vec::new();
    for class in CharacterClass::ALL {
        match classes.record(class).creation {
            CreationGate::UnlockedAt { level } => {
                if reached >= level && !earned.contains(class) {
                    earned = earned.unlocked(class);
                    events.push(ClassUnlocked { class });
                }
            }
            CreationGate::Always | CreationGate::EvolutionOnly => {}
        }
    }
    (earned, events)
}

/// The authoritative verdict on whether `class` may be created by an account
/// holding `unlocked`. The data [`CreationGate`] is the primary discriminator;
/// the earned-set is consulted only for a level-gated class. Pure, RNG-free, and
/// read-only — the earned-set is never returned, so it is taken by shared
/// reference.
#[must_use]
pub fn creation_verdict(
    class: CharacterClass,
    unlocked: &UnlockedClasses,
    classes: &ClassTable,
) -> CreationVerdict {
    match classes.record(class).creation {
        CreationGate::Always => CreationVerdict::Creatable,
        CreationGate::EvolutionOnly => CreationVerdict::EvolutionOnly,
        CreationGate::UnlockedAt { level } => {
            if unlocked.contains(class) {
                CreationVerdict::Creatable
            } else {
                CreationVerdict::Locked { required: level }
            }
        }
    }
}
