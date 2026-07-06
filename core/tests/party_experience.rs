//! The party experience split over the real `/data` Atlas: the numeric worked
//! example against the shipped exp curve and jitter band, the seed-independent
//! conservation invariant, the exactly-one-draw determinism pin, the byte-identical
//! `|Q| = 1` degeneration to the solo `award_kill_experience`, and the
//! qualification exclusions (dead / off-map / out-of-range shrink the pool).
//!
//! Load failures route through `or_abort`; every assertion is a `#[test]` body so
//! `unwrap` is exempt.

#[path = "common/dataset.rs"]
mod dataset;
#[path = "common/rng.rs"]
mod rng;

use rand_core::RngCore;

use mu_core::components::party::{MemberSlot, Membership, Vitality};
use mu_core::components::spatial::WorldPos;
use mu_core::components::tile::TileCoord;
use mu_core::components::units::{Exp, Level, MapNumber};
use mu_core::data::atlas::Atlas;
use mu_core::entities::character::Character;
use mu_core::entities::party_session::{PartyMember, PartySession};
use mu_core::services::chance::uniform_in_inclusive;
use mu_core::services::experience::award_kill_experience;
use mu_core::services::party::{MemberFact, distribute_kill_experience};

use dataset::{or_abort, real_atlas};
use rng::TestRng;

fn pos(x: u8, y: u8) -> WorldPos {
    TileCoord::new(x, y).to_world()
}

fn map0() -> MapNumber {
    MapNumber(0)
}

fn level(value: u16) -> Level {
    or_abort(Level::new(value))
}

fn active(slot: u8) -> PartyMember {
    PartyMember {
        slot: MemberSlot(slot),
        membership: Membership::Active,
    }
}

/// A party of `n` active members at slots `0..n`.
fn active_party(n: u8) -> PartySession {
    let mut party = PartySession::forming();
    for slot in 2..n {
        party = party.with_member(active(slot));
    }
    party
}

/// One live, in-range, same-map fact at `slot`, level `lvl`, experience `exp`.
fn fact(slot: u8, lvl: u16, exp: u64) -> MemberFact {
    MemberFact {
        slot: MemberSlot(slot),
        level: level(lvl),
        experience: Exp(exp),
        vitality: Vitality::Alive,
        map: map0(),
        position: pos(0, 0),
    }
}

/// A gearless Dark Knight at `lvl` holding `exp`, built the only way a character
/// can be — by deserialising its wire form.
fn knight(lvl: u16, exp: u64) -> Character {
    let json = serde_json::json!({
        "class": "dark_knight",
        "level": lvl,
        "experience": exp,
        "stats": {"kind": "standard", "strength": 150, "agility": 120, "vitality": 100, "energy": 30},
        "unspent_points": 0,
        "zen": 0,
        "placement": {"position": {"x": 0, "y": 0}, "facing": {"x": 1, "y": 0}, "movement": "grounded", "map": 0},
        "vitals": {"health": {"current": 500, "max": 500}, "mana": {"current": 400, "max": 400}, "ability": {"current": 400, "max": 400}}
    });
    or_abort(serde_json::from_value(json))
}

/// The first seed whose one jitter draw over the atlas band resolves to exactly
/// `target` percent — so the numeric worked example is pinned, not left to chance.
fn seed_with_jitter(atlas: &Atlas, target: u16) -> u64 {
    let band = atlas.progression().exp_jitter_percent;
    for seed in 0u64..100_000 {
        let mut probe = TestRng::new(seed);
        if uniform_in_inclusive(band, &mut probe) == target {
            return seed;
        }
    }
    or_abort(Err::<u64, _>("no seed in range produced the target jitter"))
}

#[test]
fn the_three_member_worked_example_matches_378_757_1137_summing_to_2272() {
    let atlas = real_atlas();
    let party = active_party(3);
    let mut rng = TestRng::new(seed_with_jitter(&atlas, 100));

    let awards = distribute_kill_experience(
        &party,
        fact(2, 30, 0),
        &[fact(0, 10, 0), fact(1, 20, 0)],
        level(30),
        &atlas,
        &mut rng,
    );

    // The killer (slot 2) seeds Q first, so the output leads with it; sort to
    // assert the per-slot amounts independent of the incidental award order.
    let mut gained: Vec<(u8, u64)> = awards.iter().map(|a| (a.slot.0, a.gained.0)).collect();
    gained.sort_unstable();
    assert_eq!(gained, vec![(0, 378), (1, 757), (2, 1137)]);
    let total: u64 = awards.iter().map(|a| a.gained.0).sum();
    assert_eq!(total, 2272, "the whole pool is distributed, nothing lost");
}

