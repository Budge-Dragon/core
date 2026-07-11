//! Map-crossing decisions: the warp command, the warp-menu availability
//! projection, the enter-gate traversal, and the Town Portal Scroll. One
//! concern — moving a character between maps, gated by discovery, level (after
//! the class warp fraction), wings (a Sky destination admits only a winged
//! traveler), and zen — distinct from the in-map locomotion and landing
//! primitives in [`crate::services::movement`], which these heads compose as
//! their arrival tail.
//!
//! One rule, two readers: [`resolve_warp`] and [`warp_menu`] share a single
//! per-entry evaluator; the command rejects on the first unmet requirement in
//! authentic check order (discovery → level → wings → zen), the projection
//! reports the complete unmet set. The fee is charged last and atomically — a
//! failed earlier check, an unseatable landing, and a rejection of any kind
//! never cost money. The wings gate keys off the *destination* environment
//! alone, so leaving a Sky map is never gated. Every arrival funnels through
//! the character's shared arrival writeback, so a crossed map is always
//! discovered.
//!
//! Determinism: [`resolve_warp`], [`traverse_enter_gate`], and
//! [`use_town_portal`] each draw exactly one random word on a successful
//! arrival (the landing pick) and none on any rejection; [`warp_menu`] is a
//! pure query and draws none.

use rand_core::RngCore;

use crate::components::collections::{EmptyCollection, OneOrMore};
use crate::components::inventory::{Cell, Inventory};
use crate::components::life::LifeState;
use crate::components::movement::Wings;
use crate::components::placement::Placement;
use crate::components::spatial::Facing;
use crate::components::units::{DebitOutcome, Level, MapNumber};
use crate::data::atlas::{Atlas, EnterGateView, Landing, WarpView};
use crate::data::classes::WarpRequirement;
use crate::data::item_definitions::ConsumeEffect;
use crate::data::map_definitions::MapEnvironment;
use crate::entities::character::Character;
use crate::events::movement::WarpOutcome;
use crate::events::travel::{
    EnterGateOutcome, TownPortalOutcome, WarpAvailability, WarpEntryStatus, WarpLockReason,
    WarpTravelOutcome,
};
use crate::services::consume::{ConsumeLookup, item_consume_effect};
use crate::services::movement::{resolve_arrival, resolve_spawn_gate_landing};
use crate::services::ratio::{nonzero, scale_ratio};

/// The level bar a character of the given class actually faces at a gate
/// posting `min_level`: the posted value for a `Full` class, the floored
/// fraction (MG/DL 2/3) otherwise. A plain `u16`, not a [`Level`] — the floor
/// of a fraction can be zero, which a 1-based level must never hold.
#[must_use]
pub(crate) fn effective_level_requirement(min_level: Level, requirement: WarpRequirement) -> u16 {
    match requirement {
        WarpRequirement::Full => min_level.get(),
        WarpRequirement::Fraction {
            numerator,
            denominator,
        } => {
            let scaled = scale_ratio(
                u32::from(min_level.get()),
                u32::from(numerator.get()),
                nonzero(u32::from(denominator.get())),
            );
            // The saturating narrow of a proven-small quotient — the
            // `u16::MAX` fallback is a boundary saturation (the ratio.rs
            // idiom), not a masked lookup absence.
            u16::try_from(scaled).unwrap_or(u16::MAX)
        }
    }
}

/// Whether one warp entry is open to a character — the single rule evaluation
/// the command and the projection share. Reasons accumulate in authentic check
/// order: discovery → level → wings → zen.
enum WarpEligibility {
    /// Every requirement is met.
    Qualified,
    /// At least one requirement is unmet.
    Blocked {
        /// Every unmet requirement, in check order.
        reasons: OneOrMore<WarpLockReason>,
    },
}

/// The traversal environment of a destination map, read off the atlas; `None`
/// for a map the atlas does not carry — the landing attempt folds that absence
/// to its own no-walkable answer downstream.
fn destination_env(atlas: &Atlas, map: MapNumber) -> Option<MapEnvironment> {
    atlas
        .map_handle(map)
        .map(|handle| handle.definition().environment)
}

