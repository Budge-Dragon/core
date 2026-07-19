//! The monster-kill death lifecycle: the death step that docks the penalty —
//! or waives it, per the caller's [`DeathPenalty`] policy — and marks a
//! character dead, and the later respawn that seats it back in town. Two pure
//! transitions over the character's [`LifeState`], each a value in and a value
//! out with no in-place edit.
//!
//! [`resolve_death`] draws **no** randomness — the penalty is pure integer ratio
//! math floored at the current level's threshold, so it never de-levels — and is
//! idempotent on an already-dead character. [`respawn`] draws exactly the landing
//! sample: one uniform pick over the death map's town gate's retained walkable
//! set (else the Lorencia fallback), refills the three vitals to the
//! class-formula maxima, clears every active effect, and returns the character
//! alive on a now-discovered town. Both mirror the
//! [`crate::services::experience::apply_experience`] writeback template.

use rand_core::RngCore;

use crate::components::active_effect::ActiveEffects;
use crate::components::combat_profile::TargetKind;
use crate::components::life::LifeState;
use crate::components::reputation::{PkStage, Reputation, Standing};
use crate::components::units::{
    CarriedZen, DebitOutcome, DurationMs, Exp, Level, Tick, TickDuration, Zen,
};
use crate::components::vitals::Vitals;
use crate::data::atlas::Atlas;
use crate::entities::character::Character;
use crate::events::death::{DeathEvent, Respawned};
use crate::services::movement::resolve_town_landing;
use crate::services::profile::character_profile;
use crate::services::ratio::{nonzero_u64, scale_ratio_u64};

/// The dead beat before respawn — an OUR-pin, 3 seconds, converted to whole
/// ticks against the host's tick base so no tick rate is baked in.
const RESPAWN_DELAY_MS: DurationMs = DurationMs(3_000);

/// Below this level a death is free — neither experience nor zen is docked.
const PENALTY_FREE_BELOW_LEVEL: u16 = 10;

/// The denominator every death penalty scales over: a percentage. The exp loss
/// is one part, the zen loss one, two, or three by level bracket.
const PERCENT_DENOMINATOR: u64 = 100;

/// Whether a death docks the experience/zen penalty. `Waived` is the SAME
/// death transition with the penalties skipped — never a second death path. A
/// plain service-input enum, not persisted state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeathPenalty {
    /// Dock the experience and zen penalty (every normal death).
    Applied,
    /// Dock nothing (a death inside a mini-game).
    Waived,
}

/// The penalty a *combat* death applies, decided by the attacker's kind: a
/// player kill waives the victim's experience and zen penalty (authentic — the
/// killer is not a monster), a monster kill applies the classic penalty. The
/// rule lives in core so a host cannot forge it: the deciding fact is the
/// attacker's core-stamped [`TargetKind`], never a client claim, which makes it
/// symmetric with the overrate matchup in the combat service. The
/// mini-game-membership waiver stays a separate host-owned override (session
/// membership is host state, not a combat fact).
#[must_use]
pub fn combat_death_penalty(attacker: TargetKind) -> DeathPenalty {
    match attacker {
        TargetKind::Player => DeathPenalty::Waived,
        TargetKind::Npc => DeathPenalty::Applied,
    }
}

/// The monster-kill death step: docks the experience and zen penalty — under
/// [`DeathPenalty::Applied`]; [`DeathPenalty::Waived`] docks nothing — and
/// marks the character dead, scheduling its respawn `RESPAWN_DELAY_MS` after
/// `at`. A pure deterministic transition — no RNG (the penalty is integer
/// ratio math), no in-place edit. On an already-dead character it is a no-op:
/// the input is returned byte-identical with an empty event list, so a second
/// death applies no second penalty. The events are `Died` first, then — under
/// `Applied` only — the exp and zen docks when their magnitude is non-zero; a
/// waived death emits `Died` alone.
#[must_use]
pub fn resolve_death(
    character: Character,
    at: Tick,
    tick: TickDuration,
    atlas: &Atlas,
    penalty: DeathPenalty,
) -> (Character, Vec<DeathEvent>) {
    match character.life() {
        LifeState::Dead { .. } => (character, Vec::new()),
        LifeState::Alive => {
            let respawn_at = at + RESPAWN_DELAY_MS.in_ticks(tick);
            let mut events = vec![DeathEvent::Died { respawn_at }];

            // The Copy scalars the penalty math reads are captured before the
            // character is moved into the writeback below, so the read and the
            // move never share one expression.
            let level = character.level();
            let unspent_points = character.unspent_points();
            let zen = character.zen();

            let docked = match penalty {
                DeathPenalty::Waived => character,
                DeathPenalty::Applied => {
                    let after_exp = match exp_penalty(&character, atlas) {
                        ExpPenalty::None => character,
                        ExpPenalty::Docked { new_exp, lost } => {
                            events.push(DeathEvent::ExperienceDocked {
                                lost,
                                remaining: new_exp,
                            });
                            character.with_progress(level, new_exp, unspent_points)
                        }
                    };

                    match zen_penalty(level, zen) {
                        ZenPenalty::None => after_exp,
                        ZenPenalty::Docked { balance, lost } => {
                            events.push(DeathEvent::ZenDocked {
                                lost,
                                remaining: balance,
                            });
                            after_exp.with_zen(balance)
                        }
                    }
                }
            };

            (docked.with_life(LifeState::Dead { respawn_at }), events)
        }
    }
}