#[test]
fn the_split_invariant_holds_across_a_seed_sweep() {
    let atlas = real_atlas();
    let party = active_party(3);
    let facts = [fact(0, 10, 0), fact(1, 20, 0), fact(2, 30, 0)];
    let killer = MemberSlot(2);
    let killer_fact = fact(2, 30, 0);
    let others = [fact(0, 10, 0), fact(1, 20, 0)];
    let level_sum: u64 = facts.iter().map(|f| u64::from(f.level.get())).sum();

    for seed in 0u64..64 {
        let mut rng = TestRng::new(seed);
        let awards =
            distribute_kill_experience(&party, killer_fact, &others, level(30), &atlas, &mut rng);
        let pool: u64 = awards.iter().map(|a| a.gained.0).sum();

        let mut floor_sum = 0u64;
        for award in &awards {
            let member_level = u64::from(
                facts
                    .iter()
                    .find(|f| f.slot == award.slot)
                    .map_or(0, |f| f.level.get()),
            );
            let proportional = u128::from(pool) * u128::from(member_level) / u128::from(level_sum);
            let proportional = u64::try_from(proportional).unwrap_or(u64::MAX);
            floor_sum += proportional;
            if award.slot == killer {
                // Killer carries its own share plus the whole remainder.
                continue;
            }
            assert_eq!(
                award.gained.0, proportional,
                "a non-killer gets exactly its floored proportional share"
            );
        }
        let remainder = pool - floor_sum;
        let killer_award = awards.iter().find(|a| a.slot == killer).map(|a| a.gained.0);
        let killer_proportional =
            u64::try_from(u128::from(pool) * 30 / u128::from(level_sum)).unwrap_or(u64::MAX);
        assert_eq!(
            killer_award,
            Some(killer_proportional + remainder),
            "the killer carries its share plus the split remainder"
        );
    }
}

#[test]
fn exactly_one_rng_word_is_consumed_per_kill_for_any_party_size() {
    let atlas = real_atlas();
    let band = atlas.progression().exp_jitter_percent;
    for size in 1u8..=5 {
        let party = if size == 1 {
            active_party(2)
        } else {
            active_party(size)
        };
        // For size 1, a two-member party with the second member dead qualifies
        // only the killer.
        let killer_fact = fact(0, 30, 0);
        let mut others: Vec<MemberFact> = (1..party.len())
            .map(|slot| fact(u8::try_from(slot).unwrap(), 30, 0))
            .collect();
        // For size 1, the sole other member is dead, so only the killer qualifies.
        if size == 1 {
            if let Some(second) = others.get_mut(0) {
                second.vitality = Vitality::Dead;
            }
        }

        let mut probe = TestRng::new(7);
        let _ = uniform_in_inclusive(band, &mut probe);

        let mut rng = TestRng::new(7);
        let _ =
            distribute_kill_experience(&party, killer_fact, &others, level(40), &atlas, &mut rng);

        assert_eq!(
            rng.next_u64(),
            probe.next_u64(),
            "distribute advances the stream by exactly one jitter draw for size {size}"
        );
    }
}

#[test]
fn the_solo_q_of_one_path_is_byte_identical_to_award_kill_experience() {
    let atlas = real_atlas();
    for (killer_level, victim, killer_exp) in [
        (20u16, 30u16, 0u64),
        (60, 60, 100_000),
        (70, 55, 5_000_000),
        (10, 40, 0),
    ] {
        // The party path: a two-member party whose second member is dead, so
        // only the killer qualifies (|Q| = 1).
        let party = PartySession::forming();
        let killer_fact = fact(0, killer_level, killer_exp);
        let others = [{
            let mut dead = fact(1, 25, 0);
            dead.vitality = Vitality::Dead;
            dead
        }];
        let mut party_rng = TestRng::new(42);
        let party_awards = distribute_kill_experience(
            &party,
            killer_fact,
            &others,
            level(victim),
            &atlas,
            &mut party_rng,
        );

        // The solo path over the same seed.
        let killer = knight(killer_level, killer_exp);
        let mut solo_rng = TestRng::new(42);
        let (gained, level_ups) =
            award_kill_experience(&killer, level(victim), &atlas, &mut solo_rng);

        assert_eq!(party_awards.len(), 1, "only the killer qualifies");
        let award = or_abort(party_awards.first().ok_or("one award"));
        assert_eq!(award.slot, MemberSlot(0));
        assert_eq!(
            award.gained, gained,
            "gained is byte-identical to the solo award"
        );
        assert_eq!(award.level_ups, level_ups, "level-ups are byte-identical");
        // The stream advanced identically — exactly one shared jitter draw.
        assert_eq!(party_rng.next_u64(), solo_rng.next_u64());
    }
}

