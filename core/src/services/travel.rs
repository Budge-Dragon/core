//! Map-crossing decisions: the warp command, the warp-menu availability
//! projection, the enter-gate traversal, and the Town Portal Scroll. One
//! concern — moving a character between maps, gated by discovery, level (after
//! the class warp fraction), and zen — distinct from the in-map locomotion and
//! landing primitives in [`crate::services::movement`], which these heads
//! compose as their arrival tail.
//!
//! One rule, two readers: [`resolve_warp`] and [`warp_menu`] share a single
//! per-entry evaluator; the command rejects on the first unmet requirement in
//! authentic check order (discovery → level → zen), the projection reports the
//! complete unmet set. The fee is charged last and atomically — a failed
//! earlier check, an unseatable landing, and a rejection of any kind never
//! cost money. Every arrival funnels through the character's shared
//! arrival writeback, so a crossed map is always discovered.
//!
//! Determinism: [`resolve_warp`], [`traverse_enter_gate`], and
//! [`use_town_portal`] each draw exactly one random word on a successful
//! arrival (the landing pick) and none on any rejection; [`warp_menu`] is a
//! pure query and draws none.

use rand_core::RngCore;

use crate::components::collections::{EmptyCollection, OneOrMore};
use crate::components::inventory::{Cell, Inventory};
use crate::components::life::LifeState;
use crate::components::placement::Placement;
use crate::components::spatial::Facing;
use crate::components::units::{DebitOutcome, Level};
use crate::data::atlas::{Atlas, EnterGateView, Landing, WarpView};
use crate::data::classes::WarpRequirement;
use crate::data::item_definitions::ConsumeEffect;
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
pub fn effective_level_requirement(min_level: Level, requirement: WarpRequirement) -> u16 {
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
/// order: discovery → level → zen.
enum WarpEligibility {
    /// Every requirement is met.
    Qualified,
    /// At least one requirement is unmet.
    Blocked {
        /// Every unmet requirement, in check order.
        reasons: OneOrMore<WarpLockReason>,
    },
}

/// Evaluates every warp requirement for one entry: target-map discovery, the
/// class-effective level bar, and the flat fee's affordability. Pure — no RNG,
/// no mutation.
fn evaluate_warp(
    character: &Character,
    warp: WarpView<'_>,
    requirement: WarpRequirement,
) -> WarpEligibility {
    let mut reasons = Vec::new();
    if !character.discovered().contains(warp.landing.map) {
        reasons.push(WarpLockReason::NotDiscovered);
    }
    let required = effective_level_requirement(warp.warp.min_level, requirement);
    if character.level().get() < required {
        reasons.push(WarpLockReason::LevelTooLow { required });
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
        handle.walk_grid(),
        handle.definition().environment,
        rng,
    ) {
        WarpOutcome::Arrived { placement } => LandingAttempt::Seated { placement },
        WarpOutcome::NoWalkableLanding => LandingAttempt::NoWalkable,
    }
}

/// The warp command: the decision head whose landing tail is
/// [`resolve_arrival`]. Guards aliveness first, evaluates discovery → level →
/// zen, proves the landing seatable, and only then debits the fee — so no
/// rejection of any kind costs money. On arrival the returned character
/// carries the debited wallet, the sampled placement, and the (idempotent)
/// discovery of the target map. Draws exactly one random word on `Arrived`,
/// none otherwise.
#[must_use]
pub fn resolve_warp(
    character: &Character,
    warp: WarpView<'_>,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Character, WarpTravelOutcome) {
    match character.life() {
        LifeState::Dead { .. } => return (character.clone(), WarpTravelOutcome::NotAlive),
        LifeState::Alive => {}
    }
    let requirement = atlas.classes().record(character.class()).warp_requirement;
    if let WarpEligibility::Blocked { reasons } = evaluate_warp(character, warp, requirement) {
        let outcome = match *reasons.first() {
            WarpLockReason::NotDiscovered => WarpTravelOutcome::NotDiscovered,
            WarpLockReason::LevelTooLow { required } => WarpTravelOutcome::LevelTooLow { required },
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
/// pure query of `(character, atlas)` — no RNG, no mutation; it shares
/// [`resolve_warp`]'s rule evaluation, so the menu and the command never
/// disagree for a living character. Defined for a dead character too (its
/// per-entry facts are the same); aliveness is the command's whole-action
/// guard, not a per-entry lock.
#[must_use]
pub fn warp_menu(character: &Character, atlas: &Atlas) -> Vec<WarpEntryStatus> {
    let requirement = atlas.classes().record(character.class()).warp_requirement;
    atlas
        .warps()
        .map(|warp| WarpEntryStatus {
            index: warp.warp.index,
            availability: match evaluate_warp(character, warp, requirement) {
                WarpEligibility::Qualified => WarpAvailability::Available,
                WarpEligibility::Blocked { reasons } => WarpAvailability::Locked { reasons },
            },
        })
        .collect()
}

/// A physical enter-gate traversal: the world's own doors, gated by the gate's
/// classic level requirement alone (after the same class fraction) — never by
/// discovery or zen. Arrival discovers the destination map through the shared
/// writeback, which is how an undiscovered map is reached in the first place.
/// Draws exactly one random word on `Arrived`, none otherwise.
#[must_use]
pub fn traverse_enter_gate(
    character: &Character,
    gate: EnterGateView<'_>,
    atlas: &Atlas,
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

    use crate::components::units::Zen;
    use crate::data::common::{GateNumber, Provenance, SourceVersion};
    use crate::data::gates_warps::{Warp, WarpIndex};

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

    fn character(level: u16, zen: u64, discovered: &serde_json::Value) -> Character {
        serde_json::from_value(serde_json::json!({
            "class": "dark_knight",
            "level": level,
            "experience": 0,
            "stats": {"kind": "standard", "strength": 60, "agility": 40, "vitality": 50, "energy": 30},
            "unspent_points": 0,
            "zen": zen,
            "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
            "vitals": {
                "health": {"current": 100, "max": 100},
                "mana": {"current": 50, "max": 50},
                "ability": {"current": 1, "max": 1}
            },
            "discovered": discovered.clone(),
        }))
        .unwrap()
    }

    fn warp_record(cost: u64, min_level: u16) -> Warp {
        Warp {
            index: WarpIndex(8),
            cost_zen: Zen(cost),
            min_level: Level::new(min_level).unwrap(),
            target_gate: GateNumber(42),
            provenance: Provenance {
                source_version: SourceVersion::V075,
                review: None,
            },
        }
    }

    fn view(warp: &Warp, map: u8) -> WarpView<'_> {
        use crate::components::tile::TileArea;
        use crate::components::units::MapNumber;
        WarpView {
            warp,
            landing: Landing {
                map: MapNumber(map),
                area: TileArea::new(10, 10, 20, 20).unwrap().to_world(),
                facing: None,
            },
        }
    }

    #[test]
    fn the_evaluator_accumulates_every_unmet_reason_in_check_order() {
        let hero = character(15, 1_000, &serde_json::json!([0]));
        let warp = warp_record(5_000, 50);
        match evaluate_warp(&hero, view(&warp, 4), WarpRequirement::Full) {
            WarpEligibility::Blocked { reasons } => {
                let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
                assert_eq!(
                    listed,
                    vec![
                        WarpLockReason::NotDiscovered,
                        WarpLockReason::LevelTooLow { required: 50 },
                        WarpLockReason::InsufficientZen { cost: Zen(5_000) },
                    ]
                );
            }
            WarpEligibility::Qualified => panic!("a triply-failing entry is blocked"),
        }
    }

    #[test]
    fn the_evaluator_qualifies_a_met_entry_and_lists_a_single_failure_alone() {
        let warp = warp_record(5_000, 50);
        // Fully qualified: discovered, level met, affordable.
        let qualified = character(60, 10_000, &serde_json::json!([0, 4]));
        assert!(matches!(
            evaluate_warp(&qualified, view(&warp, 4), WarpRequirement::Full),
            WarpEligibility::Qualified
        ));
        // Only the wallet fails.
        let poor = character(60, 4_999, &serde_json::json!([0, 4]));
        match evaluate_warp(&poor, view(&warp, 4), WarpRequirement::Full) {
            WarpEligibility::Blocked { reasons } => {
                let listed: Vec<WarpLockReason> = reasons.iter().copied().collect();
                assert_eq!(
                    listed,
                    vec![WarpLockReason::InsufficientZen { cost: Zen(5_000) }]
                );
            }
            WarpEligibility::Qualified => panic!("an unaffordable entry is blocked"),
        }
    }

    #[test]
    fn the_evaluator_carries_the_class_effective_requirement() {
        let hero = character(40, 10_000, &serde_json::json!([0, 4]));
        let warp = warp_record(5_000, 50);
        // A 2/3 class faces 33, which a level-40 clears.
        assert!(matches!(
            evaluate_warp(&hero, view(&warp, 4), fraction(2, 3)),
            WarpEligibility::Qualified
        ));
        // A Full class at the same level faces the posted 50 and fails.
        match evaluate_warp(&hero, view(&warp, 4), WarpRequirement::Full) {
            WarpEligibility::Blocked { reasons } => {
                assert_eq!(
                    *reasons.first(),
                    WarpLockReason::LevelTooLow { required: 50 }
                );
            }
            WarpEligibility::Qualified => panic!("the full requirement blocks level 40"),
        }
        // Below even the fraction, the reject carries the effective 33.
        let low = character(30, 10_000, &serde_json::json!([0, 4]));
        match evaluate_warp(&low, view(&warp, 4), fraction(2, 3)) {
            WarpEligibility::Blocked { reasons } => {
                assert_eq!(
                    *reasons.first(),
                    WarpLockReason::LevelTooLow { required: 33 }
                );
            }
            WarpEligibility::Qualified => panic!("level 30 misses the effective 33"),
        }
    }
}
