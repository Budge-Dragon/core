//! The single strike resolver: attacker profile against defender profile and
//! health, struck on a [`StrikeBasis`] (a plain weapon swing or a skill's
//! pre-selected span), one authoritative outcome. Pure and deterministic — the
//! RNG is drawn in a fixed order (hit, damage span, then the four special-hit
//! rolls) so the same seed replays bit-for-bit, and the damage span is *always*
//! rolled even when a critical or excellent overrides it or the span is a
//! collapsed `[0, 0]`, keeping the RNG consumption constant across every branch
//! and every basis.

use core::num::NonZeroU32;

use rand_core::RngCore;

use crate::components::combat_profile::{CombatProfile, TargetKind, WeaponMode};
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
/// Player-versus-player overrate numerator: the crush is suppressed (authentic
/// OpenMU `isPvp` gate keeps full damage), so the ratio is the identity 1/1.
/// CMB-CONST: flip for the server-configurability wave (e.g. 5/10) without
/// touching logic.
const OVERRATE_NUM_PVP: u32 = 1;
/// Player-versus-player overrate denominator (identity 1/1 — see
/// [`OVERRATE_NUM_PVP`]).
const OVERRATE_DEN_PVP: u32 = 1;
/// Divisor of the level-scaled minimum-damage floor (`max(1, level / 10)`).
const MIN_DAMAGE_FLOOR_DIVISOR: u32 = 10;
/// Per-mille divisor of the class skill multiplier (`× multiplier / 1000`).
const SKILL_MULTIPLIER_DENOMINATOR: u32 = 1000;

/// Which overrate ratio a strike uses, derived from the two combatants' kinds:
/// a player-versus-player strike suppresses the overrate crush (the authentic
/// `isPvp` gate), every other pairing keeps the regular crush. Derived in core
/// from the core-stamped kinds, never a claimed matchup.
enum Matchup {
    /// Both combatants are players — the overrate crush is suppressed.
    PlayerVersusPlayer,
    /// Any pairing involving a non-player — the regular overrate crush applies.
    Regular,
}

impl Matchup {
    /// Derives the matchup from the attacker's and defender's kinds, exhaustive
    /// over the (kind, kind) product — a new [`TargetKind`] variant breaks the
    /// build here.
    fn of(attacker: TargetKind, defender: TargetKind) -> Self {
        match (attacker, defender) {
            (TargetKind::Player, TargetKind::Player) => Matchup::PlayerVersusPlayer,
            (TargetKind::Player | TargetKind::Npc, TargetKind::Npc)
            | (TargetKind::Npc, TargetKind::Player) => Matchup::Regular,
        }
    }

    /// The (numerator, denominator) overrate ratio for this matchup: the regular
    /// crush for a non-player pairing, the suppressed identity for a
    /// player-versus-player strike.
    fn overrate_ratio(self) -> (u32, u32) {
        match self {
            Matchup::Regular => (OVERRATE_NUM, OVERRATE_DEN),
            Matchup::PlayerVersusPlayer => (OVERRATE_NUM_PVP, OVERRATE_DEN_PVP),
        }
    }
}

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
        attacker.weapon_mode(),
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

/// The quality-selected base minus defense, in the basis's order — the
/// per-type head, organized order-first so the physical branch can apply the
/// double-wield ×2 before subtracting defense. Normal reads the rolled span;
/// critical the augmented max; excellent applies the 6/5 either side of the
/// defense subtraction per `order`. Exhaustive over [`ExcellentOrder`] ×
/// [`HitQuality`] × [`WeaponMode`] — no wildcard.
// W-SRC: excellent 1.2× max — CONFIRMED on-era against MuEmu 0.97k C++ source
// (independent of OpenMU): `damage = (DamageMax * 120) / 100`, and OpenMU S6;
// every era 0.75→S6 uses 1.2×, and it holds under the skill-augmented base.
// The 1.1× in some fan guides is a rate-vs-magnitude confusion (the +10%
// Excellent Damage RATE option is a proc chance, not the hit magnitude).
// Physical multiplies then subtracts; wizardry subtracts then multiplies
// (AttackableExtensions.cs:106/136 vs :159-160). The double-wield ×2 is
// physical-only and PRE-defense — applied to the quality base after the
// excellent 6/5, before the subtraction (AttackableExtensions.cs:130-134);
// the span already carries the fold's ×55/100, so the net is 110%.
fn quality_after_defense(
    augmented_max: u16,
    rolled_span: u16,
    quality: HitQuality,
    order: ExcellentOrder,
    // Already 0 when the strike ignored defense.
    defense: u16,
    weapon_mode: WeaponMode,
) -> u32 {
    let max = u32::from(augmented_max);
    let def = u32::from(defense);
    match order {
        // Physical: select/multiply the quality base, double a double-wield,
        // then subtract defense.
        ExcellentOrder::MultiplyThenDefense => {
            let base = match quality {
                HitQuality::Normal => u32::from(rolled_span),
                HitQuality::Critical => max,
                HitQuality::Excellent => scale_ratio(max, 6, nonzero(5)),
            };
            let doubled = match weapon_mode {
                WeaponMode::Single => base,
                WeaponMode::DoubleWield => base.saturating_mul(2),
            };
            doubled.saturating_sub(def)
        }
        // Wizardry: subtract defense, then the excellent 6/5. Never doubled —
        // the weapon-independent span carries no double-wield ×55/100, so no
        // ×2 exists to balance it.
        ExcellentOrder::DefenseThenMultiply => match quality {
            HitQuality::Normal => u32::from(rolled_span).saturating_sub(def),
            HitQuality::Critical => max.saturating_sub(def),
            HitQuality::Excellent => scale_ratio(max.saturating_sub(def), 6, nonzero(5)),
        },
    }
}

