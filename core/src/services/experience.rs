//! Per-kill experience award and the level-ups it crosses. The base award is a
//! pooled integer formula of the victim's level, dampened when the reference
//! level far out-levels the victim, bonused for high-level victims, and scaled
//! by the era factor; the injected RNG jitters it through a single draw. Pure
//! and deterministic: the single jitter draw is the only randomness, and the
//! level-up walk is a total climb up the experience curve.
//!
//! The base formula, the jitter draw, and the level-up walk are exposed as
//! shared seams ([`unjittered_base`], [`draw_jitter_percent`],
//! [`level_ups_from`]) so the party pool ([`crate::services::party`]) reuses the
//! very same math — the solo path is the `|Q| = 1` degenerate case, byte-identical
//! by construction rather than by a copied formula.

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
    let base = unjittered_base(killer.level(), victim_level);
    let percent = draw_jitter_percent(atlas, rng);
    let gained = scale_ratio(base, u32::from(percent), nonzero(100));
    let new_total = Exp(killer.experience().0.saturating_add(u64::from(gained)));
    (
        Exp(u64::from(gained)),
        level_ups_from(killer.level(), new_total, atlas),
    )
}

/// The pre-jitter base award in the victim's level, dampened by the `reference`
/// level (the killer's level for a solo award; the qualifying set's average
/// level for a party pool), high-level-victim bonused, and era-scaled `5/4`.
/// No RNG — the jitter is applied separately so both paths share one draw.
#[must_use]
pub(crate) fn unjittered_base(reference_level: Level, victim_level: Level) -> u32 {
    let victim = u32::from(victim_level.get());
    let reference = u32::from(reference_level.get());

    let mut base = floor_div_u64_to_u32(
        u64::from(victim + 25).saturating_mul(u64::from(victim)),
        nonzero(3),
    );
    if reference > victim + OVER_LEVEL_GAP {
        base = scale_ratio(base, victim + OVER_LEVEL_GAP, nonzero(reference));
    }
    if victim >= HIGH_LEVEL_VICTIM {
        base = base.saturating_add((victim - 64).saturating_mul(victim / 4));
    }
    scale_ratio(base, EXP_FACTOR_NUM, nonzero(EXP_FACTOR_DEN))
}

/// The single per-kill jitter draw: one `uniform_in_inclusive` word over the
/// authored percent band. Shared by the solo award and the party pool so a kill
/// consumes exactly one random word regardless of party size.
#[must_use]
pub(crate) fn draw_jitter_percent(atlas: &Atlas, rng: &mut impl RngCore) -> u16 {
    uniform_in_inclusive(atlas.progression().exp_jitter_percent, rng)
}

/// The levels crossed climbing the experience curve from `start_level` toward
/// `new_total`. Monotone: the first level whose held-total exceeds `new_total`
/// ends the climb, as does the curve's cap. Parameterized by a [`Level`] so both
/// the solo killer and each party member reuse it.
#[must_use]
pub(crate) fn level_ups_from(start_level: Level, new_total: Exp, atlas: &Atlas) -> Vec<LevelUp> {
    let mut level_ups = Vec::new();
    let mut level = start_level.get().saturating_add(1);
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
