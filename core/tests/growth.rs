//! The leveling writeback (W-GROW) over the real `/data` Atlas: the core
//! [`apply_experience`] service applied directly against the shipped experience
//! curve and class table. Proves the single-level and multi-level point grants,
//! the per-class 5-vs-7 split, the vitals refill on a crossing, the two cap
//! behaviors (discard at the cap, land-on-cap with leftover), purity and
//! determinism, and that the party-split award routes through the very same rule.
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body so
//! `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use mu_core::components::class::CharacterClass;
use mu_core::components::party::{MemberSlot, Vitality};
use mu_core::components::pool::Pool;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{Exp, Level, MapNumber};
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::entities::party_session::PartySession;
use mu_core::events::progression::GrowthEvent;
use mu_core::services::experience::apply_experience;
use mu_core::services::party::{MemberFact, distribute_kill_experience};
use mu_core::services::profile::character_profile;

use dataset::{or_abort, real_atlas};
use rng::TestRng;

fn level(value: u16) -> Level {
    or_abort(Level::new(value))
}

/// Total experience the curve requires to hold `lvl`, read from the real table.
fn total_to_hold(atlas: &Atlas, lvl: u16) -> u64 {
    or_abort(atlas.exp_curve().level(lvl)).total_to_hold().0
}

/// The four classic trainable stats as a wire value.
fn standard_stats(strength: u16, agility: u16, vitality: u16, energy: u16) -> serde_json::Value {
    serde_json::json!({
        "kind": "standard",
        "strength": strength,
        "agility": agility,
        "vitality": vitality,
        "energy": energy,
    })
}

/// The four classic stats plus Command (Dark Lord line) as a wire value.
fn command_stats(
    strength: u16,
    agility: u16,
    vitality: u16,
    energy: u16,
    command: u16,
) -> serde_json::Value {
    serde_json::json!({
        "kind": "with_command",
        "strength": strength,
        "agility": agility,
        "vitality": vitality,
        "energy": energy,
        "command": command,
    })
}

/// A full-at-`max` vitals block as a wire value.
fn full_vitals(health: u32, mana: u32, ability: u32) -> serde_json::Value {
    serde_json::json!({
        "health": {"current": health, "max": health},
        "mana": {"current": mana, "max": mana},
        "ability": {"current": ability, "max": ability},
    })
}

/// A gearless character of `class`/`stats` at `lvl`/`exp`, holding `unspent`
/// banked points and the given vitals — built the only way a character can be, by
/// deserialising its wire form (the class↔stats gate re-proves on load).
fn character(
    class: &str,
    stats: &serde_json::Value,
    lvl: u16,
    exp: u64,
    unspent: u16,
    vitals: &serde_json::Value,
) -> Character {
    let json = serde_json::json!({
        "class": class,
        "level": lvl,
        "experience": exp,
        "stats": stats,
        "unspent_points": unspent,
        "zen": 0,
        "placement": {
            "position": {"x": 0, "y": 0},
            "facing": {"x": 1, "y": 0},
            "movement": "grounded",
            "map": 0,
        },
        "vitals": vitals,
    });
    or_abort(serde_json::from_value(json))
}

/// A gearless Dark Knight (5 stat points per level) at `lvl`/`exp`.
fn dark_knight(lvl: u16, exp: u64, unspent: u16, vitals: &serde_json::Value) -> Character {
    character(
        "dark_knight",
        &standard_stats(150, 120, 100, 30),
        lvl,
        exp,
        unspent,
        vitals,
    )
}

/// One live, in-range, same-map fact at `slot`, level `lvl`, experience `exp`.
fn fact(slot: u8, lvl: u16, exp: u64) -> MemberFact {
    MemberFact {
        slot: MemberSlot(slot),
        level: level(lvl),
        experience: Exp(exp),
        vitality: Vitality::Alive,
        map: MapNumber(0),
        position: TileCoord::new(0, 0).to_world(),
    }
}

/// Asserts every pool is full at its class-formula maximum on the grown hero.
fn assert_refilled_full(grown: &Character) {
    let (_profile, maxima) = character_profile(grown);
    assert_eq!(grown.vitals().health, Pool::full(maxima.max_health));
    assert_eq!(grown.vitals().mana, Pool::full(maxima.max_mana));
    assert_eq!(grown.vitals().ability, Pool::full(maxima.max_ability));
}

