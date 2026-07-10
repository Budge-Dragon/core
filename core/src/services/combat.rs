//! The single strike resolver: attacker profile against defender profile and
//! health, struck on a [`StrikeBasis`] (a plain weapon swing or a skill's
//! pre-selected span), one authoritative outcome. Pure and deterministic — the
//! RNG is drawn in a fixed order (hit, damage span, then the four special-hit
//! rolls) so the same seed replays bit-for-bit, and the damage span is *always*
//! rolled even when a critical or excellent overrides it or the span is a
//! collapsed `[0, 0]`, keeping the RNG consumption constant across every branch
//! and every basis.

use rand_core::RngCore;

use crate::components::combat_profile::CombatProfile;
use crate::components::interval::Interval;
use crate::components::pool::Pool;
use crate::components::units::{ChancePer10000, Percent};
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
/// Per-mille divisor of the class skill multiplier (`× multiplier / 1000`).
const SKILL_MULTIPLIER_DENOMINATOR: u32 = 1000;

/// Which basis a strike resolves against: a plain weapon swing, or a skill's
/// DamageType-selected span. The one input [`resolve_attack`] reads to pick the
/// span it draws, the excellent-head order, and whether a class multiplier
/// applies. A transient service intent — never persisted, never a wire type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrikeBasis {
    /// A weapon swing: the attacker's physical span, physical excellent order,
    /// and NO class multiplier. Monster attacks and player basic attacks pass
    /// this. The missing multiplier is structural — there is no field to
    /// fabricate.
    PlainSwing,
    /// A skill strike: a pre-selected augmented span, the type-selected
    /// excellent order, and the caster's class multiplier (per-mille). Minted
    /// only by the skills service's span selection, so span/order/multiplier
    /// are always mutually consistent.
    Skill {
        /// The DamageType-augmented span the strike draws from — `[0, 0]` for a
        /// None-type skill or a wizardry cast by a caster with no wizardry
        /// interval.
        span: Interval<u16>,
        /// Whether the excellent tier multiplies before or after defense — the
        /// sole per-type difference in the fold.
        excellent_order: ExcellentOrder,
        /// The class `SkillMultiplier`, per-mille (÷1000). Applied at the
        /// multiplier step to skill strikes only.
        multiplier_per_mille: u32,
    },
}

/// The order the excellent ×6/5 applies relative to defense subtraction — the
/// one behavior that differs by damage type. Physical multiplies then
/// subtracts; wizardry subtracts then multiplies. Carries no type identity
/// beyond this order, so no span/type contradiction is representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExcellentOrder {
    /// Physical: `(aug_max × 6/5) − defense`.
    MultiplyThenDefense,
    /// Wizardry: `(aug_max − defense) × 6/5`.
    DefenseThenMultiply,
}