/// Whether the destination's wings gate bars the traveler: only a known Sky
/// destination met by bare shoulders bars entry. An off-atlas destination is
/// not Sky, so the gate passes and the landing folds to its own answer.
fn wings_barred(destination_env: Option<MapEnvironment>, wings: Wings) -> bool {
    matches!(
        (destination_env, wings),
        (Some(MapEnvironment::Sky), Wings::None)
    )
}

/// Evaluates every warp requirement for one entry: target-map discovery, the
/// class-effective level bar, the destination's wings gate, and the flat fee's
/// affordability. Pure — no RNG, no mutation.
fn evaluate_warp(
    character: &Character,
    warp: WarpView<'_>,
    requirement: WarpRequirement,
    atlas: &Atlas,
    wings: Wings,
) -> WarpEligibility {
    let mut reasons = Vec::new();
    if !character.discovered().contains(warp.landing.map) {
        reasons.push(WarpLockReason::NotDiscovered);
    }
    let required = effective_level_requirement(warp.warp.min_level, requirement);
    if character.level().get() < required {
        reasons.push(WarpLockReason::LevelTooLow { required });
    }
    if wings_barred(destination_env(atlas, warp.landing.map), wings) {
        reasons.push(WarpLockReason::CannotFly);
    }
    if character.zen().get() < warp.warp.cost_zen.0 {
        reasons.push(WarpLockReason::InsufficientZen {
            cost: warp.warp.cost_zen,
        });
    }
    match OneOrMore::new(reasons) {
        Err(EmptyCollection) => WarpEligibility::Qualified,
        Ok(reasons) => WarpEligibility::Blocked { reasons },
    }
}

/// A landing attempt's result: the traveler is seatable at a sampled
/// placement, or the target area holds no walkable tile.
enum LandingAttempt {
    /// A walkable tile was sampled — one RNG word drawn.
    Seated {
        /// The sampled arrival placement.
        placement: Placement,
    },
    /// No walkable tile exists in the target area — no RNG drawn.
    NoWalkable,
}

