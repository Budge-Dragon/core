//! The mini-game entry gate: one ordered admission check over a typed intent
//! — level bracket (the reduced bracket for the special event classes),
//! exact-ticket possession, the zen fee, the player-killer bar, the open
//! entry window, and seat capacity — with one typed rejection per reason and
//! nothing spent on any rejection. Admission applies the side effects in
//! order: the fee is debited, one ticket charge is consumed (the whole item
//! at its last charge), every active magic effect is cleared, and the entrant
//! lands on the parse-proven entrance landing set — the single random word
//! this service draws.

use rand_core::RngCore;
use serde::{Deserialize, Serialize};

use crate::components::active_effect::ActiveEffects;
use crate::components::class::CharacterClass;
use crate::components::inventory::{Cell, Inventory};
use crate::components::movement::Movement;
use crate::components::placement::Placement;
use crate::components::spatial::Facing;
use crate::components::units::DebitOutcome;
use crate::data::atlas::MiniGameHandle;
use crate::data::minigame::{RosterSlot, RosterStatus, Score, TicketRequirement};
use crate::entities::character::Character;
use crate::entities::minigame_session::{MiniGamePhase, MiniGameSession, RosterMember};
use crate::services::chance::pick_one;

/// The heading an admitted entrant arrives with — the entrance landing pick
/// is the single random word, so the facing is this fixed default rather than
/// a second draw.
const ENTRANCE_FACING: Facing = Facing::POS_Y;

/// The host-supplied player-killer standing — a bare fact, never a pre-baked
/// admit/barred verdict: core reads the standing, core decides the bar.
/// Transient service input, not persisted state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkStanding {
    /// Below the entry bar (a kill warning may still enter).
    Clear,
    /// A flagged player-killer at or above the bar.
    PlayerKiller,
}

/// What an entry attempt produced, kind-tagged: admission with its seat and
/// landing, or the first failing check's rejection. On any rejection the
/// session, entrant, and bag are returned unchanged — nothing spent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EnterOutcome {
    /// Admitted; the side effects are applied (fee, ticket, effects, warp).
    Entered {
        /// The seat the entrant took.
        slot: RosterSlot,
        /// Where the entrant landed.
        placement: Placement,
    },
    /// Live level below the chosen bracket's minimum.
    LevelTooLow,
    /// Live level above the chosen bracket's maximum.
    LevelTooHigh,
    /// No valid ticket: the exact item, at the exact plus-level, with at
    /// least one charge left.
    NoTicket,
    /// Carried zen below the fee (checked, not spent).
    NotEnoughZen,
    /// Player-killer standing at or above the bar.
    PlayerKillerBarred,
    /// The phase is not `Open` — no re-entry once the window closes.
    NotOpen,
    /// The roster (dead included) is at capacity.
    Full,
}

/// Admits an entrant into an open session, or rejects with the first failing
/// check in the fixed order: bracket (by class), ticket, zen fee, PK bar,
/// phase, capacity. On admission the fee is debited, one ticket charge is
/// consumed (the whole item at its last charge), every active effect is
/// cleared, the entrant is seated on the entrance landing (one uniform pick —
/// the only random word), and the roster gains the lowest free seat, alive at
/// score zero. On any rejection everything comes back unchanged.
#[must_use]
pub fn enter_mini_game(
    session: MiniGameSession,
    handle: &MiniGameHandle<'_>,
    entrant: Character,
    bag: Inventory,
    pk: PkStanding,
    rng: &mut impl RngCore,
) -> (MiniGameSession, Character, Inventory, EnterOutcome) {
    let def = handle.definition;
    let bracket = if uses_special_bracket(entrant.class()) {
        def.special_bracket
    } else {
        def.normal_bracket
    };
    let level = entrant.level();
    if level < bracket.min() {
        return reject(session, entrant, bag, EnterOutcome::LevelTooLow);
    }
    if level > bracket.max() {
        return reject(session, entrant, bag, EnterOutcome::LevelTooHigh);
    }
    let Some(ticket_anchor) = find_ticket(&bag, def.ticket) else {
        return reject(session, entrant, bag, EnterOutcome::NoTicket);
    };
    if entrant.zen().get() < def.entrance_fee.0 {
        return reject(session, entrant, bag, EnterOutcome::NotEnoughZen);
    }
    if let PkStanding::PlayerKiller = pk {
        return reject(session, entrant, bag, EnterOutcome::PlayerKillerBarred);
    }
    let MiniGamePhase::Open { .. } = session.phase else {
        return reject(session, entrant, bag, EnterOutcome::NotOpen);
    };
    let Some(slot) = session.lowest_free_slot(def.players.max()) else {
        return reject(session, entrant, bag, EnterOutcome::Full);
    };

    // Every check passed — only now spending, in order: fee, ticket, effects,
    // warp. The two fallible seams below were proven covered by the checks
    // above, so their refusal arms fold back to the matching rejection with
    // nothing spent rather than a suppressor.
    let DebitOutcome::Debited { balance } = entrant.zen().debit(def.entrance_fee) else {
        return reject(session, entrant, bag, EnterOutcome::NotEnoughZen);
    };
    let bag = match bag.consume_one(ticket_anchor) {
        Ok(consumed) => consumed,
        Err((bag, _reason)) => return reject(session, entrant, bag, EnterOutcome::NoTicket),
    };
    let placement = entrance_landing(handle, rng);
    let admitted = entrant
        .with_zen(balance)
        .with_effects(ActiveEffects::EMPTY)
        .arrived_at(placement);
    let session = session.with_member(RosterMember {
        slot,
        status: RosterStatus::Alive,
        score: Score(0),
    });
    (
        session,
        admitted,
        bag,
        EnterOutcome::Entered { slot, placement },
    )
}