/// The shared post-head fold: + flat Greater-Damage add → overrate ×3/10 →
/// defender excellent `DamageDecrease` (PRE-floor) → level floor → attacker wing
/// increase → defender wing absorb (both POST-floor; absorb skipped when the
/// damage is 1 or less) → × skill multiplier → ×2 double → defender
/// %-reduction. Defense is already out (the head owns it); this never
/// subtracts it again. The multiplier step matches the basis, so a plain
/// swing skips it structurally (no field to read). Every gear step is the
/// exact identity at its gearless zero percent.
fn strike_tail(
    after_defense: u32,
    attacker: &CombatProfile,
    target: &CombatProfile,
    basis: &StrikeBasis,
    doubled: bool,
) -> u32 {
    let pct_den = u32::from(Percent::DENOMINATOR);
    // W-SRC: the attacker's Greater-Damage add is a FLAT add applied here, after
    // defense and before the floor — never a raise of the strike span, since the
    // quality base is already fixed, so crit/excellent cannot amplify it.
    let after_greater = after_defense.saturating_add(attacker.flat_damage_add());
    // W-SRC: the overrate penalty applies BEFORE the level floor
    // (AttackableExtensions.cs:204-217), so an overrated hit still lands at
    // least max(1, level/10). The ratio is matchup-derived: a
    // player-versus-player strike suppresses the crush (1/1 identity), so this
    // is post-draw integer math only — no RNG draw is added or reordered.
    let (overrate_num, overrate_den) = Matchup::of(attacker.kind(), target.kind()).overrate_ratio();
    let after_overrate = if target.defense_rate() > attacker.attack_rate() {
        scale_ratio(after_greater, overrate_num, nonzero(overrate_den))
    } else {
        after_greater
    };
    // W-SRC: the defender's excellent DamageDecrease applies BEFORE the level
    // floor (AttackableExtensions.cs:209) — distinct from the POST-floor wing
    // absorb; the two must never merge onto one reduction field.
    let dd = u32::from(target.incoming_dd_pct().points());
    let after_dd = scale_ratio(after_overrate, pct_den.saturating_sub(dd), nonzero(pct_den));
    let floor = (u32::from(attacker.level().get()) / MIN_DAMAGE_FLOOR_DIVISOR).max(1);
    let floored = after_dd.max(floor);
    // W-SRC: wing multipliers sit AFTER the floor and BEFORE the skill
    // multiplier — the attacker's increase first, the defender's absorb second
    // (AttackableExtensions.cs:211-223,246).
    let wing_damage = u32::from(attacker.wing_damage_pct().points());
    let after_wing_damage = scale_ratio(
        floored,
        pct_den.saturating_add(wing_damage),
        nonzero(pct_den),
    );
    // W-SRC: the absorb is skipped when the damage is 1 or less
    // (AttackableExtensions.cs:218-222) — folded on the surplus above 1, not
    // guarded.
    let wing_absorb = u32::from(target.wing_absorb_pct().points());
    let after_absorb = match NonZeroU32::new(after_wing_damage.saturating_sub(1)) {
        None => after_wing_damage,
        Some(_) => scale_ratio(
            after_wing_damage,
            pct_den.saturating_sub(wing_absorb),
            nonzero(pct_den),
        ),
    };
    // Skill strikes only: a plain swing carries no multiplier to apply.
    let after_multiplier = match basis {
        StrikeBasis::PlainSwing => after_absorb,
        StrikeBasis::Skill {
            multiplier_per_mille,
            ..
        } => scale_ratio(
            after_absorb,
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
    use crate::components::combat_profile::TargetKind;
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
            kind: TargetKind::Npc,
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
            wizardry_rise_x2: 0,
            incoming_dd_pct: Percent::ZERO,
            wing_damage_pct: Percent::ZERO,
            wing_absorb_pct: Percent::ZERO,
            weapon_mode: WeaponMode::Single,
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
    fn overrate_crush_applies_pvm_but_is_suppressed_in_pvp_at_identical_stats() {
        // Defender out-rates the attacker, so the overrate crush is in play. The
        // player-versus-monster (Regular) strike is crushed to 3/10; the
        // player-versus-player strike keeps full damage (1/1 identity). Identical
        // stats + identical seed => an identical RNG draw sequence for both, so a
        // seed lands or misses alike and only the final overrate factor differs.
        // A landed hit is sampled across a seed sweep (the out-rated hit chance
        // floors at 3%).
        let mut attacker = plain(10, 100, 100, 0, 100, 0);
        attacker.kind = TargetKind::Player;
        let mut pvm_target = plain(10, 0, 0, 0, 0, 200);
        pvm_target.kind = TargetKind::Npc; // player vs monster => Regular
        let mut pvp_target = plain(10, 0, 0, 0, 0, 200);
        pvp_target.kind = TargetKind::Player; // player vs player => suppressed

        let mut landed = 0;
        for seed in 0u64..2000 {
            let (_, pvm) = resolve_attack(
                &attacker,
                &pvm_target,
                Pool::full(500),
                &StrikeBasis::PlainSwing,
                &mut TestRng::new(seed),
            );
            let (_, pvp) = resolve_attack(
                &attacker,
                &pvp_target,
                Pool::full(500),
                &StrikeBasis::PlainSwing,
                &mut TestRng::new(seed),
            );
            if let (
                AttackOutcome::Landed { hit: pvm_hit },
                AttackOutcome::Landed { hit: pvp_hit },
            ) = (pvm, pvp)
            {
                let (dm, dp) = (pvm_hit.damage.0, pvp_hit.damage.0);
                assert!(
                    dp > dm,
                    "seed {seed}: pvp {dp} must exceed crushed pvm {dm}"
                );
                assert_eq!(dm, scale_ratio(dp, OVERRATE_NUM, nonzero(OVERRATE_DEN)));
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

    // ── Gear positions: pre-floor DD, post-floor wings, double-wield ──

    /// The base profile with its gear strike magnitudes set (percent points).
    fn with_gear(base: CombatProfile, dd: u8, wing_damage: u8, wing_absorb: u8) -> CombatProfile {
        CombatProfile {
            incoming_dd_pct: Percent::new(dd).unwrap(),
            wing_damage_pct: Percent::new(wing_damage).unwrap(),
            wing_absorb_pct: Percent::new(wing_absorb).unwrap(),
            ..base
        }
    }

    /// The base profile with its weapon mode set.
    fn with_mode(base: CombatProfile, weapon_mode: WeaponMode) -> CombatProfile {
        CombatProfile {
            weapon_mode,
            ..base
        }
    }

    #[test]
    fn wing_damage_and_absorb_apply_after_the_floor_and_dd_before_it() {
        // EQ-WING-1: after-defense 100 → DD 96 (pre-floor) → floor max(5,96)
        // → attacker wing ×116/100 = 111 → defender absorb ×84/100 = 93.
        let attacker = with_gear(plain(50, 100, 100, 0, 10_000, 0), 0, 16, 0);
        let target = with_gear(plain(50, 0, 0, 0, 0, 0), 4, 0, 16);
        assert_eq!(
            struck_damage(&attacker, &target, &StrikeBasis::PlainSwing, 3),
            93
        );
    }

    #[test]
    fn dd_is_pre_floor_while_wing_absorb_is_post_floor() {
        // EQ-WING-2: after-defense 3 → DD floor(3·96/100) = 2 → floor
        // max(5, 2) = 5 → wing floor(5·116/100) = 5 → absorb floor(5·84/100)
        // = 4. A post-floor DD (the merged-field bug) would land 3 — the
        // 4-vs-3 gap proves the two positions are distinct.
        let attacker = with_gear(plain(50, 3, 3, 0, 10_000, 0), 0, 16, 0);
        let target = with_gear(plain(50, 0, 0, 0, 0, 0), 4, 0, 16);
        assert_eq!(
            struck_damage(&attacker, &target, &StrikeBasis::PlainSwing, 3),
            4
        );
    }

    #[test]
    fn wing_absorb_is_skipped_when_the_damage_is_one() {
        // EQ-WING-3: a floor-value 1 hit keeps its 1 — the absorb never
        // applies at damage ≤ 1.
        let attacker = plain(10, 1, 1, 0, 10_000, 0);
        let target = with_gear(plain(10, 0, 0, 0, 0, 0), 0, 0, 16);
        assert_eq!(
            struck_damage(&attacker, &target, &StrikeBasis::PlainSwing, 3),
            1
        );
    }

    #[test]
    fn the_gear_positions_are_the_identity_at_their_gearless_zero() {
        // Zero DD / wing percents and Single mode reproduce the bare strike
        // byte-for-byte across a seed sweep (E4).
        let bare_attacker = plain(50, 20, 40, 0, 10_000, 0);
        let bare_target = plain(50, 0, 0, 5, 0, 0);
        let geared_attacker = with_mode(with_gear(bare_attacker, 0, 0, 0), WeaponMode::Single);
        let geared_target = with_gear(bare_target, 0, 0, 0);
        for seed in 0u64..32 {
            assert_eq!(
                struck_damage(&bare_attacker, &bare_target, &StrikeBasis::PlainSwing, seed),
                struck_damage(
                    &geared_attacker,
                    &geared_target,
                    &StrikeBasis::PlainSwing,
                    seed
                ),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn double_wield_doubles_before_defense_and_is_distinct_from_the_random_double() {
        // EQ-DW-2: the ×0.55-folded span max 110 doubles to 220 BEFORE the 20
        // defense — (110×2) − 20 = 200, never (110−20)×2 = 180.
        let attacker = with_mode(plain(10, 110, 110, 0, 10_000, 0), WeaponMode::DoubleWield);
        let target = plain(10, 0, 0, 20, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &StrikeBasis::PlainSwing, 3),
            200
        );
        // Against zero defense the net is 110% of the summed max 200 (EQ-DW-1).
        let bare = plain(10, 0, 0, 0, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &bare, &StrikeBasis::PlainSwing, 3),
            220
        );
        // A strike that ALSO rolls the random double doubles again at the
        // post-multiplier position: ((110×2) − 20) × 2 = 400.
        let doubled = with_chances(
            with_mode(plain(10, 110, 110, 0, 10_000, 0), WeaponMode::DoubleWield),
            0,
            0,
            0,
            100,
        );
        assert_eq!(
            struck_damage(&doubled, &target, &StrikeBasis::PlainSwing, 3),
            400
        );
    }

    #[test]
    fn the_excellent_multiplier_precedes_the_double_wield_doubling() {
        // Order pin: ×6/5 THEN ×2 in the physical head —
        // floor(110·6/5) = 132 → ×2 = 264 → −20 = 244.
        let attacker = with_chances(
            with_mode(plain(10, 110, 110, 0, 10_000, 0), WeaponMode::DoubleWield),
            100,
            0,
            0,
            0,
        );
        let target = plain(10, 0, 0, 20, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &StrikeBasis::PlainSwing, 3),
            244
        );
    }

    #[test]
    fn a_double_wielded_wizardry_strike_never_doubles() {
        // The wizardry order has no ×2 — a double-wielding caster's spell span
        // stands as selected: 100 − 0 = 100, never 200.
        let attacker = with_mode(plain(10, 5, 10, 0, 10_000, 0), WeaponMode::DoubleWield);
        let target = plain(10, 0, 0, 0, 0, 0);
        assert_eq!(
            struck_damage(&attacker, &target, &wizardry_basis(100, 100, 1000), 3),
            100
        );
    }

    #[test]
    fn the_gear_steps_draw_no_extra_rng_words() {
        // The DD/wing steps are pure arithmetic: a fully geared strike draws
        // exactly the bare strike's word count.
        let bare_attacker = plain(50, 33, 50, 0, 10_000, 0);
        let geared_attacker = with_mode(
            with_gear(plain(50, 33, 50, 0, 10_000, 0), 0, 16, 0),
            WeaponMode::DoubleWield,
        );
        let bare_target = plain(50, 0, 0, 0, 0, 0);
        let geared_target = with_gear(plain(50, 0, 0, 0, 0, 0), 4, 0, 16);
        let words = |attacker: &CombatProfile, target: &CombatProfile| {
            let mut rng = CountingRng {
                inner: TestRng::new(5),
                words: 0,
            };
            let (_, outcome) = resolve_attack(
                attacker,
                target,
                Pool::full(10_000),
                &StrikeBasis::PlainSwing,
                &mut rng,
            );
            assert!(!matches!(outcome, AttackOutcome::Missed));
            rng.words
        };
        assert_eq!(
            words(&bare_attacker, &bare_target),
            words(&geared_attacker, &geared_target)
        );
    }
}