/// Attempts the arrival on a landing: resolves the destination map's grid and
/// environment, then defers to [`resolve_arrival`]. A landing map the atlas
/// does not carry folds to the same no-walkable answer — a target that cannot
/// seat a traveler, whatever the cause.
fn attempt_landing(
    traveler_facing: Facing,
    landing: &Landing,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> LandingAttempt {
    let Some(handle) = atlas.map_handle(landing.map) else {
        return LandingAttempt::NoWalkable;
    };
    match resolve_arrival(
        traveler_facing,
        landing,
        handle.terrain_grid(),
        handle.definition().environment,
        rng,
    ) {
        WarpOutcome::Arrived { placement } => LandingAttempt::Seated { placement },
        WarpOutcome::NoWalkableLanding => LandingAttempt::NoWalkable,
    }
}

/// The warp command: the decision head whose landing tail is
/// [`resolve_arrival`]. Guards aliveness first, evaluates discovery → level →
/// wings → zen, proves the landing seatable, and only then debits the fee — so
/// no rejection of any kind costs money. On arrival the returned character
/// carries the debited wallet, the sampled placement, and the (idempotent)
/// discovery of the target map. Draws exactly one random word on `Arrived`,
/// none otherwise.
#[must_use]
pub fn resolve_warp(
    character: &Character,
    warp: WarpView<'_>,
    atlas: &Atlas,
    wings: Wings,
    rng: &mut impl RngCore,
) -> (Character, WarpTravelOutcome) {
    match character.life() {
        LifeState::Dead { .. } => return (character.clone(), WarpTravelOutcome::NotAlive),
        LifeState::Alive => {}
    }
    let requirement = atlas.classes().record(character.class()).warp_requirement;
    if let WarpEligibility::Blocked { reasons } =
        evaluate_warp(character, warp, requirement, atlas, wings)
    {
        let outcome = match *reasons.first() {
            WarpLockReason::NotDiscovered => WarpTravelOutcome::NotDiscovered,
            WarpLockReason::LevelTooLow { required } => WarpTravelOutcome::LevelTooLow { required },
            WarpLockReason::CannotFly => WarpTravelOutcome::CannotFly,
            WarpLockReason::InsufficientZen { cost } => WarpTravelOutcome::NotEnoughZen {
                required: cost,
                available: character.zen(),
            },
        };
        return (character.clone(), outcome);
    }
    match attempt_landing(character.placement().facing, &warp.landing, atlas, rng) {
        LandingAttempt::NoWalkable => (character.clone(), WarpTravelOutcome::NoWalkableLanding),
        LandingAttempt::Seated { placement } => {
            match character.zen().debit(warp.warp.cost_zen) {
                DebitOutcome::Debited { balance } => (
                    character.arrived_at(placement).with_zen(balance),
                    WarpTravelOutcome::Arrived { placement, balance },
                ),
                // Affordability was proven by the evaluator, so this arm is
                // structurally unreachable; it folds to the same insufficient
                // answer without a suppressor (the zen_penalty fold).
                DebitOutcome::Insufficient { balance } => (
                    character.clone(),
                    WarpTravelOutcome::NotEnoughZen {
                        required: warp.warp.cost_zen,
                        available: balance,
                    },
                ),
            }
        }
    }
}

/// The warp-menu availability projection: one status per warp entry, in
/// warp-index order, each carrying the complete set of unmet requirements. A
/// pure query of `(character, atlas, wings)` — no RNG, no mutation; it shares
/// [`resolve_warp`]'s rule evaluation, so the menu and the command never
/// disagree for a living character. Defined for a dead character too (its
/// per-entry facts are the same); aliveness is the command's whole-action
/// guard, not a per-entry lock.
#[must_use]
pub fn warp_menu(character: &Character, atlas: &Atlas, wings: Wings) -> Vec<WarpEntryStatus> {
    let requirement = atlas.classes().record(character.class()).warp_requirement;
    atlas
        .warps()
        .map(|warp| WarpEntryStatus {
            index: warp.warp.index,
            availability: match evaluate_warp(character, warp, requirement, atlas, wings) {
                WarpEligibility::Qualified => WarpAvailability::Available,
                WarpEligibility::Blocked { reasons } => WarpAvailability::Locked { reasons },
            },
        })
        .collect()
}

/// A physical enter-gate traversal: the world's own doors, gated by the gate's
/// classic level requirement (after the same class fraction) and the
/// destination's wings gate, in that order — never by discovery or zen.
/// Arrival discovers the destination map through the shared writeback, which
/// is how an undiscovered map is reached in the first place. Draws exactly one
/// random word on `Arrived`, none otherwise.
#[must_use]
pub fn traverse_enter_gate(
    character: &Character,
    gate: EnterGateView<'_>,
    atlas: &Atlas,
    wings: Wings,
    rng: &mut impl RngCore,
) -> (Character, EnterGateOutcome) {
    match character.life() {
        LifeState::Dead { .. } => return (character.clone(), EnterGateOutcome::NotAlive),
        LifeState::Alive => {}
    }
    if let Some(min_level) = gate.gate.min_level {
        let requirement = atlas.classes().record(character.class()).warp_requirement;
        let required = effective_level_requirement(min_level, requirement);
        if character.level().get() < required {
            return (
                character.clone(),
                EnterGateOutcome::LevelTooLow { required },
            );
        }
    }
    if wings_barred(destination_env(atlas, gate.landing.map), wings) {
        return (character.clone(), EnterGateOutcome::CannotFly);
    }
    match attempt_landing(character.placement().facing, &gate.landing, atlas, rng) {
        LandingAttempt::NoWalkable => (character.clone(), EnterGateOutcome::NoWalkableLanding),
        LandingAttempt::Seated { placement } => (
            character.arrived_at(placement),
            EnterGateOutcome::Arrived { placement },
        ),
    }
}

/// Reads a Town Portal Scroll: a travel service, not a recovery consume — the
/// [`crate::services::consume::use_consumable`] head permanently refuses the
/// scroll `NotRecoverable`; this is its owning service. Guards aliveness,
/// proves the addressed cell holds a town-portal scroll (the shared
/// item-identity lookup), consumes exactly one piece, and seats the traveler
/// at the current map's town gate — the same per-map destination the death
/// respawn uses — alive, vitals and effects untouched, the town discovered if
/// new. Nothing is consumed on any rejection. The town-gate arrival is total
/// (the gate's walkable set is parse-proven non-empty), so no landing failure
/// is representable. Draws exactly one random word on `Arrived`, none
/// otherwise.
#[must_use]
pub fn use_town_portal(
    character: &Character,
    inventory: Inventory,
    cell: Cell,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Character, Inventory, TownPortalOutcome) {
    match character.life() {
        LifeState::Dead { .. } => {
            return (character.clone(), inventory, TownPortalOutcome::NotAlive);
        }
        LifeState::Alive => {}
    }
    match item_consume_effect(&inventory, cell, atlas) {
        ConsumeLookup::Effect(ConsumeEffect::TownPortal) => {}
        ConsumeLookup::Empty
        | ConsumeLookup::NotConsumable
        | ConsumeLookup::Effect(
            ConsumeEffect::Healing { .. }
            | ConsumeEffect::Mana { .. }
            | ConsumeEffect::Antidote
            | ConsumeEffect::Alcohol,
        ) => return (character.clone(), inventory, TownPortalOutcome::NoScroll),
    }
    let inventory = match inventory.consume_one(cell) {
        Ok(consumed) => consumed,
        // The lookup above proved the cell covered, so this arm is
        // structurally unreachable; it folds to the same no-scroll answer with
        // the inventory handed back whole (the consume-commit fold).
        Err((whole, _reason)) => {
            return (character.clone(), whole, TownPortalOutcome::NoScroll);
        }
    };
    let (gate, env) = match atlas.town_gate_for_map(character.placement().map) {
        Some(destination) => destination,
        None => atlas.fallback_town_gate(),
    };
    let placement = resolve_spawn_gate_landing(gate, env, rng);
    (
        character.arrived_at(placement),
        inventory,
        TownPortalOutcome::Arrived { placement },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::num::NonZeroU16;

    fn fraction(numerator: u16, denominator: u16) -> WarpRequirement {
        WarpRequirement::Fraction {
            numerator: NonZeroU16::new(numerator).unwrap(),
            denominator: NonZeroU16::new(denominator).unwrap(),
        }
    }

    #[test]
    fn effective_requirement_passes_the_posted_level_through_for_full() {
        assert_eq!(
            effective_level_requirement(Level::new(50).unwrap(), WarpRequirement::Full),
            50
        );
        assert_eq!(
            effective_level_requirement(Level::new(10).unwrap(), WarpRequirement::Full),
            10
        );
    }

    #[test]
    fn effective_requirement_floors_the_fraction() {
        assert_eq!(
            effective_level_requirement(Level::new(50).unwrap(), fraction(2, 3)),
            33
        );
        assert_eq!(
            effective_level_requirement(Level::new(20).unwrap(), fraction(2, 3)),
            13
        );
        assert_eq!(
            effective_level_requirement(Level::new(10).unwrap(), fraction(2, 3)),
            6
        );
    }

    #[test]
    fn effective_requirement_saturates_an_over_wide_fraction() {
        // A numerator far above the denominator can push the scaled value past
        // u16 — the narrow saturates rather than truncating.
        assert_eq!(
            effective_level_requirement(Level::new(u16::MAX).unwrap(), fraction(u16::MAX, 1)),
            u16::MAX
        );
    }

    #[test]
    fn wings_bar_only_a_known_sky_destination_met_by_bare_shoulders() {
        assert!(wings_barred(Some(MapEnvironment::Sky), Wings::None));
        assert!(!wings_barred(Some(MapEnvironment::Sky), Wings::Equipped));
        assert!(!wings_barred(Some(MapEnvironment::Ground), Wings::None));
        assert!(!wings_barred(Some(MapEnvironment::Underwater), Wings::None));
    }

    #[test]
    fn an_off_atlas_destination_is_never_wings_gated() {
        // Absence folds to not-Sky: the wings gate passes and the landing
        // attempt owns the off-atlas answer (NoWalkableLanding).
        assert!(!wings_barred(None, Wings::None));
        assert!(!wings_barred(None, Wings::Equipped));
    }
}