/// Whether `class` is gated by a definition's *special* (reduced) level
/// bracket — true for Magic Gladiator and Dark Lord and no other class this
/// era. A total match, so a new class breaks the build until answered.
fn uses_special_bracket(class: CharacterClass) -> bool {
    match class {
        CharacterClass::MagicGladiator | CharacterClass::DarkLord => true,
        CharacterClass::DarkWizard
        | CharacterClass::SoulMaster
        | CharacterClass::DarkKnight
        | CharacterClass::BladeKnight
        | CharacterClass::FairyElf
        | CharacterClass::MuseElf => false,
    }
}

/// The anchor of the first bag item satisfying the ticket requirement: the
/// exact item, the exact plus-level, at least one charge left. Scans the
/// bag's public placed-items surface only.
fn find_ticket(bag: &Inventory, ticket: TicketRequirement) -> Option<Cell> {
    bag.placed()
        .iter()
        .find(|placed| {
            placed.item.item == ticket.item
                && placed.item.level == ticket.item_level
                && placed.item.durability.current() > 0
        })
        .map(|placed| placed.anchor)
}

/// Seats the admitted entrant over the parse-proven entrance landing set: one
/// uniform pick — the single entry random word — facing the fixed default,
/// grounded on the event map.
fn entrance_landing(handle: &MiniGameHandle<'_>, rng: &mut impl RngCore) -> Placement {
    Placement {
        position: *pick_one(handle.entrance_landing, rng),
        facing: ENTRANCE_FACING,
        movement: Movement::Grounded,
        map: handle.definition.entrance.map,
    }
}

/// A rejection: the session, entrant, and bag handed back unchanged with the
/// failing check's outcome — the entrant returned by move, never cloned.
fn reject(
    session: MiniGameSession,
    entrant: Character,
    bag: Inventory,
    outcome: EnterOutcome,
) -> (MiniGameSession, Character, Inventory, EnterOutcome) {
    (session, entrant, bag, outcome)
}

#[cfg(test)]
mod tests {
    use super::super::support::{
        CountingRng, TestRng, character, character_with_effect, fixture, open_session,
        place_ticket, ticket_instance,
    };
    use super::*;
    use crate::entities::minigame_session::MiniGamePhase;

    fn admit_ready() -> (MiniGameSession, Character, Inventory) {
        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 60, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        (session, entrant, bag)
    }

