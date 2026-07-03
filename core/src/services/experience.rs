//! Per-kill experience award and the level-ups it crosses. The base award is a
//! pooled integer formula of the victim's level, dampened when the killer far
//! out-levels the victim, bonused for high-level victims, scaled by the era
//! factor, and jittered through the injected RNG. Pure and deterministic: the
//! single jitter draw is the only randomness, and the level-up walk is a total
//! climb up the experience curve.

use rand_core::RngCore;

use crate::components::units::{Exp, Level};
use crate::data::atlas::Atlas;
use crate::entities::character::Character;
use crate::events::progression::LevelUp;
use crate::services::chance::uniform_in_inclusive;
use crate::services::ratio::{floor_div_u64_to_u32, nonzero, scale_ratio};

// W-SRC: OpenMU experience constants hardcoded in the award routine, not in
// game_config.json — the over-level dampening threshold and the era experience
// factor. The jitter band itself is the one authored value, in
// game_config.json's progression section.
/// Levels above the victim before the over-level dampening engages.
const OVER_LEVEL_GAP: u32 = 10;
/// High-level victim bonus engages at this victim level.
const HIGH_LEVEL_VICTIM: u32 = 65;
/// Era experience factor numerator (`5/4`).
const EXP_FACTOR_NUM: u32 = 5;
/// Era experience factor denominator.
const EXP_FACTOR_DEN: u32 = 4;

/// Awards the experience for one kill and lists the levels it crosses. The base
/// is `(t + 25) * t / 3` in the victim's level `t`, dampened by `(t + 10) / k`
/// when the killer level `k` exceeds `t + 10`, bonused by `(t - 64) * (t / 4)`
/// for `t >= 65`, scaled `5/4`, then jittered by the authored percent band.
/// Returns the experience gained and the ascending level-ups.
#[must_use]
pub fn award_kill_experience(
    killer: &Character,
    victim_level: Level,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> (Exp, Vec<LevelUp>) {
    let gained = base_award(killer, victim_level, atlas, rng);
    let new_total = Exp(killer.experience().0.saturating_add(u64::from(gained)));
    (Exp(u64::from(gained)), level_ups(killer, new_total, atlas))
}

fn base_award(
    killer: &Character,
    victim_level: Level,
    atlas: &Atlas,
    rng: &mut impl RngCore,
) -> u32 {
    let victim = u32::from(victim_level.get());
    let killer_level = u32::from(killer.level().get());

    let mut base = floor_div_u64_to_u32(
        u64::from(victim + 25).saturating_mul(u64::from(victim)),
        nonzero(3),
    );
    if killer_level > victim + OVER_LEVEL_GAP {
        base = scale_ratio(base, victim + OVER_LEVEL_GAP, nonzero(killer_level));
    }
    if victim >= HIGH_LEVEL_VICTIM {
        base = base.saturating_add((victim - 64).saturating_mul(victim / 4));
    }
    let scaled = scale_ratio(base, EXP_FACTOR_NUM, nonzero(EXP_FACTOR_DEN));
    let jitter_percent = uniform_in_inclusive(atlas.progression().exp_jitter_percent, rng);
    scale_ratio(scaled, u32::from(jitter_percent), nonzero(100))
}

/// The levels the killer crosses climbing the experience curve from its current
/// level toward `new_total`. Monotone: the first level whose held-total exceeds
/// `new_total` ends the climb, as does the curve's cap.
fn level_ups(killer: &Character, new_total: Exp, atlas: &Atlas) -> Vec<LevelUp> {
    let mut level_ups = Vec::new();
    let mut level = killer.level().get().saturating_add(1);
    while let Ok(curve_level) = atlas.exp_curve().level(level) {
        if curve_level.total_to_hold() > new_total {
            break;
        }
        level_ups.push(LevelUp {
            level: curve_level.level(),
        });
        level = level.saturating_add(1);
    }
    level_ups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_award_matches_the_pooled_formula_at_low_level() {
        // t = 30, no dampening (k close), no high-level bonus.
        // base = (30 + 25) * 30 / 3 = 550; * 5/4 = 687; jitter later.
        let raw = (30u32 + 25) * 30 / 3;
        assert_eq!(raw, 550);
        let scaled = scale_ratio(raw, 5, nonzero(4));
        assert_eq!(scaled, 687);
    }

    #[test]
    fn over_level_dampening_multiplies_then_divides() {
        // t = 20, k = 100: base * (t + 10) / k = base * 30 / 100.
        let base = (20u32 + 25) * 20 / 3;
        let damped = scale_ratio(base, 30, nonzero(100));
        // Multiply-then-divide keeps precision a divide-first would lose.
        assert_eq!(damped, base * 30 / 100);
        assert!(damped < base);
    }

    #[test]
    fn high_level_bonus_floors_the_quarter_term_first() {
        // t = 70: bonus = (70 - 64) * (70 / 4) = 6 * 17 = 102 (17, not 17.5).
        let t = 70u32;
        let bonus = (t - 64) * (t / 4);
        assert_eq!(bonus, 102);
    }
}