/// Resolves one strike on its basis: rolls to hit, then — on a hit — the
/// basis-selected damage span and the four special-hit rolls, folds the quality
/// head (defense in the basis's excellent order), the attacker's flat
/// Greater-Damage add, the overrate penalty, the minimum-damage floor, the
/// class skill multiplier (skill bases only), doubling, and the defender's
/// timed %-reduction, and reduces the defender's health. Returns the defender's
/// health after the strike and the [`AttackOutcome`].
#[must_use]
pub fn resolve_attack(
    attacker: &CombatProfile,
    target: &CombatProfile,
    target_health: Pool,
    basis: &StrikeBasis,
    rng: &mut impl RngCore,
) -> (Pool, AttackOutcome) {
    if !roll_per_10000(hit_chance(attacker, target), rng) {
        return (target_health, AttackOutcome::Missed);
    }

    let interval = match basis {
        StrikeBasis::PlainSwing => attacker.physical(),
        StrikeBasis::Skill { span, .. } => *span,
    };
    // The span is always drawn, even when a critical/excellent overrides the
    // base and even on a collapsed [0, 0] span, so RNG consumption is constant
    // across every damage branch and every basis.
    let span = uniform_in_inclusive(interval, rng);
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
    let excellent_order = match basis {
        // A plain weapon swing is a physical strike.
        StrikeBasis::PlainSwing => ExcellentOrder::MultiplyThenDefense,
        StrikeBasis::Skill {
            excellent_order, ..
        } => *excellent_order,
    };
    let effective_defense = if defense_ignored { 0 } else { target.defense() };
    let after_defense = quality_after_defense(
        interval.max(),
        span,
        quality,
        excellent_order,
        effective_defense,
    );
    let damage = strike_tail(after_defense, attacker, target, basis, doubled);
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

/// The quality-selected base minus defense, in the basis's order. Normal reads
/// the rolled span; critical the augmented max; excellent applies the 6/5
/// either side of the defense subtraction per `order`. Exhaustive over
/// [`HitQuality`] and [`ExcellentOrder`] — no wildcard.
fn quality_after_defense(
    augmented_max: u16,
    rolled_span: u16,
    quality: HitQuality,
    order: ExcellentOrder,
    // Already 0 when the strike ignored defense.
    defense: u16,
) -> u32 {
    let max = u32::from(augmented_max);
    let def = u32::from(defense);
    match quality {
        HitQuality::Normal => u32::from(rolled_span).saturating_sub(def),
        HitQuality::Critical => max.saturating_sub(def),
        // W-SRC: 1.2× max — CONFIRMED on-era against MuEmu 0.97k C++ source
        // (independent of OpenMU): `damage = (DamageMax * 120) / 100`, and
        // OpenMU S6; every era 0.75→S6 uses 1.2×, and it holds under the
        // skill-augmented base. The 1.1× in some fan guides is a
        // rate-vs-magnitude confusion (the +10% Excellent Damage RATE option is
        // a proc chance, not the hit magnitude). Physical multiplies then
        // subtracts; wizardry subtracts then multiplies
        // (AttackableExtensions.cs:106/136 vs :159-160).
        HitQuality::Excellent => match order {
            ExcellentOrder::MultiplyThenDefense => {
                scale_ratio(max, 6, nonzero(5)).saturating_sub(def)
            }
            ExcellentOrder::DefenseThenMultiply => {
                scale_ratio(max.saturating_sub(def), 6, nonzero(5))
            }
        },
    }
}

/// The shared post-head fold: + flat Greater-Damage add → overrate ×3/10 →
/// level floor → × skill multiplier → ×2 double → defender %-reduction.
/// Defense is already out (the head owns it); this never subtracts it again.
/// The multiplier step matches the basis, so a plain swing skips it
/// structurally (no field to read).
fn strike_tail(
    after_defense: u32,
    attacker: &CombatProfile,
    target: &CombatProfile,
    basis: &StrikeBasis,
    doubled: bool,
) -> u32 {
    // W-SRC: the attacker's Greater-Damage add is a FLAT add applied here, after
    // defense and before the floor — never a raise of the strike span, since the
    // quality base is already fixed, so crit/excellent cannot amplify it.
    let after_greater = after_defense.saturating_add(attacker.flat_damage_add());
    // W-SRC: the overrate penalty applies BEFORE the level floor
    // (AttackableExtensions.cs:204-217), so an overrated hit still lands at
    // least max(1, level/10).
    let after_overrate = if target.defense_rate() > attacker.attack_rate() {
        scale_ratio(after_greater, OVERRATE_NUM, nonzero(OVERRATE_DEN))
    } else {
        after_greater
    };
    let floor = (u32::from(attacker.level().get()) / MIN_DAMAGE_FLOOR_DIVISOR).max(1);
    let floored = after_overrate.max(floor);
    // Skill strikes only: a plain swing carries no multiplier to apply.
    let after_multiplier = match basis {
        StrikeBasis::PlainSwing => floored,
        StrikeBasis::Skill {
            multiplier_per_mille,
            ..
        } => scale_ratio(
            floored,
            *multiplier_per_mille,
            nonzero(SKILL_MULTIPLIER_DENOMINATOR),
        ),
    };
    let after_double = if doubled {
        after_multiplier.saturating_mul(2)
    } else {
        after_multiplier
    };
    // OUR-pin: the defender's timed %-reduction is the final step, applied
    // after the double (OpenMU applies it before the multiplier/double block) —
    // commutative in scope modulo per-step integer truncation. A zero reduction
    // (the gearless and effect-free case) is the identity. The caller passes
    // the effect-folded profile, so the reduction field already reflects any
    // active defensive buff.
    let reduction = u32::from(target.incoming_damage_reduction().points());
    scale_ratio(
        after_double,
        u32::from(Percent::DENOMINATOR).saturating_sub(reduction),
        nonzero(u32::from(Percent::DENOMINATOR)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::element::PerElement;
    use crate::components::interval::Interval;
    use crate::components::units::{Level, Resistance};

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
            incoming_damage_reduction: Percent::ZERO,
            flat_damage_add: 0,
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
            let (health, outcome) = resolve_attack(
                &attacker,
                &target,
                Pool::full(100),
                &StrikeBasis::PlainSwing,
                &mut rng,
            );
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
        let (_, outcome) = resolve_attack(
            &attacker,
            &target,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
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
        let (_, outcome) = resolve_attack(
            &l50,
            &strong,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(5)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
        let l4 = plain(4, 1, 1, 0, 10_000, 0);
        let mut rng = TestRng::new(4);
        let (_, outcome) = resolve_attack(
            &l4,
            &strong,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
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
            if let (_, AttackOutcome::Landed { hit }) = resolve_attack(
                &attacker,
                &target,
                Pool::full(500),
                &StrikeBasis::PlainSwing,
                &mut rng,
            ) {
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
    fn the_level_floor_wins_over_the_overrate_penalty() {
        // W-SRC reorder corner (AttackableExtensions.cs:204-217): span [3,3]
        // overrated to 3×3/10 = 0, then floored to max(1, 50/10) = 5. The
        // pre-reorder fold floored first (max(3,5) = 5) and overrated after
        // (5×3/10 = 1). The out-rating defender floors the hit chance at 3%, so
        // landed hits are sampled across a seed sweep.
        let attacker = plain(50, 3, 3, 0, 100, 0);
        let target = plain(50, 0, 0, 0, 0, 200);
        let mut rng = TestRng::new(17);
        let mut landed = 0;
        for _ in 0..2000 {
            if let (_, AttackOutcome::Landed { hit }) = resolve_attack(
                &attacker,
                &target,
                Pool::full(500),
                &StrikeBasis::PlainSwing,
                &mut rng,
            ) {
                assert_eq!(hit.damage, Damage(5), "the floor has the last word");
                landed += 1;
            }
        }
        assert!(landed > 0, "a 3% hit chance lands in 2000 tries");
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
            &StrikeBasis::PlainSwing,
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
            &StrikeBasis::PlainSwing,
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
        let (_, outcome) = resolve_attack(
            &attacker,
            &target,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
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
        let (_, outcome) = resolve_attack(
            &attacker,
            &target,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
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
        let (health, outcome) = resolve_attack(
            &attacker,
            &target,
            Pool::full(10),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        assert_eq!(health.current(), 0);
        assert!(matches!(outcome, AttackOutcome::Killed { .. }));
    }

    /// The base profile with a timed incoming-damage reduction set.
    fn with_reduction(base: CombatProfile, points: u8) -> CombatProfile {
        CombatProfile {
            incoming_damage_reduction: Percent::new(points).unwrap(),
            ..base
        }
    }

    /// The base profile with a flat Greater-Damage add set.
    fn with_flat_add(base: CombatProfile, amount: u32) -> CombatProfile {
        CombatProfile {
            flat_damage_add: amount,
            ..base
        }
    }

    #[test]
    fn flat_damage_add_lands_after_defense_and_crit_excellent_do_not_amplify_it() {
        // Fixed span 20, defender defense 5, level-10 attacker (floor 1).
        let target = plain(10, 0, 0, 5, 0, 0);
        // Normal strike: (20 - 5) + 8 flat = 23.
        let normal = with_flat_add(plain(10, 20, 20, 0, 10_000, 0), 8);
        let mut rng = TestRng::new(31);
        let (_, outcome) = resolve_attack(
            &normal,
            &target,
            Pool::full(200),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(23)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
        // Excellent 100% forces the excellent tier (base = 6/5 × max = 24); the
        // flat add is NOT amplified by the quality: (24 - 5) + 8 = 27, i.e. the
        // same +8 the normal strike got, never +8 × 6/5.
        let excellent = with_chances(
            with_flat_add(plain(10, 20, 20, 0, 10_000, 0), 8),
            100,
            0,
            0,
            0,
        );
        let mut rng = TestRng::new(31);
        let (_, outcome) = resolve_attack(
            &excellent,
            &target,
            Pool::full(200),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => {
                assert_eq!(hit.quality, HitQuality::Excellent);
                assert_eq!(hit.damage, Damage(27));
            }
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn incoming_reduction_zero_is_byte_identical_and_fifty_percent_halves() {
        // Fixed span 20, no defense/specials/overrate: the base damage is 20.
        let attacker = plain(10, 20, 20, 0, 10_000, 0);
        let plain_target = plain(10, 0, 0, 0, 0, 0);
        let mut rng = TestRng::new(21);
        let (_, outcome) = resolve_attack(
            &attacker,
            &plain_target,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(20)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
        // The same strike against a defender reducing incoming damage 50%: 20 → 10.
        let halved = with_reduction(plain(10, 0, 0, 0, 0, 0), 50);
        let mut rng = TestRng::new(21);
        let (_, outcome) = resolve_attack(
            &attacker,
            &halved,
            Pool::full(100),
            &StrikeBasis::PlainSwing,
            &mut rng,
        );
        match outcome {
            AttackOutcome::Landed { hit } => assert_eq!(hit.damage, Damage(10)),
            AttackOutcome::Missed | AttackOutcome::Killed { .. } => panic!("expected a landed hit"),
        }
    }

    #[test]
    fn same_seed_is_bit_identical() {
        let attacker = with_chances(plain(30, 10, 40, 3, 500, 100), 20, 20, 20, 20);
        let target = plain(30, 0, 0, 10, 200, 200);
        let skill_basis = StrikeBasis::Skill {
            span: Interval::new(56, 92).unwrap(),
            excellent_order: ExcellentOrder::DefenseThenMultiply,
            multiplier_per_mille: 2030,
        };
        let run = |seed: u64, basis: &StrikeBasis| {
            let mut rng = TestRng::new(seed);
            resolve_attack(&attacker, &target, Pool::full(300), basis, &mut rng)
        };
        assert_eq!(
            run(42, &StrikeBasis::PlainSwing),
            run(42, &StrikeBasis::PlainSwing)
        );
        assert_eq!(run(42, &skill_basis), run(42, &skill_basis));
    }

    /// A skill basis over `span` with the physical excellent order.
    fn physical_basis(min: u16, max: u16, multiplier_per_mille: u32) -> StrikeBasis {
        StrikeBasis::Skill {
            span: Interval::new(min, max).unwrap(),
            excellent_order: ExcellentOrder::MultiplyThenDefense,
            multiplier_per_mille,
        }
    }

    /// A skill basis over `span` with the wizardry excellent order.
    fn wizardry_basis(min: u16, max: u16, multiplier_per_mille: u32) -> StrikeBasis {
        StrikeBasis::Skill {
            span: Interval::new(min, max).unwrap(),
            excellent_order: ExcellentOrder::DefenseThenMultiply,
            multiplier_per_mille,
        }
    }

    /// The damage of a landed/killed strike, or a test failure on a miss.
    fn struck_damage(
        attacker: &CombatProfile,
        target: &CombatProfile,
        basis: &StrikeBasis,
        seed: u64,
    ) -> u32 {
        let mut rng = TestRng::new(seed);
        let (_, outcome) = resolve_attack(attacker, target, Pool::full(100_000), basis, &mut rng);
        match outcome {
            AttackOutcome::Landed { hit } | AttackOutcome::Killed { hit } => hit.damage.0,
            AttackOutcome::Missed => panic!("a 100% hit chance never misses"),
        }
    }

    #[test]
    fn a_skill_critical_reads_the_augmented_max_and_multiplies() {
        // Aug max 140, critical forced, DK multiplier 2030: 140 × 2030/1000 = 284.
        let attacker = with_chances(plain(50, 33, 50, 0, 10_000, 0), 0, 100, 0, 0);
        let target = plain(50, 0, 0, 0, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &physical_basis(93, 140, 2030), 6),
            284
        );
    }

    #[test]
    fn a_physical_excellent_multiplies_then_subtracts_defense() {
        // (92 × 6/5) − 30 = 110 − 30 = 80.
        let attacker = with_chances(plain(10, 5, 10, 0, 10_000, 0), 100, 0, 0, 0);
        let target = plain(10, 0, 0, 30, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &physical_basis(56, 92, 1000), 6),
            80
        );
    }

    #[test]
    fn a_wizardry_excellent_subtracts_defense_then_multiplies() {
        // (92 − 30) × 6/5 = 62 × 6/5 = 74 — six below the physical order's 80.
        let attacker = with_chances(plain(10, 5, 10, 0, 10_000, 0), 100, 0, 0, 0);
        let target = plain(10, 0, 0, 30, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &wizardry_basis(56, 92, 1000), 6),
            74
        );
    }

    #[test]
    fn a_collapsed_span_lands_the_floor_times_the_multiplier() {
        // [0,0] span, level 50 → floor 5, × 2030/1000 = 10; doubled → 20.
        let attacker = plain(50, 20, 20, 0, 10_000, 0);
        let target = plain(50, 0, 0, 0, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &wizardry_basis(0, 0, 2030), 3),
            10
        );
        let doubled = with_chances(plain(50, 20, 20, 0, 10_000, 0), 0, 0, 0, 100);
        assert_eq!(
            struck_damage(&doubled, &target, &wizardry_basis(0, 0, 2030), 3),
            20
        );
    }

    #[test]
    fn incoming_reduction_folds_last_after_the_skill_multiplier() {
        // OUR-pin truncation pin: the %-reduction is the LAST step — 21 ×
        // 2030/1000 = 42 (42.63 truncated), × 50/100 = 21; reduction-first
        // would truncate to 10 and land 20.
        let attacker = plain(10, 5, 10, 0, 10_000, 0);
        let target = with_reduction(plain(10, 0, 0, 0, 0, 0), 50);
        assert_eq!(
            struck_damage(&attacker, &target, &physical_basis(21, 21, 2030), 6),
            21
        );
    }

    #[test]
    fn a_thousand_per_mille_multiplier_is_the_identity() {
        // A Skill basis over the attacker's own span at ×1000 folds identically
        // to a PlainSwing under the same seed.
        let attacker = with_chances(plain(30, 10, 40, 3, 10_000, 0), 20, 20, 20, 20);
        let target = plain(30, 0, 0, 10, 0, 0);
        for seed in 0u64..32 {
            let run = |basis: &StrikeBasis| {
                let mut rng = TestRng::new(seed);
                resolve_attack(&attacker, &target, Pool::full(300), basis, &mut rng)
            };
            assert_eq!(
                run(&physical_basis(10, 40, 1000)),
                run(&StrikeBasis::PlainSwing),
                "seed {seed}"
            );
        }
    }

    /// An RNG that counts the words it hands out, for draw-count invariants.
    struct CountingRng {
        inner: TestRng,
        words: u32,
    }

    impl RngCore for CountingRng {
        fn next_u64(&mut self) -> u64 {
            self.words += 1;
            self.inner.next_u64()
        }

        fn next_u32(&mut self) -> u32 {
            self.words += 1;
            let [b0, b1, b2, b3, _, _, _, _] = self.inner.next_u64().to_le_bytes();
            u32::from_le_bytes([b0, b1, b2, b3])
        }

        fn fill_bytes(&mut self, dst: &mut [u8]) {
            self.inner.fill_bytes(dst);
        }
    }

    #[test]
    fn every_basis_draws_the_same_word_count_on_a_landed_strike() {
        // A landed strike is hit + span + four special rolls on every basis —
        // the span is drawn even on a collapsed [0,0].
        let attacker = plain(50, 33, 50, 0, 10_000, 0);
        let target = plain(50, 0, 0, 0, 0, 0);
        let words = |basis: &StrikeBasis| {
            let mut rng = CountingRng {
                inner: TestRng::new(5),
                words: 0,
            };
            let (_, outcome) =
                resolve_attack(&attacker, &target, Pool::full(1000), basis, &mut rng);
            assert!(!matches!(outcome, AttackOutcome::Missed));
            rng.words
        };
        let plain_words = words(&StrikeBasis::PlainSwing);
        assert_eq!(plain_words, words(&physical_basis(93, 140, 2030)));
        assert_eq!(plain_words, words(&wizardry_basis(56, 92, 1000)));
        assert_eq!(plain_words, words(&wizardry_basis(0, 0, 2030)));
        assert_eq!(plain_words, words(&physical_basis(0, 0, 2030)));
    }

    #[test]
    fn a_normal_skill_hit_reaches_the_inclusive_augmented_max() {
        // OUR-pin: the roll is max-INCLUSIVE — 140 − 15 = 125 is reachable,
        // and nothing beyond it ever lands.
        let attacker = plain(10, 33, 50, 0, 10_000, 0);
        let target = plain(10, 0, 0, 15, 0, 0);
        let mut saw_max = false;
        for seed in 0u64..4000 {
            let damage = struck_damage(&attacker, &target, &physical_basis(93, 140, 1000), seed);
            assert!(damage <= 125, "seed {seed}: {damage} beyond the max");
            saw_max |= damage == 125;
        }
        assert!(saw_max, "the inclusive max must be reachable");
    }
}
