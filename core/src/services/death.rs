//! The monster-kill death lifecycle: the death step that docks the penalty and
//! marks a character dead, and the later respawn that seats it back in town. Two
//! pure transitions over the character's [`LifeState`], each a value in and a
//! value out with no in-place edit.
//!
//! [`resolve_death`] draws **no** randomness — the penalty is pure integer ratio
//! math floored at the current level's threshold, so it never de-levels — and is
//! idempotent on an already-dead character. [`respawn`] draws exactly the landing
//! sample: one uniform pick over the death-map gate's retained walkable set (else
//! the Lorencia fallback), refills the three vitals to the class-formula maxima,
//! clears every active effect, and returns the character alive. Both mirror the
//! [`crate::services::experience::apply_experience`] writeback template.

use rand_core::RngCore;

use crate::components::active_effect::ActiveEffects;
use crate::components::life::LifeState;
use crate::components::movement::Movement;
use crate::components::placement::Placement;
use crate::components::pool::Pool;
use crate::components::spatial::Facing;
use crate::components::units::{
    CarriedZen, DebitOutcome, DurationMs, Exp, Level, Tick, TickDuration, Zen,
};
use crate::components::vitals::Vitals;
use crate::data::atlas::Atlas;
use crate::entities::character::Character;
use crate::events::death::{DeathEvent, Respawned};
use crate::services::chance::pick_one;
use crate::services::profile::character_profile;
use crate::services::ratio::{nonzero_u64, scale_ratio_u64};

/// The dead beat before respawn — an OUR-pin, 3 seconds, converted to whole
/// ticks against the host's tick base so no tick rate is baked in.
const RESPAWN_DELAY_MS: DurationMs = DurationMs(3_000);

/// The heading a respawn seats when its gate carries no authored direction —
/// the common MU spawn heading. An engineering pin; every real gate is
/// direction-less today, so this is the facing every respawn produces.
const DEFAULT_FACING: Facing = Facing::POS_Y;

/// Below this level a death is free — neither experience nor zen is docked.
const PENALTY_FREE_BELOW_LEVEL: u16 = 10;

/// The denominator every death penalty scales over: a percentage. The exp loss
/// is one part, the zen loss one, two, or three by level bracket.
const PERCENT_DENOMINATOR: u64 = 100;

/// The monster-kill death step: docks the experience and zen penalty and marks
/// the character dead, scheduling its respawn `RESPAWN_DELAY_MS` after `at`. A
/// pure deterministic transition — no RNG (the penalty is integer ratio math),
/// no in-place edit. On an already-dead character it is a no-op: the input is
/// returned byte-identical with an empty event list, so a second death applies
/// no second penalty. The events are `Died` first, then the exp and zen docks
/// only when their magnitude is non-zero.
#[must_use]
pub fn resolve_death(
    character: &Character,
    at: Tick,
    tick: TickDuration,
    atlas: &Atlas,
) -> (Character, Vec<DeathEvent>) {
    match character.life() {
        LifeState::Dead { .. } => (character.clone(), Vec::new()),
        LifeState::Alive => {
            let respawn_at = at + RESPAWN_DELAY_MS.in_ticks(tick);
            let mut events = vec![DeathEvent::Died { respawn_at }];

            let after_exp = match exp_penalty(character, atlas) {
                ExpPenalty::None => character.clone(),
                ExpPenalty::Docked { new_exp, lost } => {
                    events.push(DeathEvent::ExperienceDocked {
                        lost,
                        remaining: new_exp,
                    });
                    character.with_progress(character.level(), new_exp, character.unspent_points())
                }
            };

            let after_zen = match zen_penalty(character.level(), character.zen()) {
                ZenPenalty::None => after_exp,
                ZenPenalty::Docked { balance, lost } => {
                    events.push(DeathEvent::ZenDocked {
                        lost,
                        remaining: balance,
                    });
                    after_exp.with_zen(balance)
                }
            };

            (after_zen.with_life(LifeState::Dead { respawn_at }), events)
        }
    }
}

/// The respawn step: seats a dead character back in town and returns it alive.
/// Selects the death map's first spawn gate, else the Lorencia fallback; samples
/// one walkable landing tile from the gate's retained set; refills the three
/// vitals to the class-formula maxima; clears every active effect. Draws exactly
/// one random word — the landing pick. On an already-alive character it is a
/// no-op that returns before touching the RNG, so the stream is untouched.
#[must_use]
pub fn respawn(
    character: &Character,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Character, Option<Respawned>) {
    match character.life() {
        LifeState::Alive => (character.clone(), None),
        LifeState::Dead { .. } => {
            let gate = match atlas.spawn_gate(character.placement().map) {
                Some(view) => view,
                None => atlas.fallback_spawn_gate(),
            };
            let position = *pick_one(gate.landing, rng);
            let facing = match gate.facing {
                Some(authored) => authored,
                None => DEFAULT_FACING,
            };
            let map = gate.map;

            let (_profile, maxima) = character_profile(character);
            let refilled = Vitals {
                health: Pool::full(maxima.max_health),
                mana: Pool::full(maxima.max_mana),
                ability: Pool::full(maxima.max_ability),
            };

            let revived = character
                .with_placement(Placement {
                    position,
                    facing,
                    movement: Movement::Grounded,
                    map,
                })
                .with_vitals(refilled)
                .with_effects(ActiveEffects::EMPTY)
                .with_life(LifeState::Alive);

            (
                revived,
                Some(Respawned {
                    map,
                    position,
                    facing,
                }),
            )
        }
    }
}

/// The experience penalty's outcome: nothing docked, or a docked delta with the
/// floored new total.
enum ExpPenalty {
    None,
    Docked { new_exp: Exp, lost: Exp },
}