/// The respawn step: seats a dead character back in town and returns it alive.
/// Selects the death map's town gate (its own town, an override town, or
/// Lorencia), else the Lorencia fallback for a map outside the 11; seats one
/// walkable landing from the gate's retained set via the shared spawn-gate
/// primitive; refills the three vitals to the class-formula maxima; clears
/// every active effect; and — arrival being arrival — the respawn town joins
/// the discovered set. Draws exactly one random word — the landing pick. On an
/// already-alive character it is a no-op that returns before touching the RNG,
/// so the stream is untouched.
#[must_use]
pub fn respawn(
    character: Character,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Character, Option<Respawned>) {
    match character.life() {
        LifeState::Alive => (character, None),
        LifeState::Dead { .. } => {
            let placement = resolve_town_landing(atlas, character.placement().map, rng);

            let (_profile, maxima) = character_profile(&character);
            let refilled = Vitals::full(maxima);

            let revived = character
                .arrived_at(placement)
                .with_vitals(refilled)
                .with_effects(ActiveEffects::EMPTY)
                .with_life(LifeState::Alive);

            (
                revived,
                Some(Respawned {
                    map: placement.map,
                    position: placement.position,
                    facing: placement.facing,
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

    let percent = exp_loss_percent(level, character.reputation());
    let nominal = scale_ratio_u64(band, percent, nonzero_u64(PERCENT_DENOMINATOR));
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

/// The monster-death experience-loss percent, forked by the dying character's
/// player-kill standing and level band. Clean is the classic one percent; the
/// flagged bands are a `CMB-CONST` tunable, verbatim from OpenMU's
/// `PlayerLosesExperienceAfterDeathPlugIn`. A total match over the level ranges
/// and the three ladder rungs — the `0..=149` band subsumes the sub-ten levels
/// the caller's early-return already excludes.
fn exp_loss_percent(level: Level, reputation: Reputation) -> u64 {
    match reputation.standing() {
        Standing::Clean => 1,
        Standing::Flagged { stage, .. } => match level.get() {
            0..=149 => match stage {
                PkStage::Warning => 5,
                PkStage::FirstStage => 6,
                PkStage::SecondStage => 7,
            },
            150..=219 => match stage {
                PkStage::Warning => 4,
                PkStage::FirstStage => 5,
                PkStage::SecondStage => 6,
            },
            220.. => match stage {
                PkStage::Warning => 3,
                PkStage::FirstStage => 4,
                PkStage::SecondStage => 5,
            },
        },
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

    /// A clean reputation flagged at `stage` (deadline irrelevant to the
    /// exp-loss fork, which reads only the stage and the level band).
    fn flagged_rep(stage: PkStage) -> Reputation {
        Reputation::clean().with_standing(Standing::Flagged {
            stage,
            decays_at: Tick(1),
        })
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
        assert_eq!(RESPAWN_DELAY_MS, DurationMs(3_000));
    }

    #[test]
    fn combat_death_penalty_waives_for_a_player_killer_applies_for_a_monster() {
        // The waiver is decided by the attacker's core-stamped kind, so a host
        // can neither forge "a player kill is free" nor dock a player's victim.
        assert!(matches!(
            combat_death_penalty(TargetKind::Player),
            DeathPenalty::Waived
        ));
        assert!(matches!(
            combat_death_penalty(TargetKind::Npc),
            DeathPenalty::Applied
        ));
    }

    #[test]
    fn monster_death_exp_loss_scales_by_stage_and_never_de_levels() {
        // Clean stays the classic one percent — byte-identical to pre-W-PK.
        let clean = exp_loss_percent(level(100), Reputation::clean());
        assert_eq!(clean, 1);

        // Every flagged rung is heavier, climbing with the stage, at a sub-150
        // level (the harshest band).
        assert_eq!(
            exp_loss_percent(level(100), flagged_rep(PkStage::Warning)),
            5
        );
        assert_eq!(
            exp_loss_percent(level(100), flagged_rep(PkStage::FirstStage)),
            6
        );
        assert_eq!(
            exp_loss_percent(level(100), flagged_rep(PkStage::SecondStage)),
            7
        );
        assert!(clean < exp_loss_percent(level(100), flagged_rep(PkStage::Warning)));

        // The floor never de-levels: seated exactly at the level's experience
        // floor, even the heaviest rung's dock cannot fall below it — the same
        // `floor.max(exp - nominal)` clamp `exp_penalty` applies at line 226.
        let floor = 1_000_000u64;
        let band = 200_000u64;
        for stage in [PkStage::Warning, PkStage::FirstStage, PkStage::SecondStage] {
            let percent = exp_loss_percent(level(100), flagged_rep(stage));
            let nominal = scale_ratio_u64(band, percent, nonzero_u64(PERCENT_DENOMINATOR));
            let new_exp = floor.max(floor.saturating_sub(nominal));
            assert_eq!(new_exp, floor, "stage {stage:?} de-leveled below the floor");
        }
    }

    #[test]
    fn exp_loss_percent_is_total_over_bands_and_stages() {
        // Every level band × every rung resolves to exactly one whole percent;
        // clean is always the classic one, in every band.
        let table = [
            (10u16, [5u64, 6, 7]),
            (149, [5, 6, 7]),
            (150, [4, 5, 6]),
            (219, [4, 5, 6]),
            (220, [3, 4, 5]),
            (400, [3, 4, 5]),
        ];
        for (lvl, expected) in table {
            for (stage, want) in [PkStage::Warning, PkStage::FirstStage, PkStage::SecondStage]
                .into_iter()
                .zip(expected)
            {
                assert_eq!(
                    exp_loss_percent(level(lvl), flagged_rep(stage)),
                    want,
                    "level {lvl} stage {stage:?}"
                );
            }
            assert_eq!(exp_loss_percent(level(lvl), Reputation::clean()), 1);
        }
    }
}