#[test]
fn a_single_level_crossing_banks_five_points_refills_and_carries_experience() {
    let atlas = real_atlas();
    let t5 = total_to_hold(&atlas, 5);
    let t6 = total_to_hold(&atlas, 6);
    let hero = dark_knight(5, t5, 0, &full_vitals(1, 1, 1));
    let gained = Exp(t6 - t5);

    let (grown, events) = apply_experience(&hero, gained, &atlas);

    assert_eq!(grown.level(), level(6));
    assert_eq!(
        grown.unspent_points(),
        5,
        "one crossing banks 5 for a knight"
    );
    assert_eq!(grown.experience(), Exp(t6), "the new total is old + gained");
    assert_refilled_full(&grown);
    assert_eq!(
        events,
        vec![GrowthEvent::LevelsGained {
            reached: level(6),
            points_granted: 5,
        }]
    );

    // The grown character round-trips through the wire (the class↔stats gate
    // re-proves on load).
    let wire = or_abort(serde_json::to_string(&grown));
    let reloaded: Character = or_abort(serde_json::from_str(&wire));
    assert_eq!(reloaded, grown);
}

#[test]
fn a_multi_level_crossing_banks_five_per_level_refills_at_the_top_and_carries() {
    let atlas = real_atlas();
    let t5 = total_to_hold(&atlas, 5);
    let t8 = total_to_hold(&atlas, 8);
    let hero = dark_knight(5, t5, 0, &full_vitals(1, 1, 1));
    let gained = Exp(t8 - t5);

    let (grown, events) = apply_experience(&hero, gained, &atlas);

    // Crossed 6, 7, 8 → three levels, 5 points each.
    assert_eq!(grown.level(), level(8));
    assert_eq!(
        grown.unspent_points(),
        15,
        "5 per crossing over three levels"
    );
    assert_eq!(grown.experience(), Exp(t8));
    assert!(
        Exp(t8) < atlas.exp_curve().cap_total(),
        "well below the cap"
    );
    assert_refilled_full(&grown);
    assert_eq!(
        events,
        vec![GrowthEvent::LevelsGained {
            reached: level(8),
            points_granted: 15,
        }]
    );
}

#[test]
fn experience_below_the_next_threshold_carries_with_no_growth_event() {
    let atlas = real_atlas();
    let t5 = total_to_hold(&atlas, 5);
    let t6 = total_to_hold(&atlas, 6);
    // A gain that stays strictly inside the level-5 band — no crossing — and far
    // below the curve's cap: the single untested outcome row of `apply_experience`.
    let gained = Exp((t6 - t5) / 2);
    assert!(
        gained.0 > 0,
        "the band is wide enough for a positive sub-threshold gain"
    );
    assert!(
        t5 + gained.0 < t6,
        "the new total never reaches the level-6 threshold"
    );
    assert!(
        Exp(t5 + gained.0) < atlas.exp_curve().cap_total(),
        "well below the cap"
    );

    // Seat the hero mid-band holding banked points and depleted pools, so a
    // spurious refill or point grant would show.
    let hurt = serde_json::json!({
        "health": {"current": 3, "max": 500},
        "mana": {"current": 2, "max": 400},
        "ability": {"current": 1, "max": 400},
    });
    let hero = dark_knight(5, t5, 3, &hurt);
    let before = hero.clone();

    let (grown, events) = apply_experience(&hero, gained, &atlas);

    // The canonical "no growth event": experience moved, nothing crossed, nothing
    // discarded — the exp observable is owned upstream by `ExpAward`.
    assert_eq!(events, vec![]);
    assert_eq!(
        grown.experience(),
        Exp(t5 + gained.0),
        "the gain is carried"
    );
    assert_eq!(grown.level(), level(5), "no level crossed");
    assert_eq!(grown.unspent_points(), 3, "no crossing, no points granted");
    assert_eq!(grown.vitals(), hero.vitals(), "no crossing, no refill");
    // The input character is unmutated.
    assert_eq!(hero, before);
}

