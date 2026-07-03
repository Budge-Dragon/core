//! The single physical-strike resolver: attacker profile against defender
//! profile and health, one authoritative outcome. Pure and deterministic — the
//! RNG is drawn in a fixed order (hit, damage span, then the four special-hit
//! rolls) so the same seed replays bit-for-bit, and the damage span is *always*
//! rolled even when a critical or excellent overrides it, keeping the RNG
//! consumption constant across every branch.

use rand_core::RngCore;

use crate::components::combat_profile::CombatProfile;
use crate::components::pool::Pool;
use crate::components::units::ChancePer10000;
use crate::events::combat::{AttackOutcome, Damage, DamageModifiers, Hit, HitQuality};
use crate::services::chance::{roll_per_10000, roll_percent, uniform_in_inclusive};
use crate::services::ratio::{nonzero, scale_ratio};

// W-SRC: OpenMU hit/damage constants hardcoded in the combat routine, not in
// game_config.json — the minimum landed-hit chance, the overrate damage
// penalty when the defender out-rates the attacker, and the level-scaled
// minimum-damage floor divisor.
/// The floor a landed-hit chance never drops below (per 10,000).
const HIT_CHANCE_FLOOR_PER_10000: u32 = 300;
/// Overrate penalty numerator: damage kept when the defender out-rates.
const OVERRATE_NUM: u32 = 3;
/// Overrate penalty denominator.
const OVERRATE_DEN: u32 = 10;
/// Divisor of the level-scaled minimum-damage floor (`max(1, level / 10)`).
const MIN_DAMAGE_FLOOR_DIVISOR: u32 = 10;

/// Resolves one physical strike: rolls to hit, then — on a hit — the damage
/// span and the four special-hit rolls, applies defense, the overrate penalty,
/// doubling, and the minimum-damage floor, and reduces the defender's health.
/// Returns the defender's health after the strike and the [`AttackOutcome`].
#[must_use]
pub fn resolve_attack(
    attacker: &CombatProfile,
    target: &CombatProfile,
    target_health: Pool,
    rng: &mut impl RngCore,
) -> (Pool, AttackOutcome) {
    if !roll_per_10000(hit_chance(attacker, target), rng) {
        return (target_health, AttackOutcome::Missed);
    }

    // The span is always drawn, even when a critical/excellent overrides the
    // base, so RNG consumption is constant across every damage branch.
    let span = uniform_in_inclusive(attacker.physical(), rng);
    let excellent = roll_percent(attacker.excellent_chance(), rng);
    let critical = roll_percent(attacker.critical_chance(), rng);
    let defense_ignored = roll_percent(attacker.defense_ignore_chance(), rng);
    let doubled = roll_percent(attacker.double_damage_chance(), rng);

    let quality = if excellent {
        HitQuality::Excellent
    } else if critical {
        HitQuality::Critical
    } else {
        HitQuality::Normal
    };
    let damage = damage_dealt(attacker, target, span, quality, defense_ignored, doubled);
    let new_health = target_health.reduced(damage);
    let hit = Hit {
        damage: Damage(damage),
        quality,
        modifiers: DamageModifiers {
            defense_ignored,
            doubled,
        },
    };
    let outcome = if new_health.current() == 0 {
        AttackOutcome::Killed { hit }
    } else {
        AttackOutcome::Landed { hit }
    };
    (new_health, outcome)
}

/// The landed-hit chance per 10,000: when the attacker out-rates the defender,
/// `10000 - defense_rate * 10000 / attack_rate`, floored at
/// [`HIT_CHANCE_FLOOR_PER_10000`]; otherwise the bare floor. The `ar > dr` guard
/// proves the divisor non-zero.
fn hit_chance(attacker: &CombatProfile, target: &CombatProfile) -> ChancePer10000 {
    let attack_rate = u32::from(attacker.attack_rate());
    let defense_rate = u32::from(target.defense_rate());
    let numerator = if attack_rate > defense_rate {
        let reduction = scale_ratio(
            defense_rate,
            u32::from(ChancePer10000::DENOMINATOR),
            nonzero(attack_rate),
        );
        u32::from(ChancePer10000::DENOMINATOR)
            .saturating_sub(reduction)
            .max(HIT_CHANCE_FLOOR_PER_10000)
    } else {
        HIT_CHANCE_FLOOR_PER_10000
    };
    ChancePer10000::clamped(u64::from(numerator))
}