/// The zen penalty's outcome: nothing docked, or a docked amount with the
/// remaining balance.
enum ZenPenalty {
    None,
    Docked { balance: CarriedZen, lost: Zen },
}

/// The experience docked by a death: one percent of the current level's band,
/// floored at the level's own threshold so a death never de-levels. Zero below
/// level 10 and at the level cap (no next-level band exists). The reported loss
/// is the applied delta — the sliver when the floor bites, not the nominal —
/// and a zero applied delta docks nothing.
fn exp_penalty(character: &Character, atlas: &Atlas) -> ExpPenalty {
    let curve = atlas.exp_curve();
    let level = character.level();
    if level.get() < PENALTY_FREE_BELOW_LEVEL || level == curve.max_level() {
        return ExpPenalty::None;
    }

    let raw = level.get();
    // The free-below-10 branch leaves `level >= 10`; the `== max` branch removes
    // only the cap itself, so an untrusted level above the cap (`max+1 ..= u16::MAX`)
    // still reaches here. Any curve position with no next-level band — the cap, or
    // any over-cap level — has nothing to lose, so the non-`Ok` arms fold to no
    // penalty rather than fabricating a band, encoding the same rule the cap branch
    // does. The next-level read saturates, so `u16::MAX` cannot overflow the add; it
    // stays `65535`, misses the curve, and folds to no penalty. The floor is this
    // level's own threshold, read from the same lookup.
    let (floor, band) = match (curve.level(raw), curve.level(raw.saturating_add(1))) {
        (Ok(here), Ok(next)) => {
            let floor = here.total_to_hold().0;
            (floor, next.total_to_hold().0.saturating_sub(floor))
        }
        (Ok(_) | Err(_), Err(_)) | (Err(_), Ok(_)) => return ExpPenalty::None,
    };

    let nominal = scale_ratio_u64(band, 1, nonzero_u64(PERCENT_DENOMINATOR));
    let new_exp = floor.max(character.experience().0.saturating_sub(nominal));
    let lost = character.experience().0.saturating_sub(new_exp);
    if lost == 0 {
        return ExpPenalty::None;
    }
    ExpPenalty::Docked {
        new_exp: Exp(new_exp),
        lost: Exp(lost),
    }
}

/// The zen docked by a death: a bracketed percentage of carried zen, removed
/// through the balance-preserving [`CarriedZen::debit`]. Debit runs only when the
/// penalty is positive, so a sub-level-10 death and a balance whose percentage
/// floors to zero both dock nothing. The debit is provably covered (the
/// percentage is at most three of a hundred), so `Insufficient` is unreachable;
/// it folds into the same "nothing docked" path without a suppressor.
fn zen_penalty(level: Level, carried: CarriedZen) -> ZenPenalty {
    let percent = zen_penalty_percent(level);
    let penalty = scale_ratio_u64(carried.get(), percent, nonzero_u64(PERCENT_DENOMINATOR));
    if penalty == 0 {
        return ZenPenalty::None;
    }
    match carried.debit(Zen(penalty)) {
        DebitOutcome::Debited { balance } => ZenPenalty::Docked {
            balance,
            lost: Zen(penalty),
        },
        DebitOutcome::Insufficient { .. } => ZenPenalty::None,
    }
}

/// The zen penalty percentage by level bracket: nothing below 10, one percent
/// for 10–149, two for 150–219, three for 220 and up. A total match over the
/// level ranges — every level maps to exactly one bracket.
fn zen_penalty_percent(level: Level) -> u64 {
    match level.get() {
        0..=9 => 0,
        10..=149 => 1,
        150..=219 => 2,
        220.. => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(value: u16) -> Level {
        Level::new(value).unwrap()
    }

    fn carried(value: u64) -> CarriedZen {
        CarriedZen::new(value).unwrap()
    }

    #[test]
    fn zen_bracket_percentages_engage_at_their_boundary_levels() {
        assert_eq!(zen_penalty_percent(level(9)), 0);
        assert_eq!(zen_penalty_percent(level(10)), 1);
        assert_eq!(zen_penalty_percent(level(149)), 1);
        assert_eq!(zen_penalty_percent(level(150)), 2);
        assert_eq!(zen_penalty_percent(level(219)), 2);
        assert_eq!(zen_penalty_percent(level(220)), 3);
        assert_eq!(zen_penalty_percent(level(400)), 3);
    }

    #[test]
    fn zen_penalty_docks_the_bracket_percentage_of_carried() {
        for (lvl, expected_lost, expected_remaining) in [
            (100, 10_000, 990_000),
            (180, 20_000, 980_000),
            (300, 30_000, 970_000),
        ] {
            match zen_penalty(level(lvl), carried(1_000_000)) {
                ZenPenalty::Docked { balance, lost } => {
                    assert_eq!(lost, Zen(expected_lost), "level {lvl} lost");
                    assert_eq!(
                        balance,
                        carried(expected_remaining),
                        "level {lvl} remaining"
                    );
                }
                ZenPenalty::None => panic!("level {lvl} docks a positive penalty"),
            }
        }
    }

    #[test]
    fn zen_penalty_is_free_below_ten_and_when_the_floor_is_zero() {
        // Below level 10 a death is free.
        assert!(matches!(
            zen_penalty(level(6), carried(250_000)),
            ZenPenalty::None
        ));
        // One percent of 50 floors to zero, so nothing is docked.
        assert!(matches!(
            zen_penalty(level(100), carried(50)),
            ZenPenalty::None
        ));
    }

    #[test]
    fn the_our_pins_hold_their_values() {
        assert_eq!(DEFAULT_FACING, Facing::POS_Y);
        assert_eq!(RESPAWN_DELAY_MS, DurationMs(3_000));
    }
}