#[test]
fn magic_gladiator_and_dark_lord_bank_seven_points_per_crossing() {
    let atlas = real_atlas();
    // Prove the per-class grant against the real class table.
    assert_eq!(
        atlas
            .classes()
            .record(CharacterClass::MagicGladiator)
            .points_per_level,
        7
    );
    assert_eq!(
        atlas
            .classes()
            .record(CharacterClass::DarkLord)
            .points_per_level,
        7
    );

    let t5 = total_to_hold(&atlas, 5);
    let t6 = total_to_hold(&atlas, 6);
    let gained = Exp(t6 - t5);

    let magic_gladiator = character(
        "magic_gladiator",
        &standard_stats(150, 120, 100, 30),
        5,
        t5,
        0,
        &full_vitals(1, 1, 1),
    );
    let (mg_grown, mg_events) = apply_experience(&magic_gladiator, gained, &atlas);
    assert_eq!(mg_grown.level(), level(6));
    assert_eq!(mg_grown.unspent_points(), 7);
    assert_eq!(
        mg_events,
        vec![GrowthEvent::LevelsGained {
            reached: level(6),
            points_granted: 7,
        }]
    );

    let dark_lord = character(
        "dark_lord",
        &command_stats(150, 120, 100, 30, 30),
        5,
        t5,
        0,
        &full_vitals(1, 1, 1),
    );
    let (dl_grown, dl_events) = apply_experience(&dark_lord, gained, &atlas);
    assert_eq!(dl_grown.level(), level(6));
    assert_eq!(dl_grown.unspent_points(), 7);
    assert_eq!(
        dl_events,
        vec![GrowthEvent::LevelsGained {
            reached: level(6),
            points_granted: 7,
        }]
    );
}

#[test]
fn a_capped_character_discards_further_experience_and_reports_max_level() {
    let atlas = real_atlas();
    let max_level = atlas.exp_curve().max_level();
    let cap_total = atlas.exp_curve().cap_total();
    let hero = dark_knight(
        max_level.get(),
        cap_total.0,
        12,
        &full_vitals(500, 400, 400),
    );

    let (grown, events) = apply_experience(&hero, Exp(1000), &atlas);

    assert_eq!(grown.level(), max_level);
    assert_eq!(
        grown.experience(),
        cap_total,
        "over-cap experience is discarded, parked at the cap"
    );
    assert_eq!(grown.unspent_points(), 12, "no crossing, no points");
    // No crossing: vitals are carried unchanged, never refilled.
    assert_eq!(grown.vitals(), hero.vitals());
    assert_eq!(events, vec![GrowthEvent::MaxLevelReached]);
}

#[test]
fn a_crossing_that_lands_on_the_cap_reports_both_levels_gained_and_max_level() {
    let atlas = real_atlas();
    let max_level = atlas.exp_curve().max_level();
    let cap_total = atlas.exp_curve().cap_total();
    // Seat one level below the cap, one experience point under the cap total, then
    // apply a gain that overshoots the cap.
    let hero = dark_knight(
        max_level.get() - 1,
        cap_total.0 - 1,
        0,
        &full_vitals(1, 1, 1),
    );

    let (grown, events) = apply_experience(&hero, cap_total, &atlas);

    assert_eq!(grown.level(), max_level);
    assert_eq!(
        grown.experience(),
        cap_total,
        "the leftover over the cap is dropped"
    );
    assert_eq!(
        events,
        vec![
            GrowthEvent::LevelsGained {
                reached: max_level,
                points_granted: 5,
            },
            GrowthEvent::MaxLevelReached,
        ]
    );
}

#[test]
fn a_crossing_that_lands_exactly_on_the_cap_gains_the_level_without_a_discard_signal() {
    let atlas = real_atlas();
    let max_level = atlas.exp_curve().max_level();
    let cap_total = atlas.exp_curve().cap_total();
    // Seat one level below the cap, holding exactly that level's own threshold, with
    // depleted pools so the crossing's refill shows.
    let threshold = total_to_hold(&atlas, max_level.get() - 1);
    let hurt = serde_json::json!({
        "health": {"current": 3, "max": 500},
        "mana": {"current": 2, "max": 400},
        "ability": {"current": 1, "max": 400},
    });
    let hero = dark_knight(max_level.get() - 1, threshold, 0, &hurt);
    // A gain that lands the total *exactly* on the cap — zero surplus, nothing to
    // discard (the strict `>` overshoot test never fires).
    let gained = Exp(cap_total.0 - threshold);

    let (grown, events) = apply_experience(&hero, gained, &atlas);

    assert_eq!(grown.level(), max_level);
    assert_eq!(
        grown.experience(),
        cap_total,
        "the total lands exactly on the cap, wasting nothing"
    );
    assert_eq!(
        grown.unspent_points(),
        5,
        "one crossing banks 5 for a knight"
    );
    assert_refilled_full(&grown);
    // The exact landing wasted nothing, so there is no discard: LevelsGained alone,
    // never MaxLevelReached. This pins the strict `>` against a `>=` regression.
    assert_eq!(
        events,
        vec![GrowthEvent::LevelsGained {
            reached: max_level,
            points_granted: 5,
        }]
    );
    assert_eq!(events.len(), 1, "no discard signal on an exact landing");
}