/// The damage magnitude of a landed hit: the quality-selected base, minus
/// defense unless ignored, the overrate penalty when the defender out-rates,
/// doubled when the double roll succeeded, and finally floored at
/// `max(1, level / 10)` — the floor applied last so it always holds.
fn damage_dealt(
    attacker: &CombatProfile,
    target: &CombatProfile,
    span: u16,
    quality: HitQuality,
    defense_ignored: bool,
    doubled: bool,
) -> u32 {
    let max_physical = u32::from(attacker.physical().max());
    let base = match quality {
        // W-SRC: 1.2× max — CONFIRMED on-era against MuEmu 0.97k C++ source
        // (independent of OpenMU): `damage = (DamageMax * 120) / 100`, and
        // OpenMU S6; every era 0.75→S6 uses 1.2×. The 1.1× in some fan guides is
        // a rate-vs-magnitude confusion (the +10% Excellent Damage RATE option is
        // a proc chance, not the hit magnitude).
        HitQuality::Excellent => scale_ratio(max_physical, 6, nonzero(5)),
        HitQuality::Critical => max_physical,
        HitQuality::Normal => u32::from(span),
    };
    let after_defense = if defense_ignored {
        base
    } else {
        base.saturating_sub(u32::from(target.defense()))
    };
    let after_overrate = if target.defense_rate() > attacker.attack_rate() {
        scale_ratio(after_defense, OVERRATE_NUM, nonzero(OVERRATE_DEN))
    } else {
        after_defense
    };
    let after_double = if doubled {
        after_overrate.saturating_mul(2)
    } else {
        after_overrate
    };
    let floor = (u32::from(attacker.level().get()) / MIN_DAMAGE_FLOOR_DIVISOR).max(1);
    after_double.max(floor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::element::PerElement;
    use crate::components::interval::Interval;
    use crate::components::units::{Level, Percent, Resistance};

    /// Deterministic `SplitMix64` for replayable tests.
    struct TestRng {
        state: u64,
    }

    impl TestRng {
        fn new(seed: u64) -> Self {
            Self { state: seed }
        }
    }

    impl RngCore for TestRng {
        fn next_u64(&mut self) -> u64 {
            self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.state;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        }

        fn next_u32(&mut self) -> u32 {
            let [b0, b1, b2, b3, _, _, _, _] = self.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            for chunk in dst.chunks_mut(8) {
                let bytes = self.next_u64().to_le_bytes();
                for (slot, byte) in chunk.iter_mut().zip(bytes.iter()) {
                    *slot = *byte;
                }
            }
        }
    }

    fn zero_resistances() -> PerElement<Resistance> {
        PerElement {
            ice: Resistance(0),
            poison: Resistance(0),
            lightning: Resistance(0),
            fire: Resistance(0),
            earth: Resistance(0),
            wind: Resistance(0),
            water: Resistance(0),
        }
    }

    /// A gearless profile: tunable span, defense, level, and rates; all four
    /// special-hit chances zero. Layer chances on with [`with_chances`].
    fn plain(
        level: u16,
        min_phys: u16,
        max_phys: u16,
        defense: u16,
        ar: u16,
        dr: u16,
    ) -> CombatProfile {
        CombatProfile {
            level: Level::new(level).unwrap(),
            physical: Interval::new(min_phys, max_phys).unwrap(),
            wizardry: None,
            defense,
            attack_rate: ar,
            defense_rate: dr,
            resistances: zero_resistances(),
            critical_chance: Percent::ZERO,
            excellent_chance: Percent::ZERO,
            defense_ignore_chance: Percent::ZERO,
            double_damage_chance: Percent::ZERO,
        }
    }

    /// The base profile with its four special-hit chances set (percent points).
    fn with_chances(
        base: CombatProfile,
        excellent: u8,
        critical: u8,
        defense_ignore: u8,
        double: u8,
    ) -> CombatProfile {
        CombatProfile {
            critical_chance: Percent::new(critical).unwrap(),
            excellent_chance: Percent::new(excellent).unwrap(),
            defense_ignore_chance: Percent::new(defense_ignore).unwrap(),
            double_damage_chance: Percent::new(double).unwrap(),
            ..base
        }
    }

    #[test]
    fn a_miss_leaves_health_unchanged() {
        // Defender out-rates the attacker: the 3% floor governs; a seed that
        // rolls above 300/10000 misses.
        let attacker = plain(50, 10, 20, 0, 100, 100);
        let target = plain(50, 0, 0, 0, 100, 10_000);
        let mut rng = TestRng::new(1);
        let mut misses = 0;
        for _ in 0..200 {
            let (health, outcome) = resolve_attack(&attacker, &target, Pool::full(100), &mut rng);
            if matches!(outcome, AttackOutcome::Missed) {
                assert_eq!(health.current(), 100);
                misses += 1;
            }
        }
        assert!(misses > 0, "a floored hit chance must miss sometimes");
    }

    #[test]
    fn hit_chance_floors_at_three_percent_when_out_rated() {
        let attacker = plain(50, 10, 20, 0, 10, 10_000);
        let target = plain(50, 0, 0, 0, 100, 10_000);
        assert_eq!(hit_chance(&attacker, &target).numerator(), 300);
    }

    #[test]
    fn a_normal_hit_is_span_minus_defense() {
        // Fixed span: min == max == 20, defense 5, no specials, no overrate.
        let attacker = plain(10, 20, 20, 0, 10_000, 0);
        let target = plain(10, 0, 0, 5, 0, 0);
        let mut rng = TestRng::new(3);
        let (_, outcome) = resolve_attack(&attacker, &target, Pool::full(100), &mut rng);
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(15)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn minimum_damage_floor_scales_with_level() {
        // Span 1, defense 100 → base wiped to zero, so the floor governs.
        let l50 = plain(50, 1, 1, 0, 10_000, 0);
        let strong = plain(50, 0, 0, 100, 0, 0);
        let mut rng = TestRng::new(4);
        let (_, outcome) = resolve_attack(&l50, &strong, Pool::full(100), &mut rng);
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(5)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
        let l4 = plain(4, 1, 1, 0, 10_000, 0);
        let mut rng = TestRng::new(4);
        let (_, outcome) = resolve_attack(&l4, &strong, Pool::full(100), &mut rng);
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(1)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn overrate_penalty_applies_when_the_defender_out_rates() {
        // Base 100 span, defense 0; defender defense_rate > attacker attack_rate
        // keeps 3/10 → 30. Since the defender out-rates, the hit chance floors at
        // 3%, so we sample many strikes and assert every landed one is penalised.
        let attacker = plain(10, 100, 100, 0, 100, 0);
        let target = plain(10, 0, 0, 0, 0, 200);
        let mut rng = TestRng::new(5);
        let mut landed = 0;
        for _ in 0..2000 {
            if let (_, AttackOutcome::Landed { hit }) =
                resolve_attack(&attacker, &target, Pool::full(500), &mut rng)
            {
                assert_eq!(hit.damage, Damage(30));
                landed += 1;
            }
        }
        assert!(
            landed > 0,
            "a 3% hit chance must land at least once in 2000 tries"
        );
    }

    #[test]
    fn critical_uses_max_and_excellent_outranks_it() {
        // Critical: base = max_phys (30). excellent 100% forces excellent tier
        // = 6/5 * 30 = 36 and outranks a 100% critical.
        let critical = with_chances(plain(10, 10, 30, 0, 10_000, 0), 0, 100, 0, 0);
        let mut rng = TestRng::new(6);
        let (_, outcome) = resolve_attack(
            &critical,
            &plain(10, 0, 0, 0, 0, 0),
            Pool::full(200),
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => {
                assert_eq!(hit.quality, HitQuality::Critical);
                assert_eq!(hit.damage, Damage(30));
            }
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
        let excellent = with_chances(plain(10, 10, 30, 0, 10_000, 0), 100, 100, 0, 0);
        let mut rng = TestRng::new(6);
        let (_, outcome) = resolve_attack(
            &excellent,
            &plain(10, 0, 0, 0, 0, 0),
            Pool::full(200),
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => {
                assert_eq!(hit.quality, HitQuality::Excellent);
                assert_eq!(hit.damage, Damage(36));
            }
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn defense_ignore_skips_the_subtraction() {
        let attacker = with_chances(plain(10, 20, 20, 0, 10_000, 0), 0, 0, 100, 0);
        let target = plain(10, 0, 0, 15, 0, 0);
        let mut rng = TestRng::new(7);
        let (_, outcome) = resolve_attack(&attacker, &target, Pool::full(100), &mut rng);
        match outcome {
            AttackOutcome::Landed { hit } => {
                assert!(
                    hit.modifiers
                        .contains(crate::events::combat::DamageModifier::DefenseIgnored)
                );
                assert_eq!(hit.damage, Damage(20));
            }
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn double_damage_doubles_after_everything_else() {
        let attacker = with_chances(plain(10, 20, 20, 0, 10_000, 0), 0, 0, 0, 100);
        let target = plain(10, 0, 0, 5, 0, 0);
        let mut rng = TestRng::new(8);
        let (_, outcome) = resolve_attack(&attacker, &target, Pool::full(100), &mut rng);
        match outcome {
            AttackOutcome::Landed { hit } => {
                // (20 - 5) * 2 = 30.
                assert_eq!(hit.damage, Damage(30));
                assert!(
                    hit.modifiers
                        .contains(crate::events::combat::DamageModifier::Doubled)
                );
            }
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn a_lethal_hit_floors_health_at_zero_and_reports_killed() {
        let attacker = plain(10, 100, 100, 0, 10_000, 0);
        let target = plain(10, 0, 0, 0, 0, 0);
        let mut rng = TestRng::new(9);
        let (health, outcome) = resolve_attack(&attacker, &target, Pool::full(10), &mut rng);
        assert_eq!(health.current(), 0);
        assert!(matches!(outcome, AttackOutcome::Killed { .. }));
    }

    #[test]
    fn same_seed_is_bit_identical() {
        let attacker = with_chances(plain(30, 10, 40, 3, 500, 100), 20, 20, 20, 20);
        let target = plain(30, 0, 0, 10, 200, 200);
        let run = |seed: u64| {
            let mut rng = TestRng::new(seed);
            resolve_attack(&attacker, &target, Pool::full(300), &mut rng)
        };
        assert_eq!(run(42), run(42));
    }
}