#[test]
fn excluding_a_dead_or_off_map_or_out_of_range_member_shrinks_the_qualifying_set() {
    let atlas = real_atlas();
    let party = active_party(3);
    let killer_fact = fact(0, 10, 0);
    let others = [fact(1, 20, 0), fact(2, 30, 0)];
    let seed = seed_with_jitter(&atlas, 100);

    // Baseline: all three qualify.
    let mut rng = TestRng::new(seed);
    let full =
        distribute_kill_experience(&party, killer_fact, &others, level(30), &atlas, &mut rng);
    assert_eq!(full.len(), 3);

    // Slot 2 (others[1]) dead -> excluded from the award and the denominator.
    let mut dead = others;
    if let Some(f) = dead.get_mut(1) {
        f.vitality = Vitality::Dead;
    }
    let mut rng = TestRng::new(seed);
    let two = distribute_kill_experience(&party, killer_fact, &dead, level(30), &atlas, &mut rng);
    assert_eq!(two.len(), 2);
    assert!(two.iter().all(|a| a.slot != MemberSlot(2)));

    // Slot 2 off-map -> excluded likewise.
    let mut offmap = others;
    if let Some(f) = offmap.get_mut(1) {
        f.map = MapNumber(7);
    }
    let mut rng = TestRng::new(seed);
    let off = distribute_kill_experience(&party, killer_fact, &offmap, level(30), &atlas, &mut rng);
    assert_eq!(off.len(), 2);

    // Slot 2 at 13 tiles -> excluded; at 12 tiles -> included (inclusive edge).
    let mut far = others;
    if let Some(f) = far.get_mut(1) {
        f.position = pos(13, 0);
    }
    let mut rng = TestRng::new(seed);
    let out = distribute_kill_experience(&party, killer_fact, &far, level(30), &atlas, &mut rng);
    assert_eq!(out.len(), 2);

    let mut edge = others;
    if let Some(f) = edge.get_mut(1) {
        f.position = pos(12, 0);
    }
    let mut rng = TestRng::new(seed);
    let inc = distribute_kill_experience(&party, killer_fact, &edge, level(30), &atlas, &mut rng);
    assert_eq!(inc.len(), 3, "the 12-tile edge is inclusive");
}

#[test]
fn a_share_crossing_a_curve_boundary_lists_the_crossed_levels_ascending() {
    let atlas = real_atlas();
    let party = active_party(2);
    // Seat slot 1 one experience point below the total needed to hold level 6, so
    // any positive share carries it into level 6 (and possibly beyond).
    let boundary = or_abort(atlas.exp_curve().level(6)).total_to_hold().0;
    let killer_fact = fact(0, 30, 0);
    let others = [fact(1, 5, boundary.saturating_sub(1))];
    let mut rng = TestRng::new(seed_with_jitter(&atlas, 120));

    let awards =
        distribute_kill_experience(&party, killer_fact, &others, level(40), &atlas, &mut rng);
    let slot1 = or_abort(
        awards
            .iter()
            .find(|a| a.slot == MemberSlot(1))
            .ok_or("slot 1 award"),
    );
    assert!(
        !slot1.level_ups.is_empty(),
        "the share crosses at least one level"
    );
    assert_eq!(slot1.level_ups.first().map(|l| l.level.get()), Some(6));
    // Ascending, no repeats.
    let levels: Vec<u16> = slot1.level_ups.iter().map(|l| l.level.get()).collect();
    let mut sorted = levels.clone();
    sorted.sort_unstable();
    assert_eq!(levels, sorted);
}