    #[test]
    fn a_passing_entrant_pays_once_and_is_seated_alive_at_score_zero() {
        let holder = fixture();
        let handle = holder.handle();
        let (session, entrant, bag) = admit_ready();
        let mut rng = TestRng::new(7);
        let (session, admitted, bag, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        let EnterOutcome::Entered { slot, placement } = outcome else {
            panic!("expected admission, got {outcome:?}");
        };
        assert_eq!(slot, RosterSlot(0));
        // Fee debited exactly once: 100,000 - 25,000.
        assert_eq!(admitted.zen().get(), 75_000);
        // The multi-durability ticket survives with one charge consumed.
        assert_eq!(bag.placed().len(), 1);
        assert_eq!(bag.placed()[0].item.durability.current(), 1);
        // Effects cleared.
        assert_eq!(admitted.active_effects(), ActiveEffects::EMPTY);
        // Landed on the entrance map, inside the landing set.
        assert_eq!(placement.map, handle.definition.entrance.map);
        assert!(
            handle
                .entrance_landing
                .iter()
                .any(|at| *at == placement.position)
        );
        assert_eq!(placement.facing, ENTRANCE_FACING);
        assert_eq!(admitted.placement(), placement);
        // Seated alive at score zero.
        let member = session.member(RosterSlot(0)).unwrap();
        assert_eq!(member.status, RosterStatus::Alive);
        assert_eq!(member.score, Score(0));
    }

    #[test]
    fn a_ticket_at_its_last_charge_is_consumed_whole() {
        let holder = fixture();
        let handle = holder.handle();
        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 60, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(1));
        let mut rng = TestRng::new(7);
        let (_, admitted, bag, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert!(matches!(outcome, EnterOutcome::Entered { .. }));
        assert!(bag.placed().is_empty());
        assert_eq!(admitted.zen().get(), 75_000);
    }

    #[test]
    fn bracket_edges_are_inclusive_and_one_past_each_edge_rejects() {
        let holder = fixture();
        let handle = holder.handle();
        for (level, admitted) in [(15, true), (130, true)] {
            let session = open_session();
            let entrant = character(CharacterClass::DarkKnight, level, 100_000);
            let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
            let mut rng = TestRng::new(7);
            let (_, _, _, outcome) =
                enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
            assert_eq!(matches!(outcome, EnterOutcome::Entered { .. }), admitted);
        }
        for (level, expected) in [
            (14, EnterOutcome::LevelTooLow),
            (131, EnterOutcome::LevelTooHigh),
        ] {
            let session = open_session();
            let entrant = character(CharacterClass::DarkKnight, level, 100_000);
            let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
            let mut rng = TestRng::new(7);
            let (_, _, _, outcome) =
                enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
            assert_eq!(outcome, expected);
        }
    }

    #[test]
    fn the_special_classes_are_gated_by_the_special_bracket() {
        let holder = fixture();
        let handle = holder.handle();
        // 112 fits the normal 15..130 but exceeds the special 10..110.
        for class in [CharacterClass::MagicGladiator, CharacterClass::DarkLord] {
            let session = open_session();
            let entrant = character(class, 112, 100_000);
            let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
            let mut rng = TestRng::new(7);
            let (_, _, _, outcome) =
                enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
            assert_eq!(outcome, EnterOutcome::LevelTooHigh, "{class:?}");
        }
        // The same level on a standard class is admitted by the normal bracket.
        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 112, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert!(matches!(outcome, EnterOutcome::Entered { .. }));
        // And a special-class entrant inside its reduced bracket enters: 12
        // sits below the normal minimum of 15 but inside the special 10..110.
        let session = open_session();
        let entrant = character(CharacterClass::MagicGladiator, 12, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert!(matches!(outcome, EnterOutcome::Entered { .. }));
    }

    #[test]
    fn a_wrong_level_zero_charge_or_absent_ticket_rejects_with_nothing_spent() {
        let holder = fixture();
        let handle = holder.handle();
        let wrong_level = {
            let mut instance = ticket_instance(2);
            instance.level = crate::components::units::ItemLevel::new(2).unwrap();
            instance
        };
        let empty_gauge = {
            let mut instance = ticket_instance(1);
            instance.durability = crate::components::item_instance::Durability::new(0, 5).unwrap();
            instance
        };
        let wrong_item = {
            let mut instance = ticket_instance(2);
            instance.item = crate::data::common::ItemRef {
                group: 14,
                number: 13,
            };
            instance
        };
        for instance in [wrong_level, empty_gauge, wrong_item] {
            let session = open_session();
            let entrant = character(CharacterClass::DarkKnight, 60, 100_000);
            let bag = place_ticket(Inventory::empty(8, 8), instance);
            let before = bag.clone();
            let mut rng = TestRng::new(7);
            let (session_out, entrant_out, bag_out, outcome) = enter_mini_game(
                session.clone(),
                &handle,
                entrant.clone(),
                bag,
                PkStanding::Clear,
                &mut rng,
            );
            assert_eq!(outcome, EnterOutcome::NoTicket);
            assert_eq!(session_out, session);
            assert_eq!(entrant_out, entrant);
            assert_eq!(bag_out, before);
        }
    }

    #[test]
    fn an_unaffordable_fee_is_checked_never_partially_charged() {
        let holder = fixture();
        let handle = holder.handle();
        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 60, 20_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let before = bag.clone();
        let mut rng = TestRng::new(7);
        let (_, entrant_out, bag_out, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert_eq!(outcome, EnterOutcome::NotEnoughZen);
        assert_eq!(entrant_out.zen().get(), 20_000);
        assert_eq!(bag_out, before);
    }

    #[test]
    fn a_player_killer_is_barred_on_the_host_supplied_standing() {
        let holder = fixture();
        let handle = holder.handle();
        let (session, entrant, bag) = admit_ready();
        let mut rng = TestRng::new(7);
        let (_, entrant_out, _, outcome) = enter_mini_game(
            session,
            &handle,
            entrant.clone(),
            bag,
            PkStanding::PlayerKiller,
            &mut rng,
        );
        assert_eq!(outcome, EnterOutcome::PlayerKillerBarred);
        assert_eq!(entrant_out, entrant);
    }

    #[test]
    fn a_session_past_open_rejects_with_not_open() {
        let holder = fixture();
        let handle = holder.handle();
        let (session, entrant, bag) = admit_ready();
        let session = session.with_phase(MiniGamePhase::Closing {
            starts_at: crate::components::units::Tick(400),
        });
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert_eq!(outcome, EnterOutcome::NotOpen);
    }

    #[test]
    fn capacity_counts_every_entrant_dead_included() {
        let holder = fixture();
        let handle = holder.handle();
        // The fixture definition seats at most 3.
        let mut session = open_session();
        for (seat, status) in [
            (0, RosterStatus::Alive),
            (1, RosterStatus::Dead),
            (2, RosterStatus::Alive),
        ] {
            session = session.with_member(RosterMember {
                slot: RosterSlot(seat),
                status,
                score: Score(0),
            });
        }
        let entrant = character(CharacterClass::DarkKnight, 60, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert_eq!(outcome, EnterOutcome::Full);
    }

    #[test]
    fn the_check_order_reports_the_first_failure_of_a_double_failure() {
        let holder = fixture();
        let handle = holder.handle();
        // Fails BOTH the bracket and the ticket scan: the bracket wins.
        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 14, 100_000);
        let bag = Inventory::empty(8, 8);
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert_eq!(outcome, EnterOutcome::LevelTooLow);
        // Fails BOTH the ticket and the fee: the ticket wins.
        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 60, 0);
        let bag = Inventory::empty(8, 8);
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert_eq!(outcome, EnterOutcome::NoTicket);
    }

    #[test]
    fn admission_clears_a_live_effect() {
        let holder = fixture();
        let handle = holder.handle();
        let session = open_session();
        let entrant = character_with_effect(CharacterClass::DarkKnight, 60, 100_000);
        assert_ne!(entrant.active_effects(), ActiveEffects::EMPTY);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let mut rng = TestRng::new(7);
        let (_, admitted, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert!(matches!(outcome, EnterOutcome::Entered { .. }));
        assert_eq!(admitted.active_effects(), ActiveEffects::EMPTY);
    }

    #[test]
    fn entry_draws_exactly_one_random_word_on_success_and_none_on_rejection() {
        let holder = fixture();
        let handle = holder.handle();
        let (session, entrant, bag) = admit_ready();
        let mut rng = CountingRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert!(matches!(outcome, EnterOutcome::Entered { .. }));
        assert_eq!(rng.draws(), 1);

        let session = open_session();
        let entrant = character(CharacterClass::DarkKnight, 14, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let mut rng = CountingRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        assert_eq!(outcome, EnterOutcome::LevelTooLow);
        assert_eq!(rng.draws(), 0);
    }

    #[test]
    fn admission_takes_the_lowest_free_seat_after_a_leave() {
        let holder = fixture();
        let handle = holder.handle();
        let session = open_session()
            .with_member(RosterMember {
                slot: RosterSlot(0),
                status: RosterStatus::Alive,
                score: Score(0),
            })
            .with_member(RosterMember {
                slot: RosterSlot(2),
                status: RosterStatus::Alive,
                score: Score(0),
            });
        let entrant = character(CharacterClass::DarkKnight, 60, 100_000);
        let bag = place_ticket(Inventory::empty(8, 8), ticket_instance(2));
        let mut rng = TestRng::new(7);
        let (_, _, _, outcome) =
            enter_mini_game(session, &handle, entrant, bag, PkStanding::Clear, &mut rng);
        let EnterOutcome::Entered { slot, .. } = outcome else {
            panic!("expected admission, got {outcome:?}");
        };
        assert_eq!(slot, RosterSlot(1));
    }

    #[test]
    fn enter_outcome_wire_forms_are_kind_tagged() {
        assert_eq!(
            serde_json::to_string(&EnterOutcome::NoTicket).unwrap(),
            r#"{"kind":"no_ticket"}"#
        );
        assert_eq!(
            serde_json::to_string(&EnterOutcome::PlayerKillerBarred).unwrap(),
            r#"{"kind":"player_killer_barred"}"#
        );
    }
}