#[test]
fn a_hurt_hero_has_all_three_pools_refilled_on_a_crossing() {
    let atlas = real_atlas();
    let t5 = total_to_hold(&atlas, 5);
    let t6 = total_to_hold(&atlas, 6);
    // Depleted: current well below max on all three pools.
    let vitals = serde_json::json!({
        "health": {"current": 3, "max": 500},
        "mana": {"current": 2, "max": 400},
        "ability": {"current": 1, "max": 400},
    });
    let hero = dark_knight(5, t5, 0, &vitals);
    assert!(
        hero.vitals().health.current() < hero.vitals().health.max(),
        "the hero starts hurt"
    );

    let (grown, _events) = apply_experience(&hero, Exp(t6 - t5), &atlas);

    assert_refilled_full(&grown);
    assert_eq!(grown.vitals().health.current(), grown.vitals().health.max());
    assert_eq!(grown.vitals().mana.current(), grown.vitals().mana.max());
    assert_eq!(
        grown.vitals().ability.current(),
        grown.vitals().ability.max()
    );
}

#[test]
fn apply_experience_is_pure_and_deterministic() {
    let atlas = real_atlas();
    let t5 = total_to_hold(&atlas, 5);
    let t8 = total_to_hold(&atlas, 8);
    let hero = dark_knight(5, t5, 0, &full_vitals(1, 1, 1));
    let before = hero.clone();

    let (grown_a, events_a) = apply_experience(&hero, Exp(t8 - t5), &atlas);
    let (grown_b, events_b) = apply_experience(&hero, Exp(t8 - t5), &atlas);

    // Byte-identical outputs on identical inputs.
    assert_eq!(
        or_abort(serde_json::to_string(&grown_a)),
        or_abort(serde_json::to_string(&grown_b)),
    );
    assert_eq!(events_a, events_b);
    // The input character is unmutated — still equals a fresh clone.
    assert_eq!(hero, before);
}

#[test]
fn a_party_award_applied_matches_a_solo_apply_of_the_same_gained() {
    let atlas = real_atlas();
    // Seat the member one experience point below the level-6 boundary, so any
    // positive share carries it across at least one level.
    let t6 = total_to_hold(&atlas, 6);
    let seat_exp = t6 - 1;

    // The party path: a two-member forming party, killer at slot 1, member at
    // slot 0 (its award is a pure proportional share).
    let party = PartySession::forming();
    let killer = fact(1, 5, seat_exp);
    let others = [fact(0, 5, seat_exp)];
    let mut party_rng = TestRng::new(123);
    let awards =
        distribute_kill_experience(&party, killer, &others, level(30), &atlas, &mut party_rng);
    let member_award = or_abort(
        awards
            .iter()
            .find(|award| award.slot == MemberSlot(0))
            .ok_or("slot 0 award"),
    );
    let gained = member_award.gained;
    assert!(gained.0 > 0, "the member gets a positive share");
    assert!(
        !member_award.level_ups.is_empty(),
        "the share crosses a level"
    );

    // The solo path: apply that exact gained to an identical character.
    let member = dark_knight(5, seat_exp, 0, &full_vitals(1, 1, 1));
    let (grown_a, events_a) = apply_experience(&member, gained, &atlas);
    let (grown_b, _events_b) = apply_experience(&member, gained, &atlas);
    // Deterministic and byte-identical.
    assert_eq!(
        or_abort(serde_json::to_string(&grown_a)),
        or_abort(serde_json::to_string(&grown_b)),
    );

    // The service's crossing agrees with the party's own `level_ups` observable:
    // the grown level is the top of the award's ascending list.
    let top = or_abort(member_award.level_ups.last().ok_or("a crossed level")).level;
    assert_eq!(grown_a.level(), top);
    match events_a.first() {
        Some(GrowthEvent::LevelsGained { reached, .. }) => assert_eq!(*reached, top),
        Some(GrowthEvent::MaxLevelReached) | None => {
            panic!("the crossing must emit LevelsGained first")
        }
    }
}
